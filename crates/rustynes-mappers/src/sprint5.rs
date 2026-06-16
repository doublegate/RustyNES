//! Sprint 5 simple discrete-logic mappers (v1.2.0 "Curator" workstream A).
//!
//! A batch of small, well-documented pirate / unlicensed boards that share the
//! same shape as the stock discrete mappers (`NROM`, `CNROM`, `UxROM`,
//! `GxROM`, `AxROM`): a handful of bank-select latch registers, no IRQ, no
//! on-cart audio. Banking / mirroring semantics are cross-checked against the
//! `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`) and
//! the nesdev wiki.
//!
//! Boards implemented here:
//!
//! - **Mapper 38** (Bit Corp, `UNL-PCI556`): PRG/CHR latch at `$7000-$7FFF`.
//! - **Mapper 79** (`NINA-03`/`NINA-06`, AVE): PRG+CHR via `$4100-$5FFF`.
//! - **Mapper 113** (`NINA-006`/`MB-91` multicart): like 79 plus a mirroring bit.
//! - **Mapper 86** (Jaleco `JF-13`): PRG/CHR latch at `$6000-$6FFF`.
//! - **Mapper 140** (Jaleco `JF-11`/`JF-14`): PRG/CHR latch at `$6000-$7FFF`.
//! - **Mapper 41** (Caltron 6-in-1): outer register at `$6000-$67FF` + CHR-lo
//!   via `$8000-$FFFF` (with bus conflict).
//! - **Mapper 232** (Camerica Quattro / `BF9096`): two-level 16 KiB PRG banking.
//! - **Mapper 240** (C&E multicart): PRG/CHR via `$4020-$5FFF`.
//! - **Mapper 241** (`BxROM`-like pirate, e.g. "Mortal Kombat"): 32 KiB PRG bank
//!   via `$8000-$FFFF`, CHR-RAM.
//!
//! Mapper 11 (Color Dreams) is implemented in `sprint2.rs`; it is NOT redone here.

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
// Mapper 38 — Bit Corp UNL-PCI556.
//
// Single 8-bit latch at $7000-$7FFF. Low 2 bits select a 32 KiB PRG bank;
// bits 3-2 select an 8 KiB CHR bank. No bus conflicts (the register lives in
// the $6000-$7FFF window, not in PRG-ROM). Mirroring is header-fixed; no IRQ.
// ===========================================================================

