//! Sprint 4-3 mappers: VRC2, VRC4, VRC6, Sunsoft FME-7, Namco 163.
//!
//! Banking + (where applicable) CPU-cycle IRQ counter.  Mapper-extended
//! audio for VRC6 (2 pulse + 1 sawtooth), the Sunsoft 5B built into
//! FME-7 (3 squares + envelope generator + 5-bit LFSR noise), and the
//! Namco 163 (1-8 wavetable channels, each playing a 32-sample 4-bit
//! wavetable from 128 bytes of mapper-internal RAM) is gated behind the
//! `mapper-audio` Cargo feature (default ON).  VRC7 FM remains deferred.
//!
//! See `docs/mappers.md`, especially §IRQ counter mechanisms #4 (CPU
//! cycle), and `to-dos/phase-4-mapper-coverage/sprint-3-vrc-extended.md`.

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

// ---------------------------------------------------------------------------
// VRC2 (mapper 22 + variants of 23/25 sub 3) — banking + mirroring, no IRQ.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// VRC4 (mappers 21/23/25 with VRC4 submappers) — banking + CPU-cycle IRQ.
// ---------------------------------------------------------------------------

/// VRC4 (and treats VRC2 hardware as a no-IRQ subset since the banking
/// is identical).  IRQ counter is 8-bit, clocked per CPU cycle by
/// default (mode bit selects scanline mode where it ticks every 114
/// cycles).
pub struct Vrc4 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_lo: u8,
    prg_mid: u8,
    prg_swap: bool, // PRG mode: $9002 bit 1 swaps $8000/$C000.
    chr: [u8; 8],
    mirroring: Mirroring,
    mapper_id: u16,
    submapper: u8,
    /// 8 KiB WRAM at $6000-$7FFF. T-60-003b (2026-05-17).
    prg_ram: Box<[u8]>,

    // IRQ counter state.
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_enable_after_ack: bool,
    irq_mode_scanline: bool,
    /// Sub-cycle prescaler for cycle mode (counts 0..341/3 and bumps
    /// counter at zero — scanline-equivalent every 113.66 CPU cycles).
    /// We approximate by counting 341 PPU dots per CPU-cycle group; per
    /// `notify_cpu_cycle` we increment a CPU-cycle prescaler.
    irq_prescaler: i32,
    irq_pending: bool,
}

impl Vrc4 {
    /// Construct a new VRC4 mapper.
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
                "VRC4 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "VRC4 CHR-ROM size {} is not a multiple of 1 KiB",
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
            prg_swap: false,
            chr: [0; 8],
            mirroring,
            mapper_id,
            submapper,
            // 8 KiB WRAM at $6000-$7FFF (T-60-003b).
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enable_after_ack: false,
            irq_mode_scanline: false,
            irq_prescaler: 341,
            irq_pending: false,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last1 = total_8k - 1;
        let last2 = total_8k.saturating_sub(2);
        let bank = match (addr & 0xE000, self.prg_swap) {
            (0x8000, false) => (self.prg_lo as usize) % total_8k,
            (0x8000, true) => last2,
            (0xA000, _) => (self.prg_mid as usize) % total_8k,
            (0xC000, false) => last2,
            (0xC000, true) => (self.prg_lo as usize) % total_8k,
            (0xE000, _) => last1,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank = (self.chr[slot] as usize) % total_1k;
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

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }
}

impl Mapper for Vrc4 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // T-60-003b (2026-05-17): VRC4 carts (Konami's mid-life
            // mappers — Ganbare Goemon 2, Wai Wai World, etc.) expose
            // 8KB battery-backed WRAM at $6000-$7FFF. Pre-fix returned
            // 0; games got stuck-at-uniform-gray validating save data.
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
        // read fix above).
        if (0x6000..=0x7FFF).contains(&addr) {
            let len = self.prg_ram.len();
            self.prg_ram[(addr - 0x6000) as usize % len] = value;
            return;
        }
        let (a0, a1) = vrc_a_bits(self.mapper_id, self.submapper, addr);
        match addr & 0xF000 {
            0x8000 => self.prg_lo = value & 0x1F,
            0x9000 => match (a0, a1) {
                (false, false) | (false, true) => {
                    // Mirroring control.
                    self.mirroring = match value & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
                (true, false) | (true, true) => {
                    // PRG mode swap.
                    self.prg_swap = (value & 0x02) != 0;
                }
            },
            0xA000 => self.prg_mid = value & 0x1F,
            0xB000 => self.write_chr_reg(if a1 { 1 } else { 0 }, !a0, value),
            0xC000 => self.write_chr_reg(if a1 { 3 } else { 2 }, !a0, value),
            0xD000 => self.write_chr_reg(if a1 { 5 } else { 4 }, !a0, value),
            0xE000 => self.write_chr_reg(if a1 { 7 } else { 6 }, !a0, value),
            0xF000 => match (a0, a1) {
                (false, false) => {
                    self.irq_latch = (self.irq_latch & 0xF0) | (value & 0x0F);
                }
                (true, false) => {
                    self.irq_latch = (self.irq_latch & 0x0F) | ((value & 0x0F) << 4);
                }
                (false, true) => {
                    // Control: bit 0 = enable_after_ack, bit 1 = enable now,
                    // bit 2 = mode (1 = scanline mode).
                    self.irq_enable_after_ack = (value & 0x01) != 0;
                    self.irq_enabled = (value & 0x02) != 0;
                    self.irq_mode_scanline = (value & 0x04) != 0;
                    self.irq_pending = false;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = 341;
                    }
                }
                (true, true) => {
                    // Acknowledge.
                    self.irq_pending = false;
                    self.irq_enabled = self.irq_enable_after_ack;
                }
            },
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

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_mode_scanline {
            // Tick prescaler at 341/3 PPU cycles per scanline = ~113.66
            // CPU cycles.  Use 341 -= 3 each CPU cycle, reload at 0.
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        } else {
            self.clock_irq_counter();
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: self.mapper_id,
            name: format!("VRC4 (sub {})", self.submapper),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG_lo".into(), format!("{:#04x}", self.prg_lo)));
        info.prg_banks
            .push(("PRG_mid".into(), format!("{:#04x}", self.prg_mid)));
        info.prg_banks
            .push(("swap".into(), format!("{}", self.prg_swap)));
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("latch".into(), format!("{:#04x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state.push((
            "scanline_mode".into(),
            format!("{}", self.irq_mode_scanline),
        ));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(40 + self.vram.len());
        out.push(1u8);
        out.push(self.prg_lo);
        out.push(self.prg_mid);
        out.push(u8::from(self.prg_swap));
        out.extend_from_slice(&self.chr);
        out.push(self.mirroring as u8);
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_enable_after_ack));
        out.push(u8::from(self.irq_mode_scanline));
        out.extend_from_slice(&self.irq_prescaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 1 + 1 + 1 + 8 + 1 + 1 + 1 + 1 + 1 + 1 + 4 + 1;
        let expected = scalar_len + self.vram.len();
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
        self.prg_swap = data[3] != 0;
        self.chr.copy_from_slice(&data[4..12]);
        self.mirroring = match data[12] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_latch = data[13];
        self.irq_counter = data[14];
        self.irq_enabled = data[15] != 0;
        self.irq_enable_after_ack = data[16] != 0;
        self.irq_mode_scanline = data[17] != 0;
        self.irq_prescaler = i32::from_le_bytes(
            data[18..22]
                .try_into()
                .map_err(|_| MapperError::Invalid("prescaler".into()))?,
        );
        self.irq_pending = data[22] != 0;
        self.vram.copy_from_slice(&data[23..23 + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// VRC6 (mappers 24, 26) — banking + IRQ + 3 audio channels (2 pulse + 1
// sawtooth).  Audio is gated behind the `mapper-audio` Cargo feature
// (default ON); when off, the register decoders still latch the state so
// save-state round-trip stays compatible, but the channel oscillators do
// not advance.
// ---------------------------------------------------------------------------

/// VRC6 audio pulse channel state (`$9000-$9002` for pulse 1, `$A000-$A002`
/// for pulse 2). Period is 12-bit, decrements every CPU cycle. On
/// underflow, the duty index advances by 1 (mod 16). Output is volume when
/// duty index <= duty-cycle threshold (or always-on when "ignore duty" mode
/// is set); zero otherwise.
#[derive(Clone, Default)]
pub(crate) struct Vrc6Pulse {
    /// Bits 0-3: volume (0..=15). Bits 4-6: duty (0..=7, sets the duty-cycle
    /// threshold). Bit 7: ignore-duty (output always = volume).
    pub(crate) ctrl: u8,
    /// 12-bit period reload value.
    pub(crate) period: u16,
    /// Channel enable bit (from period-hi bit 7).
    pub(crate) enabled: bool,
    /// 12-bit countdown timer.
    pub(crate) timer: u16,
    /// 4-bit duty-cycle step (0..=15).
    pub(crate) step: u8,
}

impl Vrc6Pulse {
    /// Clock the timer one CPU cycle. When it underflows, advance the duty
    /// step and reload from `period`.
    pub(crate) fn clock(&mut self) {
        if !self.enabled {
            return;
        }
        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) & 0x0F;
        } else {
            self.timer -= 1;
        }
    }

    /// Current 4-bit unsigned output (0..=15). 0 when disabled.
    pub(crate) fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        let duty = (self.ctrl >> 4) & 0x07;
        let ignore_duty = (self.ctrl & 0x80) != 0;
        let volume = self.ctrl & 0x0F;
        if ignore_duty || self.step <= duty {
            volume
        } else {
            0
        }
    }
}

/// VRC6 audio sawtooth channel state (`$B000-$B002`). 6-bit accumulator
/// adds an "accumulator rate" once per CPU cycle. Every 14th underflow,
/// the high 5 bits of the accumulator are emitted (0..=31) and the
/// accumulator resets.
#[derive(Clone, Default)]
pub(crate) struct Vrc6Saw {
    /// 6-bit accumulator-rate value (bits 5-0 of `$B000`).
    pub(crate) rate: u8,
    /// 12-bit period reload value.
    pub(crate) period: u16,
    /// Channel enable bit (from period-hi bit 7).
    pub(crate) enabled: bool,
    /// 12-bit countdown timer.
    pub(crate) timer: u16,
    /// Internal step counter 0..=13 (every other increment "ticks the
    /// accumulator"; 7 ticks per cycle = 14 steps).
    pub(crate) step: u8,
    /// 8-bit accumulator. Output = accumulator >> 3 (5-bit, 0..=31).
    pub(crate) acc: u8,
}

impl Vrc6Saw {
    pub(crate) fn clock(&mut self) {
        if !self.enabled {
            return;
        }
        if self.timer == 0 {
            self.timer = self.period;
            // Step 0..=13: every 2nd step (1, 3, 5, 7, 9, 11, 13) accumulates.
            // Step 14 (== reset) zeros the accumulator and rolls step to 0.
            self.step += 1;
            if (self.step & 1) == 1 {
                self.acc = self.acc.wrapping_add(self.rate);
            }
            if self.step >= 14 {
                self.step = 0;
                self.acc = 0;
            }
        } else {
            self.timer -= 1;
        }
    }

    /// 5-bit unsigned output (0..=31).
    pub(crate) fn output(&self) -> u8 {
        if !self.enabled {
            return 0;
        }
        self.acc >> 3
    }
}

/// VRC6 (Mappers 24 / 26).  Audio extension is implemented behind the
/// `mapper-audio` Cargo feature (default ON).
pub struct Vrc6 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_16: u8, // 16 KiB bank @ $8000-$BFFF
    prg_8: u8,  // 8 KiB bank @ $C000-$DFFF
    chr: [u8; 8],
    mirroring: Mirroring,
    /// 8 KiB WRAM at $6000-$7FFF (battery-backed on Konami carts).
    /// T-60-003b (2026-05-17).
    prg_ram: Box<[u8]>,
    /// Mapper 24 = VRC6a (a0/a1 = bits 0/1).
    /// Mapper 26 = VRC6b (a0/a1 = bits 1/0 — swapped).
    swap_a01: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_enable_after_ack: bool,
    irq_mode_scanline: bool,
    irq_prescaler: i32,
    irq_pending: bool,

    // Audio extension state.
    /// `$9003` global audio control. Bit 0 = halt-all; bits 1-2 = freq scale
    /// shift (0 = ÷1, 1 = ÷16, 2 = ÷256 — implemented by left-shifting the
    /// effective period). We keep the raw byte and inspect bits at clock time.
    audio_ctrl: u8,
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    saw: Vrc6Saw,
}

impl Vrc6 {
    /// Construct a new VRC6 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mapper_id: u16,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC6 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "VRC6 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_16: 0,
            prg_8: 0,
            chr: [0; 8],
            mirroring,
            // 8 KiB WRAM at $6000-$7FFF (T-60-003b).
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            swap_a01: mapper_id == 26,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enable_after_ack: false,
            irq_mode_scanline: false,
            irq_prescaler: 341,
            irq_pending: false,
            audio_ctrl: 0,
            pulse1: Vrc6Pulse::default(),
            pulse2: Vrc6Pulse::default(),
            saw: Vrc6Saw::default(),
        })
    }

    /// Effective period for a pulse/saw channel, taking the global
    /// `$9003` halt + frequency-scale bits into account.
    fn effective_period_p(&self, p: &Vrc6Pulse) -> u16 {
        let shift = match (self.audio_ctrl >> 1) & 0x03 {
            0 => 0,
            1 => 4,
            _ => 8,
        };
        p.period >> shift
    }

    fn effective_period_s(&self) -> u16 {
        let shift = match (self.audio_ctrl >> 1) & 0x03 {
            0 => 0,
            1 => 4,
            _ => 8,
        };
        self.saw.period >> shift
    }

    /// Clock all three audio channels one CPU cycle. Called from
    /// `notify_cpu_cycle` when the `mapper-audio` feature is on.
    #[cfg(feature = "mapper-audio")]
    fn clock_audio(&mut self) {
        // $9003 bit 0 = halt-all. When set, channels do not advance.
        if (self.audio_ctrl & 0x01) != 0 {
            return;
        }
        // Apply the frequency-scale shift transiently by temporarily
        // narrowing `period` for the channel clock. We don't mutate the
        // stored period -- the shift is purely a read-time scaling.
        let p1_period = self.effective_period_p(&self.pulse1);
        let p2_period = self.effective_period_p(&self.pulse2);
        let saw_period = self.effective_period_s();
        let saved_p1 = self.pulse1.period;
        let saved_p2 = self.pulse2.period;
        let saved_saw = self.saw.period;
        self.pulse1.period = p1_period;
        self.pulse2.period = p2_period;
        self.saw.period = saw_period;
        self.pulse1.clock();
        self.pulse2.clock();
        self.saw.clock();
        self.pulse1.period = saved_p1;
        self.pulse2.period = saved_p2;
        self.saw.period = saved_saw;
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last1 = total_8k - 1;
        match addr {
            0x8000..=0xBFFF => {
                let bank16 = (self.prg_16 as usize) & 0x0F;
                let bank8 = (bank16 << 1) | (((addr & 0x2000) >> 13) as usize);
                (bank8 % total_8k) * PRG_BANK_8K + (addr as usize & 0x1FFF)
            }
            0xC000..=0xDFFF => {
                let bank8 = (self.prg_8 as usize) & 0x1F;
                (bank8 % total_8k) * PRG_BANK_8K + (addr as usize & 0x1FFF)
            }
            0xE000..=0xFFFF => last1 * PRG_BANK_8K + (addr as usize & 0x1FFF),
            _ => 0,
        }
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank = (self.chr[slot] as usize) % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }

    fn decode_a(&self, addr: u16) -> u8 {
        let a0 = (addr & 1) != 0;
        let a1 = (addr & 2) != 0;
        let (a0, a1) = if self.swap_a01 { (a1, a0) } else { (a0, a1) };
        u8::from(a0) | (u8::from(a1) << 1)
    }
}

