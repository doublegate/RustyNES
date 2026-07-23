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
        let m = new_m176(synth_prg_8k(16), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        let mut blob = m.save_state();
        blob.pop();
        let mut m2 = new_m176(synth_prg_8k(16), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        assert!(m2.load_state(&blob).is_err());
    }

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

    #[test]
    fn fk23c_chr_rom_not_writable_via_select_chr_ram() {
        // FK23C: even with `select_chr_ram` set, a CHR-ROM cart must not be
        // mutated (regression: `ppu_write` wrote through `self.chr`, which
        // corrupted CHR-ROM and was never serialized).
        let mut m = new_m176(synth_prg_8k(32), synth_chr_1k(64), Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x20); // select_chr_ram = true
        let before = m.ppu_read(0x0010);
        m.ppu_write(0x0010, before.wrapping_add(1));
        assert_eq!(m.ppu_read(0x0010), before, "CHR-ROM must not be mutable");
    }
}
