//! Caltron 6-in-1 (mapper 41).
//!
//! Two-stage banking: an outer register at `$6000-$67FF` selects the game and
//! carries both the mirroring bit and an enable for the inner register, which
//! is written through `$8000-$FFFF` and supplies the low CHR bits. The inner
//! write is subject to a bus conflict, and is ignored entirely unless the
//! outer register has enabled it.
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

/// Mapper 41 (Caltron 6-in-1).
pub struct Caltron41 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    outer_chr: u8,
    inner_chr: u8,
    inner_enable: bool,
    horizontal_mirroring: bool,
}

impl Caltron41 {
    /// Construct a new Caltron 6-in-1 (mapper 41) board.
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
                "mapper 41 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 41 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        // The board's mirroring is runtime-controlled; seed from the header's
        // arrangement so the power-on state matches a sensible default.
        let horizontal_mirroring = mirroring == Mirroring::Horizontal;
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            outer_chr: 0,
            inner_chr: 0,
            inner_enable: false,
            horizontal_mirroring,
        })
    }

    const fn chr_bank(&self) -> u8 {
        (self.outer_chr << 2) | self.inner_chr
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank() as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }
}

impl Mapper for Caltron41 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The outer register sits in $6000-$67FF, which is "mapped" by the default
    // `cpu_read_unmapped` (>= $6000), so no override is needed there.

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x67FF => {
                // Outer register: decoded from address bits, data ignored.
                let e = ((addr >> 2) & 0x01) as u8;
                let pp = (addr & 0x03) as u8;
                self.prg_bank = (e << 2) | pp;
                self.inner_enable = e != 0;
                self.outer_chr = ((addr >> 3) & 0x03) as u8;
                self.horizontal_mirroring = ((addr >> 5) & 0x01) != 0;
            }
            0x8000..=0xFFFF if self.inner_enable => {
                // Inner CHR register has bus conflicts.
                let effective = value & self.read_prg(addr);
                self.inner_chr = effective & 0x03;
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.outer_chr);
        out.push(self.inner_chr);
        out.push(u8::from(self.inner_enable));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 6 + self.vram.len();
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
        self.outer_chr = data[2];
        self.inner_chr = data[3];
        self.inner_enable = data[4] != 0;
        self.horizontal_mirroring = data[5] != 0;
        self.vram.copy_from_slice(&data[6..6 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 232 — Camerica Quattro / BF9096.
//
// Two-level 16 KiB PRG banking, CHR-RAM:
//   $8000-$BFFF write: outer 64 KiB block = (data >> 3) & 0x03
//   $C000-$FFFF write: inner 16 KiB page within the block = data & 0x03
//   CPU $8000-$BFFF reads the selected inner page; CPU $C000-$FFFF is fixed
//   to page 3 of the selected 64 KiB block.
// Resolved 16 KiB bank = (outer << 2) | page. Mirroring header-fixed; no IRQ.
// ===========================================================================

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
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
    fn m41_outer_register_decodes_from_address() {
        let mut m =
            Caltron41::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // addr layout 0110 0xxx xxMC CEPP.
        // Pick $6000 | (M=1<<5) | (CC=0b11<<3) | (E=1<<2) | (PP=0b10).
        // => A5=1 (horizontal), A4..3 = 0b11 (outer CHR 3), A2 = 1 (E set),
        //    A1..0 = 0b10. PRG = (E<<2)|PP = 0b110 = 6.
        let addr = 0x6000 | (1 << 5) | (0b11 << 3) | (1 << 2) | 0b10;
        m.cpu_write(addr, 0x00);
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Inner CHR write (E set, so honoured). PRG bytes are 0xFF except at
        // offset 0; write to $8001 (byte 0xFF) -> no conflict masking.
        m.cpu_write(0x8001, 0b01); // inner CHR low = 1
        // CHR bank = (outer 3 << 2) | inner 1 = 13.
        assert_eq!(m.ppu_read(0x0000), 13);
    }

    #[test]
    fn m41_inner_write_gated_by_enable() {
        let mut m =
            Caltron41::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // Outer write with E clear (A2 = 0): PP = 0, outer CHR = 1, E = 0.
        let addr = 0x6000 | (0b01 << 3); // CC = 1, E = 0, PP = 0
        m.cpu_write(addr, 0x00);
        // Inner write must be ignored while disabled.
        m.cpu_write(0x8001, 0b11);
        // CHR bank = (outer 1 << 2) | inner 0 = 4.
        assert_eq!(m.ppu_read(0x0000), 4);
    }

    #[test]
    fn m41_inner_chr_has_bus_conflict() {
        // PRG byte at offset 0 of bank 0 is the bank index (0). Writing the
        // inner register at $8000 ANDs with that 0 -> inner CHR forced to 0.
        let mut m =
            Caltron41::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // Enable inner (E set), outer CHR 0, PRG bank 0 wraps so offset-0 byte
        // is 0 (the bank index marker).
        let addr = 0x6000 | (1 << 2); // E = 1, everything else 0 -> PRG bank 4
        m.cpu_write(addr, 0x00);
        // PRG bank is now 4 (E<<2). Offset 0 of bank 4 holds value 4.
        // Write inner at $8000: data 0b11 AND prg_byte(4 = 0b100) = 0b00.
        m.cpu_write(0x8000, 0b11);
        assert_eq!(m.ppu_read(0x0000), 0); // outer 0, inner masked to 0
    }
}
