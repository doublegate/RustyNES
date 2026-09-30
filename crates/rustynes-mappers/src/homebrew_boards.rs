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
//! the `GeraNES` reference emulator (cross-referenced, not copied)
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
use crate::sst39sf040::{
    Sst39sf040, decode_sector_diff, encode_sector_diff, sector_bitmap_len, sector_diff_len,
};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_4K: usize = 0x1000;

/// The exact length of a flash board's save state: `fixed` bytes, then the
/// sector diff (`sst39sf040.rs`). A diff whose bitmap is cut short is
/// `Truncated`, reporting the length the bitmap alone needs. A bitmap bit past
/// the end of the chip is one `encode_sector_diff` never writes, so `Invalid`
/// (`docs/mappers.md` gotcha 12). The caller has already checked
/// `data.len() >= fixed`.
fn flash_state_len(
    mapper: u16,
    fixed: usize,
    flash_len: usize,
    data: &[u8],
) -> Result<usize, MapperError> {
    let tail = &data[fixed..];
    let bitmap = sector_bitmap_len(flash_len);
    if tail.len() < bitmap {
        return Err(MapperError::Truncated {
            expected: fixed + bitmap,
            got: data.len(),
        });
    }
    sector_diff_len(flash_len, tail)
        .map(|n| fixed + n)
        .ok_or_else(|| {
            MapperError::Invalid(format!(
                "mapper {mapper} flash bitmap marks a sector past the {flash_len}-byte chip"
            ))
        })
}
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

/// Mapper 111 (`GTROM` / Cheapocabra), written from
/// `nesdev_wiki/output/GTROM.md` (v2.9.6 "Roster": the register window, bonus
/// RAM and self-flashing were added and the board promoted to Curated).
///
/// - **Register** (`GRNC PPPP`): 32 KiB PRG bank, 8 KiB CHR-RAM bank, 8 KiB
///   nametable page, and two LEDs. The latch clocks when `/ROMSEL`, A14 and
///   A12 are all high, which is `$5000-$5FFF` and `$7000-$7FFF` and nowhere
///   else; `$6000-$6FFF` is not decoded. A read there latches too, with the
///   value floating on the bus ("reading from the register effectively
///   writes the value of open bus"), which is what
///   [`Mapper::notify_floating_read`] exists for.
/// - **PPU RAM** is one 32 KiB chip. The pattern tables use one of its first
///   two 8 KiB pages, and PPU `$2000-$3EFF` one of its last two, unmirrored.
///   Each nametable page therefore holds the four nametables plus almost
///   4 KiB of bonus RAM at `$3000-$3EFF`. The console's CIRAM is disabled.
/// - **PRG** is an SST39SF040 (`sst39sf040.rs`). Writes to `$8000-$FFFF` are
///   its commands; command addresses are A14-A0, so `5555h` is CPU `$D555`
///   and `2AAAh` is `$AAAA` in any bank. The flashed image is the board's
///   battery save ([`Mapper::save_data`]; `sram()` stays empty, since no
///   RAM sits at `$6000`), and a save state carries only the
///   sectors that differ from the ROM.
///
/// The LEDs have no emulated effect. Their bits are kept in the register so a
/// debugger shows them.
pub struct Gtrom111 {
    /// The flash contents: PRG-ROM as loaded, plus whatever was flashed.
    flash: Box<[u8]>,
    /// The PRG-ROM as loaded, for the sector diff in save states.
    original: Box<[u8]>,
    chip: Sst39sf040,
    /// 16 KiB of pattern-table RAM: two 8 KiB pages.
    chr_ram: Box<[u8]>,
    /// 16 KiB of nametable RAM: two 8 KiB pages covering `$2000-$3FFF`.
    nt_ram: Box<[u8]>,
    /// The last value latched (`GRNC PPPP`).
    reg: u8,
    prg_bank: u8,
    chr_bank: u8,
    nt_bank: u8,
}

/// GTROM save-state layout version. v2 (v2.9.6) grew the nametable pages to
/// 8 KiB and added the register, the flash state and the flashed sectors.
const GTROM_STATE_VERSION: u8 = 2;

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
            original: prg_rom.clone(),
            flash: prg_rom,
            chip: Sst39sf040::new(),
            chr_ram: vec![0u8; 2 * CHR_BANK_8K].into_boxed_slice(),
            nt_ram: vec![0u8; 2 * CHR_BANK_8K].into_boxed_slice(),
            reg: 0,
            prg_bank: 0,
            chr_bank: 0,
            nt_bank: 0,
        })
    }

    #[allow(clippy::cast_possible_truncation)]
    fn update_register(&mut self, value: u8) {
        let count = (self.flash.len() / PRG_BANK_32K).max(1);
        self.reg = value;
        // `(value & 0x0F) % count` < 16, so the cast cannot truncate.
        self.prg_bank = ((value & 0x0F) as usize % count) as u8;
        self.chr_bank = (value >> 4) & 0x01;
        self.nt_bank = (value >> 5) & 0x01;
    }

    /// The latch decodes `/ROMSEL` high, A14 high, A12 high.
    const fn is_register(addr: u16) -> bool {
        matches!(addr, 0x5000..=0x5FFF | 0x7000..=0x7FFF)
    }

    const fn chr_offset(&self, addr: u16) -> usize {
        (self.chr_bank as usize) * CHR_BANK_8K + (addr as usize & 0x1FFF)
    }

    /// `$2000-$3EFF`, unmirrored within the selected 8 KiB page.
    const fn nt_offset(&self, addr: u16) -> usize {
        (self.nt_bank as usize) * CHR_BANK_8K + (addr as usize & 0x1FFF)
    }

    fn chip_addr(&self, addr: u16) -> usize {
        (self.prg_bank as usize) * PRG_BANK_32K + (addr as usize & 0x7FFF)
    }
}

