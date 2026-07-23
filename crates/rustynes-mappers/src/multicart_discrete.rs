//! Discrete-logic multicart boards addressed by their iNES mapper number:
//! K-1029 / Contra Function 16 (mapper 15), and the 20-in-1 / Super 700-in-1
//! style boards on mappers 61 and 62.
//!
//! What these share is that the *address written to* carries as much
//! information as the byte written. Mapper 15 selects among four PRG banking
//! modes from the low two address bits; mapper 61 picks 16 KiB-vs-32 KiB mode
//! the same way; mapper 62 splits a CHR bank field across the address and the
//! data. That is cheaper in discrete logic than decoding a wide register, and
//! it is why these decode paths look address-driven rather than value-driven.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, with no commercial-oracle ROM in the tree. Banking math
//! is direct slice indexing and every bank select wraps with `% count`, so a
//! register write can never index out of bounds -- required for the `#![no_std]`
//! chip stack, which cannot afford a panic on a register access.
//!
//! See `tier.rs` (`MapperTier::BestEffort`), `docs/adr/0011-mapper-tiering.md`,
//! and `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::bool_to_int_with_if,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::unreadable_literal
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable helper (mirrors the one in the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

// ===========================================================================
// Mapper 15 — K-1029 / 100-in-1 Contra Function 16.
//
// Single register decoded across $8000-$FFFF (data + low two address bits):
//   addr bits 0-1 select the banking MODE; data holds the PRG bank, a CHR-RAM
//   mirroring bit (bit 6) and a "half-bank" bit (bit 7).
//     mode 0: 32 KiB at the 16 KiB granularity, second half = bank|1
//     mode 1: 128 KiB? upper half forced to bank|7 (UNROM-like fixed top)
//     mode 2: 8 KiB-granular ((bank<<1)|b) mirrored across the whole window
//     mode 3: single 16 KiB bank mirrored across the whole window
//   CHR is always 8 KiB RAM; CHR writes are protected in modes 0 and 3.
//   mirroring: data bit 6 (1 = horizontal, 0 = vertical). No IRQ.
// ===========================================================================

const fn byte_to_mirroring(b: u8, fallback: Mirroring) -> Mirroring {
    match b {
        0 => Mirroring::Horizontal,
        1 => Mirroring::Vertical,
        2 => Mirroring::SingleScreenA,
        3 => Mirroring::SingleScreenB,
        4 => Mirroring::FourScreen,
        5 => Mirroring::MapperControlled,
        _ => fallback,
    }
}

/// Validate a PRG-ROM image is a non-zero multiple of 8 KiB.
fn check_prg(prg: &[u8], id: u16) -> Result<(), MapperError> {
    if prg.is_empty() || !prg.len().is_multiple_of(PRG_BANK_8K) {
        return Err(MapperError::Invalid(format!(
            "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
            prg.len()
        )));
    }
    Ok(())
}

/// Allocate the CHR slice, falling back to an 8 KiB CHR-RAM bank when the ROM
/// ships no CHR-ROM. Returns `(chr, is_ram)`.
fn chr_or_ram(chr_rom: Box<[u8]>) -> (Box<[u8]>, bool) {
    if chr_rom.is_empty() {
        (vec![0u8; CHR_BANK_8K].into_boxed_slice(), true)
    } else {
        (chr_rom, false)
    }
}

const fn mirroring_to_byte(m: Mirroring) -> u8 {
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenA => 2,
        Mirroring::SingleScreenB => 3,
        Mirroring::FourScreen => 4,
        Mirroring::MapperControlled => 5,
    }
}

/// Mapper 15 (K-1029 / 100-in-1 Contra Function 16 multicart).
pub struct Multicart15 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    mode: u8,
    prg_bank: u8,
    half: u8,
    horizontal_mirroring: bool,
}

impl Multicart15 {
    /// Construct a new mapper 15 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. CHR is always 8 KiB RAM (any supplied CHR-ROM is rejected).
    pub fn new(prg_rom: Box<[u8]>, chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 15 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(format!(
                "mapper 15 uses 8 KiB CHR-RAM; got {} bytes of CHR-ROM",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mode: 0,
            prg_bank: 0,
            half: 0,
            horizontal_mirroring: false,
        })
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn read_16k(&self, bank: usize, off: usize) -> u8 {
        let bank = bank % self.prg_count_16k();
        self.prg_rom[bank * PRG_BANK_16K + off]
    }

    fn read_8k(&self, bank: usize, off: usize) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + off]
    }
}

