//! Jaleco SS88006 (iNES mapper 18) implementation.
//!
//! Used by Ganbare Goemon Gaiden, Magical Kids Doropie (The Krion
//! Conquest), Pizza Pop, etc. The mapper is wired only to A12-A14, A0-A1,
//! and D0-D3, so every bank number is written as two 4-bit nibbles across a
//! pair of sequential addresses (low nibble at the even/first address, high
//! nibble at the next).
//!
//! # Banking (`nesdev_wiki/INES_Mapper_018.xhtml`)
//!
//! - PRG: three 8 KiB switchable banks at `$8000` / `$A000` / `$C000`; the
//!   last 8 KiB is fixed at `$E000`. Bank 0 is 6 bits ($8000 low 4 + $8001
//!   low 2); banks 1/2 are similar pairs ($8002/$8003, $9000/$9001).
//! - CHR: eight 1 KiB banks; each is an 8-bit value split low4/high4 across
//!   `$A000-$DFFF` address pairs.
//! - Mirroring control at `$F002` (0 H, 1 V, 2 1scA, 3 1scB).
//!
//! # IRQ
//!
//! A 16-bit reload value is written as four nibbles to `$E000-$E003`. A
//! write to `$F000` reloads the counter from the reload value and
//! acknowledges. `$F001` selects the counter width via bits F/E/T (don't
//! propagate the borrow past bit 12 / 8 / 4 respectively; F overrides E
//! overrides T; none set = full 16-bit) and bit 0 enables counting. When
//! enabled the counter counts down each M2 cycle; when the selected window
//! borrows (underflows past its low bits) the IRQ asserts.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown,
    clippy::struct_excessive_bools
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Jaleco SS88006 mapper (iNES mapper 18).
pub struct JalecoSs88006 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    chr_is_ram: bool,

    // Three switchable 8 KiB PRG banks.
    prg_banks: [u8; 3],
    // Eight 1 KiB CHR banks.
    chr_banks: [u8; 8],
    mirroring: Mirroring,

    prg_ram_enable: bool,
    prg_ram_write: bool,

    // 16-bit IRQ reload value + counter.
    irq_reload: u16,
    irq_counter: u16,
    irq_enabled: bool,
    // Counter-width mask selected by the F/E/T bits ($F001 bits 3-1).
    irq_width: IrqWidth,
    irq_pending: bool,
}

/// Effective IRQ counter width selected by `$F001` bits F/E/T.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IrqWidth {
    Bits16,
    Bits12,
    Bits8,
    Bits4,
}

impl IrqWidth {
    const fn from_ctrl(value: u8) -> Self {
        // F (bit 3) overrides E (bit 2) overrides T (bit 1).
        if value & 0x08 != 0 {
            Self::Bits4
        } else if value & 0x04 != 0 {
            Self::Bits8
        } else if value & 0x02 != 0 {
            Self::Bits12
        } else {
            Self::Bits16
        }
    }

    /// Mask of the active counter bits.
    const fn mask(self) -> u16 {
        match self {
            Self::Bits16 => 0xFFFF,
            Self::Bits12 => 0x0FFF,
            Self::Bits8 => 0x00FF,
            Self::Bits4 => 0x000F,
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Self::Bits16 => 0,
            Self::Bits12 => 1,
            Self::Bits8 => 2,
            Self::Bits4 => 3,
        }
    }

    const fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Bits12,
            2 => Self::Bits8,
            3 => Self::Bits4,
            _ => Self::Bits16,
        }
    }
}

impl JalecoSs88006 {
    /// Construct a new SS88006 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB; CHR-ROM must be a
    /// multiple of 1 KiB (CHR-RAM allocated as 8 KiB when empty).
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
                "SS88006 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "SS88006 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            chr_is_ram,
            prg_banks: [0, 1, 2],
            chr_banks: [0; 8],
            mirroring,
            prg_ram_enable: true,
            prg_ram_write: true,
            irq_reload: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_width: IrqWidth::Bits16,
            irq_pending: false,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_banks[0] as usize) % total,
            0xA000 => (self.prg_banks[1] as usize) % total,
            0xC000 => (self.prg_banks[2] as usize) % total,
            0xE000 => last,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let slot = (addr as usize / CHR_BANK_1K) & 0x07;
        let total = (self.chr.len() / CHR_BANK_1K).max(1);
        let bank = (self.chr_banks[slot] as usize) % total;
        bank * CHR_BANK_1K + (addr as usize & (CHR_BANK_1K - 1))
    }

    fn set_nibble(reg: &mut u8, high: bool, value: u8) {
        let v = value & 0x0F;
        if high {
            *reg = (*reg & 0x0F) | (v << 4);
        } else {
            *reg = (*reg & 0xF0) | v;
        }
    }

    /// Set one nibble of a 16-bit register at `nibble` index (0 = bits 0-3,
    /// 1 = bits 4-7, ...).
    fn set_reload_nibble(&mut self, nibble: u8, value: u8) {
        let shift = (nibble & 0x03) * 4;
        let v = (value as u16) & 0x0F;
        self.irq_reload = (self.irq_reload & !(0x000F << shift)) | (v << shift);
    }
}

