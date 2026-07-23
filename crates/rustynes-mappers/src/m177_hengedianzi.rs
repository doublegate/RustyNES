//! Hengedianzi (mapper 177) -- Chinese unlicensed board.
//!
//! A 32 KiB PRG bank select plus a mirroring bit, both in one write-anywhere
//! register at `$8000-$FFFF`. Its sibling mapper 179
//! (`m179_hengedianzi.rs`) splits the same two fields across two windows.//!
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
// Mapper 31 — INL / NSF-style 4 KiB-banked board ("2A03 Puritans").
//
// Eight 4 KiB PRG slots ($8000/$9000/.../$F000), each latched by a write to
// $5FF8-$5FFF (the low three address bits pick the slot). Power-on fixes the
// last slot ($F000) to the final 4 KiB bank (0xFF & mask). CHR is 8 KiB RAM.
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 177 (Hengedianzi).
pub struct Hengedianzi177 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Hengedianzi177 {
    /// Construct a new mapper 177 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(prg_rom: Box<[u8]>, _chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 177 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Hengedianzi177 {
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
            // $8000-$FFFF: `..MP PPPP` — PRG bank is bits 0-4 (5 bits), mirroring
            // is bit 5. The old code latched all 8 bits as the bank, so a write
            // that flips the mirroring bit (e.g. $20) selected bank 32 and the
            // reset vector read garbage → blank boot.
            self.prg_bank = value & 0x1F;
            self.horizontal_mirroring = (value & 0x20) != 0;
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
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
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
        self.prg_bank = data[1];
        self.horizontal_mirroring = data[2] != 0;
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
// Mapper 179 — Hengedianzi variant.
//
// A 32 KiB PRG bank is latched via $5000-$5FFF (= value >> 1). A separate
// $8000-$FFFF write sets the mirroring bit (bit 0: 1 = horizontal, 0 =
// vertical). CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m177_prg_and_mirroring() {
        let mut m = Hengedianzi177::new(synth_prg_32k(8), &[]).unwrap();
        // value 0b0010_0011 (0x23): PRG = 0x23 % 8 = 3; bit5 set -> horizontal.
        m.cpu_write(0x8000, 0b0010_0011);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Clear bit 5 -> vertical.
        m.cpu_write(0x8000, 0b0000_0010);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m177_save_state_round_trip() {
        let mut m = Hengedianzi177::new(synth_prg_32k(8), &[]).unwrap();
        m.cpu_write(0x8000, 0b0010_0101); // PRG 5, horizontal
        m.ppu_write(0x0004, 0xEE);
        let blob = m.save_state();
        let mut m2 = Hengedianzi177::new(synth_prg_32k(8), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 5);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0004), 0xEE);
    }
}
