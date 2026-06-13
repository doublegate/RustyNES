//! Namco 175 / 340 (iNES mapper 210) implementation.
//!
//! Both are cost-reduced Namco 163 board variants **without** the expansion
//! audio, ASIC-internal RAM, ROM nametables, or (importantly) the IRQ counter
//! (`nesdev_wiki/INES_Mapper_210.xhtml`). They differ only in mirroring and
//! PRG-RAM:
//!
//! | Function   | N163 | N175 (submapper 1) | N340 (submapper 2)        |
//! |------------|------|--------------------|---------------------------|
//! | IRQ        | yes  | no                 | no                        |
//! | WRAM       | opt  | optional, enable-gated | none                  |
//! | Mirroring  | ext  | hardwired H/V      | selectable H/V/1scA/1scB  |
//!
//! NOTE: contrary to a common assumption, the **340 has no IRQ** — only the
//! full Namco 163 does. This implementation therefore never asserts an IRQ for
//! either submapper.
//!
//! # Banking
//!
//! - `$6000-$7FFF`: 8 KiB PRG-RAM (Namco 175 only; gated by `$C000` bit 0).
//! - `$8000-$9FFF` / `$A000-$BFFF` / `$C000-$DFFF`: three switchable 8 KiB PRG
//!   banks (`$E000`/`$E800`/`$F000` low bits).
//! - `$E000-$FFFF`: fixed to the last 8 KiB bank.
//! - PPU `$0000-$1FFF`: eight switchable 1 KiB CHR banks (`$8000-$BFFF`,
//!   one register per `$0800` window).
//!
//! On the Namco 340, the upper two bits of `$E000` select mirroring
//! (0=1scA, 1=Vertical, 2=1scB, 3=Horizontal). The Namco 175 ignores those
//! bits and uses the hardwired iNES-header mirroring.

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

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Board variant for mapper 210.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namco175Board {
    /// Namco 175 (submapper 1): hardwired H/V mirroring, optional enable-gated
    /// PRG-RAM at `$6000-$7FFF`.
    N175,
    /// Namco 340 (submapper 2): selectable H/V/1scA/1scB mirroring via the
    /// upper bits of `$E000`; no PRG-RAM.
    N340,
}

/// Namco 175 / 340 mapper (iNES mapper 210).
pub struct Namco175 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    prg: [u8; 3],      // switchable banks @ $8000/$A000/$C000
    chr_regs: [u8; 8], // eight 1 KiB CHR bank selects
    prg_ram_enabled: bool,
    mirroring: Mirroring,
    board: Namco175Board,
}

impl Namco175 {
    /// Construct a new Namco 175/340 mapper.
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
        board: Namco175Board,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_8K != 0 {
            return Err(MapperError::Invalid(format!(
                "Namco-175/340 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_data: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_1K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Namco-175/340 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr_data,
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg: [0; 3],
            chr_regs: [0; 8],
            prg_ram_enabled: false,
            mirroring,
            board,
        })
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg[0] as usize) % total,
            0xA000 => (self.prg[1] as usize) % total,
            0xC000 => (self.prg[2] as usize) % total,
            0xE000 => last,
            _ => 0,
        };
        bank * PRG_BANK_8K + (addr as usize & 0x1FFF)
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

impl Mapper for Namco175 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if matches!(self.board, Namco175Board::N175) && self.prg_ram_enabled {
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
                if matches!(self.board, Namco175Board::N175) && self.prg_ram_enabled {
                    let off = (addr - 0x6000) as usize % self.prg_ram.len();
                    self.prg_ram[off] = value;
                }
            }
            // CHR select: one register per $0800 window across $8000-$BFFF.
            0x8000..=0xBFFF => {
                let slot = ((addr - 0x8000) >> 11) as usize;
                if slot < 8 {
                    self.chr_regs[slot] = value;
                }
            }
            // External PRG RAM enable (Namco 175 only).
            0xC000..=0xC7FF => {
                if matches!(self.board, Namco175Board::N175) {
                    self.prg_ram_enabled = (value & 0x01) != 0;
                }
            }
            // PRG select 1 (+ Namco 340 mirroring in the upper two bits).
            0xE000..=0xE7FF => {
                self.prg[0] = value & 0x3F;
                if matches!(self.board, Namco175Board::N340) {
                    self.mirroring = match (value >> 6) & 0x03 {
                        0 => Mirroring::SingleScreenA,
                        1 => Mirroring::Vertical,
                        2 => Mirroring::SingleScreenB,
                        _ => Mirroring::Horizontal,
                    };
                }
            }
            // PRG select 2.
            0xE800..=0xEFFF => self.prg[1] = value & 0x3F,
            // PRG select 3.
            0xF000..=0xF7FF => self.prg[2] = value & 0x3F,
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

