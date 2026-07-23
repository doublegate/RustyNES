//! Waixing boards: mappers 162, 178, 242 and 253.
//!
//! Waixing produced a long line of unlicensed Chinese cartridges. Most are
//! address-decoded 32 KiB PRG banking with a mirroring bit and optional
//! PRG-RAM -- broad rather than deep, with the variation being which address
//! bits carry the bank and where the mirroring control sits.
//!
//! Mapper 253 is the exception and the reason this module is not trivial: it
//! carries a *scaled* IRQ counter (the prescaler divides the CPU clock before
//! the counter sees it) and a CHR-RAM escape, where two specific CHR bank
//! values redirect the fetch to RAM instead of ROM. Both are modelled here
//! rather than approximated.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the reference emulators (`Mesen2`, `GeraNES`) and the nesdev wiki, with no
//! commercial-oracle ROM in the tree. Banking math is direct slice indexing and
//! every bank select wraps with `% count`, so a register write can never index
//! out of bounds -- required for the `#![no_std]` chip stack, which cannot
//! afford a panic on a register access.
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

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_2K: usize = 0x0800;
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

const CHR_BANK_1K: usize = 0x0400;

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

/// Mapper 242 (Waixing `43-in-1`).
pub struct Waixing242 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// 8 KiB work-RAM at $6000-$7FFF.
    prg_ram: Box<[u8]>,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Waixing242 {
    /// Construct a new mapper 242 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 242 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            prg_bank: 0,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Waixing242 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The $6000-$7FFF work-RAM is mapped (the trait default already treats
    // $6000-$FFFF as mapped, so no override is needed).

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize]
        } else if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize] = value;
        } else if (0x8000..=0xFFFF).contains(&addr) {
            // nesdev iNES 242 decode (address-latched): bit 1 (M) = mirroring
            // (0 = vertical, 1 = horizontal); the 32 KiB PRG bank = the inner
            // bank (PRG A16..A14 = address bits 2..4) OR'd with the outer bank
            // (PRG A18..A17 = address bits 5..6) shifted into place. The whole
            // $8000-$FFFF window is one switchable 32 KiB page.
            let inner = ((addr >> 2) & 0x07) as u8;
            let outer = ((addr >> 5) & 0x03) as u8;
            // inner is A16..A14 (a 16 KiB granularity); the 32 KiB bank takes the
            // upper bits: A18..A15 = outer<<2 | inner>>1.
            self.prg_bank = (outer << 2) | (inner >> 1);
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
        let mut out =
            Vec::with_capacity(3 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len();
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
        self.horizontal_mirroring = data[2] != 0;
        let mut cursor = 3;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

/// Mapper 162 (Waixing FS304).
pub struct WaixingFs304M162 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// 8 KiB battery-backed PRG-RAM at CPU $6000-$7FFF. The Waixing RPGs read
    /// it during boot; without it they hang on a blank frame.
    prg_ram: Box<[u8]>,
    regs: [u8; 4],
    mirroring: Mirroring,
}

impl WaixingFs304M162 {
    /// Construct a new mapper 162 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 162 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            regs: [0; 4],
            mirroring,
        })
    }

    const fn prg_bank(&self) -> usize {
        let r0 = self.regs[0] as usize;
        let r1 = self.regs[1] as usize;
        let r2 = self.regs[2] as usize;
        let r3 = self.regs[3] as usize;
        let a = (r3 >> 2) & 1; // $5300.2 — A16 mode
        let b = r3 & 1; // $5300.0 — A15 mode
        let a16 = if a == 0 { 1 } else { (r0 >> 1) & 1 };
        let a15 = if b == 0 {
            (r1 >> 1) & 1
        } else if a == 0 {
            1
        } else {
            r0 & 1
        };
        let a17 = (r0 >> 2) & 1;
        let a18 = (r0 >> 3) & 1;
        let a19 = r2 & 1;
        let a20 = (r2 >> 1) & 1;
        a15 | (a16 << 1) | (a17 << 2) | (a18 << 3) | (a19 << 4) | (a20 << 5)
    }
}

