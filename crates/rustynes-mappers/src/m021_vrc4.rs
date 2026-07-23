//! Konami VRC4 (mappers 21, 23, 25).
//!
//! Register-compatible with the VRC2 in `m022_vrc2.rs` but *pin-rewired* per
//! board revision, which is why three mapper numbers describe one chip. That
//! rewiring is isolated in [`vrc_a_bits`] (duplicated here rather than shared,
//! matching how the crate treats its other small helpers).
//!
//! What VRC4 adds over VRC2 is the VRC IRQ counter -- the scanline/CPU-cycle
//! counter shared with VRC3, VRC6 and VRC7 -- plus 8 KiB of PRG-RAM on the
//! save-bearing Konami cartridges. No on-cart audio; see `m024_vrc6.rs` and
//! `m085_vrc7.rs` for the boards that have it.
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
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
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
}
