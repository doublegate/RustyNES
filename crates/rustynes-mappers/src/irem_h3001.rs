//! Irem H3001 (iNES mapper 65) implementation.
//!
//! Used by Daiku no Gen-san 2, Kaiketsu Yanchamaru 3 (Spartan X 2), etc.
//! It has a VRC4/MMC3-like banking surface and a 16-bit CPU-cycle IRQ that
//! reloads from a write-high/write-low latch and counts **down** each M2.
//!
//! # Registers (`nesdev_wiki/INES_Mapper_065.xhtml`)
//!
//! | Addr        | Purpose                                                  |
//! |-------------|----------------------------------------------------------|
//! | `$8000`     | PRG reg 0 (8 KiB @ `$8000`, or @ `$C000` per `$9000`)    |
//! | `$9000`     | bit 7 = PRG bank layout                                  |
//! | `$9001`     | bits 6-7 = mirroring (00=V, 10=H, 01/11=1scA)            |
//! | `$9003`     | bit 7 = IRQ enable                                       |
//! | `$9004`     | (any write) reload IRQ counter from the 16-bit value     |
//! | `$9005`     | IRQ reload value HIGH 8 bits                              |
//! | `$9006`     | IRQ reload value LOW 8 bits                              |
//! | `$A000`     | PRG reg 1 (8 KiB @ `$A000`)                               |
//! | `$B000-7`   | eight 1 KiB CHR bank regs (`$0000`-`$1C00`)               |
//!
//! `$E000` is always fixed to bank `$3F`; the second-fixed bank (`$8000` or
//! `$C000`, depending on `$9000` bit 7) is `$3E`.
//!
//! Powerup quirk: `$8000` reads as `$00` and `$A000` as `$01` (games rely on
//! it). We initialise both registers accordingly.
//!
//! # IRQ
//!
//! A 16-bit down-counter decrements by 1 each CPU cycle while enabled; when it
//! reaches 0 it asserts an IRQ and **stops** (no wrap, no auto-reload). Any
//! write to `$9003` or `$9004` acknowledges a pending IRQ; `$9004` also copies
//! the 16-bit reload value into the counter. `$9005`/`$9006` set the reload
//! value only (note `$9005` is the HIGH byte).
//!
//! Reuses the CPU-cycle IRQ family pattern (`sprint3.rs`, `vrc3.rs`).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::match_same_arms,
    clippy::doc_markdown
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

