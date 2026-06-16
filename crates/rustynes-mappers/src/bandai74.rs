//! Bandai discrete (iNES mapper 70) implementation.
//!
//! A UxROM-like discrete board: a single `$8000-$FFFF` write register
//! `[PPPP CCCC]` selects a 16 KiB switchable PRG bank at `$8000-$BFFF`
//! (bits 4-7) and an 8 KiB CHR bank at `$0000-$1FFF` (bits 0-3). The last
//! 16 KiB PRG bank is fixed at `$C000-$FFFF`. Mirroring is fixed from the
//! iNES header (the 1-screen mapper-controlled variant is mapper 152). No
//! IRQ.
//!
//! The real board has bus conflicts; the project models discrete boards
//! without bus-conflict emulation (matching UxROM / GxROM), which is safe
//! for the licensed library because the games write the correct value.
//!
//! See `docs/mappers.md` §Mapper coverage matrix and
//! `nesdev_wiki/INES_Mapper_070.xhtml`.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Bandai discrete mapper (iNES mapper 70).
pub struct Bandai74 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Bandai74 {
    /// Construct a new Bandai-70 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB. CHR-RAM is selected
    /// when `chr_rom` is empty; otherwise CHR-ROM length must be a multiple
    /// of 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the
    /// constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Bandai-70 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Bandai-70 expects an 8 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let bank_count = (self.chr.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % bank_count;
        bank * CHR_BANK_8K + (addr as usize & (CHR_BANK_8K - 1))
    }
}

impl Mapper for Bandai74 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let bank_count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % bank_count;
                let off = (addr - 0x8000) as usize;
                self.prg_rom[bank * PRG_BANK_16K + off]
            }
            0xC000..=0xFFFF => {
                let last = bank_count - 1;
                let off = (addr - 0xC000) as usize;
                self.prg_rom[last * PRG_BANK_16K + off]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if let 0x8000..=0xFFFF = addr {
            // [PPPP CCCC]: bits 4-7 = 16K PRG bank, bits 0-3 = 8K CHR bank.
            self.prg_bank = (value >> 4) & 0x0F;
            self.chr_bank = value & 0x0F;
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
            mapper_id: 70,
            name: "Bandai discrete (70)".into(),
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
            3 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + self.vram.len() + need_chr;
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
        let mut cursor = 3;
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

    fn synth_prg(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * PRG_BANK_16K];
        for b in 0..banks {
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

    #[test]
    fn defaults_first_prg_last_fixed() {
        let mut m = Bandai74::new(synth_prg(8), synth_chr(4), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7); // last 16K fixed
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    #[test]
    fn prg_and_chr_bank_select() {
        let mut m = Bandai74::new(synth_prg(8), synth_chr(4), Mirroring::Vertical).unwrap();
        // [PPPP CCCC]: PRG=5 (bits 4-7), CHR=3 (bits 0-3) -> value 0x53.
        m.cpu_write(0x8000, 0x53);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed unchanged
        assert_eq!(m.ppu_read(0x0000), 3);
    }

    #[test]
    fn chr_ram_round_trip() {
        let mut m = Bandai74::new(synth_prg(2), Box::new([]), Mirroring::Horizontal).unwrap();
        m.ppu_write(0x0010, 0xCD);
        assert_eq!(m.ppu_read(0x0010), 0xCD);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Bandai74::new(synth_prg(4), synth_chr(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x21);
        let blob = m.save_state();
        let mut m2 = Bandai74::new(synth_prg(4), synth_chr(2), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
    }
}