impl Mapper for Multicart15 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        let win = (addr as usize) - 0x8000; // 0..0x8000 (the visible 32 KiB)
        let bank = self.prg_bank as usize;
        match self.mode {
            0 => {
                if win < PRG_BANK_16K {
                    self.read_16k(bank, win)
                } else {
                    self.read_16k(bank | 1, win - PRG_BANK_16K)
                }
            }
            1 => {
                if win < PRG_BANK_16K {
                    self.read_16k(bank, win)
                } else {
                    self.read_16k(bank | 7, win - PRG_BANK_16K)
                }
            }
            2 => {
                // 8 KiB-granular, mirrored across the whole window.
                let off = win & (PRG_BANK_8K - 1);
                self.read_8k((bank << 1) | (self.half as usize), off)
            }
            // mode 3: a single 16 KiB bank mirrored across the window.
            _ => {
                let off = win & (PRG_BANK_16K - 1);
                self.read_16k(bank, off)
            }
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.mode = (addr & 0x03) as u8;
            self.horizontal_mirroring = (value & 0x40) != 0;
            self.prg_bank = value & 0x3F;
            self.half = u8::from((value & 0x80) != 0);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                // CHR-RAM write-protected in modes 0 and 3.
                if self.mode != 0 && self.mode != 3 {
                    self.chr_ram[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.mode);
        out.push(self.prg_bank);
        out.push(self.half);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.mode = data[1];
        self.prg_bank = data[2];
        self.half = data[3];
        self.horizontal_mirroring = data[4] != 0;
        let mut cursor = 5;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

/// Mapper 61 (0x80-style multicart).
pub struct Multicart61 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_page: u8,
    prg_16k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart61 {
    /// Construct a new mapper 61 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. CHR is always 8 KiB RAM (any supplied CHR-ROM is rejected).
    pub fn new(prg_rom: Box<[u8]>, chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 61 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(format!(
                "mapper 61 uses 8 KiB CHR-RAM; got {} bytes of CHR-ROM",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_page: 0,
            prg_16k_mode: false,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Multicart61 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        let win = (addr as usize) - 0x8000;
        if self.prg_16k_mode {
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_page as usize) % count;
            let off = win & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = ((self.prg_page >> 1) as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + win]
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let lo = (addr & 0x0F) as u8;
            let hi = ((addr >> 5) & 0x01) as u8;
            self.prg_page = (lo << 1) | hi;
            self.prg_16k_mode = (addr & 0x10) != 0;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_page);
        out.push(u8::from(self.prg_16k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_page = data[1];
        self.prg_16k_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 62 — multicart.
//
// Both the CPU address and the written byte feed the register ($8000-$FFFF).
// With `A = addr` and `D = data`:
//   prg_page          = ((A & 0x3F00) >> 8) | (A & 0x40)
//   chr_bank (4-bit?) = ((A & 0x1F) << 2) | (D & 0x03)
//   prg_16k_mode      =  (A & 0x20) != 0
//   horizontal_mirror =  (A & 0x80) != 0
// In 16 KiB mode the 16 KiB bank `prg_page` is mirrored across the window; in
// 32 KiB mode bank `prg_page >> 1` is used. CHR is 8 KiB ROM banked. No IRQ.
// ===========================================================================

/// Mapper 62 (multicart).
pub struct Multicart62 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_page: u8,
    chr_bank: u8,
    prg_16k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart62 {
    /// Construct a new mapper 62 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 62 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 62 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_page: 0,
            chr_bank: 0,
            // Seed from the header arrangement so the power-on default is sane.
            prg_16k_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }
}

impl Mapper for Multicart62 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        let win = (addr as usize) - 0x8000;
        if self.prg_16k_mode {
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_page as usize) % count;
            let off = win & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = ((self.prg_page >> 1) as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + win]
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let page_lo = ((addr & 0x3F00) >> 8) as u8;
            let page_hi = (addr & 0x40) as u8;
            self.prg_page = page_lo | page_hi;
            let chr_hi = ((addr & 0x1F) as u8) << 2;
            self.chr_bank = chr_hi | (value & 0x03);
            self.prg_16k_mode = (addr & 0x20) != 0;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_page);
        out.push(self.chr_bank);
        out.push(u8::from(self.prg_16k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_page = data[1];
        self.chr_bank = data[2];
        self.prg_16k_mode = data[3] != 0;
        self.horizontal_mirroring = data[4] != 0;
        self.vram.copy_from_slice(&data[5..5 + self.vram.len()]);
        Ok(())
    }
}

/// Mapper 200 (`MG109` NROM-128 multicart).
pub struct Multicart200 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart200 {
    /// Construct a new mapper 200 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 200 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 200 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            bank: 0,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }
}

impl Mapper for Multicart200 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // NROM-128: $8000-$BFFF mirrors $C000-$FFFF.
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.bank as usize) % count;
            let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.bank = (addr & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x08) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr.len() / CHR_BANK_8K).max(1);
                let bank = (self.bank as usize) % count;
                self.chr[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let count = (self.chr.len() / CHR_BANK_8K).max(1);
                    let bank = (self.bank as usize) % count;
                    self.chr[bank * CHR_BANK_8K + addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.bank = data[1];
        self.horizontal_mirroring = data[2] != 0;
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 201 — NROM-256 multicart (BNROM + CNROM overlaid, address-driven).
//
// Write $8000-$FFFF: the ADDRESS low byte selects one bank that drives both a
// 32 KiB PRG bank and an 8 KiB CHR bank:
//   PRG (32 KiB) = addr & 0x03   (masked to PRG bank count)
//   CHR (8 KiB)  = addr & 0x07   (masked to CHR bank count)
// (All known games use only the low 2 bits.) Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 201 (NROM-256 multicart).
pub struct Multicart201 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart201 {
    /// Construct a new mapper 201 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 201 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 201 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart201 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (addr & 0x03) as u8;
            self.chr_bank = (addr & 0x07) as u8;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 202 — 150-in-1 multicart (address latch).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   A~[.... .... .... O..O]  (PRG mode bits, combine to form a 2-bit value)
//   A~[.... .... .... RRRM]  (R = page register, M = mirroring)
//   prg_mode_is_32k = (((addr >> 1) & 0x01) == 1 && (addr & 0x01) == 1)
//                   i.e. the two "O" bits (addr bit 3 and addr bit 0) == 0b11
//   page = (addr >> 1) & 0x07
//   mirroring = addr & 0x01 (0: vertical, 1: horizontal)
// In 16 KiB mode the page maps both halves; in 32 KiB mode (page>>1) selects a
// 32 KiB bank. CHR (8 KiB) = page. Mirroring runtime; no IRQ.
//
// Per the nesdev wiki the "O" bits are addr bit 3 and addr bit 0; if both set,
// 32 KiB mode. We follow the BizHawk/Disch convention used in the wiki note.
// ===========================================================================

/// Mapper 202 (150-in-1 multicart).
pub struct Multicart202 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    page: u8,
    prg_32k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart202 {
    /// Construct a new mapper 202 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 202 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 202 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            page: 0,
            prg_32k_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.page as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart202 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                if self.prg_32k_mode {
                    // 32 KiB bank (page >> 1) spread across the whole window.
                    let lo16 = ((self.page >> 1) << 1) as usize % count;
                    let off = addr as usize - 0x8000;
                    self.prg_rom[lo16 * PRG_BANK_16K + off]
                } else {
                    // 16 KiB mode: same page mirrored at $8000 and $C000.
                    let bank = (self.page as usize) % count;
                    let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
                    self.prg_rom[bank * PRG_BANK_16K + off]
                }
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // "O" bits = addr bit 3 and addr bit 0; both set => 32 KiB mode.
            self.prg_32k_mode = (addr & 0x09) == 0x09;
            self.page = ((addr >> 1) & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x01) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.page);
        out.push(u8::from(self.prg_32k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 4 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.page = data[1];
        self.prg_32k_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 203 — 35-in-1 multicart (data latch).
//
// Write $8000-$FFFF, DATA-driven:
//   PPPP PPCC
//   PRG (16 KiB, mirrored at $8000 and $C000) = (data >> 2) & 0x3F
//   CHR (8 KiB)                               = data & 0x03
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 203 (35-in-1 multicart).
pub struct Multicart203 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart203 {
    /// Construct a new mapper 203 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 203 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 203 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart203 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // NROM-128: $8000-$BFFF mirrors $C000-$FFFF.
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_bank as usize) % count;
            let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (value >> 2) & 0x3F;
            self.chr_bank = value & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 212 — BMC Super HiK 300-in-1 (address latch).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   A~[1o.. .... .... MBBb]
//   prg_32k_mode = bit 14 of addr ("o")
//   page (3-bit)  = addr & 0x07 (drives 16 KiB PRG, 32 KiB PRG and 8 KiB CHR)
//   mirroring = bit 3 of addr (0: vertical, 1: horizontal)
//   16 KiB mode: page maps both $8000 and $C000 windows
//   32 KiB mode: (page >> 1) selects a 32 KiB bank
//   CHR (8 KiB) = page (regardless of "o")
// Reads at $6000-$7FFF with (addr & 0x10) == 0 return bit 7 set (a protection
// signature). Mirroring runtime; no IRQ.
// ===========================================================================

/// Mapper 212 (`BMC` Super `HiK` 300-in-1 multicart).
pub struct Multicart212 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    page: u8,
    prg_32k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart212 {
    /// Construct a new mapper 212 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 212 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 212 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            page: 0,
            prg_32k_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.page as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart212 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $6000-$7FFF carries the protection signature (mapped). $4020-$5FFF
        // is unmapped open bus, as for stock boards.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // Protection: (addr & 0x10) == 0 reads $80; else open-bus-ish 0.
                if (addr & 0x0010) == 0 { 0x80 } else { 0x00 }
            }
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                if self.prg_32k_mode {
                    let lo16 = ((self.page >> 1) << 1) as usize % count;
                    let off = addr as usize - 0x8000;
                    self.prg_rom[lo16 * PRG_BANK_16K + off]
                } else {
                    let bank = (self.page as usize) % count;
                    let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
                    self.prg_rom[bank * PRG_BANK_16K + off]
                }
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_32k_mode = (addr & 0x4000) != 0;
            self.page = (addr & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x0008) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.page);
        out.push(u8::from(self.prg_32k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 4 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.page = data[1];
        self.prg_32k_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 213 — 9999999-in-1 multicart (address latch; duplicate of 58).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   CHR (8 KiB)  = (addr >> 3) & 0x07
//   PRG (32 KiB) = (addr >> 1) & 0x03
// NROM-256-style mirroring (header-fixed). No IRQ.
// ===========================================================================

/// Mapper 213 (9999999-in-1 multicart).
pub struct Multicart213 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart213 {
    /// Construct a new mapper 213 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 213 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 213 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart213 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.chr_bank = ((addr >> 3) & 0x07) as u8;
            self.prg_bank = ((addr >> 1) & 0x03) as u8;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 214 — Super Gun 20-in-1 multicart (address latch).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   CHR (8 KiB)  = addr & 0x03
//   PRG (16 KiB, mirrored at $8000 and $C000) = (addr >> 2) & 0x03
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 214 (Super Gun 20-in-1 multicart).
pub struct Multicart214 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart214 {
    /// Construct a new mapper 214 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 214 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 214 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart214 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // NROM-128: $8000-$BFFF mirrors $C000-$FFFF.
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_bank as usize) % count;
            let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.chr_bank = (addr & 0x03) as u8;
            self.prg_bank = ((addr >> 2) & 0x03) as u8;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

/// Mapper 58 (multicart).
pub struct Multicart58 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    prg32_mode: bool,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart58 {
    /// Construct a new mapper 58 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 58 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 58 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        let horizontal_mirroring = mirroring == Mirroring::Horizontal;
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            prg32_mode: false,
            chr_bank: 0,
            horizontal_mirroring,
        })
    }
}

impl Mapper for Multicart58 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        if self.prg32_mode {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (((self.prg_bank & 0x06) >> 1) as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (addr & 0x07) as u8;
            self.prg32_mode = (addr & 0x40) == 0;
            self.chr_bank = ((addr >> 3) & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg32_mode));
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.prg32_mode = data[2] != 0;
        self.chr_bank = data[3];
        self.horizontal_mirroring = data[4] != 0;
        self.vram.copy_from_slice(&data[5..5 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 60 — reset-based 4-in-1 multicart.
//
// The active game is chosen by a reset counter (0..=3) that increments on each
// console reset; the selected value drives a 16 KiB PRG bank + 8 KiB CHR bank
// in lockstep. Reset-latch behaviour is host-driven (the bus/frontend would
// have to pulse a reset hook we do not model in the no_std core), so we model
// only the power-on bank (counter = 0). No IRQ.
// ===========================================================================

/// Mapper 60 (reset-based 4-in-1 multicart; power-on bank only).
pub struct Multicart60 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    bank: u8,
    mirroring: Mirroring,
}

impl Multicart60 {
    /// Construct a new mapper 60 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 60 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 60 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            // Power-on selects game 0.
            bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Multicart60 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // The reset counter drives a 16 KiB bank mirrored across the 32 KiB
            // window (NROM-128-per-game).
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 231 — 20-in-1 multicart.
//
// Address-decoded register across $8000-$FFFF (data byte ignored). For the
// absolute address A:
//   prgBank = ((A >> 5) & 0x01) | (A & 0x1E)
//   $8000 16 KiB bank = prgBank & 0x1E
//   $C000 16 KiB bank = prgBank
//   mirroring = bit 7 of A (1 = horizontal, 0 = vertical)
// CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 231 (20-in-1 multicart).
pub struct Multicart231 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank0: u8,
    prg_bank1: u8,
    horizontal_mirroring: bool,
}

impl Multicart231 {
    /// Construct a new mapper 231 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 231 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let horizontal_mirroring = mirroring == Mirroring::Horizontal;
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank0: 0,
            prg_bank1: 0,
            horizontal_mirroring,
        })
    }

    fn read_prg(&self, bank: u8, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = (bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart231 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(self.prg_bank0, addr),
            0xC000..=0xFFFF => self.read_prg(self.prg_bank1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let prg_bank = ((((addr >> 5) & 0x01) | (addr & 0x1E)) & 0xFF) as u8;
            self.prg_bank0 = prg_bank & 0x1E;
            self.prg_bank1 = prg_bank;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank0);
        out.push(self.prg_bank1);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank0 = data[1];
        self.prg_bank1 = data[2];
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

/// Mapper 234 (Maxi 15 / `BNROM`-like multicart).
pub struct Maxi15M234 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg0: u8,
    reg1: u8,
}

impl Maxi15M234 {
    /// Construct a new mapper 234 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 234 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 234 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg0: 0,
            reg1: 0,
        })
    }

    const fn update_state(&mut self) {
        if (self.reg0 & 0x40) != 0 {
            // NINA-03 mode.
            self.reg1 &= 0x71;
        } else {
            // CNROM mode: only the low 2 bits of the reg1 CHR selector matter.
            self.reg1 &= 0x31;
        }
    }

    fn latch_access(&mut self, addr: u16, value: u8) {
        if (0xFF80..=0xFF9F).contains(&addr) {
            // The reg0 latch only fires while its low 6 bits are still zero.
            if self.reg0.trailing_zeros() >= 6 {
                self.reg0 = value;
                self.update_state();
            }
        } else if (0xFFE8..=0xFFF8).contains(&addr) {
            self.reg1 = value & 0x71;
            self.update_state();
        }
    }

    const fn prg_bank(&self) -> u8 {
        if (self.reg0 & 0x40) != 0 {
            (self.reg0 & 0x0E) | (self.reg1 & 0x01)
        } else {
            self.reg0 & 0x0F
        }
    }

    const fn chr_bank(&self) -> u8 {
        if (self.reg0 & 0x40) != 0 {
            ((self.reg0 << 2) & 0x38) | ((self.reg1 >> 4) & 0x07)
        } else {
            ((self.reg0 << 2) & 0x3C) | ((self.reg1 >> 4) & 0x03)
        }
    }
}

impl Mapper for Maxi15M234 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank() as usize) % count;
            let value = self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)];
            // The latch windows live in the readable PRG space; reading them
            // triggers the same latch as a write (with the data the CPU sees).
            self.latch_access(addr, value);
            value
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.latch_access(addr, value);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank() as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if (self.reg0 & 0x80) != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg0);
        out.push(self.reg1);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg0 = data[1];
        self.reg1 = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

/// Mapper 225 (`ColorDreams` `72-in-1`).
pub struct Multicart225 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// $5800-$5FFF 4-nibble scratch RAM (4 bytes, mirrored).
    scratch: [u8; 4],
    prg_bank: u8,
    prg16_mode: bool,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart225 {
    /// Construct a new mapper 225 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 225 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 225 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            scratch: [0; 4],
            prg_bank: 0,
            prg16_mode: false,
            chr_bank: 0,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart225 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The scratch RAM answers reads in $5800-$5FFF (mapped). The rest of
    // $4020-$57FF stays open bus (the trait default); $6000-$FFFF PRG is mapped.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x57FF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x5800..=0x5FFF => self.scratch[(addr & 0x03) as usize] & 0x0F,
            0x8000..=0xBFFF => {
                let base = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    (self.prg_bank as usize) & !1
                };
                self.read_prg(base, addr)
            }
            0xC000..=0xFFFF => {
                let bank = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    ((self.prg_bank as usize) & !1) | 1
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5800..=0x5FFF => self.scratch[(addr & 0x03) as usize] = value & 0x0F,
            0x8000..=0xFFFF => {
                // nesdev iNES 225: the bank/mode are in the ADDRESS bits
                // A~[.HMO PPPP PPCC CCCC]: CHR = A0..A5 (6 bits), PRG = A6..A11
                // (6 bits), O (PRG mode) = A12 (1 = 16 KiB switchable,
                // 0 = 32 KiB), M (mirroring) = A13 (1 = horizontal), H (outer
                // high bit for both PRG and CHR) = A14.
                let high = ((addr >> 14) & 0x01) as u8;
                self.prg16_mode = (addr & 0x1000) != 0;
                self.prg_bank = (high << 6) | (((addr >> 6) & 0x3F) as u8);
                self.chr_bank = (high << 6) | ((addr & 0x3F) as u8);
                self.horizontal_mirroring = (addr & 0x2000) != 0;
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + 4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg16_mode));
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.scratch);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + 4 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.prg16_mode = data[2] != 0;
        self.chr_bank = data[3];
        self.horizontal_mirroring = data[4] != 0;
        self.scratch.copy_from_slice(&data[5..9]);
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 226 — 76-in-1 BMC.
//
// Two latch registers across $8000-$FFFF (the low address bit selects reg0 vs
// reg1; the data byte carries the bank bits):
//   reg0 ($8000, even): bits 0-4 = PRG low, bit 5 = PRG high bit, bit 6 =
//        mirroring (1 = horizontal), bit 7 = 32/16 KiB mode.
//   reg1 ($8001, odd): bit 0 = PRG bit 6 (outer block).
// The 32 KiB PRG bank = (reg1.bit0 << 6) | (reg0.bit5 << 5) | (reg0 & 0x1F).
// In 16 KiB mode both halves use the same bank. CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 226 (`76-in-1` BMC).
pub struct Multicart226 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    reg0: u8,
    reg1: u8,
}

