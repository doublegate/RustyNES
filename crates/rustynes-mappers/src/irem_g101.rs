//! Irem G-101 (iNES mapper 32) implementation.
//!
//! An Irem ASIC board (Image Fight, Major League, Kaiketsu Yancha Maru 2,
//! Magical Pop's, etc.). Two switchable 8 KiB PRG banks with a software-
//! selectable swap mode, eight switchable 1 KiB CHR banks, and a software
//! H/V mirroring control. There is **no IRQ**.
//!
//! Register map (nesdev `INES_Mapper_032.xhtml`):
//!
//! ```text
//!   $8000-$8FFF [..PP PPPP]  PRG reg 0 (8 KiB) — to $8000 (mode 0) or $C000 (mode 1)
//!   $9000-$9FFF [.... ..MP]  M = mirroring (0 = Vertical, 1 = Horizontal)
//!                            P = PRG swap mode (0 = $8000 swappable, $C000 fixed;
//!                                               1 = $C000 swappable, $8000 fixed)
//!   $A000-$AFFF [..PP PPPP]  PRG reg 1 (8 KiB) — always to $A000
//!   $B000-$B007 [CCCC CCCC]  CHR reg 0..7 (1 KiB each, $0000..$1C00)
//! ```
//!
//! Slot layout:
//!
//! - Mode 0: `$8000` = PRG reg 0, `$A000` = PRG reg 1, `$C000` = fixed {-2},
//!   `$E000` = fixed {-1}.
//! - Mode 1: `$8000` = fixed {-2}, `$A000` = PRG reg 1, `$C000` = PRG reg 0,
//!   `$E000` = fixed {-1}.
//!
//! **Submapper 1** (Major League) is the special case where the board hard-wires
//! one-screen (single-screen A) mirroring and ignores the `$9000` mirroring bit.
//! When the header carries submapper 1 (or, lacking a submapper, we detect the
//! known Major League title via a forced flag from the dispatch) the mapper
//! locks single-screen A.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Irem G-101 mapper (iNES mapper 32).
pub struct IremG101 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    /// PRG reg 0 ($8000 writes) and PRG reg 1 ($A000 writes).
    prg_bank: [u8; 2],
    /// Eight 1 KiB CHR banks ($B000-$B007).
    chr_1k: [u8; 8],
    /// PRG swap mode ($9000 bit 1): false = $8000 swappable, true = $C000.
    prg_swap_mode: bool,
    mirroring: Mirroring,
    /// Submapper-1 boards (Major League) force single-screen A and ignore the
    /// $9000 mirroring bit.
    force_one_screen: bool,
}

