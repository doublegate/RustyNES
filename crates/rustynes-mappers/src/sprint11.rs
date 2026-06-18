//! Sprint 11 MMC3-clone / Sachen-8259 / discrete-multicart mappers
//! (v1.6.0 "Studio" Workstream E mapper-breadth continuation, 126 -> 150).
//!
//! A best-effort (Tier-2) batch of unlicensed / pirate / multicart boards
//! ported from the reference emulators (`Mesen2` `Mmc3Variants/`, `Sachen/`,
//! `Codemasters/`, `Ntdec/`, `Unlicensed/`) and the nesdev wiki. Like
//! `sprint5`..`sprint10`, banking math is translated into direct slice
//! indexing and every bank select wraps with `% count`, so a register write
//! can never index out of bounds (no panics on register access — required for
//! the `#![no_std]` chip stack). All boards here are register-decode +
//! save-state unit-tested only and are **never** accuracy-gated (see `tier.rs`
//! `MapperTier::BestEffort` + `docs/adr/0011-mapper-tiering.md`).
//!
//! Two reusable cores back most of the batch:
//!
//! - [`Mmc3CloneMapper`] — a board wrapping a self-contained MMC3-style core
//!   (`Mmc3Clone`: eight bank registers, the `$8000`/`$A000`/`$C000`/`$E000`
//!   register protocol, an A12 falling-edge scanline IRQ counter) plus a
//!   board-specific outer-bank transform applied to the PRG/CHR bank index
//!   before the final slice lookup. The A12 IRQ is the standard
//!   "reload-on-zero, decrement, assert-at-zero-when-enabled" counter (the
//!   Sharp/NEC reload nuance the [`crate::Mmc3`] core models is an accuracy
//!   detail outside the BestEffort remit). Mappers: 44, 49, 52, 115, 134, 189,
//!   205, 238, 245, 348, 366.
//! - [`Sachen8259`] — the Sachen 8259 protection ASIC (`$4100`/`$4101`
//!   command/data port). The existing mapper 137 (`sprint10`) is the 8259**D**
//!   1 KiB-CHR variant; this core covers the 2 KiB-CHR A/B/C variants
//!   (mappers 141 / 138 / 139), which differ only by a CHR shift + per-slot OR
//!   constants. Ported from `Mesen2 Sachen/Sachen8259.h`.
//!
//! The remaining boards are simple register banks; two carry a CPU-cycle IRQ:
//!
//! - **Mapper 42** (FDS-to-cart conversion, *Mario Baby* / *Ai Senshi Nicol*):
//!   `$6000` PRG-RAM-window bank + `$8000` CHR + `$E000` mirroring; an
//!   enable-gated up-counting M2 IRQ that asserts while the counter is in the
//!   `$6000..$8000` window (CPU-cycle hook).
//! - **Mapper 50** (Alibaba / *SMB2J* alternate FDS-to-cart conversion): a
//!   fixed PRG layout with one bit-scrambled switchable `$C000` window and an
//!   enable-gated M2 IRQ that fires once at 4096 cycles (CPU-cycle hook).
//! - **Mapper 46** (Color Dreams "Rumble Station" 15-in-1): a `$6000` outer +
//!   `$8000` inner register pair selecting 32 KiB PRG / 8 KiB CHR.
//! - **Mapper 51** (BMC 11-in-1): a mode/bank pair with two PRG layouts and a
//!   `$6000` PRG-RAM window bank.
//! - **Mapper 57** (BMC "GK 6-in-1"): two registers (`$8000` / `$8800`)
//!   composing CHR + a 16/32 KiB PRG select.
//! - **Mapper 104** (Codemasters "Golden Five" / *Pegasus 5-in-1*): a
//!   `$8000-$9FFF` outer block-select + `$C000-$FFFF` inner 16 KiB PRG select.
//! - **Mapper 120** (Tobidase Daisakusen FDS-conversion protection): a single
//!   `$41FF` register banking the `$6000` PRG window.
//! - **Mapper 290** (NTDEC "Asder" BMC-NTD-03): a single address-decoded write
//!   carrying PRG + CHR + mirroring in the address bits.
//! - **Mapper 301** (BMC-8157): an address-as-data multicart (the written
//!   *address* selects the inner/outer PRG bank + mirroring).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::bool_to_int_with_if
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_1K: usize = 0x0400;
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

// ===========================================================================
// Mmc3Clone — reusable MMC3-style core for the clone boards.
//
// The MMC3 register protocol (NES 2.0 mapper 4):
//   $8000 even : bank-select (low 3 bits = R index, bit 6 = PRG mode,
//                bit 7 = CHR mode).
//   $8001 odd  : bank-data (the value loaded into the selected R register).
//   $A000 even : mirroring (bit 0: 0 = vertical, 1 = horizontal).
//   $C000 even : IRQ latch (reload value).
//   $C001 odd  : IRQ reload (force a reload on the next A12 rise).
//   $E000 even : IRQ disable + acknowledge.
//   $E001 odd  : IRQ enable.
//
// The A12 IRQ counter clocks on every PPU A12 rising edge: if the counter is 0
// or a reload is pending, it reloads from the latch; otherwise it decrements.
// After the update, if the counter is 0 and IRQs are enabled, the IRQ asserts.
// ===========================================================================

/// A reusable MMC3-style banking + A12-IRQ core for the clone boards.
struct Mmc3Clone {
    regs: [u8; 8],
    bank_select: u8,
    prg_mode: bool,
    chr_mode: bool,
    mirroring: Mirroring,
    irq_counter: u8,
    irq_latch: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    last_a12: bool,
    prg_count_8k: usize,
    chr_count_1k: usize,
}

