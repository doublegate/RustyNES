//! Sachen `3011` (mapper 136).
//!
//! Drives its CHR select through a TXC-style accumulator chip -- the same
//! two-stage arrangement modelled in `txc.rs`, where a value is assembled in
//! the `$4100-$4103` window and only latched into the banking registers on a
//! subsequent write. The chip state is duplicated here rather than shared
//! because the two boards clock it differently.
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
                // Wrap against the ACTUAL image length, not a rounded-up bank
                // count. `check_prg` admits any non-zero multiple of 8 KiB, so a
                // 16 KiB PRG yields `count == 1` and a modulus of 32768 — and
                // `$C000` then resolves to offset 16384, one past the end of a
                // 16 KiB slice. `% len()` is what actually upholds this crate's
                // "a register write can never index out of bounds" invariant on
                // ROM-parsed (untrusted) sizes. `check_prg` guarantees non-empty,
                // so the modulus cannot divide by zero.
                self.prg_rom[(addr as usize & 0x7FFF) % self.prg_rom.len()]
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

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

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
}
