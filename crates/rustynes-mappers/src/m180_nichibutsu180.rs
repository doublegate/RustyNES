//! Nichibutsu / Hokutosha (mapper 180) -- Crazy Climber.
//!
//! An inverted `UxROM` board: the *low* 16 KiB is fixed and the *high* 16 KiB
//! switchable, the opposite of stock `UxROM`. That inversion is not a design
//! flourish -- Crazy Climber ships with a special controller, and the board is
//! wired so the fixed half holds the code that must always be reachable.
//! Writes are subject to a bus conflict, as on any ungated discrete board.
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

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
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
// Mapper 147 — Sachen 3018 (TXC JV001).
//
// Driven by the TXC JV001 scrambling-accumulator ASIC. Four internal registers
// are written via $4100-$4103 (decoded on `addr & 0x4103`); the scrambled
// output latch updates on any $4100 / $8000-$FFFF write. The boot code performs
// a protection handshake by WRITING a value to $4102/$4100, then READING the
// chip back at $4100 and comparing — so the read MUST return the scrambled
// value, not open bus, or the boot validation loops forever.
//
// JV001 chip read value:  output = (accumulator & 0x3F) | ((inverter ^ inv) & 0xC0)
// Bank decode from the chip output latch (PRG A bits + CHR low bits):
//   PRG (32 KiB) = (output >> 4) & 0x03      (up to 128 KiB)
//   CHR ( 8 KiB) =  output       & 0x0F
// Writes land at $4100-$5FFF (register file) and at $8000-$FFFF (output latch,
// with bus conflict). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 180 (Nichibutsu `UNROM`-inverted, Crazy Climber).
pub struct Nichibutsu180 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Nichibutsu180 {
    /// Construct a new mapper 180 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 180 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
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
                "mapper 180 expects 8 KiB CHR (RAM or ROM); got {} bytes",
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

    fn read_prg(&self, bank: usize, offset_in_bank: usize) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + offset_in_bank]
    }
}

impl Mapper for Nichibutsu180 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(0, addr as usize - 0x8000),
            0xC000..=0xFFFF => self.read_prg(self.prg_bank as usize, addr as usize - 0xC000),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND with the byte currently visible at addr.
            let prg_byte = self.cpu_read_at_for_conflict(addr);
            let effective = value & prg_byte;
            self.prg_bank = effective & 0x07;
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

impl Nichibutsu180 {
    /// The byte currently visible at `addr` in the $8000-$FFFF window, used for
    /// bus-conflict masking (mirrors the active `cpu_read` banking).
    fn cpu_read_at_for_conflict(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(0, addr as usize - 0x8000),
            _ => self.read_prg(self.prg_bank as usize, addr as usize - 0xC000),
        }
    }
}

// ===========================================================================
// Mapper 185 — CNROM with CHR-disable copy protection.
//
// Stock CNROM banking (8 KiB CHR latch in $8000-$FFFF, bus conflicts), plus a
// copy-protection scheme: certain values written to the CHR register DISABLE
// CHR-ROM, causing reads to return $FF. The submapper selects which 2-bit
// pattern enables CHR; submapper 0 (the common heuristic) enables CHR whenever
// either of the low two bits is set (i.e. value & 0x03 != 0). We model the
// data-driven enable test (the per-read $2007 heuristic of GeraNES is not
// needed for the data-bus protection most mapper-185 ROMs use).
//   CHR (8 KiB) = effective & mask
//   CHR enabled (submapper 0) iff (effective & 0x03) != 0
//   submapper 4/5/6/7 enable iff (effective & 0x03) == 0/1/2/3 respectively
// PRG is fixed (16 or 32 KiB NROM). Mirroring header-fixed; no IRQ.
// ===========================================================================

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
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
    fn m180_fixes_low_switches_high() {
        let mut m =
            Nichibutsu180::new(synth_prg_16k(8), Box::new([]), Mirroring::Vertical).unwrap();
        // $8000-$BFFF is fixed to bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        // Write at $C001 (PRG byte 0xFF -> no masking) selects $C000 bank 3.
        m.cpu_write(0xC001, 3);
        assert_eq!(m.cpu_read(0xC000), 3);
        // $8000 still fixed.
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m180_bus_conflict() {
        // $C000 bank 0 offset 0 holds the bank index (0). Writing 3 there ANDs
        // with 0 -> bank 0.
        let mut m =
            Nichibutsu180::new(synth_prg_16k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC000, 3);
        assert_eq!(m.cpu_read(0xC000), 0);
    }
}
