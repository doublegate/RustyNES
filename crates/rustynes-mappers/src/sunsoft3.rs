//! Sunsoft-3 (iNES mapper 67) implementation.
//!
//! Used by Fantasy Zone II (J), Mito Koumon II, and the Vs. game Vs. Platoon.
//!
//! # Banking (`nesdev_wiki/INES_Mapper_067.xhtml`)
//!
//! - `$8000-$BFFF`: 16 KiB switchable PRG bank (selected via `$F800`).
//! - `$C000-$FFFF`: 16 KiB PRG bank, fixed to the last bank.
//! - PPU `$0000`/`$0800`/`$1000`/`$1800`: four 2 KiB CHR banks.
//!
//! # Registers (each occupies a `$0800`-aligned range)
//!
//! | Addr    | Purpose                                                       |
//! |---------|---------------------------------------------------------------|
//! | `$8800` | CHR bank 0 (2 KiB @ `$0000`)                                   |
//! | `$9800` | CHR bank 1 (2 KiB @ `$0800`)                                   |
//! | `$A800` | CHR bank 2 (2 KiB @ `$1000`)                                   |
//! | `$B800` | CHR bank 3 (2 KiB @ `$1800`)                                   |
//! | `$C800` | IRQ load (write twice: high then low)                         |
//! | `$D800` | IRQ enable (bit 4) + resets the `$C800` write toggle          |
//! | `$E800` | mirroring (bits 0-1: 0=V, 1=H, 2=1scA, 3=1scB)                |
//! | `$F800` | PRG bank (bits 0-3) @ `$8000-$BFFF`                            |
//! | `$8000` | (and mirrors) interrupt acknowledge                           |
//!
//! # IRQ
//!
//! A 16-bit down-counter (loaded directly via the write-twice `$C800`
//! register, NOT a separate reload latch) decrements every CPU cycle while
//! enabled. When it wraps `$0000`→`$FFFF` the mapper asserts an IRQ and
//! pauses itself (clears its own enable). Any write to `$D800` resets the
//! `$C800` write toggle so the next `$C800` write is the high byte. Writes to
//! `$D800` do NOT acknowledge the IRQ; only `$8000` (and its mirrors) ack.
//!
//! Reuses the CPU-cycle IRQ family pattern (`sprint3.rs`, `vrc3.rs`).

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
const CHR_BANK_2K: usize = 0x0800;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Sunsoft-3 mapper (iNES mapper 67).
pub struct Sunsoft3 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    prg_bank: u8,
    chr_banks: [u8; 4],
    mirroring: Mirroring,

    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
    // Write-twice toggle for $C800: false = next write is high byte.
    irq_write_low_next: bool,
}

