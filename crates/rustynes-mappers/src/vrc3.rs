//! Konami VRC3 (iNES mapper 73) implementation.
//!
//! The simplest Konami VRC: used only by Salamander (JP). It has:
//!
//! - 8 KiB optional PRG-RAM at `$6000-$7FFF`.
//! - A 16 KiB switchable PRG bank at `$8000-$BFFF` (selected by `$F000`).
//! - A 16 KiB PRG bank fixed to the last bank at `$C000-$FFFF`.
//! - 8 KiB CHR-RAM (no CHR banking).
//! - Fixed H/V mirroring from the iNES header (solder pads).
//! - A 16-bit CPU-cycle IRQ counter (no scanline mode, no CHR banking).
//!
//! # IRQ counter (`nesdev_wiki/VRC3.xhtml`)
//!
//! The 16-bit latch is written nibble-at-a-time across `$8000-$BFFF`:
//! `$8xxx` = bits 0-3, `$9xxx` = bits 4-7, `$Axxx` = bits 8-11, `$Bxxx` =
//! bits 12-15. `$Cxxx` is IRQ control `[.... .MEA]` (M = 8-bit mode, E =
//! enable, A = enable-on-acknowledge). `$Dxxx` acknowledges and copies A
//! into E.
//!
//! When enabled, the counter increments every CPU cycle. On overflow from
//! `$FFFF` (or `$FF` in 8-bit mode), an IRQ is asserted and the counter is
//! reloaded from the latch (8-bit mode reloads only the low 8 bits). Writing
//! `$C000` with E set reloads all 16 bits regardless of mode.
//!
//! Reuses the VRC CPU-cycle IRQ family pattern (`sprint3.rs`).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::doc_markdown
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_RAM_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Konami VRC3 mapper (iNES mapper 73).
pub struct Vrc3 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    prg_bank: u8,
    mirroring: Mirroring,

    // 16-bit IRQ latch + counter.
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_enable_after_ack: bool,
    irq_mode_8bit: bool,
    irq_pending: bool,
}

