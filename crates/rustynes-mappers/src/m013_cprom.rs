//! CPROM (mapper 13) -- Nintendo discrete board with banked CHR-RAM.
//!
//! Unusual in that the switchable half is *RAM*, not ROM: PRG is a fixed
//! 32 KiB with no banking at all, while the upper 4 KiB of the 16 KiB
//! CHR-RAM is selected by the low two bits of a write-anywhere register.
//! Used by Videomation.
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

const CHR_BANK_4K: usize = 0x1000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// CPROM (Mapper 13).
pub struct Cprom {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>, // 16 KiB total: 4 banks of 4 KiB.
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Cprom {
    /// Construct a new CPROM mapper (NES Time Lord uses this).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        if prg_rom.len() != 32 * 1024 {
            return Err(MapperError::Invalid(format!(
                "CPROM expects 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; 16 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Cprom {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 {
            return 0;
        }
        self.prg_rom[(addr as usize - 0x8000) % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.chr_bank = value & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => self.chr_ram[addr as usize],
            0x1000..=0x1FFF => {
                let bank = (self.chr_bank as usize) & 0x03;
                let off = bank * CHR_BANK_4K + (addr as usize - 0x1000);
                self.chr_ram[off % self.chr_ram.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => self.chr_ram[addr as usize] = value,
            0x1000..=0x1FFF => {
                let bank = (self.chr_bank as usize) & 0x03;
                let off = (bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr_ram.len();
                self.chr_ram[off] = value;
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
        let mut out = Vec::with_capacity(2 + self.chr_ram.len() + self.vram.len());
        out.push(1u8);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.chr_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_bank = data[1];
        self.chr_ram
            .copy_from_slice(&data[2..2 + self.chr_ram.len()]);
        let off = 2 + self.chr_ram.len();
        self.vram.copy_from_slice(&data[off..off + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cprom_chr_bank_select() {
        let mut m =
            Cprom::new(vec![0u8; 32 * 1024].into_boxed_slice(), Mirroring::Vertical).unwrap();
        m.ppu_write(0x1000, 0xAA); // bank 0
        m.cpu_write(0x8000, 1);
        m.ppu_write(0x1000, 0xBB); // bank 1
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x1000), 0xAA);
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x1000), 0xBB);
    }
}
