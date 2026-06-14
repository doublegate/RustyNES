//! Sunsoft-2 IC on the Sunsoft-3 board (iNES mapper 89).
//!
//! The one-screen-mirroring, CHR-ROM-banking variant of the Sunsoft-2 IC
//! (Tenka no Goikenban: Mito Koumon). A single `$8000-$FFFF` write register
//! switches a 16 KiB PRG bank at `$8000-$BFFF`, an 8 KiB CHR bank, and the
//! one-screen mirroring select. The last 16 KiB PRG bank is fixed at
//! `$C000-$FFFF`.
//!
//! Register (nesdev `INES_Mapper_089.xhtml`), `$8000-$FFFF`:
//!
//! ```text
//!   [CPPP MCCC]  C = CHR 8 KiB bank high bit / A16 (bit 7)
//!               PPP = PRG 16 KiB bank (bits 4-6)
//!               M = one-screen mirroring select (bit 3: 0 = A, 1 = B)
//!               CCC = CHR 8 KiB bank low 3 bits (bits 0-2)
//!                   -> chr bank = ((v >> 7) & 1) << 3 | (v & 7)
//! ```
//!
//! There is **no IRQ**.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Sunsoft-2 IC (iNES mapper 89).
pub struct Sunsoft2 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sunsoft2 {
    /// Construct a new Sunsoft-2 (mapper 89) board.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB. CHR-RAM is selected when
    /// `chr_rom` is empty; otherwise CHR-ROM length must be a multiple of 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-2 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_8K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-2 expects an 8 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        // Power-on mirroring select defaults to single-screen A; the header
        // arrangement is ignored on this one-screen board.
        let _ = mirroring;
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring: Mirroring::SingleScreenA,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let bank_count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = if addr < 0xC000 {
            (self.prg_bank as usize) % bank_count
        } else {
            bank_count - 1 // fixed last 16 KiB
        };
        let off = (addr as usize) & (PRG_BANK_16K - 1);
        self.prg_rom[bank * PRG_BANK_16K + off]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let len = self.chr.len().max(1);
        let bank = (self.chr_bank as usize) * CHR_BANK_8K;
        (bank + (addr as usize)) % len
    }
}

impl Mapper for Sunsoft2 {
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
            // Register layout `CPPP MCCC` (nesdev INES_Mapper_089; matches
            // Mesen2 `Sunsoft89::WriteRegister`): bit 7 is the CHR-bank high
            // bit (A16), bit 3 is the one-screen mirroring select.
            self.prg_bank = (value >> 4) & 0x07;
            self.chr_bank = (((value >> 7) & 0x01) << 3) | (value & 0x07);
            self.mirroring = if (value & 0x08) != 0 {
                Mirroring::SingleScreenB
            } else {
                Mirroring::SingleScreenA
            };
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr[off] = value;
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

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 89,
            name: "Sunsoft-2 (89)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG16k".into(), format!("{:#04x}", self.prg_bank)));
        info.chr_banks
            .push(("CHR8k".into(), format!("{:#04x}", self.chr_bank)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            4 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(match self.mirroring {
            Mirroring::SingleScreenB => 1,
            _ => 0,
        });
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 4 + self.vram.len() + need_chr;
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
        self.mirroring = if data[3] != 0 {
            Mirroring::SingleScreenB
        } else {
            Mirroring::SingleScreenA
        };
        let mut cursor = 4;
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

    fn synth_prg(banks_16k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_16k * PRG_BANK_16K];
        for b in 0..banks_16k {
            v[b * PRG_BANK_16K] = b as u8;
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
    fn prg_bank_and_fixed_tail() {
        let mut m = Sunsoft2::new(synth_prg(8), synth_chr(4), Mirroring::Vertical).unwrap();
        // Default PRG bank 0 at $8000, fixed {-1}=7 at $C000.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7);
        // PRG bits 4-6: value 0x30 -> bank 3.
        m.cpu_write(0x8000, 0x30);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn chr_bank_combines_a16() {
        let mut m = Sunsoft2::new(synth_prg(2), synth_chr(16), Mirroring::Vertical).unwrap();
        // CHR high bit / A16 is value bit 7: 0b1000_0010 -> ((1)<<3)|(2) = 10.
        m.cpu_write(0x8000, 0b1000_0010);
        assert_eq!(m.ppu_read(0x0000), 10);
        // Low 3 only: 0b0000_0101 -> 5.
        m.cpu_write(0x8000, 0b0000_0101);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    #[test]
    fn one_screen_mirroring_select() {
        let mut m = Sunsoft2::new(synth_prg(2), synth_chr(4), Mirroring::Vertical).unwrap();
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0x8000, 0x08); // bit 3 -> SingleScreenB
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x8000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Sunsoft2::new(synth_prg(8), synth_chr(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0b1011_1010);
        let blob = m.save_state();
        let mut m2 = Sunsoft2::new(synth_prg(8), synth_chr(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}
