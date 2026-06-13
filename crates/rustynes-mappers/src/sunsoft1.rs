//! Sunsoft-1 (iNES mapper 184) implementation.
//!
//! An early Sunsoft discrete board (Atlantis no Nazo, Kid Niki / Wanpaku
//! Kokkun no Gourmet World, The Wing of Madoola, Fuun Shaolin Kyo). PRG-ROM is
//! fixed (NROM-style 16/32 KiB). A single register in the `$6000-$7FFF` window
//! switches **two 4 KiB CHR banks** — one at PPU `$0000`, one at `$1000`:
//!
//! ```text
//!   $6000-$7FFF [.HHH .LLL]  L = CHR 4 KiB bank @ $0000 (bits 0-2)
//!                            H = CHR 4 KiB bank @ $1000 (bits 4-6)
//! ```
//!
//! The high 4 KiB bank's selectable range is board-specific; the common decode
//! used by every licensed game treats it as a 3-bit field `(v >> 4) & 7`.
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
const CHR_BANK_4K: usize = 0x1000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Sunsoft-1 mapper (iNES mapper 184).
pub struct Sunsoft1 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// CHR 4 KiB bank at $0000 and at $1000.
    chr_bank: [u8; 2],
    mirroring: Mirroring,
}

impl Sunsoft1 {
    /// Construct a new Sunsoft-1 (mapper 184) board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when the PRG-ROM is not 16/32 KiB or
    /// CHR-ROM is empty / not a multiple of 4 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.len() != PRG_BANK_16K && prg_rom.len() != PRG_BANK_32K {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-1 expects 16 or 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || chr_rom.len() % CHR_BANK_4K != 0 {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-1 expects non-empty CHR-ROM in 4 KiB units, got {} bytes",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: [0, 0],
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

    fn chr_offset(&self, addr: u16) -> usize {
        let len = self.chr_rom.len().max(1);
        let half = (addr >> 12) & 0x01; // 0 = $0000, 1 = $1000
        let bank = (self.chr_bank[half as usize] as usize) * CHR_BANK_4K;
        (bank + (addr as usize & (CHR_BANK_4K - 1))) % len
    }
}

impl Mapper for Sunsoft1 {
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
            self.chr_bank[0] = value & 0x07;
            self.chr_bank[1] = (value >> 4) & 0x07;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
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
            mapper_id: 184,
            name: "Sunsoft-1 (184)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.chr_banks
            .push(("CHR4k@0".into(), format!("{:#04x}", self.chr_bank[0])));
        info.chr_banks
            .push(("CHR4k@1".into(), format!("{:#04x}", self.chr_bank[1])));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.chr_bank);
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
        self.chr_bank.copy_from_slice(&data[1..3]);
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
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

    fn synth_chr_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_4K];
        for b in 0..banks {
            v[b * CHR_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn two_4k_chr_banks() {
        let mut m = Sunsoft1::new(
            synth_prg(PRG_BANK_32K),
            synth_chr_4k(8),
            Mirroring::Vertical,
        )
        .unwrap();
        // low = bits 0-2, high = bits 4-6. value 0b0101_0011 -> low=3, high=5.
        m.cpu_write(0x6000, 0b0101_0011);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.ppu_read(0x1000), 5);
    }

    #[test]
    fn write_outside_window_ignored() {
        let mut m = Sunsoft1::new(
            synth_prg(PRG_BANK_32K),
            synth_chr_4k(8),
            Mirroring::Vertical,
        )
        .unwrap();
        m.cpu_write(0x6000, 0b0010_0001); // low=1, high=2
        assert_eq!(m.ppu_read(0x0000), 1);
        assert_eq!(m.ppu_read(0x1000), 2);
        m.cpu_write(0x8000, 0b0111_0111); // ROM write — must not change banks
        assert_eq!(m.ppu_read(0x0000), 1);
        assert_eq!(m.ppu_read(0x1000), 2);
    }

    #[test]
    fn prg_16k_mirrors() {
        let mut m = Sunsoft1::new(
            synth_prg(PRG_BANK_16K),
            synth_chr_4k(4),
            Mirroring::Vertical,
        )
        .unwrap();
        assert_eq!(m.cpu_read(0x8000), m.cpu_read(0xC000));
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Sunsoft1::new(
            synth_prg(PRG_BANK_32K),
            synth_chr_4k(8),
            Mirroring::Vertical,
        )
        .unwrap();
        m.cpu_write(0x6000, 0b0110_0010);
        let blob = m.save_state();
        let mut m2 = Sunsoft1::new(
            synth_prg(PRG_BANK_32K),
            synth_chr_4k(8),
            Mirroring::Vertical,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.ppu_read(0x1000), m2.ppu_read(0x1000));
    }
}
