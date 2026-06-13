//! Irem/Jaleco mapper 78 (Holy Diver / Uchuusen - Cosmo Carrier).
//!
//! A UxROM + CNROM hybrid (discrete logic): a single bank-select register at
//! `$8000-$FFFF` switches a 16 KiB PRG bank, an 8 KiB CHR bank, and a single
//! mirroring bit. The two games wire that mirroring bit differently, so it is
//! **submapper-selected** (`nesdev_wiki/INES_Mapper_078.xhtml`):
//!
//! - Submapper 3 (Holy Diver): bit 3 toggles Horizontal / Vertical.
//! - Submapper 1 (Cosmo Carrier): bit 3 toggles single-screen A / B (the bit
//!   is wired directly to CIRAM A10 like `AxROM`).
//!
//! The two modes are mutually incompatible — running one game's ROM under the
//! other's wiring glitches. iNES 1.0 images often set the "alternative
//! nametables" header flag for Holy Diver; NES 2.0 uses the submapper byte.
//!
//! # Bank Select (`$8000-$FFFF`)
//!
//! ```text
//! 7  bit  0
//! CCCC MPPP
//! |||| |+++- 16 KiB PRG bank for $8000-$BFFF
//! |||| +---- Mirroring (Holy Diver: 0=H,1=V; Cosmo Carrier: 0=1scA,1=1scB)
//! ++++------ 8 KiB CHR bank for $0000-$1FFF
//! ```
//!
//! `$C000-$FFFF` is fixed to the last 16 KiB PRG bank. No PRG-RAM, no IRQ.
//! Subject to bus conflicts (the value written must match the ROM byte) — we
//! do not model bus conflicts here (none of the two games rely on a conflict
//! that differs from a plain write).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Mirroring wiring variant, selected by NES 2.0 submapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M78Variant {
    /// Submapper 3 — Holy Diver: bit 3 = Horizontal (0) / Vertical (1).
    HolyDiver,
    /// Submapper 1 — Cosmo Carrier: bit 3 = single-screen A (0) / B (1).
    CosmoCarrier,
}

/// Mapper 78 (Holy Diver / Cosmo Carrier).
pub struct M78 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
    variant: M78Variant,
}

impl M78 {
    /// Construct a new mapper-78 instance.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB; CHR-ROM (when present)
    /// must be a multiple of 8 KiB. CHR-RAM (8 KiB) is allocated when no
    /// CHR-ROM is supplied. `variant` selects the mirroring wiring.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        variant: M78Variant,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_16K != 0 {
            return Err(MapperError::Invalid(format!(
                "Mapper-78 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_8K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Mapper-78 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        // Default mirroring at power-on: bank-select bit 3 = 0.
        let mirroring = match variant {
            M78Variant::HolyDiver => Mirroring::Horizontal,
            M78Variant::CosmoCarrier => Mirroring::SingleScreenA,
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
            variant,
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
        let total_8k = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % total_8k;
        bank * CHR_BANK_8K + addr
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for M78 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
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
        if let 0x8000..=0xFFFF = addr {
            self.prg_bank = value & 0x07;
            self.chr_bank = (value >> 4) & 0x0F;
            let mbit = (value & 0x08) != 0;
            self.mirroring = match self.variant {
                M78Variant::HolyDiver => {
                    if mbit {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    }
                }
                M78Variant::CosmoCarrier => {
                    if mbit {
                        Mirroring::SingleScreenB
                    } else {
                        Mirroring::SingleScreenA
                    }
                }
            };
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

    fn nametable_address(&self, addr: u16) -> u16 {
        let off = self.nametable_offset(addr);
        u16::try_from(off & 0x07FF).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 78,
            name: match self.variant {
                M78Variant::HolyDiver => "Mapper 78 (Holy Diver)".into(),
                M78Variant::CosmoCarrier => "Mapper 78 (Cosmo Carrier)".into(),
            },
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        info.chr_banks
            .push(("CHR".into(), format!("{:#04x}", self.chr_bank)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            8 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(self.mirroring as u8);
        out.push(match self.variant {
            M78Variant::HolyDiver => 0,
            M78Variant::CosmoCarrier => 1,
        });
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // 1 + 1 + 1 + 1 + 1
        let scalar_len = 5;
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
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        self.mirroring = match data[3] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.variant = match data[4] {
            0 => M78Variant::HolyDiver,
            1 => M78Variant::CosmoCarrier,
            other => return Err(MapperError::Invalid(format!("variant {other}"))),
        };
        let mut c = 5usize;
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

    fn synth_chr(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * CHR_BANK_8K];
        for b in 0..banks_8k {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh_hd() -> M78 {
        M78::new(synth_prg(8), synth_chr(16), M78Variant::HolyDiver).unwrap()
    }

    fn fresh_cc() -> M78 {
        M78::new(synth_prg(8), synth_chr(16), M78Variant::CosmoCarrier).unwrap()
    }

    #[test]
    fn prg_low_bank_switch_high_fixed() {
        let mut m = fresh_hd();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7); // last fixed
        m.cpu_write(0x8000, 0x05); // PRG bits 0-2 = 5
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7);
    }

    #[test]
    fn chr_bank_switch_high_nibble() {
        let mut m = fresh_hd();
        m.cpu_write(0x8000, 0x30); // CHR bits 4-7 = 3
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn holy_diver_mirroring_h_v() {
        let mut m = fresh_hd();
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0x08); // bit 3 set -> Vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0x8000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn cosmo_carrier_mirroring_single_screen() {
        let mut m = fresh_cc();
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0x8000, 0x08); // bit 3 set -> 1scB
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0x8000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn no_irq() {
        let mut m = fresh_hd();
        for _ in 0..1000 {
            m.notify_cpu_cycle();
            m.notify_a12(true);
            m.notify_a12(false);
        }
        assert!(!m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh_cc();
        m.cpu_write(0x8000, 0x3D); // CHR=3, mirroring bit set, PRG=5
        m.ppu_write(0x2000, 0x66);
        let blob = m.save_state();
        let mut m2 = fresh_cc();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
        assert_eq!(m.ppu_read(0x2000), m2.ppu_read(0x2000));
    }
}