impl Multicart226 {
    /// Construct a new mapper 226 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 226 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg0: 0,
            reg1: 0,
        })
    }

    /// 7-bit 16 KiB PRG bank index: low 6 bits from reg0, high bit from reg1.
    const fn prg_bank(&self) -> usize {
        let low = (self.reg0 & 0x3F) as usize;
        let high = (self.reg1 & 0x01) as usize;
        (high << 6) | low
    }

    /// PRG mode: reg0 bit 6 set = two 16 KiB banks; clear = one 32 KiB bank.
    const fn is_16k(&self) -> bool {
        (self.reg0 & 0x40) != 0
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart226 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let bank = self.prg_bank();
            if self.is_16k() {
                // Both 16 KiB halves map the same selected bank.
                self.read_prg(bank, addr)
            } else {
                // 32 KiB mode: the bank index addresses a 32 KiB page (its low
                // bit is ignored); the high half is +1.
                let base = bank & !1;
                let bank16 = base | usize::from(addr >= 0xC000);
                self.read_prg(bank16, addr)
            }
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0xFFFF if (addr & 0x01) == 0 => self.reg0 = value,
            0x8000..=0xFFFF => self.reg1 = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        // reg0 bit 7: 0 = horizontal, 1 = vertical.
        if (self.reg0 & 0x80) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg0);
        out.push(self.reg1);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg0 = data[1];
        self.reg1 = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 227 — 1200-in-1 BMC.