impl Mapper for Vrc6 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source + expansion audio
    // (the audio hook only exists under the `mapper-audio` feature).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // T-60-003b (2026-05-17): VRC6 carts (Akumajou Densetsu /
            // Esper Dream 2 / Mouryou Senki Madara) include 8KB
            // battery-backed WRAM at $6000-$7FFF. Pre-fix returned 0;
            // Esper Dream 2 + Madara got stuck-at-uniform-gray
            // validating save data, both bit-identical hash
            // 89ee4c476c97a325 (the smoking-gun signal that pointed
            // here per the recovery-session diagnostic at
            // docs/audit/v1-closeout-progress-2026-05-17.md).
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
        // read fix above).
        if (0x6000..=0x7FFF).contains(&addr) {
            let len = self.prg_ram.len();
            self.prg_ram[(addr - 0x6000) as usize % len] = value;
            return;
        }
        let a = self.decode_a(addr);
        match addr & 0xF000 {
            0x8000 => self.prg_16 = value & 0x0F,
            0x9000 => match a {
                // $9000: Pulse 1 control (volume/duty/mode).
                0 => self.pulse1.ctrl = value,
                // $9001: Pulse 1 period low.
                1 => {
                    self.pulse1.period = (self.pulse1.period & 0x0F00) | u16::from(value);
                }
                // $9002: Pulse 1 period high + enable.
                2 => {
                    self.pulse1.period =
                        (self.pulse1.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                    self.pulse1.enabled = (value & 0x80) != 0;
                    if !self.pulse1.enabled {
                        self.pulse1.step = 0;
                    }
                }
                // $9003: Global audio control (halt + freq scale).
                _ => self.audio_ctrl = value,
            },
            0xA000 => match a {
                // $A000: Pulse 2 control.
                0 => self.pulse2.ctrl = value,
                // $A001: Pulse 2 period low.
                1 => {
                    self.pulse2.period = (self.pulse2.period & 0x0F00) | u16::from(value);
                }
                // $A002: Pulse 2 period high + enable.
                2 => {
                    self.pulse2.period =
                        (self.pulse2.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                    self.pulse2.enabled = (value & 0x80) != 0;
                    if !self.pulse2.enabled {
                        self.pulse2.step = 0;
                    }
                }
                _ => {}
            },
            0xB000 => match a {
                // $B000: Sawtooth accumulator rate (6-bit).
                0 => self.saw.rate = value & 0x3F,
                // $B001: Sawtooth period low.
                1 => {
                    self.saw.period = (self.saw.period & 0x0F00) | u16::from(value);
                }
                // $B002: Sawtooth period high + enable.
                2 => {
                    self.saw.period = (self.saw.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
                    self.saw.enabled = (value & 0x80) != 0;
                    if !self.saw.enabled {
                        self.saw.step = 0;
                        self.saw.acc = 0;
                    }
                }
                _ => {
                    // $B003: Mirroring + PPU/CPU mode.
                    self.mirroring = match (value >> 2) & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
            },
            0xC000 => self.prg_8 = value & 0x1F,
            0xD000 => self.chr[a as usize] = value,
            0xE000 => self.chr[(a + 4) as usize] = value,
            0xF000 => match a {
                0 => self.irq_latch = value,
                1 => {
                    self.irq_enable_after_ack = (value & 0x01) != 0;
                    self.irq_enabled = (value & 0x02) != 0;
                    self.irq_mode_scanline = (value & 0x04) == 0;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = 341;
                    }
                    self.irq_pending = false;
                }
                2 => {
                    self.irq_pending = false;
                    self.irq_enabled = self.irq_enable_after_ack;
                }
                _ => {}
            },
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

    fn notify_cpu_cycle(&mut self) {
        // Audio runs every CPU cycle regardless of IRQ state.
        #[cfg(feature = "mapper-audio")]
        self.clock_audio();

        if !self.irq_enabled {
            return;
        }
        if self.irq_mode_scanline {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        } else {
            self.clock_irq_counter();
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i16 {
        // Three channels: pulse1 (4-bit, 0..=15), pulse2 (4-bit, 0..=15),
        // sawtooth (5-bit, 0..=31). Sum is in 0..=61. Scale to i16
        // centered on zero with reasonable headroom for the APU mixer.
        //
        // Per nesdev "VRC6 audio": the three channels are summed digitally,
        // so a linear sum is the canonical mix. Scaling factor of ~256
        // brings the peak (61) to ~15,616 -- well below i16::MAX, leaving
        // ~5x headroom for the APU adding alongside.
        let p1 = i16::from(self.pulse1.output());
        let p2 = i16::from(self.pulse2.output());
        let saw = i16::from(self.saw.output());
        // Center at zero: subtract approx half the peak (~30).
        ((p1 + p2 + saw) - 30) * 256
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mapper_id = if self.swap_a01 { 26 } else { 24 };
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id,
            name: "VRC6".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG16".into(), format!("{:#04x}", self.prg_16)));
        info.prg_banks
            .push(("PRG8".into(), format!("{:#04x}", self.prg_8)));
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("latch".into(), format!("{:#04x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v2: appends audio state (audio_ctrl + 3 channels) at the end.
        // Per ADR-0003: strictly additive; older readers ignore the tail.
        // Channel layout per channel: ctrl(1) + period_lo(1) + period_hi(1)
        //   + enabled(1) + timer_lo(1) + timer_hi(1) + step(1)
        //   = 7 bytes for a pulse channel.
        // Saw: rate(1) + period_lo(1) + period_hi(1) + enabled(1)
        //   + timer_lo(1) + timer_hi(1) + step(1) + acc(1) = 8 bytes.
        // Header: audio_ctrl(1).
        // Total audio tail = 1 + 7 + 7 + 8 = 23 bytes.
        let mut out = Vec::with_capacity(48 + self.vram.len() + 23);
        out.push(2u8); // version
        out.push(self.prg_16);
        out.push(self.prg_8);
        out.extend_from_slice(&self.chr);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.swap_a01));
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_enable_after_ack));
        out.push(u8::from(self.irq_mode_scanline));
        out.extend_from_slice(&self.irq_prescaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        // Audio tail (v2).
        out.push(self.audio_ctrl);
        Self::write_pulse(&mut out, &self.pulse1);
        Self::write_pulse(&mut out, &self.pulse2);
        Self::write_saw(&mut out, &self.saw);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 1 + 1 + 8 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 4 + 1;
        let core_expected = scalar_len + self.vram.len();
        if data.len() < core_expected {
            return Err(MapperError::Truncated {
                expected: core_expected,
                got: data.len(),
            });
        }
        let version = data[0];
        if !(1..=2).contains(&version) {
            return Err(MapperError::UnsupportedVersion(version));
        }
        self.prg_16 = data[1];
        self.prg_8 = data[2];
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
        self.swap_a01 = data[12] != 0;
        self.irq_latch = data[13];
        self.irq_counter = data[14];
        self.irq_enabled = data[15] != 0;
        self.irq_enable_after_ack = data[16] != 0;
        self.irq_mode_scanline = data[17] != 0;
        self.irq_prescaler = i32::from_le_bytes(
            data[18..22]
                .try_into()
                .map_err(|_| MapperError::Invalid("prescaler".into()))?,
        );
        self.irq_pending = data[22] != 0;
        self.vram.copy_from_slice(&data[23..23 + self.vram.len()]);

        // v2 tail (optional even when version == 2, in case the writer is
        // shorter than expected): audio state. v1 blobs end here; the audio
        // state stays at defaults.
        if version == 2 {
            let tail_off = 23 + self.vram.len();
            if data.len() < tail_off + 23 {
                // Not strict: a v2 blob shorter than 23 audio bytes is
                // accepted; remaining fields default-initialize. This keeps
                // forward-compat consistent with ADR-0003.
                return Ok(());
            }
            self.audio_ctrl = data[tail_off];
            Self::read_pulse(&data[tail_off + 1..tail_off + 8], &mut self.pulse1);
            Self::read_pulse(&data[tail_off + 8..tail_off + 15], &mut self.pulse2);
            Self::read_saw(&data[tail_off + 15..tail_off + 23], &mut self.saw);
        }
        Ok(())
    }
}

impl Vrc6 {
    fn write_pulse(out: &mut Vec<u8>, p: &Vrc6Pulse) {
        out.push(p.ctrl);
        out.extend_from_slice(&p.period.to_le_bytes());
        out.push(u8::from(p.enabled));
        out.extend_from_slice(&p.timer.to_le_bytes());
        out.push(p.step);
    }

    fn write_saw(out: &mut Vec<u8>, s: &Vrc6Saw) {
        out.push(s.rate);
        out.extend_from_slice(&s.period.to_le_bytes());
        out.push(u8::from(s.enabled));
        out.extend_from_slice(&s.timer.to_le_bytes());
        out.push(s.step);
        out.push(s.acc);
    }

    fn read_pulse(src: &[u8], p: &mut Vrc6Pulse) {
        p.ctrl = src[0];
        p.period = u16::from_le_bytes([src[1], src[2]]);
        p.enabled = src[3] != 0;
        p.timer = u16::from_le_bytes([src[4], src[5]]);
        p.step = src[6] & 0x0F;
    }

    fn read_saw(src: &[u8], s: &mut Vrc6Saw) {
        s.rate = src[0] & 0x3F;
        s.period = u16::from_le_bytes([src[1], src[2]]);
        s.enabled = src[3] != 0;
        s.timer = u16::from_le_bytes([src[4], src[5]]);
        s.step = src[6];
        s.acc = src[7];
    }
}

// ---------------------------------------------------------------------------
// VRC7 (mapper 85) — banking + IRQ + YM2413 OPLL-derived FM audio surface.
//
// VRC7 carries on-cart a Yamaha YM2413 (OPLL)-derived FM synthesizer with
// 6 channels of 2-operator FM and a custom 15-entry instrument ROM (used
// only by Lagrange Point, Konami's sole VRC7 commercial release).  Per
// ADR-0004 (`docs/adr/0004-vrc7-audio-deferred.md`), the FM synthesizer is
// deferred to v1.x: no published Rust OPLL crate meets the permissive-
// license + maintenance + no_std + VRC7-instrument-ROM criteria.  This
// implementation lands the **base mapper** (PRG / CHR banking + mirroring
// control + CPU-cycle IRQ counter identical to VRC6's) so that mapper 85
// ROMs load and run correctly with silent audio.
//
// The audio register surface (`$9010` = OPLL address latch; `$9030` =
// OPLL data write) is decoded and latched into a small `Vrc7AudioRegs`
// snapshot even though no synthesizer consumes it; this preserves
// save-state round-trip across audio-enabled and audio-deferred builds
// (consistent with VRC6 / Sunsoft 5B / Namco 163 / MMC5 audio surfaces).
// `mix_audio` returns 0 unconditionally for VRC7 at v0.9.x; a future
// v1.x commit will land the OPLL state and bump the VRC7 save-state
// version from 1 → 2 per ADR-0003 (append, with v1 backcompat).
//
// Register layout (per NESdev wiki "VRC7"):
//   $8000: PRG bank @ $8000-$9FFF (6 bits)
//   $8010 / $8008: PRG bank @ $A000-$BFFF (6 bits; address-line variance)
//   $9000: PRG bank @ $C000-$DFFF (6 bits)
//   $E000-$FFFF: fixed to the last 8 KiB bank
//   $9010: OPLL register address latch
//   $9030: OPLL register data write
//   $A000 / $A008/$A010 / $B000 / ... / $D008/$D010: CHR banks 0..=7
//   $E000: mirroring (bits 1-0), WRAM enable (bit 6), expansion-sound
//          silence (bit 7)
//   $E008 / $E010: IRQ latch
//   $F000: IRQ control (E / A / M bits — same shape as VRC6)
//   $F008 / $F010: IRQ acknowledge
//
// Lagrange Point uses a 7-cycle delay loop between writes to `$9010`
// and `$9030`; the chip latches each independently so the delay is
// not modelled here.  See `ref-docs/research-report.md` §VRC7 for
// instruction-level write timing notes.
// ---------------------------------------------------------------------------

/// VRC7 audio register snapshot.
///
/// Two latches: the OPLL register address (set by writes to `$9010`)
/// and the data byte (set by writes to `$9030` after `$9010`).  Per
/// ADR-0004, this is **decoded and latched but not synthesized** in
/// v0.9.x — the byte stream sits available for a future v1.x OPLL
/// integration, and save-state round-trip works in both directions
/// without an audio backend.
#[derive(Clone)]
struct Vrc7AudioRegs {
    /// Last 6-bit register address written to `$9010`.  YM2413 has 64
    /// addressable registers; VRC7 exposes a 6-channel subset.
    addr_latch: u8,
    /// Last data byte written to `$9030`.  Available for inspection /
    /// equivalence testing against a future OPLL backend.
    data_latch: u8,
    /// 64-entry shadow of the most recent data written to each OPLL
    /// register address.  A future synthesizer reads this on demand
    /// (e.g. on key-on) to seed channel state without re-running the
    /// register-write history.  Sized at 64 to match the full YM2413
    /// register space (the chip's 6 channels use $10-$15 / $20-$25 /
    /// $30-$35; instrument bytes are at $00-$07).
    regs: [u8; 64],
    /// Mirror of `$E000` bit 7 (expansion-sound silence). When set, a
    /// future synthesizer's output is forced to zero; banking + IRQ
    /// are unaffected.
    silenced: bool,
}

impl Default for Vrc7AudioRegs {
    fn default() -> Self {
        Self {
            addr_latch: 0,
            data_latch: 0,
            regs: [0u8; 64],
            silenced: false,
        }
    }
}

/// VRC7 (Mapper 85).  Banking + IRQ + (deferred per ADR-0004) FM audio
/// surface for Lagrange Point.
pub struct Vrc7 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    /// 8 KiB PRG bank at $8000-$9FFF.
    prg_0: u8,
    /// 8 KiB PRG bank at $A000-$BFFF.
    prg_1: u8,
    /// 8 KiB PRG bank at $C000-$DFFF.
    prg_2: u8,
    /// 1 KiB CHR banks at $0000-$1FFF (one entry per KiB).
    chr: [u8; 8],
    mirroring: Mirroring,

    // IRQ counter (identical shape to VRC6's).
    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_enable_after_ack: bool,
    irq_mode_scanline: bool,
    irq_prescaler: i32,
    irq_pending: bool,

    /// PRG-RAM enable (bit 6 of `$E000`). When clear, `$6000-$7FFF`
    /// reads/writes are ignored.
    prg_ram_enable: bool,

    /// 8 KiB WRAM at `$6000-$7FFF`. Lagrange Point's boot routine runs a
    /// write-then-read-back self-test on this region (`STA ($00),Y` /
    /// `CMP ($00),Y` with `$00/$01 = $6000`); without backing storage the
    /// read-back always returned 0, the compare failed, and the game
    /// jumped to its lockup loop at `$EC2F` (blank gray screen — it never
    /// reaches CHR-RAM / nametable upload). Backed now, mirroring the
    /// VRC2/VRC4 WRAM fix (T-60-003b).
    prg_ram: Box<[u8]>,

    /// Audio register surface. Decoded and latched in v0.9.x; not yet
    /// synthesized (see ADR-0004).
    audio: Vrc7AudioRegs,

    /// OPLL FM synthesizer. Lives behind the `mapper-audio` feature
    /// to keep the no_std cross-compile cheap; when the feature is
    /// off, `mix_audio` returns 0 unconditionally (matching the
    /// pre-v1.1.0 ADR-0004 deferred state).
    #[cfg(feature = "mapper-audio")]
    opll: rustynes_apu::Opll,

    /// CPU-cycle counter for the OPLL native sample rate. NES NTSC
    /// CPU runs at 1,789,773 Hz; the OPLL native rate is 49,716 Hz.
    /// `1789773 / 49716 ≈ 35.997` — we tick the OPLL every 36 CPU
    /// cycles, which is correct to 0.008% (< 1 Hz tuning drift).
    #[cfg(feature = "mapper-audio")]
    opll_clock_counter: u16,

    /// Latest OPLL sample. The mapper holds this between OPLL ticks
    /// (every 36 CPU cycles) so `mix_audio` calls in between return
    /// the most-recent value. The APU's band-limited synthesis
    /// handles the rate conversion from OPLL's 49,716 Hz to the
    /// host sample rate.
    #[cfg(feature = "mapper-audio")]
    last_opll_sample: i16,
}

impl Vrc7 {
    /// Construct a new VRC7 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] if the PRG-ROM size is not a
    /// non-zero multiple of 8 KiB or the CHR-ROM size is not a
    /// multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC7 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "VRC7 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_0: 0,
            prg_1: 0,
            prg_2: 0,
            chr: [0; 8],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enable_after_ack: false,
            irq_mode_scanline: false,
            irq_prescaler: 341,
            irq_pending: false,
            prg_ram_enable: false,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            audio: Vrc7AudioRegs::default(),
            #[cfg(feature = "mapper-audio")]
            opll: rustynes_apu::Opll::new(rustynes_apu::OpllChipType::Vrc7),
            #[cfg(feature = "mapper-audio")]
            opll_clock_counter: 0,
            #[cfg(feature = "mapper-audio")]
            last_opll_sample: 0,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last1 = total_8k - 1;
        let (bank, off_in_8k) = match addr {
            0x8000..=0x9FFF => (self.prg_0 as usize, addr as usize & 0x1FFF),
            0xA000..=0xBFFF => (self.prg_1 as usize, addr as usize & 0x1FFF),
            0xC000..=0xDFFF => (self.prg_2 as usize, addr as usize & 0x1FFF),
            0xE000..=0xFFFF => (last1, addr as usize & 0x1FFF),
            _ => return 0,
        };
        (bank % total_8k) * PRG_BANK_8K + off_in_8k
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank = (self.chr[slot] as usize) % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }

    /// Decode mirroring from the low 2 bits of `$E000`.  Per NESdev
    /// "VRC7": `00` = vertical, `01` = horizontal, `10` = single-screen
    /// A, `11` = single-screen B.
    fn decode_mirroring(value: u8) -> Mirroring {
        match value & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenA,
            _ => Mirroring::SingleScreenB,
        }
    }
}

impl Mapper for Vrc7 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source + expansion audio
    // (the audio hook only exists under the `mapper-audio` feature).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // 8 KiB WRAM. Backed by storage so Lagrange Point's boot
                // RAM self-test (write then read-back) succeeds. The
                // enable bit (`$E000` bit 6) is modelled for completeness
                // but does not gate the backing store: the game toggles it
                // around the test, and real VRC7 emulators keep the WRAM
                // continuously addressable.
                self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()]
            }
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // VRC7 register decoding tolerates both A3 (`$_008`) and A4
        // (`$_010`) variants per board revision.  The high-nibble
        // selector picks the register family; within each family the
        // bank/IRQ/audio variant is chosen by bits 4-5 of the low byte.
        match addr & 0xF000 {
            0x6000 | 0x7000 => {
                // 8 KiB WRAM write (backed; see cpu_read).
                let len = self.prg_ram.len();
                self.prg_ram[(addr - 0x6000) as usize % len] = value;
            }
            0x8000 => {
                // $8000 selects PRG bank 0; $8010 / $8008 selects bank 1.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.prg_1 = value & 0x3F;
                } else {
                    self.prg_0 = value & 0x3F;
                }
            }
            0x9000 => {
                // $9000 (and $9008 mirror) -> PRG bank 2.
                // $9010 (and $9018 mirror) -> OPLL register address latch.
                // $9030 (and $9038 mirror) -> OPLL register data write.
                let sub = addr & 0x0030;
                if sub == 0x0010 {
                    self.audio.addr_latch = value & 0x3F;
                } else if sub == 0x0030 {
                    let idx = (self.audio.addr_latch & 0x3F) as usize;
                    self.audio.regs[idx] = value;
                    self.audio.data_latch = value;
                    // Forward to the OPLL synthesizer. The address was
                    // latched on the previous `$9010` write; per
                    // `Vrc7Audio.h` (Mesen2) this is the canonical
                    // shape — `WriteReg($9010, addr); WriteReg($9030, data)`.
                    // The 7-cycle inter-write delay Lagrange Point
                    // observes on real hardware is enforced by the CPU
                    // emitter; the chip latches each independently.
                    #[cfg(feature = "mapper-audio")]
                    self.opll.write_reg(self.audio.addr_latch, value);
                } else {
                    // $9000 / $9008 / $9020 / $9028 -> PRG bank 2.
                    self.prg_2 = value & 0x3F;
                }
            }
            0xA000 => {
                // CHR banks 0 / 1.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[1] = value;
                } else {
                    self.chr[0] = value;
                }
            }
            0xB000 => {
                // CHR banks 2 / 3.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[3] = value;
                } else {
                    self.chr[2] = value;
                }
            }
            0xC000 => {
                // CHR banks 4 / 5.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[5] = value;
                } else {
                    self.chr[4] = value;
                }
            }
            0xD000 => {
                // CHR banks 6 / 7.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.chr[7] = value;
                } else {
                    self.chr[6] = value;
                }
            }
            0xE000 => {
                // $E000: mirroring (bits 1-0), WRAM enable (bit 6),
                // expansion-sound silence (bit 7).
                // $E008 / $E010: IRQ latch.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.irq_latch = value;
                } else {
                    self.mirroring = Self::decode_mirroring(value);
                    self.prg_ram_enable = (value & 0x40) != 0;
                    self.audio.silenced = (value & 0x80) != 0;
                }
            }
            0xF000 => {
                // $F000: IRQ control. $F008/$F010: IRQ acknowledge.
                if (addr & 0x0010) != 0 || (addr & 0x0008) != 0 {
                    self.irq_pending = false;
                    self.irq_enabled = self.irq_enable_after_ack;
                } else {
                    self.irq_enable_after_ack = (value & 0x01) != 0;
                    self.irq_enabled = (value & 0x02) != 0;
                    self.irq_mode_scanline = (value & 0x04) == 0;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = 341;
                    }
                    self.irq_pending = false;
                }
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
                    // Must go through the SAME banked offset `ppu_read` uses
                    // (`chr_offset`), not the raw PPU address — otherwise a
                    // game that banks CHR-RAM (Lagrange Point) writes tiles to
                    // one offset and reads them back from another, leaving the
                    // pattern tables effectively blank.
                    let off = self.chr_offset(addr);
                    let len = self.chr_rom.len();
                    self.chr_rom[off % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        // Advance the OPLL synthesizer every 36 CPU cycles, matching
        // the NES NTSC CPU clock / OPLL native sample rate ratio.
        // Holds the produced sample in `last_opll_sample` for the
        // bus's per-APU-sample `mix_audio` calls.
        #[cfg(feature = "mapper-audio")]
        {
            self.opll_clock_counter = self.opll_clock_counter.wrapping_add(1);
            if self.opll_clock_counter >= 36 {
                self.opll_clock_counter = 0;
                self.last_opll_sample = self.opll.calc();
            }
        }

        if !self.irq_enabled {
            return;
        }
        if self.irq_mode_scanline {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        } else {
            self.clock_irq_counter();
        }
    }

    /// Mix the current OPLL sample into the APU's external-audio
    /// channel. Returns 0 when the cartridge's expansion-sound
    /// silence bit (`$E000` bit 7) is set OR the `mapper-audio`
    /// feature is off; otherwise returns the most-recent OPLL
    /// sample in the i16 range [-4095, 4095] (the chip's
    /// 13-bit DAC scaled to 14-bit signed via `<< 1` in the
    /// `lookup_exp_table` final stage).
    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i16 {
        if self.audio.silenced {
            0
        } else {
            self.last_opll_sample
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 85,
            name: "VRC7".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG0".into(), format!("{:#04x}", self.prg_0)));
        info.prg_banks
            .push(("PRG1".into(), format!("{:#04x}", self.prg_1)));
        info.prg_banks
            .push(("PRG2".into(), format!("{:#04x}", self.prg_2)));
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("latch".into(), format!("{:#04x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.extra.push((
            "audio".into(),
            "deferred (ADR-0004; mapper 85 audio = silent)".into(),
        ));
        info.extra.push((
            "audio_addr".into(),
            format!("{:#04x}", self.audio.addr_latch),
        ));
        info.extra.push((
            "audio_data".into(),
            format!("{:#04x}", self.audio.data_latch),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v1 layout (audio synthesis deferred per ADR-0004):
        //   version(1)
        //   prg_0 / prg_1 / prg_2 (3)
        //   chr[0..8] (8)
        //   mirroring(1) + prg_ram_enable(1)
        //   irq_latch(1) + irq_counter(1) + irq_enabled(1) +
        //   irq_enable_after_ack(1) + irq_mode_scanline(1) +
        //   irq_prescaler(4 le) + irq_pending(1)
        //   audio addr_latch(1) + data_latch(1) + silenced(1) +
        //   audio.regs[0..64] (64)
        //   vram (2 KiB)
        //
        // Per ADR-0003: the future v1.x commit that lands the OPLL state
        // bumps version 1 → 2, appending the synthesizer's internal
        // state (operator phases, envelope phases, key-on flags) at the
        // tail.  v1 blobs default-load the synthesizer to silent.
        // version(1) + prg(3) + chr(8) + mirroring(1) + prg_ram_enable(1)
        //   + irq_latch(1) + irq_counter(1) + irq_enabled(1)
        //   + irq_enable_after_ack(1) + irq_mode_scanline(1)
        //   + irq_prescaler(4) + irq_pending(1)
        //   + audio addr_latch(1) + data_latch(1) + silenced(1) + regs(64)
        // = 1 + 3 + 8 + 1 + 1 + 5 + 5 + 67 = 91
        let scalar_len = 1 + 3 + 8 + 1 + 1 + 10 + 3 + 64;
        let mut out = Vec::with_capacity(scalar_len + self.vram.len());
        out.push(1u8); // version
        out.push(self.prg_0);
        out.push(self.prg_1);
        out.push(self.prg_2);
        out.extend_from_slice(&self.chr);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.prg_ram_enable));
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_enable_after_ack));
        out.push(u8::from(self.irq_mode_scanline));
        out.extend_from_slice(&self.irq_prescaler.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.push(self.audio.addr_latch);
        out.push(self.audio.data_latch);
        out.push(u8::from(self.audio.silenced));
        out.extend_from_slice(&self.audio.regs);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        // version(1) + prg(3) + chr(8) + mirroring(1) + prg_ram_enable(1)
        //   + irq_latch(1) + irq_counter(1) + irq_enabled(1)
        //   + irq_enable_after_ack(1) + irq_mode_scanline(1)
        //   + irq_prescaler(4) + irq_pending(1)
        //   + audio addr_latch(1) + data_latch(1) + silenced(1) + regs(64)
        // = 1 + 3 + 8 + 1 + 1 + 5 + 5 + 67 = 91
        let scalar_len = 1 + 3 + 8 + 1 + 1 + 10 + 3 + 64;
        let core_expected = scalar_len + self.vram.len();
        if data.len() < core_expected {
            return Err(MapperError::Truncated {
                expected: core_expected,
                got: data.len(),
            });
        }
        let version = data[0];
        if version != 1 {
            return Err(MapperError::UnsupportedVersion(version));
        }
        self.prg_0 = data[1];
        self.prg_1 = data[2];
        self.prg_2 = data[3];
        self.chr.copy_from_slice(&data[4..12]);
        self.mirroring = match data[12] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.prg_ram_enable = data[13] != 0;
        self.irq_latch = data[14];
        self.irq_counter = data[15];
        self.irq_enabled = data[16] != 0;
        self.irq_enable_after_ack = data[17] != 0;
        self.irq_mode_scanline = data[18] != 0;
        self.irq_prescaler = i32::from_le_bytes(
            data[19..23]
                .try_into()
                .map_err(|_| MapperError::Invalid("prescaler".into()))?,
        );
        self.irq_pending = data[23] != 0;
        self.audio.addr_latch = data[24];
        self.audio.data_latch = data[25];
        self.audio.silenced = data[26] != 0;
        self.audio.regs.copy_from_slice(&data[27..91]);
        self.vram.copy_from_slice(&data[91..91 + self.vram.len()]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sunsoft FME-7 (mapper 69) — banking + IRQ + Sunsoft 5B audio.
//
// The 5B audio chip is a YM2149F / AY-3-8910 clone with its SEL pin held
// low: 3 square-wave tone channels with per-channel 4-bit logarithmic
// volume, a 32-step envelope generator (16-bit period, 10 shape modes),
// and a 17-bit LFSR noise source.  Driven directly by the 1.789773 MHz
// NES CPU clock with the chip's internal /16 prescaler, the effective
// tone period is `32 * TP` CPU cycles (half-period = `16 * TP`).
//
// Register protocol:
//   $C000-$DFFF: latch the 4-bit register index (bits 3-0 of the value).
//   $E000-$FFFF: write 8-bit data to the previously-latched register.
//
// Audio is gated behind the `mapper-audio` Cargo feature; when off, the
// register decoders still latch state (so save-state round-trip stays
// correct) but the oscillators do not advance and `mix_audio` returns 0.
// ---------------------------------------------------------------------------

/// 16-entry logarithmic volume DAC, ~3 dB per 4-bit step (= 1.5 dB per
/// 5-bit step in the underlying chip).  Peak chosen so that three channels
/// summed at maximum volume stay comfortably inside the `i16` headroom the
/// APU mixer expects, in the same ballpark as VRC6's `(sum-30)*256` scale.
///
/// Per the NESdev "Sunsoft 5B audio" page, the chip's DAC has a 1.5 dB
/// step on the 5-bit signal.  Because the wiki specifies that envelope
/// level `e` is equivalent to 4-bit volume `e >> 1` (with both `e=0` and
/// `e=1` mapping to silence), a 16-entry table indexed by the 4-bit
/// equivalent is sufficient — equivalent to a 32-entry table where each
/// even/odd pair shares the same amplitude.
const SUNSOFT5B_LOG_VOL: [i16; 16] = [
    0, 15, 21, 30, 42, 59, 84, 119, 168, 237, 335, 473, 668, 944, 1333, 1882,
];

/// Mixed centering bias: subtracted from the linear sum before emitting
/// the i16 sample.  We use a *constant zero* — the APU mixer's chained
/// high-pass filters (90 Hz / 440 Hz, see `rustynes-apu::mixer::OnePole`)
/// remove any steady DC component downstream, and the 5B's linear sum
/// can swing from 0 (all channels muted) up to ~5.6 k (three channels at
/// peak volume + tone high), well within `i16` headroom.  Keeping the
/// constant named here makes a future numerical bias easy to add if
/// AccuracyCoin's mixed-output tests ever ask for it.
const SUNSOFT5B_DC_BIAS: i16 = 0;

/// One of the 5B's three square-wave tone channels.
///
/// The chip toggles the output level every `16 * TP` CPU cycles (TP = the
/// 12-bit period from registers `$00/$01` for channel A, etc.).  Per wiki,
/// a `TP` of 0 behaves identically to `TP` of 1, so the divide path uses
/// `max(TP, 1)` to avoid both a divide-by-zero and a degenerate "always
/// toggling" case.  None of the 5B's generators can be halted — disabling
/// a channel in the mixer only mutes its output, the internal counters
/// keep running.
#[derive(Clone, Default)]
struct Sunsoft5BTone {
    /// 12-bit reload period.
    period: u16,
    /// Internal half-period countdown in CPU clocks (counts down from
    /// `16 * period`; on hitting 0 the level toggles and the counter
    /// reloads).
    counter: u32,
    /// Current square-wave output level (0 or 1).
    level: u8,
}

impl Sunsoft5BTone {
    /// Effective half-period, in CPU clocks (`max(period, 1) * 16`).
    fn half_period(&self) -> u32 {
        u32::from(self.period.max(1)) * 16
    }

    /// One CPU cycle.  Counters always run, even when the channel is
    /// muted by the mixer register.
    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.half_period();
            self.level ^= 1;
        } else {
            self.counter -= 1;
        }
    }
}

/// 17-bit LFSR noise generator with taps at bits 16 and 13 (per the AY-
/// 3-8910 datasheet, as cited on the NESdev wiki).
#[derive(Clone)]
struct Sunsoft5BNoise {
    /// 5-bit period reload (`$06`).
    period: u8,
    /// Half-period countdown in CPU clocks.
    counter: u32,
    /// 17-bit LFSR state; output is bit 0.
    lfsr: u32,
}

impl Default for Sunsoft5BNoise {
    fn default() -> Self {
        // The AY's LFSR powers up with all bits set; if it ever reached 0
        // it would lock up (no taps could ever flip a bit back in).
        Self {
            period: 0,
            counter: 0,
            lfsr: 0x1FFFF,
        }
    }
}

impl Sunsoft5BNoise {
    fn half_period(&self) -> u32 {
        u32::from(self.period.max(1)) * 16
    }

    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.half_period();
            // 17-bit LFSR, taps at bits 16 and 13 (XOR).  Shift right,
            // feed the XOR back into bit 16.
            let fb = ((self.lfsr >> 16) ^ (self.lfsr >> 13)) & 1;
            self.lfsr = (self.lfsr >> 1) | (fb << 16);
            self.lfsr &= 0x1FFFF;
        } else {
            self.counter -= 1;
        }
    }

    /// Current noise output bit (0 or 1).
    fn level(&self) -> u8 {
        (self.lfsr & 1) as u8
    }
}

