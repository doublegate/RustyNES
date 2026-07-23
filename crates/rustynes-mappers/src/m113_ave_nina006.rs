//! AVE `NINA-006` / `MB-91` multicart (mapper 113).
//!
//! Mapper 79 (`m079_ave_nina03_06.rs`) plus a register-controlled mirroring
//! bit and a wider CHR field -- the additions a multicart needs, since each
//! bundled game may want different mirroring and the combined CHR is larger
//! than any single title's. Same A8 decode in the `$4100-$5FFF` window.
//!
//! A discrete-logic board in the shape of the stock mappers (`NROM`, `CNROM`,
//! `UxROM`, `GxROM`, `AxROM`): bank-select latch registers, no IRQ, no on-cart
//! audio. Banking / mirroring semantics are cross-checked against the
//! `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, and validated by register-decode + save-state unit
//! tests.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::bool_to_int_with_if,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::unreadable_literal
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Mapper 113 (`NINA-006`/`MB-91` multicart).
pub struct Nina006M113 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    vertical_mirroring: bool,
}

impl Nina006M113 {
    /// Construct a new mapper 113 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 113 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 113 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            vertical_mirroring: false,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Nina006M113 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x4100..=0x5FFF).contains(&addr) && (addr & 0x0100) != 0 {
            self.prg_bank = (value >> 3) & 0x07;
            self.chr_bank = (value & 0x07) | ((value >> 3) & 0x08);
            self.vertical_mirroring = (value & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
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
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.vertical_mirroring {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.vertical_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 4 + self.vram.len() + chr_extra;
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
        self.vertical_mirroring = data[3] != 0;
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
mod tests {
    use super::*;

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m113_decodes_prg_chr_and_mirroring() {
        let mut m = Nina006M113::new(synth_prg_32k(4), synth_chr_8k(16)).unwrap();
        // value 0b1100_0010 (0xC2):
        //   PRG = (v>>3)&7 = (24)&7 = 0
        //   CHR = (v&7) | ((v>>3)&8) = 2 | (24 & 8 = 8) = 10
        //   mirroring bit7 set -> vertical.
        m.cpu_write(0x4100, 0b1100_0010);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 10);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // Clear bit 7 -> horizontal.
        m.cpu_write(0x4100, 0b0000_0001);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }
}
