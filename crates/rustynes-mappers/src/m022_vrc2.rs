//! Konami VRC2 (mappers 22, and sub-variants of 23 / 25).
//!
//! The VRC2 and VRC4 share a register map that is *identical in decode* but
//! *rewired at the pins*: each PCB revision ties the two low register-select
//! address lines to a different pair of CPU address pins, so the same write
//! reaches a different register depending on the board. That rewiring is the
//! only real difference between the mapper numbers, and it is isolated in
//! [`vrc_a_bits`] (duplicated in `m021_vrc4.rs`, as the crate duplicates its
//! other small shared helpers rather than coupling board modules).
//!
//! VRC2 exposes a one-byte CHR latch and, unlike VRC4, has **no IRQ counter**
//! and no on-cart audio. Its siblings: `m021_vrc4.rs`, `m073_vrc3.rs`,
//! `m024_vrc6.rs`, `m085_vrc7.rs`, `m075_vrc1.rs`.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    clippy::manual_range_patterns,
    clippy::match_same_arms,
    clippy::struct_excessive_bools,
    clippy::doc_markdown,
    clippy::range_plus_one,
    clippy::single_match_else,
    clippy::bool_to_int_with_if,
    clippy::unnested_or_patterns,
    clippy::single_match,
    clippy::doc_lazy_continuation,
    clippy::too_long_first_doc_paragraph
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Map a VRC2/4 register address to its (a0, a1) register-select pin pair.
///
/// Per the nesdev "VRC2 and VRC4" wiki, the iNES mapper number selects
/// which CPU address lines are wired to the chip's A0/A1 register-select
/// pins.  On real Konami boards the two candidate lines for each pin are
/// physically tied together, so a write to *either* one drives the pin —
/// the hardware ORs them.  Modelling that OR (rather than picking a single
/// bit) is what makes submapper-0 iNES-1.0 ROMs decode correctly: e.g.
/// mapper 23 games write CHR registers at both `$x002/$x003` (A1/A0) and
/// `$x008/$x00C` (A3/A2), and a single-bit decoder collapses the latter
/// set onto register 0.
///
/// Here `a0` is the chip's *high-nibble* select (register address +1) and
/// `a1` is the *next-register* select (register address +2), matching how
/// the callers consume the pair: `slot = a1 ? base+1 : base` and
/// `low = !a0`.  Mapped to CPU address lines per mapper:
///
/// | Mapper | a0 (high) driven by | a1 (reg-sel) driven by |
/// |--------|---------------------|------------------------|
/// | 21     | A1, A6              | A2, A7                 |  (VRC4a/c)
/// | 22     | A1                  | A0                     |  (VRC2a — A0/A1 SWAPPED)
/// | 23     | A0, A2              | A1, A3                 |  (VRC4e/f, VRC2b)
/// | 25     | A1, A3              | A0, A2                 |  (VRC4b/d, VRC2c — swapped)
///
/// VRC2a (mapper 22) and VRC2c (mapper 25) both wire the chip's A0 register
/// pin to CPU A1 and A1 to CPU A0 (the swap); VRC2b (mapper 23) is straight.
/// The v2.4.0 fix swapped 25 but left 22 straight, leaving TwinBee 3's BG
/// tiles scrambled (the sprite slots happened to land right); v2.4.1 swaps 22.
///
/// Verified against the per-game register-write traces (Crisis Force /
/// Akumajou = mapper 23 use offsets $0/$4/$8/$C; Wai Wai World 2 = mapper
/// 21 use $0/$2/$4/$6; TwinBee 3 = mapper 22 and Goemon Gaiden = mapper 25
/// use $0/$1/$2/$3).  NES 2.0 submappers, when present, pin a single line;
/// OR-ing the candidate lines is a superset that decodes those correctly
/// because a given ROM only toggles one of the board-tied lines.
fn vrc_a_bits(mapper_id: u16, _submapper: u8, addr: u16) -> (bool, bool) {
    let bit = |n: u16| (addr >> n) & 1 != 0;
    match mapper_id {
        21 => (bit(1) | bit(6), bit(2) | bit(7)),
        22 => (bit(1), bit(0)), // VRC2a: A0/A1 SWAPPED (chip A0<-CPU A1)
        25 => (bit(1) | bit(3), bit(0) | bit(2)), // VRC2c/VRC4b/d: swapped
        // Mapper 23 (and any other VRC2/4 fallback).
        _ => (bit(0) | bit(2), bit(1) | bit(3)),
    }
}