impl Sunsoft3 {
    /// Construct a new Sunsoft-3 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB; CHR-ROM (when present)
    /// must be a multiple of 2 KiB. CHR-RAM (8 KiB) is allocated when no
    /// CHR-ROM is supplied.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-3 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 4 * CHR_BANK_2K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-3 CHR-ROM size {} is not a multiple of 2 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_banks: [0; 4],
            mirroring,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
            irq_write_low_next: false,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let last = total - 1;
        let bank = match addr {
            0x8000..=0xBFFF => (self.prg_bank as usize) % total,
            _ => last, // $C000-$FFFF fixed to last bank
        };
        bank * PRG_BANK_16K + (addr as usize & 0x3FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_2k = (self.chr.len() / CHR_BANK_2K).max(1);
        let slot = addr / CHR_BANK_2K;
        let bank = (self.chr_banks[slot] as usize) % total_2k;
        bank * CHR_BANK_2K + (addr & (CHR_BANK_2K - 1))
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for Sunsoft3 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // Each register occupies a $0800-aligned window; decode the top of
        // the $8000-$FFFF range.
        match addr & 0xF800 {
            0x8000 => {
                // Interrupt acknowledge ($8000 mask).
                self.irq_pending = false;
            }
            0x8800 => self.chr_banks[0] = value,
            0x9800 => self.chr_banks[1] = value,
            0xA800 => self.chr_banks[2] = value,
            0xB800 => self.chr_banks[3] = value,
            0xC800 => {
                // Write-twice 16-bit counter (high then low). Directly sets
                // the live counter.
                if self.irq_write_low_next {
                    self.irq_counter = (self.irq_counter & 0xFF00) | (value as u16);
                } else {
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((value as u16) << 8);
                }
                self.irq_write_low_next = !self.irq_write_low_next;
            }
            0xD800 => {
                self.irq_enabled = (value & 0x10) != 0;
                // Reset the $C800 write toggle (next write is the high byte).
                self.irq_write_low_next = false;
            }
            0xE800 => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenA,
                    _ => Mirroring::SingleScreenB,
                };
            }
            0xF800 => self.prg_bank = value & 0x0F,
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
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr) % self.vram.len()],
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
                let off = self.nametable_offset(addr) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_counter == 0 {
            // Wrap $0000 -> $FFFF: assert + pause.
            self.irq_counter = 0xFFFF;
            self.irq_pending = true;
            self.irq_enabled = false;
        } else {
            self.irq_counter -= 1;
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
            mapper_id: 67,
            name: "Sunsoft-3 (67)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        for (i, b) in self.chr_banks.iter().enumerate() {
            info.chr_banks.push((format!("C{i}"), format!("{b:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            16 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.chr_banks);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(u8::from(self.irq_write_low_next));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // 1 + 1 + 4 + 1 + 2 + 1 + 1 + 1
        let scalar_len = 1 + 1 + 4 + 1 + 2 + 1 + 1 + 1;
        let expected = scalar_len + self.vram.len() + chr_part;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut c = 1usize;
        self.prg_bank = data[c];
        c += 1;
        self.chr_banks.copy_from_slice(&data[c..c + 4]);
        c += 4;
        self.mirroring = match data[c] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        c += 1;
        self.irq_counter = u16::from_le_bytes([data[c], data[c + 1]]);
        c += 2;
        self.irq_enabled = data[c] != 0;
        c += 1;
        self.irq_pending = data[c] != 0;
        c += 1;
        self.irq_write_low_next = data[c] != 0;
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
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

    fn synth_chr(banks_2k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_2k * CHR_BANK_2K];
        for b in 0..banks_2k {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh() -> Sunsoft3 {
        Sunsoft3::new(synth_prg(8), synth_chr(16), Mirroring::Vertical).unwrap()
    }

    #[test]
    fn prg_bank_select_and_fixed_last() {
        let mut m = fresh();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7); // last bank fixed
        m.cpu_write(0xF800, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn chr_four_2k_banks() {
        let mut m = fresh();
        m.cpu_write(0x8800, 3); // CHR0
        m.cpu_write(0xA800, 9); // CHR2
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.ppu_read(0x1000), 9);
    }

    #[test]
    fn mirroring_select() {
        let mut m = fresh();
        m.cpu_write(0xE800, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0xE800, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0xE800, 2);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0xE800, 3);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn irq_write_twice_loads_counter_high_then_low() {
        let mut m = fresh();
        m.cpu_write(0xD800, 0x00); // reset toggle (write-high next)
        m.cpu_write(0xC800, 0x12); // high
        m.cpu_write(0xC800, 0x34); // low
        assert_eq!(m.irq_counter, 0x1234);
    }

    #[test]
    fn irq_counts_down_wraps_and_pauses() {
        let mut m = fresh();
        m.cpu_write(0xD800, 0x00); // reset toggle
        m.cpu_write(0xC800, 0x00); // high
        m.cpu_write(0xC800, 0x02); // low -> counter = 2
        m.cpu_write(0xD800, 0x10); // enable
        m.notify_cpu_cycle(); // 2 -> 1
        m.notify_cpu_cycle(); // 1 -> 0
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // 0 -> wrap -> assert + pause
        assert!(m.irq_pending());
        assert!(!m.irq_enabled, "wrap pauses the counter");
        assert_eq!(m.irq_counter, 0xFFFF);
    }

    #[test]
    fn d800_does_not_ack_8000_does() {
        let mut m = fresh();
        m.irq_pending = true;
        m.cpu_write(0xD800, 0x00); // must NOT ack
        assert!(m.irq_pending());
        m.cpu_write(0x8000, 0x00); // acknowledges
        assert!(!m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh();
        m.cpu_write(0xF800, 3);
        m.cpu_write(0x9800, 7);
        m.cpu_write(0xD800, 0x00);
        m.cpu_write(0xC800, 0xAB);
        m.cpu_write(0xC800, 0xCD);
        m.cpu_write(0xD800, 0x10);
        m.ppu_write(0x2000, 0x44);
        let blob = m.save_state();
        let mut m2 = fresh();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0800), m2.ppu_read(0x0800));
        assert_eq!(m.irq_counter, m2.irq_counter);
        assert_eq!(m.ppu_read(0x2000), m2.ppu_read(0x2000));
    }
}
