//! Sachen 8259 ASIC (mapper 137 and the wider 8259 A/B/C/D family).
//!
//! Sachen's reusable bank-select ASIC, and a step up from the company's
//! discrete boards in `sachen_discrete.rs`: instead of a single latch it
//! implements an address-then-data register protocol through the
//! `$4100-$5FFF` window, with eight internal registers. The four die
//! revisions differ only in how the CHR bank bits are shuffled on the way
//! out, which is why the variants share one implementation and differ by a
//! bit-permutation rather than by separate decode paths.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, with no commercial-oracle ROM in the tree. Banking math
//! is direct slice indexing and every bank select wraps with `% count`, so a
//! register write can never index out of bounds -- required for the `#![no_std]`
//! chip stack, which cannot afford a panic on a register access.
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
// Mapper 40 — NTDEC 2722 (Super Mario Bros. 2J pirate conversion).
//
// PRG layout is fixed except for one switchable window:
//   $6000-$7FFF -> 8 KiB bank 6 (a copy of PRG bank 6; some dumps use it as
//                  the "intro" bank — modelled as bank 6 of the image).
//   $8000-$9FFF -> fixed bank 4
//   $A000-$BFFF -> fixed bank 5
//   $C000-$DFFF -> switchable 8 KiB bank (low 3 bits of any $E000-$FFFF write)
//   $E000-$FFFF -> fixed bank 7
// Registers (data ignored; address-decoded):
//   $8000-$9FFF : IRQ disable + acknowledge (counter held in reset).
//   $A000-$BFFF : IRQ enable (counter starts counting M2 cycles).
//   $E000-$FFFF : select the $C000 8 KiB bank (value & 0x07).
// The IRQ counter is a 12-bit M2 counter: once enabled it counts up and, when
// it reaches 4096 (0x1000), asserts the IRQ and holds. CHR is 8 KiB RAM.
// ===========================================================================

const fn byte_to_mirroring(b: u8, fallback: Mirroring) -> Mirroring {
    match b {
        0 => Mirroring::Horizontal,
        1 => Mirroring::Vertical,
        2 => Mirroring::SingleScreenA,
        3 => Mirroring::SingleScreenB,
        4 => Mirroring::FourScreen,
        5 => Mirroring::MapperControlled,
        _ => fallback,
    }
}

const fn mirroring_to_byte(m: Mirroring) -> u8 {
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenA => 2,
        Mirroring::SingleScreenB => 3,
        Mirroring::FourScreen => 4,
        Mirroring::MapperControlled => 5,
    }
}

/// Mapper 137 (Sachen 8259D).
pub struct Sachen8259M137 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    cmd: u8,
    chr_banks: [u8; 4],
    chr_outer: u8,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Sachen8259M137 {
    /// Construct a new mapper 137 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is empty / not a multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 137 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 137 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            cmd: 0,
            chr_banks: [0; 4],
            chr_outer: 0,
            prg_bank: 0,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count2k = (self.chr_rom.len() / CHR_BANK_2K).max(1);
        let slot = (addr as usize >> 11) & 0x03;
        let bank = (self.chr_banks[slot] as usize | ((self.chr_outer as usize) << 4)) % count2k;
        self.chr_rom[bank * CHR_BANK_2K + (addr as usize & 0x07FF)]
    }
}