impl Mapper for Gtrom111 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    /// The flash image: what a self-flashing GTROM game saves to. There is
    /// no RAM at `$6000`, so `sram()` stays empty.
    fn save_data(&self) -> &[u8] {
        &self.flash
    }

    fn save_data_mut(&mut self) -> &mut [u8] {
        &mut self.flash
    }

    fn clear_save_data(&mut self) {
        self.flash.copy_from_slice(&self.original);
    }

    /// The register is write-only and nothing else lives below `$8000`.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        addr < 0x8000
    }

    fn notify_floating_read(&mut self, addr: u16, value: u8) {
        if Self::is_register(addr) {
            self.update_register(value);
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x8000 {
            let a = self.chip_addr(addr);
            self.chip.id_read(a).unwrap_or(self.flash[a])
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if Self::is_register(addr) {
            self.update_register(value);
        } else if addr >= 0x8000 {
            let a = self.chip_addr(addr);
            self.chip.write(&mut self.flash, a, value);
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

    fn nametable_unfolded(&self) -> bool {
        true
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

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 111,
            name: "GTROM (111)".into(),
            mirroring: crate::mapper::mirroring_name(Mirroring::FourScreen),
            ..Default::default()
        };
        info.prg_banks
            .push(("32K".into(), format!("{:#04x}", self.prg_bank)));
        info.chr_banks
            .push(("8K".into(), format!("{}", self.chr_bank)));
        info.extra
            .push(("nt page".into(), format!("{}", self.nt_bank)));
        info.extra.push((
            "LEDs".into(),
            format!(
                "red {} green {}",
                if self.reg & 0x40 == 0 { "on" } else { "off" },
                if self.reg & 0x80 == 0 { "on" } else { "off" }
            ),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.chr_ram.len() + self.nt_ram.len());
        out.push(GTROM_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(self.nt_bank);
        out.push(self.reg);
        out.extend_from_slice(&self.chip.to_bytes());
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.nt_ram);
        encode_sector_diff(&self.flash, &self.original, &mut out);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let fixed = 7 + self.chr_ram.len() + self.nt_ram.len();
        if data.len() < fixed {
            return Err(MapperError::Truncated {
                expected: fixed,
                got: data.len(),
            });
        }
        if data[0] != GTROM_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        // v2.9.0 (re-audit NC-02): validate before assigning anything. The
        // fetch paths index with these banks unmasked (`cpu_read`,
        // `chr_offset`, `nt_offset`), so a corrupt value loaded cleanly and
        // panicked on the next fetch, after the restore had returned `Ok`.
        // These are exactly the values `update_register` can produce: a PRG
        // bank below the 32 KiB bank count, and 0 or 1 for the CHR-RAM and
        // nametable banks.
        let prg_banks = self.flash.len() / PRG_BANK_32K;
        let (prg_bank, chr_bank, nt_bank) = (data[1], data[2], data[3]);
        if usize::from(prg_bank) >= prg_banks || chr_bank > 1 || nt_bank > 1 {
            return Err(MapperError::Invalid(format!(
                "mapper 111 state banks PRG {prg_bank} / CHR {chr_bank} / NT {nt_bank} \
                 exceed the board ({prg_banks} PRG banks, 2 CHR, 2 NT)"
            )));
        }
        let chip = Sst39sf040::from_bytes([data[5], data[6]]).ok_or_else(|| {
            MapperError::Invalid(format!(
                "mapper 111 flash state {:#04x} {:#04x} is not one the chip produces",
                data[5], data[6]
            ))
        })?;
        let expected = flash_state_len(111, fixed, self.flash.len(), data)?;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        let mut flash = vec![0u8; self.flash.len()];
        decode_sector_diff(&mut flash, &self.original, &data[fixed..])
            .ok_or_else(|| MapperError::Invalid("mapper 111 flash diff".into()))?;
        self.flash.copy_from_slice(&flash);
        self.prg_bank = prg_bank;
        self.chr_bank = chr_bank;
        self.nt_bank = nt_bank;
        self.reg = data[4];
        self.chip = chip;
        let mut cursor = 7;
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.nt_ram
            .copy_from_slice(&data[cursor..cursor + self.nt_ram.len()]);
        Ok(())
    }
}

/// Mapper 28 (Action 53 homebrew multicart).
///
/// Implemented from the NESdev wiki "Action 53 mapper" page (vendored at
/// `nesdev_wiki/output/Action_53_mapper.md`) and pinned to Damian Yerrick's
/// `test28` ROM (`tests/roms/nes-test-roms/other/test28.nes`).
///
/// Four registers are selected through `$5000-$5FFF` (bit 7 = supervisor, bit
/// 0 = register) and written through `$8000-$FFFF`, with no bus conflicts:
///
/// * `$00` CHR bank: bits 0-1 pick one of four 8 KiB banks of the 32 KiB
///   CHR RAM.
/// * `$01` inner PRG bank: bits 0-3.
/// * `$80` mode: bits 0-1 mirroring (0/1 = 1-screen lower/upper, 2 =
///   vertical, 3 = horizontal), bits 2-3 PRG mode, bits 4-5 outer bank size
///   (32/64/128/256 KiB).
/// * `$81` outer PRG bank: all 8 bits.
///
/// While mirroring is 1-screen, D4 of a write to `$00` or `$01` replaces
/// mirroring bit 0 (AxROM's single-screen select); in V/H it is ignored.
///
/// PRG resolution follows the wiki's 12-row table: the "o" bits of the 16 KiB
/// bank number come from the top of the outer register and the "i" bits from
/// the bottom of the inner register, the number of inner bits growing with the
/// outer bank size. The fixed half of the UNROM-style modes (`$8000` in mode
/// 2, `$C000` in mode 3) is resolved as if the size were 32 KiB, so all outer
/// bits pass straight through. Power-on maps the last 16 KiB at `$C000`; reset
/// leaves the mapper untouched.
///
/// Until v2.9.3 this board shifted the outer bank left instead of masking its
/// low bits, masked the inner bank to one bit, fixed the wrong half in modes 2
/// and 3, ignored the CHR bank register and the D4 mirroring write, and
/// powered on at bank 1 in `$C000` -- so `test28` failed its first check. The
/// review thread on #97 reported the banking half.
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

/// Mapper 28's own save-state section version. Version 2 (v2.9.3) carries the
/// full 32 KiB of CHR RAM; version 1 carried 8 KiB and still loads, into bank 0.
const M28_STATE_VERSION: u8 = 2;
/// CHR RAM on an Action 53 board: four 8 KiB banks.
const M28_CHR_RAM: usize = 4 * CHR_BANK_8K;

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
            chr_ram: vec![0u8; M28_CHR_RAM].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_select: 0,
            chr_reg: 0,
            inner_prg: 0,
            // Power-on: the wiki specifies only that the last 16 KiB sits at
            // $C000. Mode 0 (32 KiB, size 32 KiB) with every outer bit set
            // resolves $C000 to bank `...1_1111_1111`, i.e. the last bank of
            // any power-of-two ROM once reduced modulo the bank count, and
            // $8000 to the one before it.
            mode: 0,
            outer_prg: 0xFF,
        })
    }

    /// Resolve the 16 KiB PRG bank serving a CPU address in $8000-$FFFF.
    ///
    /// The bank number is `outer << 1 | half`, with its low `size + 1` bits
    /// replaced by inner-bank bits (the wiki table's "i" positions). For the
    /// 32 KiB modes the replaced field is `inner << 1 | half`; for the
    /// switchable half of the UNROM-style modes it is `inner` itself; the
    /// fixed half keeps `outer << 1 | half` untouched (resolved as size 0).
    fn prg_bank_for(&self, addr: u16) -> usize {
        let count16 = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let size = u32::from((self.mode >> 4) & 0x03);
        let prg_mode = (self.mode >> 2) & 0x03;
        let high = addr >= 0xC000;
        let half = usize::from(high);
        let inner = usize::from(self.inner_prg & 0x0F);
        let outer_bits = (usize::from(self.outer_prg) << 1) | half;
        // `size + 1` low bits of the bank number come from the inner register.
        let mask = (1usize << (size + 1)) - 1;
        let bank = match prg_mode {
            // BNROM / AOROM: one 32 KiB bank.
            0 | 1 => (outer_bits & !mask) | (((inner << 1) | half) & mask),
            // UNROM #180: $8000 fixed (resolved as 32 KiB), $C000 switchable.
            // UNROM #2:   $8000 switchable, $C000 fixed (resolved as 32 KiB).
            2 | 3 => {
                let fixed = if prg_mode == 2 { !high } else { high };
                if fixed {
                    outer_bits
                } else {
                    (outer_bits & !mask) | (inner & mask)
                }
            }
            _ => unreachable!("PRG mode is two bits"),
        };
        bank % count16
    }

    /// Offset into the 32 KiB CHR RAM for a pattern-table address.
    fn chr_offset(&self, addr: u16) -> usize {
        usize::from(self.chr_reg & 0x03) * CHR_BANK_8K + usize::from(addr & 0x1FFF)
    }

    /// Apply a write's D4 to mirroring bit 0 while the mode is 1-screen.
    const fn latch_one_screen(&mut self, value: u8) {
        if self.mode & 0x02 == 0 {
            self.mode = (self.mode & !0x01) | ((value >> 4) & 0x01);
        }
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
                0x00 => {
                    self.chr_reg = value & 0x03;
                    self.latch_one_screen(value);
                }
                0x01 => {
                    self.inner_prg = value & 0x0F;
                    self.latch_one_screen(value);
                }
                0x80 => self.mode = value & 0x3F,
                _ => self.outer_prg = value,
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
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
        out.push(M28_STATE_VERSION);
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
        // Version 1 (before v2.9.3) carried 8 KiB of CHR RAM; it restores into
        // bank 0 with the other three banks cleared. Its register bytes mean
        // the same thing, so only the CHR length differs.
        let version = *data.first().ok_or(MapperError::Truncated {
            expected: 1,
            got: 0,
        })?;
        let chr_len = match version {
            M28_STATE_VERSION => self.chr_ram.len(),
            1 => CHR_BANK_8K,
            v => return Err(MapperError::UnsupportedVersion(v)),
        };
        let expected = 6 + self.vram.len() + chr_len;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.reg_select = data[1] & 0x81;
        self.chr_reg = data[2] & 0x03;
        self.inner_prg = data[3] & 0x0F;
        self.mode = data[4] & 0x3F;
        self.outer_prg = data[5];
        let mut cursor = 6;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram.fill(0);
        self.chr_ram[..chr_len].copy_from_slice(&data[cursor..cursor + chr_len]);
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
//     $8000-$BFFF is the flash-write window: a write there reaches the
//     SST39SF040 at bank * 16 KiB + (addr & $3FFF), so `$9555` in bank 1 is its
//     `5555h` and `$AAAA` in bank 0 its `2AAAh` (`UNROM_512.md`). Since
//     v2.9.6 the chip is modelled (`sst39sf040.rs`) and the flashed image is
//     the battery save, where before v2.9.6 the write was dropped.
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
    /// Four-screen, cartridge VRAM (the `InfiniteNESLives` board): the last
    /// 8 KiB of the 32 KiB CHR-RAM is mapped to PPU `$2000-$3EFF`, the four
    /// nametables at `$2000-$2FFF` and independent RAM at `$3000-$3EFF`
    /// (`UNROM_512.md`, "InfiniteNESLives 4-screen board"). Until v2.9.6 this
    /// was approximated as single-screen.
    FourScreen,
}

