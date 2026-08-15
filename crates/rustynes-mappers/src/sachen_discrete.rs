// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provenance: the discrete Sachen / Txc boards are derived from Mesen2 (GPL-3.0-or-later), `Sachen/Sachen8259.h` / `Txc/TxcChip.h`. See docs/originality-and-provenance.md (Section 1)
// and NOTICE for the complete, audited derivation record.
//! Sachen discrete boards addressed in the `$4100-$5FFF` expansion window:
//! mappers 133, 145 and 146.
//!
//! Sachen's unlicensed boards consistently decode on address line A8 rather
//! than in the `$8000-$FFFF` ROM window -- a way of avoiding bus conflicts
//! without gating logic, since nothing else drives the bus down there. The
//! three differ only in which bits of the written byte become the CHR select:
//! mapper 145 takes bit 7 alone, and mapper 146 is electrically the same board
//! as AVE's `NINA-03` (mapper 79), which is why its decode mirrors
//! `ave_nina.rs`.
//!
//! Sachen's later 8259 ASIC family is a different design; see
//! `sachen_8259.rs`.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the `GeraNES` reference emulator (cross-referenced, not copied)
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

/// Mapper 133 (Sachen 3009).
pub struct Sachen133 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen133 {
    /// Construct a new mapper 133 board.
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
                "mapper 133 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 133 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

impl Mapper for Sachen133 {
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
            self.prg_bank = (value >> 2) & 0x01;
            self.chr_bank = value & 0x03;
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
// Mapper 145 — Sachen SA-72007.
//
// A single CHR-bank bit (the high data bit) is decoded when the address
// satisfies (absolute & 0x4100) == 0x4100, in BOTH the $4100 register window
// and the $6000 save-RAM window:
//   CHR (8 KiB) = (value >> 7) & 0x01
// PRG is a fixed 32 KiB (bank 0). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 145 (Sachen `SA-72007`).
pub struct Sachen145 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen145 {
    /// Construct a new mapper 145 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is empty / not a multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        // Real SA-72007 dumps (e.g. "Sidewinder") are 16 KiB PRG / NROM-128-style
        // — the fixed bank is simply mirrored across the 32 KiB CPU window. Accept
        // any non-zero 16 KiB multiple (16 KiB mirrors; 32 KiB maps 1:1).
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 145 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 145 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

impl Mapper for Sachen145 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Fixed bank 0, mirrored across the 32 KiB window for sub-32 KiB PRG.
            self.prg_rom[(addr as usize - 0x8000) % self.prg_rom.len()]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // CHR bank decoded when (addr & 0x4100) == 0x4100 in both the register
        // ($4100-$5FFF) and save-RAM ($6000-$7FFF) windows.
        if (0x4100..=0x7FFF).contains(&addr) && (addr & 0x4100) == 0x4100 {
            self.chr_bank = (value >> 7) & 0x01;
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

// ===========================================================================
// Mapper 146 — Sachen (mapper-79-equivalent behaviour).
//
// Identical decode to NINA-03 (mapper 79) but Sachen wired the register into
// the $4100-$5FFF window decoded on A8 AND aliased into the $6000-$7FFF
// save-RAM window (offset by $2000). The byte selects:
//   PRG (32 KiB) = (value >> 3) & 0x01
//   CHR (8 KiB)  =  value       & 0x07
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 146 (Sachen, `NINA-03`/mapper-79-equivalent behaviour).
pub struct Sachen146 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen146 {
    /// Construct a new mapper 146 board.
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
                "mapper 146 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 146 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

    const fn apply(&mut self, value: u8) {
        self.prg_bank = (value >> 3) & 0x01;
        self.chr_bank = value & 0x07;
    }
}

impl Mapper for Sachen146 {
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
        // $4100-$5FFF on A8, and the $6000-$7FFF save-RAM alias.
        if ((0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0)
            || (0x6000..=0x7FFF).contains(&addr)
        {
            self.apply(value);
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

/// The TXC JV001 scrambling-accumulator chip (mapper 147). Distinct from the
/// non-JV001 `TxcChip` in `txc.rs` (different register/output bit positions).
/// The JV001 pre/post-scramble is a fixed hardware bit-permutation. Provenance:
/// derived from puNES's `JV001.c` / `mapper_147.c` (GPL-2.0-or-later) — the
/// bit-permutation is also documented in the nesdev wiki mapper-147 board notes.
/// See NOTICE and docs/originality-and-provenance.md (Section 1).
#[derive(Clone, Copy)]
struct Jv001Chip {
    accumulator: u8,
    inverter: u8,
    staging: u8,
    output: u8,
    increase: bool,
    /// Full 0x00/0xFF mask (NOT a bool) — puNES stores `0xFF * (value & 1)` and
    /// hard-resets it to 0xFF, so the very first handshake read inverts.
    invert: u8,
}

impl Default for Jv001Chip {
    fn default() -> Self {
        Self {
            accumulator: 0,
            inverter: 0,
            staging: 0,
            output: 0,
            increase: false,
            // Hard-reset state (puNES init_JV001): invert latched high.
            invert: 0xFF,
        }
    }
}

impl Jv001Chip {
    /// The value the chip returns on a $4100 read (the protection handshake).
    /// puNES: `((inverter ^ invert) & 0xF0) | (accumulator & 0x0F)`.
    const fn read(self) -> u8 {
        ((self.inverter ^ self.invert) & 0xF0) | (self.accumulator & 0x0F)
    }

    /// `absolute` is the full CPU address; `value` the written byte (already
    /// mapper-147-pre-scrambled by the caller). Mirrors puNES
    /// `extcl_cpu_wr_mem_JV001`.
    const fn write(&mut self, absolute: u16, value: u8) {
        if absolute < 0x8000 {
            match absolute & 0x0103 {
                0x0100 => {
                    self.accumulator = if self.increase {
                        self.accumulator.wrapping_add(1)
                    } else {
                        (self.accumulator & 0xF0) | ((self.staging ^ self.invert) & 0x0F)
                    };
                }
                0x0101 => self.invert = if value & 0x01 != 0 { 0xFF } else { 0x00 },
                0x0102 => {
                    self.staging = value & 0x0F;
                    self.inverter = value & 0xF0;
                }
                0x0103 => self.increase = (value & 0x01) != 0,
                _ => {}
            }
        } else {
            // A $8000-$FFFF access refreshes the bank-output latch.
            self.output = (self.inverter & 0xF0) | (self.accumulator & 0x0F);
        }
    }
}

/// Mapper 147 (Sachen 3018 / TXC `JV001`).
pub struct Sachen3018M147 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    jv001: Jv001Chip,
    mirroring: Mirroring,
}

impl Sachen3018M147 {
    /// Construct a new mapper 147 board.
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
                "mapper 147 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
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
                "mapper 147 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            jv001: Jv001Chip::default(),
            mirroring,
        })
    }

    /// PRG 32 KiB bank from the chip output latch (puNES `prg_fix_jv001_147`).
    const fn prg_bank(&self) -> usize {
        (((self.jv001.output & 0x20) >> 4) | (self.jv001.output & 0x01)) as usize
    }

    /// CHR 8 KiB bank from the chip output latch (puNES `chr_fix_jv001_147`).
    const fn chr_bank(&self) -> usize {
        ((self.jv001.output & 0x1E) >> 1) as usize
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = self.prg_bank() % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = self.chr_bank() % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Sachen3018M147 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The JV001 protection register answers reads at $4100 (decoded on
        // A0/A1 == 0). Everything else in $4020-$5FFF is open bus.
        !((0x4100..=0x5FFF).contains(&addr) && (addr & 0x0103) == 0x0100)
            && (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // JV001 protection handshake read. The mapper-147 board post-
            // scrambles the chip read: ((v & 0x3F) << 2) | ((v & 0xC0) >> 6)
            // (puNES extcl_cpu_rd_mem_147).
            0x4100..=0x5FFF if (addr & 0x0103) == 0x0100 => {
                let v = self.jv001.read();
                ((v & 0x3F) << 2) | ((v & 0xC0) >> 6)
            }
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // The mapper-147 board pre-scrambles every write to the JV001:
        // ((value & 0x03) << 6) | ((value & 0xFC) >> 2) (puNES
        // extcl_cpu_wr_mem_147).
        let scramble = |v: u8| ((v & 0x03) << 6) | ((v & 0xFC) >> 2);
        match addr {
            0x4100..=0x5FFF => self.jv001.write(addr, scramble(value)),
            0x8000..=0xFFFF => {
                // Bus conflict in the PRG window; the write refreshes the latch.
                let effective = value & self.read_prg(addr);
                self.jv001.write(addr, scramble(effective));
            }
            _ => {}
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
        // 6 JV001 fields (accumulator, inverter, staging, output, increase, invert).
        let mut out = Vec::with_capacity(7 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.jv001.accumulator);
        out.push(self.jv001.inverter);
        out.push(self.jv001.staging);
        out.push(self.jv001.output);
        out.push(u8::from(self.jv001.increase));
        out.push(self.jv001.invert); // full 0x00/0xFF mask
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
        let expected = 7 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.jv001.accumulator = data[1];
        self.jv001.inverter = data[2];
        self.jv001.staging = data[3];
        self.jv001.output = data[4];
        self.jv001.increase = data[5] != 0;
        self.jv001.invert = data[6]; // full 0x00/0xFF mask
        let mut cursor = 7;
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
// Mapper 148 — Sachen SA-008-A / Tengen 800008.
//
// The mapper-79 bit layout (`.... PCCC`: CHR = bits 0-2, PRG = bit 3) moved
// into the $8000-$FFFF window, introducing bus conflicts:
//   PRG (32 KiB) = (value >> 3) & 0x01
//   CHR (8 KiB)  = value & 0x07
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 148 (Sachen `SA-008-A` / Tengen 800008).
pub struct Sachen148 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen148 {
    /// Construct a new mapper 148 board.
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
                "mapper 148 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
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
                "mapper 148 CHR-ROM size {} is not a multiple of 8 KiB",
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

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Sachen148 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict.
            let effective = value & self.read_prg(addr);
            self.prg_bank = (effective >> 3) & 0x01;
            self.chr_bank = effective & 0x07;
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
// Mapper 149 — Sachen SA-0036.
//
// CNROM-like: fixed 32 KiB PRG, switchable 8 KiB CHR. The CHR bank is a single
// bit in bit 7 of the value written to $8000-$FFFF, with bus conflicts:
//   CHR (8 KiB) = (value >> 7) & 0x01
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 149 (Sachen `SA-0036`).
pub struct Sachen149 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen149 {
    /// Construct a new mapper 149 board.
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
                "mapper 149 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 149 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

    fn read_prg(&self, addr: u16) -> u8 {
        // Fixed first 32 KiB bank.
        self.prg_rom[addr as usize - 0x8000]
    }
}

impl Mapper for Sachen149 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict.
            let effective = value & self.read_prg(addr);
            self.chr_bank = (effective >> 7) & 0x01;
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

// ===========================================================================
// Mapper 150 — Sachen SA-015 / SA-630 (UNL-Sachen-74LS374N).
//
// An eight-register ASIC at $4100 (register index, write) / $4101
// (register data, read+write). Both decode on the $C101 mask: A8 selects
// index ($4100) vs. data ($4101). Each register holds 3 bits and is fully
// readable (Shogi Gakuen checks this as protection). Banking is derived from
// the registers:
//   PRG (32 KiB) = reg[5] & 0x03
//   CHR (8 KiB)  = ((reg[4] & 0x01) << 2) | (reg[6] & 0x03)
//   mirroring (reg[7] >> 1) & 0x03:
//       0: custom S0-S0-S0-S1 (lower-right unique)
//       1: Horizontal
//       2: Vertical
//       3: Single-screen A
// Reads at $4101 return (open_bus & 0xF8) | (reg[index] & 0x07); we approximate
// open bus with 0 (the protected program only inspects the low 3 bits).
// Writes are also accepted via the $6000-$7FFF mirror (addr | 0x1000). No IRQ.
// ===========================================================================

/// Mapper 150 (Sachen `SA-015`/`SA-630`, `UNL-Sachen-74LS374N`).
/// Which PCB the SA-020A ASIC is wired into.
///
/// The chip is the same eight-register part in both cases -- the NESdev wiki
/// records this explicitly under mapper 243's Errata: "The SA-150 PCB, denoted
/// by INES Mapper 150, connects the same ASIC differently, changing the meaning
/// of the CHR-bank-related register bits." Only the bank decode differs; the
/// register file, the $4100/$4101 index/data protocol, the readback, and the
/// four mirroring modes are shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sa020aBoard {
    /// SA-150 PCB (iNES mapper 150).
    Sa150,
    /// SA-020A PCB (iNES mapper 243) -- Honey Peach / Mei Nu Quan (SA-006).
    Sa020a,
}

impl Sa020aBoard {
    /// The iNES mapper number this PCB is denoted by (used in error text).
    const fn ines_id(self) -> u16 {
        match self {
            Self::Sa150 => 150,
            Self::Sa020a => 243,
        }
    }
}

/// Sachen SA-020A eight-register ASIC (iNES mappers **150 and 243**).
///
/// A fake-"74LS374N"-marked part with eight three-bit registers reached through
/// an index/data pair at `$4100`/`$4101` (mask `$C101`), driving one switchable
/// 32 KiB PRG bank, one switchable 8 KiB CHR bank, and four mirroring modes --
/// one of which is the non-standard S0-S0-S0-S1 layout.
///
/// The type is named for mapper 150 for historical reasons but backs both PCBs
/// the ASIC shipped on; see [`Sa020aBoard`]. Construct via [`Sachen150::new`]
/// for the SA-150 or [`Sachen150::new_on_board`] to pick explicitly.
pub struct Sachen150 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    current_register: u8,
    reg: [u8; 8],
    board: Sa020aBoard,
}

impl Sachen150 {
    /// Construct a new mapper 150 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        Self::new_on_board(prg_rom, chr_rom, Sa020aBoard::Sa150)
    }

    /// Construct the SA-020A ASIC on an explicit PCB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new_on_board(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        board: Sa020aBoard,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper {} PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                board.ines_id(),
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
                "mapper {} CHR-ROM size {} is not a multiple of 8 KiB",
                board.ines_id(),
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            current_register: 0,
            reg: [0u8; 8],
            board,
        })
    }

    // puNES prg_fix_150 / chr_fix_150 (the non-243 branch):
    //   PRG 32 KiB = reg[5] | (reg[2] & 0x01)
    //   CHR  8 KiB = (reg[2] << 3) | ((reg[4] & 0x01) << 2) | (reg[6] & 0x03)
    // The old decode masked PRG to reg[5] & 0x03 (dropping reg[2].0) and omitted
    // the reg[2]<<3 CHR term, so both banks resolved wrong -> blank/garbled.
    const fn prg_bank(&self) -> u8 {
        match self.board {
            Sa020aBoard::Sa150 => self.reg[5] | (self.reg[2] & 0x01),
            // Mapper 243: R5 supplies PRG A16..A15 and nothing else does, so the
            // 32 KiB bank is just those two bits -- 4 banks, the documented
            // 128 KiB maximum. R2 does NOT contribute here; that is one half of
            // what "connects the same ASIC differently" means.
            Sa020aBoard::Sa020a => self.reg[5] & 0x03,
        }
    }

    const fn chr_bank(&self) -> u8 {
        match self.board {
            Sa020aBoard::Sa150 => {
                ((self.reg[2] & 0x01) << 3) | ((self.reg[4] & 0x01) << 2) | (self.reg[6] & 0x03)
            }
            // Mapper 243 assembles the 8 KiB CHR bank from, low to high:
            // A13 = R2.0, A14 = R4.0, A15 = R6.0, A16 = R6.1. Four bits, 16
            // banks, the documented 128 KiB maximum. Note this is the SAME three
            // registers as the SA-150 above but at inverted significance -- R2
            // is the LSB here and the MSB there, which is exactly why a ROM for
            // one board renders garbage on the other.
            Sa020aBoard::Sa020a => {
                (self.reg[2] & 0x01) | ((self.reg[4] & 0x01) << 1) | ((self.reg[6] & 0x03) << 2)
            }
        }
    }

    /// Mirroring selector value `(reg[7] >> 1) & 0x03`.
    const fn mirror_sel(&self) -> u8 {
        (self.reg[7] >> 1) & 0x03
    }

    const fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x0101 {
            0x0100 => self.current_register = value & 0x07,
            0x0101 => self.reg[(self.current_register & 0x07) as usize] = value & 0x07,
            _ => {}
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank() as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank() as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Sachen150 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4100-$5FFF has the readable protection register at $4101 (decoded
        // on A8); $4020-$40FF and $4200+ without A8 are open bus.
        (0x4020..=0x5FFF).contains(&addr) && (addr & 0x0101) != 0x0101
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4100..=0x5FFF if (addr & 0x0101) == 0x0101 => {
                // Open-bus high 5 bits approximated as 0.
                self.reg[(self.current_register & 0x07) as usize] & 0x07
            }
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4100..=0x5FFF => self.write_register(addr, value),
            // $6000-$7FFF mirror: the ASIC sees these as register writes at
            // (addr + 0x1000) per the SaveRAM-mapped register path.
            0x6000..=0x7FFF => self.write_register(addr.wrapping_add(0x1000), value),
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.resolve_nametable(addr)],
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
                let off = self.resolve_nametable(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.mirror_sel() {
            1 => Mirroring::Horizontal,
            2 => Mirroring::Vertical,
            3 => Mirroring::SingleScreenA,
            // 0 = custom S0-S0-S0-S1; report as MapperControlled (the PPU
            // routes through our resolve_nametable for that case).
            _ => Mirroring::MapperControlled,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn nametable_address(&self, addr: u16) -> u16 {
        // CIRAM offset is always < 0x800, so the truncation is a no-op.
        self.resolve_nametable(addr) as u16
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(2 + 8 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.current_register);
        out.extend_from_slice(&self.reg);
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
        let expected = 2 + 8 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.current_register = data[1];
        let mut cursor = 2;
        self.reg.copy_from_slice(&data[cursor..cursor + 8]);
        cursor += 8;
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

impl Sachen150 {
    /// Resolve a nametable address to a CIRAM offset (`0..0x800`), applying the
    /// custom S0-S0-S0-S1 layout for mirroring selector 0 and the standard
    /// layouts otherwise.
    const fn resolve_nametable(&self, addr: u16) -> usize {
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        match self.mirror_sel() {
            // Custom S0-S0-S0-S1: tables 0/1/2 -> bank 0, table 3 -> bank 1.
            0 => {
                let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as usize;
                let physical = if table == 3 { 1 } else { 0 };
                physical * NAMETABLE_SIZE + local
            }
            1 => nametable_offset(addr, Mirroring::Horizontal),
            3 => nametable_offset(addr, Mirroring::SingleScreenA),
            // selector 2 (vertical) and any stray value default to vertical.
            _ => nametable_offset(addr, Mirroring::Vertical),
        }
    }
}

/// Mapper 143 (Sachen `TCA01`).
pub struct SachenTca01M143 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    mirroring: Mirroring,
}

impl SachenTca01M143 {
    /// Construct a new mapper 143 board.
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
                "mapper 143 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 143 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
        })
    }

    /// $8000-$BFFF -> first 16 KiB bank; $C000-$FFFF -> second 16 KiB bank
    /// (which equals the first on a 16 KiB image, after the modulo wrap).
    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = if addr < 0xC000 { 0 } else { 1 % count };
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for SachenTca01M143 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The protection register answers reads across the whole $4020-$5FFF
    // window (mapped). $8000-$FFFF PRG-ROM stays mapped (the trait default) —
    // a `!(...)` here would wrongly open-bus the program ROM + reset vector, so
    // the board never boots. There is no open-bus hole to carve out, so this
    // returns false for everything the board answers.
    fn cpu_read_unmapped(&self, _addr: u16) -> bool {
        false
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // Sachen TCA01 protection (puNES extcl_cpu_rd_mem_143): the chip
            // only answers when A8 is set, returning (~addr & 0x3F) in the low
            // 6 bits and leaving the top 2 bits as open bus. The old decode
            // answered across the WHOLE window with a hardcoded bit 6, so the
            // game's `(~addr & 0x3F)` protection compare failed -> blank boot.
            // The high 2 open-bus bits are approximated from the address high
            // byte (the most-recently-driven bus value in this read).
            0x4100..=0x5FFF if addr & 0x0100 != 0 => {
                ((!addr & 0x3F) as u8) | ((addr >> 8) as u8 & 0xC0)
            }
            0x4100..=0x5FFF if addr >= 0x5000 => 0xFF,
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = 0 % count;
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
        let mut out = Vec::with_capacity(1 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.vram.copy_from_slice(&data[1..=self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    /// 16 KiB-banked PRG: byte 0 of each 16 KiB bank holds the bank index.
    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
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

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m133_register_on_a8() {
        let mut m = Sachen133::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // value: PRG = (v>>2)&1, CHR = v&3. 0b0000_0111 -> PRG 1, CHR 3.
        m.cpu_write(0x4100, 0b0000_0111);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 3);
        // A8 clear -> no latch.
        m.cpu_write(0x4200, 0b0000_0000);
        assert_eq!(m.cpu_read(0x8000), 1);
    }

    #[test]
    fn m145_chr_from_data_bit7() {
        let mut m = Sachen145::new(synth_prg_32k(1), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        // Default CHR bank 0.
        assert_eq!(m.ppu_read(0x0000), 0);
        // (addr & 0x4100) == 0x4100 -> $4100 qualifies. Bit 7 set -> CHR 1.
        m.cpu_write(0x4100, 0x80);
        assert_eq!(m.ppu_read(0x0000), 1);
        // Also decoded in the $6000 save-RAM window ($6100 has 0x4100 bits).
        m.cpu_write(0x6100, 0x00);
        assert_eq!(m.ppu_read(0x0000), 0);
        // PRG is fixed 32 KiB bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m146_like_nina03() {
        let mut m = Sachen146::new(synth_prg_32k(2), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // value: PRG = (v>>3)&1, CHR = v&7. 0b0000_1101 -> PRG 1, CHR 5.
        m.cpu_write(0x4100, 0b0000_1101);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
        // Save-RAM alias also latches.
        m.cpu_write(0x6000, 0b0000_0010); // PRG 0, CHR 2
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    #[test]
    fn m147_jv001_protection_read_and_bank_decode() {
        // JV001 scramble derived from puNES `JV001.c` (GPL-2.0-or-later; also
        // documented in the nesdev wiki mapper-147 board notes). The board pre-scrambles
        // writes ((v&3)<<6)|((v&0xFC)>>2) and post-scrambles reads
        // ((v&0x3F)<<2)|((v&0xC0)>>6); the chip resets with invert=0xFF.
        let mut m =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // Disable inversion so the handshake is a clean staging->accumulator
        // copy: write 1 to $4101 (scrambled 0x01 -> 0x40, bit 0 == 0 -> invert
        // off). puNES: invert = 0xFF * (value & 1); 0x40 & 1 == 0 -> 0x00.
        m.cpu_write(0x4101, 0x01);
        // $4102 <- value V: staging = scramble(V)&0x0F, inverter = scramble(V)&0xF0.
        // Pick V = 0x14: scramble = ((0x14&3)<<6)|((0x14&0xFC)>>2) = 0|0x05 = 0x05
        //   -> staging 0x05, inverter 0x00.
        m.cpu_write(0x4102, 0x14);
        // $4100 latch (increase off, invert off): accumulator =
        //   (0 & 0xF0) | ((staging ^ 0) & 0x0F) = 0x05.
        m.cpu_write(0x4100, 0x00);
        // Handshake read: chip = ((inverter ^ invert) & 0xF0) | (acc & 0x0F)
        //   = (0x00 & 0xF0) | 0x05 = 0x05; board post-scramble = 0x05<<2 = 0x14.
        assert_eq!(m.cpu_read(0x4100), 0x14);
        // Refresh the bank-output latch (a $8000+ access): output =
        //   (inverter & 0xF0) | (acc & 0x0F) = 0x05.
        // PRG = ((out&0x20)>>4)|(out&1) = 0|1 = 1; CHR = (out&0x1E)>>1 = (4)>>1 = 2.
        m.cpu_write(0x8000, 0xFF); // bus conflict with PRG byte 0 (==0) -> 0
        // The $8000 write refreshes output from acc/inverter (0x05), not the
        // ANDed data; bank decode below reflects that.
        assert_eq!(m.cpu_read(0x8000), 1); // PRG bank 1 of 4 -> byte 0 = 1
        assert_eq!(m.ppu_read(0x0000), 2); // CHR bank 2 of 8 -> byte 0 = 2
    }

    #[test]
    fn m147_save_state_round_trip() {
        let mut m =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4101, 0x01);
        m.cpu_write(0x4102, 0x14);
        m.cpu_write(0x4100, 0x00);
        m.cpu_write(0x8000, 0x00); // refresh output latch
        let prg = m.cpu_read(0x8000);
        let chr = m.ppu_read(0x0000);
        let blob = m.save_state();
        let mut m2 =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), prg);
        assert_eq!(m2.ppu_read(0x0000), chr);
    }

    #[test]
    fn m148_latch_selects_prg_and_chr_with_conflict() {
        // PRG all-0xFF except offset 0, so the in-window write sees 0xFF.
        let mut m =
            Sachen148::new(synth_prg_32k(2), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        // value .... PCCC: PRG = bit3, CHR = bits 0-2.
        // Write at $8001 (PRG byte 0xFF -> no masking): 0b0000_1101 -> PRG 1, CHR 5.
        m.cpu_write(0x8001, 0b0000_1101);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn m149_chr_bit_in_bit7() {
        let mut m = Sachen149::new(synth_prg_32k(1), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        // Write at $8001 (PRG byte 0xFF -> no masking): bit7 set -> CHR 1.
        m.cpu_write(0x8001, 0x80);
        assert_eq!(m.ppu_read(0x0000), 1);
        // PRG is fixed bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        // bit7 clear -> CHR 0.
        m.cpu_write(0x8001, 0x00);
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    #[test]
    fn m150_register_protocol_and_banking() {
        let mut m = Sachen150::new(synth_prg_32k(4), synth_chr_8k(8)).unwrap();
        // Select register 5 (PRG), write value 2 -> PRG bank 2.
        m.cpu_write(0x4100, 5); // index
        m.cpu_write(0x4101, 2); // data
        assert_eq!(m.cpu_read(0x8000), 2);
        // Register 6 = CHR low 2 bits; register 4 bit0 = CHR bit2.
        // Set reg6 = 0b01, reg4 = 1 -> CHR = (1<<2)|1 = 5.
        m.cpu_write(0x4100, 6);
        m.cpu_write(0x4101, 0b001);
        m.cpu_write(0x4100, 4);
        m.cpu_write(0x4101, 1);
        assert_eq!(m.ppu_read(0x0000), 5);
        // Registers are readable (protection).
        m.cpu_write(0x4100, 6);
        assert_eq!(m.cpu_read(0x4101), 0b001);
    }

    #[test]
    fn m150_mirroring_modes() {
        let mut m = Sachen150::new(synth_prg_32k(1), synth_chr_8k(1)).unwrap();
        // reg7 mirroring sel = (reg7 >> 1) & 3.
        m.cpu_write(0x4100, 7);
        m.cpu_write(0x4101, 1 << 1); // sel 1 -> horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x4101, 2 << 1); // sel 2 -> vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0x4101, 3 << 1); // sel 3 -> single-screen A
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // sel 0 -> custom S0-S0-S0-S1; table 3 maps to bank 1.
        m.cpu_write(0x4101, 0);
        assert_eq!(m.current_mirroring(), Mirroring::MapperControlled);
        // table 0 ($2000) -> bank 0; table 3 ($2C00) -> bank 1.
        m.ppu_write(0x2000, 0xAA);
        m.ppu_write(0x2C00, 0xBB);
        assert_eq!(m.ppu_read(0x2000), 0xAA);
        assert_eq!(m.ppu_read(0x2C00), 0xBB);
        // table 1 ($2400) shares bank 0 with table 0 in this custom mode.
        assert_eq!(m.ppu_read(0x2400), 0xAA);
    }

    #[test]
    fn m143_protection_read_and_nrom() {
        let mut m =
            SachenTca01M143::new(synth_prg_16k(1), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // Protection answers only with A8 set: low 6 bits = ~addr & 0x3F, top
        // 2 bits = open bus (approximated from the address high byte). puNES.
        let addr = 0x4100u16;
        assert_eq!(
            m.cpu_read(addr),
            ((!addr & 0x3F) as u8) | ((addr >> 8) as u8 & 0xC0)
        );
        // A8 clear, addr >= $5000 -> $FF.
        assert_eq!(m.cpu_read(0x5000), 0xFF);
        assert!(!m.cpu_read_unmapped(0x4100));
        // NROM-128: $8000 and $C000 read the same (only) 16 KiB bank (index 0).
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 0);
    }

    #[test]
    fn m143_save_state_round_trip() {
        let mut m =
            SachenTca01M143::new(synth_prg_16k(1), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.ppu_write(0x2000, 0x33);
        let blob = m.save_state();
        let mut m2 =
            SachenTca01M143::new(synth_prg_16k(1), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x2000), 0x33);
    }

    // ---- mapper 243 (Sachen SA-020A) ---------------------------------------

    /// 128 KiB PRG (4 x 32 KiB) + 128 KiB CHR (16 x 8 KiB) -- the documented
    /// maxima, so every bank bit is reachable.
    fn m243() -> Sachen150 {
        Sachen150::new_on_board(synth_prg_32k(4), synth_chr_8k(16), Sa020aBoard::Sa020a).unwrap()
    }

    /// Write `value` to register `index` through the $4100/$4101 index/data pair.
    fn set_reg(m: &mut Sachen150, index: u8, value: u8) {
        m.cpu_write(0x4100, index);
        m.cpu_write(0x4101, value);
    }

    #[test]
    fn m243_prg_bank_is_r5_alone() {
        // R5 supplies PRG A16..A15 and nothing else contributes -- unlike the
        // SA-150 wiring, where R2 bit 0 also feeds PRG.
        let mut m = m243();
        for bank in 0..4u8 {
            set_reg(&mut m, 5, bank);
            assert_eq!(
                m.cpu_read(0x8000),
                bank,
                "R5={bank} selects 32 KiB bank {bank}"
            );
        }
        // R2 must NOT disturb PRG on this board.
        set_reg(&mut m, 5, 2);
        set_reg(&mut m, 2, 1);
        assert_eq!(m.cpu_read(0x8000), 2, "R2 does not feed PRG on the SA-020A");
    }

    #[test]
    fn m243_chr_bank_assembles_a13_a14_a15_a16() {
        // A13 = R2.0 (LSB), A14 = R4.0, A15 = R6.0, A16 = R6.1.
        let cases = [
            (0u8, 0u8, 0u8, 0u8),
            (1, 0, 0, 1),
            (0, 1, 0, 2),
            (1, 1, 0, 3),
            (0, 0, 1, 4),
            (1, 0, 1, 5),
            (0, 0, 2, 8),
            (1, 1, 3, 15),
        ];
        for (r2, r4, r6, expect) in cases {
            let mut m = m243();
            set_reg(&mut m, 2, r2);
            set_reg(&mut m, 4, r4);
            set_reg(&mut m, 6, r6);
            assert_eq!(
                m.ppu_read(0x0000),
                expect,
                "R2={r2} R4={r4} R6={r6} should select CHR bank {expect}"
            );
        }
    }

    #[test]
    fn m243_and_m150_decode_the_same_registers_differently() {
        // The whole reason 243 exists as a separate mapper: same ASIC, inverted
        // bank-bit significance. R2 is the CHR LSB on the SA-020A and the MSB on
        // the SA-150, so one setting must land on different banks.
        let mut a = m243();
        let mut b = Sachen150::new(synth_prg_32k(4), synth_chr_8k(16)).unwrap();
        for m in [&mut a, &mut b] {
            set_reg(m, 2, 1);
        }
        assert_eq!(a.ppu_read(0x0000), 1, "SA-020A: R2 is CHR A13");
        assert_eq!(b.ppu_read(0x0000), 8, "SA-150: R2 is the CHR MSB");
        assert_ne!(a.ppu_read(0x0000), b.ppu_read(0x0000));
    }

    #[test]
    fn m243_mirroring_modes_including_the_vertically_flipped_l() {
        let mut m = m243();
        // Selector lives in R7 bits 2:1.
        set_reg(&mut m, 7, 1 << 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        set_reg(&mut m, 7, 2 << 1);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        set_reg(&mut m, 7, 3 << 1);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);

        // Selector 0 is the S0-S0-S0-S1 layout: the first three nametables share
        // CIRAM page 0 and only the fourth is distinct.
        set_reg(&mut m, 7, 0);
        assert_eq!(m.current_mirroring(), Mirroring::MapperControlled);
        m.ppu_write(0x2000, 0xA1);
        m.ppu_write(0x2C00, 0xB2);
        assert_eq!(m.ppu_read(0x2000), 0xA1);
        assert_eq!(m.ppu_read(0x2400), 0xA1, "table 1 shares page 0");
        assert_eq!(m.ppu_read(0x2800), 0xA1, "table 2 shares page 0");
        assert_eq!(m.ppu_read(0x2C00), 0xB2, "table 3 is the unique page");
    }

    #[test]
    fn m243_registers_read_back_and_round_trip_through_a_save_state() {
        let mut m = m243();
        set_reg(&mut m, 6, 3);
        assert_eq!(
            m.cpu_read(0x4101),
            3,
            "the data port reads the selected register"
        );
        set_reg(&mut m, 2, 1);
        set_reg(&mut m, 7, 2 << 1);
        let blob = m.save_state();

        let mut m2 = m243();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
        assert_eq!(m2.current_mirroring(), Mirroring::Vertical);
    }
}