//
// Address-decoded register across $8000-$FFFF (Mesen2 Mapper227). For the
// absolute write address A:
//   prg_bank = ((A >> 2) & 0x1F) | ((A & 0x100) >> 3)   (6-bit 16 KiB index)
//   s_flag   = (A & 0x01)        (set: restrict / half-select)
//   prg_mode = (A >> 7) & 0x01   (set: NROM modes; clear: UNROM-like)
//   l_flag   = (A >> 9) & 0x01   (set: fix $C000 to bank|0x07; clear: &0x38)
//   mirroring = (A & 0x02) -> 1 = horizontal, 0 = vertical
// The two $8000/$C000 16 KiB windows are then composed per the Mesen2 mode
// table. The old decode read bit 0 as a 32 KiB mode, mis-applied bit 7, and
// IGNORED bit 9, so the fixed $C000 window pointed at the wrong bank and the
// multicart menu never drew. CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 227 (`1200-in-1` BMC).
#[allow(clippy::struct_excessive_bools)] // 4 independent decoded register flags
pub struct Multicart227 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    s_flag: bool,
    l_flag: bool,
    prg_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart227 {
    /// Construct a new mapper 227 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 227 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            s_flag: false,
            l_flag: false,
            prg_mode: false,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }

    /// Compose the ($8000, $C000) 16 KiB bank pair from the decoded flags,
    /// matching Mesen2 `Mapper227::WriteRegister`.
    const fn prg_pages(&self) -> (usize, usize) {
        let b = self.prg_bank as usize;
        if self.prg_mode {
            if self.s_flag {
                (b & 0xFE, (b & 0xFE) | 1) // 32 KiB pair
            } else {
                (b, b) // NROM-128 (16 KiB mirrored)
            }
        } else {
            let lo = if self.s_flag { b & 0x3E } else { b };
            let hi = if self.l_flag { b | 0x07 } else { b & 0x38 };
            (lo, hi)
        }
    }
}

impl Mapper for Multicart227 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let (p0, p1) = self.prg_pages();
        match addr {
            0x8000..=0xBFFF => self.read_prg(p0, addr),
            0xC000..=0xFFFF => self.read_prg(p1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let low = ((addr >> 2) & 0x1F) as u8;
            let high = ((addr & 0x100) >> 3) as u8; // bit 8 -> bit 5 (0x20)
            self.prg_bank = low | high;
            self.s_flag = (addr & 0x01) != 0;
            self.prg_mode = ((addr >> 7) & 0x01) != 0;
            self.l_flag = ((addr >> 9) & 0x01) != 0;
            self.horizontal_mirroring = (addr & 0x02) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.s_flag));
        out.push(u8::from(self.l_flag));
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 6 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.s_flag = data[2] != 0;
        self.l_flag = data[3] != 0;
        self.prg_mode = data[4] != 0;
        self.horizontal_mirroring = data[5] != 0;
        let mut cursor = 6;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 229 — 31-in-1 BMC.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   When (A & 0x1E) == 0: a fixed 32 KiB NROM bank 0 (the menu).
//   Otherwise: a 16 KiB PRG bank pair = (A & 0x1F) on both $8000 and $C000?
//   The documented decode: $8000 = (A & 0x1F), $C000 = (A & 0x1F) (16 KiB,
//   same bank), CHR (8 KiB) bank = A & 0x0F, mirroring = (A & 0x20).
// CHR is ROM. No IRQ.
// ===========================================================================

