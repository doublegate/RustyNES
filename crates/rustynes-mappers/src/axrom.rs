//! AxROM Mapper (Mapper 7).
//!
//! A simple mapper with 32KB PRG-ROM banking and single-screen mirroring control.
//! Used by games like Battletoads, Wizards & Warriors, and Marble Madness.
//!
//! Memory layout:
//! - PRG-ROM: 32KB switchable bank at $8000-$FFFF
//! - CHR-RAM: 8KB (no CHR-ROM banking)
//! - No PRG-RAM
//!
//! Bank selection: Write to $8000-$FFFF
//! - Bits 0-2: Select 32KB PRG bank
//! - Bit 4: Select single-screen mirroring (0 = lower, 1 = upper)

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// AxROM mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Axrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-RAM data (8KB).
    chr_ram: Vec<u8>,
    /// Number of PRG-ROM banks (32KB each).
    prg_banks: usize,
    /// Currently selected PRG bank.
    prg_bank: u8,
    /// Current mirroring mode.
    mirroring: Mirroring,
}

impl Axrom {
    /// Create a new AxROM mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 32768;
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
            mirroring: Mirroring::SingleScreenLower,
        }
    }
}

impl Mapper for Axrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM on AxROM
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
            // Bits 0-2: PRG bank select
            self.prg_bank = val & 0x07;
            // Bit 4: Mirroring select
            self.mirroring = if val & 0x10 != 0 {
                Mirroring::SingleScreenUpper
            } else {
                Mirroring::SingleScreenLower
            };
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
        7
    }

    fn mapper_name(&self) -> &'static str {
        "AxROM"
    }

    fn reset(&mut self) {
        self.prg_bank = 0;
        self.mirroring = Mirroring::SingleScreenLower;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_banks: u8) -> Rom {
        let prg_size = prg_banks as usize * 32768;

        // Fill each bank with its bank number for easy identification
        let mut prg_rom = vec![0u8; prg_size];
        for bank in 0..prg_banks as usize {
            for i in 0..32768 {
                prg_rom[bank * 32768 + i] = bank as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 7,
                prg_rom_size: prg_banks as u16 * 2, // 16KB units
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
    fn test_axrom_initial_state() {
        let rom = create_test_rom(4);
        let mapper = Axrom::new(&rom);

        // Should start at bank 0
        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_axrom_bank_switching() {
        let rom = create_test_rom(4);
        let mut mapper = Axrom::new(&rom);

        // Switch to bank 2
        mapper.write_prg(0x8000, 2);
        assert_eq!(mapper.read_prg(0x8000), 2);
        assert_eq!(mapper.read_prg(0xFFFF), 2);

        // Switch to bank 3
        mapper.write_prg(0xC000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);
    }

    #[test]
    fn test_axrom_mirroring_control() {
        let rom = create_test_rom(4);
        let mut mapper = Axrom::new(&rom);

        // Set upper screen
        mapper.write_prg(0x8000, 0x10);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);

        // Set lower screen
        mapper.write_prg(0x8000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_axrom_chr_ram() {
        let rom = create_test_rom(2);
        let mut mapper = Axrom::new(&rom);

        // CHR-RAM should be readable and writable
        assert_eq!(mapper.read_chr(0x0000), 0);
        mapper.write_chr(0x0000, 0xAB);
        assert_eq!(mapper.read_chr(0x0000), 0xAB);
    }

    #[test]
    fn test_axrom_reset() {
        let rom = create_test_rom(4);
        let mut mapper = Axrom::new(&rom);

        mapper.write_prg(0x8000, 0x13); // Bank 3, upper screen
        mapper.reset();

        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_axrom_info() {
        let rom = create_test_rom(2);
        let mapper = Axrom::new(&rom);

        assert_eq!(mapper.mapper_number(), 7);
        assert_eq!(mapper.mapper_name(), "AxROM");
    }
}
