//! `CNROM` (iNES mapper 3) implementation.
//!
//! `CNROM` has a fixed 16/32 KiB PRG-ROM (`NROM`-style) and a switchable
//! 8 KiB CHR-ROM bank selected by writes to `$8000-$FFFF`. Most `CNROM`
//! cartridges exhibit bus conflicts: the value placed on the data bus by
//! the ROM-resident byte at the written address is `AND`ed with the value
//! the CPU is writing. Per `docs/mappers.md` §Bus conflicts, we model
//! this so affected ROMs (a small but real set) behave correctly.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// `CNROM` mapper.
pub struct CnRom {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl CnRom {
    /// Construct a new `CNROM` mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when the PRG-ROM is not 16/32 KiB or
    /// CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.len() != PRG_BANK_16K && prg_rom.len() != PRG_BANK_32K {
            return Err(MapperError::Invalid(format!(
                "CNROM expects 16 or 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "CNROM expects non-empty CHR-ROM in 8 KiB units, got {} bytes",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
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
        let off = (addr - 0x8000) as usize;
        if self.prg_rom.len() == PRG_BANK_16K {
            self.prg_rom[off & (PRG_BANK_16K - 1)]
        } else {
            self.prg_rom[off]
        }
    }
}

impl Mapper for CnRom {
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
            // Bus conflict: AND the written value with the byte present at
            // the same PRG address.
            let prg_byte = self.read_prg(addr);
            let effective = value & prg_byte;
            // CHR-bank size depends on cart; for stock CNROM it's 8 KiB
            // banks selected by the low 2 bits (some clones use more).
            let bank_count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
            let mask = u8::try_from((bank_count - 1) | 0x03).unwrap_or(u8::MAX);
            self.chr_bank = effective & mask;
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        // CNROM is CHR-ROM only; the same 8 KiB-bank offset `ppu_read` resolves.
        let bank_count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = usize::from(self.chr_bank) % bank_count;
        u32::try_from(bank * CHR_BANK_8K + usize::from(addr & 0x1FFF)).ok()
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
        // CHR-ROM writes ignored.
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    // CNROM has fixed solder-pad mirroring — a game-DB header correction is valid.
    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(bytes: usize, fill: u8) -> Box<[u8]> {
        vec![fill; bytes].into_boxed_slice()
    }

    fn synth_chr(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn cnrom_chr_bank_select_with_no_conflict() {
        let mut m = CnRom::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr(4),
            Mirroring::Vertical,
        )
        .unwrap();
        // PRG byte is 0xFF everywhere, so AND has no effect.
        m.cpu_write(0x8000, 2);
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    #[test]
    fn cnrom_bus_conflict_masks_write_with_prg_byte() {
        // PRG byte at $8000 = 0x01. Writing 0x03 -> conflict yields 0x01.
        let mut prg = vec![0u8; PRG_BANK_32K];
        prg[0] = 0x01;
        let mut m = CnRom::new(prg.into_boxed_slice(), synth_chr(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x03);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn cnrom_save_state_round_trip() {
        let mut m = CnRom::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr(4),
            Mirroring::Vertical,
        )
        .unwrap();
        m.cpu_write(0x8000, 3);
        let blob = m.save_state();
        let mut m2 = CnRom::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr(4),
            Mirroring::Vertical,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), 3);
    }
}