impl Mapper for JalecoSs88006 {
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
            0x6000..=0x7FFF => {
                if self.prg_ram_enable {
                    self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()]
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            if self.prg_ram_enable && self.prg_ram_write {
                let len = self.prg_ram.len();
                self.prg_ram[(addr - 0x6000) as usize % len] = value;
            }
            return;
        }
        // Address decoded as A14-A12 (register group) + A1-A0 (register).
        let reg = addr & 0xF003;
        match reg {
            // PRG bank 0 low / high (high only has 2 bits).
            0x8000 => Self::set_nibble(&mut self.prg_banks[0], false, value),
            0x8001 => {
                let cur = self.prg_banks[0];
                self.prg_banks[0] = (cur & 0x0F) | ((value & 0x03) << 4);
            }
            // PRG bank 1.
            0x8002 => Self::set_nibble(&mut self.prg_banks[1], false, value),
            0x8003 => {
                let cur = self.prg_banks[1];
                self.prg_banks[1] = (cur & 0x0F) | ((value & 0x03) << 4);
            }
            // PRG bank 2.
            0x9000 => Self::set_nibble(&mut self.prg_banks[2], false, value),
            0x9001 => {
                let cur = self.prg_banks[2];
                self.prg_banks[2] = (cur & 0x0F) | ((value & 0x03) << 4);
            }
            // PRG-RAM protect.
            0x9002 => {
                self.prg_ram_enable = (value & 0x01) != 0;
                self.prg_ram_write = (value & 0x02) != 0;
            }
            // CHR banks 0-7, each low/high nibble pair.
            0xA000 => Self::set_nibble(&mut self.chr_banks[0], false, value),
            0xA001 => Self::set_nibble(&mut self.chr_banks[0], true, value),
            0xA002 => Self::set_nibble(&mut self.chr_banks[1], false, value),
            0xA003 => Self::set_nibble(&mut self.chr_banks[1], true, value),
            0xB000 => Self::set_nibble(&mut self.chr_banks[2], false, value),
            0xB001 => Self::set_nibble(&mut self.chr_banks[2], true, value),
            0xB002 => Self::set_nibble(&mut self.chr_banks[3], false, value),
            0xB003 => Self::set_nibble(&mut self.chr_banks[3], true, value),
            0xC000 => Self::set_nibble(&mut self.chr_banks[4], false, value),
            0xC001 => Self::set_nibble(&mut self.chr_banks[4], true, value),
            0xC002 => Self::set_nibble(&mut self.chr_banks[5], false, value),
            0xC003 => Self::set_nibble(&mut self.chr_banks[5], true, value),
            0xD000 => Self::set_nibble(&mut self.chr_banks[6], false, value),
            0xD001 => Self::set_nibble(&mut self.chr_banks[6], true, value),
            0xD002 => Self::set_nibble(&mut self.chr_banks[7], false, value),
            0xD003 => Self::set_nibble(&mut self.chr_banks[7], true, value),
            // IRQ reload value nibbles.
            0xE000 => self.set_reload_nibble(0, value),
            0xE001 => self.set_reload_nibble(1, value),
            0xE002 => self.set_reload_nibble(2, value),
            0xE003 => self.set_reload_nibble(3, value),
            // IRQ reload (copy reload value into counter + ack).
            0xF000 => {
                self.irq_counter = self.irq_reload;
                self.irq_pending = false;
            }
            // IRQ counter size + enable + ack.
            0xF001 => {
                self.irq_enabled = (value & 0x01) != 0;
                self.irq_width = IrqWidth::from_ctrl(value);
                self.irq_pending = false;
            }
            // Mirroring.
            0xF002 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Horizontal,
                    1 => Mirroring::Vertical,
                    2 => Mirroring::SingleScreenA,
                    _ => Mirroring::SingleScreenB,
                };
            }
            // $F003: expansion sound (uPD7755C ADPCM) — not emulated.
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr[off % self.chr.len()]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    let len = self.chr.len();
                    self.chr[off % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        // Count down only the active-width low bits; an IRQ fires when those
        // bits borrow (i.e. were zero before the decrement). The unused high
        // bits are preserved.
        let mask = self.irq_width.mask();
        let active = self.irq_counter & mask;
        if active == 0 {
            self.irq_pending = true;
        }
        let new_active = active.wrapping_sub(1) & mask;
        self.irq_counter = (self.irq_counter & !mask) | new_active;
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 18,
            name: "Jaleco SS88006 (18)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        for (i, b) in self.prg_banks.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_banks.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("reload".into(), format!("{:#06x}", self.irq_reload)));
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(22 + self.vram.len() + self.prg_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_banks);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.prg_ram_enable));
        out.push(u8::from(self.prg_ram_write));
        out.extend_from_slice(&self.irq_reload.to_le_bytes());
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(self.irq_width.to_byte());
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.prg_ram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 22 + self.vram.len() + self.prg_ram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_banks.copy_from_slice(&data[1..4]);
        self.chr_banks.copy_from_slice(&data[4..12]);
        self.mirroring = match data[12] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.prg_ram_enable = data[13] != 0;
        self.prg_ram_write = data[14] != 0;
        self.irq_reload = u16::from_le_bytes([data[15], data[16]]);
        self.irq_counter = u16::from_le_bytes([data[17], data[18]]);
        // Bytes 19/20/21 = enabled / width / pending.
        let mut cursor = 19;
        self.irq_enabled = data[cursor] != 0;
        cursor += 1;
        self.irq_width = IrqWidth::from_byte(data[cursor]);
        cursor += 1;
        self.irq_pending = data[cursor] != 0;
        cursor += 1;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
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
    fn prg_defaults_and_fixed_last() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 1);
        assert_eq!(m.cpu_read(0xC000), 2);
        assert_eq!(m.cpu_read(0xE000), 15); // last fixed
    }

    #[test]
    fn prg_bank_nibble_pair() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        // PRG bank 0 = 0x0A: low nibble 0xA at $8000, high 2 bits 0 at $8001.
        m.cpu_write(0x8000, 0x0A);
        assert_eq!(m.cpu_read(0x8000), 0x0A);
        // Set high 2 bits -> bank 0x1A masked to 16 banks = 0x0A still in range?
        m.cpu_write(0x8001, 0x01); // high bits -> 0x1A = 26 % 16 = 10
        assert_eq!(m.cpu_read(0x8000), 26 % 16);
    }

    #[test]
    fn chr_bank_nibble_pair() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        // CHR bank 0 = 0x1B: low nibble 0xB at $A000, high nibble 0x1 at $A001.
        m.cpu_write(0xA000, 0x0B);
        m.cpu_write(0xA001, 0x01);
        assert_eq!(m.ppu_read(0x0000), 0x1B);
        // CHR bank 4 ($1000) via $C000/$C001.
        m.cpu_write(0xC000, 0x05);
        assert_eq!(m.ppu_read(0x1000), 5);
    }

    #[test]
    fn mirroring_control() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        m.cpu_write(0xF002, 0); // Horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0xF002, 1); // Vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0xF002, 3); // 1scB
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn irq_reload_value_nibbles() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 0x0A);
        m.cpu_write(0xE001, 0x0B);
        m.cpu_write(0xE002, 0x0C);
        m.cpu_write(0xE003, 0x0D);
        assert_eq!(m.irq_reload, 0xDCBA);
    }

    #[test]
    fn irq_16bit_counts_down_to_borrow() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        // Reload = 2.
        m.cpu_write(0xE000, 0x02);
        m.cpu_write(0xF000, 0x00); // reload counter
        assert_eq!(m.irq_counter, 2);
        m.cpu_write(0xF001, 0x01); // enable, 16-bit width
        m.notify_cpu_cycle(); // 2 -> 1
        m.notify_cpu_cycle(); // 1 -> 0
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // 0 -> borrow -> IRQ
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_width_8bit_ignores_high_bits() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        // Reload = 0x0102; in 8-bit width only low byte (0x02) counts.
        m.cpu_write(0xE000, 0x02);
        m.cpu_write(0xE001, 0x00);
        m.cpu_write(0xE002, 0x01);
        m.cpu_write(0xF000, 0x00);
        // Enable + E bit (bit 2) = 8-bit width.
        m.cpu_write(0xF001, 0x01 | 0x04);
        m.notify_cpu_cycle(); // low byte 2 -> 1
        m.notify_cpu_cycle(); // 1 -> 0
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // borrow on low byte -> IRQ
        assert!(m.irq_pending());
        // High byte preserved.
        assert_eq!(m.irq_counter & 0xFF00, 0x0100);
    }

    #[test]
    fn irq_reload_acknowledges() {
        let mut m = JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Vertical).unwrap();
        m.irq_pending = true;
        m.cpu_write(0xF000, 0x00);
        assert!(!m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m =
            JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x8000, 0x05);
        m.cpu_write(0xA000, 0x03);
        m.cpu_write(0xE000, 0x07);
        m.cpu_write(0xF000, 0x00);
        m.cpu_write(0xF001, 0x05);
        let blob = m.save_state();
        let mut m2 =
            JalecoSs88006::new(synth_prg(16), synth_chr(32), Mirroring::Horizontal).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.irq_counter, m2.irq_counter);
        assert_eq!(m.irq_width, m2.irq_width);
    }
}
