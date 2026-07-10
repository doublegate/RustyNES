//! `GxROM` (iNES mapper 66) implementation.
//!
//! `GxROM` (`GNROM`, `MHROM`) selects a 32 KiB PRG bank from bits 5-4 of
//! the bank-select write and an 8 KiB CHR-ROM bank from bits 1-0. Bus
//! conflicts are present (per `docs/mappers.md` §Bus conflicts).
//! Mirroring is fixed by the iNES header; no IRQ.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// `GxROM` mapper.
pub struct GxRom {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl GxRom {
    /// Construct a new `GxROM` mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "GxROM PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "GxROM CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let bank_count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % bank_count;
        let off = (addr - 0x8000) as usize;
        self.prg_rom[bank * PRG_BANK_32K + off]
    }
}

impl Mapper for GxRom {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict.
            let prg_byte = self.read_prg(addr);
            let effective = value & prg_byte;
            self.prg_bank = (effective >> 4) & 0x03;
            self.chr_bank = effective & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let bank_count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % bank_count;
                self.chr_rom[bank * CHR_BANK_8K + (addr as usize)]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = self.nametable_offset(addr);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    // GxROM has fixed solder-pad mirroring — a game-DB header correction is valid.
    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
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
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
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
            for o in 0..PRG_BANK_32K {
                v[b * PRG_BANK_32K + o] = if o == 0 { b as u8 } else { 0xFF };
            }
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn gxrom_bank_select_no_conflict() {
        // PRG byte 0xFF, AND with 0x33 = 0x33 -> prg_bank=3, chr_bank=3.
        let mut m = GxRom::new(synth_prg(4), synth_chr(4), Mirroring::Vertical).unwrap();
        // First write goes to a $8001+ address whose PRG byte is 0xFF (we
        // initialize all but the first byte of each bank to 0xFF).
        m.cpu_write(0x8001, 0x33);
        assert_eq!(m.cpu_read(0x8000), 3); // bank 3 marker
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn gxrom_bus_conflict_at_offset_zero() {
        // At offset 0 of bank 0, byte = 0x00. ANDing kills the write.
        let mut m = GxRom::new(synth_prg(4), synth_chr(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0xFF);
        assert_eq!(m.cpu_read(0x8000), 0); // unchanged
    }
}
