//! Sprint 8 simple discrete-logic / multicart mappers (v1.3.0 "Bedrock"
//! workstream D1 mapper sweep).
//!
//! A second best-effort (Tier-2) batch of small, hook-free pirate / homebrew /
//! multicart boards ported from the `GeraNES` reference
//! (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`). Each has no IRQ, no
//! on-cart audio, and no per-cycle / A12 hook, so every board reports
//! [`MapperCaps::NONE`]. Like `sprint5`/`sprint6`/`sprint7`, banking math is
//! translated into direct slice indexing; bank selects wrap with `% count`.
//!
//! Boards implemented here:
//!
//! - **Mapper 31** (`INL`-NSF-style, e.g. "2A03 Puritans"): eight 4 KiB PRG
//!   slots latched at `$5FF8-$5FFF`; CHR-RAM.
//! - **Mapper 94** (`UN1ROM`, Senjou no Ookami): 16 KiB PRG bank from data
//!   bits 4-2 (bus conflict), fixed last bank at `$C000`; CHR-RAM.
//! - **Mapper 101** (Jaleco `JF-10` CHR latch): 8 KiB CHR bank latched via a
//!   write to `$6000-$7FFF`; single fixed 32 KiB PRG.
//! - **Mapper 218** ("Magic Floor"): no PRG/CHR-ROM banking; the pattern table
//!   is served from the console CIRAM under a fixed custom mirroring mode.
//! - **Mapper 29** (Sealie `RET-CUFROM` homebrew): 16 KiB PRG bank + 8 KiB
//!   CHR-RAM bank from one `$8000-$FFFF` latch, fixed last PRG bank at `$C000`.
//! - **Mapper 107** (Magic Dragon): 32 KiB PRG (data>>1) + 8 KiB CHR (data&..)
//!   from one `$8000-$FFFF` latch.
//! - **Mapper 143** (Sachen `TCA01`): NROM-128 (mirrored) with a simple
//!   protection read at `$4020-$5FFF` returning `(~addr & 0x3F) | 0x40`.
//! - **Mapper 177** (Hengedianzi): 32 KiB PRG + mirroring bit (bit 5) from one
//!   `$8000-$FFFF` latch; CHR-RAM.
//! - **Mapper 179** (Hengedianzi variant): 32 KiB PRG via `$5000-$5FFF`
//!   (`data>>1`) + a mirroring bit (bit 0) via `$8000-$FFFF`; CHR-RAM.
//! - **Mapper 58** (multicart): address-decoded PRG (16/32 KiB mode) + CHR + a
//!   mirroring bit; bus conflict on the data byte (ignored — address-decoded).
//! - **Mapper 60** (reset-based 4-in-1 multicart): power-on bank only is
//!   modelled (reset-latch behaviour is host-driven and not exercised here).
//! - **Mapper 231** (20-in-1 multicart): address-decoded dual 16 KiB PRG banks
//!   + a mirroring bit.
//! - **Mapper 111** (`GTROM`/Cheapocabra homebrew): 32 KiB PRG bank, 16 KiB
//!   CHR-RAM (two 8 KiB banks), 4-screen nametable RAM with a bank-select bit;
//!   the LED bit (bit 4 in the original docs) is ignored.
//! - **Mapper 234** (Maxi 15 / `BNROM`-like multicart): two latch regs in the
//!   `$FF80-$FF9F` / `$FFE8-$FFF8` windows selecting 32 KiB PRG + 8 KiB CHR in
//!   either NINA-style or CNROM-style sub-mode.

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