/// Irem H3001 mapper (iNES mapper 65).
pub struct IremH3001 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    prg_reg0: u8,
    prg_reg1: u8,
    prg_layout: bool,  // $9000 bit 7
    chr_regs: [u8; 8], // eight 1 KiB CHR bank selects

    mirroring: Mirroring,

    irq_reload: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl IremH3001 {
    /// Construct a new H3001 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB; CHR-ROM (when present)
    /// must be a multiple of 1 KiB. CHR-RAM (8 KiB) is allocated when no
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
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "H3001 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_data: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "H3001 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr_data,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            // Powerup values games rely on.
            prg_reg0: 0x00,
            prg_reg1: 0x01,
            prg_layout: false,
            chr_regs: [0; 8],
            mirroring,
            irq_reload: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        // $3E and $3F are the canonical second-last / last fixed banks but a
        // small synthetic ROM may have fewer banks — mask into range.
        let fixed_3e = 0x3E % total;
        let fixed_3f = 0x3F % total;
        let bank = match (addr & 0xE000, self.prg_layout) {
            (0x8000, false) => self.prg_reg0 as usize,
            (0x8000, true) => fixed_3e,
            (0xA000, _) => self.prg_reg1 as usize,
            (0xC000, false) => fixed_3e,
            (0xC000, true) => self.prg_reg0 as usize,
            (0xE000, _) => fixed_3f,
            _ => 0,
        };
        (bank % total) * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank = (self.chr_regs[slot] as usize) % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for IremH3001 {
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
        match addr {
            0x8000..=0x8FFF => self.prg_reg0 = value,
            0x9000..=0x9FFF => match addr & 0x0007 {
                0 => self.prg_layout = (value & 0x80) != 0,
                1 => {
                    self.mirroring = match (value >> 6) & 0x03 {
                        0 => Mirroring::Vertical,
                        2 => Mirroring::Horizontal,
                        // 1 and 3 -> one-screen A.
                        _ => Mirroring::SingleScreenA,
                    };
                }
                3 => {
                    self.irq_enabled = (value & 0x80) != 0;
                    self.irq_pending = false;
                }
                4 => {
                    self.irq_counter = self.irq_reload;
                    self.irq_pending = false;
                }
                5 => self.irq_reload = (self.irq_reload & 0x00FF) | ((value as u16) << 8),
                6 => self.irq_reload = (self.irq_reload & 0xFF00) | (value as u16),
                _ => {}
            },
            0xA000..=0xAFFF => self.prg_reg1 = value,
            0xB000..=0xBFFF => {
                let slot = (addr & 0x0007) as usize;
                self.chr_regs[slot] = value;
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
                    let len = self.chr_rom.len();
                    self.chr_rom[off % len] = value;
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
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }
        self.irq_counter -= 1;
        if self.irq_counter == 0 {
            self.irq_pending = true;
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
            mapper_id: 65,
            name: "Irem H3001 (65)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("layout".into(), format!("{}", u8::from(self.prg_layout))));
        info.prg_banks
            .push(("reg0".into(), format!("{:#04x}", self.prg_reg0)));
        info.prg_banks
            .push(("reg1".into(), format!("{:#04x}", self.prg_reg1)));
        for (i, b) in self.chr_regs.iter().enumerate() {
            info.chr_banks.push((format!("C{i}"), format!("{b:#04x}")));
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
        let mut out = Vec::with_capacity(
            24 + self.vram.len()
                + if self.chr_is_ram {
                    self.chr_rom.len()
                } else {
                    0
                },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_reg0);
        out.push(self.prg_reg1);
        out.push(u8::from(self.prg_layout));
        out.extend_from_slice(&self.chr_regs);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.irq_reload.to_le_bytes());
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        // 1 + 1 + 1 + 1 + 8 (chr regs) + 1 (mir) + 2 + 2 + 1 + 1
        let scalar_len = 1 + 1 + 1 + 1 + 8 + 1 + 2 + 2 + 1 + 1;
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
        self.prg_reg0 = data[c];
        c += 1;
        self.prg_reg1 = data[c];
        c += 1;
        self.prg_layout = data[c] != 0;
        c += 1;
        self.chr_regs.copy_from_slice(&data[c..c + 8]);
        c += 8;
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
        self.irq_reload = u16::from_le_bytes([data[c], data[c + 1]]);
        c += 2;
        self.irq_counter = u16::from_le_bytes([data[c], data[c + 1]]);
        c += 2;
        self.irq_enabled = data[c] != 0;
        c += 1;
        self.irq_pending = data[c] != 0;
        c += 1;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[c..c + self.chr_rom.len()]);
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

    fn fresh() -> IremH3001 {
        // 64 banks of 8 KiB = 512 KiB so $3E/$3F resolve to distinct banks.
        IremH3001::new(synth_prg(64), synth_chr(64), Mirroring::Vertical).unwrap()
    }

    #[test]
    fn powerup_register_values() {
        let mut m = fresh();
        // $8000 reg0 = 0, $A000 reg1 = 1 at power-on.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 1);
    }

    #[test]
    fn prg_layout_0_fixes_c000_to_3e_and_e000_to_3f() {
        let mut m = fresh();
        m.cpu_write(0x8000, 5); // reg0 -> bank 5 @ $8000
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 0x3E);
        assert_eq!(m.cpu_read(0xE000), 0x3F);
    }

    #[test]
    fn prg_layout_1_swaps_to_c000() {
        let mut m = fresh();
        m.cpu_write(0x8000, 5); // reg0
        m.cpu_write(0x9000, 0x80); // layout bit
        assert_eq!(m.cpu_read(0x8000), 0x3E); // fixed
        assert_eq!(m.cpu_read(0xC000), 5); // reg0 here now
        assert_eq!(m.cpu_read(0xE000), 0x3F);
    }

    #[test]
    fn chr_eight_1k_banks() {
        let mut m = fresh();
        m.cpu_write(0xB000, 10);
        m.cpu_write(0xB004, 20);
        assert_eq!(m.ppu_read(0x0000), 10);
        assert_eq!(m.ppu_read(0x1000), 20);
    }

    #[test]
    fn mirroring_select() {
        let mut m = fresh();
        m.cpu_write(0x9001, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0x9001, 0x80); // %10
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x9001, 0x40); // %01
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn irq_counts_down_and_asserts() {
        let mut m = fresh();
        m.cpu_write(0x9005, 0x00); // reload high
        m.cpu_write(0x9006, 0x03); // reload low -> 3
        m.cpu_write(0x9004, 0x00); // load counter from reload
        m.cpu_write(0x9003, 0x80); // enable
        m.notify_cpu_cycle(); // 3 -> 2
        m.notify_cpu_cycle(); // 2 -> 1
        assert!(!m.irq_pending());
        m.notify_cpu_cycle(); // 1 -> 0 -> assert
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_stops_at_zero_no_wrap() {
        let mut m = fresh();
        m.cpu_write(0x9005, 0x00);
        m.cpu_write(0x9006, 0x01);
        m.cpu_write(0x9004, 0x00);
        m.cpu_write(0x9003, 0x80);
        m.notify_cpu_cycle(); // -> 0, assert
        assert!(m.irq_pending());
        assert_eq!(m.irq_counter, 0);
        // Many more cycles: counter must NOT wrap below 0.
        for _ in 0..1000 {
            m.notify_cpu_cycle();
        }
        assert_eq!(m.irq_counter, 0);
    }

    #[test]
    fn write_9003_or_9004_acks() {
        let mut m = fresh();
        m.irq_pending = true;
        m.cpu_write(0x9003, 0x00); // ack (and disable)
        assert!(!m.irq_pending());
        m.irq_pending = true;
        m.cpu_write(0x9004, 0x00); // ack + reload
        assert!(!m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh();
        m.cpu_write(0x8000, 5);
        m.cpu_write(0xB002, 7);
        m.cpu_write(0x9005, 0x12);
        m.cpu_write(0x9006, 0x34);
        m.cpu_write(0x9004, 0x00);
        m.cpu_write(0x9003, 0x80);
        m.ppu_write(0x2000, 0x55);
        let blob = m.save_state();
        let mut m2 = fresh();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0800), m2.ppu_read(0x0800));
        assert_eq!(m.irq_counter, m2.irq_counter);
        assert_eq!(m.ppu_read(0x2000), m2.ppu_read(0x2000));
    }
}