impl Mmc3Clone {
    const SAVE_LEN: usize = 8 + 10;

    fn new(prg_count_8k: usize, chr_count_1k: usize, mirroring: Mirroring) -> Self {
        Self {
            regs: [0; 8],
            bank_select: 0,
            prg_mode: false,
            chr_mode: false,
            mirroring,
            irq_counter: 0,
            irq_latch: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            last_a12: false,
            prg_count_8k: prg_count_8k.max(1),
            chr_count_1k: chr_count_1k.max(1),
        }
    }

    /// Handle a write to the `$8000-$FFFF` MMC3 register space.
    fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0xE001 {
            0x8000 => {
                self.bank_select = value & 0x07;
                self.prg_mode = value & 0x40 != 0;
                self.chr_mode = value & 0x80 != 0;
            }
            0x8001 => {
                let idx = (self.bank_select & 0x07) as usize;
                self.regs[idx] = value;
            }
            0xA000 => {
                self.mirroring = if value & 0x01 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0xC000 => self.irq_latch = value,
            0xC001 => {
                self.irq_counter = 0;
                self.irq_reload = true;
            }
            0xE000 => {
                self.irq_enabled = false;
                self.irq_pending = false;
            }
            0xE001 => self.irq_enabled = true,
            _ => {}
        }
    }

    /// The base 8 KiB PRG bank for CPU slot 0..=3 ($8000/$A000/$C000/$E000),
    /// before any wrapper outer-bank transform. Mirrors the MMC3 PRG layout.
    fn prg_bank(&self, slot: usize) -> usize {
        let last = self.prg_count_8k - 1;
        let second_last = last.saturating_sub(1);
        let r6 = self.regs[6] as usize;
        let r7 = self.regs[7] as usize;
        match (slot, self.prg_mode) {
            (0, false) => r6,
            (0, true) => second_last,
            (1, _) => r7,
            (2, false) => second_last,
            (2, true) => r6,
            (3, _) => last,
            _ => 0,
        }
    }

    /// The base 1 KiB CHR bank for PPU 1 KiB slot 0..=7, before any wrapper
    /// outer-bank transform. Mirrors the MMC3 CHR layout (2 KiB R0/R1 +
    /// 1 KiB R2-R5, swapped by `chr_mode`).
    fn chr_bank(&self, slot: usize) -> usize {
        let banks: [usize; 8] = if self.chr_mode {
            [
                self.regs[2] as usize,
                self.regs[3] as usize,
                self.regs[4] as usize,
                self.regs[5] as usize,
                self.regs[0] as usize & !1,
                (self.regs[0] as usize & !1) | 1,
                self.regs[1] as usize & !1,
                (self.regs[1] as usize & !1) | 1,
            ]
        } else {
            [
                self.regs[0] as usize & !1,
                (self.regs[0] as usize & !1) | 1,
                self.regs[1] as usize & !1,
                (self.regs[1] as usize & !1) | 1,
                self.regs[2] as usize,
                self.regs[3] as usize,
                self.regs[4] as usize,
                self.regs[5] as usize,
            ]
        };
        banks[slot & 0x07]
    }

    /// Clock the A12 IRQ counter on a PPU A12 transition.
    fn notify_a12(&mut self, level: bool) {
        let rising = level && !self.last_a12;
        self.last_a12 = level;
        if !rising {
            return;
        }
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn save(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.regs);
        out.push(self.bank_select);
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.chr_mode));
        out.push(mirroring_to_byte(self.mirroring));
        out.push(self.irq_counter);
        out.push(self.irq_latch);
        out.push(u8::from(self.irq_reload));
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(u8::from(self.last_a12));
    }

    fn load(&mut self, data: &[u8]) {
        self.regs.copy_from_slice(&data[0..8]);
        self.bank_select = data[8];
        self.prg_mode = data[9] != 0;
        self.chr_mode = data[10] != 0;
        self.mirroring = byte_to_mirroring(data[11], self.mirroring);
        self.irq_counter = data[12];
        self.irq_latch = data[13];
        self.irq_reload = data[14] != 0;
        self.irq_enabled = data[15] != 0;
        self.irq_pending = data[16] != 0;
        self.last_a12 = data[17] != 0;
    }
}

/// Which clone board's outer-bank transform [`Mmc3CloneMapper`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloneBoard {
    /// Mapper 44 — 7-block selector via `$A001`.
    M44,
    /// Mapper 49 — `$6000` outer block-select with a simplified-PRG mode bit.
    M49,
    /// Mapper 52 — `$6000` outer-block / PRG+CHR-size selector.
    M52,
    /// Mapper 115 — `$5000`/`$4100` PRG-override + CHR outer-256K register.
    M115,
    /// Mapper 134 — `$6001` PRG (bit 1) + CHR (bit 5) 256 KiB outer bank.
    M134,
    /// Mapper 189 — `$4120-$7FFF` 32 KiB PRG select (overrides MMC3 PRG).
    M189,
    /// Mapper 205 — `$6000` 2-bit block-select (PRG/CHR outer window).
    M205,
    /// Mapper 238 — `$4020-$7FFF` security register (read-back LUT).
    M238,
    /// Mapper 245 — `$8001` R0 bit 1 -> PRG 256 KiB outer; CHR-RAM 4K/4K swap.
    M245,
    /// Mapper 348 — `$6800` outer-bank register (BMC-830118C).
    M348,
    /// Mapper 366 — `$6000-$7FFF` block-select (BMC-GN-45).
    M366,
}

