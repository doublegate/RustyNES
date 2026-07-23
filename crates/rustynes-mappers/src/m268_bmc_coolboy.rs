//! `COOLBOY` / `MINDKIDS` (mapper 268).
//!
//! Another MMC3-core-plus-outer-registers pirate ASIC, closely related to the
//! `FK23C` in `m176_bmc_fk23c.rs` but with its outer registers in the
//! `$6000-$7FFF` PRG-RAM window and a different mode encoding. The two are
//! kept separate rather than merged because their outer decode is where all
//! the per-board behaviour lives, and conflating them would obscure exactly
//! the part that differs.
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
}
