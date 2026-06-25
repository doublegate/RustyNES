//! `UxROM` (iNES mapper 2) implementation.
//!
//! `UxROM` (`UNROM`, `UOROM`, ...) has a switchable 16 KiB PRG bank at
//! `$8000-$BFFF` and a fixed last-bank window at `$C000-$FFFF`. Standard
//! `UxROM` ships CHR-RAM only (8 KiB). Mirroring comes from the iNES
//! header and never changes at runtime. No IRQ.
//!
//! See `docs/mappers.md` §Mapper coverage matrix for the canonical reference.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// `UxROM` mapper.
pub struct UxRom {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    bank: u8,
    mirroring: Mirroring,
}

impl UxRom {
    /// Construct a new `UxROM` mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB. CHR-RAM is selected
    /// when `chr_rom` is empty; otherwise CHR-ROM length must be 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the
    /// constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "UxROM PRG-ROM size {} is not a non-zero multiple of 16 KiB",
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
                "UxROM expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            bank: 0,
            mirroring,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for UxRom {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank_count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let bank = (self.bank as usize) % bank_count;
                let off = (addr - 0x8000) as usize;
                self.prg_rom[bank * PRG_BANK_16K + off]
            }
            0xC000..=0xFFFF => {
                let bank_count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let last = bank_count - 1;
                let off = (addr - 0xC000) as usize;
                self.prg_rom[last * PRG_BANK_16K + off]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if let 0x8000..=0xFFFF = addr {
            // Standard UxROM uses the low 4 bits (16-bank max). UOROM uses
            // 5 bits (32-bank max). Submapper 2 specifies this exactly,
            // but for our purposes masking to a power of two derived from
            // the PRG length is robust.
            let bank_count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            // Find bit width that fits bank_count; saturate at 8 bits since
            // the bank register is u8.
            let mask = u8::try_from((bank_count - 1) | 0x0F).unwrap_or(u8::MAX);
            self.bank = value & mask;
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        // UxROM CHR is unbanked; offset == address. Usually CHR-RAM (-> None).
        if self.chr_is_ram {
            None
        } else {
            Some(u32::from(addr & 0x1FFF))
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
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            2 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 2 + self.vram.len() + need_chr;
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

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn uxrom_default_first_bank_zero_last_bank_fixed() {
        let mut m = UxRom::new(synth_prg(8), Box::new([]), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn uxrom_bank_select_switches_first_window() {
        let mut m = UxRom::new(synth_prg(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7); // last fixed
        m.cpu_write(0xFFFF, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn uxrom_chr_ram_round_trip() {
        let mut m = UxRom::new(synth_prg(2), Box::new([]), Mirroring::Vertical).unwrap();
        m.ppu_write(0x0010, 0xAB);
        assert_eq!(m.ppu_read(0x0010), 0xAB);
    }
}
