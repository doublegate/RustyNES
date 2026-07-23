//! Irem, Napoleon Senki (mapper 77).
//!
//! A one-register board whose interest is its memory layout rather than its
//! logic: CHR banks at 2 KiB granularity, and four nametables of *real* VRAM
//! wired on the cartridge. That makes its nametable handling four-screen
//! rather than the usual mirrored pair -- the console's own 2 KiB of CIRAM is
//! bypassed for the upper half.
//!
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

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_2K: usize = 0x0800;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable helper (mirrors the one in the other simple-mapper modules).
// ---------------------------------------------------------------------------

/// Mapper 77 (Irem, Napoleon Senki).
pub struct Irem77 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    /// CHR RAM for $0800-$1FFF (6 KiB).
    chr_ram: Box<[u8]>,
    /// 4 KiB on-cart nametable RAM (four 1 KiB screens).
    nt_ram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
}

impl Irem77 {
    /// Construct a new mapper 77 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 77 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 77 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            chr_ram: vec![0u8; 0x1800].into_boxed_slice(), // $0800-$1FFF = 6 KiB
            nt_ram: vec![0u8; 4 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
        })
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    /// Map a $2000-$3EFF nametable address to a 4 KiB on-cart RAM offset.
    const fn nt_offset(addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as usize;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        table * NAMETABLE_SIZE + local
    }
}

impl Mapper for Irem77 {
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
            // Bus conflict: AND with the underlying PRG byte.
            let effective = value & self.read_prg(addr);
            self.prg_bank = effective & 0x0F;
            self.chr_bank = (effective >> 4) & 0x0F;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x07FF => {
                // Bottom 2 KiB: switchable CHR-ROM bank.
                let count = (self.chr_rom.len() / CHR_BANK_2K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_2K + addr as usize]
            }
            0x0800..=0x1FFF => self.chr_ram[addr as usize - 0x0800],
            0x2000..=0x3EFF => self.nt_ram[Self::nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        // $0000-$07FF is CHR-ROM (read-only); everything else is RAM.
        match addr {
            0x0800..=0x1FFF => self.chr_ram[addr as usize - 0x0800] = value,
            0x2000..=0x3EFF => self.nt_ram[Self::nt_offset(addr)] = value,
            _ => {}
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        // Consume the nametable read from on-cart 4-screen RAM.
        Some(self.nt_ram[Self::nt_offset(addr)])
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        self.nt_ram[Self::nt_offset(addr)] = value;
        true
    }

    fn current_mirroring(&self) -> Mirroring {
        Mirroring::FourScreen
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.chr_ram.len() + self.nt_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.nt_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.chr_ram.len() + self.nt_ram.len();
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
        let mut cursor = 3;
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.nt_ram
            .copy_from_slice(&data[cursor..cursor + self.nt_ram.len()]);
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

    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m77_prg_and_2k_chr_with_bus_conflict() {
        let mut m = Irem77::new(synth_prg_32k(4), synth_chr_2k(16)).unwrap();
        // Write to $8001 (byte 0xFF, transparent). [CCCC PPPP] = 0b0011_0010.
        // PRG = 2, CHR (2 KiB at $0000) = 3.
        m.cpu_write(0x8001, 0b0011_0010);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::FourScreen);
    }

    #[test]
    fn m77_chr_ram_and_four_screen_nt() {
        let mut m = Irem77::new(synth_prg_32k(2), synth_chr_2k(8)).unwrap();
        // $0800-$1FFF is CHR-RAM.
        m.ppu_write(0x0800, 0xAB);
        assert_eq!(m.ppu_read(0x0800), 0xAB);
        // Four independent nametables in on-cart RAM via the hooks.
        assert!(m.nametable_write(0x2000, 0x11));
        assert!(m.nametable_write(0x2400, 0x22));
        assert!(m.nametable_write(0x2800, 0x33));
        assert!(m.nametable_write(0x2C00, 0x44));
        assert_eq!(m.nametable_fetch(0x2000), Some(0x11));
        assert_eq!(m.nametable_fetch(0x2400), Some(0x22));
        assert_eq!(m.nametable_fetch(0x2800), Some(0x33));
        assert_eq!(m.nametable_fetch(0x2C00), Some(0x44));
    }
}