impl Vrc3 {
    /// Construct a new VRC3 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB. CHR is always 8 KiB
    /// of RAM (the board has no CHR-ROM); any supplied CHR-ROM is ignored.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on PRG size mismatch.
    pub fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "VRC3 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_RAM_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            prg_bank: 0,
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enable_after_ack: false,
            irq_mode_8bit: false,
            irq_pending: false,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for Vrc3 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xBFFF => {
                let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let bank = (self.prg_bank as usize) % total;
                self.prg_rom[bank * PRG_BANK_16K + (addr - 0x8000) as usize]
            }
            0xC000..=0xFFFF => {
                let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                let last = total - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr - 0xC000) as usize]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                let len = self.prg_ram.len();
                self.prg_ram[(addr - 0x6000) as usize % len] = value;
            }
            // IRQ latch nibbles.
            0x8000..=0x8FFF => {
                self.irq_latch = (self.irq_latch & 0xFFF0) | (value as u16 & 0x0F);
            }
            0x9000..=0x9FFF => {
                self.irq_latch = (self.irq_latch & 0xFF0F) | ((value as u16 & 0x0F) << 4);
            }
            0xA000..=0xAFFF => {
                self.irq_latch = (self.irq_latch & 0xF0FF) | ((value as u16 & 0x0F) << 8);
            }
            0xB000..=0xBFFF => {
                self.irq_latch = (self.irq_latch & 0x0FFF) | ((value as u16 & 0x0F) << 12);
            }
            // IRQ control.
            0xC000..=0xCFFF => {
                self.irq_enable_after_ack = (value & 0x01) != 0;
                self.irq_enabled = (value & 0x02) != 0;
                self.irq_mode_8bit = (value & 0x04) != 0;
                self.irq_pending = false;
                if self.irq_enabled {
                    // Reload all 16 bits regardless of mode.
                    self.irq_counter = self.irq_latch;
                }
            }
            // IRQ acknowledge.
            0xD000..=0xDFFF => {
                self.irq_pending = false;
                self.irq_enabled = self.irq_enable_after_ack;
            }
            // PRG bank select.
            0xF000..=0xFFFF => {
                self.prg_bank = value & 0x07;
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
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
        if self.irq_mode_8bit {
            // Only the low 8 bits count; on overflow from $FF reload low 8.
            let lo = (self.irq_counter & 0x00FF) as u8;
            if lo == 0xFF {
                self.irq_counter = (self.irq_counter & 0xFF00) | (self.irq_latch & 0x00FF);
                self.irq_pending = true;
            } else {
                self.irq_counter = (self.irq_counter & 0xFF00) | ((lo as u16) + 1);
            }
        } else if self.irq_counter == 0xFFFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
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
            mapper_id: 73,
            name: "Konami VRC3 (73)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        info.irq_state
            .push(("latch".into(), format!("{:#06x}", self.irq_latch)));
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("8bit".into(), format!("{}", self.irq_mode_8bit)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(12 + self.vram.len() + self.chr_ram.len() + self.prg_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_latch.to_le_bytes());
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_enable_after_ack));
        out.push(u8::from(self.irq_mode_8bit));
        out.push(u8::from(self.irq_pending));
        out.push(0); // reserved padding -> 12-byte header
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.prg_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 12 + self.vram.len() + self.chr_ram.len() + self.prg_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.mirroring = match data[2] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.irq_latch = u16::from_le_bytes([data[3], data[4]]);
        self.irq_counter = u16::from_le_bytes([data[5], data[6]]);
        self.irq_enabled = data[7] != 0;
        self.irq_enable_after_ack = data[8] != 0;
        self.irq_mode_8bit = data[9] != 0;
        self.irq_pending = data[10] != 0;
        // data[11] is reserved padding for alignment with the 12-byte header.
        let mut cursor = 12;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_16k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_16k * PRG_BANK_16K];
        for b in 0..banks_16k {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn prg_bank_select_and_fixed_last() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7);
        m.cpu_write(0xF000, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn irq_latch_nibble_assembly() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x0A); // bits 0-3
        m.cpu_write(0x9000, 0x0B); // bits 4-7
        m.cpu_write(0xA000, 0x0C); // bits 8-11
        m.cpu_write(0xB000, 0x0D); // bits 12-15
        assert_eq!(m.irq_latch, 0xDCBA);
    }

    #[test]
    fn irq_16bit_fires_after_reload_count() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        // Latch = 0xFFFE: from reload, two increments reach 0xFFFF then overflow.
        m.cpu_write(0x8000, 0x0E);
        m.cpu_write(0x9000, 0x0F);
        m.cpu_write(0xA000, 0x0F);
        m.cpu_write(0xB000, 0x0F);
        assert_eq!(m.irq_latch, 0xFFFE);
        // Enable (bit 1) + reload all 16 bits.
        m.cpu_write(0xC000, 0x02);
        assert_eq!(m.irq_counter, 0xFFFE);
        m.notify_cpu_cycle(); // 0xFFFE -> 0xFFFF
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // 0xFFFF -> overflow -> IRQ + reload
        assert!(m.irq_pending());
        assert_eq!(m.irq_counter, 0xFFFE);
    }

    #[test]
    fn irq_8bit_mode_only_low_byte() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x0E); // latch low nibble
        m.cpu_write(0x9000, 0x0F); // latch bits 4-7 -> low byte = 0xFE
        // Enable + 8-bit mode (bit 1 | bit 2).
        m.cpu_write(0xC000, 0x06);
        assert_eq!(m.irq_counter & 0xFF, 0xFE);
        m.notify_cpu_cycle(); // 0xFE -> 0xFF
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // 0xFF -> overflow -> IRQ
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_acknowledge_moves_a_into_e() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        // Enable now (E=1) and set enable-after-ack (A=1).
        m.cpu_write(0xC000, 0x03);
        // Force an IRQ by writing $D000 should clear pending; E := A (1).
        m.irq_pending = true;
        m.cpu_write(0xD000, 0x00);
        assert!(!m.irq_pending());
        assert!(m.irq_enabled);
    }

    #[test]
    fn disabled_counter_does_not_increment() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        for _ in 0..10_000 {
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending());
        assert_eq!(m.irq_counter, 0);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xF000, 3);
        m.cpu_write(0x8000, 0x05);
        m.cpu_write(0xC000, 0x02);
        m.ppu_write(0x0040, 0x99);
        let blob = m.save_state();
        let mut m2 = Vrc3::new(synth_prg(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0040), m2.ppu_read(0x0040));
        assert_eq!(m.irq_counter, m2.irq_counter);
    }
}
