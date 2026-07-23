//! Jaleco/Konami JF-05..JF-19 CNROM-style board (iNES mapper 87).
//!
//! A simple discrete-logic CHR-bank-switch board (Argus, Choplifter, The
//! Goonies, City Connection, Field Combat, etc.). PRG-ROM is fixed
//! (16 KiB mirrored or 32 KiB, NROM-style). The single register lives in the
//! `$6000-$7FFF` window (it is wired to the cartridge's PRG-RAM enable area,
//! not the `$8000-$FFFF` ROM space), and selects the 8 KiB CHR bank using a
//! **bit-swapped** 2-bit field:
//!
//! ```text
//!   $6000-$7FFF [.... ..LH]  CHR bank = (H << 0) | (L << 1)
//!                            i.e. ((v >> 1) & 1) | ((v << 1) & 2)
//! ```
//!
//! There is **no IRQ**. Mirroring is fixed from the iNES header.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

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

/// Jaleco/Konami mapper 87.
pub struct Jaleco87 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Jaleco87 {
    /// Construct a new mapper-87 board.
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
                "Mapper-87 expects 16 or 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "Mapper-87 expects non-empty CHR-ROM in 8 KiB units, got {} bytes",
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

impl Mapper for Jaleco87 {
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
        if (0x6000..=0x7FFF).contains(&addr) {
            // The 2-bit CHR-select field is bit-swapped: D0->bank bit 1,
            // D1->bank bit 0.
            self.chr_bank = ((value >> 1) & 0x01) | ((value << 1) & 0x02);
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
        // CHR-ROM writes ignored.
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 87,
            name: "Jaleco/Konami (87)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.chr_banks
            .push(("CHR8k".into(), format!("{:#04x}", self.chr_bank)));
        info
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

    fn synth_prg(bytes: usize) -> Box<[u8]> {
        let mut v = vec![0u8; bytes];
        for (i, b) in v.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
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
    fn bit_swapped_chr_select() {
        let mut m =
            Jaleco87::new(synth_prg(PRG_BANK_32K), synth_chr(4), Mirroring::Vertical).unwrap();
        // value 0b00 -> bank 0
        m.cpu_write(0x6000, 0b00);
        assert_eq!(m.ppu_read(0x0000), 0);
        // value 0b01 (D0 set) -> bank bit 1 set = bank 2
        m.cpu_write(0x6000, 0b01);
        assert_eq!(m.ppu_read(0x0000), 2);
        // value 0b10 (D1 set) -> bank bit 0 set = bank 1
        m.cpu_write(0x6000, 0b10);
        assert_eq!(m.ppu_read(0x0000), 1);
        // value 0b11 -> bank 3
        m.cpu_write(0x6000, 0b11);
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn write_outside_6000_7fff_ignored() {
        let mut m =
            Jaleco87::new(synth_prg(PRG_BANK_32K), synth_chr(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0b01); // bank 2
        assert_eq!(m.ppu_read(0x0000), 2);
        m.cpu_write(0x8000, 0b11); // ROM write must NOT change the bank
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    #[test]
    fn prg_16k_mirrors() {
        let mut m =
            Jaleco87::new(synth_prg(PRG_BANK_16K), synth_chr(2), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), m.cpu_read(0xC000));
    }

    #[test]
    fn save_state_round_trip() {
        let mut m =
            Jaleco87::new(synth_prg(PRG_BANK_32K), synth_chr(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0b11);
        let blob = m.save_state();
        let mut m2 =
            Jaleco87::new(synth_prg(PRG_BANK_32K), synth_chr(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
    }
}