impl Mapper for Sachen8259M137 {
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

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4100/$4101 are write-only registers; the rest of $4020-$5FFF is open
        // bus. $8000-$FFFF is mapped PRG.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4100 => self.cmd = value & 0x07,
            0x4101 => match self.cmd {
                0..=3 => self.chr_banks[self.cmd as usize] = value & 0x07,
                4 => self.chr_outer = value & 0x07,
                5 => self.prg_bank = value & 0x07,
                7 => self.horizontal_mirroring = (value & 0x01) != 0,
                _ => {}
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.read_chr(addr),
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if (0x2000..=0x3EFF).contains(&addr) {
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
        let mut out = Vec::with_capacity(9 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.cmd);
        out.extend_from_slice(&self.chr_banks);
        out.push(self.chr_outer);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 9 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.cmd = data[1];
        self.chr_banks.copy_from_slice(&data[2..6]);
        self.chr_outer = data[6];
        self.prg_bank = data[7];
        self.horizontal_mirroring = data[8] != 0;
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 156 — DIS23C01 DAOU (Open Corp / Daou Infosys).
//
// Separate low/high CHR-bank register banks plus a 16 KiB PRG register and an
// explicit one-screen mirroring register, all decoded in the $C000-$C014
// window:
//   $C000-$C003 : CHR low bits for 1 KiB slots 0..3.
//   $C004-$C007 : CHR low bits for 1 KiB slots 4..7.
//   $C008-$C00B : CHR high bits for slots 0..3.
//   $C00C-$C00F : CHR high bits for slots 4..7.
//   $C010       : 16 KiB PRG bank at $8000 ($C000 half fixed to last).
//   $C014       : mirroring (bit 0: 0 = SingleScreenA, 1 = SingleScreenB).
// CHR is ROM (eight 1 KiB slots). No IRQ.
// ===========================================================================

/// Which Sachen 8259 variant (CHR shift + OR constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sachen8259Variant {
    /// 8259A (mapper 141): shift 1, CHR-OR [1, 0, 1].
    A,
    /// 8259B (mapper 138): shift 0, CHR-OR [0, 0, 0].
    B,
    /// 8259C (mapper 139): shift 2, CHR-OR [1, 2, 3].
    C,
}

impl Sachen8259Variant {
    const fn shift(self) -> u8 {
        match self {
            Self::A => 1,
            Self::B => 0,
            Self::C => 2,
        }
    }
    const fn chr_or(self) -> [usize; 3] {
        match self {
            Self::A => [1, 0, 1],
            Self::B => [0, 0, 0],
            Self::C => [1, 2, 3],
        }
    }
}

/// Sachen 8259 A/B/C (mappers 141 / 138 / 139). 32 KiB PRG + 2 KiB CHR banks.
pub struct Sachen8259 {
    variant: Sachen8259Variant,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    regs: [u8; 8],
    current_reg: u8,
    mirroring: Mirroring,
}

const CHR_2K: usize = 0x0800;

impl Sachen8259 {
    /// Construct a Sachen 8259 A/B/C board.
    ///
    /// # Errors
    /// [`MapperError::Invalid`] on a bad PRG/CHR size.
    pub fn new(
        variant: Sachen8259Variant,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "Sachen 8259 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            if !chr_rom.len().is_multiple_of(CHR_2K) {
                return Err(MapperError::Invalid(format!(
                    "Sachen 8259 CHR-ROM size {} is not a multiple of 2 KiB",
                    chr_rom.len()
                )));
            }
            chr_rom
        };
        Ok(Self {
            variant,
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 4 * NAMETABLE_SIZE].into_boxed_slice(),
            regs: [0; 8],
            current_reg: 0,
            mirroring,
        })
    }

    fn update_mirroring(&mut self) {
        let simple = self.regs[7] & 0x01 == 0x01;
        self.mirroring = match (self.regs[7] >> 1) & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenB,
            _ => Mirroring::SingleScreenA,
        };
        if simple {
            self.mirroring = Mirroring::Vertical;
        }
    }

    /// Resolve the 2 KiB CHR bank for slot 0..=3.
    fn chr_bank(&self, slot: usize) -> usize {
        let simple = self.regs[7] & 0x01 == 0x01;
        let shift = self.variant.shift();
        let chr_or = self.variant.chr_or();
        let chr_high = (self.regs[4] as usize) << 3;
        match slot {
            0 => (chr_high | self.regs[0] as usize) << shift,
            1 => ((chr_high | self.regs[if simple { 0 } else { 1 }] as usize) << shift) | chr_or[0],
            2 => ((chr_high | self.regs[if simple { 0 } else { 2 }] as usize) << shift) | chr_or[1],
            _ => ((chr_high | self.regs[if simple { 0 } else { 3 }] as usize) << shift) | chr_or[2],
        }
    }
}

