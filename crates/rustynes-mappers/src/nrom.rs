//! Mapper 0: NROM
//!
//! NROM is the simplest NES mapper with no bank switching capabilities.
//! It provides direct memory mapping with optional mirroring for 16KB PRG-ROM.
//!
//! # Hardware Details
//!
//! - **PRG-ROM**: 16KB or 32KB
//! - **CHR**: 8KB CHR-ROM or CHR-RAM
//! - **Mirroring**: Fixed horizontal or vertical
//! - **Battery**: Not supported
//!
//! # Variants
//!
//! - **NROM-128**: 16KB PRG-ROM (mirrored to fill 32KB)
//! - **NROM-256**: 32KB PRG-ROM (no mirroring needed)
//!
//! # Games
//!
//! - Super Mario Bros.
//! - Donkey Kong
//! - Balloon Fight
//! - Excitebike
//! - Ice Climber
//!
//! # Memory Map
//!
//! ```text
//! CPU:
//! $8000-$BFFF: First 16KB of PRG-ROM (or mirrored in NROM-128)
//! $C000-$FFFF: Last 16KB of PRG-ROM (or mirrored in NROM-128)
//!
//! PPU:
//! $0000-$1FFF: 8KB CHR-ROM/RAM (no banking)
//! ```

use crate::{Mapper, Mirroring, Rom};

/// NROM mapper implementation (Mapper 0).
#[derive(Clone)]
pub struct Nrom {
    /// PRG-ROM data (16KB or 32KB).
    prg_rom: Vec<u8>,

    /// CHR-ROM data, or empty if CHR-RAM.
    chr_rom: Vec<u8>,

    /// CHR-RAM (8KB if `chr_rom` is empty).
    chr_ram: Vec<u8>,

    /// Nametable mirroring mode.
    mirroring: Mirroring,

    /// True if using CHR-RAM instead of CHR-ROM.
    has_chr_ram: bool,
}

impl Nrom {
    /// Create a new NROM mapper from a ROM.
    ///
    /// # Arguments
    ///
    /// * `rom` - Loaded ROM file
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - PRG-ROM size is not 16KB or 32KB
    /// - CHR size is not 8KB (or 0 for CHR-RAM)
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        // Validate PRG-ROM size
        assert!(
            rom.prg_rom.len() == 16384 || rom.prg_rom.len() == 32768,
            "NROM requires 16KB or 32KB PRG-ROM, got {} bytes",
            rom.prg_rom.len()
        );

        // Determine if using CHR-RAM or CHR-ROM
        let has_chr_ram = rom.chr_rom.is_empty();
        let chr_ram = if has_chr_ram {
            vec![0; 8192] // 8KB CHR-RAM
        } else {
            Vec::new()
        };

        // Validate CHR-ROM size if present
        if !has_chr_ram {
            assert_eq!(
                rom.chr_rom.len(),
                8192,
                "NROM requires 8KB CHR-ROM, got {} bytes",
                rom.chr_rom.len()
            );
        }

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_rom: rom.chr_rom.clone(),
            chr_ram,
            mirroring: rom.header.mirroring,
            has_chr_ram,
        }
    }

    /// Get PRG-ROM size in bytes.
    #[must_use]
    pub fn prg_size(&self) -> usize {
        self.prg_rom.len()
    }

    /// Check if using CHR-RAM.
    #[must_use]
    pub fn has_chr_ram(&self) -> bool {
        self.has_chr_ram
    }
}

impl Mapper for Nrom {
    fn read_prg(&self, addr: u16) -> u8 {
        debug_assert!(addr >= 0x8000, "Invalid PRG address: ${addr:04X}");

        let offset = (addr - 0x8000) as usize;

        // Handle mirroring for NROM-128 (16KB)
        let masked_offset = if self.prg_rom.len() == 16384 {
            offset & 0x3FFF // Mirror 16KB to fill 32KB space
        } else {
            offset
        };

        self.prg_rom[masked_offset]
    }

    fn write_prg(&mut self, _addr: u16, _value: u8) {
        // NROM has no writable registers
        // Writes to PRG space are ignored
    }

    fn read_chr(&self, addr: u16) -> u8 {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");

        if self.has_chr_ram {
            self.chr_ram[addr as usize]
        } else {
            self.chr_rom[addr as usize]
        }
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");

        if self.has_chr_ram {
            self.chr_ram[addr as usize] = value;
        }
        // CHR-ROM writes are ignored
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        0
    }

    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RomHeader;

