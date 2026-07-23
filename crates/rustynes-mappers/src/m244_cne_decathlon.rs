//! C&E Decathlon (mapper 244).
//!
//! A value-decoded sibling of the C&E multicart in `m240_cne_multicart.rs`:
//! the *written byte* selects the PRG and CHR banks through lookup tables
//! rather than supplying the bank number directly.//!
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

// ===========================================================================
// Mapper 38 — Bit Corp UNL-PCI556.
//
// Single 8-bit latch at $7000-$7FFF. Low 2 bits select a 32 KiB PRG bank;
// bits 3-2 select an 8 KiB CHR bank. No bus conflicts (the register lives in
// the $6000-$7FFF window, not in PRG-ROM). Mirroring is header-fixed; no IRQ.
// ===========================================================================

/// Mapper 244 (Decathlon).
pub struct Decathlon244 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Decathlon244 {
    /// Construct a new mapper 244 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 244 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 244 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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
}

impl Mapper for Decathlon244 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // Mapper 244 decodes the written DATA byte (not the address) through two
        // scramble LUTs, selecting CHR vs PRG by bit 3:
        //   value & 0x08 != 0 -> CHR 8 KiB = LUT_CHR[(value>>4)&7][value&7]
        //   else              -> PRG 32 KiB = LUT_PRG[(value>>4)&3][value&3]
        // The old code ignored the data byte and decoded address bits with no
        // scramble, so it banked to the wrong PRG/CHR and the menu never drew.
        // (Mesen2 Mapper244 / puNES mapper_244 carry the identical tables.)
        const LUT_PRG: [[u8; 4]; 4] = [[0, 1, 2, 3], [3, 2, 1, 0], [0, 2, 1, 3], [3, 1, 2, 0]];
        const LUT_CHR: [[u8; 8]; 8] = [
            [0, 1, 2, 3, 4, 5, 6, 7],
            [0, 2, 1, 3, 4, 6, 5, 7],
            [0, 1, 4, 5, 2, 3, 6, 7],
            [0, 4, 1, 5, 2, 6, 3, 7],
            [0, 4, 2, 6, 1, 5, 3, 7],
            [0, 2, 4, 6, 1, 3, 5, 7],
            [7, 6, 5, 4, 3, 2, 1, 0],
            [7, 6, 5, 4, 3, 2, 1, 0],
        ];
        if (0x8000..=0xFFFF).contains(&addr) {
            if value & 0x08 != 0 {
                self.chr_bank = LUT_CHR[((value >> 4) & 0x07) as usize][(value & 0x07) as usize];
            } else {
                self.prg_bank = LUT_PRG[((value >> 4) & 0x03) as usize][(value & 0x03) as usize];
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if (0x2000..=0x3EFF).contains(&addr) {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
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

// ===========================================================================
// Mapper 250 — Nitra (Time Diver Avenger).
//
// An MMC3-register-compatible board, but the register index/value normally
// carried in the data byte is instead carried in the *address* bits A0-A7,
// and the data byte is ignored. The effective MMC3 write is:
//   reg select  ($8000-$9FFE, even) : index = A0-A7.
//   reg data    ($8001-$9FFF, odd)  : value = A0-A7.
//   mirroring   ($A000-$BFFE, even) : A0.
// The board provides the MMC3 banking subset (two 8 KiB PRG + the fixed-last
// layout + 2 KiB/1 KiB CHR slots) plus a CPU-cycle (M2) IRQ counter modelled
// like the VRC-style 8-bit reload counter. CHR is ROM.
// ===========================================================================

#[cfg(test)]
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
    fn m244_value_decoded_banks() {
        let mut m =
            Decathlon244::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // PRG select (value & 0x08 == 0): value 0x11 -> LUT_PRG[(1)][1] = 2.
        m.cpu_write(0x8000, 0x11);
        assert_eq!(m.cpu_read(0x8000), 2);
        // value 0x30 -> LUT_PRG[3][0] = 3.
        m.cpu_write(0x8000, 0x30);
        assert_eq!(m.cpu_read(0x8000), 3);
        // CHR select (value & 0x08 != 0): value 0x09 -> LUT_CHR[0][1] = 1.
        m.cpu_write(0x8000, 0x09);
        assert_eq!(m.ppu_read(0x0000), 1);
        // value 0x6E -> LUT_CHR[6][6] = 1 (table row 6 reversed).
        m.cpu_write(0x8000, 0x6E);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m244_save_state_round_trip() {
        let mut m =
            Decathlon244::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x11); // PRG = LUT_PRG[1][1] = 2
        let blob = m.save_state();
        let mut m2 =
            Decathlon244::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }
}
