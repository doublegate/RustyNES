//! Nintendo `UN1ROM` (mapper 94) -- Senjou no Ookami.
//!
//! A `UxROM` (`m002_uxrom.rs`) whose PRG bank field sits in data bits 4-2 rather
//! than the low bits, so the same 16 KiB-switchable / 16 KiB-fixed layout is
//! driven by a shifted register value. CHR is RAM and there is no IRQ.//!
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

const PRG_BANK_16K: usize = 0x4000;
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

/// Mapper 94 (`UN1ROM`).
pub struct Un1rom94 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Un1rom94 {
    /// Construct a new mapper 94 board.
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
                "mapper 94 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            mirroring,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Un1rom94 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(self.prg_bank as usize, addr),
            0xC000..=0xFFFF => {
                let last = (self.prg_rom.len() / PRG_BANK_16K).max(1) - 1;
                self.read_prg(last, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND with the byte the CPU actually reads at this
            // address, using the SAME window mapping as `cpu_read` — the
            // switchable bank for $8000-$BFFF, the fixed last bank for
            // $C000-$FFFF (a register write can land in either half).
            let conflict = self.cpu_read(addr);
            let effective = value & conflict;
            // UN1ROM (Senjou no Ookami) selects the 16 KiB bank from data
            // bits 4..2 (a 3-bit field, 8 banks).
            self.prg_bank = (effective >> 2) & 0x07;
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
        let mut out = Vec::with_capacity(2 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len() + self.chr_ram.len();
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
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 101 — Jaleco JF-10 CHR latch.
//
// A single fixed 32 KiB PRG bank. An 8 KiB CHR bank is latched by a write to
// the $6000-$7FFF (PRG-RAM) window. Mirroring header-fixed; no IRQ.
// ===========================================================================

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m94_prg_bank_from_data_bits() {
        let mut m = Un1rom94::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // value (data>>2)&0x0F = 3 -> write 0b0000_1100 (12). Offset !=0 has
        // no bus conflict (0xFF), so the value sticks.
        m.cpu_write(0x8001, 0b0000_1100);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $C000 is fixed to the last 16 KiB bank (7).
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn m94_save_state_round_trip() {
        let mut m = Un1rom94::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8001, 0b0001_0000); // (16>>2)&0xF = 4
        m.ppu_write(0x0002, 0x9A);
        let blob = m.save_state();
        let mut m2 = Un1rom94::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 4);
        assert_eq!(m2.ppu_read(0x0002), 0x9A);
    }
}