impl IremG101 {
    /// Construct a new Irem G-101 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB. CHR-RAM is selected when
    /// `chr_rom` is empty; otherwise CHR-ROM length must be a multiple of 1 KiB.
    /// `force_one_screen` locks single-screen A mirroring (submapper 1 / Major
    /// League).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        force_one_screen: bool,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_8K != 0 {
            return Err(MapperError::Invalid(format!(
                "Irem-G101 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_1K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Irem-G101 expects a 1 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        let mirroring = if force_one_screen {
            Mirroring::SingleScreenA
        } else {
            mirroring
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: [0, 0],
            chr_1k: [0, 0, 0, 0, 0, 0, 0, 0],
            prg_swap_mode: false,
            mirroring,
            force_one_screen,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let bank_count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let slot = (addr >> 13) & 0x03; // 0=$8000,1=$A000,2=$C000,3=$E000
        let bank = match slot {
            0 => {
                if self.prg_swap_mode {
                    bank_count - 2 // mode 1: $8000 fixed {-2}
                } else {
                    self.prg_bank[0] as usize // mode 0: $8000 swappable
                }
            }
            1 => self.prg_bank[1] as usize, // $A000 always swappable
            2 => {
                if self.prg_swap_mode {
                    self.prg_bank[0] as usize // mode 1: $C000 swappable
                } else {
                    bank_count - 2 // mode 0: $C000 fixed {-2}
                }
            }
            _ => bank_count - 1, // $E000 always fixed {-1}
        } % bank_count;
        let off = (addr as usize) & (PRG_BANK_8K - 1);
        self.prg_rom[bank * PRG_BANK_8K + off]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let len = self.chr.len().max(1);
        let idx = ((addr >> 10) & 0x07) as usize; // 0..=7 over $0000-$1FFF
        let base = (self.chr_1k[idx] as usize) * CHR_BANK_1K;
        (base + (addr as usize & (CHR_BANK_1K - 1))) % len
    }
}

impl Mapper for IremG101 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return;
        }
        match addr & 0xF000 {
            0x8000 => self.prg_bank[0] = value & 0x3F,
            0x9000 => {
                if !self.force_one_screen {
                    self.mirroring = if (value & 0x01) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
                self.prg_swap_mode = (value & 0x02) != 0;
            }
            0xA000 => self.prg_bank[1] = value & 0x3F,
            0xB000 => {
                let idx = (addr & 0x07) as usize;
                self.chr_1k[idx] = value;
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
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
                    self.chr[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 32,
            name: "Irem G-101 (32)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("swap".into(), format!("{}", u8::from(self.prg_swap_mode))));
        for (i, b) in self.prg_bank.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_1k.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            13 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_bank);
        out.extend_from_slice(&self.chr_1k);
        out.push(u8::from(self.prg_swap_mode));
        out.push(match self.mirroring {
            Mirroring::Horizontal => 1,
            Mirroring::SingleScreenA => 2,
            _ => 0,
        });
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 13 + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank.copy_from_slice(&data[1..3]);
        self.chr_1k.copy_from_slice(&data[3..11]);
        self.prg_swap_mode = data[11] != 0;
        // Honor force_one_screen on restore.
        self.mirroring = if self.force_one_screen {
            Mirroring::SingleScreenA
        } else {
            match data[12] {
                1 => Mirroring::Horizontal,
                _ => Mirroring::Vertical,
            }
        };
        let mut cursor = 13;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
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

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn prg_mode_0_default_and_fixed_tail() {
        let mut m =
            IremG101::new(synth_prg(8), synth_chr_1k(8), Mirroring::Vertical, false).unwrap();
        // Mode 0 default: $8000 = PRG0=0, $A000 = PRG1=0, $C000 = {-2}=6, $E000 = {-1}=7.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 0);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
        m.cpu_write(0x8000, 0x03);
        m.cpu_write(0xA000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn prg_swap_mode_1_fixes_8000_swaps_c000() {
        let mut m =
            IremG101::new(synth_prg(8), synth_chr_1k(8), Mirroring::Vertical, false).unwrap();
        m.cpu_write(0x8000, 0x03); // PRG reg 0 = 3
        m.cpu_write(0x9000, 0x02); // swap mode 1
        // Mode 1: $8000 = {-2}=6, $C000 = PRG reg 0 = 3, $E000 = {-1}=7.
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn mirroring_bit_and_chr_banks() {
        let mut m =
            IremG101::new(synth_prg(4), synth_chr_1k(16), Mirroring::Vertical, false).unwrap();
        m.cpu_write(0x9000, 0x01); // mirroring bit -> Horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x9000, 0x00); // -> Vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // CHR reg 0 ($B000) selects 1 KiB bank at $0000.
        m.cpu_write(0xB000, 0x09);
        assert_eq!(m.ppu_read(0x0000), 9);
        // CHR reg 7 ($B007) selects 1 KiB bank at $1C00.
        m.cpu_write(0xB007, 0x0B);
        assert_eq!(m.ppu_read(0x1C00), 11);
    }

    #[test]
    fn force_one_screen_ignores_mirroring_bit() {
        let mut m =
            IremG101::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical, true).unwrap();
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0x9000, 0x01); // attempt to set Horizontal — ignored
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // The PRG swap bit still takes effect even on a one-screen board.
        assert!(!m.prg_swap_mode);
        m.cpu_write(0x9000, 0x02);
        assert!(m.prg_swap_mode);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m =
            IremG101::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical, false).unwrap();
        m.cpu_write(0x8000, 0x05);
        m.cpu_write(0x9000, 0x03); // mode 1 + Horizontal
        m.cpu_write(0xA000, 0x07);
        m.cpu_write(0xB003, 0x0A);
        let blob = m.save_state();
        let mut m2 =
            IremG101::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical, false).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.cpu_read(0xC000), m2.cpu_read(0xC000));
        assert_eq!(m.ppu_read(0x0C00), m2.ppu_read(0x0C00));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}