/// Mapper 94 (`UN1ROM`).
pub struct Un1rom94 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Un1rom94 {
    /// Construct a new mapper 94 board.
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
                "mapper 94 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            mirroring,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Un1rom94 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND with the byte the CPU actually reads at this
            // address, using the SAME window mapping as `cpu_read` — the
            // switchable bank for $8000-$BFFF, the fixed last bank for
            // $C000-$FFFF (a register write can land in either half).
            let conflict = self.cpu_read(addr);
            let effective = value & conflict;
            // UN1ROM (Senjou no Ookami) selects the 16 KiB bank from data
            // bits 4..2 (a 3-bit field, 8 banks).
            self.prg_bank = (effective >> 2) & 0x07;
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
        let mut out = Vec::with_capacity(2 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len() + self.chr_ram.len();
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
        let mut cursor = 2;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 101 — Jaleco JF-10 CHR latch.
//
// A single fixed 32 KiB PRG bank. An 8 KiB CHR bank is latched by a write to
// the $6000-$7FFF (PRG-RAM) window. Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 101 (Jaleco `JF-10` CHR latch).
pub struct Jaleco101 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Jaleco101 {
    /// Construct a new mapper 101 board.
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
                "mapper 101 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 101 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Jaleco101 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_rom[addr as usize - 0x8000]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // The CHR latch is in the PRG-RAM ($6000-$7FFF) window.
        if (0x6000..=0x7FFF).contains(&addr) {
            self.chr_bank = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 218 — "Magic Floor".
//
// No PRG/CHR-ROM banking: PRG is a fixed 32 KiB bank; the "CHR" is the console
// CIRAM (the 2 KiB nametable RAM) addressed directly. The board has no CHR-ROM
// at all — pattern-table reads alias into the same 2 KiB RAM that the
// nametables use, under a fixed custom mirroring mode selected by the cart
// wiring (vertical / horizontal / one-screen-A / one-screen-B). We model the
// CIRAM as mapper-owned 2 KiB VRAM and serve both pattern + nametable fetches
// from it. No IRQ.
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

/// Mapper 107 (Magic Dragon).
pub struct MagicDragon107 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl MagicDragon107 {
    /// Construct a new mapper 107 board.
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
                "mapper 107 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 107 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

impl Mapper for MagicDragon107 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value >> 1;
            self.chr_bank = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
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
// Mapper 143 — Sachen TCA01.
//
// NROM-128: $8000 and $C000 both read the first/second 16 KiB bank (a 16 KiB
// PRG is mirrored across the 32 KiB window). A simple protection register in
// the $4020-$5FFF window returns (~addr & 0x3F) | 0x40. CHR is 8 KiB ROM (or
// RAM). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 143 (Sachen `TCA01`).
pub struct SachenTca01M143 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    mirroring: Mirroring,
}

impl SachenTca01M143 {
    /// Construct a new mapper 143 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 143 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 143 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
        })
    }

    /// $8000-$BFFF -> first 16 KiB bank; $C000-$FFFF -> second 16 KiB bank
    /// (which equals the first on a 16 KiB image, after the modulo wrap).
    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = if addr < 0xC000 { 0 } else { 1 % count };
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for SachenTca01M143 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The protection register answers reads across the whole $4020-$5FFF
    // window (mapped). $8000-$FFFF PRG-ROM stays mapped (the trait default) —
    // a `!(...)` here would wrongly open-bus the program ROM + reset vector, so
    // the board never boots. There is no open-bus hole to carve out, so this
    // returns false for everything the board answers.
    fn cpu_read_unmapped(&self, _addr: u16) -> bool {
        false
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // Sachen TCA01 protection (puNES extcl_cpu_rd_mem_143): the chip
            // only answers when A8 is set, returning (~addr & 0x3F) in the low
            // 6 bits and leaving the top 2 bits as open bus. The old decode
            // answered across the WHOLE window with a hardcoded bit 6, so the
            // game's `(~addr & 0x3F)` protection compare failed -> blank boot.
            // The high 2 open-bus bits are approximated from the address high
            // byte (the most-recently-driven bus value in this read).
            0x4100..=0x5FFF if addr & 0x0100 != 0 => {
                ((!addr & 0x3F) as u8) | ((addr >> 8) as u8 & 0xC0)
            }
            0x4100..=0x5FFF if addr >= 0x5000 => 0xFF,
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = 0 % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.vram.copy_from_slice(&data[1..=self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 177 — Hengedianzi.
//
// $8000-$FFFF latch: the whole byte selects a 32 KiB PRG bank; bit 5 selects
// mirroring (1 = horizontal, 0 = vertical). CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 177 (Hengedianzi).
pub struct Hengedianzi177 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Hengedianzi177 {
    /// Construct a new mapper 177 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(prg_rom: Box<[u8]>, _chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 177 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Hengedianzi177 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // $8000-$FFFF: `..MP PPPP` — PRG bank is bits 0-4 (5 bits), mirroring
            // is bit 5. The old code latched all 8 bits as the bank, so a write
            // that flips the mirroring bit (e.g. $20) selected bank 32 and the
            // reset vector read garbage → blank boot.
            self.prg_bank = value & 0x1F;
            self.horizontal_mirroring = (value & 0x20) != 0;
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
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
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
        self.horizontal_mirroring = data[2] != 0;
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
// Mapper 179 — Hengedianzi variant.
//
// A 32 KiB PRG bank is latched via $5000-$5FFF (= value >> 1). A separate
// $8000-$FFFF write sets the mirroring bit (bit 0: 1 = horizontal, 0 =
// vertical). CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 179 (Hengedianzi variant).
pub struct Hengedianzi179 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Hengedianzi179 {
    /// Construct a new mapper 179 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(prg_rom: Box<[u8]>, _chr_rom: &[u8]) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 179 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Hengedianzi179 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The PRG-bank register answers a write-only window at $5000-$5FFF; reads
    // there are open bus, so the default `cpu_read_unmapped` is correct.

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
        match addr {
            0x5000..=0x5FFF => self.prg_bank = value >> 1,
            0x8000..=0xFFFF => self.horizontal_mirroring = (value & 0x01) != 0,
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
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
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
        self.horizontal_mirroring = data[2] != 0;
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
// Mapper 58 — multicart.
//
// Address-decoded register across $8000-$FFFF (data byte ignored). For the
// absolute address A:
//   PRG (16 KiB) bank = A & 0x07
//   32 KiB mode when (A & 0x40) == 0: use ((A & 0x06) >> 1) as the 32 KiB bank
//   CHR (8 KiB) bank = (A >> 3) & 0x07
//   mirroring = bit 7 of A (1 = horizontal, 0 = vertical)
// No IRQ.
// ===========================================================================

/// Mapper 58 (multicart).
pub struct Multicart58 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    prg32_mode: bool,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart58 {
    /// Construct a new mapper 58 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 58 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 58 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        let horizontal_mirroring = mirroring == Mirroring::Horizontal;
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            prg32_mode: false,
            chr_bank: 0,
            horizontal_mirroring,
        })
    }
}

impl Mapper for Multicart58 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        if self.prg32_mode {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (((self.prg_bank & 0x06) >> 1) as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (addr & 0x07) as u8;
            self.prg32_mode = (addr & 0x40) == 0;
            self.chr_bank = ((addr >> 3) & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
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
        let mut out = Vec::with_capacity(5 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg32_mode));
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len();
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
        self.prg32_mode = data[2] != 0;
        self.chr_bank = data[3];
        self.horizontal_mirroring = data[4] != 0;
        self.vram.copy_from_slice(&data[5..5 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 60 — reset-based 4-in-1 multicart.
//
// The active game is chosen by a reset counter (0..=3) that increments on each
// console reset; the selected value drives a 16 KiB PRG bank + 8 KiB CHR bank
// in lockstep. Reset-latch behaviour is host-driven (the bus/frontend would
// have to pulse a reset hook we do not model in the no_std core), so we model
// only the power-on bank (counter = 0). No IRQ.
// ===========================================================================

/// Mapper 60 (reset-based 4-in-1 multicart; power-on bank only).
pub struct Multicart60 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    bank: u8,
    mirroring: Mirroring,
}

impl Multicart60 {
    /// Construct a new mapper 60 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 60 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 60 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            // Power-on selects game 0.
            bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Multicart60 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // The reset counter drives a 16 KiB bank mirrored across the 32 KiB
            // window (NROM-128-per-game).
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 231 — 20-in-1 multicart.
//
// Address-decoded register across $8000-$FFFF (data byte ignored). For the
// absolute address A:
//   prgBank = ((A >> 5) & 0x01) | (A & 0x1E)
//   $8000 16 KiB bank = prgBank & 0x1E
//   $C000 16 KiB bank = prgBank
//   mirroring = bit 7 of A (1 = horizontal, 0 = vertical)
// CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 231 (20-in-1 multicart).
pub struct Multicart231 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank0: u8,
    prg_bank1: u8,
    horizontal_mirroring: bool,
}

impl Multicart231 {
    /// Construct a new mapper 231 board.
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
                "mapper 231 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let horizontal_mirroring = mirroring == Mirroring::Horizontal;
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank0: 0,
            prg_bank1: 0,
            horizontal_mirroring,
        })
    }

    fn read_prg(&self, bank: u8, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = (bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart231 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(self.prg_bank0, addr),
            0xC000..=0xFFFF => self.read_prg(self.prg_bank1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let prg_bank = ((((addr >> 5) & 0x01) | (addr & 0x1E)) & 0xFF) as u8;
            self.prg_bank0 = prg_bank & 0x1E;
            self.prg_bank1 = prg_bank;
            self.horizontal_mirroring = (addr & 0x80) != 0;
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
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank0);
        out.push(self.prg_bank1);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank0 = data[1];
        self.prg_bank1 = data[2];
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 111 — GTROM / Cheapocabra homebrew.
//
// A write/read to $5000-$5FFF (and the $7000-$7FFF save-RAM window) latches one
// register: PRG (32 KiB) bank = value & 0x0F; CHR (8 KiB) bank = (value >> 4) &
// 0x01; nametable bank = (value >> 5) & 0x01. CHR is 16 KiB RAM (two 8 KiB
// banks). The nametable is a 4-screen RAM (four 1 KiB screens per nt bank)
// inside the same 16 KiB CHR-RAM array, selected by the nt bank bit. The board
// also exposes a flashable PRG + an LED bit (bit 6) which we ignore. No IRQ.
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

/// Mapper 234 (Maxi 15 / `BNROM`-like multicart).
pub struct Maxi15M234 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg0: u8,
    reg1: u8,
}

impl Maxi15M234 {
    /// Construct a new mapper 234 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 234 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 234 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg0: 0,
            reg1: 0,
        })
    }

    const fn update_state(&mut self) {
        if (self.reg0 & 0x40) != 0 {
            // NINA-03 mode.
            self.reg1 &= 0x71;
        } else {
            // CNROM mode: only the low 2 bits of the reg1 CHR selector matter.
            self.reg1 &= 0x31;
        }
    }

    fn latch_access(&mut self, addr: u16, value: u8) {
        if (0xFF80..=0xFF9F).contains(&addr) {
            // The reg0 latch only fires while its low 6 bits are still zero.
            if self.reg0.trailing_zeros() >= 6 {
                self.reg0 = value;
                self.update_state();
            }
        } else if (0xFFE8..=0xFFF8).contains(&addr) {
            self.reg1 = value & 0x71;
            self.update_state();
        }
    }

    const fn prg_bank(&self) -> u8 {
        if (self.reg0 & 0x40) != 0 {
            (self.reg0 & 0x0E) | (self.reg1 & 0x01)
        } else {
            self.reg0 & 0x0F
        }
    }

    const fn chr_bank(&self) -> u8 {
        if (self.reg0 & 0x40) != 0 {
            ((self.reg0 << 2) & 0x38) | ((self.reg1 >> 4) & 0x07)
        } else {
            ((self.reg0 << 2) & 0x3C) | ((self.reg1 >> 4) & 0x03)
        }
    }
}

impl Mapper for Maxi15M234 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank() as usize) % count;
            let value = self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)];
            // The latch windows live in the readable PRG space; reading them
            // triggers the same latch as a write (with the data the CPU sees).
            self.latch_access(addr, value);
            value
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.latch_access(addr, value);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank() as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
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
        if (self.reg0 & 0x80) != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg0);
        out.push(self.reg1);
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
        self.reg0 = data[1];
        self.reg1 = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    /// 32 KiB-banked PRG: byte 0 of each 32 KiB bank holds the bank index, the
    /// rest is 0xFF (so a bus-conflict AND at offset 0 is observable).
    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 16 KiB-banked PRG: byte 0 of each 16 KiB bank holds the bank index.
    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 4 KiB-banked PRG: byte 0 of each 4 KiB bank holds the bank index.
    fn synth_prg_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_4K];
        for b in 0..banks {
            v[b * PRG_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 8 KiB-banked CHR: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- Mapper 31 ---------------------------------------------------------

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

    // --- Mapper 94 ---------------------------------------------------------

    #[test]
    fn m94_prg_bank_from_data_bits() {
        let mut m = Un1rom94::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // value (data>>2)&0x0F = 3 -> write 0b0000_1100 (12). Offset !=0 has
        // no bus conflict (0xFF), so the value sticks.
        m.cpu_write(0x8001, 0b0000_1100);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $C000 is fixed to the last 16 KiB bank (7).
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn m94_save_state_round_trip() {
        let mut m = Un1rom94::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8001, 0b0001_0000); // (16>>2)&0xF = 4
        m.ppu_write(0x0002, 0x9A);
        let blob = m.save_state();
        let mut m2 = Un1rom94::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 4);
        assert_eq!(m2.ppu_read(0x0002), 0x9A);
    }

    // --- Mapper 101 --------------------------------------------------------

    #[test]
    fn m101_chr_latch_in_6000_window() {
        let mut m =
            Jaleco101::new(synth_prg_32k(1), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x6000, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // PRG is fixed.
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m101_save_state_round_trip() {
        let mut m =
            Jaleco101::new(synth_prg_32k(1), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x7FFF, 6);
        let blob = m.save_state();
        let mut m2 =
            Jaleco101::new(synth_prg_32k(1), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), 6);
    }

    // --- Mapper 218 --------------------------------------------------------

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

    // --- Mapper 29 ---------------------------------------------------------

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

    // --- Mapper 107 --------------------------------------------------------

    #[test]
    fn m107_latch_selects_prg_and_chr() {
        let mut m =
            MagicDragon107::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // value 6: PRG = 6>>1 = 3; CHR = 6.
        m.cpu_write(0x8000, 6);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.ppu_read(0x0000), 6);
    }

    #[test]
    fn m107_save_state_round_trip() {
        let mut m =
            MagicDragon107::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 4); // PRG 2, CHR 4
        let blob = m.save_state();
        let mut m2 =
            MagicDragon107::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 2);
        assert_eq!(m2.ppu_read(0x0000), 4);
    }

    // --- Mapper 143 --------------------------------------------------------

    #[test]
    fn m143_protection_read_and_nrom() {
        let mut m =
            SachenTca01M143::new(synth_prg_16k(1), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // Protection answers only with A8 set: low 6 bits = ~addr & 0x3F, top
        // 2 bits = open bus (approximated from the address high byte). puNES.
        let addr = 0x4100u16;
        assert_eq!(
            m.cpu_read(addr),
            ((!addr & 0x3F) as u8) | ((addr >> 8) as u8 & 0xC0)
        );
        // A8 clear, addr >= $5000 -> $FF.
        assert_eq!(m.cpu_read(0x5000), 0xFF);
        assert!(!m.cpu_read_unmapped(0x4100));
        // NROM-128: $8000 and $C000 read the same (only) 16 KiB bank (index 0).
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 0);
    }

    #[test]
    fn m143_save_state_round_trip() {
        let mut m =
            SachenTca01M143::new(synth_prg_16k(1), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.ppu_write(0x2000, 0x33);
        let blob = m.save_state();
        let mut m2 =
            SachenTca01M143::new(synth_prg_16k(1), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x2000), 0x33);
    }

    // --- Mapper 177 --------------------------------------------------------

    #[test]
    fn m177_prg_and_mirroring() {
        let mut m = Hengedianzi177::new(synth_prg_32k(8), &[]).unwrap();
        // value 0b0010_0011 (0x23): PRG = 0x23 % 8 = 3; bit5 set -> horizontal.
        m.cpu_write(0x8000, 0b0010_0011);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Clear bit 5 -> vertical.
        m.cpu_write(0x8000, 0b0000_0010);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m177_save_state_round_trip() {
        let mut m = Hengedianzi177::new(synth_prg_32k(8), &[]).unwrap();
        m.cpu_write(0x8000, 0b0010_0101); // PRG 5, horizontal
        m.ppu_write(0x0004, 0xEE);
        let blob = m.save_state();
        let mut m2 = Hengedianzi177::new(synth_prg_32k(8), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 5);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0004), 0xEE);
    }

    // --- Mapper 179 --------------------------------------------------------

    #[test]
    fn m179_prg_via_5000_and_mirror_via_8000() {
        let mut m = Hengedianzi179::new(synth_prg_32k(8), &[]).unwrap();
        // PRG = value >> 1; write 6 -> bank 3.
        m.cpu_write(0x5000, 6);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Mirroring bit at $8000-$FFFF (bit 0).
        m.cpu_write(0x8000, 0x01);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m179_save_state_round_trip() {
        let mut m = Hengedianzi179::new(synth_prg_32k(8), &[]).unwrap();
        m.cpu_write(0x5000, 8); // PRG 4
        m.cpu_write(0x8000, 0x01); // horizontal
        m.ppu_write(0x0006, 0x12);
        let blob = m.save_state();
        let mut m2 = Hengedianzi179::new(synth_prg_32k(8), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 4);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0006), 0x12);
    }

    // --- Mapper 58 ---------------------------------------------------------

    #[test]
    fn m58_address_decoded_banking() {
        let mut m =
            Multicart58::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // 16 KiB mode (bit 6 set): A = $8000 | (1<<6) | (CHR=2<<3) | (PRG=3).
        let addr = 0x8000 | (1 << 6) | (0b010 << 3) | 0b011;
        m.cpu_write(addr, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3); // 16 KiB bank 3
        assert_eq!(m.ppu_read(0x0000), 2); // CHR bank 2
        // 32 KiB mode (bit 6 clear): PRG bank = (A&6)>>1. A low bits = 0b110 = 6
        // -> 32 KiB bank (6&6)>>1 = 3.
        let addr32 = 0x8000 | 0b110;
        m.cpu_write(addr32, 0x00);
        // synth_prg_16k(8) = 4 32 KiB banks; bank 3 offset 0 holds index 6.
        assert_eq!(m.cpu_read(0x8000), 6);
    }

    #[test]
    fn m58_save_state_round_trip() {
        let mut m =
            Multicart58::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        let addr = 0x8000 | (1 << 7) | (1 << 6) | (0b001 << 3) | 0b010;
        m.cpu_write(addr, 0x00);
        let blob = m.save_state();
        let mut m2 =
            Multicart58::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 2);
        assert_eq!(m2.ppu_read(0x0000), 1);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }

    // --- Mapper 60 ---------------------------------------------------------

    #[test]
    fn m60_power_on_bank_zero() {
        let mut m =
            Multicart60::new(synth_prg_16k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.ppu_read(0x0000), 0);
        // Writes are ignored (reset-driven selection not modelled).
        m.cpu_write(0x8000, 0xFF);
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m60_save_state_round_trip() {
        let mut m =
            Multicart60::new(synth_prg_16k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m.ppu_write(0x2000, 0x44);
        let blob = m.save_state();
        let mut m2 =
            Multicart60::new(synth_prg_16k(4), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x2000), 0x44);
    }

    // --- Mapper 231 --------------------------------------------------------

    #[test]
    fn m231_dual_bank_address_decoded() {
        let mut m = Multicart231::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        // A bits: prgBank = ((A>>5)&1) | (A&0x1E). Pick A = $8000 | 0x14 (= 0b1_0100).
        // A&0x1E = 0x14 = 20; (A>>5)&1 = 0. prgBank = 20.
        // bank0 = 20 & 0x1E = 20; bank1 = 20.
        let addr = 0x8000 | 0x14;
        m.cpu_write(addr, 0x00);
        assert_eq!(m.cpu_read(0x8000), 20);
        assert_eq!(m.cpu_read(0xC000), 20);
        // Set bit 5 (0x20): contributes 1 to bank1's LSB; A&0x1E unchanged.
        let addr2 = 0x8000 | 0x20 | 0x14; // (A>>5)&1 = 1
        m.cpu_write(addr2, 0x00);
        // prgBank = 1 | 20 = 21; bank0 = 21 & 0x1E = 20; bank1 = 21.
        assert_eq!(m.cpu_read(0x8000), 20);
        assert_eq!(m.cpu_read(0xC000), 21);
    }

    #[test]
    fn m231_save_state_round_trip() {
        let mut m = Multicart231::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000 | 0x80 | 0x04, 0x00); // horizontal, bank bits
        m.ppu_write(0x0008, 0x66);
        let blob = m.save_state();
        let mut m2 = Multicart231::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0008), 0x66);
    }

    // --- Mapper 111 --------------------------------------------------------

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

    // --- Mapper 234 --------------------------------------------------------

    #[test]
    fn m234_cnrom_mode_banks() {
        let mut m =
            Maxi15M234::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // reg0 latch (CNROM mode, bit6 clear). Write $FF80 with value 0x05:
        //   reg0 = 0x05 -> prgBank = reg0 & 0x0F = 5.
        //   chrBank = ((reg0<<2)&0x3C) | ((reg1>>4)&3) = (0x14) | 0 = 20.
        m.cpu_write(0xFF80, 0x05);
        assert_eq!(m.cpu_read(0x8000), 5);
        // chrBank = ((0x05<<2)&0x3C) = 0x14 = 20; 16-bank ROM wraps to 4.
        assert_eq!(m.ppu_read(0x0000), 20 % 16);
        // reg1 sets CHR low bits via $FFE8 (value 0x10 -> (0x10>>4)&3 = 1).
        m.cpu_write(0xFFE8, 0x10);
        // chrBank = 0x14 | 0x01 = 0x15 = 21; wraps to 5.
        assert_eq!(m.ppu_read(0x0000), 0x15 % 16);
    }

    #[test]
    fn m234_save_state_round_trip() {
        let mut m =
            Maxi15M234::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0xFF80, 0x83); // reg0: horizontal (bit7), prg = 3
        m.cpu_write(0xFFE8, 0x20);
        let blob = m.save_state();
        let mut m2 =
            Maxi15M234::new(synth_prg_32k(8), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), 3);
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }
}
