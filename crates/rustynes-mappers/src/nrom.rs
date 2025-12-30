//! NROM Mapper (Mapper 0).
//!
//! The simplest NES mapper with no bank switching. Used by early games
//! like Super Mario Bros., Donkey Kong, and Ice Climber.
//!
//! Memory layout:
//! - PRG-ROM: 16KB or 32KB mapped to $8000-$FFFF
//! - CHR-ROM: 8KB mapped to PPU $0000-$1FFF (or CHR-RAM if none)
//! - No PRG-RAM (some variants have 2-8KB at $6000-$7FFF)

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// NROM mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Nrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-ROM/RAM data.
    chr: Vec<u8>,
    /// Whether CHR is RAM (writable).
    chr_is_ram: bool,
    /// PRG-ROM size (16KB or 32KB).
    prg_size: usize,
    /// Nametable mirroring mode.
    mirroring: Mirroring,
}

impl Nrom {
    /// Create a new NROM mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_size = rom.prg_rom.len();
        let chr_is_ram = rom.chr_rom.is_empty();
        let chr = if chr_is_ram {
            // 8KB CHR-RAM
            vec![0u8; 8192]
        } else {
            rom.chr_rom.clone()
        };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr,
            chr_is_ram,
            prg_size,
            mirroring: rom.header.mirroring,
        }
    }
}

impl Mapper for Nrom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // No PRG-RAM on standard NROM
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

    fn write_prg(&mut self, _addr: u16, _val: u8) {
        // NROM has no writable registers or PRG-RAM
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let offset = (addr & 0x1FFF) as usize;
        self.chr.get(offset).copied().unwrap_or(0)
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
        0
    }

    fn mapper_name(&self) -> &'static str {
        "NROM"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_size: usize, chr_size: usize) -> Rom {
        let prg_rom: Vec<u8> = (0..prg_size).map(|i| (i & 0xFF) as u8).collect();
        let chr_rom: Vec<u8> = (0..chr_size).map(|i| ((i + 128) & 0xFF) as u8).collect();

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 0,
                prg_rom_size: (prg_size / 16384) as u16,
                chr_rom_size: (chr_size / 8192) as u16,
                prg_ram_size: 0,
                chr_ram_size: if chr_size == 0 { 8192 } else { 0 },
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
    fn test_nrom_16kb_mirroring() {
        let rom = create_test_rom(16384, 8192);
        let mapper = Nrom::new(&rom);

        // Read from $8000 and $C000 should return same value (mirrored)
        assert_eq!(mapper.read_prg(0x8000), mapper.read_prg(0xC000));
        assert_eq!(mapper.read_prg(0x8100), mapper.read_prg(0xC100));
    }

    #[test]
    fn test_nrom_32kb_no_mirroring() {
        let rom = create_test_rom(32768, 8192);
        let mapper = Nrom::new(&rom);

        // $8000 and $C000 should be different in 32KB mode
        // $8000 maps to offset 0, $C000 maps to offset $4000
        let prg_low = mapper.read_prg(0x8000);
        let prg_high = mapper.read_prg(0xC000);
        assert_eq!(prg_low, 0x00);
        assert_eq!(prg_high, 0x00); // ((0x4000) & 0xFF) = 0
    }

    #[test]
    fn test_nrom_chr_rom() {
        let rom = create_test_rom(16384, 8192);
        let mut mapper = Nrom::new(&rom);

        // CHR-ROM should be readable
        assert_eq!(mapper.read_chr(0x0000), 128); // (0 + 128) & 0xFF

        // CHR-ROM should not be writable
        mapper.write_chr(0x0000, 0xFF);
        assert_eq!(mapper.read_chr(0x0000), 128);
    }

    #[test]
    fn test_nrom_chr_ram() {
        let rom = create_test_rom(16384, 0); // No CHR-ROM = CHR-RAM
        let mut mapper = Nrom::new(&rom);

        // CHR-RAM should be readable and writable
        assert_eq!(mapper.read_chr(0x0000), 0);
        mapper.write_chr(0x0000, 0x42);
        assert_eq!(mapper.read_chr(0x0000), 0x42);
    }

    #[test]
    fn test_nrom_mirroring() {
        let rom = create_test_rom(16384, 8192);
        let mapper = Nrom::new(&rom);

        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
        assert_eq!(mapper.mapper_number(), 0);
        assert_eq!(mapper.mapper_name(), "NROM");
    }
}