impl Mapper for WaixingFs304M162 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
                let bank = self.prg_bank() % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $5000-$5FFF carries the write-only register block; the rest of
        // $4020-$5FFF is open bus. $6000-$7FFF is PRG-RAM and $8000-$FFFF is
        // mapped PRG.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize] = value;
        } else if (0x5000..=0x5FFF).contains(&addr) {
            let idx = ((addr >> 8) & 0x03) as usize;
            self.regs[idx] = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
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
        let mut out =
            Vec::with_capacity(5 + self.vram.len() + self.chr_ram.len() + self.prg_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.prg_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len() + self.chr_ram.len() + self.prg_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.regs.copy_from_slice(&data[1..5]);
        let mut cursor = 5;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 178 — Waixing / San Guo Zhong Chen (educational series).
//
// A $4800-$4803 register block plus 8 KiB work-RAM at $6000 (NESdev
// INES_Mapper_178 / Waixing FS305):
//   $4800 : bit 0 = mirroring (0 = vertical, 1 = horizontal),
//           bits 1-2 = PRG banking mode
//             0 = NROM-256 / BNROM (32 KiB switchable)
//             1 = UNROM (16 KiB switchable at $8000, fixed-111b at $C000)
//             2 = NROM-128 (16 KiB mirrored)
//             3 = UNROM variant ($C000 = inner|1 instead of all-ones).
//   $4801 : bits 0-2 = inner PRG bank (PRG A16..A14, i.e. 16 KiB units).
//   $4802 : outer PRG bank (PRG A17+).
//   $4803 : PRG-RAM bank (stored only; the staged games use a single 8 KiB).
// 16 KiB bank = (reg2 << 3) | (reg1 & 0x07). The OLD code read bit 0 of $4800
// as the PRG mode (it is the MIRRORING bit) and bit 1 as mirroring (it is a
// PRG-mode bit) — the two were swapped, and the bank composition masked wrong,
// so educational titles booted the wrong bank and blanked. CHR is 8 KiB RAM.
// ===========================================================================

/// Mapper 178 (Waixing educational series).
pub struct Waixing178 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    regs: [u8; 4],
}

impl Waixing178 {
    /// Construct a new mapper 178 board.
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
                "mapper 178 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            regs: [0; 4],
        })
    }

    /// Composed inner+outer 16 KiB bank: outer ($4802) shifted past the 3-bit
    /// inner ($4801 bits 0-2).
    const fn prg_base16(&self) -> usize {
        ((self.regs[2] as usize) << 3) | (self.regs[1] as usize & 0x07)
    }

    /// PRG banking mode from $4800 bits 1-2 (0..=3).
    const fn prg_mode(&self) -> u8 {
        (self.regs[0] >> 1) & 0x03
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Waixing178 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let base = self.prg_base16();
                // $8000: NROM-256/BNROM (mode 0) presents the even bank of the
                // 32 KiB pair; every other mode switches a 16 KiB bank directly.
                let bank = if self.prg_mode() == 0 {
                    base & !1
                } else {
                    base
                };
                self.read_prg(bank, addr)
            }
            0xC000..=0xFFFF => {
                let base = self.prg_base16();
                let bank = match self.prg_mode() {
                    // NROM-256 / BNROM: high half of the 32 KiB pair.
                    0 => (base & !1) | 1,
                    // UNROM: $C000 fixed to the last bank of the outer block
                    // (inner bits = 111b).
                    1 => (base & !0x07) | 0x07,
                    // NROM-128: 16 KiB mirrored.
                    2 => base,
                    // UNROM variant: $C000 = inner | 1.
                    _ => base | 1,
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4800-$4803 are write-only registers; $6000-$FFFF is mapped
        // (work-RAM + PRG).  The remaining $4020-$5FFF window is open bus.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4800..=0x4803 => self.regs[(addr - 0x4800) as usize] = value,
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
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
        // $4800 bit 0: 0 = vertical, 1 = horizontal.
        if (self.regs[0] & 0x01) != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(5 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.regs.copy_from_slice(&data[1..5]);
        let mut cursor = 5;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

/// Waixing VRC4-clone (mapper 253, *Dragon Ball Z*).
pub struct Waixing253 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    /// `true` when the cart supplied no CHR-ROM, so `self.chr` is the cart's
    /// (writable) CHR-RAM and must be serialized in the save-state. Distinct
    /// from the 2 KiB `chr_ram` escape (the Mesen2 `lo == 4|5` window), which
    /// always exists.
    chr_is_ram: bool,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    prg_count_8k: usize,
    chr_count_1k: usize,
    prg: [u8; 2],
    chr_low: [u8; 8],
    chr_high: [u8; 8],
    force_chr_rom: bool,
    irq_reload: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_scaler: u16,
    irq_pending: bool,
}

impl Waixing253 {
    const SAVE_LEN: usize = 2 + 8 + 8 + 1 + 1 + 1 + 1 + 2 + 1 + 1;

    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 253)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            // CHR-RAM variant: allocate the conventional 8 KiB (matching the
            // MMC3 `8 * CHR_BANK_1K` convention) so the banked CHR path has a
            // real, writable backing store instead of a stub 1 KiB.
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
                return Err(MapperError::Invalid(format!(
                    "mapper 253 CHR-ROM size {} is not a multiple of 1 KiB",
                    chr_rom.len()
                )));
            }
            chr_rom
        };
        let prg_count_8k = prg_rom.len() / PRG_BANK_8K;
        let chr_count_1k = (chr.len() / CHR_BANK_1K).max(1);
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            chr_ram: vec![0u8; CHR_BANK_2K].into_boxed_slice(), // 2 KiB CHR-RAM escape.
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg_count_8k,
            chr_count_1k,
            prg: [0; 2],
            chr_low: [0; 8],
            chr_high: [0; 8],
            force_chr_rom: false,
            irq_reload: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_scaler: 0,
            irq_pending: false,
        })
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let count = self.prg_count_8k;
        match slot {
            0 => self.prg[0] as usize % count,
            1 => self.prg[1] as usize % count,
            2 => count.saturating_sub(2),
            _ => count - 1,
        }
    }
}

