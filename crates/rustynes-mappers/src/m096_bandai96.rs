//! Bandai Oeka Kids (mapper 96).
//!
//! The unusual one in this batch: the low CHR bank bits are not written by the
//! CPU at all -- they are latched from the *PPU address bus* during nametable
//! fetches. The board watches for a fetch in `$2000-$2FFF` and captures two
//! address bits, so the CHR bank in use tracks which nametable quadrant the
//! PPU is currently reading. That is how the drawing-tablet software swaps
//! character data per screen region without CPU involvement, and it is why
//! this board needs a PPU-read hook where its peers need none.
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

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_4K: usize = 0x1000;
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

/// Mapper 96 (Bandai Oeka Kids).
pub struct Bandai96 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    outer_chr: u8,
    inner_chr: u8,
    last_ppu_addr: u16,
    mirroring: Mirroring,
}

impl Bandai96 {
    /// Construct a new mapper 96 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 4 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 96 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            // Two 4 KiB CHR-RAM banks (the Oeka Kids drawing buffer).
            vec![0u8; 2 * CHR_BANK_4K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_4K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 96 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            outer_chr: 0,
            inner_chr: 0,
            last_ppu_addr: 0,
            mirroring,
        })
    }

    fn chr_count_4k(&self) -> usize {
        (self.chr.len() / CHR_BANK_4K).max(1)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        // $0000 slot uses outer|inner; $1000 slot uses outer|0x03.
        let slot = (addr >> 12) & 0x01;
        let bank = if slot == 0 {
            self.outer_chr | self.inner_chr
        } else {
            self.outer_chr | 0x03
        };
        let bank = (bank as usize) % self.chr_count_4k();
        bank * CHR_BANK_4K + (addr as usize & (CHR_BANK_4K - 1))
    }
}

impl Mapper for Bandai96 {
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
            self.prg_bank = value & 0x03;
            self.outer_chr = value & 0x04;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let masked = addr & 0x3FFF;
        // Sniff the PPU address bus: rising edge into a nametable fetch latches
        // the CHR inner bank from address bits 9-8.
        if (self.last_ppu_addr & 0x3000) != 0x2000 && (masked & 0x3000) == 0x2000 {
            self.inner_chr = ((masked >> 8) & 0x03) as u8;
        }
        self.last_ppu_addr = masked;
        match masked {
            0x0000..=0x1FFF => self.chr[self.chr_offset(masked)],
            0x2000..=0x3EFF => self.vram[nametable_offset(masked, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let masked = addr & 0x3FFF;
        if (self.last_ppu_addr & 0x3000) != 0x2000 && (masked & 0x3000) == 0x2000 {
            self.inner_chr = ((masked >> 8) & 0x03) as u8;
        }
        self.last_ppu_addr = masked;
        match masked {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(masked);
                    self.chr[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(masked, self.mirroring);
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
        let mut out = Vec::with_capacity(6 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.outer_chr);
        out.push(self.inner_chr);
        out.push((self.last_ppu_addr & 0xFF) as u8);
        out.push((self.last_ppu_addr >> 8) as u8);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 6 + self.vram.len() + chr_extra;
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
        self.last_ppu_addr = u16::from(data[4]) | (u16::from(data[5]) << 8);
        let mut cursor = 6;
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

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_4K];
        for b in 0..banks {
            v[b * CHR_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m96_prg_and_outer_chr() {
        let mut m =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        // PRG = bits 0-1, outer CHR = bit 2.
        m.cpu_write(0x8000, 0b0000_0011); // PRG 3, outer 0
        assert_eq!(m.cpu_read(0x8000), 3);
        // $1000 slot = outer|0x03 = 3.
        assert_eq!(m.ppu_read(0x1000), 3);
    }

    #[test]
    fn m96_inner_chr_latched_from_ppu_bus() {
        let mut m =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        // outer = 0 (PRG write bit2 clear).
        m.cpu_write(0x8000, 0);
        // Approach a nametable fetch from a non-$2xxx address (e.g. a pattern
        // fetch at $0000), then fetch $2100 -> inner = (0x2100>>8)&3 = 1.
        let _ = m.ppu_read(0x0000);
        let _ = m.ppu_read(0x2100);
        // $0000 slot bank = outer|inner = 0|1 = 1.
        assert_eq!(m.ppu_read(0x0000), 1);
        // Fetch $2300 -> inner = 3. (Must re-approach from outside $2xxx.)
        let _ = m.ppu_read(0x0000);
        let _ = m.ppu_read(0x2300);
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn m96_save_state_round_trips_ppu_bus_latch() {
        let mut b =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        b.cpu_write(0x8000, 0);
        let _ = b.ppu_read(0x0000);
        let _ = b.ppu_read(0x2200);
        let blob = b.save_state();
        let mut b2 =
            Bandai96::new(synth_prg_32k(4), synth_chr_4k(8), Mirroring::Horizontal).unwrap();
        b2.load_state(&blob).unwrap();
        assert_eq!(b2.ppu_read(0x0000), b.ppu_read(0x0000));
    }
}