/// VRC2 (Mapper 22 + sub-variants of 23/25).
pub struct Vrc2 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_lo: u8,
    prg_mid: u8,
    chr: [u8; 8],
    mirroring: Mirroring,
    mapper_id: u16,
    submapper: u8,
    /// 8 KiB WRAM at $6000-$7FFF (battery-backed on most Konami carts).
    /// T-60-003b (2026-05-17).
    prg_ram: Box<[u8]>,
}

impl Vrc2 {
    /// Construct a new VRC2 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mapper_id: u16,
        submapper: u8,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC2 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "VRC2 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_lo: 0,
            prg_mid: 1,
            chr: [0; 8],
            mirroring,
            mapper_id,
            submapper,
            // 8 KiB WRAM at $6000-$7FFF (T-60-003b).
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last1 = total_8k - 1;
        let last2 = total_8k.saturating_sub(2);
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_lo as usize) % total_8k,
            0xA000 => (self.prg_mid as usize) % total_8k,
            0xC000 => last2,
            0xE000 => last1,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        // VRC2a (mapper 22) does not connect the low bit of the CHR bank
        // value: the effective 1 KiB bank is `register >> 1`.  Real ROMs
        // rely on this — e.g. TwinBee 3 writes bank $A8 (168) to a slot of
        // a 128 KiB (128-bank) CHR-ROM, which is only in range as $54 (84)
        // after the shift.  CHR-RAM carts (chr_is_ram) address linearly,
        // and only mapper 22 has the dropped-low-bit wiring (mappers 23/25
        // are routed to the Vrc4 type, but guard on the id regardless).
        let raw = if self.mapper_id == 22 && !self.chr_is_ram {
            self.chr[slot] as usize >> 1
        } else {
            self.chr[slot] as usize
        };
        let bank = raw % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn write_chr_reg(&mut self, slot: usize, low: bool, value: u8) {
        let cur = self.chr[slot];
        let v = if low {
            (cur & 0xF0) | (value & 0x0F)
        } else {
            (cur & 0x0F) | ((value & 0x1F) << 4)
        };
        self.chr[slot] = v;
    }
}