impl Mapper for Waixing253 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                let bank = self.prg_bank(slot);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0xB000..=0xE00C).contains(&addr) {
            let slot = ((((addr & 0x08) | (addr >> 8)) >> 3) as usize).wrapping_add(2) & 0x07;
            let shift = (addr & 0x04) as u8;
            let lo = (self.chr_low[slot] & (0xF0u8 >> shift)) | (value << shift);
            self.chr_low[slot] = lo;
            if slot == 0 {
                if lo == 0xC8 {
                    self.force_chr_rom = false;
                } else if lo == 0x88 {
                    self.force_chr_rom = true;
                }
            }
            if shift != 0 {
                self.chr_high[slot] = value >> 4;
            }
        } else {
            match addr {
                0x8010 => self.prg[0] = value,
                0xA010 => self.prg[1] = value,
                0x9400 => {
                    self.mirroring = match value & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
                0xF000 => {
                    self.irq_reload = (self.irq_reload & 0xF0) | (value & 0x0F);
                    self.irq_pending = false;
                }
                0xF004 => {
                    self.irq_reload = (self.irq_reload & 0x0F) | (value << 4);
                    self.irq_pending = false;
                }
                0xF008 => {
                    self.irq_counter = self.irq_reload;
                    self.irq_enabled = value & 0x02 != 0;
                    self.irq_scaler = 0;
                    self.irq_pending = false;
                }
                _ => {}
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) / CHR_BANK_1K;
                let lo = self.chr_low[slot];
                if (lo == 4 || lo == 5) && !self.force_chr_rom {
                    let page = (lo as usize & 0x01) * CHR_BANK_1K;
                    return self.chr_ram
                        [(page + (addr as usize & 0x3FF)) & (self.chr_ram.len() - 1)];
                }
                let page =
                    (lo as usize | ((self.chr_high[slot] as usize) << 8)) % self.chr_count_1k;
                self.chr[page * CHR_BANK_1K + (addr as usize & 0x3FF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) / CHR_BANK_1K;
                let lo = self.chr_low[slot];
                if (lo == 4 || lo == 5) && !self.force_chr_rom {
                    let page = (lo as usize & 0x01) * CHR_BANK_1K;
                    let off = (page + (addr as usize & 0x3FF)) & (self.chr_ram.len() - 1);
                    self.chr_ram[off] = value;
                } else if self.chr_is_ram {
                    // CHR-RAM variant: writes land in the banked CHR store
                    // (mirrors the `ppu_read` banked path). For a CHR-ROM cart
                    // this is a no-op (ROM is not writable).
                    let page =
                        (lo as usize | ((self.chr_high[slot] as usize) << 8)) % self.chr_count_1k;
                    self.chr[page * CHR_BANK_1K + (addr as usize & 0x3FF)] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if self.irq_enabled {
            self.irq_scaler += 1;
            if self.irq_scaler >= 114 {
                self.irq_scaler = 0;
                self.irq_counter = self.irq_counter.wrapping_add(1);
                if self.irq_counter == 0 {
                    self.irq_counter = self.irq_reload;
                    self.irq_pending = true;
                }
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        // CHR-RAM variant: the banked `self.chr` is mutable, so serialize it.
        let chr_ram_main = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(
            1 + Self::SAVE_LEN + self.vram.len() + self.chr_ram.len() + chr_ram_main,
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg);
        out.extend_from_slice(&self.chr_low);
        out.extend_from_slice(&self.chr_high);
        out.push(u8::from(self.force_chr_rom));
        out.push(self.irq_reload);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.extend_from_slice(&self.irq_scaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram_main = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + self.chr_ram.len() + chr_ram_main;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut c = 1;
        self.prg.copy_from_slice(&data[c..c + 2]);
        c += 2;
        self.chr_low.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.chr_high.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.force_chr_rom = data[c] != 0;
        self.irq_reload = data[c + 1];
        self.irq_counter = data[c + 2];
        self.irq_enabled = data[c + 3] != 0;
        self.irq_scaler = u16::from_le_bytes([data[c + 4], data[c + 5]]);
        self.irq_pending = data[c + 6] != 0;
        self.mirroring = byte_to_mirroring(data[c + 7], self.mirroring);
        c += 8;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[c..c + self.chr_ram.len()]);
        c += self.chr_ram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 253 (Waixing VRC4-clone, *Dragon Ball Z*).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG/CHR size.
pub fn new_m253(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Waixing253, MapperError> {
    Waixing253::new(prg_rom, chr_rom, mirroring)
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    /// 1 KiB-banked CHR: byte 0 of each 1 KiB bank holds the bank index.
    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

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

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m242_address_decoded_32k_and_mirror() {
        let mut m = Waixing242::new(synth_prg_32k(16), &[], Mirroring::Vertical).unwrap();
        // A = 0x8018: bank = (0x18>>3)&0x0F = 3; mirror = (A&2)==0 -> V.
        m.cpu_write(0x8018, 0);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // A = 0x801A: mirror bit set -> horizontal.
        m.cpu_write(0x801A, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m242_save_state_round_trip() {
        let mut m = Waixing242::new(synth_prg_32k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8028, 0); // bank 5
        m.ppu_write(0x0006, 0x44);
        let blob = m.save_state();
        let mut m2 = Waixing242::new(synth_prg_32k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0006), 0x44);
    }

    #[test]
    fn m162_regs_compose_prg() {
        let mut m = WaixingFs304M162::new(synth_prg_32k(64), &[], Mirroring::Vertical).unwrap();
        // Reset: all regs 0 -> A=$5300.2=0 -> A16=1, A15=$5100.1=0 -> bank #2.
        assert_eq!(m.cpu_read(0x8000), 2);
        // Waixing mode $5300=$04 ($5300.2=1): A16=$5000.1, A15=$5100.1.
        m.cpu_write(0x5300, 0x04);
        m.cpu_write(0x5000, 0x02); // $5000.1 = 1 -> A16 = 1 -> bank still has A16 set
        assert_eq!(m.cpu_read(0x8000), 2); // A16=1 -> bank 2
        m.cpu_write(0x5100, 0x02); // $5100.1 = 1 -> A15 = 1 -> bank 3
        assert_eq!(m.cpu_read(0x8000), 3);
        m.cpu_write(0x5000, 0x00); // $5000.1 = 0 -> A16 = 0; A15 still 1 -> bank 1
        assert_eq!(m.cpu_read(0x8000), 1);
        // A17/A18 from $5000 bits 2/3; A19/A20 from $5200 bits 0/1.
        m.cpu_write(0x5000, 0x0C); // bits 3,2 -> A18,A17 = 1,1 -> +12; A16=0,A15(=$5100.1)=1 -> 1
        m.cpu_write(0x5200, 0x03); // A20,A19 = 1,1 -> +48
        assert_eq!(m.cpu_read(0x8000), 1 + 12 + 48);
    }

    #[test]
    fn m162_save_state_round_trip() {
        let mut m = WaixingFs304M162::new(synth_prg_32k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 6);
        m.ppu_write(0x0012, 0x44);
        let blob = m.save_state();
        let mut m2 = WaixingFs304M162::new(synth_prg_32k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0012), 0x44);
    }

    #[test]
    fn m178_prg_mode_and_work_ram() {
        let mut m = Waixing178::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // UNROM mode (bits 1-2 = 01 -> $4800 = 0x02); inner reg1 = 3 -> base 3.
        m.cpu_write(0x4800, 0x02);
        m.cpu_write(0x4801, 0x03);
        m.cpu_write(0x4802, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3); // switchable 16 KiB at $8000
        assert_eq!(m.cpu_read(0xC000), 7); // UNROM: $C000 fixed to inner 111b
        // NROM-256 / BNROM (mode 0): 32 KiB pair from the even bank.
        m.cpu_write(0x4800, 0x00);
        m.cpu_write(0x4801, 0x02); // base 2 -> 32 KiB pair (2,3)
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
        // Work-RAM round trips.
        m.cpu_write(0x6000, 0x77);
        assert_eq!(m.cpu_read(0x6000), 0x77);
        // Mirroring: $4800 bit 0 (1 = horizontal).
        m.cpu_write(0x4800, 0x01);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x4800, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m178_save_state_round_trip() {
        let mut m = Waixing178::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x4800, 0x02);
        m.cpu_write(0x4801, 0x02);
        m.cpu_write(0x6010, 0x55);
        let blob = m.save_state();
        let mut m2 = Waixing178::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.cpu_read(0x6010), 0x55);
    }

    #[test]
    fn waixing253_prg_and_scaled_irq() {
        let mut m = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8010, 4); // prg[0] = 4
        m.cpu_write(0xA010, 6); // prg[1] = 6
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 6);
        assert_eq!(m.cpu_read(0xE000), 15); // fixed last.

        m.cpu_write(0xF000, 0x0E); // reload low
        m.cpu_write(0xF008, 0x02); // load + enable
        // counter loaded with 0x0E; needs (0x100-0x0E) ticks * 114.
        let mut fired = false;
        for _ in 0..(256 * 115) {
            m.notify_cpu_cycle();
            if m.irq_pending() {
                fired = true;
                break;
            }
        }
        assert!(fired);
    }

    #[test]
    fn waixing253_chr_ram_escape_and_round_trip() {
        let mut m = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        // CHR low reg value 4 on slot 0 + not force-rom -> CHR-RAM.
        m.cpu_write(0xB000, 0x04); // slot 0 low nibble = 4
        m.ppu_write(0x0000, 0x5E);
        assert_eq!(m.ppu_read(0x0000), 0x5E);
        let blob = m.save_state();
        let mut m2 = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), 0x5E);
    }

    #[test]
    fn waixing253_chr_ram_variant_writable_and_round_trips() {
        // No CHR-ROM => the banked `self.chr` is 8 KiB CHR-RAM and must be
        // writable through the normal banked path (regression: it was a
        // read-only 1 KiB stub that `ppu_write` never touched).
        let mut m = new_m253(synth_prg_8k(16), Box::new([]), Mirroring::Vertical).unwrap();
        // Default chr_low[0] == 0 -> banked CHR path (not the 4/5 escape).
        m.ppu_write(0x0000, 0xA5);
        m.ppu_write(0x0123, 0x3C);
        assert_eq!(m.ppu_read(0x0000), 0xA5);
        assert_eq!(m.ppu_read(0x0123), 0x3C);
        // The 8 KiB CHR-RAM must survive a save-state round trip.
        let blob = m.save_state();
        let mut m2 = new_m253(synth_prg_8k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), 0xA5);
        assert_eq!(m2.ppu_read(0x0123), 0x3C);
    }

    #[test]
    fn waixing253_chr_rom_not_writable() {
        // With CHR-ROM provided, `ppu_write` on the banked path is a no-op.
        let mut m = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        let before = m.ppu_read(0x0010);
        m.ppu_write(0x0010, before.wrapping_add(1));
        assert_eq!(m.ppu_read(0x0010), before, "CHR-ROM must not be mutable");
    }
}
