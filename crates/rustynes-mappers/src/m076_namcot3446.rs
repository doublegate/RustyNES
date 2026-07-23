//! Namcot 3446 (mapper 76).
//!
//! A cut-down relative of the Namco 118 (`namco118.rs`): the same register-pair
//! protocol -- write a register index to the even address, then its value to
//! the odd one -- but with a reduced bank layout and no IRQ. It is the shape
//! Namco used for cheaper cartridges before the MMC3-class boards took over.
//!
//! Its sibling 3425 is in `m095_namcot3425.rs`.//!
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
const CHR_BANK_2K: usize = 0x0800;
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
// Mapper 28 — Action 53 homebrew multicart.
//
// A single outer register at $5000-$5FFF selects which inner register a
// $8000-$FFFF write targets (reg index in bits 7-6 of the $5xxx value). The
// four inner registers are:
//   reg 0 ($00): CHR bank (8 KiB CHR-RAM is single-bank, so this only stores).
//   reg 1 ($01): low PRG bank bits.
//   reg 2 ($80): mode/mirroring: bits 0-1 = mirroring, bits 2-3 = PRG mode,
//                bits 4-5 = outer-bank size mask.
//   reg 3 ($81): outer PRG bank.
// We model the documented PRG-banking + mirroring; CHR is 8 KiB RAM. No IRQ.
//
// The resolved PRG layout follows the nesdev "Action 53" decode: the 32 KiB
// CPU window splits into two 16 KiB halves. Mode (bits 2-3 of reg 2) picks:
//   0/1 (NROM-256): both halves track the selected 32 KiB bank.
//   2  (UNROM):     $8000 = selectable 16 KiB, $C000 = fixed last-in-outer.
//   3  (NROM-128):  both halves mirror one 16 KiB bank.
// ===========================================================================

/// Mapper 76 (`NAMCOT-3446`).
pub struct Namcot3446M76 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    prg_banks: [u8; 2],
    chr_banks: [u8; 4],
    mirroring: Mirroring,
}

impl Namcot3446M76 {
    /// Construct a new mapper 76 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 76 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 76 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            prg_banks: [0, 1],
            chr_banks: [0, 1, 2, 3],
            mirroring,
        })
    }

    fn prg_offset(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Namcot3446M76 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        match addr {
            0x8000..=0x9FFF => self.prg_offset(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.prg_offset(self.prg_banks[1] as usize, addr),
            // `last - 1` would underflow on a single-8 KiB-bank ROM (`last == 0`);
            // `prg_offset`'s modulo makes both forms identical for multi-bank ROMs.
            0xC000..=0xDFFF => self.prg_offset(last.saturating_sub(1), addr),
            0xE000..=0xFFFF => self.prg_offset(last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF if (addr & 0x01) == 0 => self.reg_index = value & 0x07,
            0x8000..=0x9FFF => match self.reg_index {
                2 => self.chr_banks[0] = value,
                3 => self.chr_banks[1] = value,
                4 => self.chr_banks[2] = value,
                5 => self.chr_banks[3] = value,
                6 => self.prg_banks[0] = value,
                7 => self.prg_banks[1] = value,
                _ => {}
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr >> 11) as usize & 0x03;
                let count = (self.chr_rom.len() / CHR_BANK_2K).max(1);
                let bank = (self.chr_banks[slot] as usize) % count;
                self.chr_rom[bank * CHR_BANK_2K + (addr as usize & 0x07FF)]
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
        let mut out = Vec::with_capacity(1 + 1 + 2 + 4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_banks);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + 1 + 2 + 4 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_index = data[1];
        self.prg_banks.copy_from_slice(&data[2..4]);
        self.chr_banks.copy_from_slice(&data[4..8]);
        self.vram.copy_from_slice(&data[8..8 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 174 — NTDEC 5-in-1 multicart.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   PRG: bits 4-7 of A select the bank; bit 7 picks 16 KiB (1) vs 32 KiB (0).
//   We follow the documented decode: bank = (A >> 4) & 0x07; 32 KiB mode when
//   (A & 0x80) == 0; CHR (8 KiB) bank = (A >> 1) & 0x07; mirroring = A & 1.
// CHR is ROM. No IRQ.
// ===========================================================================

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m76_register_pairs_select_banks() {
        let mut m =
            Namcot3446M76::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        // index 6 -> PRG $8000 = bank 3.
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // index 2 -> CHR slot 0 = bank 5.
        m.cpu_write(0x8000, 2);
        m.cpu_write(0x8001, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // $C000/$E000 fixed to last two banks (6, 7).
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m76_save_state_round_trip() {
        let mut m =
            Namcot3446M76::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 4); // PRG $A000 = bank 4
        let blob = m.save_state();
        let mut m2 =
            Namcot3446M76::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xA000), 4);
    }
}
