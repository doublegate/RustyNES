//! GxROM Mapper (Mapper 66).
//!
//! A simple mapper with PRG-ROM and CHR-ROM banking.
//! Used by games like Super Mario Bros. + Duck Hunt, Gumshoe, and Dragon Power.
//!
//! Memory layout:
//! - PRG-ROM: 32KB switchable bank at $8000-$FFFF
//! - CHR-ROM: 8KB switchable bank at PPU $0000-$1FFF
//! - No PRG-RAM
//!
//! Bank selection: Write to $8000-$FFFF
//! - Bits 0-1: Select 8KB CHR bank
//! - Bits 4-5: Select 32KB PRG bank

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// GxROM mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Gxrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-ROM/RAM data.
    chr: Vec<u8>,
    /// Whether CHR is RAM (writable).
    chr_is_ram: bool,
    /// Number of PRG-ROM banks (32KB each).
    prg_banks: usize,
    /// Number of CHR banks (8KB each).
    chr_banks: usize,
    /// Currently selected PRG bank.
    prg_bank: u8,
    /// Currently selected CHR bank.
    chr_bank: u8,
    /// Nametable mirroring mode.
    mirroring: Mirroring,
}

impl Gxrom {
    /// Create a new GxROM mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 32768;
        let chr_is_ram = rom.chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 8192]
        } else {
            rom.chr_rom.clone()
        };
        let chr_banks = if chr_is_ram { 1 } else { chr.len() / 8192 };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr,
            chr_is_ram,
            prg_banks: prg_banks.max(1),
            chr_banks: chr_banks.max(1),
            prg_bank: 0,
            chr_bank: 0,
            mirroring: rom.header.mirroring,
        }
    }
}

impl Mapper for Gxrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM
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
            // Bits 0-1: CHR bank select
            self.chr_bank = val & 0x03;
            // Bits 4-5: PRG bank select
            self.prg_bank = (val >> 4) & 0x03;
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let bank = (self.chr_bank as usize) % self.chr_banks;
        let offset = (addr & 0x1FFF) as usize;
        self.chr.get(bank * 8192 + offset).copied().unwrap_or(0)
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram {
            let offset = (addr & 0x1FFF) as usize;
            if let Some(byte) = self.chr.get_mut(offset) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        66
    }

    fn mapper_name(&self) -> &'static str {
        "GxROM"
    }

    fn reset(&mut self) {
        self.prg_bank = 0;
        self.chr_bank = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_banks: u8, chr_banks: u8) -> Rom {
        let prg_size = prg_banks as usize * 32768;
        let chr_size = chr_banks as usize * 8192;

        // Fill each bank with its bank number
        let mut prg_rom = vec![0u8; prg_size];
        for bank in 0..prg_banks as usize {
            for i in 0..32768 {
                prg_rom[bank * 32768 + i] = bank as u8;
            }
        }

        let mut chr_rom = vec![0u8; chr_size];
        for bank in 0..chr_banks as usize {
            for i in 0..8192 {
                chr_rom[bank * 8192 + i] = (bank + 0x80) as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 66,
                prg_rom_size: prg_banks as u16 * 2,
                chr_rom_size: chr_banks as u16,
                prg_ram_size: 0,
                chr_ram_size: if chr_banks == 0 { 8192 } else { 0 },
                mirroring: Mirroring::Vertical,
                has_battery: false,
                has_trainer: false,
                tv_system: 0,
            },
            prg_rom,
            chr_rom,
            trainer: None,
        }
    }

    #[test]
    fn test_gxrom_initial_state() {
        let rom = create_test_rom(4, 4);
        let mapper = Gxrom::new(&rom);

        // Should start at bank 0
        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.read_chr(0x0000), 0x80);
    }

    #[test]
    fn test_gxrom_prg_banking() {
        let rom = create_test_rom(4, 4);
        let mut mapper = Gxrom::new(&rom);

        // Switch to PRG bank 2 (bits 4-5 = 0x20)
        mapper.write_prg(0x8000, 0x20);
        assert_eq!(mapper.read_prg(0x8000), 2);
        assert_eq!(mapper.read_prg(0xFFFF), 2);
    }

    #[test]
    fn test_gxrom_chr_banking() {
        let rom = create_test_rom(4, 4);
        let mut mapper = Gxrom::new(&rom);

        // Switch to CHR bank 3 (bits 0-1 = 0x03)
        mapper.write_prg(0x8000, 0x03);
        assert_eq!(mapper.read_chr(0x0000), 0x83);
    }

    #[test]
    fn test_gxrom_combined_banking() {
        let rom = create_test_rom(4, 4);
        let mut mapper = Gxrom::new(&rom);

        // PRG bank 1, CHR bank 2 (0x12)
        mapper.write_prg(0x8000, 0x12);
        assert_eq!(mapper.read_prg(0x8000), 1);
        assert_eq!(mapper.read_chr(0x0000), 0x82);
    }

    #[test]
    fn test_gxrom_chr_ram() {
        let rom = create_test_rom(2, 0);
        let mut mapper = Gxrom::new(&rom);

        assert_eq!(mapper.read_chr(0x0000), 0);
        mapper.write_chr(0x0000, 0xAB);
        assert_eq!(mapper.read_chr(0x0000), 0xAB);
    }

    #[test]
    fn test_gxrom_reset() {
        let rom = create_test_rom(4, 4);
        let mut mapper = Gxrom::new(&rom);

        mapper.write_prg(0x8000, 0x33);
        mapper.reset();

        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.read_chr(0x0000), 0x80);
    }

    #[test]
    fn test_gxrom_info() {
        let rom = create_test_rom(2, 2);
        let mapper = Gxrom::new(&rom);

        assert_eq!(mapper.mapper_number(), 66);
        assert_eq!(mapper.mapper_name(), "GxROM");
    }
}
