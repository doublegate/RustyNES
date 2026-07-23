//! Konami VRC1 (mapper 75) -- the first and simplest VRC ASIC.
//!
//! Three 8 KiB PRG banks plus a fixed last bank, two 4 KiB CHR banks, and
//! mirroring control. The quirk worth knowing: each CHR bank register is
//! only four bits wide, and its *fifth* bit lives in the mirroring register
//! at `$9000` -- so a CHR bank select above 15 requires writing two
//! different registers.
//!
//! Unlike VRC2/VRC4/VRC6/VRC7 there is no IRQ counter and no on-cart audio;
//! see `vrc2_vrc4.rs`, `m073_vrc3.rs`, `vrc6.rs`, `m085_vrc7.rs` for those.
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

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// VRC1 (Mapper 75).
pub struct Vrc1 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_banks: [u8; 3], // $8000, $A000, $C000
    chr_lo: u8,
    chr_hi: u8,
    chr_lo_msb: u8,
    chr_hi_msb: u8,
    mirroring: Mirroring,
}

impl Vrc1 {
    /// Construct a new VRC1 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC1 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_4K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "VRC1 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_banks: [0, 1, 2],
            chr_lo: 0,
            chr_hi: 0,
            chr_lo_msb: 0,
            chr_hi_msb: 0,
            mirroring,
        })
    }
}

impl Mapper for Vrc1 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total_8k - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_banks[0] as usize) % total_8k,
            0xA000 => (self.prg_banks[1] as usize) % total_8k,
            0xC000 => (self.prg_banks[2] as usize) % total_8k,
            0xE000 => last,
            _ => return 0,
        };
        self.prg_rom[(bank * PRG_BANK_8K + (addr as usize & 0x1FFF)) % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xF000 {
            0x8000 => self.prg_banks[0] = value & 0x0F,
            0x9000 => {
                // Mirroring (bit 0) + CHR MSB bits.
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
                self.chr_lo_msb = (value >> 1) & 1;
                self.chr_hi_msb = (value >> 2) & 1;
            }
            0xA000 => self.prg_banks[1] = value & 0x0F,
            0xC000 => self.prg_banks[2] = value & 0x0F,
            0xE000 => self.chr_lo = value & 0x0F,
            0xF000 => self.chr_hi = value & 0x0F,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => {
                let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
                let bank = (((self.chr_lo_msb as usize) << 4) | (self.chr_lo as usize)) % total_4k;
                self.chr_rom[(bank * CHR_BANK_4K + addr as usize) % self.chr_rom.len()]
            }
            0x1000..=0x1FFF => {
                let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
                let bank = (((self.chr_hi_msb as usize) << 4) | (self.chr_hi as usize)) % total_4k;
                self.chr_rom[(bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr_rom.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let len = self.chr_rom.len();
                    self.chr_rom[addr as usize % len] = value;
                }
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
        let mut out = Vec::with_capacity(16 + self.vram.len());
        out.push(1u8);
        out.extend_from_slice(&self.prg_banks);
        out.push(self.chr_lo);
        out.push(self.chr_hi);
        out.push(self.chr_lo_msb);
        out.push(self.chr_hi_msb);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 9 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_banks.copy_from_slice(&data[1..4]);
        self.chr_lo = data[4];
        self.chr_hi = data[5];
        self.chr_lo_msb = data[6];
        self.chr_hi_msb = data[7];
        self.mirroring = match data[8] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
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
    fn vrc1_basic_banking() {
        let mut m = Vrc1::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $E000 is fixed last bank.
        assert_eq!(m.cpu_read(0xE000), 7);
    }
}