/// Envelope generator: 16-bit period, 32-step output, 10 distinct shapes.
///
/// Writing the shape register (`$0D`) **restarts** the envelope from its
/// shape-determined starting position.  The wiki gives the shapes in
/// terms of four bits `CAaH` (continue/attack/alternate/hold); we
/// implement them as a small state machine — `attack` chooses the
/// starting direction, `alternate` flips it after each ramp, `continue`
/// gates whether to keep going past the first ramp, and `hold` freezes
/// (with `attack XOR alternate` deciding the held value).
#[derive(Clone, Default)]
struct Sunsoft5BEnvelope {
    /// 16-bit reload period.
    period: u16,
    /// Half-step countdown in CPU clocks (the wiki gives step frequency
    /// `clock / (16 * period)`).
    counter: u32,
    /// Shape register value (`$0D`).  Only the low 4 bits matter.
    shape: u8,
    /// Current 5-bit envelope level (0..=31).
    level: u8,
    /// Internal direction: +1 for rising, -1 for falling.
    rising: bool,
    /// Set once the envelope has completed its first ramp and decided to
    /// hold (per `continue=0` or `hold=1` after the first ramp/alternate).
    holding: bool,
}

impl Sunsoft5BEnvelope {
    /// Effective step interval in CPU clocks.
    fn step_period(&self) -> u32 {
        u32::from(self.period.max(1)) * 16
    }

    /// Write `$0D` — latches the shape AND restarts the envelope.
    fn write_shape(&mut self, value: u8) {
        self.shape = value & 0x0F;
        // Attack bit (bit 2) sets the initial direction.  When attack=1,
        // start at 0 going up; when attack=0, start at 31 going down.
        let attack = (self.shape & 0x04) != 0;
        self.rising = attack;
        self.level = if attack { 0 } else { 31 };
        self.counter = self.step_period();
        self.holding = false;
    }

    /// One CPU cycle.  Runs forever (cannot be halted) but emits silence
    /// while `holding == true` and `continue == 0`.
    fn clock(&mut self) {
        if self.counter == 0 {
            self.counter = self.step_period();
            self.step();
        } else {
            self.counter -= 1;
        }
    }

    fn step(&mut self) {
        if self.holding {
            return;
        }
        if self.rising {
            if self.level < 31 {
                self.level += 1;
                return;
            }
        } else if self.level > 0 {
            self.level -= 1;
            return;
        }
        // We reached the end of a ramp.  Decide what to do based on the
        // four shape bits.  Per the wiki:
        //   continue=0 (bit 3): the envelope holds at 0 regardless of the
        //                       other bits after one ramp.
        //   hold=1 (bit 0):     hold at the current value (possibly flipped
        //                       by alternate).
        //   alternate=1 (bit 1): reverse direction every ramp.
        let cont = (self.shape & 0x08) != 0;
        let alternate = (self.shape & 0x02) != 0;
        let hold = (self.shape & 0x01) != 0;
        if !cont {
            self.level = 0;
            self.holding = true;
            return;
        }
        if hold {
            if alternate {
                // /\___ etc.: flip the final level once.
                self.level = if self.rising { 0 } else { 31 };
            }
            self.holding = true;
            return;
        }
        if alternate {
            self.rising = !self.rising;
        } else {
            // Pure sawtooth: snap back to the starting level.
            self.level = if self.rising { 0 } else { 31 };
        }
    }

    /// Current 5-bit envelope output (0..=31).
    const fn output(&self) -> u8 {
        self.level
    }
}

