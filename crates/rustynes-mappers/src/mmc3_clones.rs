//! MMC3-clone ASICs: mappers 44, 49, 52, 115, 134, 189, 205, 238, 245, 348,
//! 366 and relatives.
//!
//! Unlicensed manufacturers cloned the MMC3 more than any other Nintendo
//! ASIC, because it was the cheapest way to run existing MMC3 games off a
//! multicart. The clones keep the MMC3 register protocol and its A12-driven
//! scanline IRQ counter *exactly*, and add an outer bank register that
//! selects which 128/256/512 KiB "cartridge" the inner MMC3 sees.
//!
//! That is why this is one implementation with a board discriminant
//! ([`CloneBoard`]) rather than eleven: the shared [`Mmc3Clone`] core carries
//! the real MMC3 behaviour, and each board contributes only its outer-register
//! decode. Getting the IRQ timing right once benefits all of them.
//!
//! The genuine Nintendo MMC3 is in `m004_mmc3.rs`.
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
const CHR_BANK_1K: usize = 0x0400;
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

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

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
}