    fn nametable_address(&self, addr: u16) -> u16 {
        let off = self.nametable_offset(addr);
        u16::try_from(off & 0x07FF).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 210,
            name: match self.board {
                Namco175Board::N175 => "Namco 175 (210)".into(),
                Namco175Board::N340 => "Namco 340 (210)".into(),
            },
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        for (i, b) in self.prg.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_regs.iter().enumerate() {
            info.chr_banks.push((format!("C{i}"), format!("{b:#04x}")));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            16 + self.prg_ram.len()
                + self.vram.len()
                + if self.chr_is_ram {
                    self.chr_rom.len()
                } else {
                    0
                },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg);
        out.extend_from_slice(&self.chr_regs);
        out.push(u8::from(self.prg_ram_enabled));
        out.push(self.mirroring as u8);
        out.push(match self.board {
            Namco175Board::N175 => 0,
            Namco175Board::N340 => 1,
        });
        out.extend_from_slice(&self.prg_ram);
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
        // 1 + 3 + 8 + 1 + 1 + 1
        let scalar_len = 1 + 3 + 8 + 1 + 1 + 1;
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
        self.prg.copy_from_slice(&data[c..c + 3]);
        c += 3;
        self.chr_regs.copy_from_slice(&data[c..c + 8]);
        c += 8;
        self.prg_ram_enabled = data[c] != 0;
        c += 1;
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
        self.board = match data[c] {
            0 => Namco175Board::N175,
            1 => Namco175Board::N340,
            other => return Err(MapperError::Invalid(format!("board {other}"))),
        };
        c += 1;
        self.prg_ram
            .copy_from_slice(&data[c..c + self.prg_ram.len()]);
        c += self.prg_ram.len();
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

    fn fresh175() -> Namco175 {
        Namco175::new(
            synth_prg(16),
            synth_chr(64),
            Mirroring::Vertical,
            Namco175Board::N175,
        )
        .unwrap()
    }

    fn fresh340() -> Namco175 {
        Namco175::new(
            synth_prg(16),
            synth_chr(64),
            Mirroring::Vertical,
            Namco175Board::N340,
        )
        .unwrap()
    }

    #[test]
    fn prg_three_switchable_plus_fixed() {
        let mut m = fresh175();
        assert_eq!(m.cpu_read(0xE000), 15); // last bank fixed
        m.cpu_write(0xE000, 3); // PRG0 = 3
        m.cpu_write(0xE800, 5); // PRG1 = 5
        m.cpu_write(0xF000, 9); // PRG2 = 9
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 9);
        assert_eq!(m.cpu_read(0xE000), 15);
    }

    #[test]
    fn chr_eight_1k_banks() {
        let mut m = fresh175();
        m.cpu_write(0x8000, 10); // C0
        m.cpu_write(0xA000, 20); // C4 @ $1000
        assert_eq!(m.ppu_read(0x0000), 10);
        assert_eq!(m.ppu_read(0x1000), 20);
    }

    #[test]
    fn n175_no_irq() {
        let mut m = fresh175();
        for _ in 0..2000 {
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending());
    }

    #[test]
    fn n340_no_irq() {
        let mut m = fresh340();
        for _ in 0..2000 {
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending());
    }

    #[test]
    fn n175_prg_ram_enable_gated() {
        let mut m = fresh175();
        // Disabled -> writes ignored.
        m.cpu_write(0x6000, 0xAB);
        assert_eq!(m.cpu_read(0x6000), 0);
        m.cpu_write(0xC000, 0x01); // enable
        m.cpu_write(0x6000, 0xCD);
        assert_eq!(m.cpu_read(0x6000), 0xCD);
    }

    #[test]
    fn n175_mirroring_hardwired_ignores_e000_upper_bits() {
        let mut m = fresh175();
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // Upper bits of $E000 must NOT change mirroring on the 175.
        m.cpu_write(0xE000, 0xC0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn n340_mirroring_select() {
        let mut m = fresh340();
        m.cpu_write(0xE000, 0x00); // %00 -> 1scA
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0xE000, 0x40); // %01 -> Vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0xE000, 0x80); // %10 -> 1scB
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        m.cpu_write(0xE000, 0xC0); // %11 -> Horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh340();
        m.cpu_write(0xE000, 0x40 | 3);
        m.cpu_write(0x9000, 7);
        m.ppu_write(0x2000, 0x33);
        let blob = m.save_state();
        let mut m2 = fresh340();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0800), m2.ppu_read(0x0800));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}