/// Mapper 229 (`31-in-1` BMC).
pub struct Multicart229 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// Latched absolute address bits (low 6) used by the decode.
    addr_latch: u8,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart229 {
    /// Construct a new mapper 229 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 229 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 229 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            addr_latch: 0,
            chr_bank: 0,
            horizontal_mirroring: false,
        })
    }

    /// True when the latched address selects the fixed 32 KiB NROM menu bank.
    const fn is_menu(&self) -> bool {
        (self.addr_latch & 0x1E) == 0
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart229 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        if self.is_menu() {
            // Fixed 32 KiB NROM bank 0.
            let bank16 = usize::from(addr >= 0xC000);
            self.read_prg(bank16, addr)
        } else {
            // 16 KiB bank from the latch, mirrored across both halves.
            let bank = (self.addr_latch & 0x1F) as usize;
            self.read_prg(bank, addr)
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.addr_latch = (addr & 0x3F) as u8;
            self.chr_bank = (addr & 0x0F) as u8;
            self.horizontal_mirroring = (addr & 0x20) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.addr_latch);
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.addr_latch = data[1];
        self.chr_bank = data[2];
        self.horizontal_mirroring = data[3] != 0;
        self.vram.copy_from_slice(&data[4..4 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 233 — 42-in-1 reset-based BMC.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   PRG: bits 0-4 select a 16 KiB bank; bit 5 picks 32/16 KiB mode.
//   mirroring: bits 6-7 -> 0 = one-screen A, 1 = one-screen B, 2 = vertical,
//              3 = horizontal.
// A reset toggles a separate "outer block" line that selects the upper or lower
// half of the ROM; that line is host-driven (the physical reset button), so we
// model it as a fixed power-on `0`. CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 233 (`42-in-1` reset-based BMC).
pub struct Multicart233 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// Reset-selected outer block (host-driven; fixed at power-on).
    outer_block: u8,
    prg_bank: u8,
    /// reg bit 5: set = 16 KiB mode (bank mirrored to both halves), clear =
    /// 32 KiB mode (the pair at bank>>1). puNES `prg_fix_233`.
    mode_16k: bool,
    mirror_mode: u8,
}

impl Multicart233 {
    /// Construct a new mapper 233 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 233 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            outer_block: 0,
            prg_bank: 0,
            mode_16k: false,
            mirror_mode: 0,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart233 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        // puNES prg_fix_233: bank = (reg & 0x1F) | reset. reg bit 5 set =>
        // 16 KiB mode (the SAME bank mirrored to both halves); clear => 32 KiB
        // mode (the pair at bank>>1). The previous code had the mode bit
        // inverted (treating bit-5-set as 32 KiB), so the menu's expected bank
        // never mapped and the multicart booted blank.
        let bank16 = (self.prg_bank as usize) | ((self.outer_block as usize) << 5);
        match addr {
            0x8000..=0xBFFF => {
                let b = if self.mode_16k { bank16 } else { bank16 & !1 };
                self.read_prg(b, addr)
            }
            0xC000..=0xFFFF => {
                let b = if self.mode_16k {
                    bank16
                } else {
                    (bank16 & !1) | 1
                };
                self.read_prg(b, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, _addr: u16, value: u8) {
        // puNES iNES 233: a $8000-$FFFF write carries the full register in the
        // DATA byte. PPPPP (bits 0-4) = PRG page (combined with the reset outer
        // line), bit 5 = 16 KiB mode select (set = 16 KiB mirrored both halves,
        // clear = 32 KiB pair), MM (bits 6-7) = mirroring (0 = 1-screen A,
        // 1 = vertical, 2 = horizontal, 3 = 1-screen B).
        self.prg_bank = value & 0x1F;
        self.mode_16k = (value & 0x20) != 0;
        self.mirror_mode = (value >> 6) & 0x03;
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.mirror_mode {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::Vertical,
            2 => Mirroring::Horizontal,
            _ => Mirroring::SingleScreenB,
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.outer_block);
        out.push(self.prg_bank);
        out.push(u8::from(self.mode_16k));
        out.push(self.mirror_mode);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.outer_block = data[1];
        self.prg_bank = data[2];
        self.mode_16k = data[3] != 0;
        // Mask to the valid 0..=3 range so a corrupt / hand-edited save state
        // can never produce an out-of-range mirroring mode (adopted from the
        // PR #116 Gemini review).
        self.mirror_mode = data[4] & 0x03;
        let mut cursor = 5;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

/// Which discrete board the [`DiscreteMapper`] models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscreteBoard {
    /// Mapper 46 (Color Dreams "Rumble Station" 15-in-1).
    M46,
    /// Mapper 51 (BMC 11-in-1).
    M51,
    /// Mapper 57 (BMC GK 6-in-1).
    M57,
    /// Mapper 104 (Codemasters Golden Five / Pegasus 5-in-1).
    M104,
    /// Mapper 120 (Tobidase Daisakusen FDS-conversion protection).
    M120,
    /// Mapper 290 (NTDEC Asder BMC-NTD-03).
    M290,
    /// Mapper 301 (BMC-8157 address-as-data multicart).
    M301,
}

/// A discrete unlicensed/multicart board with a simple register surface.
pub struct DiscreteMapper {
    board: DiscreteBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    reg0: u8,
    reg1: u8,
    /// For 301 (address-as-data) the last-written address.
    last_addr: u16,
    mirroring: Mirroring,
}

impl DiscreteMapper {
    fn new(
        board: DiscreteBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        id: u16,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            board,
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg0: 0,
            reg1: 0,
            last_addr: 0,
            mirroring,
        })
    }

    fn prg_16k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }

    fn prg_32k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
    }

    fn prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn chr_8k_bank(&self) -> usize {
        match self.board {
            DiscreteBoard::M46 => {
                ((self.reg0 as usize & 0xF0) >> 1) | ((self.reg1 as usize & 0x70) >> 4)
            }
            DiscreteBoard::M57 => {
                ((self.reg0 as usize & 0x40) >> 3) | ((self.reg0 | self.reg1) as usize & 0x07)
            }
            _ => 0,
        }
    }
}

impl Mapper for DiscreteMapper {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match self.board {
            DiscreteBoard::M46 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    let bank = ((self.reg0 as usize & 0x0F) << 1) | (self.reg1 as usize & 0x01);
                    self.prg_32k(bank, addr)
                } else {
                    0
                }
            }
            DiscreteBoard::M51 => {
                let bank4 = (self.reg0 as usize) << 2;
                match addr {
                    0x6000..=0x7FFF => {
                        let b = if self.reg1 & 0x01 != 0 {
                            0x23 | bank4
                        } else {
                            0x2F | bank4
                        };
                        self.prg_8k(b, addr)
                    }
                    0x8000..=0xBFFF if self.reg1 & 0x01 != 0 => self.prg_16k(bank4 >> 1, addr),
                    0xC000..=0xFFFF if self.reg1 & 0x01 != 0 => {
                        self.prg_16k((bank4 >> 1) | 1, addr)
                    }
                    0x8000..=0xBFFF => {
                        self.prg_16k((bank4 >> 1) | (self.reg1 as usize >> 1 & 0x01), addr)
                    }
                    0xC000..=0xFFFF => self.prg_16k((bank4 >> 1) | 0x07, addr),
                    _ => 0,
                }
            }
            DiscreteBoard::M57 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    if self.reg1 & 0x10 != 0 {
                        let b = ((self.reg1 as usize >> 5) & 0x06) >> 1;
                        self.prg_32k(b, addr)
                    } else {
                        let b16 = (self.reg1 as usize >> 5) & 0x07;
                        self.prg_16k(b16, addr)
                    }
                } else {
                    0
                }
            }
            DiscreteBoard::M104 => match addr {
                0x8000..=0xBFFF => self.prg_16k(self.reg0 as usize, addr),
                0xC000..=0xFFFF => {
                    let high = (self.reg1 as usize & 0x70) | 0x0F;
                    self.prg_16k(high, addr)
                }
                _ => 0,
            },
            DiscreteBoard::M120 => match addr {
                0x6000..=0x7FFF => self.prg_8k(self.reg0 as usize, addr),
                0x8000..=0xFFFF => {
                    let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
                    let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                    let base = count.saturating_sub(4);
                    self.prg_8k(base + slot, addr)
                }
                _ => 0,
            },
            DiscreteBoard::M290 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.prg_16k(self.reg0 as usize, addr)
                } else {
                    0
                }
            }
            DiscreteBoard::M301 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    let a = self.last_addr;
                    let inner = (a >> 2) & 0x07;
                    let outer128 = (a >> 5) & 0x03;
                    // A7 is the 256 KiB outer-bank select; without it any PRG
                    // image > 256 KiB can only reach its low half. Slot it
                    // between the 128 KiB (A5-A6) and 512 KiB (A8) selects.
                    let outer256 = (a >> 7) & 0x01;
                    let outer512 = (a >> 8) & 0x01;
                    let bank16 = (outer512 << 6) | (outer256 << 5) | (outer128 << 3) | inner;
                    self.prg_16k(bank16 as usize, addr)
                } else {
                    0
                }
            }
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match self.board {
            DiscreteBoard::M51 | DiscreteBoard::M120 => (0x4020..=0x5FFF).contains(&addr),
            _ => (0x4020..=0x7FFF).contains(&addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            DiscreteBoard::M46 => {
                if addr < 0x8000 {
                    self.reg0 = value;
                } else {
                    self.reg1 = value;
                }
            }
            DiscreteBoard::M51 => match addr {
                0x6000..=0x7FFF => self.reg1 = ((value >> 3) & 0x02) | ((value >> 1) & 0x01),
                0xC000..=0xDFFF => {
                    self.reg0 = value & 0x0F;
                    self.reg1 = ((value >> 3) & 0x02) | (self.reg1 & 0x01);
                }
                0x8000..=0xFFFF => self.reg0 = value & 0x0F,
                _ => {}
            },
            DiscreteBoard::M57 => match addr & 0x8800 {
                0x8000 => self.reg0 = value,
                0x8800 => self.reg1 = value,
                _ => {}
            },
            DiscreteBoard::M104 => {
                if addr >= 0xC000 {
                    self.reg0 = (self.reg0 & 0xF0) | (value & 0x0F);
                } else if (0x8000..=0x9FFF).contains(&addr) && value & 0x08 != 0 {
                    self.reg0 = (self.reg0 & 0x0F) | ((value << 4) & 0x70);
                    self.reg1 = value;
                }
            }
            DiscreteBoard::M120 => {
                if addr == 0x41FF {
                    self.reg0 = value;
                }
            }
            DiscreteBoard::M290 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    let prg = ((addr >> 10) & 0x1E) as u8;
                    let chr = (((addr & 0x0300) >> 5) | (addr & 0x07)) as u8;
                    self.reg0 = if addr & 0x80 != 0 {
                        prg | ((addr >> 6) & 1) as u8
                    } else {
                        prg & 0xFE
                    };
                    self.reg1 = chr;
                    self.mirroring = if addr & 0x400 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            DiscreteBoard::M301 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.last_addr = addr;
                    self.mirroring = if addr & 0x02 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                let count = (self.chr.len() / CHR_BANK_8K).max(1);
                let bank = self.chr_8k_bank() % count;
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let off = addr as usize & (self.chr.len() - 1);
                self.chr[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(6 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg0);
        out.push(self.reg1);
        out.push((self.last_addr & 0xFF) as u8);
        out.push((self.last_addr >> 8) as u8);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 6 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg0 = data[1];
        self.reg1 = data[2];
        self.last_addr = u16::from(data[3]) | (u16::from(data[4]) << 8);
        self.mirroring = byte_to_mirroring(data[5], self.mirroring);
        let mut cursor = 6;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        }
        Ok(())
    }
}

macro_rules! discrete_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<DiscreteMapper, MapperError> {
            DiscreteMapper::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

discrete_ctor!(
    new_m46,
    DiscreteBoard::M46,
    46,
    "Mapper 46 (Color Dreams Rumble Station 15-in-1)."
);
discrete_ctor!(
    new_m51,
    DiscreteBoard::M51,
    51,
    "Mapper 51 (BMC 11-in-1 multicart)."
);
discrete_ctor!(
    new_m57,
    DiscreteBoard::M57,
    57,
    "Mapper 57 (BMC GK 6-in-1 multicart)."
);
discrete_ctor!(
    new_m104,
    DiscreteBoard::M104,
    104,
    "Mapper 104 (Codemasters Golden Five / Pegasus 5-in-1)."
);
discrete_ctor!(
    new_m120,
    DiscreteBoard::M120,
    120,
    "Mapper 120 (Tobidase Daisakusen FDS-conversion protection)."
);
discrete_ctor!(
    new_m290,
    DiscreteBoard::M290,
    290,
    "Mapper 290 (NTDEC Asder BMC-NTD-03)."
);
discrete_ctor!(
    new_m301,
    DiscreteBoard::M301,
    301,
    "Mapper 301 (BMC-8157 address-as-data multicart)."
);

/// Discrete NROM/UNROM 2-in-1 BMC multicart (mapper 204).
pub struct Bmc204 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// 16 KiB PRG windows for $8000 and $C000.
    prg0: usize,
    prg1: usize,
    /// 8 KiB CHR window.
    chr8: usize,
}

