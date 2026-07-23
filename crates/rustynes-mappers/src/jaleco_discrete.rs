//! Jaleco `JF-13` (mapper 86) and `JF-11` / `JF-14` (mapper 140) -- discrete
//! PRG/CHR latch boards.
//!
//! Both put their single bank-select latch in the PRG-RAM window rather than
//! at `$8000`: `$6000-$6FFF` for mapper 86, the full `$6000-$7FFF` for
//! mapper 140.
//!
//! Jaleco's larger ASICs live in `m087_jaleco87.rs` and `m018_jaleco_ss88006.rs`.
//!
//! A discrete-logic board in the shape of the stock mappers (`NROM`, `CNROM`,
//! `UxROM`, `GxROM`, `AxROM`): bank-select latch registers, no IRQ, no on-cart
//! audio. Banking / mirroring semantics are cross-checked against the
//! `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, and validated by register-decode + save-state unit
//! tests.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

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
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Mapper 86 (Jaleco `JF-13`).
pub struct Jaleco86 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Jaleco86 {
    /// Construct a new mapper 86 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 86 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 86 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Jaleco86 {
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

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // Bank register at $6000-$6FFF only; $7000-$7FFF is the ADPCM port.
        if (0x6000..=0x6FFF).contains(&addr) {
            self.prg_bank = (value >> 4) & 0x03;
            self.chr_bank = (value & 0x03) | ((value >> 4) & 0x04);
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
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
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
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 140 — Jaleco JF-11 / JF-14.
//
// Single latch across the whole $6000-$7FFF window. The byte PPPP_CCCC selects:
//   PRG = (value >> 4) & 0x03   (32 KiB)
//   CHR = value & 0x0F          (8 KiB, 4-bit)
// Mirroring is header-fixed; no IRQ.
// ===========================================================================

/// Mapper 140 (Jaleco `JF-11`/`JF-14`).
pub struct Jaleco140 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Jaleco140 {
    /// Construct a new mapper 140 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 140 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 140 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Jaleco140 {
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

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_bank = (value >> 4) & 0x03;
            self.chr_bank = value & 0x0F;
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
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
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
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

/// Shared register/strobe state for the Jaleco JF-17/19 family (mappers 72/92).
// The four flags each model a distinct hardware signal (CHR-RAM presence, the
// two edge-triggered latch strobes, and the JF-17-vs-JF-19 PRG layout); they
// are not a bitfield-able state and reading them as named bools is clearest.
#[allow(clippy::struct_excessive_bools)]
struct JalecoLatch {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    prev_prg_strobe: bool,
    prev_chr_strobe: bool,
    mirroring: Mirroring,
    /// Mask applied to the PRG-bank nibble (0x0F for 72, 0x1F for 92).
    prg_field_mask: u8,
    /// PRG window layout. `false` (mapper 72, JF-17): switchable bank at
    /// `$8000-$BFFF`, fixed LAST bank at `$C000-$FFFF`. `true` (mapper 92,
    /// JF-19): fixed FIRST bank at `$8000-$BFFF`, switchable bank at
    /// `$C000-$FFFF` — the reset vector lives in the fixed half, so this layout
    /// is load-bearing for boot.
    switchable_high: bool,
}

impl JalecoLatch {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        prg_field_mask: u8,
        switchable_high: bool,
        id: u16,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper {id} PRG-ROM size {} is not a non-zero multiple of 16 KiB",
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
                "mapper {id} CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            prev_prg_strobe: false,
            prev_chr_strobe: false,
            mirroring,
            prg_field_mask,
            switchable_high,
        })
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn read_prg_bank(&self, bank: usize, off: usize) -> u8 {
        let bank = bank % self.prg_count_16k();
        self.prg_rom[bank * PRG_BANK_16K + off]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }

    fn cpu_read(&self, addr: u16) -> u8 {
        let last = self.prg_count_16k() - 1;
        match addr {
            0x8000..=0xBFFF => {
                // JF-19 (mapper 92): fixed FIRST bank here. JF-17 (mapper 72):
                // switchable bank here.
                let bank = if self.switchable_high {
                    0
                } else {
                    self.prg_bank as usize
                };
                self.read_prg_bank(bank, addr as usize - 0x8000)
            }
            0xC000..=0xFFFF => {
                // JF-19: switchable bank here. JF-17: fixed LAST bank here.
                let bank = if self.switchable_high {
                    self.prg_bank as usize
                } else {
                    last
                };
                self.read_prg_bank(bank, addr as usize - 0xC000)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND the written byte with the underlying PRG byte.
            let effective = value & self.cpu_read(addr);
            let prg_strobe = (effective & 0x80) != 0;
            let chr_strobe = (effective & 0x40) != 0;
            if prg_strobe && !self.prev_prg_strobe {
                self.prg_bank = effective & self.prg_field_mask;
            }
            if chr_strobe && !self.prev_chr_strobe {
                self.chr_bank = effective & 0x0F;
            }
            self.prev_prg_strobe = prg_strobe;
            self.prev_chr_strobe = chr_strobe;
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
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
                    self.chr[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(5 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.prev_prg_strobe));
        out.push(u8::from(self.prev_chr_strobe));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 5 + self.vram.len() + chr_extra;
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
        self.prev_prg_strobe = data[3] != 0;
        self.prev_chr_strobe = data[4] != 0;
        let mut cursor = 5;
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

/// Mapper 72 (Jaleco `JF-17`/`JF-19`).
pub struct Jaleco72 {
    inner: JalecoLatch,
}

impl Jaleco72 {
    /// Construct a new mapper 72 board.
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
        Ok(Self {
            inner: JalecoLatch::new(prg_rom, chr_rom, mirroring, 0x0F, false, 72)?,
        })
    }
}

impl Mapper for Jaleco72 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.inner.ppu_read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.inner.ppu_write(addr, value);
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        self.inner.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        self.inner.load_state(data)
    }
}

/// Mapper 92 (Jaleco `JF-19`-variant — like 72 with a 5-bit PRG field).
pub struct Jaleco92 {
    inner: JalecoLatch,
}

impl Jaleco92 {
    /// Construct a new mapper 92 board.
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
        Ok(Self {
            inner: JalecoLatch::new(prg_rom, chr_rom, mirroring, 0x1F, true, 92)?,
        })
    }
}

impl Mapper for Jaleco92 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.inner.cpu_read(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.inner.cpu_write(addr, value);
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.inner.ppu_read(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.inner.ppu_write(addr, value);
    }

    fn current_mirroring(&self) -> Mirroring {
        self.inner.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        self.inner.save_state()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        self.inner.load_state(data)
    }
}

/// Mapper 101 (Jaleco `JF-10` CHR latch).
pub struct Jaleco101 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Jaleco101 {
    /// Construct a new mapper 101 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 101 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 101 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Jaleco101 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_rom[addr as usize - 0x8000]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // The CHR latch is in the PRG-RAM ($6000-$7FFF) window.
        if (0x6000..=0x7FFF).contains(&addr) {
            self.chr_bank = value;
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
        out.push(self.chr_bank);
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
        self.chr_bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 16 KiB-banked PRG: byte 0 of each 16 KiB bank holds the bank index.
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
    fn m86_latch_selects_prg_and_chr() {
        let mut m = Jaleco86::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // value VVdd_pPcc; PRG = (v>>4)&3, CHR = (v&3) | ((v>>4)&4).
        // 0b0011_0010: PRG = (0b0011_0010>>4)&3 = 0b11&3 = 3.
        //              CHR = (0b10) | ((0b0011)&4 -> 0) = 2.
        m.cpu_write(0x6000, 0b0011_0010);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.ppu_read(0x0000), 2);
        // High CHR bit: value 0b0100_0001 -> CHR = 1 | ((0b0100)&4 = 4) = 5.
        m.cpu_write(0x6000, 0b0100_0001);
        assert_eq!(m.ppu_read(0x0000), 5);
        // $7000 (ADPCM port) must NOT change banking.
        m.cpu_write(0x7000, 0xFF);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn m140_latch_selects_prg_and_chr() {
        let mut m =
            Jaleco140::new(synth_prg_32k(4), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // value PPPP_CCCC -> PRG = (v>>4)&3, CHR = v&0x0F.
        m.cpu_write(0x6FFF, 0b0010_1010); // PRG 2, CHR 10
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.ppu_read(0x0000), 10);
        // $7FFF is still in the $6000-$7FFF window.
        m.cpu_write(0x7FFF, 0b0001_0011); // PRG 1, CHR 3
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn m72_strobe_latches_on_rising_edge() {
        let mut m = Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // PRG window offsets >0 hold 0xFF, so bus conflict is transparent there.
        // Write to $8001 (byte 0xFF). PRG strobe (bit7) rising + bank 3.
        m.cpu_write(0x8001, 0b1000_0011);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Last 16 KiB bank fixed at $C000: bank 7.
        assert_eq!(m.cpu_read(0xC000), 7);
        // CHR strobe (bit6) rising + bank 5.
        m.cpu_write(0x8001, 0b0100_0101);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn m72_no_relatch_without_falling_edge() {
        let mut m = Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8001, 0b1000_0011); // latch PRG 3
        assert_eq!(m.cpu_read(0x8000), 3);
        // Strobe still high, new bank value -> must NOT re-latch.
        m.cpu_write(0x8001, 0b1000_0101);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Drop strobe, then raise again -> re-latches.
        m.cpu_write(0x8001, 0b0000_0000);
        m.cpu_write(0x8001, 0b1000_0101);
        assert_eq!(m.cpu_read(0x8000), 5);
    }

    #[test]
    fn m92_uses_5bit_prg_field() {
        let mut m =
            Jaleco92::new(synth_prg_16k(32), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // 5-bit PRG field: value 0b1001_0001 -> strobe + bank 0x11 = 17.
        m.cpu_write(0x8001, 0b1001_0001);
        // JF-19 layout: $8000 is the FIXED first bank (0); the switchable bank
        // appears at $C000.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 17);
    }

    #[test]
    fn m72_save_state_round_trips_strobe_state() {
        let mut j = Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        j.cpu_write(0x8001, 0b1100_0011); // PRG 3 + CHR 3, both strobes high
        let blob = j.save_state();
        let mut j2 =
            Jaleco72::new(synth_prg_16k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        j2.load_state(&blob).unwrap();
        assert_eq!(j2.cpu_read(0x8000), 3);
        assert_eq!(j2.ppu_read(0x0000), 3);
        // Strobe still high after restore -> a same-value write must not relatch
        // from a fresh edge.
        j2.cpu_write(0x8001, 0b1100_0101);
        assert_eq!(j2.cpu_read(0x8000), 3);
    }

    #[test]
    fn m101_chr_latch_in_6000_window() {
        let mut m =
            Jaleco101::new(synth_prg_32k(1), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x6000, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // PRG is fixed.
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m101_save_state_round_trip() {
        let mut m =
            Jaleco101::new(synth_prg_32k(1), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x7FFF, 6);
        let blob = m.save_state();
        let mut m2 =
            Jaleco101::new(synth_prg_32k(1), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), 6);
    }
}
