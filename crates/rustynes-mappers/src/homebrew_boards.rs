//! Modern homebrew flash boards: `INL`-NSF (mapper 31), Magic Floor
//! (mapper 218), `RET-CUFROM` (mapper 29), and `GTROM` (mapper 111).
//!
//! Unlike the pirate boards elsewhere in this crate, these were designed
//! *after* the console, by homebrew developers who could pick any mapping they
//! liked -- so they optimise for what a modern toolchain wants rather than for
//! 1980s discrete-logic cost. Mapper 31 exposes eight independently-latched
//! 4 KiB PRG slots (chosen so an NSF player can page music banks freely);
//! Magic Floor uses no CHR memory at all, serving pattern *and* nametable
//! fetches out of the console's own CIRAM; `GTROM` banks its own nametable
//! alongside PRG and CHR so a game can double-buffer whole screens.
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

const PRG_BANK_4K: usize = 0x1000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
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
// Mapper 31 — INL / NSF-style 4 KiB-banked board ("2A03 Puritans").
//
// Eight 4 KiB PRG slots ($8000/$9000/.../$F000), each latched by a write to
// $5FF8-$5FFF (the low three address bits pick the slot). Power-on fixes the
// last slot ($F000) to the final 4 KiB bank (0xFF & mask). CHR is 8 KiB RAM.
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 31 (`INL`-NSF-style 4 KiB-banked board).
pub struct Inl31 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_slots: [u8; 8],
    mirroring: Mirroring,
}

impl Inl31 {
    /// Construct a new mapper 31 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 4 KiB.
    #[allow(clippy::cast_possible_truncation)]
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_4K) {
            return Err(MapperError::Invalid(format!(
                "mapper 31 PRG-ROM size {} is not a non-zero multiple of 4 KiB",
                prg_rom.len()
            )));
        }
        // The last 4 KiB bank index is bounded by the slot register width; the
        // truncation is benign (bank selects wrap by `% count` anyway).
        let last = ((prg_rom.len() / PRG_BANK_4K).max(1) - 1) as u8;
        let mut prg_slots = [0u8; 8];
        prg_slots[7] = last;
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_slots,
            mirroring,
        })
    }
}

