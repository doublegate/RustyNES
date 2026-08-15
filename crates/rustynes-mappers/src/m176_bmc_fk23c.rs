// SPDX-License-Identifier: GPL-3.0-or-later
//
// Provenance: the BMC-FK23C banking is derived from Mesen2 (GPL-3.0-or-later), `Waixing/Fk23C.h`. See docs/originality-and-provenance.md (Section 1)
// and NOTICE for the complete, audited derivation record.
//! `FK23C` / `BMC-FK23C` (mapper 176) -- the most widely reused pirate ASIC.
//!
//! An MMC3 core wrapped in four outer registers at `$5000-$5FFF` that can
//! *override* the MMC3 entirely: depending on the mode bits the chip either
//! passes banking through to the inner MMC3 or substitutes its own 16/32 KiB
//! layout, and it can redirect CHR to RAM. That flexibility is why one chip
//! backs so many different Chinese multicarts -- the same silicon is
//! configured per-cartridge by the outer registers rather than by a board
//! respin.
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
    clippy::bool_to_int_with_if,
    clippy::unreadable_literal
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

// v2 adds the FS005 RAM Configuration Register byte and the submapper-2 CHR-RAM
// overlay, both of which change the serialized length.
const SAVE_STATE_VERSION: u8 = 2;

// ---------------------------------------------------------------------------
// Shared nametable + mirroring helpers (mirror the other simple-mapper modules).
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

// ===========================================================================
// Fk23c (mapper 176) — Waixing FK23C 8/16 Mbit BMC ASIC.
//
// A $5000-$5003 config bank wrapping a full MMC3 register surface (eight bank
// registers + the $8000/$A000/$C000/$E000 protocol + an A12 scanline IRQ) with
// an outer-bank / extended-MMC3 / CNROM-CHR mode. This is the
// register-decode-faithful BestEffort port: the MMC3 PRG/CHR layout plus the
// FK23C $5000 banking modes (0-2 MMC3, 3 = 32 KiB, 4 = whole-256 KiB) and the
// $5001/$5002 outer PRG/CHR base bits. Register map per the NESdev wiki FK23C /
// mapper-176 documentation; the banking implementation is derived from Mesen2's
// `Waixing/Fk23C.h` (GPL-3.0-or-later). See NOTICE + docs/originality-and-provenance.md §1.
// ===========================================================================

/// Waixing FK23C 8/16 Mbit BMC ASIC (mapper 176).
pub struct Fk23c {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    wram: Box<[u8]>,
    mirroring: Mirroring,
    prg_count_8k: usize,
    chr_count_1k: usize,
    // MMC3 core.
    regs: [u8; 8],
    bank_select: u8,
    prg_mode: bool,
    chr_mode: bool,
    irq_counter: u8,
    irq_latch: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
    last_a12: bool,
    // FK23C config ($5000-$5003).
    prg_banking_mode: u8,
    outer_chr_64k: bool,
    select_chr_ram: bool,
    mmc3_chr_mode: bool,
    cnrom_chr_mode: bool,
    extended_mmc3: bool,
    prg_base: u16,
    chr_base: u8,
    cnrom_chr_reg: u8,
    // ---- FS005 (submapper 2) ------------------------------------------------
    /// NES 2.0 submapper. Only `2` (WAIXING-FS005/FS006) changes behaviour; every
    /// other value keeps the submapper-0/1 FK23C decode this type already had.
    submapper: u8,
    /// RAM Configuration Register (`$A001`), submapper 2 only. Latched verbatim;
    /// the individual fields are decoded on use so that the "functions like MMC3
    /// `$A001` until bit 5 is set" rule needs no shadow state.
    ram_cfg: u8,
    /// 8 KiB of CHR-RAM overlaying the first eight 1 KiB CHR banks when the
    /// mixed CHR-ROM/CHR-RAM mode is armed (`$A001.5` set, `$A001.2` set).
    /// Empty on every other submapper, so nothing else pays for it.
    chr_ram: Box<[u8]>,
}

