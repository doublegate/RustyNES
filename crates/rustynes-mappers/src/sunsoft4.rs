//! Sunsoft-4 (iNES mapper 68) implementation.
//!
//! Used by After Burner (US), Maharaja (J), Nantettatte!! Baseball (J). Its
//! distinguishing feature is the ability to map **CHR ROM into the nametable
//! address space** (`$2000-$2FFF`): two 1 KiB nametable-bank registers select
//! CHR-ROM pages to back the nametables, gated by an enable bit.
//!
//! # Banking (`nesdev_wiki/INES_Mapper_068.xhtml`)
//!
//! - `$6000-$7FFF`: 8 KiB PRG-RAM (gated by `$F000` bit 4).
//! - `$8000-$BFFF`: 16 KiB switchable PRG bank (`$F000` bits 0-3).
//! - `$C000-$FFFF`: 16 KiB PRG bank fixed to the last internal bank.
//! - PPU `$0000`/`$0800`/`$1000`/`$1800`: four 2 KiB CHR banks
//!   (`$8000`/`$9000`/`$A000`/`$B000`).
//!
//! # Registers (each occupies a `$1000`-aligned range)
//!
//! | Addr    | Purpose                                                       |
//! |---------|---------------------------------------------------------------|
//! | `$8000` | CHR pattern bank 0 (2 KiB @ `$0000`)                          |
//! | `$9000` | CHR pattern bank 1 (2 KiB @ `$0800`)                          |
//! | `$A000` | CHR pattern bank 2 (2 KiB @ `$1000`)                          |
//! | `$B000` | CHR pattern bank 3 (2 KiB @ `$1800`)                          |
//! | `$C000` | CHR nametable bank 0 (1 KiB; D7 ignored, treated as 1)        |
//! | `$D000` | CHR nametable bank 1 (1 KiB; D7 ignored, treated as 1)        |
//! | `$E000` | nametable control: bits 0-1 mirroring, bit 4 CIRAM/CHR-ROM    |
//! | `$F000` | PRG bank (bits 0-3) + bit 4 = enable PRG-RAM                  |
//!
//! # CHR-ROM nametables
//!
//! When `$E000` bit 4 is set, nametable fetches in `$2000-$2FFF` come from
//! CHR ROM instead of CIRAM: the lower logical nametable uses the `$C000`
//! bank, the upper uses the `$D000` bank, with the lower/upper assignment
//! following the `$E000` mirroring mode (the same H/V/1scA/1scB table used for
//! CIRAM). Only D6-D0 of `$C000`/`$D000` are used; D7 is forced to 1, so
//! nametable banks live in the last 128 KiB of CHR ROM.
//!
//! This is the canonical use of the [`Mapper::nametable_fetch`] hook (the PPU
//! consults it before reading CIRAM); CIRAM mode falls back to
//! [`Mapper::nametable_address`].

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
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Sunsoft-4 mapper (iNES mapper 68).
pub struct Sunsoft4 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    prg_bank: u8,
    prg_ram_enabled: bool,
    chr_banks: [u8; 4], // 2 KiB pattern banks
    nt_banks: [u8; 2],  // 1 KiB CHR-ROM nametable banks
    mirroring: Mirroring,
    nt_rom_mode: bool, // $E000 bit 4
}

