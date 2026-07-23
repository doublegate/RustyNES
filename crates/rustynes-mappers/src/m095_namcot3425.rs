//! Namcot 3425 (mapper 95).
//!
//! The same Namco register-pair protocol as the 3446 in
//! `m076_namcot3446.rs`, with one addition worth knowing: a bit of the CHR
//! bank number is routed to the nametable select, so the board drives
//! single-screen mirroring from the *CHR banking registers* rather than from a
//! dedicated mirroring register.//!
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

const CHR_BANK_1K: usize = 0x0400;

/// Mapper 95 (`NAMCOT-3425`, *Dragon Buster*).
pub struct Namcot3425M95 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    prg_banks: [u8; 2],
    // chr[0],chr[1] are 2 KiB selects; chr[2..6] are 1 KiB selects.
    chr_regs: [u8; 6],
    one_screen_b: bool,
}

impl Namcot3425M95 {
    /// Construct a new mapper 95 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 95 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 95 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            prg_banks: [0, 1],
            chr_regs: [0; 6],
            one_screen_b: false,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        // Resolve the 1 KiB bank for this CHR address.
        let bank1k = match addr {
            0x0000..=0x07FF => (self.chr_regs[0] as usize & !1) + ((addr as usize >> 10) & 1),
            0x0800..=0x0FFF => (self.chr_regs[1] as usize & !1) + ((addr as usize >> 10) & 1),
            0x1000..=0x13FF => self.chr_regs[2] as usize,
            0x1400..=0x17FF => self.chr_regs[3] as usize,
            0x1800..=0x1BFF => self.chr_regs[4] as usize,
            _ => self.chr_regs[5] as usize,
        };
        let bank = bank1k % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for Namcot3425M95 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        match addr {
            0x8000..=0x9FFF => self.read_prg(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.read_prg(self.prg_banks[1] as usize, addr),
            0xC000..=0xDFFF => self.read_prg(last - 1, addr),
            0xE000..=0xFFFF => self.read_prg(last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF if addr & 1 == 0 => self.reg_index = value & 0x07,
            0x8000..=0x9FFF => match self.reg_index {
                0 => {
                    self.chr_regs[0] = value & 0x3F;
                    // CHR reg 0 bit 5 drives one-screen select on this board.
                    self.one_screen_b = (value & 0x20) != 0;
                }
                1 => self.chr_regs[1] = value & 0x3F,
                2..=5 => self.chr_regs[self.reg_index as usize] = value & 0x3F,
                6 => self.prg_banks[0] = value,
                _ => self.prg_banks[1] = value,
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.read_chr(addr),
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if (0x2000..=0x3EFF).contains(&addr) {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.one_screen_b {
            Mirroring::SingleScreenB
        } else {
            Mirroring::SingleScreenA
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(11 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_regs);
        out.push(u8::from(self.one_screen_b));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 11 + self.vram.len();
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
        self.chr_regs.copy_from_slice(&data[4..10]);
        self.one_screen_b = data[10] != 0;
        self.vram.copy_from_slice(&data[11..11 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 112 — NTDEC ASDER / Huang-1.
//
// An indexed register port (no A12 IRQ — distinct from the MMC3 it resembles):
//   $8000 : register index (bits 0-2).
//   $A000 : register data.
//   $C000 : CHR high bits / outer (modelled as an outer CHR bank add).
//   $E000 : mirroring (bit 0: 0 = vertical, 1 = horizontal).
// Register slots:
//   0 -> PRG bank at $8000 (8 KiB)
//   1 -> PRG bank at $A000 (8 KiB)
//   2 -> CHR 2 KiB at $0000
//   3 -> CHR 2 KiB at $0800
//   4..7 -> CHR 1 KiB at $1000/$1400/$1800/$1C00
// $C000/$E000 are fixed to the last two 8 KiB PRG banks. CHR is ROM.
// ===========================================================================

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
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
    fn m95_prg_select_and_one_screen() {
        let mut m =
            Namcot3425M95::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // PRG reg 6 -> $8000.
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // CHR reg 0, value with bit 5 set -> one-screen B.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 0x20);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        // $C000/$E000 fixed to last two (6,7).
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m95_save_state_round_trip() {
        let mut m =
            Namcot3425M95::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 4);
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 0x20);
        let blob = m.save_state();
        let mut m2 =
            Namcot3425M95::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xA000), m.cpu_read(0xA000));
        assert_eq!(m2.current_mirroring(), Mirroring::SingleScreenB);
    }
}
