//! NTDEC boards decoded from the address bus: mappers 63 and 174.
//!
//! NTDEC's multicart designs consistently push the bank selection into the
//! *address* of the write rather than its data -- the cartridge decodes which
//! address in `$8000-$FFFF` was touched and banks accordingly. That costs
//! nothing in discrete logic (the address lines are already there) and needs
//! no data-bus buffer, which is why so many cheap multicarts work this way.
//!
//! NTDEC's later ASIC-based boards are in this module too as they are added;
//! see also `sachen_8259.rs` for the comparable Sachen family.
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

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
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
// Mapper 28 — Action 53 homebrew multicart.
//
// A single outer register at $5000-$5FFF selects which inner register a
// $8000-$FFFF write targets (reg index in bits 7-6 of the $5xxx value). The
// four inner registers are:
//   reg 0 ($00): CHR bank (8 KiB CHR-RAM is single-bank, so this only stores).
//   reg 1 ($01): low PRG bank bits.
//   reg 2 ($80): mode/mirroring: bits 0-1 = mirroring, bits 2-3 = PRG mode,
//                bits 4-5 = outer-bank size mask.
//   reg 3 ($81): outer PRG bank.
// We model the documented PRG-banking + mirroring; CHR is 8 KiB RAM. No IRQ.
//
// The resolved PRG layout follows the nesdev "Action 53" decode: the 32 KiB
// CPU window splits into two 16 KiB halves. Mode (bits 2-3 of reg 2) picks:
//   0/1 (NROM-256): both halves track the selected 32 KiB bank.
//   2  (UNROM):     $8000 = selectable 16 KiB, $C000 = fixed last-in-outer.
//   3  (NROM-128):  both halves mirror one 16 KiB bank.
// ===========================================================================

const CHR_BANK_1K: usize = 0x0400;

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

/// Mapper 63 (NTDEC `0324` multicart).
pub struct Ntdec63 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    prg32_mode: bool,
    horizontal_mirroring: bool,
}