impl Bmc204 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 204)?;
        if prg_rom.len() < PRG_BANK_16K {
            return Err(MapperError::Invalid(format!(
                "mapper 204 PRG-ROM size {} is smaller than one 16 KiB bank",
                prg_rom.len()
            )));
        }
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        let mut m = Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg0: 0,
            prg1: 0,
            chr8: 0,
        };
        // Power-on: WriteRegister(0x8000, 0).
        m.write_addr(0x8000);
        Ok(m)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn chr_count_8k(&self) -> usize {
        (self.chr.len() / CHR_BANK_8K).max(1)
    }

    fn write_addr(&mut self, addr: u16) {
        let bit_mask = (addr & 0x06) as usize;
        let page = bit_mask
            + if bit_mask == 0x06 {
                0
            } else {
                (addr & 0x01) as usize
            };
        self.prg0 = page;
        self.prg1 = bit_mask
            + if bit_mask == 0x06 {
                1
            } else {
                (addr & 0x01) as usize
            };
        self.chr8 = page;
        self.mirroring = if addr & 0x10 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
    }

    fn prg_byte(&self, slot16: usize, addr: u16) -> u8 {
        let count = self.prg_count_16k();
        let bank = slot16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Bmc204 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.prg_byte(self.prg0, addr),
            0xC000..=0xFFFF => self.prg_byte(self.prg1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if addr >= 0x8000 {
            self.write_addr(addr);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                let bank = self.chr8 % self.chr_count_8k();
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + 12 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        out.extend_from_slice(&(self.prg1 as u32).to_le_bytes());
        out.extend_from_slice(&(self.chr8 as u32).to_le_bytes());
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 12 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        self.prg0 = rd(1);
        self.prg1 = rd(5);
        self.chr8 = rd(9);
        let mut c = 13;
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 204 (discrete NROM/UNROM 2-in-1 BMC multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m204(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Bmc204, MapperError> {
    Bmc204::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// NtdecN625092 (mapper 221) — NTDEC N625092 multicart.
//
// $8000 latches a 16-bit "mode" from the written address; $C000 latches the
// 3-bit inner PRG register. The outer bank is `(mode & 0xFC) >> 2`. When
// `mode & 0x02` the board is in UNROM-style mode (a switchable $8000 + a fixed
// $C000), with a NROM-256 sub-case when `mode & 0x0100`; otherwise both 16 KiB
// windows mirror the same NROM bank. `mode & 0x01` flips the mirroring. CHR is a
// single fixed 8 KiB window. Ported from Mesen2 Ntdec/Mapper221.h.
// ===========================================================================

/// TXC/BMC-11160 multicart (mapper 299).
pub struct Bmc11160 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// 32 KiB PRG window.
    prg32: usize,
    /// 8 KiB CHR window.
    chr8: usize,
}

impl Bmc11160 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 299)?;
        if prg_rom.len() < PRG_BANK_32K {
            return Err(MapperError::Invalid(format!(
                "mapper 299 PRG-ROM size {} is smaller than one 32 KiB bank",
                prg_rom.len()
            )));
        }
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        let mut m = Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg32: 0,
            chr8: 0,
        };
        // Power-on (Reset): WriteRegister(0x8000, 0).
        m.write_reg(0);
        Ok(m)
    }

    fn prg_count_32k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_32K).max(1)
    }

    fn chr_count_8k(&self) -> usize {
        (self.chr.len() / CHR_BANK_8K).max(1)
    }

    fn write_reg(&mut self, value: u8) {
        let bank = ((value >> 4) & 0x07) as usize;
        self.prg32 = bank;
        self.chr8 = (bank << 2) | (value as usize & 0x03);
        self.mirroring = if value & 0x80 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
    }
}