impl Mapper for Vrc2 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // T-60-003b (2026-05-17): Konami's VRC2 carts include 8KB
            // battery-backed WRAM at $6000-$7FFF (e.g., Ganbare Goemon 2
            // reads its save magic from $7E14 area at boot). Pre-fix
            // returned 0 here; the games' save-validation paths got
            // stuck-at-uniform-gray as a result. Now reads the
            // allocated `prg_ram` byte.
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // T-60-003b (2026-05-17): WRAM at $6000-$7FFF (paired with the
        // read fix above). Without the write path, save data written by
        // the game is silently dropped on the floor.
        if (0x6000..=0x7FFF).contains(&addr) {
            let len = self.prg_ram.len();
            self.prg_ram[(addr - 0x6000) as usize % len] = value;
            return;
        }
        let (a0, a1) = vrc_a_bits(self.mapper_id, self.submapper, addr);
        match addr & 0xF000 {
            0x8000 => self.prg_lo = value & 0x1F,
            0x9000 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenA,
                    _ => Mirroring::SingleScreenB,
                };
            }
            0xA000 => self.prg_mid = value & 0x1F,
            0xB000 => {
                let slot = if a1 { 1 } else { 0 };
                self.write_chr_reg(slot, !a0, value);
            }
            0xC000 => {
                let slot = if a1 { 3 } else { 2 };
                self.write_chr_reg(slot, !a0, value);
            }
            0xD000 => {
                let slot = if a1 { 5 } else { 4 };
                self.write_chr_reg(slot, !a0, value);
            }
            0xE000 => {
                let slot = if a1 { 7 } else { 6 };
                self.write_chr_reg(slot, !a0, value);
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr_rom[off % self.chr_rom.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let len = self.chr_rom.len();
                    self.chr_rom[addr as usize % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 + self.vram.len());
        out.push(1u8);
        out.push(self.prg_lo);
        out.push(self.prg_mid);
        out.extend_from_slice(&self.chr);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 12 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != 1 {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_lo = data[1];
        self.prg_mid = data[2];
        self.chr.copy_from_slice(&data[3..11]);
        self.mirroring = match data[11] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[12..12 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
        for b in 0..banks_1k {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn vrc24_a_bits_per_board_pin_rewiring() {
        // The a0 (high-nibble) and a1 (register-select) pins are wired to
        // different CPU address lines per mapper number. On real Konami
        // boards the two candidate lines for each pin are tied together, so
        // the decode ORs them. Confirmed against per-game register-write
        // traces (see vrc_a_bits doc comment). Base $8000; only the low
        // decode bits matter. `(a0, a1)`.
        //
        // Mapper 21: a0 = A1|A6, a1 = A2|A7.
        assert_eq!(vrc_a_bits(21, 0, 0x8000 | (1 << 1)), (true, false));
        assert_eq!(vrc_a_bits(21, 0, 0x8000 | (1 << 6)), (true, false));
        assert_eq!(vrc_a_bits(21, 0, 0x8000 | (1 << 2)), (false, true));
        assert_eq!(vrc_a_bits(21, 0, 0x8000 | (1 << 7)), (false, true));
        // Mapper 22 (VRC2a): a0 = A1, a1 = A0 (SWAPPED, like VRC2c/m25).
        assert_eq!(vrc_a_bits(22, 0, 0x8000 | (1 << 1)), (true, false));
        assert_eq!(vrc_a_bits(22, 0, 0x8000 | (1 << 0)), (false, true));
        // Mapper 23: a0 = A0|A2, a1 = A1|A3 (Crisis Force uses A2/A3).
        assert_eq!(vrc_a_bits(23, 0, 0x8000 | (1 << 0)), (true, false));
        assert_eq!(vrc_a_bits(23, 0, 0x8000 | (1 << 2)), (true, false));
        assert_eq!(vrc_a_bits(23, 0, 0x8000 | (1 << 1)), (false, true));
        assert_eq!(vrc_a_bits(23, 0, 0x8000 | (1 << 3)), (false, true));
        // Mapper 25: a0 = A1|A3, a1 = A0|A2 (swapped).
        assert_eq!(vrc_a_bits(25, 0, 0x8000 | (1 << 1)), (true, false));
        assert_eq!(vrc_a_bits(25, 0, 0x8000 | (1 << 3)), (true, false));
        assert_eq!(vrc_a_bits(25, 0, 0x8000 | (1 << 0)), (false, true));
        assert_eq!(vrc_a_bits(25, 0, 0x8000 | (1 << 2)), (false, true));
    }

    #[test]
    fn vrc2_prg_bank_registers_and_fixed_banks() {
        // 8 PRG banks (each tagged with its index byte at the bank base).
        let mut m = Vrc2::new(synth(8), synth_chr(8), 22, 0, Mirroring::Vertical).unwrap();
        // $8000 selects the $8000-$9FFF bank (prg_lo); $A000 selects the
        // $A000-$BFFF bank (prg_mid). $C000/$E000 are fixed to last-2/last-1.
        m.cpu_write(0x8000, 3);
        m.cpu_write(0xA000, 5);
        assert_eq!(m.cpu_read(0x8000), 3, "prg_lo -> bank 3");
        assert_eq!(m.cpu_read(0xA000), 5, "prg_mid -> bank 5");
        assert_eq!(m.cpu_read(0xC000), 6, "fixed -> last-2 (bank 6 of 8)");
        assert_eq!(m.cpu_read(0xE000), 7, "fixed -> last-1 (bank 7 of 8)");
        // The 5-bit bank field masks high bits.
        m.cpu_write(0x8000, 0xE0 | 2);
        assert_eq!(m.cpu_read(0x8000), 2, "high bits above 5-bit field ignored");
    }

    #[test]
    fn vrc2_mirroring_control_register() {
        let mut m = Vrc2::new(synth(8), synth_chr(8), 22, 0, Mirroring::Vertical).unwrap();
        m.cpu_write(0x9000, 0);
        assert_eq!(m.mirroring, Mirroring::Vertical);
        m.cpu_write(0x9000, 1);
        assert_eq!(m.mirroring, Mirroring::Horizontal);
        m.cpu_write(0x9000, 2);
        assert_eq!(m.mirroring, Mirroring::SingleScreenA);
        m.cpu_write(0x9000, 3);
        assert_eq!(m.mirroring, Mirroring::SingleScreenB);
    }

    #[test]
    fn vrc2_chr_bank_low_high_nibble_split() {
        // CHR registers are written as low/high nibbles selected by a0, with
        // the bank slot pair selected by a1. Using VRC2b default wiring
        // (a0=bit0, a1=bit1), $B000 writes CHR slot 0 (a1=0): low nibble at
        // a0=0, high nibble at a0=1. Assemble bank 0x12 into slot 0 and read
        // CHR byte 0 (each CHR bank base is tagged with its index byte).
        let mut m = Vrc2::new(synth(8), synth_chr(0x20), 23, 3, Mirroring::Vertical).unwrap();
        // $B000 (a0=0): low nibble = 0x2.
        m.cpu_write(0xB000, 0x2);
        // $B001 (a0=1): high nibble = 0x1 -> bank = 0x12.
        m.cpu_write(0xB001, 0x1);
        assert_eq!(m.ppu_read(0x0000), 0x12, "CHR slot 0 -> bank 0x12");
    }
}