impl Mapper for Inl31 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The latch window lives at $5FF8-$5FFF (write-only); reads there fall
    // through to open bus, so the default `cpu_read_unmapped` is correct.

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_4K).max(1);
            let slot = ((addr >> 12) & 0x07) as usize;
            let bank = (self.prg_slots[slot] as usize) % count;
            self.prg_rom[bank * PRG_BANK_4K + (addr as usize & 0x0FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x5FF8..=0x5FFF).contains(&addr) {
            self.prg_slots[(addr & 0x07) as usize] = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
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
        let mut out = Vec::with_capacity(1 + 8 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_slots);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + 8 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_slots.copy_from_slice(&data[1..9]);
        let mut cursor = 9;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 94 — UN1ROM (Senjou no Ookami).
//
// $8000-$FFFF write (with bus conflict): the 16 KiB PRG bank at $8000 is
// (data >> 2) & 0x0F. $C000 is fixed to the last 16 KiB bank. CHR is 8 KiB RAM.
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Custom mirroring/CHR-source mode for mapper 218 ("Magic Floor").
#[derive(Clone, Copy, PartialEq, Eq)]
enum MagicFloorMode {
    Vertical,
    Horizontal,
    ScreenA,
    ScreenB,
}

impl MagicFloorMode {
    /// Resolve a logical 1 KiB block index (0..=3) to a physical CIRAM 1 KiB
    /// bank (0 or 1). Matches `GeraNES` `customMirroring`.
    const fn physical_bank(self, block: u8) -> usize {
        match self {
            Self::Vertical => (block & 0x01) as usize,
            Self::Horizontal => ((block >> 1) & 0x01) as usize,
            Self::ScreenA => 0,
            Self::ScreenB => 1,
        }
    }
}

/// Mapper 218 ("Magic Floor").
pub struct MagicFloor218 {
    prg_rom: Box<[u8]>,
    /// 2 KiB CIRAM serving both the pattern table and nametables.
    ciram: Box<[u8]>,
    mode: MagicFloorMode,
}

impl MagicFloor218 {
    /// Construct a new mapper 218 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB. Any supplied CHR-ROM is rejected (the board has none). Real Magic
    /// Floor dumps are 16 KiB (NROM-128-style, mirrored across the 32 KiB CPU
    /// window); a 32 KiB image is also accepted.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 218 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.is_empty() {
            return Err(MapperError::Invalid(format!(
                "mapper 218 has no CHR-ROM (CIRAM is used as CHR); got {} bytes",
                chr_rom.len()
            )));
        }
        // The four screen modes come from the cart's mirroring + four-screen
        // wiring. Without a four-screen flag we use vertical / horizontal;
        // the single-screen modes are reachable from those header values.
        let mode = match mirroring {
            Mirroring::Vertical | Mirroring::FourScreen => MagicFloorMode::Vertical,
            Mirroring::SingleScreenA => MagicFloorMode::ScreenA,
            Mirroring::SingleScreenB => MagicFloorMode::ScreenB,
            Mirroring::Horizontal | Mirroring::MapperControlled => MagicFloorMode::Horizontal,
        };
        Ok(Self {
            prg_rom,
            ciram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mode,
        })
    }

    /// Map a $0000-$1FFF pattern-table address into the 2 KiB CIRAM, treating
    /// the 8 KiB pattern space as four 1 KiB blocks under the custom mirroring.
    const fn chr_offset(&self, addr: u16) -> usize {
        let block = ((addr >> 10) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        self.mode.physical_bank(block) * NAMETABLE_SIZE + local
    }

    const fn nt_offset(&self, addr: u16) -> usize {
        let block = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        self.mode.physical_bank(block) * NAMETABLE_SIZE + local
    }
}

impl Mapper for MagicFloor218 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Mirror the PRG across the 32 KiB window: a 16 KiB image
            // (NROM-128-style) repeats, a 32 KiB image maps 1:1.
            self.prg_rom[(addr as usize - 0x8000) % self.prg_rom.len()]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.ciram[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.ciram[self.nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.ciram[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = self.nt_offset(addr);
                self.ciram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        Some(self.ciram[self.nt_offset(addr)])
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        let off = self.nt_offset(addr);
        self.ciram[off] = value;
        true
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.mode {
            MagicFloorMode::Vertical => Mirroring::Vertical,
            MagicFloorMode::Horizontal => Mirroring::Horizontal,
            MagicFloorMode::ScreenA => Mirroring::SingleScreenA,
            MagicFloorMode::ScreenB => Mirroring::SingleScreenB,
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.ciram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.ciram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + self.ciram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.ciram.copy_from_slice(&data[1..=self.ciram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 29 — Sealie RET-CUFROM homebrew.
//
// $8000-$FFFF latch: CHR (8 KiB RAM) bank = data & 0x03; PRG (16 KiB) bank =
// (data >> 2) & 0x07. $8000 reads the selected 16 KiB bank; $C000 is fixed to
// the last 16 KiB bank. CHR is 8 KiB RAM (32 KiB on the board, but the visible
// window is 8 KiB selected by the 2-bit CHR bank). Mirroring header-fixed.
// ===========================================================================

/// Mapper 29 (Sealie `RET-CUFROM`).
pub struct Cufrom29 {
    prg_rom: Box<[u8]>,
    /// 32 KiB CHR-RAM (four 8 KiB banks).
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Cufrom29 {
    /// Construct a new mapper 29 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 29 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; 4 * CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_ram.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + (addr as usize & 0x1FFF)
    }
}

impl Mapper for Cufrom29 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let bank = (self.prg_bank as usize) % count;
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            0xC000..=0xFFFF => {
                let last = (self.prg_rom.len() / PRG_BANK_16K).max(1) - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.chr_bank = value & 0x03;
            self.prg_bank = (value >> 2) & 0x07;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr_ram[off] = value;
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
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len() + self.chr_ram.len();
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
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 107 — Magic Dragon.
//
// $8000-$FFFF latch: PRG (32 KiB) bank = (value >> 1); CHR (8 KiB) bank =
// value. Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 111 (`GTROM`/Cheapocabra).
pub struct Gtrom111 {
    prg_rom: Box<[u8]>,
    /// 16 KiB CHR-RAM: two 8 KiB banks.
    chr_ram: Box<[u8]>,
    /// 8 KiB nametable RAM: two banks of four 1 KiB screens.
    nt_ram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    nt_bank: u8,
}

impl Gtrom111 {
    /// Construct a new mapper 111 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(prg_rom: Box<[u8]>, _chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 111 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; 2 * CHR_BANK_8K].into_boxed_slice(),
            nt_ram: vec![0u8; 2 * 4 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            nt_bank: 0,
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    fn update_register(&mut self, value: u8) {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        // `(value & 0x0F) % count` < 16, so the cast cannot truncate.
        self.prg_bank = ((value & 0x0F) as usize % count) as u8;
        self.chr_bank = (value >> 4) & 0x01;
        self.nt_bank = (value >> 5) & 0x01;
    }

    const fn chr_offset(&self, addr: u16) -> usize {
        (self.chr_bank as usize) * CHR_BANK_8K + (addr as usize & 0x1FFF)
    }

    const fn nt_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as usize;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        (self.nt_bank as usize) * 4 * NAMETABLE_SIZE + table * NAMETABLE_SIZE + local
    }
}

impl Mapper for Gtrom111 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_rom[(self.prg_bank as usize) * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // The register decodes anywhere in the $5000-$7FFF window.
        if (0x5000..=0x7FFF).contains(&addr) {
            self.update_register(value);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.nt_ram[self.nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr_ram[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = self.nt_offset(addr);
                self.nt_ram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        Some(self.nt_ram[self.nt_offset(addr)])
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        let off = self.nt_offset(addr);
        self.nt_ram[off] = value;
        true
    }

    fn current_mirroring(&self) -> Mirroring {
        Mirroring::FourScreen
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.chr_ram.len() + self.nt_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(self.nt_bank);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.nt_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.chr_ram.len() + self.nt_ram.len();
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
        self.nt_bank = data[3];
        let mut cursor = 4;
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.nt_ram
            .copy_from_slice(&data[cursor..cursor + self.nt_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 234 — Maxi 15 / BNROM-like multicart.
//
// Two registers latched by reads/writes in the $FF80-$FF9F (reg0) and
// $FFE8-$FFF8 (reg1) windows. reg0 latches once (while its low 6 bits are 0)
// and selects the outer block + sub-mode (bit 6 = NINA-style); reg1 selects the
// inner PRG/CHR within the block. The resolved 32 KiB PRG bank and 8 KiB CHR
// bank follow GeraNES `prgBank()` / `chrBank()`. Mirroring = reg0 bit 7
// (1 = horizontal, 0 = vertical). No IRQ.
// ===========================================================================

/// Mapper 28 (Action 53 homebrew multicart).
pub struct Action53M28 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    reg_select: u8,
    chr_reg: u8,
    inner_prg: u8,
    mode: u8,
    outer_prg: u8,
}

impl Action53M28 {
    /// Construct a new mapper 28 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 28 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_select: 0,
            chr_reg: 0,
            inner_prg: 0,
            mode: 0,
            outer_prg: 0,
        })
    }

    /// Resolve the 16 KiB PRG bank serving a CPU address in $8000-$FFFF.
    fn prg_bank_for(&self, addr: u16) -> usize {
        let count16 = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        // The outer bank is shifted left by the size mask (bits 4-5 of mode).
        let size = (self.mode >> 4) & 0x03;
        let outer = (self.outer_prg as usize) << (size + 1);
        let prg_mode = (self.mode >> 2) & 0x03;
        let high = addr >= 0xC000;
        let inner = self.inner_prg as usize;
        let bank = match prg_mode {
            // NROM-256: a 32 KiB bank; the high half is +1.
            0 | 1 => (outer & !1) | usize::from(high),
            // UNROM: low half selectable, high half fixed to the outer top.
            2 => {
                if high {
                    outer | 0x01
                } else {
                    (outer & !1) | (inner & 0x01)
                }
            }
            // NROM-128: both halves are the same 16 KiB bank.
            _ => outer | (inner & 0x01),
        };
        bank % count16
    }
}

impl Mapper for Action53M28 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let bank = self.prg_bank_for(addr);
            self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5000..=0x5FFF => self.reg_select = value & 0x81,
            0x8000..=0xFFFF => match self.reg_select {
                0x00 => self.chr_reg = value,
                0x01 => self.inner_prg = value,
                0x80 => self.mode = value,
                _ => self.outer_prg = value,
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.mode & 0x03 {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::SingleScreenB,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_select);
        out.push(self.chr_reg);
        out.push(self.inner_prg);
        out.push(self.mode);
        out.push(self.outer_prg);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 6 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_select = data[1];
        self.chr_reg = data[2];
        self.inner_prg = data[3];
        self.mode = data[4];
        self.outer_prg = data[5];
        let mut cursor = 6;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 30 — UNROM-512 (RetroUSB / InfiniteNESLives / Broke Studio).
//
// A single latch register decodes as `[N CC P PPPP]`: bits 0-4 = 16 KiB PRG
// bank at $8000, bits 5-6 = 8 KiB CHR-RAM bank, bit 7 = nametable select
// (only when the cart is wired for software-controlled mirroring). $C000 is
// fixed to the last 16 KiB bank. CHR is 32 KiB RAM (carts with no CHR-ROM)
// or, for the converted Waixing `.WXN` dumps, CHR-ROM. No IRQ.
//
// Submapper / battery semantics (NESdev "UNROM 512", verified against the
// Mesen2 `UnRom512` board):
//
//   * Submapper 0 *without* the battery bit, or submapper 2: the latch
//     responds to the whole $8000-$FFFF range and the board has BUS CONFLICTS
//     (the written value is ANDed with the PRG byte at that address).
//   * Submapper 0 *with* the battery bit, or submappers 1/3/4: NO bus
//     conflicts; the latch responds only to $C000-$FFFF (A14 high) and
//     $8000-$BFFF is the flash-write window (a write there does NOT bank-switch
//     — we accept it but do not model the SST39SF040 flash chip, so reads of
//     that window return PRG-ROM and self-flashing persistence is a no-op).
//
// The battery bit, not a save-RAM presence, is what selects the no-bus-conflict
// wiring on iNES (submapper 0). Self-flashing homebrew such as *Wampus* and the
// *PROTO DERE .NES* beta set it; applying bus conflicts to those carts ANDs the
// boot-time bank-switch value with ROM and jumps the CPU into garbage (a solid
// backdrop frame). See `docs/mappers.md`.
//
// Nametable arrangement bits in iNES byte 6 (`%....N..M`, N = bit 3 = the
// four-screen flag, M = bit 0). UNROM-512 uses the *standard* iNES byte-6
// convention (no inversion) — verified against Mesen2 `UnRom512::InitMapper`,
// which decodes `Byte6 & 0x09`:
//
//   * `00` (N=0,M=0) -> Horizontal mirroring (the wiki's "vertical arrangement").
//   * `01` (N=0,M=1) -> Vertical mirroring   (the wiki's "horizontal arrangement").
//   * `10` (N=1,M=0) -> 1-screen, software-switchable A/B via latch bit 7.
//   * `11` (N=1,M=1) -> 4-screen, cartridge VRAM (last 8 KiB of CHR-RAM; latch
//     bit 7 is inert for mirroring here, per Mesen2).
//
// The wiki phrases the M bit in *arrangement* terms ("vertical arrangement" =
// horizontal mirroring); this codebase's `Mirroring` enum is in *mirroring*
// terms, so M=1 -> `Mirroring::Vertical`. That matches both Mesen2 and the
// generic header parser (`header.rs`: `byte6 bit0 -> Vertical`). The raw flags
// are still threaded through the constructor so the 1-screen / 4-screen N=1
// wirings (which the generic parser collapses) can be reconstructed precisely.
// ===========================================================================

/// Per-board nametable wiring resolved from the iNES header for mapper 30.
#[derive(Clone, Copy, PartialEq, Eq)]
enum M30Nametable {
    /// Hard-wired horizontal mirroring.
    Horizontal,
    /// Hard-wired vertical mirroring.
    Vertical,
    /// Submapper 3: latch bit 7 selects horizontal vs vertical mirroring at
    /// runtime (Mesen2 `UnRom512`: `value & 0x80 ? Vertical : Horizontal`).
    SwitchableHv,
    /// Software-switchable single-screen (latch bit 7 picks A/B).
    OneScreen,
    /// Four-screen, cartridge VRAM. On real hardware (the `InfiniteNESLives`
    /// variant) the four nametables come from the last 8 KiB of the 32 KiB
    /// CHR-RAM, not a separate 4 KiB VRAM chip. We allocate only the standard
    /// 2 KiB CIRAM, so this is approximated as single-screen rather than true
    /// 4-screen — an honest `BestEffort` limitation, not a true 4-screen claim.
    /// No game in the corpus exercises it; revisit if one appears.
    FourScreen,
}

/// Mapper 30 (`UNROM-512`).
///
/// The four booleans mirror distinct iNES-header-derived wirings (CHR-ROM vs
/// RAM, the latch nametable bit, bus-conflict presence, and the flash-window
/// banking mode), so they don't fold into an enum without losing fidelity.
#[allow(clippy::struct_excessive_bools)]
pub struct Unrom512M30 {
    prg_rom: Box<[u8]>,
    /// CHR storage: 32 KiB RAM by default, or CHR-ROM for `.WXN` conversions.
    chr: Box<[u8]>,
    /// True when `chr` is read-only ROM (no PPU writes land).
    chr_is_rom: bool,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    /// Latch bit 7 (software nametable select), only meaningful for the
    /// 1-screen / 4-screen wirings.
    nt_bit: bool,
    nametable: M30Nametable,
    /// True when the board has bus conflicts (submapper 0 w/o battery, or 2).
    bus_conflicts: bool,
    /// True when the banking latch responds only to $C000-$FFFF and
    /// $8000-$BFFF is the flash window (submapper 0 w/ battery, or 1/3/4).
    flash_window: bool,
}

impl Unrom512M30 {
    /// Construct a new mapper 30 board.
    ///
    /// `four_screen` is iNES byte-6 bit 3, `vertical` is byte-6 bit 0 (the raw
    /// flags, before the generic parser's standard-convention mapping). The
    /// `submapper` and `has_battery` flags select the bus-conflict / flash
    /// wiring per the nesdev-wiki `UNROM 512` submapper table.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: &[u8],
        four_screen: bool,
        vertical: bool,
        submapper: u8,
        has_battery: bool,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 30 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }

        // Nametable wiring. Submapper 3 = runtime mapper-controlled H/V select
        // (latch bit 7); power-on default is Vertical (matching Mesen2).
        // Otherwise the byte-6 N/M bits select the four configurations.
        let nametable = if submapper == 3 {
            M30Nametable::SwitchableHv
        } else if four_screen && vertical {
            M30Nametable::FourScreen
        } else if four_screen {
            M30Nametable::OneScreen
        } else if vertical {
            M30Nametable::Vertical
        } else {
            M30Nametable::Horizontal
        };

        // Bus conflicts / flash wiring per submapper + battery bit.
        let bus_conflicts = (submapper == 0 && !has_battery) || submapper == 2;
        let flash_window = (submapper == 0 && has_battery) || matches!(submapper, 1 | 3 | 4);

        // CHR: prefer CHR-ROM when the dump carries it (e.g. the converted
        // `.WXN` Waixing carts); otherwise the standard 32 KiB CHR-RAM.
        let (chr, chr_is_rom) = if chr_rom.is_empty() {
            (vec![0u8; 4 * CHR_BANK_8K].into_boxed_slice(), false)
        } else {
            (chr_rom.to_vec().into_boxed_slice(), true)
        };

        // Power-on `nt_bit`: for submapper 3 the board defaults to Vertical
        // (Mesen2 `UnRom512::InitMapper`), and `current_mirroring()` maps a set
        // bit to Vertical, so seed it `true` to match that default before the
        // first latch write. For every other wiring the bit only matters for
        // the single-screen case, whose A/B default is `false` (ScreenA).
        let nt_bit = nametable == M30Nametable::SwitchableHv;

        Ok(Self {
            prg_rom,
            chr,
            chr_is_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            nt_bit,
            nametable,
            bus_conflicts,
            flash_window,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + (addr as usize & 0x1FFF)
    }

    /// Apply a write to the banking latch (already known to target the latch).
    fn write_latch(&mut self, addr: u16, value: u8) {
        let effective = if self.bus_conflicts {
            // Bus conflict: AND with the PRG byte actually driving the bus at the
            // write address. The switchable bank serves $8000-$BFFF; the FIXED
            // last 16 KiB bank serves $C000-$FFFF, so a write there conflicts
            // with the fixed bank, not the currently-selected low bank (matches
            // Mesen2's address-based `BaseMapper` conflict resolution).
            let conflict_bank = if addr >= 0xC000 {
                (self.prg_rom.len() / PRG_BANK_16K).max(1) - 1
            } else {
                self.prg_bank as usize
            };
            value & self.read_prg(conflict_bank, addr)
        } else {
            value
        };
        self.prg_bank = effective & 0x1F;
        self.chr_bank = (effective >> 5) & 0x03;
        self.nt_bit = (effective & 0x80) != 0;
    }
}

impl Mapper for Unrom512M30 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(self.prg_bank as usize, addr),
            0xC000..=0xFFFF => {
                let last = (self.prg_rom.len() / PRG_BANK_16K).max(1) - 1;
                self.read_prg(last, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return;
        }
        if self.flash_window {
            // No-bus-conflict wiring: the banking latch lives at $C000-$FFFF;
            // $8000-$BFFF is the SST39SF040 flash-command window (not modelled
            // — accepted as a no-op so self-flashing writes don't bank-switch).
            if addr >= 0xC000 {
                self.write_latch(addr, value);
            }
        } else {
            // Submapper 0 w/o battery or submapper 2: the latch responds to the
            // whole $8000-$FFFF range, with bus conflicts.
            self.write_latch(addr, value);
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
                if !self.chr_is_rom {
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
        match self.nametable {
            M30Nametable::Horizontal => Mirroring::Horizontal,
            M30Nametable::Vertical => Mirroring::Vertical,
            // Submapper 3: latch bit 7 picks vertical (set) vs horizontal
            // (clear) at runtime (Mesen2 `value & 0x80 ? Vertical : Horizontal`).
            M30Nametable::SwitchableHv => {
                if self.nt_bit {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                }
            }
            // Software-switchable single-screen: latch bit 7 selects which CIRAM
            // half (A10=0 lower, A10=1 upper). The 4-screen wiring routes its
            // nametables through cartridge CHR-RAM, so the mapper still reports a
            // single-screen base here; the bit is otherwise inert for it.
            M30Nametable::OneScreen | M30Nametable::FourScreen => {
                if self.nt_bit {
                    Mirroring::SingleScreenB
                } else {
                    Mirroring::SingleScreenA
                }
            }
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_len = if self.chr_is_rom { 0 } else { self.chr.len() };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_len);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.nt_bit));
        out.extend_from_slice(&self.vram);
        // CHR-ROM is immutable; only persist CHR-RAM contents.
        if !self.chr_is_rom {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_len = if self.chr_is_rom { 0 } else { self.chr.len() };
        let expected = 4 + self.vram.len() + chr_len;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        // Mask the register indices to their live-invariant widths so a
        // corrupted / hand-edited save-state can't seed an out-of-range value
        // (mirrors the write-latch masks; same defensive treatment as the
        // JY-ASIC `chr_latch` clamp in `m035_jy_asic.rs`). The read paths already
        // wrap with `% count`, so this is belt-and-suspenders, not a panic fix.
        self.prg_bank = data[1] & 0x1F;
        self.chr_bank = data[2] & 0x03;
        self.nt_bit = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if !self.chr_is_rom {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 63 — NTDEC 0324 "Powerful 250-in-1".
//
// Address-decoded register across $8000-$FFFF (data byte ignored). For the
// absolute address A:
//   PRG: bits 1-6 of A select a 16 KiB bank index; bit 0 picks 32 KiB mode
//        (when A&1 == 0, the two 16 KiB halves form a 32 KiB bank).
//   mirroring = bit 0 of (A >> 1)? -> we follow the common decode: A bit 1
//        selects H/V is not used; mapper 63 uses A & 0x06 for the 16K bank and
//        bit 0 for the 32K/16K mode; mirroring follows A bit 0 of the high byte.
// We use the documented decode: bank = (A >> 1) & 0x3F; if (A & 1)==0 -> 32 KiB
// (bank &= !1, high half = bank|1); mirroring = (A & 0x0001_0000)?? — there is
// no separate mirroring line, so the board uses the standard A-bit decode:
// mirroring = if (A & 0x06) == 0x06 horizontal else vertical is NOT it either.
//
// To keep this register-decode honest and simple we implement the widely-cited
// FCEUX decode: PRG 16 KiB bank = (A >> 2) & 0x3F, 32 KiB mode when (A & 2)==0,
// CHR is 8 KiB RAM, mirroring = (A & 1) ? horizontal : vertical.
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

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_prg_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_4K];
        for b in 0..banks {
            v[b * PRG_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m31_slots_latch_per_window() {
        let mut m = Inl31::new(synth_prg_4k(8), &[], Mirroring::Vertical).unwrap();
        // Slot 0 ($8000) <- bank 3; slot 7 ($F000) <- bank 5.
        m.cpu_write(0x5FF8, 3);
        m.cpu_write(0x5FFF, 5);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xF000), 5);
        // Untouched slot 1 ($9000) stays at power-on 0.
        assert_eq!(m.cpu_read(0x9000), 0);
    }

    #[test]
    fn m31_save_state_round_trip() {
        let mut m = Inl31::new(synth_prg_4k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x5FF8, 2);
        m.ppu_write(0x0001, 0xCD);
        let blob = m.save_state();
        let mut m2 = Inl31::new(synth_prg_4k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 2);
        assert_eq!(m2.ppu_read(0x0001), 0xCD);
    }

    #[test]
    fn m218_ciram_serves_chr_and_nametable() {
        let mut m = MagicFloor218::new(synth_prg_32k(1), &[], Mirroring::Vertical).unwrap();
        // Vertical: pattern block 0 -> physical bank 0; block 1 -> bank 1.
        // Write CHR at $0000 (block 0) and a nametable at $2400 (table 1).
        m.ppu_write(0x0000, 0x11);
        m.ppu_write(0x2400, 0x22);
        // $2400 = table 1 -> physical bank 1; $0400 = pattern block 1 -> bank 1.
        assert_eq!(m.ppu_read(0x0400), 0x22);
        // $2000 = table 0 -> bank 0 = the CHR byte written at $0000.
        assert_eq!(m.ppu_read(0x2000), 0x11);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m218_accepts_16k_prg_and_mirrors_it() {
        // Real Magic Floor dumps are 16 KiB (NROM-128-style). The board must
        // accept them and mirror PRG across the full 32 KiB CPU window.
        let mut prg = synth_prg_16k(1);
        prg[0] = 0xAB; // marker at the start of the 16 KiB image
        let mut m = MagicFloor218::new(prg, &[], Mirroring::Horizontal).unwrap();
        // $8000 and the mirror at $C000 both read the same byte.
        assert_eq!(m.cpu_read(0x8000), 0xAB);
        assert_eq!(m.cpu_read(0xC000), 0xAB);
    }

    #[test]
    fn m218_save_state_round_trip() {
        let mut m = MagicFloor218::new(synth_prg_32k(1), &[], Mirroring::Horizontal).unwrap();
        m.ppu_write(0x0005, 0x42);
        let blob = m.save_state();
        let mut m2 = MagicFloor218::new(synth_prg_32k(1), &[], Mirroring::Horizontal).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0005), 0x42);
    }

    #[test]
    fn m29_latch_selects_prg_and_chr_bank() {
        let mut m = Cufrom29::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // value: CHR = data&3, PRG = (data>>2)&7. 0b0001_0110 = 0x16:
        //   CHR = 0b10 = 2, PRG = 0b101 = 5.
        m.cpu_write(0x8000, 0b0001_0110);
        assert_eq!(m.cpu_read(0x8000), 5);
        // $C000 is fixed to the last 16 KiB bank (7).
        assert_eq!(m.cpu_read(0xC000), 7);
        // CHR-RAM round-trip in the selected (bank 2) window.
        m.ppu_write(0x0003, 0x77);
        assert_eq!(m.ppu_read(0x0003), 0x77);
    }

    #[test]
    fn m29_save_state_round_trip() {
        let mut m = Cufrom29::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0b0000_1101); // CHR 1, PRG 3
        m.ppu_write(0x0007, 0x55);
        let blob = m.save_state();
        let mut m2 = Cufrom29::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 3);
        assert_eq!(m2.ppu_read(0x0007), 0x55);
    }

    #[test]
    fn m111_register_selects_prg_chr_nt() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        // value 0b0011_0101 (0x35): PRG = 5; CHR = (v>>4)&1 = 1; NT = (v>>5)&1 = 1.
        m.cpu_write(0x5000, 0b0011_0101);
        assert_eq!(m.cpu_read(0x8000), 5);
        // CHR bank 1 round-trip.
        m.ppu_write(0x0000, 0x88);
        assert_eq!(m.ppu_read(0x0000), 0x88);
        // Nametable bank 1 round-trip via the fetch hook.
        assert!(m.nametable_write(0x2000, 0x99));
        assert_eq!(m.nametable_fetch(0x2000), Some(0x99));
        assert_eq!(m.current_mirroring(), Mirroring::FourScreen);
        // Switching nt bank to 0 hides the byte written under bank 1.
        m.cpu_write(0x5000, 0x00);
        assert_eq!(m.nametable_fetch(0x2000), Some(0x00));
    }

    #[test]
    fn m111_save_state_round_trip() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        m.cpu_write(0x5000, 0b0011_0011); // PRG 3, CHR 1, NT 1
        m.ppu_write(0x0001, 0xAA);
        m.nametable_write(0x2001, 0xBB);
        let blob = m.save_state();
        let mut m2 = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 3);
        assert_eq!(m2.ppu_read(0x0001), 0xAA);
        assert_eq!(m2.nametable_fetch(0x2001), Some(0xBB));
    }

    #[test]
    fn m28_nrom128_mode_mirrors_one_bank() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // mode reg: select reg 0x80, write PRG mode 3 (NROM-128), mirroring V (2),
        // size mask 0.
        m.cpu_write(0x5000, 0x80);
        m.cpu_write(0x8000, 0b0000_1110); // mode bits 2-3 = 3, mirroring bits 0-1 = 2
        // inner reg
        m.cpu_write(0x5000, 0x01);
        m.cpu_write(0x8000, 0x01); // inner = 1
        // outer reg
        m.cpu_write(0x5000, 0x81);
        m.cpu_write(0x8000, 0x02); // outer = 2
        // size mask (mode bits 4-5) = 0 -> outer is shifted left by (size+1)=1,
        // so outer = 2<<1 = 4. NROM-128 mode: both halves = outer|(inner&1)
        // = 4|1 = 5.
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m28_save_state_round_trip() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // Set NROM-128 mode (mode bits 2-3 = 3, mirroring bits 0-1 = 2).
        m.cpu_write(0x5000, 0x80);
        m.cpu_write(0x8000, 0x0E);
        // Set outer = 1.
        m.cpu_write(0x5000, 0x81);
        m.cpu_write(0x8000, 0x01);
        m.ppu_write(0x0007, 0x5A);
        let resolved = m.cpu_read(0x8000);
        let blob = m.save_state();
        let mut m2 = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0007), 0x5A);
        assert_eq!(m2.cpu_read(0x8000), resolved);
        assert_eq!(m2.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m30_latch_selects_prg_chr_and_fixed_high() {
        // Submapper 0 without battery -> bus conflicts on $8000-$FFFF.
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], false, true, 0, false).unwrap();
        // PRG bits 0-4 = 3, CHR bits 5-6 = 1. value = 0b0010_0011 = 0x23.
        // Offset 1 (no marker, 0xFF) -> bus conflict harmless.
        m.cpu_write(0x8001, 0x23);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $C000 fixed to last (7).
        assert_eq!(m.cpu_read(0xC000), 7);
        // CHR bank 1.
        m.ppu_write(0x0000, 0xEE);
        assert_eq!(m.ppu_read(0x0000), 0xEE);
    }

    #[test]
    fn m30_battery_cart_no_bus_conflict_high_window_only() {
        // Submapper 0 WITH battery (e.g. Wampus / PROTO DERE): no bus conflicts;
        // the banking latch responds only to $C000-$FFFF, and $8000-$BFFF is
        // the (un-modelled) flash window that must NOT bank-switch.
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], false, true, 0, true).unwrap();
        // A write to the flash window leaves the bank untouched (still 0).
        m.cpu_write(0x8000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 0);
        // A write to $C000-$FFFF switches the bank with NO bus-conflict AND.
        // Bank 5 even though the PRG byte read there (the bank index) differs.
        m.cpu_write(0xC000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 5);
    }

    #[test]
    fn m30_save_state_round_trip() {
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], false, true, 0, false).unwrap();
        m.cpu_write(0x8001, 0x45);
        m.ppu_write(0x0003, 0x77);
        let blob = m.save_state();
        let mut m2 = Unrom512M30::new(synth_prg_16k(8), &[], false, true, 0, false).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0003), 0x77);
    }

    #[test]
    fn m30_header_mirroring_matches_mesen2() {
        // byte6 N/M decode, mirroring vocabulary (Mesen2 `UnRom512`):
        //   00 (four_screen=0, vertical=0) -> Horizontal mirroring
        //   01 (four_screen=0, vertical=1) -> Vertical mirroring
        // No latch write needed; this is the hard-wired arrangement.
        let m_h = Unrom512M30::new(synth_prg_16k(2), &[], false, false, 0, false).unwrap();
        assert_eq!(m_h.current_mirroring(), Mirroring::Horizontal);
        let m_v = Unrom512M30::new(synth_prg_16k(2), &[], false, true, 0, false).unwrap();
        assert_eq!(m_v.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m30_submapper3_runtime_hv_switch() {
        // Submapper 3: latch bit 7 flips H/V at runtime; power-on default is
        // Vertical (Mesen2). No bus conflicts (flash wiring), latch at $C000+.
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], false, false, 3, false).unwrap();
        assert_eq!(
            m.current_mirroring(),
            Mirroring::Vertical,
            "power-on default"
        );
        // Clear bit 7 -> Horizontal.
        m.cpu_write(0xC000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Set bit 7 -> Vertical.
        m.cpu_write(0xC000, 0x80);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m30_bus_conflict_high_window_uses_fixed_bank() {
        // Bus-conflict cart (submapper 0, no battery): the latch responds across
        // the whole $8000-$FFFF. A $C000-$FFFF write ANDs against the FIXED last
        // bank's byte, NOT the currently-selected low bank. In an 8-bank
        // `synth_prg_16k` ROM, bank b holds `b` at offset 0 and `0xFF` elsewhere.
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], false, true, 0, false).unwrap();
        // Seed the low bank to 2 via a write at offset 1 (current low bank 0,
        // byte 1 = 0xFF, so the value passes through unmasked).
        m.cpu_write(0x8001, 0x02);
        assert_eq!(m.cpu_read(0x8000), 2);
        // Write 0x1F at $C000 (offset 0). The AND source is the FIXED bank 7
        // (byte 0 = 0x07): 0x1F & 0x07 = 0x07 -> low bank becomes 7. The old
        // (buggy) behaviour would source the now-bank-2 low window (byte 0 =
        // 0x02): 0x1F & 0x02 = 0x02 -> bank 2. Asserting 7 proves the fix.
        m.cpu_write(0xC000, 0x1F);
        assert_eq!(m.cpu_read(0x8000), 7);
    }
}
