//! CNROM with CHR copy protection (mapper 185).
//!
//! Electrically a stock CNROM, but the board's CHR-ROM is used as a
//! protection check: the game reads a known pattern back from CHR and, if the
//! value is wrong, the board disables CHR entirely so the screen fills with
//! garbage. Emulating it means modelling the *disable*, not just the banking
//! -- and the exact value that counts as "correct" varies by submapper, which
//! is why the decode matches on submapper rather than assuming one rule.
//!
//! Stock CNROM is in `m003_cnrom.rs`.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, with no commercial-oracle ROM in the tree. Banking math
//! is direct slice indexing and every bank select wraps with `% count`, so a
//! register write can never index out of bounds -- required for the `#![no_std]`
//! chip stack, which cannot afford a panic on a register access.
//!
//! See `tier.rs` (`MapperTier::BestEffort`), `docs/adr/0011-mapper-tiering.md`,
//! and `docs/mappers.md` §Mapper coverage matrix.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable helper (mirrors the one in the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Mapper 185 (`CNROM` with CHR-disable copy protection).
pub struct CnRom185 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_reg_raw: u8,
    chr_bank: u8,
    /// CHR-ROM enable latch. Powers on ENABLED (Mesen2 `CnromProtect`); the
    /// protection write may disable it. Initialising this to a derived-from-
    /// `chr_reg_raw=0` value left CHR reading $FF before the first register
    /// write, so the title screen never drew -> blank boot.
    chr_enabled: bool,
    sub_mapper: u8,
    mirroring: Mirroring,
}

impl CnRom185 {
    /// Construct a new mapper 185 board.
    ///
    /// `sub_mapper` selects the CHR-enable pattern (0 = default heuristic,
    /// 4..=7 = exact-match `value & 0x03`).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not 16/32 KiB or CHR-ROM is
    /// empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        sub_mapper: u8,
    ) -> Result<Self, MapperError> {
        if prg_rom.len() != PRG_BANK_16K && prg_rom.len() != PRG_BANK_32K {
            return Err(MapperError::Invalid(format!(
                "mapper 185 expects 16 or 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 185 expects non-empty CHR-ROM in 8 KiB units, got {} bytes",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_reg_raw: 0,
            chr_bank: 0,
            chr_enabled: true,
            sub_mapper: sub_mapper & 0x0F,
            mirroring,
        })
    }

    // The per-submapper CHR-enable rule (Mesen2 CnromProtect): submapper 0 is a
    // heuristic on the raw written latch; 4..=7 are exact low-2-bit matches.
    #[allow(clippy::verbose_bit_mask)]
    const fn chr_enable_for(&self, value: u8) -> bool {
        match self.sub_mapper {
            4 => (value & 0x03) == 0,
            5 => (value & 0x03) == 1,
            6 => (value & 0x03) == 2,
            7 => (value & 0x03) == 3,
            // Submapper 0 heuristic: enabled iff low nibble nonzero and != $13.
            _ => (value & 0x0F) != 0 && value != 0x13,
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let off = (addr - 0x8000) as usize;
        if self.prg_rom.len() == PRG_BANK_16K {
            self.prg_rom[off & (PRG_BANK_16K - 1)]
        } else {
            self.prg_rom[off]
        }
    }
}

impl Mapper for CnRom185 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict (mapper 185 always has AND-type bus conflicts).
            let effective = value & self.read_prg(addr);
            self.chr_reg_raw = effective;
            self.chr_enabled = self.chr_enable_for(effective);
            let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
            let mask = u8::try_from((count - 1) | 0x03).unwrap_or(u8::MAX);
            self.chr_bank = effective & mask;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_enabled {
                    let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                    let bank = (self.chr_bank as usize) % count;
                    self.chr_rom[bank * CHR_BANK_8K + addr as usize]
                } else {
                    // CHR disabled by protection: the open bus reads $FF (D0 is
                    // held high by a pull-up, which $FF already satisfies).
                    0xFF
                }
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.chr_reg_raw);
        out.push(self.chr_bank);
        out.push(self.sub_mapper);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_reg_raw = data[1];
        self.chr_bank = data[2];
        self.sub_mapper = data[3] & 0x0F;
        // chr_enabled is a deterministic function of the latched register +
        // submapper, so it is reconstructed rather than serialised (keeps the
        // save format stable). Power-on (chr_reg_raw == 0) restores to enabled
        // only if the heuristic agrees; the first write re-evaluates anyway.
        self.chr_enabled = self.chr_enable_for(self.chr_reg_raw);
        self.vram.copy_from_slice(&data[4..4 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_prg(bytes: usize, fill: u8) -> Box<[u8]> {
        vec![fill; bytes].into_boxed_slice()
    }

    #[test]
    fn m185_chr_disable_protection_default() {
        let mut m = CnRom185::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr_8k(4),
            Mirroring::Vertical,
            0,
        )
        .unwrap();
        // Power-on: CHR is ENABLED before any register write (Mesen2), so the
        // title screen draws. (The old derive-from-zero model read $FF here.)
        assert_eq!(m.ppu_read(0x0000), 0);
        // Submapper-0 heuristic: enabled iff (value & 0x0F) != 0 and value != $13.
        // Write 1 -> enabled, bank = 1 & mask.
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x0000), 1);
        // Write 0 -> CHR disabled -> reads $FF.
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x0000), 0xFF);
        // Write $13 -> the documented disabled sentinel -> $FF.
        m.cpu_write(0x8000, 0x13);
        assert_eq!(m.ppu_read(0x0000), 0xFF);
    }

    #[test]
    fn m185_submapper_exact_match() {
        let mut m = CnRom185::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr_8k(4),
            Mirroring::Vertical,
            4, // enabled iff (value & 3) == 0
        )
        .unwrap();
        m.cpu_write(0x8000, 0); // (0 & 3) == 0 -> enabled, bank 0
        assert_eq!(m.ppu_read(0x0000), 0);
        m.cpu_write(0x8000, 1); // (1 & 3) == 1 != 0 -> disabled
        assert_eq!(m.ppu_read(0x0000), 0xFF);
    }
}