/// Mapper 38 (Bit Corp `UNL-PCI556`).
pub struct Bitcorp38 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Bitcorp38 {
    /// Construct a new mapper 38 board.
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
                "mapper 38 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 38 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

impl Mapper for Bitcorp38 {
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
        if (0x7000..=0x7FFF).contains(&addr) {
            self.prg_bank = value & 0x03;
            self.chr_bank = (value >> 2) & 0x03;
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
// Mapper 79 — AVE NINA-03 / NINA-06.
//
// One register decoded across $4100-$5FFF: any address with A8 set
// (`addr & 0x0100 != 0`) latches the byte. CHR = data bits 0-2 (8 KiB),
// PRG = data bit 3 (32 KiB). CHR may be RAM. Mirroring is header-fixed; no IRQ.
// ===========================================================================

/// Mapper 79 (AVE `NINA-03`/`NINA-06`).
pub struct Nina0379 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Nina0379 {
    /// Construct a new mapper 79 board.
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
                "mapper 79 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
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
                "mapper 79 CHR-ROM size {} is not a multiple of 8 KiB",
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
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Nina0379 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The register window lives in $4100-$5FFF; reads there are open bus
    // (write-only registers), so the default `cpu_read_unmapped` is correct.

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
        // $4100-$5FFF, decoded on A8.
        if (0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0 {
            self.chr_bank = value & 0x07;
            self.prg_bank = (value >> 3) & 0x01;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
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

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
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
        self.prg_bank = data[1];
        self.chr_bank = data[2];
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
// Mapper 113 — NINA-006 / MB-91 multicart.
//
// Same $4100-$5FFF register window as mapper 79, but the bank layout differs
// and a mirroring bit is added:
//   data: M0pp pccc   (M = vertical mirroring, p = PRG bits, c = CHR bits)
//   PRG  = (data >> 3) & 0x07          (32 KiB)
//   CHR  = (data & 0x07) | ((data >> 3) & 0x08)   (8 KiB, 4-bit)
//   mirroring = bit 7 (1 = vertical, 0 = horizontal)
// CHR may be RAM. No IRQ.
// ===========================================================================

/// Mapper 113 (`NINA-006`/`MB-91` multicart).
pub struct Nina006M113 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    vertical_mirroring: bool,
}

impl Nina006M113 {
    /// Construct a new mapper 113 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 113 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
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
                "mapper 113 CHR-ROM size {} is not a multiple of 8 KiB",
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
            vertical_mirroring: false,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Nina006M113 {
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
        if (0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0 {
            self.prg_bank = (value >> 3) & 0x07;
            self.chr_bank = (value & 0x07) | ((value >> 3) & 0x08);
            self.vertical_mirroring = (value & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
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
                    self.chr[off] = value;
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
        if self.vertical_mirroring {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.vertical_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
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
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        self.vertical_mirroring = data[3] != 0;
        let mut cursor = 4;
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
// Mapper 86 — Jaleco JF-13.
//
// Single latch at $6000-$6FFF (writes to $7000-$7FFF address the on-cart
// sample-playback ADPCM, which we do not emulate). The latch byte VVdd_pPcc
// selects:
//   PRG = (value >> 4) & 0x03            (32 KiB)
//   CHR = (value & 0x03) | ((value >> 4) & 0x04)   (8 KiB, 3-bit)
// Mirroring is header-fixed; no IRQ.
// ===========================================================================

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

// ===========================================================================
// Mapper 41 — Caltron 6-in-1.
//
// Two registers:
//   $6000-$67FF (outer, decoded from ADDRESS bits, data ignored):
//      addr layout 0110 0xxx xxMC CEPP
//      PRG (32 KiB) = (E << 2) | PP   where E = A2, PP = A1..A0
//      outer CHR (high 2 bits of the 8 KiB bank) = A4..A3 (CC)
//      mirroring = A5 (M): 1 = horizontal, 0 = vertical
//      E also gates the inner CHR register: the inner write is honoured only
//      while E (= A2 of the last outer write) is set.
//   $8000-$FFFF (inner CHR, from DATA bits, WITH bus conflict):
//      inner CHR (low 2 bits) = data & 0x03
// 8 KiB CHR bank = (CC << 2) | cc. No IRQ.
// ===========================================================================

/// Mapper 41 (Caltron 6-in-1).
pub struct Caltron41 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    outer_chr: u8,
    inner_chr: u8,
    inner_enable: bool,
    horizontal_mirroring: bool,
}

impl Caltron41 {
    /// Construct a new Caltron 6-in-1 (mapper 41) board.
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
                "mapper 41 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 41 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        // The board's mirroring is runtime-controlled; seed from the header's
        // arrangement so the power-on state matches a sensible default.
        let horizontal_mirroring = mirroring == Mirroring::Horizontal;
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            outer_chr: 0,
            inner_chr: 0,
            inner_enable: false,
            horizontal_mirroring,
        })
    }

    const fn chr_bank(&self) -> u8 {
        (self.outer_chr << 2) | self.inner_chr
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank() as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }
}

impl Mapper for Caltron41 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The outer register sits in $6000-$67FF, which is "mapped" by the default
    // `cpu_read_unmapped` (>= $6000), so no override is needed there.

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x67FF => {
                // Outer register: decoded from address bits, data ignored.
                let e = ((addr >> 2) & 0x01) as u8;
                let pp = (addr & 0x03) as u8;
                self.prg_bank = (e << 2) | pp;
                self.inner_enable = e != 0;
                self.outer_chr = ((addr >> 3) & 0x03) as u8;
                self.horizontal_mirroring = ((addr >> 5) & 0x01) != 0;
            }
            0x8000..=0xFFFF if self.inner_enable => {
                // Inner CHR register has bus conflicts.
                let effective = value & self.read_prg(addr);
                self.inner_chr = effective & 0x03;
            }
            _ => {}
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
        let mut out = Vec::with_capacity(6 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.outer_chr);
        out.push(self.inner_chr);
        out.push(u8::from(self.inner_enable));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 6 + self.vram.len();
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
        self.outer_chr = data[2];
        self.inner_chr = data[3];
        self.inner_enable = data[4] != 0;
        self.horizontal_mirroring = data[5] != 0;
        self.vram.copy_from_slice(&data[6..6 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 232 — Camerica Quattro / BF9096.
//
// Two-level 16 KiB PRG banking, CHR-RAM:
//   $8000-$BFFF write: outer 64 KiB block = (data >> 3) & 0x03
//   $C000-$FFFF write: inner 16 KiB page within the block = data & 0x03
//   CPU $8000-$BFFF reads the selected inner page; CPU $C000-$FFFF is fixed
//   to page 3 of the selected 64 KiB block.
// Resolved 16 KiB bank = (outer << 2) | page. Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 232 (Camerica Quattro / `BF9096`).
pub struct Camerica232 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    outer_block: u8,
    inner_page: u8,
    mirroring: Mirroring,
}

impl Camerica232 {
    /// Construct a new mapper 232 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. CHR is always 8 KiB RAM (any supplied CHR-ROM is rejected).
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 232 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr: Box<[u8]> = if chr_rom.is_empty() {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() == CHR_BANK_8K {
            // Some dumps carry 8 KiB CHR-ROM; accept and use it read-only.
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 232 expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            outer_block: 0,
            inner_page: 0,
            mirroring,
        })
    }

    fn map_16k(&self, page_in_block: u8) -> usize {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = ((self.outer_block << 2) | (page_in_block & 0x03)) as usize;
        bank % count
    }
}

impl Mapper for Camerica232 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.map_16k(self.inner_page);
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000)]
            }
            0xC000..=0xFFFF => {
                let bank = self.map_16k(3);
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0xC000)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0xBFFF => self.outer_block = (value >> 3) & 0x03,
            0xC000..=0xFFFF => self.inner_page = value & 0x03,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                // CHR-RAM dumps allow writes; if CHR is the supplied 8 KiB
                // image we still let the program scribble its 8 KiB window.
                self.chr[addr as usize] = value;
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
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.outer_block);
        out.push(self.inner_page);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len() + self.chr.len();
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
        self.inner_page = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr
            .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 240 — C&E multicart.
//
// One register across $4020-$5FFF: byte DDDD_PPPP
//   PRG (32 KiB) = (data >> 4) & 0x0F
//   CHR (8 KiB)  = data & 0x0F
// The register window overlaps the normal WRAM range; many 240 boards have no
// PRG-RAM, so the register is the only thing wired at $4020-$5FFF. Mirroring is
// header-fixed; no IRQ.
// ===========================================================================

/// Mapper 240 (C&E multicart).
pub struct Cne240 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Cne240 {
    /// Construct a new mapper 240 board.
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
                "mapper 240 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 240 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

impl Mapper for Cne240 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The register window is write-only at $4020-$5FFF; reads there fall
    // through to open bus, so the default `cpu_read_unmapped` is correct.

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
        if (0x4020..=0x5FFF).contains(&addr) {
            self.prg_bank = (value >> 4) & 0x0F;
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

// ===========================================================================
// Mapper 241 — BxROM-like pirate ("Mortal Kombat" and friends).
//
// A single 32 KiB PRG bank selected by the whole byte written to $8000-$FFFF
// (no bus conflict; no register-bit masking beyond the modulo wrap). CHR is
// always 8 KiB RAM. Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 241 (`BxROM`-like pirate board).
pub struct Bxrom241 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Bxrom241 {
    /// Construct a new mapper 241 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 241 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() == CHR_BANK_8K {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 241 expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Bxrom241 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    self.chr[addr as usize] = value;
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
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(2 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 2 + self.vram.len() + chr_extra;
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
        let mut cursor = 2;
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

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    /// 32 KiB-banked PRG: byte 0 of each 32 KiB bank holds the bank index, the
    /// rest is 0xFF (so a bus-conflict AND at offset 0 is observable while
    /// other offsets are transparent).
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

    /// 8 KiB-banked CHR: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- Mapper 38 ---------------------------------------------------------

    #[test]
    fn m38_latch_selects_prg_and_chr() {
        let mut m = Bitcorp38::new(synth_prg_32k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // value 0b0000_1110: PRG = 0b10 = 2, CHR = 0b11 = 3.
        m.cpu_write(0x7123, 0b0000_1110);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.ppu_read(0x0000), 3);
        // Writes outside $7000-$7FFF are ignored.
        m.cpu_write(0x6000, 0xFF);
        assert_eq!(m.cpu_read(0x8000), 2);
    }

    #[test]
    fn m38_save_state_round_trip() {
        let mut m = Bitcorp38::new(synth_prg_32k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x7000, 0b0000_1101); // PRG 1, CHR 3
        let blob = m.save_state();
        let mut m2 =
            Bitcorp38::new(synth_prg_32k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 1);
        assert_eq!(m2.ppu_read(0x0000), 3);
    }

    // --- Mapper 79 ---------------------------------------------------------

    #[test]
    fn m79_register_decodes_on_a8() {
        let mut m =
            Nina0379::new(synth_prg_32k(2), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        // $4100 has A8 set: value 0b0000_1101 -> CHR = 0b101 = 5, PRG = bit3 = 1.
        m.cpu_write(0x4100, 0b0000_1101);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn m79_no_decode_without_a8() {
        let mut m =
            Nina0379::new(synth_prg_32k(2), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        // $4000-class addresses are not in $4100-$5FFF; and an in-range addr
        // with A8 clear ($4200) must NOT latch.
        m.cpu_write(0x4200, 0b0000_1111);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    // --- Mapper 113 --------------------------------------------------------

    #[test]
    fn m113_decodes_prg_chr_and_mirroring() {
        let mut m = Nina006M113::new(synth_prg_32k(4), synth_chr_8k(16)).unwrap();
        // value 0b1100_0010 (0xC2):
        //   PRG = (v>>3)&7 = (24)&7 = 0
        //   CHR = (v&7) | ((v>>3)&8) = 2 | (24 & 8 = 8) = 10
        //   mirroring bit7 set -> vertical.
        m.cpu_write(0x4100, 0b1100_0010);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 10);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // Clear bit 7 -> horizontal.
        m.cpu_write(0x4100, 0b0000_0001);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    // --- Mapper 86 ---------------------------------------------------------

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

    // --- Mapper 140 --------------------------------------------------------

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

    // --- Mapper 41 ---------------------------------------------------------

    #[test]
    fn m41_outer_register_decodes_from_address() {
        let mut m =
            Caltron41::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // addr layout 0110 0xxx xxMC CEPP.
        // Pick $6000 | (M=1<<5) | (CC=0b11<<3) | (E=1<<2) | (PP=0b10).
        // => A5=1 (horizontal), A4..3 = 0b11 (outer CHR 3), A2 = 1 (E set),
        //    A1..0 = 0b10. PRG = (E<<2)|PP = 0b110 = 6.
        let addr = 0x6000 | (1 << 5) | (0b11 << 3) | (1 << 2) | 0b10;
        m.cpu_write(addr, 0x00);
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Inner CHR write (E set, so honoured). PRG bytes are 0xFF except at
        // offset 0; write to $8001 (byte 0xFF) -> no conflict masking.
        m.cpu_write(0x8001, 0b01); // inner CHR low = 1
        // CHR bank = (outer 3 << 2) | inner 1 = 13.
        assert_eq!(m.ppu_read(0x0000), 13);
    }

    #[test]
    fn m41_inner_write_gated_by_enable() {
        let mut m =
            Caltron41::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // Outer write with E clear (A2 = 0): PP = 0, outer CHR = 1, E = 0.
        let addr = 0x6000 | (0b01 << 3); // CC = 1, E = 0, PP = 0
        m.cpu_write(addr, 0x00);
        // Inner write must be ignored while disabled.
        m.cpu_write(0x8001, 0b11);
        // CHR bank = (outer 1 << 2) | inner 0 = 4.
        assert_eq!(m.ppu_read(0x0000), 4);
    }

    #[test]
    fn m41_inner_chr_has_bus_conflict() {
        // PRG byte at offset 0 of bank 0 is the bank index (0). Writing the
        // inner register at $8000 ANDs with that 0 -> inner CHR forced to 0.
        let mut m =
            Caltron41::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // Enable inner (E set), outer CHR 0, PRG bank 0 wraps so offset-0 byte
        // is 0 (the bank index marker).
        let addr = 0x6000 | (1 << 2); // E = 1, everything else 0 -> PRG bank 4
        m.cpu_write(addr, 0x00);
        // PRG bank is now 4 (E<<2). Offset 0 of bank 4 holds value 4.
        // Write inner at $8000: data 0b11 AND prg_byte(4 = 0b100) = 0b00.
        m.cpu_write(0x8000, 0b11);
        assert_eq!(m.ppu_read(0x0000), 0); // outer 0, inner masked to 0
    }

    // --- Mapper 232 --------------------------------------------------------

    #[test]
    fn m232_two_level_banking() {
        // 8 16 KiB banks = 2 blocks of 4 pages.
        let mut m = Camerica232::new(synth_prg_16k(8), Box::new([]), Mirroring::Vertical).unwrap();
        // Select outer block 1 ($8000 write, bits 4-3 = 0b01 << 3 = 0b1000).
        m.cpu_write(0x8000, 0b0000_1000);
        // Select inner page 2 ($C000 write, bits 1-0).
        m.cpu_write(0xC000, 0b0000_0010);
        // $8000-$BFFF reads bank (1<<2)|2 = 6.
        assert_eq!(m.cpu_read(0x8000), 6);
        // $C000-$FFFF is fixed to page 3 of block 1 -> bank (1<<2)|3 = 7.
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    // --- Mapper 240 --------------------------------------------------------

    #[test]
    fn m240_register_in_4020_5fff() {
        let mut m = Cne240::new(synth_prg_32k(4), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // value DDDD_PPPP: PRG = (v>>4)&0xF, CHR = v&0xF.
        m.cpu_write(0x5000, 0b0011_1010); // PRG 3, CHR 10
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.ppu_read(0x0000), 10);
        // $4020 is the bottom of the register window.
        m.cpu_write(0x4020, 0b0001_0101); // PRG 1, CHR 5
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
        // Reads in the register window fall through to open bus.
        assert!(m.cpu_read_unmapped(0x5000));
    }

    // --- Mapper 241 --------------------------------------------------------

    #[test]
    fn m241_full_byte_selects_32k_prg() {
        let mut m = Bxrom241::new(synth_prg_32k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        // No bus conflict: the written value sticks even though offset 0 of the
        // landing bank is not 0xFF.
        m.cpu_write(0xFFFF, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn m241_chr_ram_round_trip() {
        let mut m = Bxrom241::new(synth_prg_32k(2), Box::new([]), Mirroring::Vertical).unwrap();
        m.ppu_write(0x0010, 0xAB);
        assert_eq!(m.ppu_read(0x0010), 0xAB);
    }
}