impl Mapper for Sachen8259 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
                let bank = (self.regs[5] as usize) % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x7FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xC101 {
            0x4100 => self.current_reg = value & 0x07,
            0x4101 => {
                self.regs[(self.current_reg & 0x07) as usize] = value & 0x07;
                self.update_mirroring();
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                let slot = (addr as usize) / CHR_2K;
                let count = (self.chr.len() / CHR_2K).max(1);
                let bank = self.chr_bank(slot) % count;
                self.chr[bank * CHR_2K + (addr as usize & (CHR_2K - 1))]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let off = addr as usize & (self.chr.len() - 1);
                self.chr[off] = value;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(11 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.push(self.current_reg);
        out.extend_from_slice(&self.regs);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 11 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.current_reg = data[1];
        self.regs.copy_from_slice(&data[2..10]);
        self.mirroring = byte_to_mirroring(data[10], self.mirroring);
        let mut cursor = 11;
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
// Mapper 42 — FDS-to-cartridge conversion (Mario Baby / Ai Senshi Nicol).
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

    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m137_command_data_chr_and_prg() {
        let mut m =
            Sachen8259M137::new(synth_prg_32k(4), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        // cmd 5 -> PRG 32 KiB bank 2.
        m.cpu_write(0x4100, 5);
        m.cpu_write(0x4101, 2);
        assert_eq!(m.cpu_read(0x8000), 2);
        // cmd 0 -> CHR slot 0 = bank 3.
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4101, 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        // cmd 7 -> horizontal mirroring.
        m.cpu_write(0x4100, 7);
        m.cpu_write(0x4101, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m137_save_state_round_trip() {
        let mut m =
            Sachen8259M137::new(synth_prg_32k(4), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4100, 5);
        m.cpu_write(0x4101, 1);
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4101, 2);
        let blob = m.save_state();
        let mut m2 =
            Sachen8259M137::new(synth_prg_32k(4), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }

    #[test]
    fn sachen8259_prg_and_reg_protocol() {
        let mut m = Sachen8259::new(
            Sachen8259Variant::B,
            synth_prg_32k(4),
            synth_chr_2k(16),
            Mirroring::Vertical,
        )
        .unwrap();
        m.cpu_write(0x4100, 5);
        m.cpu_write(0x4101, 2);
        assert_eq!(m.cpu_read(0x8000), 2);
        m.cpu_write(0x4100, 7);
        m.cpu_write(0x4101, 2); // reg7 = 2 -> mirroring bits (2>>1)&3 == 1 -> horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn sachen8259_variants_differ_by_shift() {
        let mut b = Sachen8259::new(
            Sachen8259Variant::B,
            synth_prg_32k(2),
            synth_chr_2k(16),
            Mirroring::Vertical,
        )
        .unwrap();
        b.cpu_write(0x4100, 0);
        b.cpu_write(0x4101, 1);
        let mut a = Sachen8259::new(
            Sachen8259Variant::A,
            synth_prg_32k(2),
            synth_chr_2k(16),
            Mirroring::Vertical,
        )
        .unwrap();
        a.cpu_write(0x4100, 0);
        a.cpu_write(0x4101, 1);
        assert_eq!(b.ppu_read(0x0000), 1); // shift 0.
        assert_eq!(a.ppu_read(0x0000), 2); // shift 1.
    }

    #[test]
    fn sachen8259_save_state_round_trip() {
        let mut m = Sachen8259::new(
            Sachen8259Variant::C,
            synth_prg_32k(4),
            synth_chr_2k(32),
            Mirroring::Vertical,
        )
        .unwrap();
        m.cpu_write(0x4100, 5);
        m.cpu_write(0x4101, 3);
        m.cpu_write(0x4100, 4);
        m.cpu_write(0x4101, 1);
        let blob = m.save_state();
        let mut m2 = Sachen8259::new(
            Sachen8259Variant::C,
            synth_prg_32k(4),
            synth_chr_2k(32),
            Mirroring::Vertical,
        )
        .unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }
}