impl Fk23c {
    /// Number of 1 KiB CHR banks the FS005 mixed-memory mode redirects to
    /// CHR-RAM: "the first 8 KiB of CHR space".
    const FS005_CHR_RAM_BANKS: usize = 8;

    // 8 regs + 9 MMC3 scalars + 6 config bools + 2 prg_base + 3 (chr_base,
    // cnrom_chr_reg, mirroring) + 1 ram_cfg.
    const SAVE_LEN: usize = 8 + 9 + 6 + 2 + 3 + 1;

    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        submapper: u8,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 176)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 0x40000].into_boxed_slice() // up to 256 KiB CHR-RAM.
        } else {
            if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
                return Err(MapperError::Invalid(format!(
                    "mapper 176 CHR-ROM size {} is not a multiple of 1 KiB",
                    chr_rom.len()
                )));
            }
            chr_rom
        };
        let prg_count_8k = prg_rom.len() / PRG_BANK_8K;
        let chr_count_1k = (chr.len() / CHR_BANK_1K).max(1);
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            wram: vec![0u8; 0x8000].into_boxed_slice(),
            mirroring,
            prg_count_8k,
            chr_count_1k,
            regs: [0; 8],
            bank_select: 0,
            prg_mode: false,
            chr_mode: false,
            irq_counter: 0,
            irq_latch: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            last_a12: false,
            prg_banking_mode: 0,
            outer_chr_64k: false,
            select_chr_ram: false,
            mmc3_chr_mode: true,
            cnrom_chr_mode: false,
            extended_mmc3: false,
            prg_base: 0,
            chr_base: 0,
            cnrom_chr_reg: 0,
            submapper,
            ram_cfg: 0,
            chr_ram: if submapper == 2 {
                vec![0u8; Self::FS005_CHR_RAM_BANKS * CHR_BANK_1K].into_boxed_slice()
            } else {
                Box::default()
            },
        })
    }

    // ---- FS005 (submapper 2) register decode --------------------------------
    //
    // Implemented from the NESdev wiki "INES Mapper 176" page (Registers ->
    // Mirroring Register / RAM Configuration Register / Solder Pad / Protection)
    // plus the NESdev forum thread that settles the `$46`/`$47` question. These
    // are hardware facts read from public documentation, not a port: unlike the
    // FK23C banking transforms above, no reference-emulator source was consulted
    // for any of the code below.

    /// True on the WAIXING-FS005/FS006 board (NES 2.0 submapper 2).
    const fn is_fs005(&self) -> bool {
        self.submapper == 2
    }

    /// True once `$A001.5` has turned `$A001` from an MMC3-compatible WRAM-protect
    /// register into the FS005 RAM Configuration Register. Every other field of
    /// `$A001` is documented as "ignored if Bit 5 is clear", so this gates them all.
    const fn ram_cfg_enabled(&self) -> bool {
        self.is_fs005() && (self.ram_cfg & 0x20) != 0
    }

    /// True while the mapper registers are visible in `$5000-$5FFF`.
    ///
    /// `$A001.6` clear (with the RAM Configuration Register enabled) swaps that
    /// window for WRAM, which is the whole mechanism behind the Waixing
    /// copy-protection sequence: the game stashes three bytes through the window
    /// while it is RAM, re-enables the registers, and then reads the same bytes
    /// back through `$6000-$7FFF` to derive the code it jumps to.
    const fn outer_regs_enabled(&self) -> bool {
        !self.ram_cfg_enabled() || (self.ram_cfg & 0x40) != 0
    }

    /// 8 KiB WRAM bank visible at `$6000-$7FFF`. Fixed at bank 0 until the RAM
    /// Configuration Register is enabled, which is what grows the board's WRAM
    /// from 8 KiB to 32 KiB.
    const fn wram_bank(&self) -> usize {
        if self.ram_cfg_enabled() {
            (self.ram_cfg & 0x03) as usize
        } else {
            0
        }
    }

    /// Offset of the `$5000-$5FFF` WRAM window: the second 4 KiB of WRAM bank 2,
    /// fixed there regardless of which bank `$6000-$7FFF` currently shows.
    const FS005_REG_WINDOW_OFF: usize = 2 * 0x2000 + 0x1000;

    /// True when a resolved 1 KiB CHR bank should come from CHR-RAM rather than
    /// CHR-ROM (`$A001.2`, the mapper-195-like mixed-memory mode).
    const fn chr_bank_is_ram(&self, bank: usize) -> bool {
        self.ram_cfg_enabled() && (self.ram_cfg & 0x04) != 0 && bank < Self::FS005_CHR_RAM_BANKS
    }

    fn prg_bank_mmc3(&self, slot: usize) -> usize {
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

    fn resolve_prg(&self, slot: usize) -> usize {
        let outer = (self.prg_base as usize) << 1;
        let bank = match self.prg_banking_mode {
            0..=2 => {
                if self.extended_mmc3 {
                    self.prg_bank_mmc3(slot) | outer
                } else {
                    let inner_mask = 0x3F >> self.prg_banking_mode;
                    let outer = outer & !inner_mask;
                    (self.prg_bank_mmc3(slot) & inner_mask) | outer
                }
            }
            3 => {
                // 32 KiB fixed window from the outer base.
                (outer & !0x03) + slot
            }
            _ => {
                // mode 4: whole 256 KiB.
                ((self.prg_base as usize & 0xFFE) << 1 & !0x07) + slot
            }
        };
        bank % self.prg_count_8k
    }

    fn chr_bank_mmc3(&self, slot: usize) -> usize {
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

    fn resolve_chr(&self, slot: usize) -> usize {
        let bank = if self.mmc3_chr_mode {
            let outer = (self.chr_base as usize) << 3;
            if self.extended_mmc3 {
                self.chr_bank_mmc3(slot) | outer
            } else {
                let inner_mask = if self.outer_chr_64k { 0x7F } else { 0xFF };
                let outer = outer & !inner_mask;
                (self.chr_bank_mmc3(slot) & inner_mask) | outer
            }
        } else {
            // CNROM mode: 8 KiB blocks from the CNROM CHR reg + base.
            let inner_mask = if self.cnrom_chr_mode {
                if self.outer_chr_64k { 1 } else { 3 }
            } else {
                0
            };
            (((self.cnrom_chr_reg as usize & inner_mask) | self.chr_base as usize) << 3) + slot
        };
        bank % self.chr_count_1k
    }

    fn write_5000(&mut self, addr: u16, value: u8) {
        match addr & 0x03 {
            0 => {
                self.prg_banking_mode = value & 0x07;
                self.outer_chr_64k = value & 0x10 != 0;
                self.select_chr_ram = value & 0x20 != 0;
                self.mmc3_chr_mode = value & 0x40 == 0;
                // $5xx0.3 = PRG A21, $5xx0.7 = PRG A22. `prg_base` bit N carries
                // PRG A(14+N), so A21 -> bit 7 and A22 -> bit 8. Documented as
                // submapper-2-only; on every other submapper those two bits have
                // no PRG meaning, so leave the base alone rather than folding in
                // address lines the board does not wire.
                if self.is_fs005() {
                    self.prg_base = (self.prg_base & !0x180)
                        | (((value as u16) & 0x80) << 1)
                        | (((value as u16) & 0x08) << 4);
                }
            }
            1 => self.prg_base = (self.prg_base & !0x7F) | (value as u16 & 0x7F),
            2 => {
                // $5xx2.6-7 = PRG A24..A23 and $5xx2.5 = PRG A25 (submapper 2
                // only), i.e. `prg_base` bits 9, 10 and 11 respectively.
                if self.is_fs005() {
                    self.prg_base = (self.prg_base & !0xE00)
                        | (((value as u16) & 0x40) << 3)
                        | (((value as u16) & 0x80) << 3)
                        | (((value as u16) & 0x20) << 6);
                }
                self.chr_base = value;
                self.cnrom_chr_reg = 0;
            }
            _ => {
                self.extended_mmc3 = value & 0x02 != 0;
                self.cnrom_chr_mode = value & 0x44 != 0;
            }
        }
    }

    fn write_mmc3(&mut self, addr: u16, value: u8) {
        if self.cnrom_chr_mode && (addr <= 0x9FFF || addr >= 0xC000) {
            self.cnrom_chr_reg = value & 0x03;
        }
        // The 8025 decodes its MMC3-compatible registers with mask $E003, not the
        // stock MMC3 $E001 -- verified on real hardware per the NESdev wiki, and
        // load-bearing: some multicart games depend on a write to $9FFF doing
        // nothing, which only holds if bit 1 participates in the decode.
        match addr & 0xE003 {
            0x8000 => {
                // Submapper 2 swaps bank-select values $46 and $47 -- that is,
                // register 6 and register 7 exchange places, but ONLY while the
                // PRG-invert bit ($8000.6) is set. Plain $06/$07 are unswapped.
                let mut reg = value & 0x0F;
                if self.is_fs005() && (value & 0x40) != 0 && matches!(reg, 6 | 7) {
                    reg ^= 1;
                }
                self.bank_select = reg;
                self.prg_mode = value & 0x40 != 0;
                self.chr_mode = value & 0x80 != 0;
            }
            0x8001 => {
                let idx = (self.bank_select & 0x07) as usize;
                self.regs[idx] = value;
            }
            0xA000 => {
                // Two mirroring bits; single-screen is only wired once the RAM
                // Configuration Register is enabled, so a stray $A000 write on a
                // submapper-0/1 multicart cannot reach a single-screen page.
                self.mirroring = match (value & 0x03, self.ram_cfg_enabled()) {
                    (0, _) => Mirroring::Vertical,
                    (2, true) => Mirroring::SingleScreenA,
                    (3, true) => Mirroring::SingleScreenB,
                    _ => Mirroring::Horizontal,
                };
            }
            0xA001 if self.is_fs005() => {
                self.ram_cfg = value;
                // Enabling or disabling single-screen support can invalidate the
                // mirroring latched from a previous $A000 write; re-derive it
                // rather than leaving a single-screen page selected on a board
                // that no longer offers one.
                if !self.ram_cfg_enabled()
                    && matches!(
                        self.mirroring,
                        Mirroring::SingleScreenA | Mirroring::SingleScreenB
                    )
                {
                    self.mirroring = Mirroring::Horizontal;
                }
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
}

impl Mapper for Fk23c {
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
            // With the outer bank registers switched off, $5000-$5FFF is not a
            // register window at all: it reads the second 4 KiB of WRAM bank 2.
            0x5000..=0x5FFF if !self.outer_regs_enabled() => {
                self.wram[Self::FS005_REG_WINDOW_OFF + (addr as usize & 0x0FFF)]
            }
            0x6000..=0x7FFF => self.wram[self.wram_bank() * 0x2000 + (addr as usize & 0x1FFF)],
            0x8000..=0x9FFF => {
                let b = self.resolve_prg(0);
                self.prg_rom[b * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0xA000..=0xBFFF => {
                let b = self.resolve_prg(1);
                self.prg_rom[b * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0xC000..=0xDFFF => {
                let b = self.resolve_prg(2);
                self.prg_rom[b * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0xE000..=0xFFFF => {
                let b = self.resolve_prg(3);
                self.prg_rom[b * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5000..=0x5FFF => {
                if self.outer_regs_enabled() {
                    self.write_5000(addr, value);
                } else {
                    self.wram[Self::FS005_REG_WINDOW_OFF + (addr as usize & 0x0FFF)] = value;
                }
            }
            0x6000..=0x7FFF => {
                let off = self.wram_bank() * 0x2000 + (addr as usize & 0x1FFF);
                self.wram[off] = value;
            }
            0x8000..=0xFFFF => self.write_mmc3(addr, value),
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram || self.select_chr_ram {
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                let slot = (addr as usize) / CHR_BANK_1K;
                let b = self.resolve_chr(slot);
                if self.chr_bank_is_ram(b) {
                    return self.chr_ram[b * CHR_BANK_1K + (addr as usize & 0x3FF)];
                }
                self.chr[b * CHR_BANK_1K + (addr as usize & 0x3FF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            // Only the CHR-RAM variant accepts CHR writes. When the cart
            // provided CHR-ROM (`chr_is_ram == false`), the `select_chr_ram`
            // banking bit selects a flat-CHR read window but must NOT make
            // the ROM mutable: writing it here would corrupt CHR-ROM and
            // (since `save_state` only serializes `self.chr` when
            // `chr_is_ram`) would not round-trip across a save-state. Gate
            // the write on `chr_is_ram` so behaviour + serialization agree.
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (self.chr.len() - 1)] = value;
            }
            // FS005 mixed CHR-ROM/CHR-RAM: a bank the RAM Configuration Register
            // has redirected into the overlay IS writable even on a CHR-ROM cart,
            // and `chr_ram` is serialized, so behaviour and save-state agree.
            0x0000..=0x1FFF => {
                let slot = (addr as usize) / CHR_BANK_1K;
                let b = self.resolve_chr(slot);
                if self.chr_bank_is_ram(b) {
                    self.chr_ram[b * CHR_BANK_1K + (addr as usize & 0x3FF)] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

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
        let mut out =
            Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + self.wram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.push(self.bank_select);
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.chr_mode));
        out.push(self.irq_counter);
        out.push(self.irq_latch);
        out.push(u8::from(self.irq_reload));
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(u8::from(self.last_a12));
        out.push(self.prg_banking_mode);
        out.push(u8::from(self.outer_chr_64k));
        out.push(u8::from(self.select_chr_ram));
        out.push(u8::from(self.mmc3_chr_mode));
        out.push(u8::from(self.cnrom_chr_mode));
        out.push(u8::from(self.extended_mmc3));
        out.extend_from_slice(&self.prg_base.to_le_bytes());
        out.push(self.chr_base);
        out.push(self.cnrom_chr_reg);
        out.push(mirroring_to_byte(self.mirroring));
        out.push(self.ram_cfg);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.wram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        // Zero-length on every submapper but 2, so this is a no-op there.
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected =
            1 + Self::SAVE_LEN + self.vram.len() + self.wram.len() + chr_ram + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut c = 1;
        self.regs.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.bank_select = data[c];
        self.prg_mode = data[c + 1] != 0;
        self.chr_mode = data[c + 2] != 0;
        self.irq_counter = data[c + 3];
        self.irq_latch = data[c + 4];
        self.irq_reload = data[c + 5] != 0;
        self.irq_enabled = data[c + 6] != 0;
        self.irq_pending = data[c + 7] != 0;
        self.last_a12 = data[c + 8] != 0;
        c += 9;
        self.prg_banking_mode = data[c];
        self.outer_chr_64k = data[c + 1] != 0;
        self.select_chr_ram = data[c + 2] != 0;
        self.mmc3_chr_mode = data[c + 3] != 0;
        self.cnrom_chr_mode = data[c + 4] != 0;
        self.extended_mmc3 = data[c + 5] != 0;
        c += 6;
        self.prg_base = u16::from_le_bytes([data[c], data[c + 1]]);
        c += 2;
        self.chr_base = data[c];
        self.cnrom_chr_reg = data[c + 1];
        self.mirroring = byte_to_mirroring(data[c + 2], self.mirroring);
        c += 3;
        self.ram_cfg = data[c];
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.wram.copy_from_slice(&data[c..c + self.wram.len()]);
        c += self.wram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
            c += self.chr.len();
        }
        let n = self.chr_ram.len();
        self.chr_ram.copy_from_slice(&data[c..c + n]);
        Ok(())
    }
}

/// Mapper 176 (8025 enhanced-MMC3 ASIC: Waixing FK23C, and FS005 on submapper 2).
///
/// `submapper` is the NES 2.0 submapper number. Only `2` (WAIXING-FS005/FS006)
/// selects different behaviour; every other value -- including the `0` an iNES 1.0
/// image implies -- keeps the FK23C decode.
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG/CHR size.
pub fn new_m176(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
    submapper: u8,
) -> Result<Fk23c, MapperError> {
    Fk23c::new(prg_rom, chr_rom, mirroring, submapper)
}

// ===========================================================================
// Coolboy (mapper 268) — COOLBOY / MINDKIDS MMC3-clone.
//
// An MMC3 core wrapped by four $6000-$7FFF outer-bank registers that supply
// PRG/CHR base bits + a wider/narrower mask + an extended-bank mode. The
// COOLBOY/MINDKIDS banking transforms are a register-decode BestEffort model;
// the register map is per the nesdev wiki COOLBOY / mapper-268 board notes, and
// the implementation is derived from Mesen2's `Mmc3Variants/MMC3_Coolboy.h`
// (GPL-3.0-or-later) and the FCEUX banking transforms (GPL-2.0-or-later).
// See NOTICE + docs/originality-and-provenance.md §1.
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
    fn fk23c_truncated_save_state_rejected() {
        let m = new_m176(synth_prg_8k(16), synth_chr_1k(32), Mirroring::Vertical, 0).unwrap();
        let mut blob = m.save_state();
        blob.pop();
        let mut m2 = new_m176(synth_prg_8k(16), synth_chr_1k(32), Mirroring::Vertical, 0).unwrap();
        assert!(m2.load_state(&blob).is_err());
    }

    #[test]
    fn fk23c_mmc3_prg_and_a12_irq() {
        let mut m = new_m176(synth_prg_8k(32), synth_chr_1k(64), Mirroring::Vertical, 0).unwrap();
        m.cpu_write(0x8000, 0x06); // select R6
        m.cpu_write(0x8001, 5);
        assert_eq!(m.cpu_read(0x8000), 5); // R6 @ $8000
        assert_eq!(m.cpu_read(0xE000), 31); // last @ $E000

        m.cpu_write(0xC000, 2); // latch
        m.cpu_write(0xC001, 0); // reload
        m.cpu_write(0xE001, 0); // enable
        for _ in 0..3 {
            m.notify_a12(false);
            m.notify_a12(true);
        }
        assert!(m.irq_pending());
        m.cpu_write(0xE000, 0); // disable + ack
        assert!(!m.irq_pending());
    }

    #[test]
    fn fk23c_outer_prg_base() {
        let mut m = new_m176(synth_prg_8k(128), synth_chr_1k(64), Mirroring::Vertical, 0).unwrap();
        // $5001 sets PRG base low bits; $5000 mode 0 = MMC3 with outer.
        m.cpu_write(0x5001, 0x08); // prg_base low = 8 -> outer = 16 (8<<1)
        m.cpu_write(0x8000, 0x06); // R6
        m.cpu_write(0x8001, 0); // R6 = 0
        // mode 0 inner_mask = 0x3F, outer = 16 & ~0x3F = 0 -> bank 0. base only
        // affects above the inner window; just confirm read is in range + no panic.
        let _ = m.cpu_read(0x8000);
        assert!(m.cpu_read(0x8000) < 128);
    }

    #[test]
    fn fk23c_save_state_round_trip() {
        let mut m = new_m176(synth_prg_8k(32), Box::new([]), Mirroring::Vertical, 0).unwrap();
        m.cpu_write(0x5000, 0x20); // select CHR-RAM
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 7);
        m.ppu_write(0x0040, 0x99);
        m.cpu_write(0x6000, 0x5A); // WRAM
        let blob = m.save_state();
        let mut m2 = new_m176(synth_prg_8k(32), Box::new([]), Mirroring::Vertical, 0).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0040), 0x99);
        assert_eq!(m2.cpu_read(0x6000), 0x5A);
    }

    #[test]
    fn fk23c_chr_rom_not_writable_via_select_chr_ram() {
        // FK23C: even with `select_chr_ram` set, a CHR-ROM cart must not be
        // mutated (regression: `ppu_write` wrote through `self.chr`, which
        // corrupted CHR-ROM and was never serialized).
        let mut m = new_m176(synth_prg_8k(32), synth_chr_1k(64), Mirroring::Vertical, 0).unwrap();
        m.cpu_write(0x5000, 0x20); // select_chr_ram = true
        let before = m.ppu_read(0x0010);
        m.ppu_write(0x0010, before.wrapping_add(1));
        assert_eq!(m.ppu_read(0x0010), before, "CHR-ROM must not be mutable");
    }

    // ---- FS005 (submapper 2) ------------------------------------------------

    fn fs005() -> Fk23c {
        new_m176(synth_prg_8k(32), synth_chr_1k(256), Mirroring::Vertical, 2).unwrap()
    }

    #[test]
    fn fs005_a001_bit5_switches_a001_from_mmc3_to_ram_config() {
        let mut m = fs005();
        // Bit 5 clear: $A001 behaves as on the MMC3, so the WRAM bank select in
        // bits 0-1 is inert and $6000 still shows bank 0.
        m.cpu_write(0xA001, 0x02);
        m.cpu_write(0x6000, 0xAA);
        m.cpu_write(0xA001, 0x00);
        assert_eq!(
            m.cpu_read(0x6000),
            0xAA,
            "bank select must be ignored while $A001.5 is clear"
        );

        // Bit 5 set: the same bits now pick one of four 8 KiB banks, so the byte
        // written to bank 0 is no longer visible from bank 2.
        m.cpu_write(0xA001, 0x22);
        assert_ne!(m.cpu_read(0x6000), 0xAA, "bank 2 must not alias bank 0");
        m.cpu_write(0x6000, 0x55);
        m.cpu_write(0xA001, 0x20); // back to bank 0
        assert_eq!(m.cpu_read(0x6000), 0xAA);
        m.cpu_write(0xA001, 0x22); // bank 2 again
        assert_eq!(m.cpu_read(0x6000), 0x55);
    }

    #[test]
    fn fs005_protection_sequence_round_trips_through_the_register_window() {
        // The documented Waixing protection dance: park $5000-$5FFF over WRAM,
        // stash bytes there, re-enable the registers, and read the same bytes
        // back through $6000-$7FFF with WRAM bank 2 selected.
        let mut m = fs005();
        m.cpu_write(0xA001, 0xA1); // enable cfg, outer regs OFF
        m.cpu_write(0x5000, 0x11);
        m.cpu_write(0x5010, 0x22);
        m.cpu_write(0x5013, 0x33);
        m.cpu_write(0xA001, 0xE2); // outer regs ON, WRAM bank 2 at $6000

        assert_eq!(m.cpu_read(0x7000), 0x11);
        assert_eq!(m.cpu_read(0x7010), 0x22);
        assert_eq!(m.cpu_read(0x7013), 0x33);
    }

    #[test]
    fn fs005_disabled_register_window_does_not_reach_the_mapper() {
        // The point of the window swap is that those writes must NOT be decoded
        // as mode-register writes. $5000 = $07 would otherwise select an unused
        // PRG banking mode and change what $8000 reads.
        let mut m = fs005();
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 5);
        let before = m.cpu_read(0x8000);

        m.cpu_write(0xA001, 0xA1); // registers off
        m.cpu_write(0x5000, 0x04); // would be NROM-256 mode if it landed
        assert_eq!(
            m.cpu_read(0x8000),
            before,
            "a disabled window must not bank"
        );

        m.cpu_write(0xA001, 0xE0); // registers back on
        m.cpu_write(0x5000, 0x04);
        assert_ne!(m.cpu_read(0x8000), before, "an enabled window must bank");
    }

    #[test]
    fn fs005_a000_selects_single_screen_only_while_ram_config_is_enabled() {
        let mut m = fs005();
        m.cpu_write(0xA000, 0x02);
        assert_eq!(
            m.current_mirroring(),
            Mirroring::Horizontal,
            "single-screen is unwired until $A001.5 is set"
        );

        m.cpu_write(0xA001, 0x20);
        m.cpu_write(0xA000, 0x02);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0xA000, 0x03);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0xA000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);

        // Turning the RAM Configuration Register back off must not strand a
        // single-screen page on a board that no longer offers one.
        m.cpu_write(0xA000, 0x03);
        m.cpu_write(0xA001, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn fs005_swaps_bank_select_46_and_47_but_not_06_and_07() {
        let mut m = fs005();
        // $46/$47: PRG-invert set, so registers 6 and 7 exchange places.
        m.cpu_write(0x8000, 0x46);
        m.cpu_write(0x8001, 9);
        m.cpu_write(0x8000, 0x47);
        m.cpu_write(0x8001, 4);
        // Under the swap, $46 wrote R7 (-> $A000) and $47 wrote R6 (-> $C000,
        // because the PRG-invert bit is set).
        assert_eq!(m.cpu_read(0xA000), 9);
        assert_eq!(m.cpu_read(0xC000), 4);

        // $06/$07 are explicitly NOT swapped.
        let mut m = fs005();
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 9);
        m.cpu_write(0x8000, 0x07);
        m.cpu_write(0x8001, 4);
        assert_eq!(m.cpu_read(0x8000), 9, "R6 at $8000 with PRG-invert clear");
        assert_eq!(m.cpu_read(0xA000), 4, "R7 at $A000");
    }

    #[test]
    fn fk23c_bank_select_46_47_is_not_swapped_on_submapper_0() {
        // The swap is an FS005 property; the FK23C multicarts must be unchanged.
        let mut m = new_m176(synth_prg_8k(32), synth_chr_1k(64), Mirroring::Vertical, 0).unwrap();
        m.cpu_write(0x8000, 0x46);
        m.cpu_write(0x8001, 9);
        assert_eq!(m.cpu_read(0xC000), 9, "R6 at $C000 with PRG-invert set");
    }

    #[test]
    fn mmc3_registers_decode_with_mask_e003_so_9fff_does_nothing() {
        let mut m = fs005();
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 5);
        let before = m.cpu_read(0x8000);
        // $9FFF & $E003 == $8003, which is not a register. Under the stock MMC3
        // $E001 mask it would alias $8001 and rewrite R6.
        m.cpu_write(0x9FFF, 0x1F);
        assert_eq!(m.cpu_read(0x8000), before);
    }

    #[test]
    fn fs005_mixed_chr_makes_the_first_8k_writable_on_a_chr_rom_cart() {
        let mut m = fs005();
        m.cpu_write(0xA001, 0x24); // cfg enabled + first 8 KiB is CHR-RAM

        let before = m.ppu_read(0x0010);
        m.ppu_write(0x0010, before.wrapping_add(1));
        assert_eq!(
            m.ppu_read(0x0010),
            before.wrapping_add(1),
            "the redirected banks are RAM and must accept writes"
        );

        // With the mode off, the same address is CHR-ROM again and unwritable --
        // and reads the ROM byte, not the overlay.
        m.cpu_write(0xA001, 0x20);
        assert_eq!(m.ppu_read(0x0010), before);
        m.ppu_write(0x0010, before.wrapping_add(2));
        assert_eq!(m.ppu_read(0x0010), before, "CHR-ROM must stay immutable");
    }

    #[test]
    fn fs005_save_state_round_trips_ram_config_and_chr_overlay() {
        let mut m = fs005();
        m.cpu_write(0xA001, 0x26); // cfg on, mixed CHR on, WRAM bank 2
        m.ppu_write(0x0010, 0x5A);
        m.cpu_write(0x6000, 0xC3);
        m.cpu_write(0x8000, 0x46);
        m.cpu_write(0x8001, 9);
        let blob = m.save_state();

        let mut m2 = fs005();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0010), 0x5A, "CHR-RAM overlay must round-trip");
        assert_eq!(m2.cpu_read(0x6000), 0xC3, "banked WRAM must round-trip");
        assert_eq!(m2.cpu_read(0xA000), m.cpu_read(0xA000));
        assert_eq!(m2.current_mirroring(), m.current_mirroring());
    }

    #[test]
    fn fs005_state_blob_is_longer_than_the_fk23c_one() {
        // Guards the SAVE_LEN / overlay arithmetic: submapper 2 carries an extra
        // 8 KiB of CHR-RAM, so the two lengths must not be equal by accident.
        let a = new_m176(synth_prg_8k(32), synth_chr_1k(256), Mirroring::Vertical, 0)
            .unwrap()
            .save_state()
            .len();
        let b = fs005().save_state().len();
        assert_eq!(b - a, Fk23c::FS005_CHR_RAM_BANKS * CHR_BANK_1K);
    }
}
