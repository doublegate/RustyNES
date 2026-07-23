//! Fong Shen Bang / Feng Shen Bang (mapper 246).
//!
//! Four bank-select registers in the `$6000-$67FF` window -- two PRG, two
//! CHR -- with battery-backed PRG-RAM sharing the same `$6000` region above
//! the register window. The split matters: a write below `$6800` is a
//! register, a write above it is save RAM.
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

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_2K: usize = 0x0800;
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

/// Mapper 246 (`Fong Shen Bang` / G0151-1).
pub struct FongShenBang246 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// 2 KiB battery-backed PRG-RAM at $6800-$6FFF.
    prg_ram: Box<[u8]>,
    prg_banks: [u8; 4],
    chr_banks: [u8; 4],
    mirroring: Mirroring,
}

impl FongShenBang246 {
    /// Construct a new mapper 246 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 246 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 246 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        // Power-on (per the nesdev wiki): the $6000-$6002 PRG regs are 0, but
        // $6003 (the $E000-$FFFF slot) initializes to 0xFF — so the reset vector
        // at $FFFC resolves into the last PRG bank, where the boot code lives.
        let prg_banks = [0, 0, 0, 0xFF];
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; 0x0800].into_boxed_slice(),
            prg_banks,
            chr_banks: [0, 0, 0, 0],
            mirroring,
        })
    }

    fn prg_byte(&self, slot: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let mut bank = self.prg_banks[slot] as usize;
        // $E000-$FFFF hardware quirk: reads from $FFE4-$FFE7, $FFEC-$FFEF,
        // $FFF4-$FFF7, and $FFFC-$FFFF force PRG A17 high (bank bit 4 of an 8 KiB
        // index). The interrupt/reset vectors live in that forced region.
        if slot == 3 {
            let low = addr & 0x001F;
            let in_window = (0xFFE4..=0xFFFF).contains(&addr)
                && matches!(low, 0x04..=0x07 | 0x0C..=0x0F | 0x14..=0x17 | 0x1C..=0x1F);
            if in_window {
                bank |= 0x10;
            }
        }
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for FongShenBang246 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // Only the dead sub-ranges below the PRG window are open bus: $4020-$67FF
    // (the write-only register file at $6000-$67FF + the $4020-$5FFF gap) and
    // the $7000-$7FFF mirror gap. The 2 KiB PRG-RAM at $6800-$6FFF and the PRG
    // ROM at $8000-$FFFF are mapped (matching the trait default of "$6000-$FFFF
    // is mapped" but carving out the register/gap holes).
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x67FF).contains(&addr) || (0x7000..=0x7FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6800..=0x6FFF => self.prg_ram[(addr - 0x6800) as usize],
            0x8000..=0x9FFF => self.prg_byte(0, addr),
            0xA000..=0xBFFF => self.prg_byte(1, addr),
            0xC000..=0xDFFF => self.prg_byte(2, addr),
            0xE000..=0xFFFF => self.prg_byte(3, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x6003 => self.prg_banks[(addr & 0x03) as usize] = value,
            0x6004..=0x6007 => self.chr_banks[(addr & 0x03) as usize] = value,
            0x6800..=0x6FFF => self.prg_ram[(addr - 0x6800) as usize] = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr >> 11) as usize & 0x03;
                let count = (self.chr_rom.len() / CHR_BANK_2K).max(1);
                let bank = (self.chr_banks[slot] as usize) % count;
                self.chr_rom[bank * CHR_BANK_2K + (addr as usize & 0x07FF)]
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
        let mut out = Vec::with_capacity(1 + 4 + 4 + self.prg_ram.len() + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_banks);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + 4 + 4 + self.prg_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_banks.copy_from_slice(&data[1..5]);
        self.chr_banks.copy_from_slice(&data[5..9]);
        let mut cursor = 9;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m246_register_banking_and_prg_ram() {
        let mut m =
            FongShenBang246::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        // $6000 -> PRG $8000 = bank 3.
        m.cpu_write(0x6000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $6004 -> CHR slot 0 = bank 5.
        m.cpu_write(0x6004, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // PRG-RAM round-trips at $6800.
        m.cpu_write(0x6800, 0xC4);
        assert_eq!(m.cpu_read(0x6800), 0xC4);
    }

    #[test]
    fn m246_save_state_round_trip() {
        let mut m =
            FongShenBang246::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6001, 4); // PRG $A000 = bank 4
        m.cpu_write(0x6007, 6); // CHR slot 3 = bank 6
        m.cpu_write(0x6900, 0x9D); // PRG-RAM at $6800-$6FFF
        let blob = m.save_state();
        let mut m2 =
            FongShenBang246::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xA000), 4);
        assert_eq!(m2.ppu_read(0x1800), 6);
        assert_eq!(m2.cpu_read(0x6900), 0x9D);
    }
}
