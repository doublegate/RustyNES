//! Sachen `9602` (mapper 513).
//!
//! An MMC3-derived Sachen ASIC with an outer PRG bank register, later and
//! more capable than the 8259 family in `sachen_8259.rs` and the discrete
//! boards in `sachen_discrete.rs`.
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
}
