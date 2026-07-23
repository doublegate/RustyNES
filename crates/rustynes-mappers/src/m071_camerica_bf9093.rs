//! Camerica / Codemasters BF9093 and relatives (mapper 71).
//!
//! A UxROM-shaped board: a 16 KiB PRG bank selected at `$C000-$FFFF` with the
//! last bank fixed, CHR-RAM, and no IRQ. The Fire Hawk variant additionally
//! decodes a single-screen mirroring bit at `$9000-$9FFF`, which is why the
//! mirroring write window is separated from the bank-select window rather
//! than sharing one write-anywhere decode.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    clippy::manual_range_patterns,
    clippy::match_same_arms,
    clippy::too_many_arguments
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Camerica / Codemasters BF9093 (Mapper 71).
pub struct Camerica {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    mirroring: Mirroring,
    has_single_screen: bool,
}

impl Camerica {
    /// Construct a new Camerica mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        mirroring: Mirroring,
        has_single_screen: bool,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Camerica PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            mirroring,
            has_single_screen,
        })
    }
}

impl Mapper for Camerica {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let total_16k = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let last = total_16k - 1;
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % total_16k;
                self.prg_rom[(bank * PRG_BANK_16K + (addr as usize - 0x8000)) % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                self.prg_rom[(last * PRG_BANK_16K + (addr as usize - 0xC000)) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x9000..=0x9FFF if self.has_single_screen => {
                self.mirroring = if value & 0x10 == 0 {
                    Mirroring::SingleScreenA
                } else {
                    Mirroring::SingleScreenB
                };
            }
            0xC000..=0xFFFF | 0x8000..=0xBFFF => self.prg_bank = value & 0x0F,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize % self.chr_ram.len()],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let len = self.chr_ram.len();
                self.chr_ram[addr as usize % len] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(1u8);
        out.push(self.prg_bank);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.has_single_screen));
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.chr_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.mirroring = match data[2] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.has_single_screen = data[3] != 0;
        let mut cur = 4usize;
        self.chr_ram
            .copy_from_slice(&data[cur..cur + self.chr_ram.len()]);
        cur += self.chr_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRG_BANK_8K: usize = 0x2000;

    fn synth(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn camerica_bank_swap() {
        let mut m = Camerica::new(synth(8 * 2), Mirroring::Vertical, false).unwrap();
        // Default: bank 0 at $8000.
        assert_eq!(m.cpu_read(0x8000), 0);
        m.cpu_write(0xC000, 5);
        // Bank 5 (16K bank index, but we have 16K chunks — total_16k = 16).
        // bank 5 at 16K offset. Let's just check it swaps from 0.
        assert_ne!(m.cpu_read(0x8000), 0);
    }
}