impl Mapper for Bmc11160 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let count = self.prg_count_32k();
                let bank = self.prg32 % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.write_reg(value);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                let bank = self.chr8 % self.chr_count_8k();
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + 8 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg32 as u32).to_le_bytes());
        out.extend_from_slice(&(self.chr8 as u32).to_le_bytes());
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 8 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        self.prg32 = rd(1);
        self.chr8 = rd(5);
        let mut c = 9;
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 299 (TXC/BMC-11160 multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m299(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Bmc11160, MapperError> {
    Bmc11160::new(prg_rom, chr_rom, mirroring)
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    /// 8 KiB-banked PRG: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 32 KiB-banked PRG: byte 0 of each 32 KiB bank holds the bank index (all
    /// other offsets are transparent).
    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m15_mode0_two_16k_halves() {
        // 8 16 KiB banks = 128 KiB.
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        // mode 0 ($8000), prg_bank = 2, mirroring bit clear (vertical).
        m.cpu_write(0x8000, 0b0000_0010);
        assert_eq!(m.cpu_read(0x8000), 2); // low half = bank 2
        assert_eq!(m.cpu_read(0xC000), 3); // high half = bank 2|1 = 3
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m15_mirroring_bit() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        m.cpu_write(0x8000, 0b0100_0000); // bit 6 = horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m15_mode3_single_bank_mirrored() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        // mode 3 ($8003), prg_bank = 5.
        m.cpu_write(0x8003, 0b0000_0101);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 5); // 16 KiB mirrored across the window
    }

    #[test]
    fn m15_chr_ram_write_protect() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        // mode 0 -> protected.
        m.cpu_write(0x8000, 0);
        m.ppu_write(0x0000, 0xAB);
        assert_eq!(m.ppu_read(0x0000), 0);
        // mode 2 -> writable.
        m.cpu_write(0x8002, 0);
        m.ppu_write(0x0000, 0xCD);
        assert_eq!(m.ppu_read(0x0000), 0xCD);
    }

    #[test]
    fn m61_16k_mode_address_decode() {
        // 16 16 KiB banks.
        let mut m = Multicart61::new(synth_prg_16k(16), &[]).unwrap();
        // Choose addr with A&0x0F = 3, A>>5&1 = 0 -> page = 6; A&0x10 set (16k);
        // A&0x80 set (horizontal). addr = 0x8000 | 0x10 | 0x80 | 0x03 = 0x8093.
        m.cpu_write(0x8093, 0x00);
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.cpu_read(0xC000), 6); // 16 KiB mirrored
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m61_32k_mode() {
        let mut m = Multicart61::new(synth_prg_16k(16), &[]).unwrap();
        // A&0x0F = 2, A>>5&1 = 0 -> page = 4; 32 KiB mode (A&0x10 clear).
        // 32 KiB bank = page>>1 = 2. addr = 0x8000 | 0x02 = 0x8002.
        m.cpu_write(0x8002, 0x00);
        // 32 KiB bank 2 = 16 KiB banks 4 and 5.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn m62_address_and_data_decode() {
        let mut m =
            Multicart62::new(synth_prg_16k(8), synth_chr_8k(256), Mirroring::Vertical).unwrap();
        // prg_page = ((A&0x3F00)>>8) | (A&0x40); pick A bits so page small.
        // A = 0x8000 | (0x01 << 8) | 0x20(16k mode) | 0x80(horiz) | 0x05(chr lo)
        //   prg_page = 0x01, 16k mode, horizontal, chr = (5<<2)|data&3.
        let addr = 0x8000 | (0x01 << 8) | 0x20 | 0x80 | 0x05;
        m.cpu_write(addr, 0x02); // data low 2 bits = 2
        assert_eq!(m.cpu_read(0x8000), 1); // 16k bank 1
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // chr_bank = (5<<2)|2 = 22.
        assert_eq!(m.ppu_read(0x0000), 22);
    }

    #[test]
    fn m15_save_state_round_trips_mirroring() {
        let mut m = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        m.cpu_write(0x8001, 0b0100_0101);
        let blob = m.save_state();
        let mut m2 = Multicart15::new(synth_prg_16k(8), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), m.current_mirroring());
    }

    #[test]
    fn m200_address_latch() {
        let mut m =
            Multicart200::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // Write address with low bits = 3 and bit3 set (horizontal).
        m.cpu_write(0x8000 | 0x0B, 0x00); // 0x0B = 0b1011: bank 3, H bit set
        assert_eq!(m.cpu_read(0x8000), 3);
        // NROM-128: $8000 mirrors $C000.
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // bit3 clear -> vertical.
        m.cpu_write(0x8000 | 0x02, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        assert_eq!(m.cpu_read(0x8000), 2);
    }

    #[test]
    fn m201_address_drives_prg_and_chr() {
        let mut m =
            Multicart201::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // addr low 3 bits = 0b011: PRG = 3 & 3 = 3, CHR = 3.
        m.cpu_write(0x8000 | 0x03, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        // addr low 3 bits = 0b101: PRG = 5 & 3 = 1, CHR = 5.
        m.cpu_write(0x8000 | 0x05, 0x00);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn m202_16k_mode() {
        let mut m =
            Multicart202::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // page = (addr>>1)&7. Pick page 3 -> addr bits 3..1 = 0b011 -> addr = 0b0110.
        // O bits (bit3 and bit0): bit3 = 0, bit0 = 0 -> not both set -> 16k mode.
        // mirroring = addr bit0 = 0 -> vertical.
        m.cpu_write(0x8000 | 0b0110, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3); // mirrored in 16k mode
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m202_32k_mode_and_mirroring() {
        let mut m =
            Multicart202::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // Both O bits set: addr bit3 = 1 and bit0 = 1.
        // page = (addr>>1)&7. addr = 0b1001 | (page<<1). Pick page 2 -> page<<1 = 0b100.
        // addr = 0b1101 = 0x0D: bit3=1, bit0=1 -> 32k mode. page = (0xD>>1)&7 = 6&7 = 6.
        // Recompute to make page even/clear: choose addr = 0x09 (0b1001): page = (9>>1)&7 = 4.
        //   bit3=1, bit0=1 -> 32k. mirroring = bit0 = 1 -> horizontal.
        m.cpu_write(0x8000 | 0x09, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // 32k bank = (page>>1)<<1 = (4>>1)<<1 = 4. Bank 4 at $8000, bank 5 at $C000.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn m203_data_latch() {
        let mut m =
            Multicart203::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // value PPPPPPCC: PRG = value>>2, CHR = value&3.
        // 0b0000_1110 = 0x0E: PRG = 3, CHR = 2.
        m.cpu_write(0x8000, 0x0E);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3); // mirrored
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    #[test]
    fn m212_16k_mode_and_protection_read() {
        let mut m =
            Multicart212::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // 16k mode (bit14 clear). page = addr & 7 = 3, mirroring bit3 = 1 (H).
        m.cpu_write(0x8000 | 0x0B, 0x00); // 0b1011: page 3, H bit set
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Protection read: $6000 (addr&0x10 == 0) -> bit7 set.
        assert_eq!(m.cpu_read(0x6000) & 0x80, 0x80);
        assert_eq!(m.cpu_read(0x6010), 0x00);
    }

    #[test]
    fn m212_32k_mode() {
        let mut m =
            Multicart212::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // bit14 set -> 32k mode. page = addr & 7 = 4. 32k bank = (4>>1)<<1 = 4.
        m.cpu_write(0xC000 | 0x04, 0x00); // 0xC004: bit14 set, page 4
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn m213_address_latch() {
        let mut m =
            Multicart213::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // CHR = (addr>>3)&7, PRG = (addr>>1)&3.
        // Pick CHR 5, PRG 2: addr bits: (5<<3)|(2<<1) = 0x28 | 0x04 = 0x2C.
        m.cpu_write(0x8000 | 0x2C, 0x00);
        assert_eq!(m.ppu_read(0x0000), 5);
        assert_eq!(m.cpu_read(0x8000), 2);
    }

    #[test]
    fn m214_address_latch() {
        let mut m =
            Multicart214::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // CHR = addr & 3, PRG = (addr>>2)&3.
        // Pick PRG 2, CHR 1: addr bits = (2<<2)|1 = 0x09.
        m.cpu_write(0x8000 | 0x09, 0x00);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 2); // mirrored
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m58_address_decoded_banking() {
        let mut m =
            Multicart58::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // 16 KiB mode (bit 6 set): A = $8000 | (1<<6) | (CHR=2<<3) | (PRG=3).
        let addr = 0x8000 | (1 << 6) | (0b010 << 3) | 0b011;
        m.cpu_write(addr, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3); // 16 KiB bank 3
        assert_eq!(m.ppu_read(0x0000), 2); // CHR bank 2
        // 32 KiB mode (bit 6 clear): PRG bank = (A&6)>>1. A low bits = 0b110 = 6
        // -> 32 KiB bank (6&6)>>1 = 3.
        let addr32 = 0x8000 | 0b110;
        m.cpu_write(addr32, 0x00);
        // synth_prg_16k(8) = 4 32 KiB banks; bank 3 offset 0 holds index 6.
        assert_eq!(m.cpu_read(0x8000), 6);
    }

    #[test]
    fn m58_save_state_round_trip() {
        let mut m =
            Multicart58::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        let addr = 0x8000 | (1 << 7) | (1 << 6) | (0b001 << 3) | 0b010;
        m.cpu_write(addr, 0x00);
        let blob = m.save_state();
        let mut m2 =
            Multicart58::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 2);
        assert_eq!(m2.ppu_read(0x0000), 1);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m60_power_on_bank_zero() {
        let mut m =
            Multicart60::new(synth_prg_16k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 0);
        // Writes are ignored (reset-driven selection not modelled).
        m.cpu_write(0x8000, 0xFF);
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m60_save_state_round_trip() {
        let mut m =
            Multicart60::new(synth_prg_16k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m.ppu_write(0x2000, 0x44);
        let blob = m.save_state();
        let mut m2 =
            Multicart60::new(synth_prg_16k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x2000), 0x44);
    }

    #[test]
    fn m231_dual_bank_address_decoded() {
        let mut m = Multicart231::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        // A bits: prgBank = ((A>>5)&1) | (A&0x1E). Pick A = $8000 | 0x14 (= 0b1_0100).
        // A&0x1E = 0x14 = 20; (A>>5)&1 = 0. prgBank = 20.
        // bank0 = 20 & 0x1E = 20; bank1 = 20.
        let addr = 0x8000 | 0x14;
        m.cpu_write(addr, 0x00);
        assert_eq!(m.cpu_read(0x8000), 20);
        assert_eq!(m.cpu_read(0xC000), 20);
        // Set bit 5 (0x20): contributes 1 to bank1's LSB; A&0x1E unchanged.
        let addr2 = 0x8000 | 0x20 | 0x14; // (A>>5)&1 = 1
        m.cpu_write(addr2, 0x00);
        // prgBank = 1 | 20 = 21; bank0 = 21 & 0x1E = 20; bank1 = 21.
        assert_eq!(m.cpu_read(0x8000), 20);
        assert_eq!(m.cpu_read(0xC000), 21);
    }

    #[test]
    fn m231_save_state_round_trip() {
        let mut m = Multicart231::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000 | 0x80 | 0x04, 0x00); // horizontal, bank bits
        m.ppu_write(0x0008, 0x66);
        let blob = m.save_state();
        let mut m2 = Multicart231::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0008), 0x66);
    }

    #[test]
    fn m234_cnrom_mode_banks() {
        let mut m =
            Maxi15M234::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // reg0 latch (CNROM mode, bit6 clear). Write $FF80 with value 0x05:
        //   reg0 = 0x05 -> prgBank = reg0 & 0x0F = 5.
        //   chrBank = ((reg0<<2)&0x3C) | ((reg1>>4)&3) = (0x14) | 0 = 20.
        m.cpu_write(0xFF80, 0x05);
        assert_eq!(m.cpu_read(0x8000), 5);
        // chrBank = ((0x05<<2)&0x3C) = 0x14 = 20; 16-bank ROM wraps to 4.
        assert_eq!(m.ppu_read(0x0000), 20 % 16);
        // reg1 sets CHR low bits via $FFE8 (value 0x10 -> (0x10>>4)&3 = 1).
        m.cpu_write(0xFFE8, 0x10);
        // chrBank = 0x14 | 0x01 = 0x15 = 21; wraps to 5.
        assert_eq!(m.ppu_read(0x0000), 0x15 % 16);
    }

    #[test]
    fn m234_save_state_round_trip() {
        let mut m =
            Maxi15M234::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0xFF80, 0x83); // reg0: horizontal (bit7), prg = 3
        m.cpu_write(0xFFE8, 0x20);
        let blob = m.save_state();
        let mut m2 =
            Maxi15M234::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 3);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m225_address_decoded_and_scratch_ram() {
        let mut m =
            Multicart225::new(synth_prg_16k(16), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // nesdev decode A~[.HMO PPPP PPCC CCCC]: PRG = A6..A9, O(mode) = A10,
        // M(mirror) = A11, H = A14. A = 0x8080: PRG = (0x80>>6)&0xF = 2; O = 0 ->
        // 32K; M = 0 -> vertical; CHR = A&0x3F = 0.
        m.cpu_write(0x8080, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // Scratch RAM round-trips low nibble.
        m.cpu_write(0x5800, 0xA9);
        assert_eq!(m.cpu_read(0x5800), 0x09);
    }

    #[test]
    fn m225_save_state_round_trip() {
        let mut m =
            Multicart225::new(synth_prg_16k(16), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x9180, 0); // some bank
        m.cpu_write(0x5803, 0x05);
        let blob = m.save_state();
        let mut m2 =
            Multicart225::new(synth_prg_16k(16), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.cpu_read(0x5803), 0x05);
    }

    #[test]
    fn m226_two_regs_select_prg_and_mirror() {
        let mut m = Multicart226::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // reg0 (even): low bits = 3, bit6 = mirror H. value 0b0100_0011 = 0x43.
        m.cpu_write(0x8000, 0x43);
        // reg1 (odd): bit0 = 0.
        m.cpu_write(0x8001, 0x00);
        // 16K mode (reg0 bit7 = 0): bank 3 on both halves.
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m226_save_state_round_trip() {
        let mut m = Multicart226::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x85); // 32K mode, low = 5
        m.cpu_write(0x8001, 0x00);
        m.ppu_write(0x0001, 0x66);
        let blob = m.save_state();
        let mut m2 = Multicart226::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0001), 0x66);
    }

    #[test]
    fn m227_address_decoded_bank() {
        let mut m = Multicart227::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // A = 0x8008: prg_bank = (0x8008>>2)&0x1F = 2; s=(A&1)=0, prg_mode=
        // (A>>7)&1=0, l=(A>>9)&1=0, mirror=(A&2)=0 -> V. UNROM-like, s=0,l=0:
        // $8000 = bank 2, $C000 = bank & 0x38 = 0.
        m.cpu_write(0x8008, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // l_flag set (A bit 9 = 0x200): $C000 fixed to bank | 0x07.
        // A = 0x8208: prg_bank still 2, l=1 -> $C000 = 2 | 7 = 7.
        m.cpu_write(0x8208, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 7);
        // prg_mode set without s (A bit 7 = 0x80): NROM-128, both halves = bank.
        // A = 0x8088: prg_bank = (0x8088>>2)&0x1F = 2; prg_mode=1, s=0.
        m.cpu_write(0x8088, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 2);
    }

    #[test]
    fn m227_save_state_round_trip() {
        let mut m = Multicart227::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x808B, 0); // prg_mode + s (32K pair) + A&2 -> H
        m.ppu_write(0x0002, 0x33);
        let blob = m.save_state();
        let mut m2 = Multicart227::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0002), 0x33);
    }

    #[test]
    fn m229_menu_bank_and_game_bank() {
        let mut m =
            Multicart229::new(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // A with low 5 bits zero -> menu (fixed NROM-32 bank 0).
        m.cpu_write(0x8000, 0);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 1);
        // A = 0x8003: latch = 3 (non-menu) -> 16K bank 3 on both halves.
        m.cpu_write(0x8003, 0);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn m229_save_state_round_trip() {
        let mut m =
            Multicart229::new(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8025, 0); // latch with chr + mirror H
        let blob = m.save_state();
        let mut m2 =
            Multicart229::new(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.current_mirroring(), m.current_mirroring());
    }

    #[test]
    fn m233_bank_and_mirror_modes() {
        let mut m = Multicart233::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // puNES: bit 5 SET = 16 KiB mode (one bank mirrored to both halves),
        // bits 6-7 = mirroring. value 0xA5 = MM=10 (horizontal), bit5=1 (16K),
        // bank = 0x05.
        m.cpu_write(0x8000, 0xA5);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // bit 5 CLEAR = 32 KiB mode: the pair at (bank>>1)<<1. value 0x05 ->
        // bank 5, 32K pair = banks 4 ($8000) / 5 ($C000).
        m.cpu_write(0x8000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn m233_save_state_round_trip() {
        let mut m = Multicart233::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x26); // bit5=1 -> 16K mode, bank 6
        m.ppu_write(0x0004, 0x88);
        let blob = m.save_state();
        let mut m2 = Multicart233::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0004), 0x88);
    }

    #[test]
    fn m46_outer_inner_prg() {
        let mut m = new_m46(synth_prg_32k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0x02); // outer.
        m.cpu_write(0x8000, 0x01); // inner.
        // bank = (2 << 1) | 1 = 5.
        assert_eq!(m.cpu_read(0x8000), 5);
    }

    #[test]
    fn m104_golden_five_blocks() {
        let mut m = new_m104(synth_prg_16k(32), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x08 | 0x02); // block bits -> reg0 high = 0x20.
        m.cpu_write(0xC000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 37 % 32); // inner ((0x20)|5) = 37.
        assert_eq!(m.cpu_read(0xC000), 47 % 32); // high|0x0F = 47.
    }

    #[test]
    fn m290_address_decoded_mirror() {
        let mut m = new_m290(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8400, 0); // bit 0x400 -> horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m301_address_as_data_mirror() {
        let mut m = new_m301(synth_prg_16k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8002, 0); // addr bit 1 -> horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn discrete_save_state_round_trip() {
        let mut m = new_m51(synth_prg_8k(64), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0x10);
        m.cpu_write(0xC000, 0x05);
        m.ppu_write(0x0020, 0x7F);
        let blob = m.save_state();
        let mut m2 = new_m51(synth_prg_8k(64), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0020), 0x7F);
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
    }

    fn prg(banks_8k: usize) -> Box<[u8]> {
        // Fill each 8 KiB bank with its index so bank routing is observable.
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for (i, b) in v.chunks_mut(PRG_BANK_8K).enumerate() {
            b.fill(i as u8);
        }
        v.into_boxed_slice()
    }

    fn chr(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * CHR_BANK_8K];
        for (i, b) in v.chunks_mut(CHR_BANK_8K).enumerate() {
            b.fill(i as u8);
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m299_load_state_rejects_truncated_and_bad_version() {
        let m = new_m299(prg(8), chr(8), Mirroring::Horizontal).unwrap();
        let mut s = m.save_state();
        // Truncate.
        let mut t = m.save_state();
        t.pop();
        let mut m2 = new_m299(prg(8), chr(8), Mirroring::Horizontal).unwrap();
        assert!(matches!(
            m2.load_state(&t),
            Err(MapperError::Truncated { .. })
        ));
        // Bad version.
        s[0] = 0xFF;
        assert!(matches!(
            m2.load_state(&s),
            Err(MapperError::UnsupportedVersion(0xFF))
        ));
    }

    #[test]
    fn m204_address_decode_selects_prg_and_chr() {
        let mut m = new_m204(prg(16), chr(8), Mirroring::Vertical).unwrap();
        // bitMask = addr&6 = 0; page = 0 + (addr&1) = 0.
        m.cpu_write(0x8000, 0);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 0); // prg1 = 0 + (addr&1) = 0
        // addr 0x8007: bitMask = 6 -> page = 6+0 = 6; prg1 = 6+1 = 7.
        m.cpu_write(0x8007, 0);
        assert_eq!(m.cpu_read(0x8000), 12, "16k page 6 -> 8k bank 12");
    }

    #[test]
    fn m204_distinct_halves_in_bitmask6_mode() {
        let mut m = new_m204(prg(16), chr(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8006, 0); // bitMask 6: prg0=6, prg1=7 (16 KiB pages)
        // 16 KiB page 6 => 8 KiB bank 12 at $8000.
        assert_eq!(m.cpu_read(0x8000), 12);
        // 16 KiB page 7 => 8 KiB bank 14 at $C000.
        assert_eq!(m.cpu_read(0xC000), 14);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m204_mirroring_bit() {
        let mut m = new_m204(prg(4), chr(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8010, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m299_value_decode_selects_prg_chr_mirror() {
        let mut m = new_m299(prg(8 * 4), chr(32), Mirroring::Horizontal).unwrap();
        // 32 KiB PRG: 4 banks of 32 KiB (32 8-KiB banks / 4). value 0x10:
        // bank = (0x10>>4)&7 = 1; chr8 = (1<<2)|0 = 4; bit7 clear => horizontal.
        m.cpu_write(0x8000, 0x10);
        assert_eq!(m.cpu_read(0x8000), 4, "32k bank 1 -> 8k bank 4");
        assert_eq!(m.ppu_read(0x0000), 4, "chr 8k bank 4");
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m299_chr_low_bits_and_mirror() {
        let mut m = new_m299(prg(8 * 2), chr(16), Mirroring::Horizontal).unwrap();
        // value 0x83: bank = 0; chr8 = (0<<2)|3 = 3; bit7 set => vertical.
        m.cpu_write(0xFFFF, 0x83);
        assert_eq!(m.cpu_read(0x8000), 0, "32k bank 0");
        assert_eq!(m.ppu_read(0x0000), 3, "chr 8k bank 3");
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m204_m299_save_load_round_trip() {
        // m204
        let mut a = new_m204(prg(16), chr(8), Mirroring::Vertical).unwrap();
        a.cpu_write(0x8006, 0);
        let s = a.save_state();
        let mut b = new_m204(prg(16), chr(8), Mirroring::Horizontal).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0xC000), b.cpu_read(0xC000));
        assert_eq!(a.current_mirroring(), b.current_mirroring());

        // m299
        let mut a = new_m299(prg(8 * 4), chr(32), Mirroring::Horizontal).unwrap();
        a.cpu_write(0x8000, 0x91);
        let s = a.save_state();
        let mut b = new_m299(prg(8 * 4), chr(32), Mirroring::Horizontal).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0x8000), b.cpu_read(0x8000));
        assert_eq!(a.ppu_read(0x0000), b.ppu_read(0x0000));
        assert_eq!(a.current_mirroring(), b.current_mirroring());
    }

    #[test]
    fn m204_m299_bad_prg_size_is_rejected() {
        // 100 bytes is not a multiple of 8 KiB.
        assert!(
            new_m204(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
        assert!(
            new_m299(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
    }
}