    fn create_test_rom(prg_size: usize, chr_size: usize, mirroring: Mirroring) -> Rom {
        let header = RomHeader {
            prg_rom_size: prg_size,
            chr_rom_size: chr_size,
            mapper_number: 0,
            submapper: 0,
            mirroring,
            has_battery: false,
            has_trainer: false,
            nes2_format: false,
            prg_ram_size: 0,
            prg_nvram_size: 0,
            chr_ram_size: if chr_size == 0 { 8192 } else { 0 },
            chr_nvram_size: 0,
        };

        Rom {
            header,
            trainer: None,
            prg_rom: vec![0; prg_size],
            chr_rom: if chr_size > 0 {
                vec![0; chr_size]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn test_nrom_256() {
        let rom = create_test_rom(32768, 8192, Mirroring::Horizontal);
        let mapper = Nrom::new(&rom);

        assert_eq!(mapper.prg_size(), 32768);
        assert_eq!(mapper.mapper_number(), 0);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert!(!mapper.has_chr_ram());
    }

    #[test]
    fn test_nrom_128() {
        let rom = create_test_rom(16384, 8192, Mirroring::Vertical);
        let mapper = Nrom::new(&rom);

        assert_eq!(mapper.prg_size(), 16384);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn test_prg_read_nrom_256() {
        let mut rom = create_test_rom(32768, 8192, Mirroring::Horizontal);
        rom.prg_rom[0x0000] = 0x42;
        rom.prg_rom[0x7FFF] = 0x55;

        let mapper = Nrom::new(&rom);

        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0x55);
    }

    #[test]
    fn test_prg_read_nrom_128_mirroring() {
        let mut rom = create_test_rom(16384, 8192, Mirroring::Horizontal);
        rom.prg_rom[0x0000] = 0x42;
        rom.prg_rom[0x3FFF] = 0x55;

        let mapper = Nrom::new(&rom);

        // First 16KB
        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xBFFF), 0x55);

        // Mirrored second 16KB
        assert_eq!(mapper.read_prg(0xC000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0x55);
    }

    #[test]
    fn test_chr_rom_read() {
        let mut rom = create_test_rom(16384, 8192, Mirroring::Horizontal);
        rom.chr_rom[0x0000] = 0xAA;
        rom.chr_rom[0x1FFF] = 0xBB;

        let mapper = Nrom::new(&rom);

        assert_eq!(mapper.read_chr(0x0000), 0xAA);
        assert_eq!(mapper.read_chr(0x1FFF), 0xBB);
    }

    #[test]
    fn test_chr_ram_read_write() {
        let rom = create_test_rom(16384, 0, Mirroring::Horizontal);
        let mut mapper = Nrom::new(&rom);

        assert!(mapper.has_chr_ram());

        // Write to CHR-RAM
        mapper.write_chr(0x0000, 0x42);
        mapper.write_chr(0x1FFF, 0x55);

        // Read back
        assert_eq!(mapper.read_chr(0x0000), 0x42);
        assert_eq!(mapper.read_chr(0x1FFF), 0x55);
    }

    #[test]
    fn test_chr_rom_write_ignored() {
        let mut rom = create_test_rom(16384, 8192, Mirroring::Horizontal);
        rom.chr_rom[0x0000] = 0xAA;

        let mut mapper = Nrom::new(&rom);

        // Write should be ignored
        mapper.write_chr(0x0000, 0x42);
        assert_eq!(mapper.read_chr(0x0000), 0xAA);
    }

    #[test]
    fn test_prg_write_ignored() {
        let mut rom = create_test_rom(32768, 8192, Mirroring::Horizontal);
        rom.prg_rom[0x0000] = 0xAA;

        let mut mapper = Nrom::new(&rom);

        // Write should be ignored
        mapper.write_prg(0x8000, 0x42);
        assert_eq!(mapper.read_prg(0x8000), 0xAA);
    }

    #[test]
    fn test_no_irq() {
        let rom = create_test_rom(16384, 8192, Mirroring::Horizontal);
        let mapper = Nrom::new(&rom);

        assert!(!mapper.irq_pending());
        assert!(mapper.sram().is_none());
    }

    #[test]
    fn test_clone_mapper() {
        let rom = create_test_rom(16384, 0, Mirroring::Horizontal);
        let mut mapper = Nrom::new(&rom);

        mapper.write_chr(0x1000, 0x42);

        let cloned = mapper.clone_mapper();
        assert_eq!(cloned.read_chr(0x1000), 0x42);
        assert_eq!(cloned.mapper_number(), 0);
    }

    #[test]
    #[should_panic(expected = "NROM requires 16KB or 32KB PRG-ROM")]
    fn test_invalid_prg_size() {
        let rom = create_test_rom(8192, 8192, Mirroring::Horizontal);
        let _ = Nrom::new(&rom);
    }

    #[test]
    #[should_panic(expected = "NROM requires 8KB CHR-ROM")]
    fn test_invalid_chr_size() {
        let rom = create_test_rom(16384, 16384, Mirroring::Horizontal);
        let _ = Nrom::new(&rom);
    }
}
