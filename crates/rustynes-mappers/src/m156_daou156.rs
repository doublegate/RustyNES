//! Daou Infosys (mapper 156) -- Korean licensed boards, e.g. Metal Force.
//!
//! Separate register windows for PRG and for each of the eight 1 KiB CHR
//! slots, plus a runtime single-screen mirroring control -- unusually
//! fine-grained for a board with no IRQ. The CHR registers are 16 bits wide,
//! split across a low and a high write, which is why each slot occupies two
//! addresses.
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
const CHR_BANK_1K: usize = 0x0400;
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

/// Mapper 156 (DIS23C01 DAOU).
pub struct Daou156 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    // 8 low nibbles + 8 high nibbles, composed into a 1 KiB bank per slot.
    chr_lo: [u8; 8],
    chr_hi: [u8; 8],
    mirroring: Mirroring,
}

impl Daou156 {
    /// Construct a new mapper 156 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 156 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 156 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_lo: [0; 8],
            chr_hi: [0; 8],
            // DAOU/DIS23C01 powers on single-screen (nametable A) per Mesen2
            // InitMapper; the $C014 register flips it to H/V at runtime.
            mirroring: Mirroring::SingleScreenA,
        })
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = (addr as usize >> 10) & 0x07;
        let bank = ((self.chr_lo[slot] as usize) | ((self.chr_hi[slot] as usize) << 8)) % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for Daou156 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % count;
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            0xC000..=0xFFFF => {
                let last = count - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // $C000-$C00F: 16 CHR-bank-nibble registers. Mesen2 decodes the
            // 1 KiB slot as (addr & 0x03) + (addr >= 0xC008 ? 4 : 0) and selects
            // the low/high nibble array by bit 2 (0x04) — NOT a flat lo[0..8] /
            // hi[0..8] split. The old flat decode wrote the wrong slot's nibble,
            // so CHR banks resolved to garbage → blank/garbled boot.
            0xC000..=0xC00F => {
                let slot = ((addr & 0x03) + if addr >= 0xC008 { 4 } else { 0 }) as usize;
                if addr & 0x04 != 0 {
                    self.chr_hi[slot] = value;
                } else {
                    self.chr_lo[slot] = value;
                }
            }
            0xC010 => self.prg_bank = value,
            // $C014: 0 = vertical, 1 = horizontal (Mesen2). The old code mapped
            // this to a single-screen A/B toggle, which never matched the game's
            // expected nametable layout.
            0xC014 => {
                self.mirroring = if value & 0x01 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
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
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(19 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.chr_lo);
        out.extend_from_slice(&self.chr_hi);
        out.push(match self.mirroring {
            Mirroring::Horizontal => 0,
            Mirroring::Vertical => 1,
            Mirroring::SingleScreenB => 2,
            _ => 3, // SingleScreenA (power-on default) + any other
        });
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 19 + self.vram.len();
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
        self.chr_lo.copy_from_slice(&data[2..10]);
        self.chr_hi.copy_from_slice(&data[10..18]);
        self.mirroring = match data[18] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenB,
            _ => Mirroring::SingleScreenA,
        };
        self.vram.copy_from_slice(&data[19..19 + self.vram.len()]);
        Ok(())
    }
}

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

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m156_chr_compose_prg_and_mirroring() {
        let mut m = Daou156::new(synth_prg_16k(8), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        // Power-on mirroring is single-screen A (Mesen2 InitMapper).
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // PRG $C010 -> bank 3.
        m.cpu_write(0xC010, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed last
        // CHR slot 0: low = 5 ($C000), high = 0 -> bank 5.
        m.cpu_write(0xC000, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // High nibble of slot 0 lives at $C004 (bit 2 selects the high array):
        // low 5 | (high 1 << 8) = 0x105, wraps mod 32 -> 5.
        m.cpu_write(0xC004, 1);
        assert_eq!(m.ppu_read(0x0000), (0x105usize % 32) as u8);
        // Mirroring $C014: 1 = horizontal, 0 = vertical.
        m.cpu_write(0xC014, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0xC014, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m156_save_state_round_trip() {
        let mut m = Daou156::new(synth_prg_16k(8), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC010, 2);
        m.cpu_write(0xC001, 4);
        m.cpu_write(0xC014, 1);
        let blob = m.save_state();
        let mut m2 = Daou156::new(synth_prg_16k(8), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0400), m.ppu_read(0x0400));
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }
}