impl Ntdec63 {
    /// Construct a new mapper 63 board.
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
                "mapper 63 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            prg32_mode: true,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Ntdec63 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let base = (self.prg_bank as usize) & if self.prg32_mode { !1 } else { !0 };
                self.read_prg(base, addr)
            }
            0xC000..=0xFFFF => {
                let base = self.prg_bank as usize;
                let bank = if self.prg32_mode {
                    (base & !1) | 1
                } else {
                    base
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = ((addr >> 2) & 0x3F) as u8;
            self.prg32_mode = (addr & 0x02) == 0;
            self.horizontal_mirroring = (addr & 0x01) != 0;
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
        out.push(self.prg_bank);
        out.push(u8::from(self.prg32_mode));
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
        self.prg_bank = data[1];
        self.prg32_mode = data[2] != 0;
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
// Mapper 76 — NAMCOT-3446 (Namco 109 variant, e.g. Digital Devil Story:
// Megami Tensei).
//
// MMC3-like register port at $8000 (index) / $8001 (data), but with only the
// CNROM-style 2 KiB CHR + simple PRG layout:
//   index 2 -> CHR bank 0 (2 KiB at $0000)
//   index 3 -> CHR bank 1 (2 KiB at $0800)
//   index 4 -> CHR bank 2 (2 KiB at $1000)
//   index 5 -> CHR bank 3 (2 KiB at $1800)
//   index 6 -> PRG bank at $8000 (8 KiB)
//   index 7 -> PRG bank at $A000 (8 KiB)
// $C000 and $E000 are fixed to the last two 8 KiB banks. Mirroring is
// header-fixed (the board has no mirroring register). No IRQ.
// ===========================================================================

/// Mapper 174 (NTDEC `5-in-1`).
pub struct Ntdec174 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    prg16_mode: bool,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Ntdec174 {
    /// Construct a new mapper 174 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 174 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 174 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            prg16_mode: false,
            chr_bank: 0,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Ntdec174 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let base = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    (self.prg_bank as usize) & !1
                };
                self.read_prg(base, addr)
            }
            0xC000..=0xFFFF => {
                let bank = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    ((self.prg_bank as usize) & !1) | 1
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = ((addr >> 4) & 0x07) as u8;
            self.prg16_mode = (addr & 0x80) != 0;
            self.chr_bank = ((addr >> 1) & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x01) != 0;
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
        out.push(u8::from(self.prg16_mode));
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
        self.prg16_mode = data[2] != 0;
        self.chr_bank = data[3];
        self.horizontal_mirroring = data[4] != 0;
        self.vram.copy_from_slice(&data[5..5 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 225 — ColorDreams 72-in-1 multicart.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   mode (bit 12): 0 = 32 KiB PRG, 1 = 16 KiB PRG.
//   high bit (bit 14): outer bank select (combines with the low bits).
//   PRG bank index = ((A >> 14) & 1) << 6 | ((A >> 7) & 0x3F)  (8-bit space)
//   CHR (8 KiB) bank = A & 0x3F (with the high bit folded in).
//   mirroring = (A >> 13) & 1 -> 1 = horizontal, 0 = vertical.
// A separate $5800-$5FFF four-byte scratch register block is modelled as RAM.
// CHR is ROM. No IRQ.
// ===========================================================================

/// Mapper 40 (NTDEC 2722, *SMB2J* pirate).
pub struct Ntdec2722M40 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    switch_bank: u8,
    irq_enabled: bool,
    irq_counter: u16,
    irq_pending: bool,
    mirroring: Mirroring,
}

impl Ntdec2722M40 {
    /// Construct a new mapper 40 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 40 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            switch_bank: 0,
            irq_enabled: false,
            irq_counter: 0,
            irq_pending: false,
            mirroring,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Ntdec2722M40 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.read_prg(6, addr),
            0x8000..=0x9FFF => self.read_prg(4, addr),
            0xA000..=0xBFFF => self.read_prg(5, addr),
            0xC000..=0xDFFF => self.read_prg(self.switch_bank as usize, addr),
            0xE000..=0xFFFF => self.read_prg(7, addr),
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $6000-$FFFF is mapped PRG; the $4020-$5FFF window is open bus.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                // IRQ disable + acknowledge; counter held in reset.
                self.irq_enabled = false;
                self.irq_pending = false;
                self.irq_counter = 0;
            }
            0xA000..=0xBFFF => self.irq_enabled = true,
            0xE000..=0xFFFF => self.switch_bank = value & 0x07,
            _ => {}
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

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        // 12-bit M2 counter; asserts (and holds) at 4096.
        if self.irq_counter >= 0x1000 {
            self.irq_pending = true;
        } else {
            self.irq_counter += 1;
            if self.irq_counter >= 0x1000 {
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.switch_bank);
        out.push(u8::from(self.irq_enabled));
        out.push((self.irq_counter & 0xFF) as u8);
        out.push((self.irq_counter >> 8) as u8);
        out.push(u8::from(self.irq_pending));
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
        self.switch_bank = data[1];
        self.irq_enabled = data[2] != 0;
        self.irq_counter = u16::from(data[3]) | (u16::from(data[4]) << 8);
        self.irq_pending = data[5] != 0;
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
// Mapper 81 — NTDEC Super Gun (CNROM-like with a wider PRG select).
//
// A single $8000-$FFFF register; the written byte carries:
//   bits 2-3 : 16 KiB PRG bank at $8000 (the $C000 half is fixed to the last).
//   bits 0-1 : 8 KiB CHR bank.
// Mirroring is header-fixed. No IRQ.
// ===========================================================================

/// Mapper 81 (NTDEC Super Gun).
pub struct Ntdec81 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Ntdec81 {
    /// Construct a new mapper 81 board.
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
                "mapper 81 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 81 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
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

impl Mapper for Ntdec81 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % count;
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            0xC000..=0xFFFF => {
                let last = count - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (value >> 2) & 0x03;
            self.chr_bank = value & 0x03;
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
// Mapper 95 — NAMCOT-3425 (Dragon Buster).
//
// An MMC3-subset register port at $8000 (index) / $8001 (data), but with no
// A12 IRQ and no PRG/CHR mode bits. The eight register slots map like MMC3's
// banking-only subset:
//   index 0/1 -> 2 KiB CHR at $0000 / $0800
//   index 2..5 -> 1 KiB CHR at $1000 / $1400 / $1800 / $1C00
//   index 6/7 -> 8 KiB PRG at $8000 / $A000 ($C000/$E000 fixed to last two)
// The board's distinctive feature: bit 5 of the value written to CHR register
// 0 (and 1) drives one-screen nametable selection (A on 0, B on 1) for that
// half of the screen; we model the simpler whole-screen single-screen select
// derived from CHR reg 0 bit 5, which is what the documented Dragon Buster
// decode uses. CHR is ROM.
// ===========================================================================

/// Mapper 112 (NTDEC ASDER / Huang-1).
pub struct NtdecAsder112 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    prg_banks: [u8; 2],
    // chr[0..2] = 2 KiB selects; chr[2..6] = 1 KiB selects.
    chr_regs: [u8; 6],
    horizontal_mirroring: bool,
}

impl NtdecAsder112 {
    /// Construct a new mapper 112 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 112 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 112 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            prg_banks: [0, 1],
            chr_regs: [0; 6],
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let bank1k = match addr {
            0x0000..=0x07FF => (self.chr_regs[0] as usize & !1) + ((addr as usize >> 10) & 1),
            0x0800..=0x0FFF => (self.chr_regs[1] as usize & !1) + ((addr as usize >> 10) & 1),
            0x1000..=0x13FF => self.chr_regs[2] as usize,
            0x1400..=0x17FF => self.chr_regs[3] as usize,
            0x1800..=0x1BFF => self.chr_regs[4] as usize,
            _ => self.chr_regs[5] as usize,
        };
        let bank = bank1k % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for NtdecAsder112 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        match addr {
            0x8000..=0x9FFF => self.read_prg(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.read_prg(self.prg_banks[1] as usize, addr),
            0xC000..=0xDFFF => self.read_prg(last - 1, addr),
            0xE000..=0xFFFF => self.read_prg(last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xE001 {
            0x8000 => self.reg_index = value & 0x07,
            0xA000 => match self.reg_index {
                0 => self.prg_banks[0] = value,
                1 => self.prg_banks[1] = value,
                2 => self.chr_regs[0] = value,
                3 => self.chr_regs[1] = value,
                4 => self.chr_regs[2] = value,
                5 => self.chr_regs[3] = value,
                6 => self.chr_regs[4] = value,
                _ => self.chr_regs[5] = value,
            },
            0xE000 => self.horizontal_mirroring = (value & 0x01) != 0,
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
        let mut out = Vec::with_capacity(11 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_regs);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 11 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_index = data[1];
        self.prg_banks.copy_from_slice(&data[2..4]);
        self.chr_regs.copy_from_slice(&data[4..10]);
        self.horizontal_mirroring = data[10] != 0;
        self.vram.copy_from_slice(&data[11..11 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 137 — Sachen 8259D.
//
// A $4100/$4101 command/data protection-style board (the 8259 family). $4100
// latches a 3-bit command index; $4101 supplies the data for that command:
//   cmd 0..3 : CHR 2 KiB bank selects (slots 0..3 at $0000/$0800/$1000/$1800).
//   cmd 4    : (high CHR bits — modelled as an outer CHR add; we keep it as a
//              stored register that biases all CHR slots).
//   cmd 5    : PRG 32 KiB bank select (low bits).
//   cmd 7    : mirroring / mode (bit 0: 0 = vertical, 1 = horizontal).
// CHR is ROM, four 2 KiB banks. The 8259D variant uses straight 2 KiB CHR
// slots (8259A/B/C reorder the low CHR address lines; that reorder is omitted
// here as it does not affect the register-decode contract).
// ===========================================================================

/// Validate a PRG-ROM image is a non-zero multiple of 8 KiB.
fn check_prg(prg: &[u8], id: u16) -> Result<(), MapperError> {
    if prg.is_empty() || !prg.len().is_multiple_of(PRG_BANK_8K) {
        return Err(MapperError::Invalid(format!(
            "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
            prg.len()
        )));
    }
    Ok(())
}

/// Allocate the CHR slice, falling back to an 8 KiB CHR-RAM bank when the ROM
/// ships no CHR-ROM. Returns `(chr, is_ram)`.
fn chr_or_ram(chr_rom: Box<[u8]>) -> (Box<[u8]>, bool) {
    if chr_rom.is_empty() {
        (vec![0u8; CHR_BANK_8K].into_boxed_slice(), true)
    } else {
        (chr_rom, false)
    }
}

// ===========================================================================
// NtdecTc112 (mapper 193) — NTDEC TC-112 (*Fighting Hero*).
//
// PRG: 8 KiB pages. The last three 8 KiB windows ($A000/$C000/$E000) are fixed
// to the final three banks; $8000 is the one switchable window (register 3).
// CHR: 2 KiB pages. Register 0 selects a paired 2 KiB window into the first two
// slots ($0000 + $0800), register 1 the third ($1000), register 2 the fourth
// ($1800). Registers live at $6000-$7FFF (addr & 3). Ported from Mesen2
// Ntdec/NtdecTc112.h.
// ===========================================================================

/// NTDEC TC-112 (mapper 193).
pub struct NtdecTc112 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// Switchable 8 KiB PRG window for $8000.
    prg0: usize,
    /// 2 KiB CHR windows for $0000/$0800/$1000/$1800.
    chr2: [usize; 4],
}

impl NtdecTc112 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 193)?;
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg0: 0,
            chr2: [0; 4],
        })
    }

    fn prg_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_count_2k(&self) -> usize {
        (self.chr.len() / CHR_BANK_2K).max(1)
    }
}

impl Mapper for NtdecTc112 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // The final three 8 KiB windows are fixed to the last three banks.
            0x8000..=0xFFFF => {
                let count = self.prg_count_8k();
                let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                let bank = match slot {
                    0 => self.prg0 % count,
                    // `saturating_sub` guards a malformed sub-4-bank PRG image
                    // (the subtraction would otherwise underflow + panic).
                    _ => count.saturating_sub(4 - slot) % count, // last-3 fixed window
                };
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            match addr & 0x03 {
                0 => {
                    // Paired 2 KiB CHR select into slots 0 and 1.
                    self.chr2[0] = (value >> 1) as usize;
                    self.chr2[1] = (value >> 1) as usize + 1;
                }
                1 => self.chr2[2] = (value >> 1) as usize,
                2 => self.chr2[3] = (value >> 1) as usize,
                _ => self.prg0 = value as usize,
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                let slot = (addr as usize) / CHR_BANK_2K;
                let count = self.chr_count_2k();
                let bank = self.chr2[slot] % count;
                self.chr[bank * CHR_BANK_2K + (addr as usize & (CHR_BANK_2K - 1))]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let mut out = Vec::with_capacity(1 + 4 + 16 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        for c in &self.chr2 {
            out.extend_from_slice(&(*c as u32).to_le_bytes());
        }
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 4 + 16 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        let mut c = 1;
        self.prg0 = rd(c);
        c += 4;
        for s in &mut self.chr2 {
            *s = rd(c);
            c += 4;
        }
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 193 (NTDEC TC-112, *Fighting Hero*).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m193(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<NtdecTc112, MapperError> {
    NtdecTc112::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// Bmc204 (mapper 204) — discrete NROM/UNROM 2-in-1 BMC multicart.
//
// The written *address* low bits select the layout: `bitMask = addr & 0x06`
// gives the 16 KiB PRG block, and (when bitMask != 0x06) `addr & 1` picks the
// inner half. Both PRG windows ($8000 + $C000) and the 8 KiB CHR window track
// the decoded page; `addr & 0x10` flips the mirroring. Ported from Mesen2
// Unlicensed/Mapper204.h.
// ===========================================================================

/// NTDEC N625092 multicart (mapper 221).
pub struct NtdecN625092 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    mode: u16,
    prg_reg: u8,
    /// 16 KiB PRG windows for $8000 and $C000.
    prg0: usize,
    prg1: usize,
}

impl NtdecN625092 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 221)?;
        if prg_rom.len() < PRG_BANK_16K {
            return Err(MapperError::Invalid(format!(
                "mapper 221 PRG-ROM size {} is smaller than one 16 KiB bank",
                prg_rom.len()
            )));
        }
        let (chr, chr_is_ram) = chr_or_ram(chr_rom);
        let mut m = Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            mode: 0,
            prg_reg: 0,
            prg0: 0,
            prg1: 0,
        };
        m.update_state();
        Ok(m)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn update_state(&mut self) {
        let outer = ((self.mode & 0xFC) >> 2) as usize;
        let reg = self.prg_reg as usize;
        if self.mode & 0x02 != 0 {
            if self.mode & 0x0100 != 0 {
                // NROM-256 sub-case: switchable low + fixed (outer | 7) high.
                self.prg0 = outer | reg;
                self.prg1 = outer | 0x07;
            } else {
                // UNROM 2x16 KiB aligned pair (SelectPrgPage2x): the inner reg
                // (masked to even) selects a 32 KiB-aligned window.
                let b = outer | (reg & 0x06);
                self.prg0 = b;
                self.prg1 = b | 1;
            }
        } else {
            // NROM: both windows mirror the same bank.
            self.prg0 = outer | reg;
            self.prg1 = outer | reg;
        }
        self.mirroring = if self.mode & 0x01 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
    }

    fn prg_byte(&self, slot16: usize, addr: u16) -> u8 {
        let count = self.prg_count_16k();
        let bank = slot16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for NtdecN625092 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.prg_byte(self.prg0, addr),
            0xC000..=0xFFFF => self.prg_byte(self.prg1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        match addr & 0xC000 {
            0x8000 => {
                self.mode = addr;
                self.update_state();
            }
            0xC000 => {
                self.prg_reg = (addr & 0x07) as u8;
                self.update_state();
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize & (CHR_BANK_8K - 1)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let mut out = Vec::with_capacity(1 + 2 + 1 + 8 + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.mode.to_le_bytes());
        out.push(self.prg_reg);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        out.extend_from_slice(&(self.prg1 as u32).to_le_bytes());
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + 2 + 1 + 8 + 1 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        self.mode = u16::from_le_bytes([data[1], data[2]]);
        self.prg_reg = data[3];
        self.prg0 = rd(4);
        self.prg1 = rd(8);
        let mut c = 12;
        self.mirroring = byte_to_mirroring(data[c], self.mirroring);
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 221 (NTDEC N625092 multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m221(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<NtdecN625092, MapperError> {
    NtdecN625092::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// Bmc11160 (mapper 299) — TXC/BMC-11160 multicart.
//
// One value-decoded $8000-$FFFF register: bits 4-6 select a 32 KiB PRG bank,
// the 8 KiB CHR bank is `(bank << 2) | (value & 0x03)`, and bit 7 flips the
// mirroring (set => vertical). Ported from Mesen2 Txc/Bmc11160.h.
// ===========================================================================

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    /// 1 KiB-banked CHR: byte 0 of each 1 KiB bank holds the bank index.
    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
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

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
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
    fn m63_address_decoded_bank_and_mode() {
        let mut m = Ntdec63::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // 32 KiB mode: A & 2 == 0. bank = (A>>2)&0x3F. Choose A=0x8008 ->
        // (0x8008>>2)&0x3F = 0x02 -> bank 2; (A&2)==0 -> 32K; (A&1)==0 -> V.
        m.cpu_write(0x8008, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3); // 32K high half = bank|1
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m63_save_state_round_trip() {
        let mut m = Ntdec63::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8009, 0); // A&1 == 1 -> horizontal
        m.ppu_write(0x0010, 0x12);
        let blob = m.save_state();
        let mut m2 = Ntdec63::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0010), 0x12);
    }

    #[test]
    fn m174_address_decoded_prg_chr_mirror() {
        let mut m = Ntdec174::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // A = 0x8020: prg = (0x20>>4)&7 = 2; (A&0x80)==0 -> 32K; chr =
        // (0x20>>1)&7 = 0; (A&1)==0 -> V.
        m.cpu_write(0x8020, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.ppu_read(0x0000), 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m174_save_state_round_trip() {
        let mut m = Ntdec174::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x80A3, 0); // 16K mode + chr select + horizontal
        let blob = m.save_state();
        let mut m2 = Ntdec174::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.current_mirroring(), m.current_mirroring());
    }

    #[test]
    fn m40_fixed_layout_and_switchable_window() {
        let mut m = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        // Fixed banks.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xE000), 7);
        // Switch $C000 to bank 3.
        m.cpu_write(0xE000, 3);
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn m40_irq_fires_after_enable() {
        let mut m = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        assert!(!m.irq_pending());
        m.cpu_write(0xA000, 0); // enable
        for _ in 0..0x1000 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        m.cpu_write(0x8000, 0); // disable + ack
        assert!(!m.irq_pending());
    }

    #[test]
    fn m40_save_state_round_trip() {
        let mut m = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 2);
        m.cpu_write(0xA000, 0);
        m.notify_cpu_cycle();
        m.ppu_write(0x0005, 0x9A);
        let blob = m.save_state();
        let mut m2 = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xC000), 2);
        assert_eq!(m2.ppu_read(0x0005), 0x9A);
    }

    #[test]
    fn m81_prg_and_chr_select() {
        let mut m = Ntdec81::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // PRG bits 2-3 = 2, CHR bits 0-1 = 1. value = (2<<2)|1 = 0x09.
        m.cpu_write(0x8000, 0x09);
        assert_eq!(m.cpu_read(0x8000), 2);
        // $C000 fixed to last (7).
        assert_eq!(m.cpu_read(0xC000), 7);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m81_save_state_round_trip() {
        let mut m = Ntdec81::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x06);
        let blob = m.save_state();
        let mut m2 = Ntdec81::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }

    #[test]
    fn m112_indexed_prg_chr_and_mirroring() {
        let mut m =
            NtdecAsder112::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // Select reg 0 (PRG $8000) -> bank 3.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0xA000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Select reg 1 (PRG $A000) -> bank 2.
        m.cpu_write(0x8000, 1);
        m.cpu_write(0xA000, 2);
        assert_eq!(m.cpu_read(0xA000), 2);
        // Mirroring register.
        m.cpu_write(0xE000, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Fixed last two.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m112_save_state_round_trip() {
        let mut m =
            NtdecAsder112::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0);
        m.cpu_write(0xA000, 5);
        m.cpu_write(0xE000, 1);
        let blob = m.save_state();
        let mut m2 =
            NtdecAsder112::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }

    fn prg(banks_8k: usize) -> Box<[u8]> {
        // Fill each 8 KiB bank with its index so bank routing is observable.
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for (i, b) in v.chunks_mut(PRG_BANK_8K).enumerate() {
            b.fill(i as u8);
        }
        v.into_boxed_slice()
    }

    fn chr(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * CHR_BANK_8K];
        for (i, b) in v.chunks_mut(CHR_BANK_8K).enumerate() {
            b.fill(i as u8);
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m193_last_three_prg_windows_are_fixed() {
        // 8 banks of 8 KiB. The last three 8 KiB windows must be fixed to the
        // last three banks (5,6,7) regardless of the switchable $8000 select.
        let mut m = new_m193(prg(8), chr(4), Mirroring::Vertical).unwrap();
        // $8000 defaults to bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 5, "$A000 fixed to last-3");
        assert_eq!(m.cpu_read(0xC000), 6, "$C000 fixed to last-3");
        assert_eq!(m.cpu_read(0xE000), 7, "$E000 fixed to last-3");
        // Register 3 selects the switchable $8000 8 KiB window.
        m.cpu_write(0x6003, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Fixed windows are unaffected.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m193_chr_registers_select_2k_windows() {
        let mut m = new_m193(prg(2), chr(4), Mirroring::Vertical).unwrap();
        // reg0: paired 2 KiB select into slots 0+1. value 4 => (4>>1)=2 / 3.
        m.cpu_write(0x6000, 4);
        // chr bank N (2 KiB) of a 4x8KiB image => 16 2-KiB banks; bank 2 lives in
        // 8 KiB CHR bank 0 (banks 0..3) so its byte == 0.
        assert_eq!(m.ppu_read(0x0000), 0); // slot0 -> 2k bank 2 -> 8k bank0
        // reg1 -> slot2 ($1000); reg2 -> slot3 ($1800).
        m.cpu_write(0x6001, 8); // (8>>1)=4 -> 2k bank4 -> 8k bank1
        assert_eq!(m.ppu_read(0x1000), 1);
        m.cpu_write(0x6002, 12); // (12>>1)=6 -> 2k bank6 -> 8k bank1
        assert_eq!(m.ppu_read(0x1800), 1);
    }

    #[test]
    fn m193_chr_ram_when_no_chr_rom() {
        let mut m = new_m193(prg(2), Box::new([]), Mirroring::Vertical).unwrap();
        m.ppu_write(0x0123, 0xAB);
        assert_eq!(m.ppu_read(0x0123), 0xAB);
    }

    #[test]
    fn m221_nrom_mode_mirrors_both_windows() {
        let mut m = new_m221(prg(16), chr(1), Mirroring::Vertical).unwrap();
        // mode = $8000 (mode&2 == 0 => NROM); outer = 0; prg_reg default 0.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0xC003, 0); // inner reg = 3
        // NROM: both windows == outer|reg == 3.
        assert_eq!(m.cpu_read(0x8000), 6, "16k page 3 -> 8k bank 6");
        assert_eq!(m.cpu_read(0xC000), 6);
    }

    #[test]
    fn m221_unrom_nrom256_subcase() {
        let mut m = new_m221(prg(16), chr(1), Mirroring::Vertical).unwrap();
        // mode bits: set bit1 (UNROM) and bit8 (NROM-256 sub-case).
        // addr = 0x8000 | 0x0102 = 0x8102.
        m.cpu_write(0x8102, 0);
        m.cpu_write(0xC002, 0); // inner reg = 2
        // outer = (0x0102 & 0xFC) >> 2 = 0x00. prg0 = 0|2 = 2; prg1 = 0|7 = 7.
        assert_eq!(m.cpu_read(0x8000), 4, "16k page 2 -> 8k bank 4");
        assert_eq!(m.cpu_read(0xC000), 14, "16k page 7 -> 8k bank 14");
    }

    #[test]
    fn m221_mirroring_bit() {
        let mut m = new_m221(prg(4), chr(1), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x8001, 0); // mode&1 set => horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0); // mode&1 clear => vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m193_m221_save_load_round_trip() {
        // m193
        let mut a = new_m193(prg(8), chr(4), Mirroring::Vertical).unwrap();
        a.cpu_write(0x6003, 5);
        a.cpu_write(0x6000, 6);
        let s = a.save_state();
        let mut b = new_m193(prg(8), chr(4), Mirroring::Vertical).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0x8000), b.cpu_read(0x8000));
        assert_eq!(a.ppu_read(0x0000), b.ppu_read(0x0000));

        // m221
        let mut a = new_m221(prg(16), chr(1), Mirroring::Vertical).unwrap();
        a.cpu_write(0x8102, 0);
        a.cpu_write(0xC002, 0);
        let s = a.save_state();
        let mut b = new_m221(prg(16), chr(1), Mirroring::Horizontal).unwrap();
        b.load_state(&s).unwrap();
        assert_eq!(a.cpu_read(0x8000), b.cpu_read(0x8000));
        assert_eq!(a.cpu_read(0xC000), b.cpu_read(0xC000));
    }

    #[test]
    fn m193_m221_bad_prg_size_is_rejected() {
        // 100 bytes is not a multiple of 8 KiB.
        assert!(
            new_m193(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
        assert!(
            new_m221(
                vec![0u8; 100].into_boxed_slice(),
                chr(1),
                Mirroring::Vertical
            )
            .is_err()
        );
    }
}
