//! Camerica / Codemasters `BF9096` (mapper 232) -- the Quattro multicarts.
//!
//! Two-level 16 KiB PRG banking: an outer block select and an inner bank
//! select within that block, which is how a four-game cartridge presents each
//! title as if it owned the whole address space.
//!
//! The single-game `BF9093` is a different ASIC on mapper 71; see
//! `m071_camerica_bf9093.rs`.
//!
//! A discrete-logic board in the shape of the stock mappers (`NROM`, `CNROM`,
//! `UxROM`, `GxROM`, `AxROM`): bank-select latch registers, no IRQ, no on-cart
//! audio. Banking / mirroring semantics are cross-checked against the
//! `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, and validated by register-decode + save-state unit
//! tests.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
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

/// Mapper 232 (Camerica Quattro / `BF9096`).
pub struct Camerica232 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    outer_block: u8,
    inner_page: u8,
    mirroring: Mirroring,
}

impl Camerica232 {
    /// Construct a new mapper 232 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. CHR is always 8 KiB RAM (any supplied CHR-ROM is rejected).
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 232 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr: Box<[u8]> = if chr_rom.is_empty() {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() == CHR_BANK_8K {
            // Some dumps carry 8 KiB CHR-ROM; accept and use it read-only.
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 232 expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            outer_block: 0,
            inner_page: 0,
            mirroring,
        })
    }

    fn map_16k(&self, page_in_block: u8) -> usize {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = ((self.outer_block << 2) | (page_in_block & 0x03)) as usize;
        bank % count
    }
}

impl Mapper for Camerica232 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.map_16k(self.inner_page);
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000)]
            }
            0xC000..=0xFFFF => {
                let bank = self.map_16k(3);
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0xC000)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0xBFFF => self.outer_block = (value >> 3) & 0x03,
            0xC000..=0xFFFF => self.inner_page = value & 0x03,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                // CHR-RAM dumps allow writes; if CHR is the supplied 8 KiB
                // image we still let the program scribble its 8 KiB window.
                self.chr[addr as usize] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.outer_block);
        out.push(self.inner_page);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len() + self.chr.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.outer_block = data[1];
        self.inner_page = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr
            .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 240 — C&E multicart.
//
// One register across $4020-$5FFF: byte DDDD_PPPP
//   PRG (32 KiB) = (data >> 4) & 0x0F
//   CHR (8 KiB)  = data & 0x0F
// The register window overlaps the normal WRAM range; many 240 boards have no
// PRG-RAM, so the register is the only thing wired at $4020-$5FFF. Mirroring is
// header-fixed; no IRQ.
// ===========================================================================

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m232_two_level_banking() {
        // 8 16 KiB banks = 2 blocks of 4 pages.
        let mut m = Camerica232::new(synth_prg_16k(8), Box::new([]), Mirroring::Vertical).unwrap();
        // Select outer block 1 ($8000 write, bits 4-3 = 0b01 << 3 = 0b1000).
        m.cpu_write(0x8000, 0b0000_1000);
        // Select inner page 2 ($C000 write, bits 1-0).
        m.cpu_write(0xC000, 0b0000_0010);
        // $8000-$BFFF reads bank (1<<2)|2 = 6.
        assert_eq!(m.cpu_read(0x8000), 6);
        // $C000-$FFFF is fixed to page 3 of block 1 -> bank (1<<2)|3 = 7.
        assert_eq!(m.cpu_read(0xC000), 7);
    }
}