impl Sunsoft4 {
    /// Construct a new Sunsoft-4 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB; CHR-ROM (when present)
    /// must be a multiple of 1 KiB. CHR-RAM (8 KiB) is allocated when no
    /// CHR-ROM is supplied (the CHR-ROM nametable feature then has no backing
    /// ROM and behaves as a 1 KiB-banked RAM read).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-4 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 4 * CHR_BANK_2K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_1K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-4 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            prg_ram_enabled: false,
            chr_banks: [0; 4],
            nt_banks: [0; 2],
            mirroring,
            nt_rom_mode: false,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let last = total - 1;
        let bank = match addr {
            0x8000..=0xBFFF => (self.prg_bank as usize) % total,
            _ => last,
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

    /// Map a logical nametable index (0..=3) to the physical lower/upper bank
    /// (0 or 1) under the current `$E000` mirroring mode.
    fn nt_physical(&self, table: u8) -> usize {
        self.mirroring.physical_bank(table)
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        self.nt_physical(table) * NAMETABLE_SIZE + local
    }

    /// Read a CHR-ROM nametable byte for `addr` in `$2000-$2FFF` using the
    /// 1 KiB `$C000`/`$D000` bank registers (D7 forced to 1).
    fn chr_nt_byte(&self, addr: u16) -> u8 {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        // Lower physical nametable -> $C000 bank; upper -> $D000 bank.
        let reg = self.nt_banks[self.nt_physical(table)] as usize;
        // D6-D0 used; D7 forced to 1.
        let bank = (reg & 0x7F) | 0x80;
        let total_1k = (self.chr.len() / CHR_BANK_1K).max(1);
        let off = (bank % total_1k) * CHR_BANK_1K + local;
        self.chr[off % self.chr.len()]
    }
}

impl Mapper for Sunsoft4 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled {
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
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled {
                    let off = (addr - 0x6000) as usize % self.prg_ram.len();
                    self.prg_ram[off] = value;
                }
            }
            // Each register occupies a $1000-aligned window.
            0x8000..=0x8FFF => self.chr_banks[0] = value,
            0x9000..=0x9FFF => self.chr_banks[1] = value,
            0xA000..=0xAFFF => self.chr_banks[2] = value,
            0xB000..=0xBFFF => self.chr_banks[3] = value,
            0xC000..=0xCFFF => self.nt_banks[0] = value,
            0xD000..=0xDFFF => self.nt_banks[1] = value,
            0xE000..=0xEFFF => {
                self.mirroring = match value & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenA,
                    _ => Mirroring::SingleScreenB,
                };
                self.nt_rom_mode = (value & 0x10) != 0;
            }
            0xF000..=0xFFFF => {
                self.prg_bank = value & 0x0F;
                self.prg_ram_enabled = (value & 0x10) != 0;
            }
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
            0x2000..=0x3EFF => {
                // CHR-ROM nametable mode is handled by `nametable_fetch`
                // (the PPU consults it first). This fallback path serves
                // CIRAM for the non-ROM-nametable case.
                if self.nt_rom_mode {
                    self.chr_nt_byte(addr | 0x2000)
                } else {
                    self.vram[self.nametable_offset(addr) % self.vram.len()]
                }
            }
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
            // In CHR-ROM nametable mode writes are dropped (ROM-backed).
            0x2000..=0x3EFF if !self.nt_rom_mode => {
                let off = self.nametable_offset(addr) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        if self.nt_rom_mode && (0x2000..=0x2FFF).contains(&(addr & 0x3FFF)) {
            Some(self.chr_nt_byte(addr & 0x2FFF))
        } else {
            None
        }
    }

    fn nametable_write(&mut self, _addr: u16, _value: u8) -> bool {
        // In CHR-ROM nametable mode the PPU's CIRAM write is suppressed (the
        // nametables are ROM-backed). In CIRAM mode return false so the PPU
        // performs its normal CIRAM write via `nametable_address`.
        self.nt_rom_mode
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        let off = self.nametable_offset(addr);
        u16::try_from(off & 0x07FF).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 68,
            name: "Sunsoft-4 (68)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        info.prg_banks
            .push(("ram_en".into(), format!("{}", self.prg_ram_enabled)));
        for (i, b) in self.chr_banks.iter().enumerate() {
            info.chr_banks.push((format!("C{i}"), format!("{b:#04x}")));
        }
        info.extra
            .push(("ntROM".into(), format!("{}", self.nt_rom_mode)));
        info.extra
            .push(("NT0".into(), format!("{:#04x}", self.nt_banks[0])));
        info.extra
            .push(("NT1".into(), format!("{:#04x}", self.nt_banks[1])));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            16 + self.prg_ram.len()
                + self.vram.len()
                + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg_ram_enabled));
        out.extend_from_slice(&self.chr_banks);
        out.extend_from_slice(&self.nt_banks);
        out.push(self.mirroring as u8);
        out.push(u8::from(self.nt_rom_mode));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // 1 + 1 + 1 + 4 + 2 + 1 + 1
        let scalar_len = 1 + 1 + 1 + 4 + 2 + 1 + 1;
        let expected = scalar_len + self.prg_ram.len() + self.vram.len() + chr_part;
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
        self.prg_ram_enabled = data[c] != 0;
        c += 1;
        self.chr_banks.copy_from_slice(&data[c..c + 4]);
        c += 4;
        self.nt_banks.copy_from_slice(&data[c..c + 2]);
        c += 2;
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
        self.nt_rom_mode = data[c] != 0;
        c += 1;
        self.prg_ram
            .copy_from_slice(&data[c..c + self.prg_ram.len()]);
        c += self.prg_ram.len();
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

    /// 256 KiB CHR = 256 1 KiB banks, so D7-forced nametable banks ($80+) are
    /// reachable. Each 1 KiB bank's first byte = bank index (wrapped to u8).
    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
        for b in 0..banks_1k {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh() -> Sunsoft4 {
        Sunsoft4::new(synth_prg(8), synth_chr(256), Mirroring::Vertical).unwrap()
    }

    #[test]
    fn prg_bank_select_and_fixed_last() {
        let mut m = fresh();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7);
        m.cpu_write(0xF000, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn chr_four_2k_pattern_banks() {
        let mut m = fresh();
        // 2 KiB bank = 2 of the 1 KiB synth banks; first byte = bank*2.
        m.cpu_write(0x8000, 3); // pattern bank 0 -> 2 KiB bank 3 -> 1k bank 6
        assert_eq!(m.ppu_read(0x0000), 6);
        m.cpu_write(0xA000, 9); // pattern bank 2 @ $1000 -> 1k bank 18
        assert_eq!(m.ppu_read(0x1000), 18);
    }

    #[test]
    fn prg_ram_gated_by_f000_bit4() {
        let mut m = fresh();
        // Disabled by default -> writes ignored, reads 0.
        m.cpu_write(0x6000, 0xAB);
        assert_eq!(m.cpu_read(0x6000), 0);
        // Enable.
        m.cpu_write(0xF000, 0x10);
        m.cpu_write(0x6000, 0xCD);
        assert_eq!(m.cpu_read(0x6000), 0xCD);
    }

    #[test]
    fn nametable_chr_rom_mode_serves_chr_bytes() {
        let mut m = fresh();
        // Vertical mirroring: $2000 -> table 0 -> lower -> NT0 bank ($C000).
        m.cpu_write(0xC000, 0x05); // NT0 bank -> (0x05 & 0x7F)|0x80 = 0x85 = 133
        m.cpu_write(0xD000, 0x06); // NT1 bank -> 0x86 = 134
        // Enable ROM nametable mode (bit 4), keep vertical (bits 0-1 = 0).
        m.cpu_write(0xE000, 0x10);
        // $2000 (table 0, vertical -> physical 0 -> NT0 = 133).
        assert_eq!(m.nametable_fetch(0x2000), Some(133));
        // $2400 (table 1, vertical -> physical 1 -> NT1 = 134).
        assert_eq!(m.nametable_fetch(0x2400), Some(134));
    }

    #[test]
    fn nametable_ciram_mode_returns_none_from_fetch() {
        let mut m = fresh();
        // ROM nametable mode OFF -> nametable_fetch returns None (PPU uses
        // CIRAM via nametable_address).
        m.cpu_write(0xE000, 0x00);
        assert_eq!(m.nametable_fetch(0x2000), None);
        assert!(!m.nametable_write(0x2000, 0));
    }

    #[test]
    fn rom_nametable_mode_suppresses_ciram_write() {
        let mut m = fresh();
        m.cpu_write(0xE000, 0x10); // ROM nametable mode
        assert!(m.nametable_write(0x2000, 0x42));
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh();
        m.cpu_write(0xF000, 0x10 | 3);
        m.cpu_write(0x9000, 7);
        m.cpu_write(0xC000, 0x12);
        m.cpu_write(0xE000, 0x11);
        m.cpu_write(0x6000, 0x99);
        let blob = m.save_state();
        let mut m2 = fresh();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.cpu_read(0x6000), m2.cpu_read(0x6000));
        assert_eq!(m.nametable_fetch(0x2000), m2.nametable_fetch(0x2000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}
