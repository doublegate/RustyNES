//! UxROM Mapper (Mapper 2).
//!
//! A simple mapper with switchable PRG-ROM banking. Used by games like
//! Mega Man, Castlevania, and Contra.
//!
//! Memory layout:
//! - PRG-ROM: Two 16KB banks
//!   - $8000-$BFFF: Switchable bank
//!   - $C000-$FFFF: Fixed to last bank
//! - CHR-RAM: 8KB (no CHR-ROM banking)
//! - No PRG-RAM
//!
//! Bank selection: Write to $8000-$FFFF selects PRG bank

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// UxROM mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Uxrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-RAM data (8KB).
    chr_ram: Vec<u8>,
    /// Number of PRG-ROM banks (16KB each).
    prg_banks: usize,
    /// Currently selected PRG bank.
    prg_bank: u8,
    /// Nametable mirroring mode.
    mirroring: Mirroring,
}

impl Uxrom {
    /// Create a new UxROM mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 16384;

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_ram: vec![0u8; 8192],
            prg_banks,
            prg_bank: 0,
            mirroring: rom.header.mirroring,
        }
    }
}

impl Mapper for Uxrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM on UxROM
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bank select - different UxROM variants use different bit masks
            // Standard UxROM uses bits 0-3
            self.prg_bank = val & 0x0F;
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
        2
    }

    fn mapper_name(&self) -> &'static str {
        "UxROM"
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
        let prg_size = prg_banks as usize * 16384;

        // Fill each bank with its bank number for easy identification
        let mut prg_rom = vec![0u8; prg_size];
        for bank in 0..prg_banks as usize {
            for i in 0..16384 {
                prg_rom[bank * 16384 + i] = bank as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 2,
                prg_rom_size: prg_banks as u16,
                chr_rom_size: 0,
                prg_ram_size: 0,
                chr_ram_size: 8192,
                mirroring: Mirroring::Vertical,
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
    fn test_uxrom_initial_state() {
        let rom = create_test_rom(8);
        let mapper = Uxrom::new(&rom);

        // $8000-$BFFF should be bank 0
        assert_eq!(mapper.read_prg(0x8000), 0);

        // $C000-$FFFF should be last bank (7)
        assert_eq!(mapper.read_prg(0xC000), 7);
    }

    #[test]
    fn test_uxrom_bank_switching() {
        let rom = create_test_rom(8);
        let mut mapper = Uxrom::new(&rom);

        // Switch to bank 3
        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);

        // Last bank should still be 7
        assert_eq!(mapper.read_prg(0xC000), 7);

        // Switch to bank 5
        mapper.write_prg(0xC000, 5); // Can write anywhere in $8000-$FFFF
        assert_eq!(mapper.read_prg(0x8000), 5);
    }

    #[test]
    fn test_uxrom_chr_ram() {
        let rom = create_test_rom(4);
        let mut mapper = Uxrom::new(&rom);

        // CHR-RAM should be readable and writable
        assert_eq!(mapper.read_chr(0x0000), 0);
        mapper.write_chr(0x0000, 0xAB);
        assert_eq!(mapper.read_chr(0x0000), 0xAB);

        // Test mirroring
        mapper.write_chr(0x1234, 0xCD);
        assert_eq!(mapper.read_chr(0x1234), 0xCD);
    }

    #[test]
    fn test_uxrom_mirroring() {
        let rom = create_test_rom(4);
        let mapper = Uxrom::new(&rom);

        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
        assert_eq!(mapper.mapper_number(), 2);
        assert_eq!(mapper.mapper_name(), "UxROM");
    }

    #[test]
    fn test_uxrom_reset() {
        let rom = create_test_rom(8);
        let mut mapper = Uxrom::new(&rom);

        mapper.write_prg(0x8000, 5);
        assert_eq!(mapper.read_prg(0x8000), 5);

        mapper.reset();
        assert_eq!(mapper.read_prg(0x8000), 0);
    }

    #[test]
    fn test_uxrom_no_prg_ram() {
        let rom = create_test_rom(4);
        let mapper = Uxrom::new(&rom);

        // PRG-RAM area should return 0
        assert_eq!(mapper.read_prg(0x6000), 0);
        assert_eq!(mapper.read_prg(0x7FFF), 0);
    }
}
