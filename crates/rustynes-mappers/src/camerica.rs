//! Camerica/Codemasters Mapper (Mapper 71).
//!
//! Used by Camerica and Codemasters games including Fire Hawk, Bee 52,
//! Big Nose the Caveman, and MiG 29 Soviet Fighter.
//!
//! Memory layout:
//! - PRG-ROM: Two 16KB banks
//!   - $8000-$BFFF: Switchable bank
//!   - $C000-$FFFF: Fixed to last bank
//! - CHR-RAM: 8KB at PPU $0000-$1FFF
//! - No PRG-RAM
//!
//! Bank selection:
//! - $8000-$9FFF: Mirroring control (bit 4) - Codemasters variant
//! - $C000-$FFFF: PRG bank select (bits 0-3)

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Camerica/Codemasters mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Camerica {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-RAM data (8KB).
    chr_ram: Vec<u8>,
    /// Number of PRG-ROM banks (16KB each).
    prg_banks: usize,
    /// Currently selected PRG bank.
    prg_bank: u8,
    /// Current mirroring mode.
    mirroring: Mirroring,
    /// Original mirroring from ROM header.
    original_mirroring: Mirroring,
}

impl Camerica {
    /// Create a new Camerica mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 16384;
        let chr_ram = if rom.chr_rom.is_empty() {
            vec![0u8; 8192]
        } else {
            rom.chr_rom.clone()
        };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_ram,
            prg_banks: prg_banks.max(1),
            prg_bank: 0,
            mirroring: rom.header.mirroring,
            original_mirroring: rom.header.mirroring,
        }
    }
}

impl Mapper for Camerica {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM
                0
            }
            0x8000..=0xBFFF => {
                // Switchable bank
                let bank = (self.prg_bank as usize) % self.prg_banks.max(1);
                let offset = (addr - 0x8000) as usize;
                self.prg_rom
                    .get(bank * 16384 + offset)
                    .copied()
                    .unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                // Fixed to last bank
                let bank = self.prg_banks.saturating_sub(1);
                let offset = (addr - 0xC000) as usize;
                self.prg_rom
                    .get(bank * 16384 + offset)
                    .copied()
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFF => {
                // Codemasters mirroring control (bit 4)
                // 0 = single-screen lower, 1 = single-screen upper
                // Some variants use horizontal/vertical instead
                if val & 0x10 != 0 {
                    self.mirroring = Mirroring::SingleScreenUpper;
                } else {
                    self.mirroring = Mirroring::SingleScreenLower;
                }
            }
            0xC000..=0xFFFF => {
                // PRG bank select
                self.prg_bank = val & 0x0F;
            }
            _ => {}
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let offset = (addr & 0x1FFF) as usize;
        self.chr_ram.get(offset).copied().unwrap_or(0)
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        let offset = (addr & 0x1FFF) as usize;
        if let Some(byte) = self.chr_ram.get_mut(offset) {
            *byte = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        71
    }

    fn mapper_name(&self) -> &'static str {
        "Camerica"
    }

    fn reset(&mut self) {
        self.prg_bank = 0;
        self.mirroring = self.original_mirroring;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_banks: u8) -> Rom {
        let prg_size = prg_banks as usize * 16384;

        // Fill each bank with its bank number
        let mut prg_rom = vec![0u8; prg_size];
        for bank in 0..prg_banks as usize {
            for i in 0..16384 {
                prg_rom[bank * 16384 + i] = bank as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 71,
                prg_rom_size: prg_banks as u16,
                chr_rom_size: 0,
                prg_ram_size: 0,
                chr_ram_size: 8192,
                mirroring: Mirroring::SingleScreenLower,
                has_battery: false,
                has_trainer: false,
                tv_system: 0,
            },
            prg_rom,
            chr_rom: Vec::new(),
            trainer: None,
        }
    }

    #[test]
    fn test_camerica_initial_state() {
        let rom = create_test_rom(8);
        let mapper = Camerica::new(&rom);

        // $8000-$BFFF should be bank 0
        assert_eq!(mapper.read_prg(0x8000), 0);
        // $C000-$FFFF should be last bank (7)
        assert_eq!(mapper.read_prg(0xC000), 7);
    }

    #[test]
    fn test_camerica_bank_switching() {
        let rom = create_test_rom(8);
        let mut mapper = Camerica::new(&rom);

        // Switch to bank 3
        mapper.write_prg(0xC000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);

        // Last bank should still be 7
        assert_eq!(mapper.read_prg(0xC000), 7);

        // Switch to bank 5
        mapper.write_prg(0xFFFF, 5);
        assert_eq!(mapper.read_prg(0x8000), 5);
    }

    #[test]
    fn test_camerica_mirroring_control() {
        let rom = create_test_rom(8);
        let mut mapper = Camerica::new(&rom);

        // Set upper screen
        mapper.write_prg(0x9000, 0x10);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);

        // Set lower screen
        mapper.write_prg(0x8000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_camerica_chr_ram() {
        let rom = create_test_rom(4);
        let mut mapper = Camerica::new(&rom);

        // CHR-RAM should be readable and writable
        assert_eq!(mapper.read_chr(0x0000), 0);
        mapper.write_chr(0x0000, 0xAB);
        assert_eq!(mapper.read_chr(0x0000), 0xAB);
    }

    #[test]
    fn test_camerica_reset() {
        let rom = create_test_rom(8);
        let mut mapper = Camerica::new(&rom);

        mapper.write_prg(0xC000, 5);
        mapper.write_prg(0x9000, 0x10);
        mapper.reset();

        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_camerica_info() {
        let rom = create_test_rom(4);
        let mapper = Camerica::new(&rom);

        assert_eq!(mapper.mapper_number(), 71);
        assert_eq!(mapper.mapper_name(), "Camerica");
    }

    #[test]
    fn test_camerica_no_prg_ram() {
        let rom = create_test_rom(4);
        let mapper = Camerica::new(&rom);

        // PRG-RAM area should return 0
        assert_eq!(mapper.read_prg(0x6000), 0);
        assert_eq!(mapper.read_prg(0x7FFF), 0);
    }
}
