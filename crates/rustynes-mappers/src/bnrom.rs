//! BNROM Mapper (Mapper 34).
//!
//! A simple mapper with 32KB PRG-ROM banking.
//! Used by Deadly Towers, Impossible Mission II, and others.
//!
//! Memory layout:
//! - PRG-ROM: 32KB switchable bank at $8000-$FFFF
//! - CHR-RAM: 8KB at PPU $0000-$1FFF
//! - No PRG-RAM
//!
//! Bank selection: Write to $8000-$FFFF
//! - Bits 0-1: Select 32KB PRG bank

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// BNROM mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Bnrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-RAM data (8KB).
    chr_ram: Vec<u8>,
    /// Number of PRG-ROM banks (32KB each).
    prg_banks: usize,
    /// Currently selected PRG bank.
    prg_bank: u8,
    /// Nametable mirroring mode.
    mirroring: Mirroring,
}

impl Bnrom {
    /// Create a new BNROM mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 32768;
        let chr_ram = if rom.chr_rom.is_empty() {
            vec![0u8; 8192]
        } else {
            // BNROM typically uses CHR-RAM, but handle CHR-ROM case
            rom.chr_rom.clone()
        };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_ram,
            prg_banks: prg_banks.max(1),
            prg_bank: 0,
            mirroring: rom.header.mirroring,
        }
    }
}

impl Mapper for Bnrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM on BNROM
                0
            }
            0x8000..=0xFFFF => {
                // 32KB switchable bank
                let bank = (self.prg_bank as usize) % self.prg_banks;
                let offset = (addr - 0x8000) as usize;
                self.prg_rom
                    .get(bank * 32768 + offset)
                    .copied()
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Standard BNROM uses bits 0-1 for bank select
            // Some variants may use more bits
            self.prg_bank = val & 0x03;
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let offset = (addr & 0x1FFF) as usize;
        self.chr_ram.get(offset).copied().unwrap_or(0)
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        // BNROM uses CHR-RAM, always writable
        let offset = (addr & 0x1FFF) as usize;
        if let Some(byte) = self.chr_ram.get_mut(offset) {
            *byte = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        34
    }

    fn mapper_name(&self) -> &'static str {
        "BNROM"
    }

    fn reset(&mut self) {
        self.prg_bank = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_banks: u8) -> Rom {
        let prg_size = prg_banks as usize * 32768;

        // Fill each bank with its bank number
        let mut prg_rom = vec![0u8; prg_size];
        for bank in 0..prg_banks as usize {
            for i in 0..32768 {
                prg_rom[bank * 32768 + i] = bank as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 34,
                prg_rom_size: prg_banks as u16 * 2,
                chr_rom_size: 0,
                prg_ram_size: 0,
                chr_ram_size: 8192,
                mirroring: Mirroring::Horizontal,
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
    fn test_bnrom_initial_state() {
        let rom = create_test_rom(4);
        let mapper = Bnrom::new(&rom);

        // Should start at bank 0
        assert_eq!(mapper.read_prg(0x8000), 0);
    }

    #[test]
    fn test_bnrom_bank_switching() {
        let rom = create_test_rom(4);
        let mut mapper = Bnrom::new(&rom);

        // Switch to bank 2
        mapper.write_prg(0x8000, 2);
        assert_eq!(mapper.read_prg(0x8000), 2);
        assert_eq!(mapper.read_prg(0xFFFF), 2);

        // Switch to bank 3
        mapper.write_prg(0xC000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);
    }

    #[test]
    fn test_bnrom_bank_wrapping() {
        let rom = create_test_rom(4);
        let mut mapper = Bnrom::new(&rom);

        // Bank 4 should wrap to bank 0
        mapper.write_prg(0x8000, 4);
        assert_eq!(mapper.read_prg(0x8000), 0);
    }

    #[test]
    fn test_bnrom_chr_ram() {
        let rom = create_test_rom(2);
        let mut mapper = Bnrom::new(&rom);

        // CHR-RAM should be readable and writable
        assert_eq!(mapper.read_chr(0x0000), 0);
        mapper.write_chr(0x0000, 0xAB);
        assert_eq!(mapper.read_chr(0x0000), 0xAB);

        mapper.write_chr(0x1234, 0xCD);
        assert_eq!(mapper.read_chr(0x1234), 0xCD);
    }

    #[test]
    fn test_bnrom_reset() {
        let rom = create_test_rom(4);
        let mut mapper = Bnrom::new(&rom);

        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);

        mapper.reset();
        assert_eq!(mapper.read_prg(0x8000), 0);
    }

    #[test]
    fn test_bnrom_info() {
        let rom = create_test_rom(2);
        let mapper = Bnrom::new(&rom);

        assert_eq!(mapper.mapper_number(), 34);
        assert_eq!(mapper.mapper_name(), "BNROM");
    }

    #[test]
    fn test_bnrom_no_prg_ram() {
        let rom = create_test_rom(2);
        let mapper = Bnrom::new(&rom);

        // PRG-RAM area should return 0
        assert_eq!(mapper.read_prg(0x6000), 0);
        assert_eq!(mapper.read_prg(0x7FFF), 0);
    }
}
