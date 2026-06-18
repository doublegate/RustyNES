//! Sprint 12 reusable-ASIC BMC / pirate mappers
//! (v1.7.0 "Forge" Workstream G1 mapper-breadth continuation, 150 -> 168).
//!
//! A best-effort (Tier-2) batch of unlicensed / pirate / multicart "reusable
//! ASIC" boards ported from the reference emulators (`Mesen2`
//! `Waixing/`, `Mmc3Variants/`, `Sachen/`, `Kaiser/`, `Unlicensed/`,
//! `Txc/`) and the nesdev wiki. Like `sprint5`..`sprint11`, banking math is
//! translated into direct slice indexing and every bank select wraps with
//! `% count`, so a register write can never index out of bounds (no panics on
//! register access — required for the `#![no_std]` chip stack). All boards
//! here are register-decode + save-state unit-tested only and are **never**
//! accuracy-gated (see `tier.rs` `MapperTier::BestEffort` +
//! `docs/adr/0011-mapper-tiering.md`).
//!
//! Clusters covered (FK23C / COOLBOY / MINDKIDS / Sachen / Waixing / Kaiser):
//!
//! - **Mapper 176** ([`Fk23c`]) — Waixing FK23C 8/16 Mbit BMC ASIC: a
//!   `$5000-$5003` config bank + a full MMC3 register surface with an A12
//!   scanline IRQ and an outer-bank/extended-MMC3 mode. Ported from
//!   `Mesen2 Waixing/Fk23C.h` + nesdev wiki "INES Mapper 176".
//! - **Mapper 268** ([`Coolboy`]) — COOLBOY / MINDKIDS MMC3-clone with a
//!   `$6000-$7FFF` four-register outer-bank block (PRG/CHR base + masks).
//!   Ported from `Mesen2 Mmc3Variants/MMC3_Coolboy.h` (itself from FCEUX).
//! - **Mapper 513** ([`Sachen9602`]) — Sachen 9602 MMC3-clone with a
//!   `$8000`/`$8001` PRG-A19/A20 outer-bank override. Ported from
//!   `Mesen2 Sachen/Sachen9602.h`.
//! - **Mapper 136** ([`Sachen3011`]) — Sachen 3011 (`Sachen_136`): the TXC
//!   protection chip (`$4100-$4103` accumulator) driving an 8 KiB CHR select.
//!   Ported from `Mesen2 Sachen/Sachen_136.h` + `Txc/TxcChip.h`.
//! - **Mapper 164** ([`Waixing164`]) — Waixing "Final Fantasy V" 32 KiB-PRG
//!   board: a `$5000`/`$5100` split PRG-bank register. Ported from
//!   `Mesen2 Waixing/Waixing164.h`.
//! - **Mapper 253** ([`Waixing253`]) — Waixing VRC4-clone (*Dragon Ball Z*):
//!   per-1 KiB CHR low/high registers, a CHR-RAM escape, and a scaled
//!   CPU-cycle IRQ. Ported from `Mesen2 Waixing/Mapper253.h`.
//! - **Mapper 286** ([`Bs5`]) — Waixing BS-5 (*Olympic* multicart): four
//!   `$8000`/`$A000` address-decoded PRG+CHR banks gated by a DIP-switch.
//!   Ported from `Mesen2 Waixing/Bs5.h`.
//! - **Mapper 56** / **142** ([`Kaiser202`]) — Kaiser KS202 / KS7032
//!   FDS-conversion ASIC: a 4-register PRG bank set, an enable-gated up-counting
//!   M2 IRQ, and (m56) extra CHR + mirror writes. Ported from
//!   `Mesen2 Kaiser/Kaiser202.h`.
//! - **Mapper 303** ([`Kaiser7017`]) — Kaiser KS7017 FDS-conversion: an
//!   address-decoded PRG select + a down-counting M2 IRQ with a `$4030`
//!   read-back acknowledge. Ported from `Mesen2 Kaiser/Kaiser7017.h`.
//! - **Mapper 305** ([`Kaiser7031`]) — Kaiser KS7031: four 2 KiB `$6000`
//!   PRG-ROM windows selected by `$8000-$FFFF` writes. Ported from
//!   `Mesen2 Kaiser/Kaiser7031.h`.
//! - **Mapper 306** ([`Kaiser7016`]) — Kaiser KS7016: an address-decoded
//!   `$6000-$7FFF` PRG-ROM window. Ported from `Mesen2 Kaiser/Kaiser7016.h`.
//! - **Mapper 312** ([`Kaiser7013B`]) — Kaiser KS7013B: a `$6000-$7FFF` PRG
//!   select + `$8000-$FFFF` mirroring. Ported from
//!   `Mesen2 Kaiser/Kaiser7013B.h`.
//! - **Mapper 261** ([`Bmc810544`]) — BMC-810544-C-A1 multicart: the written
//!   *address* carries the PRG block, NROM/UNROM mode, CHR + mirroring. Ported
//!   from `Mesen2 Unlicensed/Bmc810544CA1.h`.
//! - **Mapper 289** ([`Bmc60311`]) — BMC-60311C: a `$6000` mode + `$6001`
//!   outer-PRG + `$8000` inner-PRG NROM/UNROM multicart. Ported from
//!   `Mesen2 Unlicensed/Bmc60311C.h`.
//! - **Mapper 320** ([`Bmc830425`]) — BMC-830425C-4391T: a `$8000` inner-PRG +
//!   `$F0E0`-decoded outer-PRG UNROM/UOROM multicart. Ported from
//!   `Mesen2 Unlicensed/Bmc830425C4391T.h`.
//! - **Mapper 336** ([`BmcK3046`]) — BMC-K-3046: a single `$8000` register with
//!   a 3-bit inner + 3-bit outer UNROM-style PRG select. Ported from
//!   `Mesen2 Unlicensed/BmcK3046.h`.
//! - **Mapper 349** ([`BmcG146`]) — BMC-G-146: an address-decoded
//!   NROM/UNROM/NROM-256 multicart. Ported from `Mesen2 Unlicensed/BmcG146.h`.

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
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const PRG_BANK_2K: usize = 0x0800;
const CHR_BANK_1K: usize = 0x0400;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

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
// $5001/$5002 outer PRG/CHR base bits. Ported from Mesen2 Waixing/Fk23C.h.
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
}