// ===========================================================================
// Mmc3CloneMapper — wraps `Mmc3Clone` + a `CloneBoard` outer transform.
// ===========================================================================

/// An MMC3-clone board: the shared MMC3-style core plus a board-specific
/// outer-bank register and PRG/CHR transform.
pub struct Mmc3CloneMapper {
    board: CloneBoard,
    core: Mmc3Clone,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    /// Board-specific outer register (semantics per `CloneBoard`).
    outer: u8,
    /// A second board register where needed (115 CHR-hi / protection read).
    outer2: u8,
}

impl Mmc3CloneMapper {
    fn new(
        board: CloneBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        mapper_id: u16,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper {mapper_id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 0x8000].into_boxed_slice() // 32 KiB CHR-RAM (245 needs >8K).
        } else {
            if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
                return Err(MapperError::Invalid(format!(
                    "mapper {mapper_id} CHR-ROM size {} is not a multiple of 1 KiB",
                    chr_rom.len()
                )));
            }
            chr_rom
        };
        let prg_count_8k = prg_rom.len() / PRG_BANK_8K;
        let chr_count_1k = (chr.len() / CHR_BANK_1K).max(1);
        Ok(Self {
            board,
            core: Mmc3Clone::new(prg_count_8k, chr_count_1k, mirroring),
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            outer: 0,
            outer2: 0,
        })
    }

    /// Resolve the final 8 KiB PRG bank for a CPU slot after the board outer
    /// transform.
    fn resolve_prg(&self, slot: usize) -> usize {
        let base = self.core.prg_bank(slot);
        let count = self.core.prg_count_8k;
        let bank = match self.board {
            CloneBoard::M44 => {
                let block = (self.outer & 0x07).min(6) as usize;
                let mask = if block <= 5 { 0x0F } else { 0x1F };
                (base & mask) | (block * 0x10)
            }
            CloneBoard::M49 => {
                let block = ((self.outer >> 6) & 0x03) as usize;
                if self.outer & 0x01 != 0 {
                    (base & 0x0F) | (block * 0x10)
                } else {
                    ((self.outer >> 4) & 0x03) as usize * 4 + slot
                }
            }
            CloneBoard::M52 => {
                if self.outer & 0x08 != 0 {
                    (base & 0x0F) | ((self.outer as usize & 0x07) << 4)
                } else {
                    (base & 0x1F) | ((self.outer as usize & 0x06) << 4)
                }
            }
            CloneBoard::M115 => {
                if self.outer & 0x80 != 0 {
                    if self.outer & 0x20 != 0 {
                        ((self.outer as usize & 0x0F) >> 1) * 4 + slot
                    } else {
                        let b16 = (self.outer as usize & 0x0F) * 2;
                        b16 + (slot & 0x01)
                    }
                } else {
                    base
                }
            }
            CloneBoard::M134 => (base & 0x1F) | ((self.outer as usize & 0x02) << 4),
            CloneBoard::M189 => {
                let page = ((self.outer as usize) | (self.outer as usize >> 4)) & 0x07;
                page * 4 + slot
            }
            CloneBoard::M205 => {
                let block = self.outer as usize & 0x03;
                let mask = if block <= 1 { 0x1F } else { 0x0F };
                (base & mask) | (block * 0x10)
            }
            CloneBoard::M238 => base,
            CloneBoard::M245 => {
                let or = if self.core.regs[0] & 0x02 != 0 {
                    0x40
                } else {
                    0
                };
                (base & 0x3F) | or
            }
            CloneBoard::M348 => (base & 0x0F) | ((self.outer as usize & 0x0C) << 2),
            CloneBoard::M366 => (base & 0x0F) | (self.outer as usize & 0x30),
        };
        bank % count
    }

    /// Resolve the final 1 KiB CHR bank for a PPU slot after the board outer
    /// transform.
    fn resolve_chr(&self, slot: usize) -> usize {
        let base = self.core.chr_bank(slot);
        let count = self.core.chr_count_1k;
        let bank = match self.board {
            CloneBoard::M44 => {
                let block = (self.outer & 0x07).min(6) as usize;
                let mask = if block <= 5 { 0x7F } else { 0xFF };
                (base & mask) | (block * 0x80)
            }
            CloneBoard::M49 => {
                let block = ((self.outer >> 6) & 0x03) as usize;
                (base & 0x7F) | (block * 0x80)
            }
            CloneBoard::M52 => {
                if self.outer & 0x40 != 0 {
                    (base & 0x7F)
                        | (((self.outer as usize & 0x04) | ((self.outer as usize >> 4) & 0x03))
                            << 7)
                } else {
                    (base & 0xFF)
                        | (((self.outer as usize & 0x04) | ((self.outer as usize >> 4) & 0x02))
                            << 7)
                }
            }
            CloneBoard::M115 => base | ((self.outer2 as usize & 0x01) << 8),
            CloneBoard::M134 => (base & 0xFF) | ((self.outer as usize & 0x20) << 3),
            CloneBoard::M189 => base,
            CloneBoard::M205 => {
                let block = self.outer as usize & 0x03;
                if block >= 2 {
                    (base & 0x7F) | 0x100
                } else {
                    base | if block == 1 { 0x80 } else { 0 }
                }
            }
            CloneBoard::M238 => base,
            CloneBoard::M245 => base, // CHR-RAM; handled in ppu_read.
            CloneBoard::M348 => (base & 0x7F) | ((self.outer as usize & 0x0C) << 5),
            CloneBoard::M366 => (base & 0x7F) | ((self.outer as usize & 0x30) << 3),
        };
        bank % count
    }
}

