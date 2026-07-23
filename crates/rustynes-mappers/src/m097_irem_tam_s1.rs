//! Irem `TAM-S1`, Kaiketsu Yanchamaru (mapper 97).
//!
//! Inverts the normal `UxROM` arrangement: the *first* 16 KiB is fixed and the
//! *second* switchable. That matters because the 6502 reset and interrupt
//! vectors live at the top of the address space -- on this board they sit in
//! the switchable half, so the bank in place at reset is load-bearing.//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the reference emulators (`Mesen2`, `GeraNES`) and the nesdev wiki, with no
//! commercial-oracle ROM in the tree. Banking math is direct slice indexing and
//! every bank select wraps with `% count`, so a register write can never index
//! out of bounds -- required for the `#![no_std]` chip stack, which cannot
//! afford a panic on a register access.
//!
//! See `tier.rs` (`MapperTier::BestEffort`), `docs/adr/0011-mapper-tiering.md`,
//! and `docs/mappers.md` §Mapper coverage matrix.

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

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable helper (mirrors the one in the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

// ===========================================================================
// Mapper 15 — K-1029 / 100-in-1 Contra Function 16.
//
// Single register decoded across $8000-$FFFF (data + low two address bits):
//   addr bits 0-1 select the banking MODE; data holds the PRG bank, a CHR-RAM
//   mirroring bit (bit 6) and a "half-bank" bit (bit 7).
//     mode 0: 32 KiB at the 16 KiB granularity, second half = bank|1
//     mode 1: 128 KiB? upper half forced to bank|7 (UNROM-like fixed top)
//     mode 2: 8 KiB-granular ((bank<<1)|b) mirrored across the whole window
//     mode 3: single 16 KiB bank mirrored across the whole window
//   CHR is always 8 KiB RAM; CHR writes are protected in modes 0 and 3.
//   mirroring: data bit 6 (1 = horizontal, 0 = vertical). No IRQ.
// ===========================================================================

/// Mapper 97 (Irem `TAM-S1`, Kaiketsu Yanchamaru).
pub struct Irem97 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    vertical_mirroring: bool,
}

impl Irem97 {
    /// Construct a new mapper 97 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 97 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
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
                "mapper 97 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            vertical_mirroring: mirroring == Mirroring::Vertical,
        })
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }
}

impl Mapper for Irem97 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // $8000-$BFFF fixed to the last 16 KiB bank.
            0x8000..=0xBFFF => {
                let last = self.prg_count_16k() - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize - 0x8000)]
            }
            // $C000-$FFFF switchable.
            0xC000..=0xFFFF => {
                let bank = (self.prg_bank as usize) % self.prg_count_16k();
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0xC000)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value & 0x1F;
            self.vertical_mirroring = (value & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
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
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.vertical_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + self.vram.len() + chr_extra;
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
        self.vertical_mirroring = data[2] != 0;
        let mut cursor = 3;
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

// ===========================================================================
// Mapper 132 — TXC 22211.
//
// Driven by the TXC scrambling-accumulator chip (the non-JV001 variant). The
// chip has four internal registers written via $4100-$4103 (decoded on
// addr & 0xE103) and an output latch updated on any $8000-$FFFF write:
//   output = (accumulator & 0x0F) | ((inverter & 0x08) << 1)
// The mapper then resolves:
//   PRG (32 KiB) = (output >> 2) & 0x01
//   CHR (8 KiB)  =  output       & 0x03
// `readMapperRegister` at $4100|$4103==0x4100 returns the chip read value in
// the low nibble. Mirroring header-fixed; no IRQ.
// ===========================================================================

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
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
    fn m97_fixed_first_switchable_second() {
        let mut m = Irem97::new(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Horizontal).unwrap();
        // $8000-$BFFF fixed to last bank (7).
        assert_eq!(m.cpu_read(0x8000), 7);
        // Switch $C000 bank to 3, set vertical mirroring (bit 7).
        m.cpu_write(0x8000, 0b1000_0011);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // $8000 still fixed.
        assert_eq!(m.cpu_read(0x8000), 7);
    }
}