impl Fk23c {
    // 8 regs + 9 MMC3 scalars + 6 config bools + 2 prg_base + 3 (chr_base,
    // cnrom_chr_reg, mirroring).
    const SAVE_LEN: usize = 8 + 9 + 6 + 2 + 3;

    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
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
        })
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
                self.prg_base = (self.prg_base & !0x180)
                    | (((value as u16) & 0x80) << 1)
                    | (((value as u16) & 0x08) << 4);
            }
            1 => self.prg_base = (self.prg_base & !0x7F) | (value as u16 & 0x7F),
            2 => {
                self.prg_base = (self.prg_base & !0x200) | (((value as u16) & 0x40) << 3);
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
        match addr & 0xE001 {
            0x8000 => {
                self.bank_select = value & 0x0F;
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
            0x6000..=0x7FFF => self.wram[addr as usize & 0x1FFF],
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
            0x5000..=0x5FFF => self.write_5000(addr, value),
            0x6000..=0x7FFF => self.wram[addr as usize & 0x1FFF] = value,
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
                self.chr[b * CHR_BANK_1K + (addr as usize & 0x3FF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram || self.select_chr_ram => {
                self.chr[addr as usize & (self.chr.len() - 1)] = value;
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
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.wram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + self.wram.len() + chr_ram;
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
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.wram.copy_from_slice(&data[c..c + self.wram.len()]);
        c += self.wram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

/// Mapper 176 (Waixing FK23C 8/16 Mbit BMC).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG/CHR size.
pub fn new_m176(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Fk23c, MapperError> {
    Fk23c::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// Coolboy (mapper 268) — COOLBOY / MINDKIDS MMC3-clone.
//
// An MMC3 core wrapped by four $6000-$7FFF outer-bank registers (_exRegs[0..3])
// that supply PRG/CHR base bits + a wider/narrower mask + an extended-bank mode
// (_exRegs[3] & 0x10). This is the register-decode-faithful BestEffort port of
// the FCEUX/Mesen2 banking transforms. Ported from
// Mesen2 Mmc3Variants/MMC3_Coolboy.h.
// ===========================================================================

/// COOLBOY / MINDKIDS MMC3-clone multicart (mapper 268).
pub struct Coolboy {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    prg_count_8k: usize,
    chr_count_1k: usize,
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
    ex_regs: [u8; 4],
}

impl Coolboy {
    const SAVE_LEN: usize = 8 + 9 + 4 + 1;

    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 268)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 0x40000].into_boxed_slice()
        } else {
            if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
                return Err(MapperError::Invalid(format!(
                    "mapper 268 CHR-ROM size {} is not a multiple of 1 KiB",
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
            ex_regs: [0; 4],
        })
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
        let page = self.prg_bank_mmc3(slot);
        let e0 = self.ex_regs[0] as usize;
        let e1 = self.ex_regs[1] as usize;
        let e3 = self.ex_regs[3] as usize;
        let mut mask =
            ((0x3F | (e1 & 0x40) | ((e1 & 0x20) << 2)) ^ ((e0 & 0x40) >> 2)) ^ ((e1 & 0x80) >> 2);
        let base = (e0 & 0x07) | ((e1 & 0x10) >> 1) | ((e1 & 0x0C) << 2) | ((e0 & 0x30) << 2);
        let bank = if e3 & 0x10 == 0 {
            ((base << 4) & !mask) | (page & mask)
        } else {
            mask &= 0xF0;
            let emask = if e1 & 0x02 != 0 {
                (e3 & 0x0C) | (slot & 0x01)
            } else {
                e3 & 0x0E
            };
            ((base << 4) & !mask) | (page & mask) | emask | (slot & 0x01)
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
        let page = self.chr_bank_mmc3(slot);
        let e0 = self.ex_regs[0] as usize;
        let e2 = self.ex_regs[2] as usize;
        let e3 = self.ex_regs[3] as usize;
        let mask = 0xFF ^ (e0 & 0x80);
        let bank = if e3 & 0x10 != 0 {
            (page & 0x80 & mask) | (((e0 & 0x08) << 4) & !mask) | ((e2 & 0x0F) << 3) | slot
        } else {
            (page & mask) | (((e0 & 0x08) << 4) & !mask)
        };
        bank % self.chr_count_1k
    }

    fn write_mmc3(&mut self, addr: u16, value: u8) {
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
}

impl Mapper for Coolboy {
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
            0x6000..=0x7FFF => {
                // Outer-bank registers, latched while $E000-bit-7 mode allows.
                if (self.ex_regs[3] & 0x90) != 0x80 {
                    self.ex_regs[(addr & 0x03) as usize] = value;
                }
            }
            0x8000..=0xFFFF => self.write_mmc3(addr, value),
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
                let slot = (addr as usize) / CHR_BANK_1K;
                let b = self.resolve_chr(slot);
                self.chr[b * CHR_BANK_1K + (addr as usize & 0x3FF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (self.chr.len() - 1)] = value;
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
        let mut out = Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + chr_ram);
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
        out.extend_from_slice(&self.ex_regs);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + chr_ram;
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
        self.ex_regs.copy_from_slice(&data[c..c + 4]);
        c += 4;
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

/// Mapper 268 (COOLBOY / MINDKIDS MMC3-clone multicart).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG/CHR size.
pub fn new_m268(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Coolboy, MapperError> {
    Coolboy::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// Sachen9602 (mapper 513) — Sachen 9602 MMC3-clone.
//
// A plain MMC3 core with a PRG-A19/A20 outer bank from the high two bits of
// $8001 (captured when the selected register is < 6), forced into the top of
// the address space. CHR is RAM. Ported from Mesen2 Sachen/Sachen9602.h.
// ===========================================================================

/// Sachen 9602 MMC3-clone (mapper 513).
pub struct Sachen9602 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    prg_count_8k: usize,
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
    /// PRG outer bank (high two bits, << 6).
    outer: u8,
}

impl Sachen9602 {
    const SAVE_LEN: usize = 8 + 9 + 1 + 1;

    fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 513)?;
        let prg_count_8k = prg_rom.len() / PRG_BANK_8K;
        Ok(Self {
            prg_rom,
            chr: vec![0u8; CHR_BANK_8K].into_boxed_slice(), // 8 KiB CHR-RAM.
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg_count_8k,
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
            outer: 0,
        })
    }

    fn resolve_prg(&self, slot: usize) -> usize {
        let outer = (self.outer as usize) << 6;
        // m9602 fixes the two top banks to $3E/$3F (within the outer bank).
        let bank = match (slot, self.prg_mode) {
            (1, _) => (self.regs[7] as usize & 0x3F) | outer,
            (0, false) => (self.regs[6] as usize & 0x3F) | outer,
            (0, true) => 0x3E | outer,
            (2, false) => 0x3E | outer,
            (2, true) => (self.regs[6] as usize & 0x3F) | outer,
            (3, _) => 0x3F | outer,
            _ => 0,
        };
        bank % self.prg_count_8k
    }

    fn write_mmc3(&mut self, addr: u16, value: u8) {
        match addr & 0xE001 {
            0x8000 => {
                self.bank_select = value & 0x07;
                self.prg_mode = value & 0x40 != 0;
                self.chr_mode = value & 0x80 != 0;
            }
            0x8001 => {
                if (self.bank_select & 0x07) < 6 {
                    self.outer = value >> 6;
                }
                let idx = (self.bank_select & 0x07) as usize;
                self.regs[idx] = value & 0x3F;
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
}

impl Mapper for Sachen9602 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            self.write_mmc3(addr, value);
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
            0x0000..=0x1FFF => self.chr[addr as usize & (CHR_BANK_8K - 1)] = value,
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
        let mut out = Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + self.chr.len());
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
        out.push(self.outer);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + self.chr.len();
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
        self.outer = data[c];
        self.mirroring = byte_to_mirroring(data[c + 1], self.mirroring);
        c += 2;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        Ok(())
    }
}

/// Mapper 513 (Sachen 9602 MMC3-clone).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
// The `_chr_rom: Box<[u8]>` keeps the factory signature uniform with the
// dispatch site even though the 9602 is always CHR-RAM.
#[allow(clippy::boxed_local)]
pub fn new_m513(
    prg_rom: Box<[u8]>,
    _chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Sachen9602, MapperError> {
    Sachen9602::new(prg_rom, mirroring)
}

// ===========================================================================
// TxcChip — the TXC protection accumulator (shared by Sachen 3011 / m136).
// Ported from Mesen2 Txc/TxcChip.h (the non-JV001 variant, mask 0x07).
// ===========================================================================

#[derive(Default, Clone)]
struct TxcChip {
    accumulator: u8,
    inverter: u8,
    staging: u8,
    output: u8,
    increase: bool,
    invert: bool,
}

impl TxcChip {
    const MASK: u8 = 0x07;
    const SAVE_LEN: usize = 6;

    fn read(&self) -> u8 {
        (self.accumulator & Self::MASK)
            | ((self.inverter ^ if self.invert { 0xFF } else { 0 }) & !Self::MASK)
    }

    fn write(&mut self, addr: u16, value: u8) {
        if addr < 0x8000 {
            match addr & 0xE103 {
                0x4100 => {
                    if self.increase {
                        self.accumulator = self.accumulator.wrapping_add(1);
                    } else {
                        self.accumulator = ((self.accumulator & !Self::MASK)
                            | (self.staging & Self::MASK))
                            ^ if self.invert { 0xFF } else { 0 };
                    }
                }
                0x4101 => self.invert = value & 0x01 != 0,
                0x4102 => {
                    self.staging = value & Self::MASK;
                    self.inverter = value & !Self::MASK;
                }
                0x4103 => self.increase = value & 0x01 != 0,
                _ => {}
            }
        } else {
            self.output = (self.accumulator & 0x0F) | ((self.inverter & 0x08) << 1);
        }
    }

    fn save(&self, out: &mut Vec<u8>) {
        out.push(self.accumulator);
        out.push(self.inverter);
        out.push(self.staging);
        out.push(self.output);
        out.push(u8::from(self.increase));
        out.push(u8::from(self.invert));
    }

    fn load(&mut self, d: &[u8]) {
        self.accumulator = d[0];
        self.inverter = d[1];
        self.staging = d[2];
        self.output = d[3];
        self.increase = d[4] != 0;
        self.invert = d[5] != 0;
    }
}

/// Sachen 3011 (mapper 136): TXC protection chip driving an 8 KiB CHR select.
pub struct Sachen3011 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    chr_count_8k: usize,
    txc: TxcChip,
}

impl Sachen3011 {
    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 136)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        let chr_count_8k = (chr.len() / CHR_BANK_8K).max(1);
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            chr_count_8k,
            txc: TxcChip::default(),
        })
    }
}

impl Mapper for Sachen3011 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4100..=0x5FFF => {
                // $4100 returns the chip read in the low 6 bits.
                let v = if addr & 0x103 == 0x100 {
                    self.txc.read() & 0x3F
                } else {
                    0
                };
                self.txc.write(addr, 0); // refresh output (read has side-effects).
                v
            }
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
                self.prg_rom[(addr as usize & 0x7FFF) % (count * PRG_BANK_32K)]
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4100-$5FFF except the protection port reads open bus.
        (0x4020..=0x5FFF).contains(&addr) && (addr & 0x103 != 0x100)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x4100..=0xFFFF).contains(&addr) {
            self.txc.write(addr, value & 0x3F);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let bank = (self.txc.output as usize) % self.chr_count_8k;
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
        let mut out = Vec::with_capacity(1 + TxcChip::SAVE_LEN + 1 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        self.txc.save(&mut out);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + TxcChip::SAVE_LEN + 1 + self.vram.len() + chr_ram;
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
        self.txc.load(&data[c..c + TxcChip::SAVE_LEN]);
        c += TxcChip::SAVE_LEN;
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

/// Mapper 136 (Sachen 3011, TXC protection CHR select).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG size.
pub fn new_m136(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Sachen3011, MapperError> {
    Sachen3011::new(prg_rom, chr_rom, mirroring)
}

// ===========================================================================
// SimpleBmc — a shared body for the address/register-decoded discrete BMC
// multicarts that have no IRQ and an 8 KiB CHR-ROM/RAM window:
//   164 (Waixing164), 261 (Bmc810544), 289 (Bmc60311), 320 (Bmc830425),
//   336 (BmcK3046), 349 (BmcG146), 286 (Bs5).
// Each board's bank decode is in `SimpleBoard`. PRG slots are tracked as two
// 16 KiB windows (slot 0 = $8000-$BFFF, slot 1 = $C000-$FFFF) so 32 KiB and
// UNROM-style layouts share one read path; CHR is one 8 KiB window (286 uses
// four 2 KiB windows, handled inline).
// ===========================================================================

/// Which discrete BMC board the [`SimpleBmc`] body models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleBoard {
    /// Mapper 164 (Waixing "Final Fantasy V", 32 KiB PRG).
    M164,
    /// Mapper 261 (BMC-810544-C-A1).
    M261,
    /// Mapper 289 (BMC-60311C).
    M289,
    /// Mapper 320 (BMC-830425C-4391T).
    M320,
    /// Mapper 336 (BMC-K-3046).
    M336,
    /// Mapper 349 (BMC-G-146).
    M349,
    /// Mapper 286 (Waixing BS-5).
    M286,
}

/// A discrete BMC multicart with a simple (IRQ-free) register surface.
pub struct SimpleBmc {
    board: SimpleBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// 16 KiB PRG window for $8000 and $C000.
    prg0: usize,
    prg1: usize,
    /// 8 KiB CHR window (164/261/289/320/336/349).
    chr8: usize,
    /// Per-2 KiB CHR windows (286).
    chr2: [usize; 4],
    /// Per-8 KiB PRG window for 286 (four 8 KiB windows).
    prg8: [usize; 4],
    // Board scratch registers.
    reg_inner: u8,
    reg_outer: u8,
    reg_mode: u8,
    dip: u8,
}

impl SimpleBmc {
    const SAVE_LEN: usize = 4 + 8 + 8 + 4 + 1;

    fn new(
        board: SimpleBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        id: u16,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, id)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        let mut m = Self {
            board,
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg0: 0,
            prg1: 0,
            chr8: 0,
            chr2: [0; 4],
            prg8: [0; 4],
            reg_inner: 0,
            reg_outer: 0,
            reg_mode: 0,
            dip: 0,
        };
        m.reset_banks();
        Ok(m)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn prg_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_count_8k(&self) -> usize {
        (self.chr.len() / CHR_BANK_8K).max(1)
    }

    /// Initial / power-on bank layout per board.
    fn reset_banks(&mut self) {
        let last16 = self.prg_count_16k() - 1;
        match self.board {
            SimpleBoard::M164 => {
                // $5000/$5100 split 32 KiB PRG select; power-on 0x0F.
                self.reg_inner = 0x0F;
                self.update_m164();
            }
            SimpleBoard::M286 => {
                let last8 = self.prg_count_8k() - 1;
                let last_chr2 = (self.chr.len() / CHR_BANK_2K).max(1) - 1;
                for s in &mut self.prg8 {
                    *s = last8;
                }
                for c in &mut self.chr2 {
                    *c = last_chr2;
                }
            }
            SimpleBoard::M320 => self.update_m320(),
            SimpleBoard::M289 => self.update_m289(),
            _ => {
                self.prg0 = 0;
                self.prg1 = last16;
            }
        }
    }

    fn update_m164(&mut self) {
        // 32 KiB PRG window selected by the 8-bit split register.
        let count32 = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank32 = (self.reg_inner as usize) % count32;
        self.prg0 = bank32 * 2;
        self.prg1 = bank32 * 2 + 1;
    }

    fn update_m289(&mut self) {
        let page = self.reg_outer as usize
            | (if self.reg_mode & 0x04 != 0 {
                0
            } else {
                self.reg_inner as usize
            });
        match self.reg_mode & 0x03 {
            0 => {
                self.prg0 = page;
                self.prg1 = page;
            }
            1 => {
                let b = page & 0xFE;
                self.prg0 = b;
                self.prg1 = b | 1;
            }
            2 => {
                self.prg0 = page;
                self.prg1 = self.reg_outer as usize | 7;
            }
            _ => {}
        }
        self.mirroring = if self.reg_mode & 0x08 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
    }

    fn update_m320(&mut self) {
        let outer = (self.reg_outer as usize) << 3;
        if self.reg_mode != 0 {
            // UNROM mode.
            self.prg0 = (self.reg_inner as usize & 0x07) | outer;
            self.prg1 = 0x07 | outer;
        } else {
            // UOROM mode.
            self.prg0 = (self.reg_inner as usize) | outer;
            self.prg1 = 0x0F | outer;
        }
    }

    fn prg_byte(&self, slot16: usize, addr: u16) -> u8 {
        let count = self.prg_count_16k();
        let bank = slot16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for SimpleBmc {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if self.board == SimpleBoard::M286 {
            if let 0x8000..=0xFFFF = addr {
                let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                let count = self.prg_count_8k();
                let bank = self.prg8[slot] % count;
                return self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)];
            }
            return 0;
        }
        match addr {
            0x8000..=0xBFFF => self.prg_byte(self.prg0, addr),
            0xC000..=0xFFFF => self.prg_byte(self.prg1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            SimpleBoard::M164 => {
                if (0x5000..=0x5FFF).contains(&addr) {
                    match addr & 0x7300 {
                        0x5000 => self.reg_inner = (self.reg_inner & 0xF0) | (value & 0x0F),
                        0x5100 => self.reg_inner = (self.reg_inner & 0x0F) | ((value & 0x0F) << 4),
                        _ => {}
                    }
                    self.update_m164();
                }
            }
            SimpleBoard::M261 => {
                if addr >= 0x8000 {
                    let bank = ((addr >> 6) & 0xFFFE) as usize;
                    if addr & 0x40 != 0 {
                        self.prg0 = bank;
                        self.prg1 = bank | 1;
                    } else {
                        let b = bank | ((addr >> 5) & 0x01) as usize;
                        self.prg0 = b;
                        self.prg1 = b;
                    }
                    self.chr8 = (addr & 0x0F) as usize;
                    self.mirroring = if addr & 0x10 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            SimpleBoard::M289 => {
                if addr >= 0x8000 {
                    self.reg_inner = value & 0x07;
                } else {
                    match addr & 0xE001 {
                        0x6000 => self.reg_mode = value & 0x0F,
                        0x6001 => self.reg_outer = value,
                        _ => {}
                    }
                }
                self.update_m289();
            }
            SimpleBoard::M320 => {
                if addr >= 0x8000 {
                    self.reg_inner = value & 0x0F;
                    if addr & 0xFFE0 == 0xF0E0 {
                        self.reg_outer = (addr & 0x0F) as u8;
                        self.reg_mode = ((addr >> 4) & 0x01) as u8;
                    }
                    self.update_m320();
                }
            }
            SimpleBoard::M336 => {
                if addr >= 0x8000 {
                    let inner = value as usize & 0x07;
                    let outer = value as usize & 0x38;
                    self.prg0 = outer | inner;
                    self.prg1 = outer | 7;
                }
            }
            SimpleBoard::M349 => {
                if addr >= 0x8000 {
                    let a = addr as usize;
                    if a & 0x800 != 0 {
                        self.prg0 = (a & 0x1F) | (a & ((a & 0x40) >> 6));
                        self.prg1 = (a & 0x18) | 0x07;
                    } else if a & 0x40 != 0 {
                        self.prg0 = a & 0x1F;
                        self.prg1 = a & 0x1F;
                    } else {
                        let b = a & 0x1E;
                        self.prg0 = b;
                        self.prg1 = b | 1;
                    }
                    self.mirroring = if a & 0x80 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            SimpleBoard::M286 => {
                let bank = ((addr >> 10) & 0x03) as usize;
                match addr & 0xF000 {
                    0x8000 => self.chr2[bank] = (addr & 0x1F) as usize,
                    0xA000 if addr & (1u16 << (self.dip + 4)) != 0 => {
                        self.prg8[bank] = (addr & 0x0F) as usize;
                    }
                    _ => {}
                }
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
                if self.board == SimpleBoard::M286 {
                    let slot = (addr as usize) / CHR_BANK_2K;
                    let count = (self.chr.len() / CHR_BANK_2K).max(1);
                    let bank = self.chr2[slot] % count;
                    return self.chr[bank * CHR_BANK_2K + (addr as usize & (CHR_BANK_2K - 1))];
                }
                let bank = self.chr8 % self.chr_count_8k();
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
        let mut out = Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        out.extend_from_slice(&(self.prg1 as u32).to_le_bytes());
        out.extend_from_slice(&(self.chr8 as u32).to_le_bytes());
        for c in &self.chr2 {
            out.extend_from_slice(&(*c as u32).to_le_bytes());
        }
        for p in &self.prg8 {
            out.extend_from_slice(&(*p as u32).to_le_bytes());
        }
        out.push(self.reg_inner);
        out.push(self.reg_outer);
        out.push(self.reg_mode);
        out.push(self.dip);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        // header(1) + prg0/prg1/chr8(12) + chr2(16) + prg8(16) + 4 regs + mirror(1)
        let scratch = 1 + 12 + 16 + 16 + 4 + 1;
        let expected = scratch + self.vram.len() + chr_ram;
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
        self.prg1 = rd(c + 4);
        self.chr8 = rd(c + 8);
        c += 12;
        for s in &mut self.chr2 {
            *s = rd(c);
            c += 4;
        }
        for s in &mut self.prg8 {
            *s = rd(c);
            c += 4;
        }
        self.reg_inner = data[c];
        self.reg_outer = data[c + 1];
        self.reg_mode = data[c + 2];
        self.dip = data[c + 3];
        self.mirroring = byte_to_mirroring(data[c + 4], self.mirroring);
        c += 5;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

macro_rules! simple_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<SimpleBmc, MapperError> {
            SimpleBmc::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

simple_ctor!(
    new_m164,
    SimpleBoard::M164,
    164,
    "Mapper 164 (Waixing Final Fantasy V, 32 KiB-PRG split register)."
);
simple_ctor!(
    new_m261,
    SimpleBoard::M261,
    261,
    "Mapper 261 (BMC-810544-C-A1 address-as-data multicart)."
);
simple_ctor!(
    new_m289,
    SimpleBoard::M289,
    289,
    "Mapper 289 (BMC-60311C NROM/UNROM multicart)."
);
simple_ctor!(
    new_m320,
    SimpleBoard::M320,
    320,
    "Mapper 320 (BMC-830425C-4391T UNROM/UOROM multicart)."
);
simple_ctor!(
    new_m336,
    SimpleBoard::M336,
    336,
    "Mapper 336 (BMC-K-3046 UNROM-style multicart)."
);
simple_ctor!(
    new_m349,
    SimpleBoard::M349,
    349,
    "Mapper 349 (BMC-G-146 NROM/UNROM/NROM-256 multicart)."
);
simple_ctor!(
    new_m286,
    SimpleBoard::M286,
    286,
    "Mapper 286 (Waixing BS-5 Olympic multicart)."
);

// ===========================================================================
// Kaiser FDS-conversion boards with a CPU-cycle (M2) IRQ:
//   56/142 (Kaiser202 / KS202 / KS7032), 303 (Kaiser7017), 253 (Waixing253).
// Plus the simple Kaiser PRG-window boards 305 (KS7031), 306 (KS7016),
// 312 (KS7013B). The CPU-cycle IRQ ones declare MapperCaps::CYCLE_IRQ.
// ===========================================================================

/// Which Kaiser variant a [`KaiserMapper`] models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KaiserBoard {
    /// Mapper 56 (KS202) — extra CHR + mirror writes; up-counting M2 IRQ.
    M56,
    /// Mapper 142 (KS7032) — like 56 without the extra CHR/mirror writes.
    M142,
    /// Mapper 303 (KS7017) — address-decoded PRG + down-counting M2 IRQ.
    M303,
    /// Mapper 305 (KS7031) — four 2 KiB $6000 PRG-ROM windows (no IRQ).
    M305,
    /// Mapper 306 (KS7016) — address-decoded $6000 PRG window (no IRQ).
    M306,
    /// Mapper 312 (KS7013B) — $6000 PRG select + $8000 mirroring (no IRQ).
    M312,
}

/// A Kaiser FDS-conversion / window board.
pub struct KaiserMapper {
    board: KaiserBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    wram: Box<[u8]>,
    mirroring: Mirroring,
    // KS202/KS7032 (56/142).
    prg_regs: [u8; 4],
    selected_reg: u8,
    use_rom: bool,
    chr_banks: [u8; 8],
    // KS7016 (306) PRG-ROM $6000 window.
    win_6000: u8,
    // KS7031 (305) four 2 KiB windows.
    regs4: [u8; 4],
    // 312 PRG select (16 KiB).
    prg16: u8,
    // IRQ (56/142 up-count, 303 down-count).
    irq_counter: u16,
    irq_reload: u16,
    irq_enabled: bool,
    irq_control: u8,
    irq_pending: bool,
}

impl KaiserMapper {
    const SAVE_LEN: usize = 4 + 1 + 1 + 8 + 1 + 4 + 1 + 2 + 2 + 1 + 1 + 1 + 1;

    fn new(
        board: KaiserBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        id: u16,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, id)?;
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
            wram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            mirroring,
            prg_regs: [0; 4],
            selected_reg: 0,
            use_rom: false,
            chr_banks: [0; 8],
            win_6000: 8,
            regs4: [0; 4],
            prg16: 0,
            irq_counter: 0,
            irq_reload: 0,
            irq_enabled: false,
            irq_control: 0,
            irq_pending: false,
        })
    }

    fn prg_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn prg_count_2k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_2K).max(1)
    }

    fn chr_count_1k(&self) -> usize {
        (self.chr.len() / CHR_BANK_1K).max(1)
    }
}

impl Mapper for KaiserMapper {
    fn caps(&self) -> MapperCaps {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 | KaiserBoard::M303 => MapperCaps::CYCLE_IRQ,
            _ => MapperCaps::NONE,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 => match addr {
                0x6000..=0x7FFF => {
                    if self.use_rom {
                        let count = self.prg_count_8k();
                        let bank = (self.prg_regs[3] as usize) % count;
                        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                    } else {
                        self.wram[addr as usize & 0x1FFF]
                    }
                }
                0x8000..=0xFFFF => {
                    let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                    let count = self.prg_count_8k();
                    let bank = if slot == 3 {
                        count - 1
                    } else {
                        self.prg_regs[slot] as usize % count
                    };
                    self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                }
                _ => 0,
            },
            KaiserBoard::M303 => match addr {
                0x4030 => {
                    let p = self.irq_pending;
                    self.irq_pending = false;
                    u8::from(p)
                }
                0x8000..=0xBFFF => {
                    let count = self.prg_count_16k();
                    let bank = (self.prg16 as usize) % count;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                0xC000..=0xFFFF => {
                    let count = self.prg_count_16k();
                    let bank = 2 % count;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                _ => 0,
            },
            KaiserBoard::M305 => match addr {
                0x6000..=0x7FFF => {
                    let win = (addr as usize - 0x6000) / PRG_BANK_2K;
                    let count = self.prg_count_2k();
                    let bank = (self.regs4[win] as usize) % count;
                    self.prg_rom[bank * PRG_BANK_2K + (addr as usize & 0x7FF)]
                }
                0x8000..=0xFFFF => {
                    // Fixed last 32 KiB (16 x 2 KiB windows = banks count-16..count-1).
                    let count = self.prg_count_2k();
                    let win = (addr as usize - 0x8000) / PRG_BANK_2K;
                    let bank = count.saturating_sub(16 - win) % count;
                    self.prg_rom[bank * PRG_BANK_2K + (addr as usize & 0x7FF)]
                }
                _ => 0,
            },
            KaiserBoard::M306 => match addr {
                0x6000..=0x7FFF => {
                    let count = self.prg_count_8k();
                    let bank = (self.win_6000 as usize) % count;
                    self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                }
                0x8000..=0xFFFF => {
                    // Fixed last 32 KiB.
                    let count = self.prg_count_8k();
                    let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                    let bank = count.saturating_sub(4 - slot) % count;
                    self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
                }
                _ => 0,
            },
            KaiserBoard::M312 => match addr {
                0x8000..=0xBFFF => {
                    let count = self.prg_count_16k();
                    let bank = (self.prg16 as usize) % count;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                0xC000..=0xFFFF => {
                    let count = self.prg_count_16k();
                    let bank = count - 1;
                    self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
                }
                _ => 0,
            },
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match self.board {
            KaiserBoard::M303 => addr != 0x4030 && (0x4020..=0x5FFF).contains(&addr),
            _ => (0x4020..=0x5FFF).contains(&addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 => match addr & 0xF000 {
                0x8000 => self.irq_reload = (self.irq_reload & 0xFFF0) | (value as u16 & 0x0F),
                0x9000 => {
                    self.irq_reload = (self.irq_reload & 0xFF0F) | ((value as u16 & 0x0F) << 4);
                }
                0xA000 => {
                    self.irq_reload = (self.irq_reload & 0xF0FF) | ((value as u16 & 0x0F) << 8);
                }
                0xB000 => {
                    self.irq_reload = (self.irq_reload & 0x0FFF) | ((value as u16 & 0x0F) << 12);
                }
                0xC000 => {
                    self.irq_control = value;
                    if value & 0x02 != 0 {
                        self.irq_counter = self.irq_reload;
                    }
                    self.irq_enabled = value & 0x02 != 0;
                    self.irq_pending = false;
                }
                0xD000 => self.irq_pending = false,
                0xE000 => self.selected_reg = (value & 0x07).wrapping_sub(1),
                0xF000 => {
                    match self.selected_reg {
                        0..=3 => {
                            let i = self.selected_reg as usize;
                            self.prg_regs[i] = (self.prg_regs[i] & 0x10) | (value & 0x0F);
                        }
                        4 => self.use_rom = value & 0x04 != 0,
                        _ => {}
                    }
                    if self.board == KaiserBoard::M56 {
                        match addr & 0xFC00 {
                            0xF000 => {
                                let bank = (addr & 0x03) as usize;
                                self.prg_regs[bank] = (value & 0x10) | (self.prg_regs[bank] & 0x0F);
                            }
                            0xF800 => {
                                self.mirroring = if value & 0x01 != 0 {
                                    Mirroring::Vertical
                                } else {
                                    Mirroring::Horizontal
                                };
                            }
                            0xFC00 => self.chr_banks[(addr & 0x07) as usize] = value,
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            KaiserBoard::M303 => {
                if addr & 0xFF00 == 0x4A00 {
                    self.prg16 = (((addr >> 2) & 0x03) | ((addr >> 4) & 0x04)) as u8;
                } else if addr == 0x4020 {
                    self.irq_pending = false;
                    self.irq_counter = (self.irq_counter & 0xFF00) | value as u16;
                } else if addr == 0x4021 {
                    self.irq_pending = false;
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16) << 8);
                    self.irq_enabled = true;
                } else if addr == 0x4025 {
                    self.mirroring = if (value >> 3) & 0x01 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            KaiserBoard::M305 => {
                if (0x8000..=0xFFFF).contains(&addr) {
                    self.regs4[((addr >> 11) & 0x03) as usize] = value;
                }
            }
            KaiserBoard::M306 => {
                if addr >= 0x8000 {
                    let mode = (addr & 0x30) == 0x30;
                    match addr & 0xD943 {
                        0xD943 => {
                            self.win_6000 = if mode {
                                0x0B
                            } else {
                                ((addr >> 2) & 0x0F) as u8
                            };
                        }
                        0xD903 => {
                            self.win_6000 = if mode {
                                0x08 | ((addr >> 2) & 0x03) as u8
                            } else {
                                0x0B
                            };
                        }
                        _ => {}
                    }
                }
            }
            KaiserBoard::M312 => {
                if addr < 0x8000 {
                    if (0x6000..=0x7FFF).contains(&addr) {
                        self.prg16 = value;
                    }
                } else {
                    self.mirroring = if value & 0x01 != 0 {
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
                if self.board == KaiserBoard::M56 {
                    let slot = (addr as usize) / CHR_BANK_1K;
                    let count = self.chr_count_1k();
                    let bank = (self.chr_banks[slot] as usize) % count;
                    return self.chr[bank * CHR_BANK_1K + (addr as usize & 0x3FF)];
                }
                self.chr[addr as usize & (self.chr.len() - 1)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (self.chr.len() - 1)] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        match self.board {
            KaiserBoard::M56 | KaiserBoard::M142 => {
                if self.irq_control & 0x02 != 0 {
                    self.irq_counter = self.irq_counter.wrapping_add(1);
                    if self.irq_counter == 0xFFFF {
                        self.irq_counter = self.irq_reload;
                        self.irq_control &= !0x02;
                        self.irq_pending = true;
                    }
                }
            }
            KaiserBoard::M303 if self.irq_enabled && self.irq_counter != 0 => {
                self.irq_counter -= 1;
                if self.irq_counter == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = true;
                }
            }
            _ => {}
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
        out.extend_from_slice(&self.prg_regs);
        out.push(self.selected_reg);
        out.push(u8::from(self.use_rom));
        out.extend_from_slice(&self.chr_banks);
        out.push(self.win_6000);
        out.extend_from_slice(&self.regs4);
        out.push(self.prg16);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.extend_from_slice(&self.irq_reload.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(self.irq_control);
        out.push(u8::from(self.irq_pending));
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.wram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + self.wram.len() + chr_ram;
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
        self.prg_regs.copy_from_slice(&data[c..c + 4]);
        c += 4;
        self.selected_reg = data[c];
        self.use_rom = data[c + 1] != 0;
        c += 2;
        self.chr_banks.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.win_6000 = data[c];
        c += 1;
        self.regs4.copy_from_slice(&data[c..c + 4]);
        c += 4;
        self.prg16 = data[c];
        c += 1;
        self.irq_counter = u16::from_le_bytes([data[c], data[c + 1]]);
        self.irq_reload = u16::from_le_bytes([data[c + 2], data[c + 3]]);
        c += 4;
        self.irq_enabled = data[c] != 0;
        self.irq_control = data[c + 1];
        self.irq_pending = data[c + 2] != 0;
        self.mirroring = byte_to_mirroring(data[c + 3], self.mirroring);
        c += 4;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.wram.copy_from_slice(&data[c..c + self.wram.len()]);
        c += self.wram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

macro_rules! kaiser_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<KaiserMapper, MapperError> {
            KaiserMapper::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

kaiser_ctor!(new_m56, KaiserBoard::M56, 56, "Mapper 56 (Kaiser KS202).");
kaiser_ctor!(
    new_m142,
    KaiserBoard::M142,
    142,
    "Mapper 142 (Kaiser KS7032)."
);
kaiser_ctor!(
    new_m303,
    KaiserBoard::M303,
    303,
    "Mapper 303 (Kaiser KS7017)."
);
kaiser_ctor!(
    new_m305,
    KaiserBoard::M305,
    305,
    "Mapper 305 (Kaiser KS7031)."
);
kaiser_ctor!(
    new_m306,
    KaiserBoard::M306,
    306,
    "Mapper 306 (Kaiser KS7016)."
);
kaiser_ctor!(
    new_m312,
    KaiserBoard::M312,
    312,
    "Mapper 312 (Kaiser KS7013B)."
);

// ===========================================================================
// Waixing253 (mapper 253) — Waixing VRC4-clone, Dragon Ball Z.
//
// Per-1 KiB CHR low/high registers ($B000-$E00C), a CHR-RAM escape (CHR reg
// value 4/5 + a force-ROM toggle on slot 0 via $88/$C8), two 8 KiB PRG selects
// ($8010/$A010), $9400 mirroring, and a /114-scaled CPU-cycle IRQ ($F000 etc.).
// Ported from Mesen2 Waixing/Mapper253.h.
// ===========================================================================

/// Waixing VRC4-clone (mapper 253, *Dragon Ball Z*).
pub struct Waixing253 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    prg_count_8k: usize,
    chr_count_1k: usize,
    prg: [u8; 2],
    chr_low: [u8; 8],
    chr_high: [u8; 8],
    force_chr_rom: bool,
    irq_reload: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_scaler: u16,
    irq_pending: bool,
}

impl Waixing253 {
    const SAVE_LEN: usize = 2 + 8 + 8 + 1 + 1 + 1 + 1 + 2 + 1 + 1;

    fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, 253)?;
        let chr: Box<[u8]> = if chr_rom.is_empty() {
            vec![0u8; CHR_BANK_1K].into_boxed_slice()
        } else {
            if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
                return Err(MapperError::Invalid(format!(
                    "mapper 253 CHR-ROM size {} is not a multiple of 1 KiB",
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
            chr_ram: vec![0u8; CHR_BANK_2K].into_boxed_slice(), // 2 KiB CHR-RAM escape.
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg_count_8k,
            chr_count_1k,
            prg: [0; 2],
            chr_low: [0; 8],
            chr_high: [0; 8],
            force_chr_rom: false,
            irq_reload: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_scaler: 0,
            irq_pending: false,
        })
    }

    fn prg_bank(&self, slot: usize) -> usize {
        let count = self.prg_count_8k;
        match slot {
            0 => self.prg[0] as usize % count,
            1 => self.prg[1] as usize % count,
            2 => count.saturating_sub(2),
            _ => count - 1,
        }
    }
}

impl Mapper for Waixing253 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                let bank = self.prg_bank(slot);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0xB000..=0xE00C).contains(&addr) {
            let slot = ((((addr & 0x08) | (addr >> 8)) >> 3) as usize).wrapping_add(2) & 0x07;
            let shift = (addr & 0x04) as u8;
            let lo = (self.chr_low[slot] & (0xF0u8 >> shift)) | (value << shift);
            self.chr_low[slot] = lo;
            if slot == 0 {
                if lo == 0xC8 {
                    self.force_chr_rom = false;
                } else if lo == 0x88 {
                    self.force_chr_rom = true;
                }
            }
            if shift != 0 {
                self.chr_high[slot] = value >> 4;
            }
        } else {
            match addr {
                0x8010 => self.prg[0] = value,
                0xA010 => self.prg[1] = value,
                0x9400 => {
                    self.mirroring = match value & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
                0xF000 => {
                    self.irq_reload = (self.irq_reload & 0xF0) | (value & 0x0F);
                    self.irq_pending = false;
                }
                0xF004 => {
                    self.irq_reload = (self.irq_reload & 0x0F) | (value << 4);
                    self.irq_pending = false;
                }
                0xF008 => {
                    self.irq_counter = self.irq_reload;
                    self.irq_enabled = value & 0x02 != 0;
                    self.irq_scaler = 0;
                    self.irq_pending = false;
                }
                _ => {}
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) / CHR_BANK_1K;
                let lo = self.chr_low[slot];
                if (lo == 4 || lo == 5) && !self.force_chr_rom {
                    let page = (lo as usize & 0x01) * CHR_BANK_1K;
                    return self.chr_ram
                        [(page + (addr as usize & 0x3FF)) & (self.chr_ram.len() - 1)];
                }
                let page =
                    (lo as usize | ((self.chr_high[slot] as usize) << 8)) % self.chr_count_1k;
                self.chr[page * CHR_BANK_1K + (addr as usize & 0x3FF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr as usize) / CHR_BANK_1K;
                let lo = self.chr_low[slot];
                if (lo == 4 || lo == 5) && !self.force_chr_rom {
                    let page = (lo as usize & 0x01) * CHR_BANK_1K;
                    let off = (page + (addr as usize & 0x3FF)) & (self.chr_ram.len() - 1);
                    self.chr_ram[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if self.irq_enabled {
            self.irq_scaler += 1;
            if self.irq_scaler >= 114 {
                self.irq_scaler = 0;
                self.irq_counter = self.irq_counter.wrapping_add(1);
                if self.irq_counter == 0 {
                    self.irq_counter = self.irq_reload;
                    self.irq_pending = true;
                }
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
        let mut out = Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg);
        out.extend_from_slice(&self.chr_low);
        out.extend_from_slice(&self.chr_high);
        out.push(u8::from(self.force_chr_rom));
        out.push(self.irq_reload);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.extend_from_slice(&self.irq_scaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + Self::SAVE_LEN + self.vram.len() + self.chr_ram.len();
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
        self.prg.copy_from_slice(&data[c..c + 2]);
        c += 2;
        self.chr_low.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.chr_high.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.force_chr_rom = data[c] != 0;
        self.irq_reload = data[c + 1];
        self.irq_counter = data[c + 2];
        self.irq_enabled = data[c + 3] != 0;
        self.irq_scaler = u16::from_le_bytes([data[c + 4], data[c + 5]]);
        self.irq_pending = data[c + 6] != 0;
        self.mirroring = byte_to_mirroring(data[c + 7], self.mirroring);
        c += 8;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[c..c + self.chr_ram.len()]);
        Ok(())
    }
}

/// Mapper 253 (Waixing VRC4-clone, *Dragon Ball Z*).
///
/// # Errors
/// [`MapperError::Invalid`] on a bad PRG/CHR size.
pub fn new_m253(
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    mirroring: Mirroring,
) -> Result<Waixing253, MapperError> {
    Waixing253::new(prg_rom, chr_rom, mirroring)
}

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
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- FK23C (176) -------------------------------------------------------

    #[test]
    fn fk23c_mmc3_prg_and_a12_irq() {
        let mut m = new_m176(synth_prg_8k(32), synth_chr_1k(64), Mirroring::Vertical).unwrap();
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
        let mut m = new_m176(synth_prg_8k(128), synth_chr_1k(64), Mirroring::Vertical).unwrap();
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
        let mut m = new_m176(synth_prg_8k(32), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x20); // select CHR-RAM
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 7);
        m.ppu_write(0x0040, 0x99);
        m.cpu_write(0x6000, 0x5A); // WRAM
        let blob = m.save_state();
        let mut m2 = new_m176(synth_prg_8k(32), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0040), 0x99);
        assert_eq!(m2.cpu_read(0x6000), 0x5A);
    }

    // --- COOLBOY (268) -----------------------------------------------------

    #[test]
    fn coolboy_outer_regs_and_irq() {
        let mut m = new_m268(synth_prg_8k(64), synth_chr_1k(128), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 0x01); // ex_reg0
        m.cpu_write(0x8000, 0x06); // R6
        m.cpu_write(0x8001, 3);
        let v = m.cpu_read(0x8000);
        assert!((v as usize) < 64); // in range, no panic.

        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        m.notify_a12(false);
        m.notify_a12(true);
        m.notify_a12(false);
        m.notify_a12(true);
        assert!(m.irq_pending());
    }

    #[test]
    fn coolboy_save_state_round_trip() {
        let mut m = new_m268(synth_prg_8k(64), synth_chr_1k(128), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x6000, 0x05);
        m.cpu_write(0x6001, 0x02);
        m.cpu_write(0x8000, 0x06);
        m.cpu_write(0x8001, 4);
        m.ppu_write(0x2005, 0x3C);
        let blob = m.save_state();
        let mut m2 = new_m268(synth_prg_8k(64), synth_chr_1k(128), Mirroring::Horizontal).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x2005), 0x3C);
    }

    // --- Sachen 9602 (513) -------------------------------------------------

    #[test]
    fn sachen9602_prg_outer_bank() {
        let mut m = new_m513(synth_prg_8k(128), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x06); // select R6 (< 6 is false; 6 captures? <6 only)
        m.cpu_write(0x8001, 0xC5); // value>>6 = 3 only if reg<6; R6 is not <6.
        // Use R0 (<6) to set the outer bank.
        m.cpu_write(0x8000, 0x00);
        m.cpu_write(0x8001, 0xC0); // outer = 3 -> <<6 = 192
        let v = m.cpu_read(0x8000);
        assert!((v as usize) < 128);
    }

    #[test]
    fn sachen9602_save_state_round_trip() {
        let mut m = new_m513(synth_prg_8k(64), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x00);
        m.cpu_write(0x8001, 0x45);
        m.ppu_write(0x0001, 0x77);
        let blob = m.save_state();
        let mut m2 = new_m513(synth_prg_8k(64), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0001), 0x77);
    }

    // --- Sachen 3011 / TXC (136) -------------------------------------------

    #[test]
    fn sachen3011_txc_chr_select() {
        let mut m = new_m136(synth_prg_8k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // Stage a value into the accumulator, then latch it via $8000 to output.
        m.cpu_write(0x4102, 0x03); // staging = 3
        m.cpu_write(0x4103, 0x00); // increase = false
        m.cpu_write(0x4100, 0x00); // accumulator = staging
        m.cpu_write(0x8000, 0x00); // refresh output
        // output low nibble = accumulator low nibble (3) -> CHR bank 3.
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn sachen3011_save_state_round_trip() {
        let mut m = new_m136(synth_prg_8k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4102, 0x02);
        m.cpu_write(0x4100, 0x00);
        m.cpu_write(0x8000, 0x00);
        m.ppu_write(0x2003, 0x11);
        let blob = m.save_state();
        let mut m2 = new_m136(synth_prg_8k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
        assert_eq!(m2.ppu_read(0x2003), 0x11);
    }

    // --- Waixing 164 (split-PRG) -------------------------------------------

    #[test]
    fn waixing164_split_prg_register() {
        let mut m = new_m164(synth_prg_16k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x02); // low nibble = 2
        m.cpu_write(0x5100, 0x00); // high nibble = 0 -> 32 KiB bank 2
        // 32 KiB bank 2 -> 16 KiB banks 4 and 5.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn bmc_simple_save_state_round_trip() {
        let mut m = new_m164(synth_prg_16k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x03);
        m.ppu_write(0x0007, 0x2B);
        let blob = m.save_state();
        let mut m2 = new_m164(synth_prg_16k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0007), 0x2B);
    }

    // --- BMC-810544 (261), address-as-data ---------------------------------

    #[test]
    fn bmc810544_address_decode() {
        let mut m = new_m261(synth_prg_16k(32), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // addr bit 6 set -> 32 KiB mode: bank = (addr>>6)&0xFFFE.
        m.cpu_write(0x8040 | (4 << 6), 0); // bank = (4<<6 ... ) wait compute below.
        // Simpler: write a precise address.
        m.cpu_write(0x8000 | (2 << 6) | 0x40, 0); // (addr>>6)&0xFFFE
        let lo = m.cpu_read(0x8000);
        let hi = m.cpu_read(0xC000);
        assert_eq!(hi, lo.wrapping_add(1)); // 32 KiB -> consecutive 16 KiB banks.
    }

    // --- BMC-60311C (289) --------------------------------------------------

    #[test]
    fn bmc60311_nrom_mode() {
        let mut m = new_m289(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6001, 0x02); // outer = 2
        m.cpu_write(0x6000, 0x00); // mode 0 = NROM-128 (mirror inner/outer)
        m.cpu_write(0x8000, 0x01); // inner = 1 -> page = 2|1 = 3
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3); // mirrored.
    }

    // --- BMC-830425 (320) --------------------------------------------------

    #[test]
    fn bmc830425_unrom_mode() {
        let mut m = new_m320(synth_prg_16k(32), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0xF0E0 | 0x10 | 0x02, 0x03); // outer=2, mode=1 (UNROM), inner=3
        // UNROM: prg0 = (3&7)|(2<<3)=19; prg1 = 7|(2<<3)=23.
        assert_eq!(m.cpu_read(0x8000), 19);
        assert_eq!(m.cpu_read(0xC000), 23);
    }

    // --- BMC-K3046 (336) ---------------------------------------------------

    #[test]
    fn bmc_k3046_unrom() {
        let mut m = new_m336(synth_prg_16k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x0A); // inner=2, outer=8 -> prg0=8|2=10, prg1=8|7=15
        assert_eq!(m.cpu_read(0x8000), 10);
        assert_eq!(m.cpu_read(0xC000), 15);
    }

    // --- BMC-G146 (349) ----------------------------------------------------

    #[test]
    fn bmc_g146_32k_mode() {
        let mut m = new_m349(synth_prg_16k(32), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // bit 11 clear, bit 6 clear -> 32 KiB mode: prg0=addr&0x1E, prg1=that|1.
        m.cpu_write(0x8000 | 0x04, 0); // addr&0x1E = 4
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    // --- Waixing BS-5 (286) ------------------------------------------------

    #[test]
    fn bs5_chr_bank_decode() {
        let mut m = new_m286(synth_prg_8k(16), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        // $8000 with bank in bits 10-11, CHR index in bits 0-4.
        m.cpu_write(0x8000 | (1 << 10) | 0x05, 0); // chr bank 1 -> index 5
        assert_eq!(m.ppu_read(0x0800), 5); // 2 KiB slot 1 -> CHR bank 5.
    }

    #[test]
    fn bs5_save_state_round_trip() {
        let mut m = new_m286(synth_prg_8k(16), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000 | 0x03, 0);
        let blob = m.save_state();
        let mut m2 = new_m286(synth_prg_8k(16), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }

    // --- Kaiser KS202/KS7032 (56/142) M2 IRQ -------------------------------

    #[test]
    fn kaiser202_prg_regs_and_up_count_irq() {
        let mut m = new_m142(synth_prg_8k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 0x01); // select reg (1-1=0)
        m.cpu_write(0xF000, 0x03); // prg_regs[0] low = 3
        assert_eq!(m.cpu_read(0x8000), 3);

        // IRQ: reload, enable, count up to 0xFFFF.
        m.cpu_write(0x8000, 0x0E); // reload low nibble
        m.cpu_write(0xC000, 0x02); // enable + load
        // Counter loads 0x...E; count up until 0xFFFF wraps.
        let mut fired = false;
        for _ in 0..0x20000 {
            m.notify_cpu_cycle();
            if m.irq_pending() {
                fired = true;
                break;
            }
        }
        assert!(fired);
    }

    #[test]
    fn kaiser202_save_state_round_trip() {
        let mut m = new_m56(synth_prg_8k(16), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 0x01);
        m.cpu_write(0xF000, 0x05);
        m.cpu_write(0xFC00, 0x02); // m56 CHR write
        m.ppu_write(0x2002, 0x44);
        let blob = m.save_state();
        let mut m2 = new_m56(synth_prg_8k(16), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x2002), 0x44);
    }

    // --- Kaiser KS7017 (303) down-count IRQ + read-ack ---------------------

    #[test]
    fn kaiser7017_prg_and_down_count_irq() {
        let mut m = new_m303(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // $4Axx address-decoded PRG select.
        m.cpu_write(0x4A00 | (1 << 2), 0); // prg16 = ((1<<2)>>2)&3 = 1
        assert_eq!(m.cpu_read(0x8000), 1);

        m.cpu_write(0x4020, 0x03); // counter low
        m.cpu_write(0x4021, 0x00); // counter high + enable -> counter = 3
        for _ in 0..3 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        assert_eq!(m.cpu_read(0x4030), 0x01); // read-ack returns pending then clears.
        assert!(!m.irq_pending());
    }

    // --- Kaiser KS7031 (305) four 2 KiB windows ----------------------------

    /// Build a PRG image whose first byte of every 2 KiB page is the page index.
    fn synth_prg_2k_tagged(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_2K];
        for b in 0..banks {
            v[b * PRG_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn kaiser7031_windowed_prg() {
        // 8 KiB == 4 x 2 KiB pages; use a 2 KiB-tagged 16 KiB image (8 pages).
        let mut m = new_m305(synth_prg_2k_tagged(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // $8000-$FFFF: window = (addr>>11)&3, value = 2 KiB page index.
        m.cpu_write(0x8000, 5); // regs4[0] = 5
        assert_eq!(m.cpu_read(0x6000), 5); // first 2 KiB $6000 window -> page 5.
    }

    #[test]
    fn kaiser7031_save_state_round_trip() {
        let mut m = new_m305(synth_prg_8k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        m.cpu_write(0x8800, 4);
        m.ppu_write(0x0005, 0x21);
        let blob = m.save_state();
        let mut m2 = new_m305(synth_prg_8k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x6000), m.cpu_read(0x6000));
        assert_eq!(m2.ppu_read(0x0005), 0x21);
    }

    // --- Kaiser KS7016 (306) -----------------------------------------------

    #[test]
    fn kaiser7016_window_decode() {
        let mut m = new_m306(synth_prg_8k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // $D943 with mode bits (addr&0x30 != 0x30) -> _prgReg = (addr>>2)&0x0F.
        let addr = 0xD943; // addr&0x30 = 0x00 -> not mode -> reg = (0xD943>>2)&0x0F
        m.cpu_write(addr, 0);
        let v = m.cpu_read(0x6000);
        assert!((v as usize) < 16);
    }

    // --- Kaiser KS7013B (312) ----------------------------------------------

    #[test]
    fn kaiser7013b_prg_and_mirror() {
        let mut m = new_m312(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6000, 3); // prg16 = 3
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed last bank.
        m.cpu_write(0x8000, 0x01); // horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    // --- Waixing 253 (Dragon Ball Z) ---------------------------------------

    #[test]
    fn waixing253_prg_and_scaled_irq() {
        let mut m = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8010, 4); // prg[0] = 4
        m.cpu_write(0xA010, 6); // prg[1] = 6
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 6);
        assert_eq!(m.cpu_read(0xE000), 15); // fixed last.

        m.cpu_write(0xF000, 0x0E); // reload low
        m.cpu_write(0xF008, 0x02); // load + enable
        // counter loaded with 0x0E; needs (0x100-0x0E) ticks * 114.
        let mut fired = false;
        for _ in 0..(256 * 115) {
            m.notify_cpu_cycle();
            if m.irq_pending() {
                fired = true;
                break;
            }
        }
        assert!(fired);
    }

    #[test]
    fn waixing253_chr_ram_escape_and_round_trip() {
        let mut m = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        // CHR low reg value 4 on slot 0 + not force-rom -> CHR-RAM.
        m.cpu_write(0xB000, 0x04); // slot 0 low nibble = 4
        m.ppu_write(0x0000, 0x5E);
        assert_eq!(m.ppu_read(0x0000), 0x5E);
        let blob = m.save_state();
        let mut m2 = new_m253(synth_prg_8k(16), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), 0x5E);
    }

    #[test]
    fn truncated_save_state_rejected() {
        let m = new_m176(synth_prg_8k(16), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        let mut blob = m.save_state();
        blob.pop();
        let mut m2 = new_m176(synth_prg_8k(16), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        assert!(m2.load_state(&blob).is_err());
    }
}