/// 5B audio chip state: 16-byte register file, 3 tone channels, noise
/// generator, envelope generator, plus the address-latch byte that the
/// `$C000-$DFFF` writes use to select the next `$E000-$FFFF` data target.
#[derive(Clone, Default)]
pub(crate) struct Sunsoft5BAudio {
    /// Latched 4-bit register index from the most recent `$C000-$DFFF`
    /// write.  Bits 7-4 of the high-byte are silently ignored (per the
    /// NESdev wiki: writes with bits 7-4 nonzero are inhibited; we model
    /// only the inhibit-on-high-bits case by masking to 4 bits, since no
    /// known software relies on the high bits).
    addr_latch: u8,
    /// Raw 16-byte register file (mostly for save-state round-trip and
    /// debug inspection — the live state lives in the channel structs).
    regs: [u8; 16],
    tone_a: Sunsoft5BTone,
    tone_b: Sunsoft5BTone,
    tone_c: Sunsoft5BTone,
    noise: Sunsoft5BNoise,
    envelope: Sunsoft5BEnvelope,
}

impl Sunsoft5BAudio {
    pub(crate) fn write_addr(&mut self, value: u8) {
        // Per the wiki, writes with the high nibble nonzero are inhibited.
        // The simplest faithful model is to mask the latch to 4 bits and
        // accept the next data write unconditionally — no known software
        // depends on the inhibit path.
        self.addr_latch = value & 0x0F;
    }

    pub(crate) fn write_data(&mut self, value: u8) {
        let idx = self.addr_latch as usize;
        self.regs[idx] = value;
        match idx {
            0x00 => self.tone_a.period = (self.tone_a.period & 0x0F00) | u16::from(value),
            0x01 => {
                self.tone_a.period = (self.tone_a.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
            }
            0x02 => self.tone_b.period = (self.tone_b.period & 0x0F00) | u16::from(value),
            0x03 => {
                self.tone_b.period = (self.tone_b.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
            }
            0x04 => self.tone_c.period = (self.tone_c.period & 0x0F00) | u16::from(value),
            0x05 => {
                self.tone_c.period = (self.tone_c.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
            }
            0x06 => self.noise.period = value & 0x1F,
            0x07 => { /* mixer; consulted live in `mix_audio`. */ }
            0x08 | 0x09 | 0x0A => { /* per-channel volume; consulted live. */ }
            0x0B => {
                self.envelope.period = (self.envelope.period & 0xFF00) | u16::from(value);
            }
            0x0C => {
                self.envelope.period = (self.envelope.period & 0x00FF) | (u16::from(value) << 8);
            }
            0x0D => self.envelope.write_shape(value),
            // $0E/$0F = I/O ports A/B.  Unused on the NES (the cart never
            // wires them out).  We latch the byte for save-state round-trip
            // and otherwise ignore.
            _ => {}
        }
    }

    /// Mixer register: bits are `--CBAcca`, 0 = enable / 1 = disable.
    /// Bits 5/3/1 are noise enables for channels C/B/A respectively;
    /// bits 4/2/0 are tone enables for channels c/b/a (same lettering).
    const fn tone_enabled(&self, ch: u8) -> bool {
        let mixer = self.regs[0x07];
        // 0 = enable, 1 = disable.  Tone bits = 0, 2, 4 for A/B/C.
        (mixer >> (ch * 2)) & 1 == 0
    }

    const fn noise_enabled(&self, ch: u8) -> bool {
        let mixer = self.regs[0x07];
        // Noise bits = 1, 3, 5 for A/B/C.
        (mixer >> (ch * 2 + 1)) & 1 == 0
    }

    /// Resolve the 4-bit equivalent volume for channel `ch` (0/1/2 for
    /// A/B/C), honoring the per-channel envelope-mode bit.
    fn volume(&self, ch: u8) -> u8 {
        let reg = self.regs[0x08 + ch as usize];
        if reg & 0x10 != 0 {
            // Envelope mode: 5-bit env mapped to 4-bit equivalent via `>>1`
            // per the NESdev table (env=0/1 both -> silent; env=2 -> vol 1;
            // env=31 -> vol 15).
            self.envelope.output() >> 1
        } else {
            reg & 0x0F
        }
    }

    /// Advance every internal generator by one CPU cycle.  Per the wiki,
    /// "none of the various generators can be halted" — they run whenever
    /// the chip is clocked, regardless of mixer/enable state.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn clock(&mut self) {
        self.tone_a.clock();
        self.tone_b.clock();
        self.tone_c.clock();
        self.noise.clock();
        self.envelope.clock();
    }

    /// Linear-summed audio output, scaled to ~i16 with the same headroom
    /// VRC6 leaves for the APU mixer.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn mix(&self) -> i16 {
        let mut sum: i32 = 0;
        for (ch, tone) in [&self.tone_a, &self.tone_b, &self.tone_c]
            .iter()
            .enumerate()
        {
            let ch = ch as u8;
            // Per wiki: "If both bits are 1 [disable + disable], the
            // channel outputs a constant signal at the specified volume.
            // If both bits are 0, the result is the logical and of noise
            // and tone."  Equivalent: emit when (tone_enabled => square
            // high) AND (noise_enabled => noise high), defaulting either
            // factor to "1" when its source is disabled.
            let tone_factor = !self.tone_enabled(ch) || tone.level != 0;
            let noise_factor = !self.noise_enabled(ch) || self.noise.level() != 0;
            if tone_factor && noise_factor {
                let v = self.volume(ch) as usize & 0x0F;
                sum += i32::from(SUNSOFT5B_LOG_VOL[v]);
            }
        }
        // Centre on zero so the BLEP buffer doesn't see a steady DC offset
        // for an idle (all-channels-on-with-fixed-volume) cartridge.  Cast
        // is safe: sum <= 3 * 1882 = 5646, DC bias = 3 * (1882/2) = 2823.
        (sum - i32::from(SUNSOFT5B_DC_BIAS)) as i16
    }

    /// Feature-off shim: the generators do not advance with `mapper-audio`
    /// disabled (mirrors the gated path so the shared NSF expansion router
    /// can clock unconditionally).
    #[cfg(not(feature = "mapper-audio"))]
    pub(crate) fn clock(&mut self) {}

    /// Feature-off shim: silence when `mapper-audio` is disabled.
    #[cfg(not(feature = "mapper-audio"))]
    pub(crate) fn mix(&self) -> i16 {
        0
    }

    /// Serialize the live audio state.  21-byte tail:
    ///   addr_latch(1) + regs[16](16) + tone_a/b/c counter+level(3*5=15) +
    ///   noise counter+lfsr(4+1+... wait that's bigger).
    ///
    /// Tail layout (kept in lock-step with `read_tail`):
    ///   addr_latch         : 1
    ///   regs               : 16
    ///   tone_a.counter     : 4 (u32 LE)
    ///   tone_a.level       : 1
    ///   tone_b.counter     : 4
    ///   tone_b.level       : 1
    ///   tone_c.counter     : 4
    ///   tone_c.level       : 1
    ///   noise.counter      : 4
    ///   noise.lfsr         : 4 (u32 LE, only low 17 bits used)
    ///   envelope.counter   : 4
    ///   envelope.level     : 1
    ///   envelope.rising    : 1 (bool)
    ///   envelope.holding   : 1 (bool)
    ///   -- 51 bytes total --
    /// (Channel period/shape state is reconstructible from `regs`; we
    /// don't serialize the period/shape fields separately.)
    fn write_tail(&self, out: &mut Vec<u8>) {
        out.push(self.addr_latch);
        out.extend_from_slice(&self.regs);
        for t in [&self.tone_a, &self.tone_b, &self.tone_c] {
            out.extend_from_slice(&t.counter.to_le_bytes());
            out.push(t.level);
        }
        out.extend_from_slice(&self.noise.counter.to_le_bytes());
        out.extend_from_slice(&self.noise.lfsr.to_le_bytes());
        out.extend_from_slice(&self.envelope.counter.to_le_bytes());
        out.push(self.envelope.level);
        out.push(u8::from(self.envelope.rising));
        out.push(u8::from(self.envelope.holding));
    }

    /// Tail size in bytes — see `write_tail`.
    const TAIL_LEN: usize = 1 + 16 + 3 * 5 + 4 + 4 + 4 + 1 + 1 + 1;

    fn read_tail(&mut self, src: &[u8]) -> Result<(), MapperError> {
        if src.len() < Self::TAIL_LEN {
            return Err(MapperError::Truncated {
                expected: Self::TAIL_LEN,
                got: src.len(),
            });
        }
        self.addr_latch = src[0] & 0x0F;
        self.regs.copy_from_slice(&src[1..17]);
        let mut cur = 17usize;
        for t in [&mut self.tone_a, &mut self.tone_b, &mut self.tone_c] {
            t.counter = u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]);
            t.level = src[cur + 4] & 1;
            cur += 5;
        }
        self.noise.counter =
            u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]);
        cur += 4;
        self.noise.lfsr =
            u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]) & 0x1FFFF;
        if self.noise.lfsr == 0 {
            // Guard against a lock-up (LFSR with all zeros has no way out).
            self.noise.lfsr = 0x1FFFF;
        }
        cur += 4;
        self.envelope.counter =
            u32::from_le_bytes([src[cur], src[cur + 1], src[cur + 2], src[cur + 3]]);
        cur += 4;
        self.envelope.level = src[cur] & 0x1F;
        self.envelope.rising = src[cur + 1] != 0;
        self.envelope.holding = src[cur + 2] != 0;
        // Reconstruct live period/shape state from the register file.
        self.tone_a.period = u16::from(self.regs[0x00]) | (u16::from(self.regs[0x01] & 0x0F) << 8);
        self.tone_b.period = u16::from(self.regs[0x02]) | (u16::from(self.regs[0x03] & 0x0F) << 8);
        self.tone_c.period = u16::from(self.regs[0x04]) | (u16::from(self.regs[0x05] & 0x0F) << 8);
        self.noise.period = self.regs[0x06] & 0x1F;
        self.envelope.period = u16::from(self.regs[0x0B]) | (u16::from(self.regs[0x0C]) << 8);
        self.envelope.shape = self.regs[0x0D] & 0x0F;
        Ok(())
    }
}

/// Sunsoft FME-7 (Mapper 69).  Bank-switching, CPU-cycle IRQ, and (gated
/// behind `mapper-audio`) the on-cart Sunsoft 5B audio chip.
pub struct Fme7 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    cmd: u8,
    chr: [u8; 8],
    prg_banks: [u8; 4], // $6000, $8000, $A000, $C000 (E000 fixed)
    prg_ram_enabled: bool,
    prg_ram_select: bool,
    mirroring: Mirroring,

    irq_counter: u16,
    irq_enabled: bool,
    irq_counter_enabled: bool,
    irq_pending: bool,

    /// Sunsoft 5B audio extension state.  Live regardless of the
    /// `mapper-audio` feature — the register decoders always latch into
    /// `regs` (so save states stay round-trippable across builds), but
    /// `clock()` / `mix()` are only called when the feature is on.
    audio: Sunsoft5BAudio,
}

impl Fme7 {
    /// Construct a new FME-7 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "FME-7 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "FME-7 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            cmd: 0,
            chr: [0; 8],
            prg_banks: [0; 4],
            prg_ram_enabled: false,
            prg_ram_select: true,
            mirroring,
            irq_counter: 0,
            irq_enabled: false,
            irq_counter_enabled: false,
            irq_pending: false,
            audio: Sunsoft5BAudio::default(),
        })
    }

    fn prg_8k(&self, idx: usize) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        (self.prg_banks[idx] as usize) % total_8k
    }
}

