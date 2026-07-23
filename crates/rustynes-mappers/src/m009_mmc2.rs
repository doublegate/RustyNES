//! Nintendo MMC2 (`PxROM`, mapper 9) -- Punch-Out!!
//!
//! The defining feature is a *tile-fetch CHR latch*: the PPU pattern-table
//! address the cartridge sees during rendering selects which of two banked CHR
//! windows stays mapped. Fetching tile `$FD` or `$FE` from a pattern half
//! latches that half to one of two banks, so the mapper switches CHR banks
//! mid-scanline with no CPU involvement -- the trick Punch-Out!! uses to draw a
//! large animated opponent out of a small CHR ROM. That requires a hook on PPU
//! pattern fetches, unlike every other board in this size class.
//!
//! PRG is 8 KiB switchable at `$8000` plus three fixed banks. The closely
//! related MMC4 is in `m010_mmc4.rs` -- same CHR latch, different PRG
//! granularity.
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

/// MMC2 (Mapper 9).
pub struct Mmc2 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_lo_fd: u8,
    chr_lo_fe: u8,
    chr_hi_fd: u8,
    chr_hi_fe: u8,
    /// `false` -> use the FD bank for window 0 (`$0000-$0FFF`).
    latch_lo_fe: bool,
    /// `false` -> use the FD bank for window 1 (`$1000-$1FFF`).
    latch_hi_fe: bool,
    mirroring: Mirroring,
}

impl Mmc2 {
    /// Construct a new MMC2 mapper.
    ///
    /// PRG must be a non-zero multiple of 8 KiB; CHR-ROM is mandatory and
    /// must be a multiple of 4 KiB.
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
                "MMC2 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "MMC2 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_lo_fd: 0,
            chr_lo_fe: 0,
            chr_hi_fd: 0,
            chr_hi_fe: 0,
            latch_lo_fe: false,
            latch_hi_fe: false,
            mirroring,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = self.prg_rom.len() / PRG_BANK_8K;
        let last3 = total_8k.saturating_sub(3);
        let last2 = total_8k.saturating_sub(2);
        let last1 = total_8k.saturating_sub(1);
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_bank as usize) % total_8k.max(1),
            0xA000 => last3,
            0xC000 => last2,
            _ => last1, // $E000 + the implicit fallback
        };
        bank * PRG_BANK_8K + ((addr as usize) & 0x1FFF)
    }

    fn chr_offset(&mut self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
        let bank = if addr < CHR_BANK_4K {
            let b = if self.latch_lo_fe {
                self.chr_lo_fe
            } else {
                self.chr_lo_fd
            };
            (b as usize) % total_4k
        } else {
            let b = if self.latch_hi_fe {
                self.chr_hi_fe
            } else {
                self.chr_hi_fd
            };
            (b as usize) % total_4k
        };
        bank * CHR_BANK_4K + (addr & (CHR_BANK_4K - 1))
    }

    /// Update the CHR latch based on the fetched pattern address.
    /// $0FD8-$0FDF -> window 0 latch FD; $0FE8-$0FEF -> window 0 latch FE;
    /// similarly $1FD8-$1FDF / $1FE8-$1FEF for window 1.  Per nesdev wiki.
    fn update_latch(&mut self, addr: u16) {
        match addr & 0x3FF8 {
            0x0FD8 => self.latch_lo_fe = false,
            0x0FE8 => self.latch_lo_fe = true,
            0x1FD8 => self.latch_hi_fe = false,
            0x1FE8 => self.latch_hi_fe = true,
            _ => {}
        }
    }
}

impl Mapper for Mmc2 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xF000 {
            0xA000 => self.prg_bank = value & 0x0F,
            0xB000 => self.chr_lo_fd = value & 0x1F,
            0xC000 => self.chr_lo_fe = value & 0x1F,
            0xD000 => self.chr_hi_fd = value & 0x1F,
            0xE000 => self.chr_hi_fe = value & 0x1F,
            0xF000 => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                let v = self.chr_rom[off % self.chr_rom.len()];
                self.update_latch(addr);
                v
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
                    let off = self.chr_offset(addr);
                    let len = self.chr_rom.len();
                    self.chr_rom[off % len] = value;
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
        out.push(1u8); // version
        out.push(self.prg_bank);
        out.push(self.chr_lo_fd);
        out.push(self.chr_lo_fe);
        out.push(self.chr_hi_fd);
        out.push(self.chr_hi_fe);
        out.push(u8::from(self.latch_lo_fe));
        out.push(u8::from(self.latch_hi_fe));
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
        self.prg_bank = data[1];
        self.chr_lo_fd = data[2];
        self.chr_lo_fe = data[3];
        self.chr_hi_fd = data[4];
        self.chr_hi_fe = data[5];
        self.latch_lo_fe = data[6] != 0;
        self.latch_hi_fe = data[7] != 0;
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
    fn mmc2_swap_window_via_latch() {
        let mut m = Mmc2::new(synth(8), synth_chr_4k(4), Mirroring::Vertical).unwrap();
        m.chr_lo_fd = 0;
        m.chr_lo_fe = 1;
        // Default latch is FD -> bank 0 byte 0 = 0.
        assert_eq!(m.ppu_read(0x0000), 0);
        // Reading the FE sentinel switches to FE bank.
        let _ = m.ppu_read(0x0FE8);
        assert_eq!(m.ppu_read(0x0000), 1);
    }
}