/// UNROM 512 save-state layout version. v2 (v2.9.6) appends the flash
/// chip's command state and, on a flashable board, the flashed sectors.
const M30_STATE_VERSION: u8 = 2;

/// Mapper 30 (`UNROM-512`).
///
/// The four booleans mirror distinct iNES-header-derived wirings (CHR-ROM vs
/// RAM, the latch nametable bit, bus-conflict presence, and the flash-window
/// banking mode), so they don't fold into an enum without losing fidelity.
#[allow(clippy::struct_excessive_bools)]
pub struct Unrom512M30 {
    /// PRG: the SST39SF040's contents on a flashable board.
    prg_rom: Box<[u8]>,
    /// The PRG as loaded, for the save state's sector diff (flashable only).
    original: Box<[u8]>,
    chip: Sst39sf040,
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
            original: if flash_window && !chr_is_rom {
                prg_rom.clone()
            } else {
                Box::new([])
            },
            prg_rom,
            chip: Sst39sf040::new(),
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
        let a = self.prg_chip_addr(bank, addr);
        self.chip.id_read(a).unwrap_or(self.prg_rom[a])
    }

    /// Whether writes to `$8000-$BFFF` reach a flash chip. The flashable
    /// wiring needs the flash window AND CHR-RAM: UNROM 512 carries CHR-RAM,
    /// while the CHR-ROM images headered as mapper 30 are Waixing FS005 `.WXN`
    /// conversions (`UNROM_512.md`), whose MMC3-style register writes would
    /// otherwise program the "ROM". Measured on *Shui Hu Zhuan*: 7,012 such
    /// writes in 1,200 frames rewrote 21,602 bytes of it before this check.
    const fn flashable(&self) -> bool {
        self.flash_window && !self.chr_is_rom
    }

    fn prg_chip_addr(&self, bank: usize, addr: u16) -> usize {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        (bank % count) * PRG_BANK_16K + (addr as usize & 0x3FFF)
    }

    /// The four-screen board's nametable RAM, the last 8 KiB of CHR-RAM,
    /// when the board has the full 32 KiB it is defined for.
    fn four_screen_offset(&self, addr: u16) -> Option<usize> {
        (self.nametable == M30Nametable::FourScreen
            && !self.chr_is_rom
            && self.chr.len() == 4 * CHR_BANK_8K)
            .then(|| 3 * CHR_BANK_8K + (addr as usize & 0x1FFF))
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

    /// The flash image on a flashable board, its save; nothing otherwise.
    /// There is never RAM at `$6000`, so `sram()` stays empty.
    fn save_data(&self) -> &[u8] {
        if self.flashable() { &self.prg_rom } else { &[] }
    }

    fn save_data_mut(&mut self) -> &mut [u8] {
        if self.flashable() {
            &mut self.prg_rom
        } else {
            &mut []
        }
    }

    fn clear_save_data(&mut self) {
        if self.flashable() {
            self.prg_rom.copy_from_slice(&self.original);
        }
    }

    /// Nothing drives `$4020-$7FFF` on any wiring.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        addr < 0x8000
    }

    fn nametable_unfolded(&self) -> bool {
        self.four_screen_offset(0x2000).is_some()
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        self.four_screen_offset(addr).map(|off| self.chr[off])
    }

    fn nametable_write(&mut self, addr: u16, value: u8) -> bool {
        match self.four_screen_offset(addr) {
            Some(off) => {
                self.chr[off] = value;
                true
            }
            None => false,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return;
        }
        if self.flash_window {
            // No-bus-conflict wiring: the banking latch lives at $C000-$FFFF;
            // $8000-$BFFF writes reach the SST39SF040 in the selected bank.
            if addr >= 0xC000 {
                self.write_latch(addr, value);
            } else if self.flashable() {
                let a = self.prg_chip_addr(self.prg_bank as usize, addr);
                self.chip.write(&mut self.prg_rom, a, value);
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
            // The four-screen board with its 32 KiB of CHR-RAM owns the
            // nametables outright.
            M30Nametable::FourScreen if self.four_screen_offset(0x2000).is_some() => {
                Mirroring::FourScreen
            }
            // Software-switchable single-screen: latch bit 7 selects which CIRAM
            // half (A10=0 lower, A10=1 upper). A four-screen header on a board
            // without 32 KiB of CHR-RAM, which the page leaves undefined, keeps
            // this single-screen base.
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
        let mut out = Vec::with_capacity(6 + self.vram.len() + chr_len);
        out.push(M30_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.nt_bit));
        out.extend_from_slice(&self.vram);
        // CHR-ROM is immutable; only persist CHR-RAM contents.
        if !self.chr_is_rom {
            out.extend_from_slice(&self.chr);
        }
        out.extend_from_slice(&self.chip.to_bytes());
        if self.flashable() {
            encode_sector_diff(&self.prg_rom, &self.original, &mut out);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_len = if self.chr_is_rom { 0 } else { self.chr.len() };
        let fixed = 6 + self.vram.len() + chr_len;
        if data.len() < fixed {
            return Err(MapperError::Truncated {
                expected: fixed,
                got: data.len(),
            });
        }
        if data[0] != M30_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        // Validate everything before assigning anything: a refused state
        // leaves the board, and above all its flash, as it was.
        let (s0, s1) = (data[fixed - 2], data[fixed - 1]);
        let chip = Sst39sf040::from_bytes([s0, s1]).ok_or_else(|| {
            MapperError::Invalid(format!(
                "mapper 30 flash state {s0:#04x} {s1:#04x} is not one the chip produces"
            ))
        })?;
        let expected = if self.flashable() {
            flash_state_len(30, fixed, self.prg_rom.len(), data)?
        } else {
            fixed
        };
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if self.flashable() {
            let mut flash = vec![0u8; self.prg_rom.len()];
            decode_sector_diff(&mut flash, &self.original, &data[fixed..])
                .ok_or_else(|| MapperError::Invalid("mapper 30 flash diff".into()))?;
            self.prg_rom.copy_from_slice(&flash);
        }
        self.chip = chip;
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

    /// v2.9.6: the latch decodes `/ROMSEL`, A14 and A12 high: `$5000` and
    /// `$7000` pages only (`GTROM.md`, "Hardware Teardown").
    #[test]
    fn m111_register_window_is_5000_and_7000_only() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        m.cpu_write(0x6000, 0x03);
        assert_eq!(m.cpu_read(0x8000), 0, "$6000 is not decoded");
        m.cpu_write(0x4FFF, 0x03);
        assert_eq!(m.cpu_read(0x8000), 0, "$4FFF is not decoded");
        m.cpu_write(0x7ABC, 0x03);
        assert_eq!(m.cpu_read(0x8000), 3);
        m.cpu_write(0x5FFF, 0x04);
        assert_eq!(m.cpu_read(0x8000), 4);
        assert!(m.cpu_read_unmapped(0x5000), "the register is write-only");
    }

    /// "reading from the register effectively writes the value of open bus".
    #[test]
    fn m111_a_read_latches_the_floating_value() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        m.notify_floating_read(0x5000, 0x26);
        assert_eq!(m.cpu_read(0x8000), 6);
        m.notify_floating_read(0x6000, 0x01);
        assert_eq!(m.cpu_read(0x8000), 6, "outside the window: no latch");
    }

    /// PPU `$3000-$3EFF` is RAM of its own, per nametable page.
    #[test]
    fn m111_bonus_ram_at_3000_is_not_a_mirror() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        assert!(m.nametable_unfolded());
        m.nametable_write(0x2123, 0x11);
        m.nametable_write(0x3123, 0x22);
        assert_eq!(m.nametable_fetch(0x2123), Some(0x11));
        assert_eq!(m.nametable_fetch(0x3123), Some(0x22));
        m.cpu_write(0x5000, 0x20); // the other nametable page
        assert_eq!(m.nametable_fetch(0x3123), Some(0x00));
        m.nametable_write(0x3EFF, 0x33);
        m.cpu_write(0x5000, 0x00);
        assert_eq!(m.nametable_fetch(0x3123), Some(0x22));
        assert_eq!(m.nametable_fetch(0x3EFF), Some(0x00));
    }

    /// Self-flashing: `5555h` is `$D555` and `2AAAh` is `$AAAA` in any bank.
    #[test]
    fn m111_flash_program_and_erase_through_the_cpu_window() {
        let mut m = Gtrom111::new(synth_prg_32k(16), &[]).unwrap();
        m.cpu_write(0x5000, 0x07);
        for (a, v) in [
            (0xD555u16, 0xAAu8),
            (0xAAAA, 0x55),
            (0xD555, 0xA0),
            (0x9000, 0x3C),
        ] {
            m.cpu_write(a, v);
        }
        assert_eq!(m.cpu_read(0x9000), 0x3C, "programmed");
        assert_eq!(
            m.save_data()[7 * PRG_BANK_32K + 0x1000],
            0x3C,
            "save_data() is the flash"
        );
        assert!(m.sram().is_empty(), "GTROM has no RAM at $6000");
        m.cpu_write(0x5000, 0x02);
        assert_eq!(m.cpu_read(0x9000), 0xFF, "another bank is untouched");
        m.cpu_write(0x5000, 0x07);
        for (a, v) in [
            (0xD555u16, 0xAAu8),
            (0xAAAA, 0x55),
            (0xD555, 0x80),
            (0xD555, 0xAA),
            (0xAAAA, 0x55),
            (0x9800, 0x30),
        ] {
            m.cpu_write(a, v);
        }
        assert_eq!(m.cpu_read(0x9000), 0xFF, "the 4 KiB sector is erased");
        assert_eq!(m.cpu_read(0x8000), 0x07, "the neighbouring sector is not");
    }

    #[test]
    fn m111_state_carries_only_flashed_sectors() {
        let mut m = Gtrom111::new(synth_prg_32k(16), &[]).unwrap();
        let clean = m.save_state().len();
        for (a, v) in [
            (0xD555u16, 0xAAu8),
            (0xAAAA, 0x55),
            (0xD555, 0xA0),
            (0xC001, 0x00),
        ] {
            m.cpu_write(a, v);
        }
        m.nametable_write(0x3456, 0x78);
        let blob = m.save_state();
        assert_eq!(blob.len(), clean + 0x1000, "one flashed sector");
        let mut m2 = Gtrom111::new(synth_prg_32k(16), &[]).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xC001), 0x00);
        assert_eq!(m2.nametable_fetch(0x3456), Some(0x78));
        assert_eq!(m2.save_state(), blob);
        // A clean state restores the loaded ROM over a flashed one.
        let fresh = Gtrom111::new(synth_prg_32k(16), &[]).unwrap().save_state();
        m2.load_state(&fresh).unwrap();
        assert_eq!(m2.cpu_read(0xC001), 0xFF);
    }

    /// v2.9.0 re-audit NC-02: a restored bank the board cannot hold is
    /// rejected, not stored. `load_state` assigned `prg_bank`, `chr_bank` and
    /// `nt_bank` raw from the blob and the fetch paths use them unmasked, so a
    /// corrupt `.rns` (or RetroArch `.state`) for a GTROM game loaded cleanly
    /// and panicked on the next CPU fetch from `$8000` — too late for the
    /// restore's rollback to help. `update_register` can only produce a PRG
    /// bank below the 32 KiB bank count and 0/1 for the other two.
    #[test]
    fn m111_load_state_rejects_banks_the_board_cannot_hold() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        m.cpu_write(0x5000, 0b0011_0011); // PRG 3, CHR 1, NT 1
        let good = m.save_state();
        for (byte, value) in [(1, 8), (1, 0xFF), (2, 2), (2, 0xFF), (3, 2), (3, 0xFF)] {
            let mut bad = good.clone();
            bad[byte] = value;
            let mut m2 = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
            assert!(
                matches!(m2.load_state(&bad), Err(MapperError::Invalid(_))),
                "byte {byte} = {value:#04x} must be rejected"
            );
        }
        // Every value the register can produce still loads.
        for (byte, max) in [(1, 7), (2, 1), (3, 1)] {
            for value in 0..=max {
                let mut ok = good.clone();
                ok[byte] = value;
                let mut m2 = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
                m2.load_state(&ok).unwrap();
                let _ = m2.cpu_read(0xFFFF);
                let _ = m2.ppu_read(0x1FFF);
                let _ = m2.nametable_fetch(0x2FFF);
            }
        }
    }

    /// The NESdev "Action 53 mapper" table (A22-A14 output per mode value and
    /// outer bank size), transcribed row for row as data. `o` = outer-bank bit
    /// taken from the TOP of `$81`, `i` = inner-bank bit taken from the
    /// BOTTOM of `$01`, `0`/`1` = a literal (CPU A14 in the 32 KiB modes).
    const M28_WIKI_TABLE: [(u8, &str, &str); 12] = [
        (0x00, "oooooooo0", "oooooooo1"),
        (0x08, "oooooooo0", "ooooooooi"),
        (0x0C, "ooooooooi", "oooooooo1"),
        (0x10, "oooooooi0", "oooooooi1"),
        (0x18, "oooooooo0", "oooooooii"),
        (0x1C, "oooooooii", "oooooooo1"),
        (0x20, "ooooooii0", "ooooooii1"),
        (0x28, "oooooooo0", "ooooooiii"),
        (0x2C, "ooooooiii", "oooooooo1"),
        (0x30, "oooooiii0", "oooooiii1"),
        (0x38, "oooooooo0", "oooooiiii"),
        (0x3C, "oooooiiii", "oooooooo1"),
    ];

    /// Expand one table pattern into a 9-bit bank number.
    fn m28_expected(pattern: &str, outer: u8, inner: u8) -> usize {
        let os = pattern.bytes().filter(|&c| c == b'o').count();
        let is = pattern.bytes().filter(|&c| c == b'i').count();
        // "o"s are the topmost outer bits, "i"s the bottommost inner bits.
        let mut o_bits = (0..os).map(|k| (outer >> (7 - k)) & 1);
        let mut i_bits = (0..is).rev().map(|k| (inner >> k) & 1);
        pattern.bytes().fold(0usize, |acc, c| {
            let bit = match c {
                b'o' => o_bits.next().unwrap(),
                b'i' => i_bits.next().unwrap(),
                b'0' => 0,
                _ => 1,
            };
            (acc << 1) | usize::from(bit)
        })
    }

    #[test]
    fn m28_prg_banking_matches_every_row_of_the_wiki_table() {
        // A 512-bank (8 MiB) image, so the modulo in `prg_bank_for` never
        // wraps and all nine output bits (A22-A14) are compared.
        let mut m = Action53M28::new(synth_prg_16k(512), &[], Mirroring::Vertical).unwrap();
        for (mode_base, lo, hi) in M28_WIKI_TABLE {
            // Each row covers a range of mode values; the table's row value
            // plus every mirroring setting (and, for the 32 KiB rows, both
            // PRG-mode encodings 0 and 1) must resolve identically.
            let variants: &[u8] = if mode_base & 0x0C == 0 {
                &[0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7]
            } else {
                &[0x0, 0x1, 0x2, 0x3]
            };
            for &v in variants {
                m.mode = mode_base | v;
                for outer in 0..=255u8 {
                    for inner in 0..16u8 {
                        m.outer_prg = outer;
                        m.inner_prg = inner;
                        assert_eq!(
                            m.prg_bank_for(0x8000),
                            m28_expected(lo, outer, inner),
                            "mode ${:02X} outer ${outer:02X} inner {inner} at $8000",
                            m.mode
                        );
                        assert_eq!(
                            m.prg_bank_for(0xC000),
                            m28_expected(hi, outer, inner),
                            "mode ${:02X} outer ${outer:02X} inner {inner} at $C000",
                            m.mode
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn m28_powers_on_with_the_last_bank_at_c000() {
        // test28's first check ("DOES NOT POWER ON WITH LAST BANK IN
        // $C000-$FFFF"), which the pre-v2.9.3 board failed.
        let mut m = Action53M28::new(synth_prg_16k(32), &[], Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0xC000), 31);
    }

    #[test]
    fn m28_chr_register_banks_32k_of_chr_ram() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        for bank in 0..4u8 {
            m.cpu_write(0x5000, 0x00);
            m.cpu_write(0x8000, bank);
            m.ppu_write(0x0123, 0xA0 | bank);
        }
        for bank in 0..4u8 {
            m.cpu_write(0x5000, 0x00);
            m.cpu_write(0x8000, bank);
            assert_eq!(m.ppu_read(0x0123), 0xA0 | bank, "CHR bank {bank}");
        }
    }

    #[test]
    fn m28_d4_selects_the_single_screen_only_in_one_screen_modes() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x80);
        m.cpu_write(0x8000, 0x00); // 1-screen lower
        m.cpu_write(0x5000, 0x01);
        m.cpu_write(0x8000, 0x10); // inner write with D4 set
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x5000, 0x00);
        m.cpu_write(0x8000, 0x00); // CHR write with D4 clear
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // Vertical: D4 is ignored.
        m.cpu_write(0x5000, 0x80);
        m.cpu_write(0x8000, 0x02);
        m.cpu_write(0x5000, 0x01);
        m.cpu_write(0x8000, 0x10);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m28_loads_a_version_1_state_with_8k_of_chr() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        let mut v1 = vec![1u8, 0x81, 0x00, 0x03, 0x0E, 0x02];
        v1.extend(core::iter::repeat_n(0u8, 2 * NAMETABLE_SIZE));
        let mut chr = vec![0u8; CHR_BANK_8K];
        chr[7] = 0x5A;
        v1.extend_from_slice(&chr);
        m.load_state(&v1).unwrap();
        assert_eq!(m.ppu_read(0x0007), 0x5A);
        assert_eq!(m.mode, 0x0E);
        assert_eq!(m.outer_prg, 0x02);
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

    /// v2.9.6: the flashable wiring programs the SST39SF040 with the
    /// wiki's own sequence (`UNROM_512.md`, "Write a byte").
    #[test]
    fn m30_flashable_board_programs_and_erases_the_chip() {
        let mut m = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        assert_eq!(
            m.save_data().len(),
            16 * PRG_BANK_16K,
            "the flash is the save"
        );
        assert!(m.sram().is_empty(), "no RAM at $6000");
        for (bank, a, v) in [
            (1u8, 0x9555u16, 0xAAu8),
            (0, 0xAAAA, 0x55),
            (1, 0x9555, 0xA0),
            (5, 0x8123, 0x42),
        ] {
            m.cpu_write(0xC000, bank);
            m.cpu_write(a, v);
        }
        m.cpu_write(0xC000, 5);
        assert_eq!(m.cpu_read(0x8123), 0x42);
        let blob = m.save_state();
        let mut m2 = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(
            m2.cpu_read(0x8123),
            0x42,
            "the flashed sector is in the state"
        );
        // Erase the sector again ("Erase 4KB Flash Sector").
        for (bank, a, v) in [
            (1u8, 0x9555u16, 0xAAu8),
            (0, 0xAAAA, 0x55),
            (1, 0x9555, 0x80),
            (1, 0x9555, 0xAA),
            (0, 0xAAAA, 0x55),
            (5, 0x8000, 0x30),
        ] {
            m.cpu_write(0xC000, bank);
            m.cpu_write(a, v);
        }
        m.cpu_write(0xC000, 5);
        assert_eq!(m.cpu_read(0x8123), 0xFF);
    }

    /// The reset a power-on movie performs (`power_on_for_movie` ->
    /// `clear_save_data`): a flashed UNROM 512 goes back to the image as
    /// loaded, byte for byte. Neither zeros (a ROM with no program in it) nor
    /// the flashed image (a movie that replays differently with a save) is
    /// right. GTROM's half is pinned end to end in `roster_boards.rs`.
    #[test]
    fn m30_clear_save_data_restores_the_image_as_loaded() {
        let fresh = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        let mut m = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        for (bank, a, v) in [
            (1u8, 0x9555u16, 0xAAu8),
            (0, 0xAAAA, 0x55),
            (1, 0x9555, 0xA0),
            (5, 0x8123, 0x00),
        ] {
            m.cpu_write(0xC000, bank);
            m.cpu_write(a, v);
        }
        assert_ne!(
            m.save_data(),
            fresh.save_data(),
            "the program changed the flash"
        );
        m.clear_save_data();
        assert_eq!(
            m.save_data(),
            fresh.save_data(),
            "back to the image as loaded"
        );
    }

    /// The flash chip's two state bytes take only the values `to_bytes` writes
    /// (`docs/mappers.md` gotcha 12): steps 0-6 and a 0/1 ID-mode flag. Both
    /// boards refuse anything else before assigning a field.
    #[test]
    fn flash_state_bytes_the_chip_cannot_produce_are_refused() {
        let gt = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        let good = gt.save_state();
        for (i, v) in [(5usize, 7u8), (5, 0xFF), (6, 2)] {
            let mut blob = good.clone();
            blob[i] = v;
            let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
            assert!(
                matches!(m.load_state(&blob), Err(MapperError::Invalid(_))),
                "GTROM byte {i} = {v:#x}"
            );
            assert_eq!(m.save_state(), good, "GTROM: nothing assigned");
        }
        let m30 = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        let good = m30.save_state();
        let fixed = 6 + m30.vram.len() + m30.chr.len();
        for (i, v) in [(fixed - 2, 7u8), (fixed - 1, 2)] {
            let mut blob = good.clone();
            blob[i] = v;
            let mut m = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
            assert!(
                matches!(m.load_state(&blob), Err(MapperError::Invalid(_))),
                "UNROM 512 byte {i} = {v:#x}"
            );
            assert_eq!(m.save_state(), good, "UNROM 512: nothing assigned");
        }
    }

    /// A cut-short flash section reports the length the state really needs.
    /// It used to report `fixed + 1`, whatever the bitmap said.
    #[test]
    fn m111_truncated_state_reports_its_exact_length() {
        let mut m = Gtrom111::new(synth_prg_32k(8), &[]).unwrap();
        for (a, v) in [
            (0xD555u16, 0xAAu8),
            (0xAAAA, 0x55),
            (0xD555, 0xA0),
            (0x8100, 0x42),
        ] {
            m.cpu_write(a, v);
        }
        let blob = m.save_state();
        let err = Gtrom111::new(synth_prg_32k(8), &[])
            .unwrap()
            .load_state(&blob[..blob.len() - 100])
            .unwrap_err();
        assert!(
            matches!(err, MapperError::Truncated { expected, .. } if expected == blob.len()),
            "{err:?}"
        );
    }

    /// A state refused for trailing bytes must leave the flash as it was. The
    /// decoded image used to be copied in before the length check.
    #[test]
    fn m30_refused_state_leaves_the_flash_untouched() {
        let mut a = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        for (bank, addr, v) in [
            (1u8, 0x9555u16, 0xAAu8),
            (0, 0xAAAA, 0x55),
            (1, 0x9555, 0xA0),
            (5, 0x8123, 0x00),
        ] {
            a.cpu_write(0xC000, bank);
            a.cpu_write(addr, v);
        }
        let mut blob = a.save_state();
        blob.push(0);
        let mut b = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 1, false).unwrap();
        let before = b.save_data().to_vec();
        assert!(b.load_state(&blob).is_err());
        assert_eq!(b.save_data(), &before[..], "the flash is unchanged");
    }

    /// A CHR-ROM image headered as mapper 30 is a Waixing FS005 `.WXN`
    /// conversion, not UNROM 512 (`UNROM_512.md`). Its register writes must not
    /// program a "flash": that rewrote 21,602 bytes of *Shui Hu Zhuan*.
    #[test]
    fn m30_chr_rom_image_is_never_flashed() {
        let chr = vec![0u8; 0x8000];
        let mut m = Unrom512M30::new(synth_prg_16k(16), &chr, false, false, 0, true).unwrap();
        assert!(m.save_data().is_empty(), "no flash save on a CHR-ROM image");
        for (a, v) in [
            (0x9555u16, 0xAAu8),
            (0xAAAA, 0x55),
            (0x9555, 0xA0),
            (0x8123, 0x00),
        ] {
            m.cpu_write(0xC000, 1);
            m.cpu_write(a, v);
        }
        m.cpu_write(0xC000, 0);
        assert_eq!(m.cpu_read(0x8123), 0xFF, "the ROM is untouched");
    }

    #[test]
    fn m30_non_flashable_board_has_no_flash() {
        let mut m = Unrom512M30::new(synth_prg_16k(16), &[], false, true, 0, false).unwrap();
        assert!(m.save_data().is_empty());
        m.cpu_write(0x9555, 0xAA);
        assert!(m.cpu_read_unmapped(0x6000));
    }

    /// The four-screen board: the last 8 KiB of the 32 KiB CHR-RAM is PPU
    /// `$2000-$3EFF`, `$3000-$3EFF` included.
    #[test]
    fn m30_four_screen_uses_the_last_chr_ram_bank() {
        let mut m = Unrom512M30::new(synth_prg_16k(16), &[], true, true, 0, true).unwrap();
        assert_eq!(m.current_mirroring(), Mirroring::FourScreen);
        assert!(m.nametable_unfolded());
        assert!(m.nametable_write(0x2C00, 0x44));
        assert!(m.nametable_write(0x3C00, 0x55));
        assert_eq!(m.nametable_fetch(0x2C00), Some(0x44));
        assert_eq!(m.nametable_fetch(0x3C00), Some(0x55));
        // The same bytes seen as pattern data in CHR bank 3.
        m.cpu_write(0xC000, 0x60);
        assert_eq!(m.ppu_read(0x0C00), 0x44);
        assert_eq!(m.ppu_read(0x1C00), 0x55);
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