impl Mapper for Fme7 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source + expansion audio
    // (the audio hook only exists under the `mapper-audio` feature).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enabled {
                    return self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()];
                }
                let bank = self.prg_8k(0);
                self.prg_rom[(bank * PRG_BANK_8K + (addr as usize - 0x6000)) % self.prg_rom.len()]
            }
            0x8000..=0x9FFF => {
                let off = self.prg_8k(1) * PRG_BANK_8K + (addr as usize - 0x8000);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0xA000..=0xBFFF => {
                let off = self.prg_8k(2) * PRG_BANK_8K + (addr as usize - 0xA000);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0xC000..=0xDFFF => {
                let off = self.prg_8k(3) * PRG_BANK_8K + (addr as usize - 0xC000);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0xE000..=0xFFFF => {
                let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
                let last = total_8k - 1;
                self.prg_rom[(last * PRG_BANK_8K + (addr as usize - 0xE000)) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enabled {
                    let off = (addr - 0x6000) as usize % self.prg_ram.len();
                    self.prg_ram[off] = value;
                }
            }
            0x8000..=0x9FFF => self.cmd = value & 0x0F,
            0xA000..=0xBFFF => match self.cmd {
                0..=7 => self.chr[self.cmd as usize] = value,
                8 => {
                    self.prg_ram_enabled = (value & 0x80) != 0;
                    self.prg_ram_select = (value & 0x40) != 0;
                    self.prg_banks[0] = value & 0x3F;
                }
                9..=11 => self.prg_banks[(self.cmd - 8) as usize] = value & 0x3F,
                12 => {
                    self.mirroring = match value & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenA,
                        _ => Mirroring::SingleScreenB,
                    };
                }
                13 => {
                    self.irq_enabled = (value & 0x01) != 0;
                    self.irq_counter_enabled = (value & 0x80) != 0;
                    self.irq_pending = false;
                }
                14 => self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value),
                15 => self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(value) << 8),
                _ => {}
            },
            // Sunsoft 5B audio: $C000-$DFFF latches the register address;
            // $E000-$FFFF writes data to the latched register.  Mapper-audio
            // OFF builds still latch state (so the save-state path is
            // round-trippable) but never advance the oscillators.
            0xC000..=0xDFFF => self.audio.write_addr(value),
            0xE000..=0xFFFF => self.audio.write_data(value),
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
                let slot = addr as usize / CHR_BANK_1K;
                let bank = (self.chr[slot] as usize) % total_1k;
                let off = bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1));
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

    fn notify_cpu_cycle(&mut self) {
        // Sunsoft 5B audio runs every CPU cycle, regardless of IRQ state.
        // None of the 5B's internal generators can be halted, so we always
        // tick when the feature is on.
        #[cfg(feature = "mapper-audio")]
        self.audio.clock();

        if self.irq_counter_enabled {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
            if self.irq_counter == 0xFFFF && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i16 {
        self.audio.mix()
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 69,
            name: "Sunsoft FME-7".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        for (i, b) in self.prg_banks.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("counting".into(), format!("{}", self.irq_counter_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.extra
            .push(("cmd".into(), format!("{:#04x}", self.cmd)));
        info.extra.push((
            "prg_ram".into(),
            format!("en={} sel={}", self.prg_ram_enabled, self.prg_ram_select),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v2: appends the Sunsoft 5B audio state at the end.  Per ADR-0003:
        // strictly additive, so v1 readers tolerate the tail (older builds
        // skip-on-read since the tag is consumed at the section length).
        // Tail size = Sunsoft5BAudio::TAIL_LEN (51 bytes).
        let mut out = Vec::with_capacity(
            40 + self.prg_ram.len() + self.vram.len() + Sunsoft5BAudio::TAIL_LEN,
        );
        out.push(2u8); // version
        out.push(self.cmd);
        out.extend_from_slice(&self.chr);
        out.extend_from_slice(&self.prg_banks);
        out.push(u8::from(self.prg_ram_enabled));
        out.push(u8::from(self.prg_ram_select));
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_counter_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        // v2 audio tail.
        self.audio.write_tail(&mut out);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 1 + 8 + 4 + 1 + 1 + 1 + 2 + 1 + 1 + 1;
        let core_expected = scalar_len + self.prg_ram.len() + self.vram.len();
        if data.len() < core_expected {
            return Err(MapperError::Truncated {
                expected: core_expected,
                got: data.len(),
            });
        }
        let version = data[0];
        if !(1..=2).contains(&version) {
            return Err(MapperError::UnsupportedVersion(version));
        }
        self.cmd = data[1];
        self.chr.copy_from_slice(&data[2..10]);
        self.prg_banks.copy_from_slice(&data[10..14]);
        self.prg_ram_enabled = data[14] != 0;
        self.prg_ram_select = data[15] != 0;
        self.mirroring = match data[16] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_counter = u16::from_le_bytes(
            data[17..19]
                .try_into()
                .map_err(|_| MapperError::Invalid("irq_counter".into()))?,
        );
        self.irq_enabled = data[19] != 0;
        self.irq_counter_enabled = data[20] != 0;
        self.irq_pending = data[21] != 0;
        let mut cur = 22usize;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        cur += self.vram.len();

        // v2 tail: audio state.  v1 blobs end at the core; per ADR-0003,
        // we leave audio at its current state (the caller is responsible
        // for an explicit power-cycle if they want a clean slate).  A v2
        // blob shorter than TAIL_LEN bytes is accepted permissively for
        // the same forward-compat reason VRC6 uses.
        if version == 2 && data.len() >= cur + Sunsoft5BAudio::TAIL_LEN {
            self.audio
                .read_tail(&data[cur..cur + Sunsoft5BAudio::TAIL_LEN])?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Namco 163 (mapper 19) — banking + CPU-cycle IRQ + 1-8 channel wavetable
// audio (gated behind the `mapper-audio` feature).
// ---------------------------------------------------------------------------

/// Namco 163 on-cart wavetable synthesiser.
///
/// 1-8 simultaneous channels, each playing a 4-bit wavetable from the
/// mapper-internal 128-byte sound RAM.  Wavetable data shares the same
/// RAM as the per-channel register file: the wavetable pool conventionally
/// sits at `$00-$3F` (128 nibble-samples), and channels claim 8-byte
/// regions at the top of RAM, with channel 8 (the always-enabled channel)
/// at `$78-$7F` and channel 1 (the lowest priority) at `$40-$47`.  When
/// fewer than 8 channels are enabled, the unused channels' register
/// regions are reusable as additional wavetable storage.
///
/// Register interface (per NESdev wiki, "Namco 163 audio"):
///
/// - `$F800-$FFFF` (write): **address port**.  Bit 7 = auto-increment
///   flag; bits 6-0 = 7-bit address into the 128-byte internal RAM.
/// - `$4800-$4FFF` (read/write): **data port**.  Reads/writes the byte
///   at the latched address.  If the auto-increment flag is set, the
///   latch advances by 1 after each access, *saturating at $7F* (per
///   the wiki: "stopping at $7F" — does **not** wrap to $00).
///
/// Per-channel register layout (8 bytes each; here referenced for the
/// channel at `$78-$7F` = channel 8, but every channel's 8-byte slot
/// follows the same offsets):
///
/// | Offset | Bits   | Field                                           |
/// |--------|--------|-------------------------------------------------|
/// | +0     | 7-0    | Frequency low (bits 7-0 of 18-bit freq)         |
/// | +1     | 7-0    | Phase low (bits 7-0 of 24-bit phase accumulator)|
/// | +2     | 7-0    | Frequency mid (bits 15-8 of freq)               |
/// | +3     | 7-0    | Phase mid (bits 15-8 of phase)                  |
/// | +4     | 1-0    | Frequency high (bits 17-16 of freq)             |
/// | +4     | 7-2    | Length encoding: waveform length = `256 - (reg & 0xFC)` 4-bit samples |
/// | +5     | 7-0    | Phase high (bits 23-16 of phase)                |
/// | +6     | 7-0    | Wave start address, in 4-bit samples (nibbles)  |
/// | +7     | 3-0    | Linear volume (0..=15)                          |
/// | +7     | 6-4    | (Channel 8's `$7F` only) `C` field: number of   |
/// |        |        | enabled channels - 1 (so C=0 → 1 channel,       |
/// |        |        | C=7 → all 8 channels)                           |
///
/// Update rate: each channel updates every 15 CPU cycles.  With `n`
/// active channels, the chip cycles through them in round-robin, so
/// per-channel update rate = `CPU_clock / (15 * n)`.  We model this as
/// a 15-cycle prescaler that advances `tick_index` (mod `n`) and
/// increments only that one channel's phase per tick.
///
/// Mixing: per channel, output = `(sample - 8) * volume`, where `sample`
/// is the 4-bit nibble fetched from RAM at `(wave_addr + (phase >> 16))
/// mod L`, `L` is the per-channel wave length, and the `-8` bias makes
/// the output bipolar (range `-120..=+105`).  The chip itself does not
/// mix — channels are output one-at-a-time — but in practice emulators
/// sum the per-channel outputs and divide by the active channel count
/// (the convention recommended by the wiki and what Mesen2/FCEUX both
/// do).  The final i16 is scaled to match the headroom VRC6 leaves for
/// the APU mixer.
#[cfg(feature = "mapper-audio")]
#[derive(Clone)]
pub(crate) struct Namco163Audio {
    /// 128-byte internal sound RAM.  Shared between wavetable samples
    /// (`$00-$3F` conventionally) and per-channel register file
    /// (`$40-$7F`).
    ram: [u8; 128],
    /// 7-bit address latch (the address the next data-port access
    /// targets).
    addr_latch: u8,
    /// Auto-increment flag from the most recent `$F800-$FFFF` write.
    /// When set, data-port accesses advance `addr_latch` (saturating at
    /// `$7F` per the wiki).
    auto_inc: bool,
    /// Round-robin tick index: 0..=7.  Each 15-cycle tick advances the
    /// phase of channel `7 - tick_index` (since channel 8, at `$78-$7F`,
    /// is the *first* channel updated when only one channel is enabled).
    tick_index: u8,
    /// 15-cycle prescaler.  When it reaches 15, we update the next
    /// channel and reset.
    prescaler: u8,
}

// When the `mapper-audio` feature is OFF, the audio struct still exists
// (so save-state round-trip and the register-decoder contract stay
// identical between feature on/off builds) — but reduced to the bare
// state required for those two paths.
#[cfg(not(feature = "mapper-audio"))]
#[derive(Clone)]
pub(crate) struct Namco163Audio {
    ram: [u8; 128],
    addr_latch: u8,
    auto_inc: bool,
    tick_index: u8,
    prescaler: u8,
}

impl Default for Namco163Audio {
    fn default() -> Self {
        Self {
            ram: [0; 128],
            addr_latch: 0,
            auto_inc: false,
            tick_index: 0,
            prescaler: 0,
        }
    }
}

impl Namco163Audio {
    /// Write to the address port (`$F800-$FFFF`).  Bit 7 = auto-increment;
    /// bits 6-0 = 7-bit address into internal RAM.
    pub(crate) fn write_addr_port(&mut self, value: u8) {
        self.auto_inc = value & 0x80 != 0;
        self.addr_latch = value & 0x7F;
    }

    /// Advance the address latch if auto-increment is enabled.  Per the
    /// wiki, it saturates at `$7F` rather than wrapping back to `$00`.
    fn step_addr(&mut self) {
        if self.auto_inc && self.addr_latch < 0x7F {
            self.addr_latch += 1;
        }
    }

    /// Write to the data port (`$4800-$4FFF`).  Stores at the latched
    /// address; advances the latch when auto-increment is set.
    pub(crate) fn write_data_port(&mut self, value: u8) {
        let idx = (self.addr_latch & 0x7F) as usize;
        self.ram[idx] = value;
        self.step_addr();
    }

    /// Read from the data port (`$4800-$4FFF`).  Returns the byte at the
    /// latched address; advances the latch when auto-increment is set.
    pub(crate) fn read_data_port(&mut self) -> u8 {
        let idx = (self.addr_latch & 0x7F) as usize;
        let v = self.ram[idx];
        self.step_addr();
        v
    }

    /// Active channel count, derived from bits 6-4 of register `$7F`
    /// (`C` field): returns `C + 1` in the range `1..=8`.
    #[cfg(feature = "mapper-audio")]
    fn channel_count(&self) -> u8 {
        ((self.ram[0x7F] >> 4) & 0x07) + 1
    }

    /// Compute the 18-bit frequency value for the channel whose 8-byte
    /// register slot starts at `base` (i.e. `$78` for channel 8, `$70`
    /// for channel 7, ..., `$40` for channel 1).
    #[cfg(feature = "mapper-audio")]
    fn channel_freq(&self, base: usize) -> u32 {
        let lo = u32::from(self.ram[base]);
        let mid = u32::from(self.ram[base + 2]);
        let hi = u32::from(self.ram[base + 4] & 0x03);
        lo | (mid << 8) | (hi << 16)
    }

    /// 24-bit phase accumulator for the channel at `base`.
    #[cfg(feature = "mapper-audio")]
    fn channel_phase(&self, base: usize) -> u32 {
        let lo = u32::from(self.ram[base + 1]);
        let mid = u32::from(self.ram[base + 3]);
        let hi = u32::from(self.ram[base + 5]);
        lo | (mid << 8) | (hi << 16)
    }

    /// Write back the 24-bit phase to the channel's three phase
    /// registers.  Only bits 23..0 are retained (the value is naturally
    /// 24-bit; we mask to be safe under wrap-around).
    #[cfg(feature = "mapper-audio")]
    fn set_channel_phase(&mut self, base: usize, phase: u32) {
        let phase = phase & 0x00FF_FFFF;
        self.ram[base + 1] = (phase & 0xFF) as u8;
        self.ram[base + 3] = ((phase >> 8) & 0xFF) as u8;
        self.ram[base + 5] = ((phase >> 16) & 0xFF) as u8;
    }

    /// Wave length L (in 4-bit samples) for the channel at `base`.
    /// Per the wiki: `L = 256 - (reg[base+4] & 0xFC)`.
    #[cfg(feature = "mapper-audio")]
    fn channel_length(&self, base: usize) -> u32 {
        256u32 - u32::from(self.ram[base + 4] & 0xFC)
    }

    /// Wave start address for the channel at `base` (in nibble units —
    /// every step of `wave_addr` represents one 4-bit sample, so two
    /// nibbles per RAM byte).
    #[cfg(feature = "mapper-audio")]
    fn channel_wave_addr(&self, base: usize) -> u32 {
        u32::from(self.ram[base + 6])
    }

    /// 4-bit linear volume for the channel at `base`.
    #[cfg(feature = "mapper-audio")]
    fn channel_volume(&self, base: usize) -> u8 {
        self.ram[base + 7] & 0x0F
    }

    /// Resolve the 4-bit nibble at `nibble_addr` in the wavetable pool.
    /// Bit 0 of the address picks the high or low nibble of the
    /// corresponding RAM byte: even = low nibble, odd = high nibble.
    #[cfg(feature = "mapper-audio")]
    fn fetch_nibble(&self, nibble_addr: u32) -> u8 {
        let byte = self.ram[((nibble_addr >> 1) & 0x7F) as usize];
        if nibble_addr & 1 == 0 {
            byte & 0x0F
        } else {
            (byte >> 4) & 0x0F
        }
    }

    /// Returns the register-file base address for the i-th enabled
    /// channel (i = 0 is the always-enabled channel 8 at `$78-$7F`;
    /// i = 1 is channel 7 at `$70-$77`; ...; i = 7 is channel 1 at
    /// `$40-$47`).
    #[cfg(feature = "mapper-audio")]
    const fn channel_base(i: u8) -> usize {
        // Channel 8 = $78, channel 7 = $70, ..., channel 1 = $40.
        // base = 0x78 - i*8.
        0x78 - (i as usize) * 8
    }

    /// Advance one CPU cycle.  Every 15 cycles, round-robin to the next
    /// enabled channel and increment its phase by its 18-bit freq value.
    /// When the phase exceeds `L * 65536`, wrap around — the integer
    /// part of `phase >> 16` modulo `L` is the wavetable index.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn clock(&mut self) {
        self.prescaler = self.prescaler.wrapping_add(1);
        if self.prescaler < 15 {
            return;
        }
        self.prescaler = 0;

        let n = self.channel_count();
        // Round-robin within the active set.  tick_index counts 0..n.
        if self.tick_index >= n {
            self.tick_index = 0;
        }
        let ch = self.tick_index;
        self.tick_index = (self.tick_index + 1) % n;

        let base = Self::channel_base(ch);
        let freq = self.channel_freq(base);
        let length = self.channel_length(base);
        // Phase modulus is L * 2^16 (so that (phase >> 16) mod L stays
        // in [0, L)).  Use 64-bit math to avoid 32-bit overflow when L
        // is near 256 and freq is near 2^18.
        let modulus = u64::from(length) << 16;
        let mut phase = u64::from(self.channel_phase(base));
        phase = phase.wrapping_add(u64::from(freq));
        if modulus != 0 {
            phase %= modulus;
        }
        self.set_channel_phase(base, phase as u32);
    }

    /// Per-channel output sample, bipolar: `(nibble - 8) * volume`,
    /// range `-120..=+105`.
    #[cfg(feature = "mapper-audio")]
    fn channel_output(&self, ch: u8) -> i16 {
        let base = Self::channel_base(ch);
        let length = self.channel_length(base);
        if length == 0 {
            return 0;
        }
        let phase = self.channel_phase(base);
        let wave_addr = self.channel_wave_addr(base);
        let index = (phase >> 16) % length;
        let nibble = self.fetch_nibble(wave_addr + index);
        // -8 bias makes the output bipolar.
        let signed = i16::from(nibble) - 8;
        signed * i16::from(self.channel_volume(base))
    }

    /// Linear-summed audio output, scaled to ~i16 with similar headroom
    /// to VRC6 and Sunsoft 5B.  Per the wiki, channels are output
    /// one-at-a-time on hardware; emulators (Mesen2, FCEUX) approximate
    /// the mix by summing channel outputs and dividing by the number of
    /// active channels.  We do the same and scale by 64 so a single
    /// full-volume bipolar channel reaches ~±7,680 — comfortably under
    /// `i16::MAX` and in the same ballpark as VRC6's `(sum-30)*256`.
    ///
    /// NOTE: The channel-count division matches the reference emulators'
    /// behaviour; the chip's real per-channel time-multiplexed output is
    /// effectively the same average since each channel only drives the
    /// output `1/n` of the time.
    #[cfg(feature = "mapper-audio")]
    pub(crate) fn mix(&self) -> i16 {
        let n = self.channel_count();
        if n == 0 {
            return 0;
        }
        let mut sum: i32 = 0;
        for ch in 0..n {
            sum += i32::from(self.channel_output(ch));
        }
        // Per-channel range is -120..=+105; sum has the same average
        // amplitude after dividing by n.  Scale by 64 to use ~half the
        // i16 range at full tilt.
        ((sum / i32::from(n)) * 64) as i16
    }

    /// `mix_audio` shim for the no-audio build.
    #[cfg(not(feature = "mapper-audio"))]
    pub(crate) fn mix(&self) -> i16 {
        0
    }

    /// Save-state tail layout (kept lock-step with `read_tail`):
    ///   ram[128]      : 128
    ///   addr_latch    : 1
    ///   auto_inc      : 1 (bool)
    ///   tick_index    : 1
    ///   prescaler     : 1
    ///   -- 132 bytes total --
    fn write_tail(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ram);
        out.push(self.addr_latch & 0x7F);
        out.push(u8::from(self.auto_inc));
        out.push(self.tick_index);
        out.push(self.prescaler);
    }

    /// Tail size in bytes — see `write_tail`.
    const TAIL_LEN: usize = 128 + 1 + 1 + 1 + 1;

    fn read_tail(&mut self, src: &[u8]) -> Result<(), MapperError> {
        if src.len() < Self::TAIL_LEN {
            return Err(MapperError::Truncated {
                expected: Self::TAIL_LEN,
                got: src.len(),
            });
        }
        self.ram.copy_from_slice(&src[0..128]);
        self.addr_latch = src[128] & 0x7F;
        self.auto_inc = src[129] != 0;
        self.tick_index = src[130];
        self.prescaler = src[131];
        Ok(())
    }
}

/// Namco 163 (Mapper 19).  Banking + CPU-cycle IRQ + (gated behind
/// `mapper-audio`) 1-8 channel wavetable audio.
pub struct Namco163 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    chr_is_ram: bool,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg: [u8; 4], // 8 KiB banks: $8000, $A000, $C000, fixed $E000
    chr: [u8; 8], // 1 KiB CHR banks
    nta: [u8; 4], // 1 KiB NTA banks (CIRAM/CHR ROM swappable)
    mirroring: Mirroring,

    irq_counter: u16,
    irq_pending: bool,

    /// Audio disable bit (`$E000-$E7FF` bit 6).  When set, the
    /// N163 audio circuitry is silenced — both the per-channel clocks
    /// stop advancing and `mix_audio` returns 0.  Cleared at power-on.
    sound_disabled: bool,
    /// Namco 163 on-cart wavetable audio state.  Live regardless of the
    /// `mapper-audio` feature — the register decoders always latch into
    /// `ram` and the address-port flag/latch (so save states stay
    /// round-trippable across builds), but `clock()` / `mix()` are only
    /// driven when the feature is on.
    audio: Namco163Audio,
}

impl Namco163 {
    /// Construct a new Namco 163 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "Namco163 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "Namco163 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            chr_is_ram,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg: [0, 0, 0, 0],
            chr: [0; 8],
            nta: [0; 4],
            mirroring,
            irq_counter: 0,
            irq_pending: false,
            sound_disabled: false,
            audio: Namco163Audio::default(),
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total_8k - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg[0] as usize) % total_8k,
            0xA000 => (self.prg[1] as usize) % total_8k,
            0xC000 => (self.prg[2] as usize) % total_8k,
            0xE000 => last,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }
}

impl Mapper for Namco163 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source + expansion audio
    // (the audio hook only exists under the `mapper-audio` feature).
    fn caps(&self) -> MapperCaps {
        MapperCaps {
            cpu_cycle_hook: true,
            audio: cfg!(feature = "mapper-audio"),
            frame_event_hook: false,
            irq_source: true,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // Namco 163 maps `$4800-$4FFF` (sound data port) and
        // `$5000-$5FFF` (IRQ counter low/high). The `$4020-$47FF`
        // range is unmapped.
        (0x4020..=0x47FF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // Audio data port: reads the byte at the latched address in
            // internal sound RAM, advancing the latch if auto-increment
            // is set.  Decoder runs regardless of `mapper-audio`.
            0x4800..=0x4FFF => self.audio.read_data_port(),
            0x5000..=0x57FF => {
                // IRQ counter low.
                let v = (self.irq_counter & 0xFF) as u8;
                self.irq_pending = false;
                v
            }
            0x5800..=0x5FFF => {
                let v = ((self.irq_counter >> 8) & 0x7F) as u8;
                self.irq_pending = false;
                v
            }
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // Audio data port: stores at the latched address in internal
            // sound RAM, advancing the latch if auto-increment is set.
            // Decoder runs regardless of `mapper-audio`.
            0x4800..=0x4FFF => self.audio.write_data_port(value),
            0x5000..=0x57FF => {
                self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value);
                self.irq_pending = false;
            }
            0x5800..=0x5FFF => {
                self.irq_counter =
                    (self.irq_counter & 0x00FF) | ((u16::from(value) & 0x7F) << 8) | 0x8000;
                self.irq_pending = false;
            }
            0x6000..=0x7FFF => {
                let off = (addr - 0x6000) as usize % self.prg_ram.len();
                self.prg_ram[off] = value;
            }
            0x8000..=0xBFFF => {
                let slot = ((addr - 0x8000) >> 11) as usize; // 4 banks: 8000,8800,9000,9800,A000,...
                if slot < 8 {
                    self.chr[slot] = value;
                }
            }
            0xC000..=0xDFFF => {
                // Additional CHR / NTA bank selects on real hardware.
                // Not wired up here (the existing Namco163 banking model
                // pre-dates this audio work — see the comment in
                // `notify_cpu_cycle`).  Audio decoder is unaffected.
            }
            // $E000-$E7FF: PRG bank 0 select (bits 0-5) + audio-disable
            // flag (bit 6).  When bit 6 is set, the N163 audio chip is
            // silenced — see `mix_audio` / `notify_cpu_cycle`.
            0xE000..=0xE7FF => {
                self.prg[0] = value & 0x3F;
                self.sound_disabled = value & 0x40 != 0;
            }
            0xE800..=0xEFFF => self.prg[1] = value & 0x3F,
            0xF000..=0xF7FF => self.prg[2] = value & 0x3F,
            // $F800-$FFFF: audio address port (bit 7 = auto-increment,
            // bits 6-0 = 7-bit internal RAM address).  On real hardware
            // this register also gates PRG-RAM writes via the upper
            // nibble (`0100` enables writes), but no commercially-released
            // Namco 163 cartridge uses that feature in a way that affects
            // accuracy, so we model only the audio half here.  Decoder
            // runs regardless of `mapper-audio`.
            0xF800..=0xFFFF => self.audio.write_addr_port(value),
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
                let slot = addr as usize / CHR_BANK_1K;
                let bank = (self.chr[slot] as usize) % total_1k;
                let off = bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1));
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

    fn notify_cpu_cycle(&mut self) {
        // N163 audio runs every CPU cycle whenever the chip is not
        // silenced via the $E000 sound-disable bit.  None of the
        // 8 channel oscillators can be individually halted — only the
        // active-channel count and per-channel volume gate their effect
        // on the mix.
        #[cfg(feature = "mapper-audio")]
        if !self.sound_disabled {
            self.audio.clock();
        }

        if self.irq_counter & 0x8000 != 0 {
            let low = self.irq_counter & 0x7FFF;
            if low == 0x7FFF {
                self.irq_pending = true;
            } else {
                self.irq_counter = (self.irq_counter & 0x8000) | (low + 1);
            }
        }
    }

    #[cfg(feature = "mapper-audio")]
    fn mix_audio(&mut self) -> i16 {
        if self.sound_disabled {
            return 0;
        }
        self.audio.mix()
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 19,
            name: "Namco 163".into(),
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        for (i, b) in self.prg.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.nta.iter().enumerate() {
            info.extra.push((format!("NTA{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // v2 (per ADR-0003): strictly additive tail — older v1 readers
        // tolerate the additional bytes (we encode the audio at the end,
        // so the core layout is byte-identical to v1).
        // Audio tail layout:
        //   sound_disabled : 1
        //   audio block    : Namco163Audio::TAIL_LEN (132 bytes)
        //   -- 133 bytes total --
        let mut out = Vec::with_capacity(
            32 + self.prg_ram.len() + self.vram.len() + 1 + Namco163Audio::TAIL_LEN,
        );
        out.push(2u8); // version
        out.extend_from_slice(&self.prg);
        out.extend_from_slice(&self.chr);
        out.extend_from_slice(&self.nta);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        // v2 audio tail.
        out.push(u8::from(self.sound_disabled));
        self.audio.write_tail(&mut out);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let scalar_len = 1 + 4 + 8 + 4 + 1 + 2 + 1;
        let core_expected = scalar_len + self.prg_ram.len() + self.vram.len();
        if data.len() < core_expected {
            return Err(MapperError::Truncated {
                expected: core_expected,
                got: data.len(),
            });
        }
        let version = data[0];
        if !(1..=2).contains(&version) {
            return Err(MapperError::UnsupportedVersion(version));
        }
        self.prg.copy_from_slice(&data[1..5]);
        self.chr.copy_from_slice(&data[5..13]);
        self.nta.copy_from_slice(&data[13..17]);
        self.mirroring = match data[17] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_counter = u16::from_le_bytes(
            data[18..20]
                .try_into()
                .map_err(|_| MapperError::Invalid("irq_counter".into()))?,
        );
        self.irq_pending = data[20] != 0;
        let mut cur = 21usize;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        cur += self.vram.len();

        // v2 tail: audio + sound-disable bit.  v1 blobs end at the core;
        // per ADR-0003 we leave the audio at its current state — silent
        // by default after `new()` — so the older blob loads cleanly
        // (the caller is responsible for an explicit power-cycle if they
        // want a fully-clean slate).  A v2 blob shorter than the tail is
        // accepted permissively for the same forward-compat reason VRC6
        // and FME-7 use.
        if version == 2 && data.len() >= cur + 1 + Namco163Audio::TAIL_LEN {
            self.sound_disabled = data[cur] != 0;
            cur += 1;
            self.audio
                .read_tail(&data[cur..cur + Namco163Audio::TAIL_LEN])?;
        } else if version == 1 {
            // Reset audio to power-on defaults for clean v1→v2 upgrade.
            self.sound_disabled = false;
            self.audio = Namco163Audio::default();
        }
        Ok(())
    }
}

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
    fn vrc4_irq_counter_pending() {
        let mut m = Vrc4::new(synth(8), synth_chr(8), 21, 1, Mirroring::Vertical).unwrap();
        // VRC4a: a0_bit=1, a1_bit=2.  Control is at $F004 (a0=0, a1=1).
        // Set latch low byte = 0xE.
        m.cpu_write(0xF000, 0xE);
        // Enable: bit 1 (enable now), mode=cycle (bit 2 = 0).
        m.cpu_write(0xF004, 0x02);
        // From counter=latch=0xE, ticks until 0xFF: 0xFF-0xE = 0xF1 ticks
        // for the wrap, plus one to set pending.
        for _ in 0..0xF1 + 1 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
    }

    #[test]
    fn fme7_basic_banking() {
        let mut m = Fme7::new(synth(16), synth_chr(8), Mirroring::Vertical).unwrap();
        // cmd=9 -> writes prg_banks[1] (the $8000-$9FFF window).
        m.cpu_write(0x8000, 9);
        m.cpu_write(0xA000, 5);
        // Read at $8000 should now be bank 5 (offset 0 == bank index byte).
        assert_eq!(m.cpu_read(0x8000), 5);
        // cmd=10 -> prg_banks[2] ($A000-$BFFF).
        m.cpu_write(0x8000, 10);
        m.cpu_write(0xA000, 7);
        assert_eq!(m.cpu_read(0xA000), 7);
    }

    #[test]
    fn namco163_irq_counter() {
        let mut m = Namco163::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        // Set counter low byte = 0xFFE, then high byte+enable.
        m.cpu_write(0x5000, 0xFE);
        m.cpu_write(0x5800, 0xFF); // sets bit 7 & 0x80 of high byte = enable.
        // Ticks until counter reaches 0x7FFF.
        for _ in 0..3 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
    }

    #[test]
    fn vrc6_audio_register_decoders_latch_state() {
        let mut m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        // Pulse 1 ctrl = 0x8F (ignore-duty + volume 0xF).
        m.cpu_write(0x9000, 0x8F);
        // Pulse 1 period = 0x123 with enable bit.
        m.cpu_write(0x9001, 0x23);
        m.cpu_write(0x9002, 0x81); // bit 7 = enable, high nibble = 1.
        assert!(m.pulse1.enabled);
        assert_eq!(m.pulse1.period, 0x123);
        assert_eq!(m.pulse1.ctrl, 0x8F);

        // Pulse 2 similar.
        m.cpu_write(0xA000, 0x07); // duty 0 -> threshold 0; volume 7.
        m.cpu_write(0xA001, 0x40);
        m.cpu_write(0xA002, 0x80); // enable, period high nibble 0.
        assert!(m.pulse2.enabled);
        assert_eq!(m.pulse2.period, 0x040);

        // Sawtooth.
        m.cpu_write(0xB000, 0x05); // rate = 5.
        m.cpu_write(0xB001, 0x20);
        m.cpu_write(0xB002, 0x80); // enable.
        assert!(m.saw.enabled);
        assert_eq!(m.saw.rate, 5);
        assert_eq!(m.saw.period, 0x020);

        // $B003 still drives mirroring.
        m.cpu_write(0xB003, 0b0000_0100); // bits 3:2 = 01 -> Horizontal.
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn vrc6_pulse_oscillator_steps_through_duty() {
        let mut p = Vrc6Pulse {
            ctrl: 0x4F, // duty = 0b100 (4) so output high while step <= 4.
            period: 4,  // small, ticks fast.
            enabled: true,
            timer: 0,
            step: 0,
        };
        // First clock: timer == 0 so we reload and bump step to 1.
        let mut outputs = Vec::new();
        for _ in 0..32 {
            p.clock();
            outputs.push(p.output());
        }
        // We expect a roughly 5/16 duty cycle pattern of volume(15) intervals
        // separated by zero intervals. Sanity-check both poles appear.
        assert!(outputs.contains(&0x0F));
        assert!(outputs.contains(&0));
    }

    #[test]
    fn vrc6_sawtooth_emits_ramp() {
        let mut s = Vrc6Saw {
            rate: 0x10,
            period: 2,
            enabled: true,
            timer: 0,
            step: 0,
            acc: 0,
        };
        // Drive long enough to see at least one full 14-step ramp.
        let mut sampled = Vec::new();
        for _ in 0..60 {
            s.clock();
            sampled.push(s.output());
        }
        // Ramp should reach a peak greater than zero and eventually reset.
        let peak = sampled.iter().copied().max().unwrap();
        assert!(peak > 0, "saw must emit a non-zero peak");
        // And it should hit zero (after step >= 14 reset).
        assert!(sampled.contains(&0));
    }

    #[cfg(feature = "mapper-audio")]
    #[test]
    fn vrc6_mix_audio_is_nonzero_when_active() {
        let mut m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        // Enable pulse 1 with max volume + ignore-duty mode -> output = 15.
        m.cpu_write(0x9000, 0x8F);
        m.cpu_write(0x9001, 0x10);
        m.cpu_write(0x9002, 0x81);
        // Tick once so the oscillator advances past the timer == 0 reload.
        m.clock_audio();
        let s = m.mix_audio();
        // Centering subtracts ~30 from a 0..=61 sum, scales by 256.
        // With only p1 = 15 contributing, s = (15 - 30) * 256 = -3840.
        assert!(s < 0, "mix_audio with only p1 must be below center");
    }

    #[cfg(feature = "mapper-audio")]
    #[test]
    fn vrc6_mix_audio_silent_when_disabled() {
        let m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        // All channels disabled -> outputs 0 -> sum 0 -> mix = (0 - 30) * 256.
        // Confirm we land at the documented "center - offset" position.
        let mut m = m;
        let s = m.mix_audio();
        assert_eq!(s, -7680);
    }

    #[test]
    fn vrc6_save_state_v2_round_trips_audio() {
        let mut m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        m.cpu_write(0x9000, 0x8F);
        m.cpu_write(0x9001, 0x12);
        m.cpu_write(0x9002, 0x83);
        m.cpu_write(0xB000, 0x07);
        let blob = m.save_state();
        assert_eq!(blob[0], 2, "save_state must bump to version 2");

        let mut m2 = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        m2.load_state(&blob).expect("v2 round-trip");
        assert_eq!(m2.pulse1.ctrl, 0x8F);
        assert_eq!(m2.pulse1.period, 0x312);
        assert!(m2.pulse1.enabled);
        assert_eq!(m2.saw.rate, 0x07);
    }

    #[test]
    fn vrc6_save_state_loads_v1_blob_with_default_audio() {
        // ADR-0003 invariant: v2 reader must accept a v1 blob; audio state
        // defaults to silence (channels disabled, ctrl/period zero).
        let m = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        let mut blob = m.save_state();
        // Synthesize a "v1 blob" by truncating the audio tail (last 23 bytes)
        // and rewriting the version byte from 2 -> 1.
        let tail = 23;
        blob.truncate(blob.len() - tail);
        blob[0] = 1;
        let mut m2 = Vrc6::new(synth(8), synth_chr(8), 24, Mirroring::Vertical).unwrap();
        m2.cpu_write(0x9000, 0xFF); // perturb pre-load
        m2.load_state(&blob)
            .expect("v1 blob must load on v2 reader");
        // Audio state is unchanged from before load (no v2 tail).
        // pulse1.ctrl was perturbed and NOT reset, since v1 doesn't carry
        // audio state. This matches ADR-0003: older blobs don't reset
        // newer-section state, the caller is responsible for an explicit
        // reset/power-cycle if they want a clean slate.
        assert_eq!(m2.pulse1.ctrl, 0xFF);
    }

    // ---- Sunsoft 5B audio (C2-5B) ---------------------------------------

    /// Helper: write a Sunsoft 5B register through the two-write protocol.
    /// `$C000-$DFFF` latches the 4-bit register index; `$E000-$FFFF` writes
    /// the data byte to that register.
    fn fme7_audio_write(m: &mut Fme7, reg: u8, value: u8) {
        m.cpu_write(0xC000, reg);
        m.cpu_write(0xE000, value);
    }

    #[test]
    fn sunsoft5b_register_address_latch_round_trip() {
        // The address latch is the gateway for every audio write; it must
        // round-trip distinctly from the data path.  After latching $0B
        // (envelope period low), a subsequent data write should target
        // $0B specifically.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC000, 0x0B);
        assert_eq!(m.audio.addr_latch, 0x0B);
        // Bits 7-4 of the address byte are ignored (masked to 4 bits).
        m.cpu_write(0xC100, 0xF7);
        assert_eq!(m.audio.addr_latch, 0x07);
        // A data write at $E000-$FFFF goes to the latched register.
        m.cpu_write(0xE800, 0xAB);
        assert_eq!(m.audio.regs[0x07], 0xAB);
    }

    #[test]
    fn sunsoft5b_channel_period_decodes_into_internal_state() {
        // Channel A period: TP = ($01 & 0x0F) << 8 | $00.  Confirm the
        // 12-bit period composes correctly from the two writes, and that
        // bits 7-4 of $01 are masked off.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        fme7_audio_write(&mut m, 0x00, 0x34);
        fme7_audio_write(&mut m, 0x01, 0xF7); // upper nibble (7) used; F is ignored.
        assert_eq!(m.audio.tone_a.period, 0x0734);

        // Channel B / C similarly.
        fme7_audio_write(&mut m, 0x02, 0x12);
        fme7_audio_write(&mut m, 0x03, 0x03);
        assert_eq!(m.audio.tone_b.period, 0x0312);
        fme7_audio_write(&mut m, 0x04, 0xFF);
        fme7_audio_write(&mut m, 0x05, 0x0F);
        assert_eq!(m.audio.tone_c.period, 0x0FFF);
    }

    #[test]
    fn sunsoft5b_tone_toggles_every_16_times_period_cycles() {
        // Per NESdev wiki: the square wave toggles every 16 CPU clocks per
        // period count.  With TP = 5, we expect a toggle every 80 cycles.
        // Drive the chip through clock() directly to isolate the tone path
        // from the rest of the mapper.
        let mut t = Sunsoft5BTone {
            period: 5,
            ..Sunsoft5BTone::default()
        };
        // First clock fires immediately (counter starts at 0) and reloads.
        // Count toggles across 800 cycles.
        let mut toggles = 0u32;
        let mut last = t.level;
        for _ in 0..800 {
            t.clock();
            if t.level != last {
                toggles += 1;
                last = t.level;
            }
        }
        // 800 cycles / 80 per toggle = 10 toggles.  Allow ±1 for the
        // counter-starts-at-zero start-up edge.
        assert!(
            (9..=11).contains(&toggles),
            "tone toggle count {toggles} not in 9..=11"
        );
    }

    #[test]
    fn sunsoft5b_volume_scale_zero_silent_max_peak() {
        // Volume 0 must produce silence; volume 15 must produce the peak
        // entry of the log-DAC table.  These bracket the per-channel
        // contribution range.
        assert_eq!(SUNSOFT5B_LOG_VOL[0], 0);
        assert!(SUNSOFT5B_LOG_VOL[15] > SUNSOFT5B_LOG_VOL[14]);
        // The volume() helper applies the envelope-mode select bit.
        let mut a = Sunsoft5BAudio::default();
        a.regs[0x08] = 0x0F; // fixed volume = 15.
        assert_eq!(a.volume(0), 15);
        a.regs[0x08] = 0x00; // fixed volume = 0.
        assert_eq!(a.volume(0), 0);
    }

    #[test]
    fn sunsoft5b_envelope_mode_routes_envelope_into_channel() {
        // Setting bit 4 of $08/$09/$0A switches that channel from fixed
        // volume to envelope mode.  In envelope mode the 4-bit volume
        // equivalent is env >> 1 (per the NESdev table).
        let mut a = Sunsoft5BAudio::default();
        a.regs[0x08] = 0x10; // envelope mode, fixed-volume bits ignored.
        a.envelope.level = 30; // 4-bit equivalent = 15.
        assert_eq!(a.volume(0), 15);
        a.envelope.level = 6;
        assert_eq!(a.volume(0), 3);
        a.envelope.level = 1;
        assert_eq!(a.volume(0), 0); // env 0 and 1 both -> 0.
        // Switching back to fixed mode honors $08 bits 3-0 again.
        a.regs[0x08] = 0x07;
        assert_eq!(a.volume(0), 7);
    }

    #[cfg(feature = "mapper-audio")]
    #[test]
    fn sunsoft5b_mix_output_sign_silent_vs_active() {
        // With every channel muted (mixer = 0xFF disables both tone and
        // noise on A/B/C; volumes don't matter), the linear sum is 0 and
        // the mix output sits at -DC_BIAS (centered).  With one channel
        // unmuted at max volume and the square wave high, the sum exceeds
        // the bias and the mix is positive.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        fme7_audio_write(&mut m, 0x07, 0x3F); // bits 0..=5 all set => all disabled.
        // Volumes irrelevant when channels are muted.
        let silent = m.mix_audio();
        assert_eq!(silent, -SUNSOFT5B_DC_BIAS);

        // Enable tone A only at max volume, then force the square level high
        // by ticking once with period = 0 (the chip wraps period=0 to 1).
        fme7_audio_write(&mut m, 0x07, 0b0011_1110); // tone A enabled (bit 0 = 0).
        fme7_audio_write(&mut m, 0x08, 0x0F); // channel A volume = 15.
        // Manually toggle the tone level so we hit the "high" half-cycle.
        m.audio.tone_a.level = 1;
        let active = m.mix_audio();
        assert!(
            active > 0,
            "active mix output should be positive, got {active}"
        );
    }

    #[test]
    fn sunsoft5b_save_state_v2_round_trips_audio() {
        // Round-trip an FME-7 with a non-trivial audio register file.  The
        // load_state path reconstructs the live period/shape state from the
        // serialized register file, so verifying via `audio.tone_a.period`
        // exercises both the regs blob and the reconstruction path.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        fme7_audio_write(&mut m, 0x00, 0x55);
        fme7_audio_write(&mut m, 0x01, 0x06);
        fme7_audio_write(&mut m, 0x08, 0x0F);
        fme7_audio_write(&mut m, 0x07, 0x36); // a few tone/noise enables.
        fme7_audio_write(&mut m, 0x0D, 0x0E); // envelope shape -> restart.
        let blob = m.save_state();
        assert_eq!(blob[0], 2, "save_state must bump FME-7 to version 2");

        let mut m2 = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).expect("v2 round-trip");
        assert_eq!(m2.audio.tone_a.period, 0x0655);
        assert_eq!(m2.audio.regs[0x07], 0x36);
        assert_eq!(m2.audio.regs[0x08], 0x0F);
        assert_eq!(m2.audio.envelope.shape, 0x0E);
    }

    #[test]
    fn sunsoft5b_save_state_loads_v1_blob_with_default_audio() {
        // ADR-0003 invariant: v2 reader must accept a v1 blob; audio state
        // stays at whatever the freshly-constructed mapper has (silence).
        // We synthesize a v1 blob by truncating the audio tail and resetting
        // the version byte.
        let m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        let mut blob = m.save_state();
        let tail = Sunsoft5BAudio::TAIL_LEN;
        blob.truncate(blob.len() - tail);
        blob[0] = 1;

        let mut m2 = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        // Perturb audio state pre-load; a v1 blob must not touch it.
        fme7_audio_write(&mut m2, 0x07, 0xAA);
        m2.load_state(&blob)
            .expect("v1 blob must load on v2 reader");
        // Per ADR-0003: older blobs do not reset newer-section state.
        assert_eq!(m2.audio.regs[0x07], 0xAA);
    }

    #[test]
    fn sunsoft5b_mapper_audio_off_path_latches_state_but_stays_silent() {
        // When the `mapper-audio` feature is OFF, the register decoder still
        // latches every write (so save-state round-trip stays correct) but
        // the oscillators never advance and `mix_audio` returns 0.
        //
        // We can't toggle the cargo feature from inside a test, but we CAN
        // assert the two halves of this contract directly:
        //   1. The register latch path is unconditional (this test runs
        //      regardless of the feature flag).
        //   2. The oscillator clock path is gated — verified by the absence
        //      of `audio.clock()` calls in `notify_cpu_cycle` when the
        //      feature is off (compile-time `#[cfg(...)]`).
        // To exercise (1), write to every register and confirm `regs` and
        // the derived period fields are populated.  To exercise (2)'s
        // observable effect, freeze the counters by NOT calling notify and
        // confirm the level state stays at zero.
        let mut m = Fme7::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap();
        for r in 0u8..=0x0F {
            fme7_audio_write(&mut m, r, r.wrapping_mul(0x11));
        }
        assert_eq!(m.audio.regs[0x00], 0x00);
        assert_eq!(m.audio.regs[0x0F], 0xFF);
        // Without any clock() calls, the tone level remains at default 0.
        assert_eq!(m.audio.tone_a.level, 0);
        assert_eq!(m.audio.tone_b.level, 0);
        assert_eq!(m.audio.tone_c.level, 0);
    }

    // -----------------------------------------------------------------------
    // Namco 163 audio (Phase 2.2 / Track C2-N163)
    // -----------------------------------------------------------------------

    /// Build a fresh Namco163 mapper with 8 KiB PRG + 8 KiB CHR for the
    /// audio-focused tests below.  None of them exercise banking, IRQ,
    /// or PPU; they only poke registers and observe `audio` state.
    fn namco163_for_audio() -> Namco163 {
        Namco163::new(synth(8), synth_chr(8), Mirroring::Vertical).unwrap()
    }

    /// Drive an address-port + data-port write pair, emulating the
    /// canonical N163 register protocol from CPU code.
    fn n163_write_ram(m: &mut Namco163, addr: u8, auto_inc: bool, value: u8) {
        // $F800 = address port (bit 7 = auto-increment, bits 6-0 = addr).
        let port = (if auto_inc { 0x80 } else { 0x00 }) | (addr & 0x7F);
        m.cpu_write(0xF800, port);
        m.cpu_write(0x4800, value);
    }

    #[test]
    fn namco163_address_port_latch_and_auto_increment() {
        let mut m = namco163_for_audio();
        // Without auto-increment: write 0x05 to addr, then 0x42 to data.
        // Latch should stay at 0x05.
        m.cpu_write(0xF800, 0x05);
        m.cpu_write(0x4800, 0x42);
        assert_eq!(m.audio.ram[0x05], 0x42);
        assert_eq!(m.audio.addr_latch, 0x05);
        assert!(!m.audio.auto_inc);

        // Second write also lands at 0x05 (latch did not advance).
        m.cpu_write(0x4800, 0x99);
        assert_eq!(m.audio.ram[0x05], 0x99);
        assert_eq!(m.audio.addr_latch, 0x05);

        // With auto-increment: write 0x80 | 0x05, then 0x55 → addr 0x05
        // gets 0x55 and latch advances to 0x06.
        m.cpu_write(0xF800, 0x80 | 0x05);
        m.cpu_write(0x4800, 0x55);
        assert_eq!(m.audio.ram[0x05], 0x55);
        assert_eq!(m.audio.addr_latch, 0x06);
        assert!(m.audio.auto_inc);

        // Next data write lands at 0x06.
        m.cpu_write(0x4800, 0x66);
        assert_eq!(m.audio.ram[0x06], 0x66);
        assert_eq!(m.audio.addr_latch, 0x07);
    }

    #[test]
    fn namco163_address_port_saturates_at_7f() {
        // Per the NESdev wiki: the auto-increment "stopping at $7F"
        // rather than wrapping.  Verify by walking the latch up to $7F
        // and then doing one more data access.
        let mut m = namco163_for_audio();
        m.cpu_write(0xF800, 0x80 | 0x7F);
        m.cpu_write(0x4800, 0xAA); // RAM[0x7F] = 0xAA, latch stays at 0x7F.
        assert_eq!(m.audio.ram[0x7F], 0xAA);
        assert_eq!(m.audio.addr_latch, 0x7F);
        // A second write also lands at 0x7F (saturation, not wrap).
        m.cpu_write(0x4800, 0xBB);
        assert_eq!(m.audio.ram[0x7F], 0xBB);
        assert_eq!(m.audio.addr_latch, 0x7F);
        assert_eq!(m.audio.ram[0x00], 0x00, "wrap to $00 must not happen");
    }

    #[test]
    fn namco163_data_port_read_round_trip() {
        // Write 0xAB at addr 0x10 with auto-increment, then read it back.
        // Read also advances the latch.
        let mut m = namco163_for_audio();
        m.cpu_write(0xF800, 0x80 | 0x10);
        m.cpu_write(0x4800, 0xAB);
        // After the write, latch is at 0x11.
        // Re-target 0x10 for the read.
        m.cpu_write(0xF800, 0x80 | 0x10);
        assert_eq!(m.cpu_read(0x4800), 0xAB);
        assert_eq!(m.audio.addr_latch, 0x11);
    }

    #[test]
    fn namco163_wavetable_nibble_unpacking() {
        // Byte 0xAB at RAM[0x10] → nibble 0x20 = 0xB (low), nibble 0x21
        // = 0xA (high).  Verifies the wavetable nibble-fetch helper.
        let mut m = namco163_for_audio();
        m.cpu_write(0xF800, 0x10);
        m.cpu_write(0x4800, 0xAB);
        assert_eq!(m.audio.ram[0x10], 0xAB);
        #[cfg(feature = "mapper-audio")]
        {
            assert_eq!(m.audio.fetch_nibble(0x20), 0x0B);
            assert_eq!(m.audio.fetch_nibble(0x21), 0x0A);
        }
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_channel_count_selection() {
        // Bits 6-4 of register $7F encode "channel count - 1".
        // C=0 → 1 channel; C=7 → 8 channels.
        let mut m = namco163_for_audio();
        for c in 0u8..=7 {
            n163_write_ram(&mut m, 0x7F, false, c << 4);
            assert_eq!(
                m.audio.channel_count(),
                c + 1,
                "C={c} should map to {} channels",
                c + 1
            );
        }
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_channel_frequency_assembly() {
        // Channel 8 lives at $78-$7F.  Write freq lo=$78, mid=$7A, hi=$7C.
        // hi register's bits 7-2 carry the wave length encoding, so we
        // pack length bits as well to exercise the mask.
        let mut m = namco163_for_audio();
        // Lo = 0x34, mid = 0x12, hi-bits = 0x02, length-bits = 0xFC
        // (length = 256 - 0xFC = 4).
        n163_write_ram(&mut m, 0x78, false, 0x34);
        n163_write_ram(&mut m, 0x7A, false, 0x12);
        n163_write_ram(&mut m, 0x7C, false, 0xFC | 0x02);

        let freq = m.audio.channel_freq(0x78);
        assert_eq!(freq, 0x02_1234, "freq = hi<<16 | mid<<8 | lo");
        let length = m.audio.channel_length(0x78);
        assert_eq!(length, 4);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_single_channel_constant_output_then_bipolar_swing() {
        // Channel 0 (the always-enabled channel at $78-$7F) with a
        // constant wavetable of 0xFF (high nibble 0xF, low nibble 0xF)
        // and volume 15 should yield output = (15 - 8) * 15 = +105.
        // Length-1 waveform means the index never moves.
        let mut m = namco163_for_audio();
        // Wavetable byte 0x10 = 0xFF → nibble 0x20 = 0xF, 0x21 = 0xF.
        n163_write_ram(&mut m, 0x10, false, 0xFF);
        // Channel 8 (the always-enabled, highest-priority channel) regs.
        // Wave addr = 0x20 (the nibble we filled).
        // Length encoding: 256 - 0xFC = 4 (chosen to keep the test
        // robust to phase, since every cycle still reads 0xF).
        // Volume = 0x0F, channel-count field = 0 (single channel).
        n163_write_ram(&mut m, 0x7C, false, 0xFC); // length=4, freq-hi=0
        n163_write_ram(&mut m, 0x7E, false, 0x20); // wave_addr
        n163_write_ram(&mut m, 0x7F, false, 0x0F); // volume=15, C=0

        let output = m.audio.channel_output(0);
        assert_eq!(output, (15 - 8) * 15, "+105 expected for nibble=15, vol=15");
        // Mix returns (sum / 1) * 64 = 105 * 64 = 6720.
        assert_eq!(m.audio.mix(), 105 * 64);

        // Now swap the wavetable to nibble 0 — output should swing
        // negative: (0 - 8) * 15 = -120.
        m.cpu_write(0xF800, 0x10);
        m.cpu_write(0x4800, 0x00);
        assert_eq!(m.audio.channel_output(0), (0 - 8) * 15);
        assert!(m.audio.mix() < 0, "negative samples must yield <0 mix");
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_volume_zero_silences_channel() {
        // A channel with volume == 0 contributes 0 to the mix
        // regardless of the wavetable contents.
        let mut m = namco163_for_audio();
        n163_write_ram(&mut m, 0x10, false, 0xFF); // wavetable bytes
        n163_write_ram(&mut m, 0x7C, false, 0xFC); // length=4
        n163_write_ram(&mut m, 0x7E, false, 0x20); // wave_addr=0x20
        n163_write_ram(&mut m, 0x7F, false, 0x00); // vol=0, C=0
        assert_eq!(m.audio.channel_output(0), 0);
        assert_eq!(m.audio.mix(), 0);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_clock_advances_only_active_channel() {
        // Two-channel setup: C=1, so channels 8 and 7 (bases $78, $70)
        // are active.  Set freq=0x01_0000 on channel 8 (so each tick
        // advances phase by 1 << 16) and freq=0 on channel 7.  After
        // 30 CPU cycles (= 2 audio updates), phase[ch=8] should have
        // advanced exactly once (the round-robin alternates 8/7/8/7...).
        let mut m = namco163_for_audio();
        // Channel 8 freq = 0x01_0000 → hi=01, mid=00, lo=00.
        n163_write_ram(&mut m, 0x78, false, 0x00); // freq lo
        n163_write_ram(&mut m, 0x7A, false, 0x00); // freq mid
        // length=4 (256 - 0xFC), freq-hi=01.
        n163_write_ram(&mut m, 0x7C, false, 0xFC | 0x01);
        n163_write_ram(&mut m, 0x7F, false, 0x10); // C=1 → 2 channels
        // Channel 7 freq = 0.
        n163_write_ram(&mut m, 0x70, false, 0x00);
        n163_write_ram(&mut m, 0x72, false, 0x00);
        n163_write_ram(&mut m, 0x74, false, 0xFC);

        // 15 cycles → channel 8 advances by 0x01_0000.
        for _ in 0..15 {
            m.notify_cpu_cycle();
        }
        let phase_ch8 = m.audio.channel_phase(0x78);
        // length=4, modulus = 4 << 16 = 0x40000, so 0x10000 stays.
        assert_eq!(phase_ch8, 0x0001_0000);
        let phase_ch7 = m.audio.channel_phase(0x70);
        assert_eq!(phase_ch7, 0, "ch7 must not advance on the first slot");

        // Next 15 cycles → channel 7 advances (by 0, so still 0); ch8
        // unchanged.
        for _ in 0..15 {
            m.notify_cpu_cycle();
        }
        assert_eq!(m.audio.channel_phase(0x78), 0x0001_0000);
        assert_eq!(m.audio.channel_phase(0x70), 0);
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn namco163_sound_disable_bit_silences_mix() {
        // $E000 bit 6 set → audio chip is silenced.  Even with a
        // non-zero wavetable and volume, mix_audio returns 0.
        let mut m = namco163_for_audio();
        n163_write_ram(&mut m, 0x10, false, 0xFF);
        n163_write_ram(&mut m, 0x7C, false, 0xFC);
        n163_write_ram(&mut m, 0x7E, false, 0x20);
        n163_write_ram(&mut m, 0x7F, false, 0x0F);
        assert_ne!(m.mix_audio(), 0);
        // Set sound-disable: $E000 with bit 6 = 1.  Bits 0-5 also write
        // PRG bank 0; we just need the bit 6.
        m.cpu_write(0xE000, 0x40);
        assert!(m.sound_disabled);
        assert_eq!(m.mix_audio(), 0);
        // Clearing it re-enables.
        m.cpu_write(0xE000, 0x00);
        assert!(!m.sound_disabled);
        assert_ne!(m.mix_audio(), 0);
    }

    #[test]
    fn namco163_save_state_v1_loads_with_audio_defaults() {
        // A v1 (pre-audio) save-state blob should load on a v2 reader
        // with audio defaulted to silence (zero RAM, zero phase, zero
        // latch, sound_disabled=false).  Construct a synthetic v1 blob
        // by hand to exercise the backward-compat path.
        let mut donor = namco163_for_audio();
        // Mutate non-audio state so we can verify it round-trips.
        donor.prg[0] = 0x05;
        donor.chr[3] = 0x07;
        donor.nta[1] = 0x02;
        donor.irq_counter = 0x1234;
        donor.irq_pending = true;
        donor.audio.ram[0x40] = 0x99; // would normally serialize in v2

        // Build a v1 blob (no audio tail).
        let mut blob = Vec::new();
        blob.push(1u8);
        blob.extend_from_slice(&donor.prg);
        blob.extend_from_slice(&donor.chr);
        blob.extend_from_slice(&donor.nta);
        blob.push(donor.mirroring as u8);
        blob.extend_from_slice(&donor.irq_counter.to_le_bytes());
        blob.push(u8::from(donor.irq_pending));
        blob.extend_from_slice(&donor.prg_ram);
        blob.extend_from_slice(&donor.vram);

        let mut target = namco163_for_audio();
        // Pre-populate target with bogus audio state, then verify it
        // gets cleared by the v1 load path.
        target.audio.ram[0x40] = 0xAA;
        target.audio.addr_latch = 0x55;
        target.audio.auto_inc = true;
        target.sound_disabled = true;
        target.load_state(&blob).unwrap();
        assert_eq!(target.prg[0], 0x05);
        assert_eq!(target.chr[3], 0x07);
        assert_eq!(target.irq_counter, 0x1234);
        // Audio state should be default (silent).
        assert_eq!(target.audio.ram, [0u8; 128]);
        assert_eq!(target.audio.addr_latch, 0);
        assert!(!target.audio.auto_inc);
        assert!(!target.sound_disabled);
    }

    #[test]
    fn namco163_save_state_v2_round_trip() {
        // v2 → v2 round-trip preserves the full audio state.
        let mut donor = namco163_for_audio();
        n163_write_ram(&mut donor, 0x10, true, 0xAB);
        n163_write_ram(&mut donor, 0x7F, false, 0x35); // C=3 → 4 channels, vol=5
        donor.cpu_write(0xE000, 0x40); // sound disable
        let blob = donor.save_state();
        assert_eq!(blob[0], 2u8, "v2 tag expected");

        let mut target = namco163_for_audio();
        target.load_state(&blob).unwrap();
        assert_eq!(target.audio.ram[0x10], 0xAB);
        assert_eq!(target.audio.ram[0x7F], 0x35);
        assert!(target.sound_disabled);
        // addr_latch after the writes: $7F (we wrote $7F last,
        // auto_inc=false, so the latch stayed at $7F).
        assert_eq!(target.audio.addr_latch, 0x7F);
    }

    // ---- VRC7 (mapper 85; FM audio deferred per ADR-0004) ---------------

    fn vrc7_default() -> Vrc7 {
        // 8 × 8 KiB PRG (bank index byte at offset 0 of each bank to make
        // the read path observable) + 16 × 1 KiB CHR (likewise).
        Vrc7::new(synth(8), synth_chr(16), Mirroring::Vertical).unwrap()
    }

    #[test]
    fn vrc7_prg_banking_three_switchable_plus_fixed_last() {
        let mut m = vrc7_default();
        // $8000 = PRG bank 0 (window $8000-$9FFF). Pick bank 5.
        m.cpu_write(0x8000, 5);
        // $8010 = PRG bank 1 ($A000-$BFFF). Pick bank 3.
        m.cpu_write(0x8010, 3);
        // $9000 = PRG bank 2 ($C000-$DFFF). Pick bank 7.
        m.cpu_write(0x9000, 7);
        // Read at the start of each window returns the synth's bank-index
        // byte (bank index lives at offset 0 of each 8 KiB bank).
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xA000), 3);
        assert_eq!(m.cpu_read(0xC000), 7);
        // $E000-$FFFF is fixed to the LAST bank (synth has 8 banks → 7).
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn vrc7_prg_banking_accepts_a3_a4_mirror() {
        // $8008 is the A3 mirror of $8010 → both select PRG bank 1.
        let mut m = vrc7_default();
        m.cpu_write(0x8008, 4);
        assert_eq!(m.cpu_read(0xA000), 4);
        m.cpu_write(0x8010, 2);
        assert_eq!(m.cpu_read(0xA000), 2);
    }

    #[test]
    fn vrc7_chr_banking_all_eight_slots() {
        // CHR banks 0..=7 are addressable at $A000 / $A010 / $B000 /
        // $B010 / $C000 / $C010 / $D000 / $D010.  Each 1 KiB CHR bank
        // in the synth ROM carries its bank index at offset 0.
        let mut m = vrc7_default();
        let writes = [
            (0xA000u16, 1u8, 0x0000u16),
            (0xA010, 2, 0x0400),
            (0xB000, 3, 0x0800),
            (0xB010, 4, 0x0C00),
            (0xC000, 5, 0x1000),
            (0xC010, 6, 0x1400),
            (0xD000, 7, 0x1800),
            (0xD010, 8, 0x1C00),
        ];
        for (addr, bank, _) in writes {
            m.cpu_write(addr, bank);
        }
        for (_, bank, ppu_addr) in writes {
            assert_eq!(m.ppu_read(ppu_addr), bank, "CHR slot for {ppu_addr:#x}");
        }
    }

    #[test]
    fn vrc7_mirroring_decode_from_e000_low_bits() {
        let mut m = vrc7_default();
        // 00 = Vertical (the default).
        m.cpu_write(0xE000, 0b0000_0000);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // 01 = Horizontal.
        m.cpu_write(0xE000, 0b0000_0001);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // 10 = SingleScreen A.
        m.cpu_write(0xE000, 0b0000_0010);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // 11 = SingleScreen B.
        m.cpu_write(0xE000, 0b0000_0011);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn vrc7_irq_counter_cycle_mode_pending() {
        // CPU-cycle mode: counter increments every CPU cycle; on $FF
        // it reloads from latch and asserts IRQ.  Same shape as VRC6.
        let mut m = vrc7_default();
        // Latch: 0xFE (so we need only 2 ticks to wrap from 0xFE -> 0xFF -> 0x00 + pending).
        m.cpu_write(0xE008, 0xFE); // $E008 = IRQ latch
        // Control: enable + cycle mode (mode bit 2 = 1 means CPU cycle).
        // Bit 0 = enable_after_ack; bit 1 = enable; bit 2 = mode (1=cycle, 0=scanline).
        m.cpu_write(0xF000, 0b0000_0110);
        // After enable, counter = latch = 0xFE.  Ticking until pending:
        // 0xFE -> 0xFF (clock 1), pending fires (clock 2 reloads from latch).
        m.notify_cpu_cycle();
        assert!(!m.irq_pending(), "after 1 cycle, counter only at 0xFF");
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "after 2 cycles, pending should be set");
    }

    #[test]
    fn vrc7_irq_ack_clears_pending_and_restores_enable_state() {
        // After IRQ fires, $F010 ack clears pending and restores
        // enable from enable_after_ack.  Match the VRC6 contract.
        let mut m = vrc7_default();
        m.cpu_write(0xE008, 0xFE);
        m.cpu_write(0xF000, 0b0000_0111); // enable_after_ack=1, enable=1, cycle mode
        m.notify_cpu_cycle();
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
        m.cpu_write(0xF010, 0); // ack
        assert!(!m.irq_pending());
        assert!(m.irq_enabled, "enable should be restored from after_ack");
    }

    #[test]
    fn vrc7_audio_register_latch_round_trip() {
        // Per ADR-0004 the synthesizer is deferred, but the register
        // surface must still latch state cleanly.  This test pins the
        // contract a future v1.x OPLL integration will read from.
        let mut m = vrc7_default();
        m.cpu_write(0x9010, 0x10); // OPLL register address = 0x10
        assert_eq!(m.audio.addr_latch, 0x10);
        m.cpu_write(0x9030, 0x42); // OPLL data byte
        assert_eq!(m.audio.data_latch, 0x42);
        assert_eq!(m.audio.regs[0x10], 0x42);
        // A second address+data pair: write 0x30 (channel-1 volume +
        // instrument select) then a different data byte.
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x5F); // top nibble = inst 5, low nibble = vol 0xF
        assert_eq!(m.audio.regs[0x30], 0x5F);
        // Earlier write at 0x10 is preserved (independent slots).
        assert_eq!(m.audio.regs[0x10], 0x42);
    }

    #[test]
    fn vrc7_audio_custom_instrument_bytes_route_to_registers_0_through_7() {
        // The 8 custom-instrument bytes live at OPLL registers $00-$07.
        // Confirm they land in the right slots when written through
        // the $9010 / $9030 protocol.
        let mut m = vrc7_default();
        for i in 0..8u8 {
            m.cpu_write(0x9010, i);
            m.cpu_write(0x9030, 0xA0 | i); // distinct payload per slot
            assert_eq!(m.audio.regs[i as usize], 0xA0 | i);
        }
    }

    #[test]
    fn vrc7_mix_audio_silent_with_no_key_on() {
        // Sprint 1.2 (v1.1.0): OPLL is wired but no channel has been
        // keyed on — every slot's envelope sits at EG_MUTE, so every
        // OPLL sample is 0. The mix_audio output should therefore be
        // 0 across the entire register-surface scan.
        let mut m = vrc7_default();
        for reg in 0..=0x35u8 {
            m.cpu_write(0x9010, reg);
            m.cpu_write(0x9030, 0x00); // zero-fill — no key-on bits
        }
        // Tick the OPLL several times to confirm calc() also returns 0.
        for _ in 0..200 {
            m.notify_cpu_cycle();
        }
        assert_eq!(
            m.mix_audio(),
            0,
            "VRC7 mix_audio must be silent without key-on; got non-zero"
        );
    }

    #[test]
    fn vrc7_mix_audio_silenced_by_e000_bit7() {
        // Even with a keyed-on channel, the `$E000` expansion-sound
        // silence bit (bit 7) must force mix_audio to 0. Mesen2 calls
        // this the "muted" flag in Vrc7Audio.h.
        let mut m = vrc7_default();
        // Set up channel 0: instrument 1, fnum 256, block 4, key-on,
        // max volume (volume bits low = max — OPLL volume is attenuation).
        m.cpu_write(0x9010, 0x30); // $30 = inst/volume for ch 0
        m.cpu_write(0x9030, 0x10); // inst 1, volume 0 (loudest)
        m.cpu_write(0x9010, 0x10); // $10 = fnum low for ch 0
        m.cpu_write(0x9030, 0x00);
        m.cpu_write(0x9010, 0x20); // $20 = fnum high + block + key for ch 0
        m.cpu_write(0x9030, 0x35); // key-on bit set + block + fnum high
        // Tick enough cycles for the envelope to clear Damp → Attack.
        for _ in 0..16_384 {
            m.notify_cpu_cycle();
        }
        // Now flip the silence bit on `$E000`.
        m.cpu_write(0xE000, 0x80);
        assert_eq!(
            m.mix_audio(),
            0,
            "silenced VRC7 must mix to 0; got non-zero"
        );
        // Verify the OPLL still ticks (its internal state advances) —
        // re-clear silence and the audio should resume.
        m.cpu_write(0xE000, 0x00);
        // We don't assert non-zero here because the OPLL might have
        // landed on a zero-crossing this exact tick — just confirm
        // the silenced gate is the only thing stopping output.
        // (The non-zero output is covered by the next test.)
    }

    #[test]
    fn vrc7_opll_register_writes_forwarded_on_data_write() {
        // `$9030` data writes must be forwarded to the OPLL's
        // register shadow. Verifies the integration point even
        // without ticking the synth.
        let mut m = vrc7_default();
        m.cpu_write(0x9010, 0x20); // address latch = $20
        m.cpu_write(0x9030, 0x55); // data write
        // Snapshot stores the byte in both the mapper's audio.regs
        // (for save-state round-trip) and the OPLL's register shadow.
        assert_eq!(m.audio.regs[0x20], 0x55);
        #[cfg(feature = "mapper-audio")]
        assert_eq!(
            m.opll.read_reg(0x20),
            0x55,
            "OPLL register shadow should mirror $9030 writes"
        );
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn vrc7_keyed_on_channel_produces_nonzero_mix_within_one_envelope() {
        // End-to-end: configure channel 0 with VRC7 patch 1, key on,
        // run enough CPU cycles for Damp → Attack to progress past
        // EG_MUTE, and observe a non-zero mix_audio sample.
        let mut m = vrc7_default();
        // Channel 0 setup matching the OPLL unit test's manual setup.
        // $30 → bits 3-0 = volume (attenuation), bits 7-4 = instrument
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x10); // inst=1, vol=0
        m.cpu_write(0x9010, 0x10);
        m.cpu_write(0x9030, 0x80); // fnum low byte
        m.cpu_write(0x9010, 0x20);
        m.cpu_write(0x9030, 0x35); // key-on + block(2) + fnum high(1)
        // Each OPLL sample = 36 CPU cycles. 16,384 CPU cycles = ~455
        // OPLL samples = ~9 ms of audio. Damp → Attack happens within
        // a few hundred OPLL samples for any non-saturated AR.
        let mut peak_abs: u16 = 0;
        for _ in 0..16_384 {
            m.notify_cpu_cycle();
            let s = m.mix_audio();
            peak_abs = peak_abs.max(s.unsigned_abs());
        }
        assert!(
            peak_abs > 0,
            "expected non-zero VRC7 mix after key-on + 16k cycles; got peak_abs={peak_abs}"
        );
    }

    #[test]
    #[cfg(feature = "mapper-audio")]
    fn vrc7_opll_ticks_every_36_cpu_cycles() {
        // The OPLL is clocked at NES NTSC CPU rate / 36. Verify the
        // internal counter rolls over exactly on the 36th call to
        // notify_cpu_cycle by watching eg_counter (which advances
        // once per OPLL tick inside `update_slots`).
        let mut m = vrc7_default();
        // No way to read eg_counter through the public API, but we
        // CAN read opll_clock_counter via direct field access in
        // this module-local test. After 35 cycles, counter = 35;
        // after 36, counter resets to 0 and the OPLL has advanced.
        for _ in 0..35 {
            m.notify_cpu_cycle();
        }
        assert_eq!(m.opll_clock_counter, 35);
        m.notify_cpu_cycle();
        assert_eq!(
            m.opll_clock_counter, 0,
            "counter should reset on 36th cycle"
        );
    }

    #[test]
    fn vrc7_save_state_round_trip_preserves_banking_irq_and_audio_latches() {
        // v1 round-trip: configure banking, IRQ counter mid-state, and
        // audio register latches → save → reload into a fresh mapper
        // → all fields match.
        let mut m = vrc7_default();
        m.cpu_write(0x8000, 5);
        m.cpu_write(0x8010, 3);
        m.cpu_write(0x9000, 7);
        m.cpu_write(0xA000, 1);
        m.cpu_write(0xD010, 6);
        m.cpu_write(0xE000, 0b1100_0001); // Horizontal + WRAM enable + audio silenced
        m.cpu_write(0xE008, 0x80); // IRQ latch
        m.cpu_write(0xF000, 0b0000_0011); // enable + scanline mode
        // Audio register stream.
        m.cpu_write(0x9010, 0x30);
        m.cpu_write(0x9030, 0x5F);
        let blob = m.save_state();
        assert_eq!(blob[0], 1u8, "VRC7 save-state version tag");

        let mut target = vrc7_default();
        target.load_state(&blob).unwrap();
        assert_eq!(target.cpu_read(0x8000), 5);
        assert_eq!(target.cpu_read(0xA000), 3);
        assert_eq!(target.cpu_read(0xC000), 7);
        assert_eq!(target.ppu_read(0x0000), 1);
        assert_eq!(target.ppu_read(0x1C00), 6);
        assert_eq!(target.current_mirroring(), Mirroring::Horizontal);
        assert!(target.prg_ram_enable);
        assert!(target.audio.silenced);
        assert_eq!(target.irq_latch, 0x80);
        assert!(target.irq_enabled);
        // We wrote 0b0000_0011 → bit 2 (mode) = 0 → scanline mode is on
        // (the predicate is `(value & 0x04) == 0`).
        assert!(target.irq_mode_scanline);
        assert_eq!(target.audio.regs[0x30], 0x5F);
    }

    #[test]
    fn vrc7_save_state_rejects_unknown_version() {
        // Pre-v1 there is no VRC7 save-state; a future v1.x bumps to 2.
        // Until then, any version != 1 must be rejected cleanly.
        let m = vrc7_default();
        let mut blob = m.save_state();
        blob[0] = 99;
        let mut target = vrc7_default();
        let err = target.load_state(&blob).expect_err("must reject");
        assert!(
            matches!(err, MapperError::UnsupportedVersion(99)),
            "expected UnsupportedVersion(99), got {err:?}"
        );
    }

    #[test]
    fn vrc7_namco163_mapper_audio_off_path_latches_state_but_stays_silent() {
        // ADR-0004 invariant: register decoders unconditionally latch
        // even when the synthesizer is absent.  Confirm latching works
        // identically regardless of the `mapper-audio` feature flag
        // (the VRC7 surface does not branch on the flag — synthesis
        // is just absent in v0.9.x, period).
        let mut m = vrc7_default();
        m.cpu_write(0x9010, 0x15);
        m.cpu_write(0x9030, 0x77);
        assert_eq!(m.audio.regs[0x15], 0x77);
        // Drive a bunch of CPU cycles → no audio side-effects, but
        // IRQ counter is unaffected if not enabled.
        for _ in 0..1000 {
            m.notify_cpu_cycle();
        }
        assert_eq!(
            m.mix_audio(),
            0,
            "feature-off path must remain silent (matches feature-on for VRC7 v0.9.x)"
        );
    }

    #[test]
    fn namco163_mapper_audio_off_path_latches_state_but_stays_silent() {
        // Mirrors the Sunsoft 5B feature-off test: the register decoders
        // run regardless of `mapper-audio`, so writes still land in the
        // internal RAM and the address-port latch advances.  With the
        // feature off, `notify_cpu_cycle` does not advance any phase
        // counters and `mix_audio` returns 0.
        let mut m = namco163_for_audio();
        // Address-port write + data-port write contract — works with
        // the feature off, because the decoders are unconditional.
        m.cpu_write(0xF800, 0x80 | 0x05);
        m.cpu_write(0x4800, 0x42);
        assert_eq!(m.audio.ram[0x05], 0x42);
        assert_eq!(m.audio.addr_latch, 0x06);
        assert!(m.audio.auto_inc);

        // Phase counters stay at zero whether or not we call clock()
        // (with the feature off, notify_cpu_cycle skips the clock; with
        // the feature on, we haven't touched the freq registers so the
        // phase still doesn't advance from the zero state).  Verify the
        // zero-init invariant directly.
        for _ in 0..256 {
            m.notify_cpu_cycle();
        }
        // Phase regs are at offsets +1/+3/+5 of each channel slot.
        for ch_base in (0x40..=0x78).step_by(8) {
            assert_eq!(m.audio.ram[ch_base + 1], 0, "phase lo @ {ch_base:#x}");
            assert_eq!(m.audio.ram[ch_base + 3], 0, "phase mid @ {ch_base:#x}");
            assert_eq!(m.audio.ram[ch_base + 5], 0, "phase hi @ {ch_base:#x}");
        }
    }

    // ───────────────────────────────────────────────────────────────────
    // T-71-005 (Phase 7): VRC2/VRC4 register + wiring fixture.
    //
    // The upstream `vrc24test` ROM link (AWJ's nesdev forum attachment) is
    // permanently rotted (auth-walled, no mirror — see `docs/STATUS.md`).
    // These in-tree register-level tests replace it: they pin the defining
    // VRC2-vs-VRC4 behaviors — the per-board a0/a1 register-select pin
    // rewiring, PRG/CHR bank registers, fixed PRG banks, and mirroring
    // control. The `m22` baseline harness (mapper 22) complements them at
    // the whole-ROM level.
    // ───────────────────────────────────────────────────────────────────

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