impl Mapper for Mmc3CloneMapper {
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: false,
            audio: false,
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.resolve_prg(0);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0xA000..=0xBFFF => {
                let bank = self.resolve_prg(1);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0xC000..=0xDFFF => {
                let bank = self.resolve_prg(2);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0xE000..=0xFFFF => {
                let bank = self.resolve_prg(3);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            // 115 protection read-back at $5000-$5FFF.
            0x5000..=0x5FFF if matches!(self.board, CloneBoard::M115) => self.outer2,
            // 238 security read-back at $4020-$7FFF.
            0x4020..=0x7FFF if matches!(self.board, CloneBoard::M238) => self.outer2,
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match self.board {
            CloneBoard::M115 => (0x4020..=0x4FFF).contains(&addr),
            CloneBoard::M238 => false, // $4020-$7FFF is all mapped (security reg).
            _ => (0x4020..=0x5FFF).contains(&addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            CloneBoard::M115 => match addr {
                0x5080 => self.outer2 = value,
                0x4100..=0x7FFF => {
                    if addr & 0x01 == 0 {
                        self.outer = value; // PRG override reg.
                    } else {
                        self.outer2 = value; // CHR-hi reg (bit 0 used).
                    }
                }
                0x8000..=0xFFFF => self.core.write_register(addr, value),
                _ => {}
            },
            CloneBoard::M134 => {
                if addr == 0x6001 {
                    self.outer = value;
                } else if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M189 => {
                if (0x4120..=0x7FFF).contains(&addr) {
                    self.outer = value;
                } else if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M238 => {
                if (0x4020..=0x7FFF).contains(&addr) {
                    const LUT: [u8; 4] = [0x00, 0x02, 0x02, 0x03];
                    self.outer2 = LUT[(value & 0x03) as usize];
                } else if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M44 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    if addr & 0xE001 == 0xA001 {
                        self.outer = value & 0x07;
                    }
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M348 => {
                if (0x6800..=0x68FF).contains(&addr) {
                    self.outer = value;
                } else if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M366 => {
                if (0x6000..=0x7FFF).contains(&addr) {
                    self.outer = if addr < 0x7000 {
                        (addr as u8) & 0x30
                    } else {
                        value & 0x30
                    };
                } else if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M49 | CloneBoard::M52 | CloneBoard::M205 => {
                if (0x6000..=0x7FFF).contains(&addr) {
                    self.outer = value;
                } else if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
            CloneBoard::M245 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.core.write_register(addr, value);
                }
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    if matches!(self.board, CloneBoard::M245) {
                        let half = if self.core.chr_mode { 0x1000 } else { 0 };
                        let off = (half ^ (addr as usize & 0x1FFF)) & (self.chr.len() - 1);
                        return self.chr[off];
                    }
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                let slot = (addr as usize) / CHR_BANK_1K;
                let bank = self.resolve_chr(slot);
                self.chr[bank * CHR_BANK_1K + (addr as usize & 0x3FF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.core.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                if matches!(self.board, CloneBoard::M245) {
                    let half = if self.core.chr_mode { 0x1000 } else { 0 };
                    let off = (half ^ (addr as usize & 0x1FFF)) & (self.chr.len() - 1);
                    self.chr[off] = value;
                } else {
                    let off = addr as usize & (self.chr.len() - 1);
                    self.chr[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.core.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_a12(&mut self, level: bool) {
        self.core.notify_a12(level);
    }

    fn irq_pending(&self) -> bool {
        self.core.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.core.irq_pending = false;
    }

    fn current_mirroring(&self) -> Mirroring {
        self.core.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(3 + Mmc3Clone::SAVE_LEN + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.push(self.outer);
        out.push(self.outer2);
        self.core.save(&mut out);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + Mmc3Clone::SAVE_LEN + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.outer = data[1];
        self.outer2 = data[2];
        let mut cursor = 3;
        self.core.load(&data[cursor..cursor + Mmc3Clone::SAVE_LEN]);
        cursor += Mmc3Clone::SAVE_LEN;
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

macro_rules! clone_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<Mmc3CloneMapper, MapperError> {
            Mmc3CloneMapper::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

clone_ctor!(
    new_m44,
    CloneBoard::M44,
    44,
    "Mapper 44 (BMC SuperBig 7-in-1 MMC3 multicart)."
);
clone_ctor!(
    new_m49,
    CloneBoard::M49,
    49,
    "Mapper 49 (BMC 4-in-1 MMC3 multicart)."
);
clone_ctor!(
    new_m52,
    CloneBoard::M52,
    52,
    "Mapper 52 (BMC Mario 7-in-1 MMC3 multicart)."
);
clone_ctor!(
    new_m115,
    CloneBoard::M115,
    115,
    "Mapper 115 (Kasheng SFC-02B/-03/-004 MMC3 clone)."
);
clone_ctor!(
    new_m134,
    CloneBoard::M134,
    134,
    "Mapper 134 (T4A54A / WX-KB4K MMC3-clone multicart)."
);
clone_ctor!(
    new_m189,
    CloneBoard::M189,
    189,
    "Mapper 189 (TXC 32 KiB-PRG MMC3 clone)."
);
clone_ctor!(
    new_m205,
    CloneBoard::M205,
    205,
    "Mapper 205 (BMC 3-in-1 / 15-in-1 MMC3 multicart)."
);
clone_ctor!(
    new_m238,
    CloneBoard::M238,
    238,
    "Mapper 238 (MMC3 clone + $4020-$7FFF security LUT)."
);
clone_ctor!(
    new_m245,
    CloneBoard::M245,
    245,
    "Mapper 245 (Waixing MMC3 clone, CHR-RAM PRG-256K outer)."
);
clone_ctor!(
    new_m348,
    CloneBoard::M348,
    348,
    "Mapper 348 (BMC-830118C MMC3 multicart)."
);
clone_ctor!(
    new_m366,
    CloneBoard::M366,
    366,
    "Mapper 366 (BMC-GN-45 MMC3 multicart)."
);

// ===========================================================================
// Sachen 8259 (A/B/C) — the 2 KiB-CHR variants of the protection ASIC.
//
// $4100 (addr & 0xC101 == 0x4100) : command — selects internal reg 0..=7.
// $4101 (addr & 0xC101 == 0x4101) : data    — writes the selected reg (& 0x07).
// 32 KiB fixed PRG ($8000), four 2 KiB CHR banks. The variants differ only by a
// CHR left-shift and three per-slot OR constants:
//   8259A: shift 1, chrOr [1,0,1]   (mapper 141)
//   8259B: shift 0, chrOr [0,0,0]   (mapper 138)
//   8259C: shift 2, chrOr [1,2,3]   (mapper 139)
// reg7 bits 1-2 select mirroring (reg7 bit 0 = "simple mode" override).
// reg5 selects the 32 KiB PRG bank; reg4 supplies the CHR high bits.
// Ported from Mesen2 Sachen/Sachen8259.h.
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

/// Mapper 42 (FDS-to-cart conversion: *Mario Baby* / *Ai Senshi Nicol*).
pub struct Mapper42 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    prg_ram_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Mapper42 {
    /// Construct a mapper 42 board.
    ///
    /// # Errors
    /// [`MapperError::Invalid`] on a bad PRG size.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 42 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram_bank: 0,
            chr_bank: 0,
            mirroring,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        })
    }

    fn prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Mapper42 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        match addr {
            0x6000..=0x7FFF => self.prg_8k(self.prg_ram_bank as usize, addr),
            0x8000..=0x9FFF => self.prg_8k(count.saturating_sub(4), addr),
            0xA000..=0xBFFF => self.prg_8k(count.saturating_sub(3), addr),
            0xC000..=0xDFFF => self.prg_8k(count.saturating_sub(2), addr),
            0xE000..=0xFFFF => self.prg_8k(count.saturating_sub(1), addr),
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xE003 {
            0x8000 => self.chr_bank = value & 0x0F,
            0xE000 => self.prg_ram_bank = value & 0x0F,
            0xE001 => {
                self.mirroring = if value & 0x08 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
            0xE002 => {
                self.irq_enabled = value & 0x02 != 0;
                if !self.irq_enabled {
                    self.irq_pending = false;
                    self.irq_counter = 0;
                }
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
                let count = (self.chr.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
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

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        self.irq_counter += 1;
        if self.irq_counter >= 0x8000 {
            self.irq_counter -= 0x8000;
        }
        self.irq_pending = self.irq_counter >= 0x6000;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(8 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_ram_bank);
        out.push(self.chr_bank);
        out.push(mirroring_to_byte(self.mirroring));
        out.push((self.irq_counter & 0xFF) as u8);
        out.push((self.irq_counter >> 8) as u8);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 8 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_ram_bank = data[1];
        self.chr_bank = data[2];
        self.mirroring = byte_to_mirroring(data[3], self.mirroring);
        self.irq_counter = u16::from(data[4]) | (u16::from(data[5]) << 8);
        self.irq_enabled = data[6] != 0;
        self.irq_pending = data[7] != 0;
        let mut cursor = 8;
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
// Mapper 50 — Alibaba / SMB2J alternate FDS-to-cartridge conversion.
//
// Fixed PRG layout (8 KiB banks): $6000 -> bank 15, $8000 -> bank 8,
// $A000 -> bank 9, $C000 -> switchable, $E000 -> bank 11. The $C000 bank is
// written via $4020 (addr & 0x4120 == 0x4020) with a bit-scrambled value:
//   bank = (v & 0x08) | ((v & 0x01) << 2) | ((v & 0x06) >> 1).
// $4120 (addr & 0x4120 == 0x4120): IRQ enable (bit 0). When enabled, an M2
// counter counts up and asserts once at 4096 cycles, then disables. Disabling
// clears + acknowledges. 8 KiB CHR-RAM.
// ===========================================================================

/// Mapper 50 (Alibaba / *SMB2J* alternate FDS-to-cart conversion).
pub struct Mapper50 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    switch_bank: u8,
    irq_enabled: bool,
    irq_counter: u16,
    irq_pending: bool,
    mirroring: Mirroring,
}

impl Mapper50 {
    /// Construct a mapper 50 board.
    ///
    /// # Errors
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 50 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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

    fn prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Mapper50 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_8k(15, addr),
            0x8000..=0x9FFF => self.prg_8k(8, addr),
            0xA000..=0xBFFF => self.prg_8k(9, addr),
            0xC000..=0xDFFF => self.prg_8k(self.switch_bank as usize, addr),
            0xE000..=0xFFFF => self.prg_8k(11, addr),
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0x4120 {
            0x4020 => {
                self.switch_bank = (value & 0x08) | ((value & 0x01) << 2) | ((value & 0x06) >> 1);
            }
            0x4120 => {
                // Both enable and disable (re)start the counter from 0 and
                // clear any pending line; only the enable flag itself differs.
                // On IRQ-enable this means a fresh enable after a prior fire
                // counts a full period rather than tripping on a stale counter
                // / latched IRQ.
                self.irq_enabled = value & 0x01 != 0;
                self.irq_pending = false;
                self.irq_counter = 0;
            }
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
        self.irq_counter += 1;
        if self.irq_counter == 0x1000 {
            self.irq_pending = true;
            self.irq_enabled = false;
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
// DiscreteMapper — small hook-free single/dual-register multicart boards
// (46/51/57/104/120/290/301). 32/16 KiB PRG window + 8 KiB CHR-ROM/RAM.
// ===========================================================================

/// Which discrete board the [`DiscreteMapper`] models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscreteBoard {
    /// Mapper 46 (Color Dreams "Rumble Station" 15-in-1).
    M46,
    /// Mapper 51 (BMC 11-in-1).
    M51,
    /// Mapper 57 (BMC GK 6-in-1).
    M57,
    /// Mapper 104 (Codemasters Golden Five / Pegasus 5-in-1).
    M104,
    /// Mapper 120 (Tobidase Daisakusen FDS-conversion protection).
    M120,
    /// Mapper 290 (NTDEC Asder BMC-NTD-03).
    M290,
    /// Mapper 301 (BMC-8157 address-as-data multicart).
    M301,
}

/// A discrete unlicensed/multicart board with a simple register surface.
pub struct DiscreteMapper {
    board: DiscreteBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    reg0: u8,
    reg1: u8,
    /// For 301 (address-as-data) the last-written address.
    last_addr: u16,
    mirroring: Mirroring,
}

impl DiscreteMapper {
    fn new(
        board: DiscreteBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        id: u16,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            board,
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg0: 0,
            reg1: 0,
            last_addr: 0,
            mirroring,
        })
    }

    fn prg_16k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }

    fn prg_32k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
    }

    fn prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn chr_8k_bank(&self) -> usize {
        match self.board {
            DiscreteBoard::M46 => {
                ((self.reg0 as usize & 0xF0) >> 1) | ((self.reg1 as usize & 0x70) >> 4)
            }
            DiscreteBoard::M57 => {
                ((self.reg0 as usize & 0x40) >> 3) | ((self.reg0 | self.reg1) as usize & 0x07)
            }
            _ => 0,
        }
    }
}

impl Mapper for DiscreteMapper {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match self.board {
            DiscreteBoard::M46 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    let bank = ((self.reg0 as usize & 0x0F) << 1) | (self.reg1 as usize & 0x01);
                    self.prg_32k(bank, addr)
                } else {
                    0
                }
            }
            DiscreteBoard::M51 => {
                let bank4 = (self.reg0 as usize) << 2;
                match addr {
                    0x6000..=0x7FFF => {
                        let b = if self.reg1 & 0x01 != 0 {
                            0x23 | bank4
                        } else {
                            0x2F | bank4
                        };
                        self.prg_8k(b, addr)
                    }
                    0x8000..=0xBFFF if self.reg1 & 0x01 != 0 => self.prg_16k(bank4 >> 1, addr),
                    0xC000..=0xFFFF if self.reg1 & 0x01 != 0 => {
                        self.prg_16k((bank4 >> 1) | 1, addr)
                    }
                    0x8000..=0xBFFF => {
                        self.prg_16k((bank4 >> 1) | (self.reg1 as usize >> 1 & 0x01), addr)
                    }
                    0xC000..=0xFFFF => self.prg_16k((bank4 >> 1) | 0x07, addr),
                    _ => 0,
                }
            }
            DiscreteBoard::M57 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    if self.reg1 & 0x10 != 0 {
                        let b = ((self.reg1 as usize >> 5) & 0x06) >> 1;
                        self.prg_32k(b, addr)
                    } else {
                        let b16 = (self.reg1 as usize >> 5) & 0x07;
                        self.prg_16k(b16, addr)
                    }
                } else {
                    0
                }
            }
            DiscreteBoard::M104 => match addr {
                0x8000..=0xBFFF => self.prg_16k(self.reg0 as usize, addr),
                0xC000..=0xFFFF => {
                    let high = (self.reg1 as usize & 0x70) | 0x0F;
                    self.prg_16k(high, addr)
                }
                _ => 0,
            },
            DiscreteBoard::M120 => match addr {
                0x6000..=0x7FFF => self.prg_8k(self.reg0 as usize, addr),
                0x8000..=0xFFFF => {
                    let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
                    let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                    let base = count.saturating_sub(4);
                    self.prg_8k(base + slot, addr)
                }
                _ => 0,
            },
            DiscreteBoard::M290 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.prg_16k(self.reg0 as usize, addr)
                } else {
                    0
                }
            }
            DiscreteBoard::M301 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    let a = self.last_addr;
                    let inner = (a >> 2) & 0x07;
                    let outer128 = (a >> 5) & 0x03;
                    // A7 is the 256 KiB outer-bank select; without it any PRG
                    // image > 256 KiB can only reach its low half. Slot it
                    // between the 128 KiB (A5-A6) and 512 KiB (A8) selects.
                    let outer256 = (a >> 7) & 0x01;
                    let outer512 = (a >> 8) & 0x01;
                    let bank16 = (outer512 << 6) | (outer256 << 5) | (outer128 << 3) | inner;
                    self.prg_16k(bank16 as usize, addr)
                } else {
                    0
                }
            }
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match self.board {
            DiscreteBoard::M51 | DiscreteBoard::M120 => (0x4020..=0x5FFF).contains(&addr),
            _ => (0x4020..=0x7FFF).contains(&addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            DiscreteBoard::M46 => {
                if addr < 0x8000 {
                    self.reg0 = value;
                } else {
                    self.reg1 = value;
                }
            }
            DiscreteBoard::M51 => match addr {
                0x6000..=0x7FFF => self.reg1 = ((value >> 3) & 0x02) | ((value >> 1) & 0x01),
                0xC000..=0xDFFF => {
                    self.reg0 = value & 0x0F;
                    self.reg1 = ((value >> 3) & 0x02) | (self.reg1 & 0x01);
                }
                0x8000..=0xFFFF => self.reg0 = value & 0x0F,
                _ => {}
            },
            DiscreteBoard::M57 => match addr & 0x8800 {
                0x8000 => self.reg0 = value,
                0x8800 => self.reg1 = value,
                _ => {}
            },
            DiscreteBoard::M104 => {
                if addr >= 0xC000 {
                    self.reg0 = (self.reg0 & 0xF0) | (value & 0x0F);
                } else if (0x8000..=0x9FFF).contains(&addr) && value & 0x08 != 0 {
                    self.reg0 = (self.reg0 & 0x0F) | ((value << 4) & 0x70);
                    self.reg1 = value;
                }
            }
            DiscreteBoard::M120 => {
                if addr == 0x41FF {
                    self.reg0 = value;
                }
            }
            DiscreteBoard::M290 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    let prg = ((addr >> 10) & 0x1E) as u8;
                    let chr = (((addr & 0x0300) >> 5) | (addr & 0x07)) as u8;
                    self.reg0 = if addr & 0x80 != 0 {
                        prg | ((addr >> 6) & 1) as u8
                    } else {
                        prg & 0xFE
                    };
                    self.reg1 = chr;
                    self.mirroring = if addr & 0x400 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            DiscreteBoard::M301 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.last_addr = addr;
                    self.mirroring = if addr & 0x02 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                let count = (self.chr.len() / CHR_BANK_8K).max(1);
                let bank = self.chr_8k_bank() % count;
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
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
        let mut out = Vec::with_capacity(6 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg0);
        out.push(self.reg1);
        out.push((self.last_addr & 0xFF) as u8);
        out.push((self.last_addr >> 8) as u8);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 6 + self.vram.len() + chr_ram;
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
        self.last_addr = u16::from(data[3]) | (u16::from(data[4]) << 8);
        self.mirroring = byte_to_mirroring(data[5], self.mirroring);
        let mut cursor = 6;
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

macro_rules! discrete_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<DiscreteMapper, MapperError> {
            DiscreteMapper::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

discrete_ctor!(
    new_m46,
    DiscreteBoard::M46,
    46,
    "Mapper 46 (Color Dreams Rumble Station 15-in-1)."
);
discrete_ctor!(
    new_m51,
    DiscreteBoard::M51,
    51,
    "Mapper 51 (BMC 11-in-1 multicart)."
);
discrete_ctor!(
    new_m57,
    DiscreteBoard::M57,
    57,
    "Mapper 57 (BMC GK 6-in-1 multicart)."
);
discrete_ctor!(
    new_m104,
    DiscreteBoard::M104,
    104,
    "Mapper 104 (Codemasters Golden Five / Pegasus 5-in-1)."
);
discrete_ctor!(
    new_m120,
    DiscreteBoard::M120,
    120,
    "Mapper 120 (Tobidase Daisakusen FDS-conversion protection)."
);
discrete_ctor!(
    new_m290,
    DiscreteBoard::M290,
    290,
    "Mapper 290 (NTDEC Asder BMC-NTD-03)."
);
discrete_ctor!(
    new_m301,
    DiscreteBoard::M301,
    301,
    "Mapper 301 (BMC-8157 address-as-data multicart)."
);

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
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

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
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

    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_2K];
        for b in 0..banks {
            v[b * CHR_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- MMC3-clone core: standard MMC3 PRG/CHR layout + A12 IRQ ------------

    #[test]
    fn mmc3_clone_prg_layout_and_a12_irq() {
        let mut m = new_m245(synth_prg_8k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x06); // bank-select R6
        m.cpu_write(0x8001, 5);
        m.cpu_write(0x8000, 0x07); // bank-select R7
        m.cpu_write(0x8001, 6);
        assert_eq!(m.cpu_read(0x8000), 5); // R6 @ $8000
        assert_eq!(m.cpu_read(0xA000), 6); // R7 @ $A000
        assert_eq!(m.cpu_read(0xE000), 15); // last @ $E000

        m.cpu_write(0xC000, 2); // latch
        m.cpu_write(0xC001, 0); // reload
        m.cpu_write(0xE001, 0); // enable
        assert!(!m.irq_pending());
        for _ in 0..3 {
            m.notify_a12(false);
            m.notify_a12(true);
        }
        assert!(m.irq_pending());
        m.cpu_write(0xE000, 0); // disable + ack
        assert!(!m.irq_pending());
    }

    #[test]
    fn m245_prg_outer_bank_from_reg0_bit1() {
        let mut m = new_m245(synth_prg_8k(128), Box::new([]), Mirroring::Vertical).unwrap();
        // m245: the PRG-A18 outer bit is R0 bit 1 (select reg 0, write value).
        m.cpu_write(0x8000, 0x00); // select R0
        m.cpu_write(0x8001, 0x02); // R0 bit 1 set -> PRG OR 0x40
        m.cpu_write(0x8000, 0x06); // select R6
        m.cpu_write(0x8001, 5); // R6 = 5
        // R6 base 5 -> (5 & 0x3F) | 0x40 = 69.
        assert_eq!(m.cpu_read(0x8000), 69);
    }

    #[test]
    fn m115_chr_outer_and_protection_read() {
        let mut m = new_m115(synth_prg_8k(32), synth_chr_1k(512), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4101, 0x01); // CHR-hi reg bit 0 -> +0x100.
        assert_eq!(m.ppu_read(0x0000), 0); // bank 256 % 512 -> stored index 0.
        m.cpu_write(0x5080, 0xAB);
        assert_eq!(m.cpu_read(0x5000), 0xAB);
    }

    #[test]
    fn m189_prg_32k_select_overrides_mmc3() {
        let mut m = new_m189(synth_prg_8k(32), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4120, 0x33); // (3|3) = 3 -> page 3 -> bank 12.
        assert_eq!(m.cpu_read(0x8000), 12);
        assert_eq!(m.cpu_read(0xA000), 13);
    }

    #[test]
    fn mmc3_clone_save_state_round_trip() {
        let mut m = new_m115(synth_prg_8k(32), synth_chr_1k(256), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 4);
        m.cpu_write(0x4101, 0x01);
        m.cpu_write(0xC000, 7);
        m.ppu_write(0x2005, 0x5A);
        let blob = m.save_state();
        let mut m2 = new_m115(synth_prg_8k(32), synth_chr_1k(256), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x2005), 0x5A);
    }

    #[test]
    fn m245_chr_ram_round_trip() {
        let mut m = new_m245(synth_prg_8k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m.ppu_write(0x0010, 0x42);
        let blob = m.save_state();
        let mut m2 = new_m245(synth_prg_8k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0010), 0x42);
    }

    // --- Sachen 8259 A/B/C --------------------------------------------------

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

    // --- Mapper 42 ----------------------------------------------------------

    #[test]
    fn m42_fixed_tail_and_switchable_window() {
        let mut m = Mapper42::new(synth_prg_8k(8), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
        m.cpu_write(0xE000, 3);
        assert_eq!(m.cpu_read(0x6000), 3);
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m42_irq_window() {
        let mut m = Mapper42::new(synth_prg_8k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE002, 0x02); // enable
        let mut fired = false;
        for _ in 0..0x8000 {
            m.notify_cpu_cycle();
            if m.irq_pending() {
                fired = true;
                break;
            }
        }
        assert!(fired);
        m.cpu_write(0xE002, 0x00); // disable + clear
        assert!(!m.irq_pending());
    }

    #[test]
    fn m42_save_state_round_trip() {
        let mut m = Mapper42::new(synth_prg_8k(8), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 2);
        m.cpu_write(0x8000, 1);
        m.cpu_write(0xE002, 0x02);
        m.notify_cpu_cycle();
        let blob = m.save_state();
        let mut m2 = Mapper42::new(synth_prg_8k(8), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x6000), 2);
        assert_eq!(m2.ppu_read(0x0000), 1);
    }

    // --- Mapper 50 ----------------------------------------------------------

    #[test]
    fn m50_fixed_layout_and_scrambled_switch() {
        let mut m = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x6000), 15);
        assert_eq!(m.cpu_read(0x8000), 8);
        assert_eq!(m.cpu_read(0xA000), 9);
        assert_eq!(m.cpu_read(0xE000), 11);
        // value 0x05 -> (0x05&8)|((0x05&1)<<2)|((0x05&6)>>1) = 0 | 4 | 2 = 6.
        m.cpu_write(0x4020, 0x05);
        assert_eq!(m.cpu_read(0xC000), 6);
    }

    #[test]
    fn m50_irq_fires_once_then_disables() {
        let mut m = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x4120, 0x01); // enable
        for _ in 0..0x1000 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        m.cpu_write(0x4120, 0x00); // disable + ack
        assert!(!m.irq_pending());
    }

    #[test]
    fn m50_save_state_round_trip() {
        let mut m = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x4020, 0x05);
        m.cpu_write(0x4120, 0x01);
        m.notify_cpu_cycle();
        m.ppu_write(0x0007, 0x33);
        let blob = m.save_state();
        let mut m2 = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xC000), 6);
        assert_eq!(m2.ppu_read(0x0007), 0x33);
    }

    // --- Discrete boards ----------------------------------------------------

    #[test]
    fn m46_outer_inner_prg() {
        let mut m = new_m46(synth_prg_32k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0x02); // outer.
        m.cpu_write(0x8000, 0x01); // inner.
        // bank = (2 << 1) | 1 = 5.
        assert_eq!(m.cpu_read(0x8000), 5);
    }

    #[test]
    fn m104_golden_five_blocks() {
        let mut m = new_m104(synth_prg_16k(32), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x08 | 0x02); // block bits -> reg0 high = 0x20.
        m.cpu_write(0xC000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 37 % 32); // inner ((0x20)|5) = 37.
        assert_eq!(m.cpu_read(0xC000), 47 % 32); // high|0x0F = 47.
    }

    #[test]
    fn m290_address_decoded_mirror() {
        let mut m = new_m290(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8400, 0); // bit 0x400 -> horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m301_address_as_data_mirror() {
        let mut m = new_m301(synth_prg_16k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8002, 0); // addr bit 1 -> horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn discrete_save_state_round_trip() {
        let mut m = new_m51(synth_prg_8k(64), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0x10);
        m.cpu_write(0xC000, 0x05);
        m.ppu_write(0x0020, 0x7F);
        let blob = m.save_state();
        let mut m2 = new_m51(synth_prg_8k(64), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0020), 0x7F);
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
    }
}
