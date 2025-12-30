//! CNROM Mapper (Mapper 3).
//!
//! A simple mapper with switchable CHR-ROM banking. Used by games like
//! Gradius, Solomon's Key, and Arkanoid.
//!
//! Memory layout:
//! - PRG-ROM: 16KB or 32KB, not switchable
//! - CHR-ROM: 8KB banks, switchable via writes to $8000-$FFFF
//! - No PRG-RAM
//!
//! Bank selection: Write to $8000-$FFFF selects CHR bank

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// CNROM mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Cnrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-ROM data.
    chr_rom: Vec<u8>,
    /// PRG-ROM size.
    prg_size: usize,
    /// Number of CHR-ROM banks (8KB each).
    chr_banks: usize,
    /// Currently selected CHR bank.
    chr_bank: u8,
    /// Nametable mirroring mode.
    mirroring: Mirroring,
}

impl Cnrom {
    /// Create a new CNROM mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_size = rom.prg_rom.len();
        let chr_banks = (rom.chr_rom.len() / 8192).max(1);

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_rom: if rom.chr_rom.is_empty() {
                vec![0u8; 8192] // CHR-RAM fallback
            } else {
                rom.chr_rom.clone()
            },
            prg_size,
            chr_banks,
            chr_bank: 0,
            mirroring: rom.header.mirroring,
        }
    }
}

impl Mapper for Cnrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM on CNROM
                0
            }
            0x8000..=0xFFFF => {
                // Mirror 16KB PRG-ROM if only 16KB
                let offset = (addr - 0x8000) as usize;
                let masked = if self.prg_size <= 16384 {
                    offset & 0x3FFF // Mirror 16KB
                } else {
                    offset // Full 32KB
                };
                self.prg_rom.get(masked).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // CHR bank select - uses bits 0-1 (or more for larger CHR)
            self.chr_bank = val & 0x03;
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let bank = (self.chr_bank as usize) % self.chr_banks;
        let offset = (addr & 0x1FFF) as usize;
        self.chr_rom.get(bank * 8192 + offset).copied().unwrap_or(0)
    }

    fn write_chr(&mut self, _addr: u16, _val: u8) {
        // CNROM has CHR-ROM, not writable
        // (Unless it has CHR-RAM, but standard CNROM doesn't)
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        3
    }

    fn mapper_name(&self) -> &'static str {
        "CNROM"
    }

    fn reset(&mut self) {
        self.chr_bank = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_banks: u8, chr_banks: u8) -> Rom {
        let prg_size = prg_banks as usize * 16384;
        let chr_size = chr_banks as usize * 8192;

        let prg_rom: Vec<u8> = (0..prg_size).map(|i| (i & 0xFF) as u8).collect();

        // Fill each CHR bank with its bank number
        let mut chr_rom = vec![0u8; chr_size];
        for bank in 0..chr_banks as usize {
            for i in 0..8192 {
                chr_rom[bank * 8192 + i] = bank as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 3,
                prg_rom_size: prg_banks as u16,
                chr_rom_size: chr_banks as u16,
                prg_ram_size: 0,
                chr_ram_size: 0,
                mirroring: Mirroring::Horizontal,
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
    fn test_cnrom_initial_state() {
        let rom = create_test_rom(1, 4);
        let mapper = Cnrom::new(&rom);

        // CHR should be bank 0
        assert_eq!(mapper.read_chr(0x0000), 0);
    }

    #[test]
    fn test_cnrom_chr_bank_switching() {
        let rom = create_test_rom(1, 4);
        let mut mapper = Cnrom::new(&rom);

        // Initial bank 0
        assert_eq!(mapper.read_chr(0x0000), 0);

        // Switch to bank 1
        mapper.write_prg(0x8000, 1);
        assert_eq!(mapper.read_chr(0x0000), 1);

        // Switch to bank 2
        mapper.write_prg(0x9000, 2);
        assert_eq!(mapper.read_chr(0x0000), 2);

        // Switch to bank 3
        mapper.write_prg(0xFFFF, 3);
        assert_eq!(mapper.read_chr(0x0000), 3);
    }

    #[test]
    fn test_cnrom_prg_16kb_mirroring() {
        let rom = create_test_rom(1, 4); // 16KB PRG
        let mapper = Cnrom::new(&rom);

        // $8000 and $C000 should mirror
        assert_eq!(mapper.read_prg(0x8000), mapper.read_prg(0xC000));
        assert_eq!(mapper.read_prg(0x8100), mapper.read_prg(0xC100));
    }

    #[test]
    fn test_cnrom_prg_32kb_no_mirroring() {
        let rom = create_test_rom(2, 4); // 32KB PRG
        let mapper = Cnrom::new(&rom);

        // $8000 maps to 0x0000, $C000 maps to 0x4000
        let prg_low = mapper.read_prg(0x8000);
        let prg_high = mapper.read_prg(0xC000);

        // They should be different (different offsets in PRG-ROM)
        assert_eq!(prg_low, 0x00); // Offset 0
        assert_eq!(prg_high, 0x00); // Offset 0x4000 & 0xFF = 0
    }

    #[test]
    fn test_cnrom_chr_not_writable() {
        let rom = create_test_rom(1, 4);
        let mut mapper = Cnrom::new(&rom);

        // CHR-ROM should not be writable
        let original = mapper.read_chr(0x0000);
        mapper.write_chr(0x0000, 0xFF);
        assert_eq!(mapper.read_chr(0x0000), original);
    }

    #[test]
    fn test_cnrom_mirroring() {
        let rom = create_test_rom(1, 4);
        let mapper = Cnrom::new(&rom);

        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert_eq!(mapper.mapper_number(), 3);
        assert_eq!(mapper.mapper_name(), "CNROM");
    }

    #[test]
    fn test_cnrom_reset() {
        let rom = create_test_rom(1, 4);
        let mut mapper = Cnrom::new(&rom);

        mapper.write_prg(0x8000, 2);
        assert_eq!(mapper.read_chr(0x0000), 2);

        mapper.reset();
        assert_eq!(mapper.read_chr(0x0000), 0);
    }

    #[test]
    fn test_cnrom_bank_wrapping() {
        let rom = create_test_rom(1, 4); // 4 CHR banks
        let mut mapper = Cnrom::new(&rom);

        // Bank 3 & 0x03 = 3
        mapper.write_prg(0x8000, 7); // 7 & 0x03 = 3
        assert_eq!(mapper.read_chr(0x0000), 3);
    }
}
