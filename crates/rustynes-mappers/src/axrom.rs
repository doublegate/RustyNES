//! `AxROM` (iNES mapper 7) implementation.
//!
//! `AxROM` (`AMROM`, `ANROM`, `AOROM`, ...) switches a single 32 KiB PRG bank
//! across `$8000-$FFFF`. Bit 4 of the bank-select write toggles between
//! single-screen-A and single-screen-B mirroring. CHR is always RAM (8 KiB)
//! on stock `AxROM`. No IRQ, no bus conflicts on most variants (`AOROM`
//! technically has them; `AMROM` / `ANROM` do not — we omit conflicts
//! since cleanly-written titles don't depend on them and modeling-them-
//! without-cause makes the much more common `AMROM` / `ANROM` ROMs
//! misbehave on certain edges).

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// `AxROM` mapper.
pub struct AxRom {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    bank: u8,
    mirroring_b: bool,
}

impl AxRom {
    /// Construct a new `AxROM` mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "AxROM PRG-ROM size {} is not a non-zero multiple of 32 KiB",
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
                "AxROM expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            bank: 0,
            mirroring_b: false,
        })
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = usize::from(self.mirroring_b);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for AxRom {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let bank_count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.bank as usize) % bank_count;
            let off = (addr - 0x8000) as usize;
            self.prg_rom[bank * PRG_BANK_32K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.bank = value & 0x07;
            self.mirroring_b = (value & 0x10) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
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
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.mirroring_b {
            Mirroring::SingleScreenB
        } else {
            Mirroring::SingleScreenA
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            3 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.bank);
        out.push(u8::from(self.mirroring_b));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.bank = data[1];
        self.mirroring_b = data[2] != 0;
        let mut cursor = 3;
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

    fn synth_prg(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn axrom_default_bank_zero() {
        let mut m = AxRom::new(synth_prg(4), Box::new([])).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn axrom_bank_select_and_mirroring() {
        let mut m = AxRom::new(synth_prg(4), Box::new([])).unwrap();
        m.cpu_write(0x8000, 0b0001_0010); // bank 2, mirror B
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x8000, 0b0000_0001); // bank 1, mirror A
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn axrom_chr_ram_round_trip() {
        let mut m = AxRom::new(synth_prg(2), Box::new([])).unwrap();
        m.ppu_write(0x0123, 0xAB);
        assert_eq!(m.ppu_read(0x0123), 0xAB);
    }
}
