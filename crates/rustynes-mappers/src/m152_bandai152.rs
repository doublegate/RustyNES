//! Bandai 74161/161 with software 1-screen mirroring (iNES mapper 152).
//!
//! Electrically identical to the Bandai discrete board (mapper 70) — a single
//! `$8000-$FFFF` write register `[MPPP CCCC]` selecting a 16 KiB switchable PRG
//! bank at `$8000-$BFFF` (bits 4-6) and an 8 KiB CHR bank (bits 0-3), with the
//! last 16 KiB PRG bank fixed at `$C000-$FFFF` — but bit 7 of the same write
//! drives a software 1-screen mirroring select (0 = one-screen A / lower bank,
//! 1 = one-screen B / upper bank) instead of the fixed iNES-header mirroring.
//!
//! Register (nesdev `INES_Mapper_152.xhtml`), `$8000-$FFFF`:
//!
//! ```text
//!   [MPPP CCCC]  M = 1-screen select (0 = screen A, 1 = screen B)
//!               P = PRG reg (16 KiB @ $8000, bits 4-6)
//!               C = CHR reg (8 KiB @ $0000, bits 0-3)
//! ```
//!
//! Used by Arkanoid II, Pocket Zaurus, Saint Seiya. CHR is ROM (or RAM when
//! the header declares no CHR-ROM). No IRQ. The real board has bus conflicts;
//! the project models discrete boards without bus-conflict emulation (matching
//! UxROM / GxROM / mapper 70), which is safe for the licensed library because
//! the games write the correct value.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

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

/// Bandai 74161/161 1-screen mapper (iNES mapper 152).
pub struct Bandai152 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Bandai152 {
    /// Construct a new Bandai-152 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB. CHR-RAM is selected
    /// when `chr_rom` is empty; otherwise CHR-ROM length must be a multiple
    /// of 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the
    /// constraints.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Bandai-152 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
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
                "Bandai-152 expects an 8 KiB multiple of CHR; got {} bytes",
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
            // Power-on default: one-screen A (bit 7 clear).
            mirroring: Mirroring::SingleScreenA,
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

impl Mapper for Bandai152 {
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
            // [MPPP CCCC]: bit 7 = 1-screen select, bits 4-6 = 16K PRG bank,
            // bits 0-3 = 8K CHR bank.
            self.mirroring = if (value & 0x80) != 0 {
                Mirroring::SingleScreenB
            } else {
                Mirroring::SingleScreenA
            };
            self.prg_bank = (value >> 4) & 0x07;
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
            mapper_id: 152,
            name: "Bandai 74161/161 (152)".into(),
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
            4 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        // Persist the mirroring select (bit 7).
        out.push(u8::from(self.mirroring == Mirroring::SingleScreenB));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 4 + self.vram.len() + need_chr;
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
        self.mirroring = if data[3] != 0 {
            Mirroring::SingleScreenB
        } else {
            Mirroring::SingleScreenA
        };
        let mut cursor = 4;
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
    fn defaults_first_prg_last_fixed_and_screen_a() {
        let mut m = Bandai152::new(synth_prg(8), synth_chr(4)).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7); // last 16K fixed
        assert_eq!(m.ppu_read(0x0000), 0);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn prg_chr_and_mirror_select() {
        let mut m = Bandai152::new(synth_prg(8), synth_chr(8)).unwrap();
        // [MPPP CCCC]: M=1 (screen B), PRG=5 (bits 4-6), CHR=3 (bits 0-3).
        // value = 1000_0000 | 0101_0000 | 0000_0011 = 0xD3.
        m.cpu_write(0x8000, 0xD3);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed unchanged
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        // Clear bit 7 -> screen A.
        m.cpu_write(0x8000, 0x53);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
    }

    #[test]
    fn prg_bank_masks_to_three_bits() {
        // Only bits 4-6 select PRG (bit 7 is the mirroring select, not PRG).
        let mut m = Bandai152::new(synth_prg(8), synth_chr(4)).unwrap();
        m.cpu_write(0x8000, 0xF0); // bits 4-6 = 0b111 = 7, bit 7 set
        assert_eq!(m.cpu_read(0x8000), 7);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn chr_ram_round_trip() {
        let mut m = Bandai152::new(synth_prg(2), Box::new([])).unwrap();
        m.ppu_write(0x0010, 0xCD);
        assert_eq!(m.ppu_read(0x0010), 0xCD);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Bandai152::new(synth_prg(4), synth_chr(2)).unwrap();
        m.cpu_write(0x8000, 0x91); // screen B, PRG=1, CHR=1
        let blob = m.save_state();
        let mut m2 = Bandai152::new(synth_prg(4), synth_chr(2)).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}
