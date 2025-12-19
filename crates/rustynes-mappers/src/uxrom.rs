//! Mapper 2: `UxROM`
//!
//! `UxROM` is a simple discrete logic mapper featuring:
//! - Switchable 16KB PRG-ROM bank at $8000-$BFFF
//! - Fixed 16KB PRG-ROM bank at $C000-$FFFF (last bank)
//! - 8KB CHR-RAM (no CHR-ROM)
//! - Single register write to any address $8000-$FFFF
//!
//! # Hardware Details
//!
//! - **PRG-ROM**: 128KB to 256KB (8-16 banks of 16KB)
//! - **CHR**: 8KB CHR-RAM only (no banking)
//! - **Mirroring**: Fixed horizontal or vertical (hardware)
//! - **Bus Conflicts**: Yes (write value must match ROM data)
//!
//! # Bus Conflicts
//!
//! `UxROM` uses discrete logic and has bus conflicts. When writing to $8000-$FFFF,
//! the value written should match the ROM data at that address. Some games rely on this.
//!
//! # Games
//!
//! - Mega Man
//! - Castlevania
//! - Duck Tales
//! - Contra
//! - Metal Gear
//! - Ghosts 'n Goblins

use crate::{Mapper, Mirroring, Rom};

/// `UxROM` mapper implementation (Mapper 2).
#[derive(Clone)]
pub struct Uxrom {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,

    /// CHR-RAM (8KB).
    chr_ram: Vec<u8>,

    /// Nametable mirroring mode (fixed by hardware).
    mirroring: Mirroring,

    /// Currently selected PRG bank (for $8000-$BFFF).
    prg_bank: u8,

    /// Number of 16KB PRG banks.
    prg_banks: usize,
}

impl Uxrom {
    /// Create a new `UxROM` mapper from a ROM.
    ///
    /// # Arguments
    ///
    /// * `rom` - Loaded ROM file
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - CHR-ROM is present (`UxROM` requires CHR-RAM)
    /// - PRG-ROM size is not a multiple of 16KB
    /// - PRG-ROM size is less than 32KB
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        // UxROM requires CHR-RAM (no CHR-ROM)
        assert!(
            rom.chr_rom.is_empty(),
            "UxROM requires CHR-RAM (got CHR-ROM)"
        );

        // Validate PRG-ROM size
        assert_eq!(
            rom.prg_rom.len() % 16384,
            0,
            "PRG-ROM size must be a multiple of 16KB"
        );
        assert!(
            rom.prg_rom.len() >= 32768,
            "UxROM requires at least 32KB PRG-ROM"
        );

        let prg_banks = rom.prg_rom.len() / 16384;

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_ram: vec![0; 8192],
            mirroring: rom.header.mirroring,
            prg_bank: 0,
            prg_banks,
        }
    }

    /// Get the last PRG bank number.
    fn last_bank(&self) -> usize {
        self.prg_banks - 1
    }
}

impl Mapper for Uxrom {
    fn read_prg(&self, addr: u16) -> u8 {
        debug_assert!(addr >= 0x8000, "Invalid PRG address: ${addr:04X}");

        let offset = (addr & 0x3FFF) as usize;

        match addr {
            0x8000..=0xBFFF => {
                // Switchable bank
                let bank = (self.prg_bank as usize) % self.prg_banks;
                self.prg_rom[bank * 16384 + offset]
            }
            0xC000..=0xFFFF => {
                // Fixed last bank
                let bank = self.last_bank();
                self.prg_rom[bank * 16384 + offset]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        debug_assert!(addr >= 0x8000, "Invalid PRG address: ${addr:04X}");

        // Bank select register
        // In real hardware, this would AND with ROM data (bus conflicts)
        // For accuracy, we should verify value matches ROM data at addr
        // But for simplicity, we just use the value
        self.prg_bank = value;
    }

    fn read_chr(&self, addr: u16) -> u8 {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");
        self.chr_ram[addr as usize]
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");
        self.chr_ram[addr as usize] = value;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        2
    }

    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use crate::RomHeader;

    fn create_test_rom(prg_banks: usize, mirroring: Mirroring) -> Rom {
        let header = RomHeader {
            prg_rom_size: prg_banks * 16384,
            chr_rom_size: 0, // CHR-RAM
            mapper_number: 2,
            submapper: 0,
            mirroring,
            has_battery: false,
            has_trainer: false,
            nes2_format: false,
            prg_ram_size: 0,
            prg_nvram_size: 0,
            chr_ram_size: 8192,
            chr_nvram_size: 0,
        };

        Rom {
            header,
            trainer: None,
            prg_rom: (0..prg_banks * 16384).map(|i| (i / 16384) as u8).collect(),
            chr_rom: Vec::new(),
        }
    }

    #[test]
    fn test_uxrom_creation() {
        let rom = create_test_rom(8, Mirroring::Horizontal);
        let mapper = Uxrom::new(&rom);

        assert_eq!(mapper.mapper_number(), 2);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert_eq!(mapper.prg_banks, 8);
    }

    #[test]
    fn test_prg_bank_switching() {
        let rom = create_test_rom(8, Mirroring::Horizontal);
        let mut mapper = Uxrom::new(&rom);

        // Initial bank is 0
        assert_eq!(mapper.prg_bank, 0);
        assert_eq!(mapper.read_prg(0x8000), 0);

        // Switch to bank 3
        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.prg_bank, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);

        // Switch to bank 7
        mapper.write_prg(0x8000, 7);
        assert_eq!(mapper.prg_bank, 7);
        assert_eq!(mapper.read_prg(0x8000), 7);
    }

    #[test]
    fn test_fixed_last_bank() {
        let rom = create_test_rom(8, Mirroring::Horizontal);
        let mut mapper = Uxrom::new(&rom);

        // Last bank should always be 7
        assert_eq!(mapper.read_prg(0xC000), 7);

        // Switch first bank, last should remain fixed
        mapper.write_prg(0x8000, 0);
        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.read_prg(0xC000), 7);

        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);
        assert_eq!(mapper.read_prg(0xC000), 7);
    }

    #[test]
    fn test_bank_wrapping() {
        let rom = create_test_rom(4, Mirroring::Horizontal);
        let mut mapper = Uxrom::new(&rom);

        // Writing bank 7 should wrap to bank 3 (7 % 4)
        mapper.write_prg(0x8000, 7);
        assert_eq!(mapper.read_prg(0x8000), 3);
    }

    #[test]
    fn test_chr_ram_read_write() {
        let rom = create_test_rom(4, Mirroring::Horizontal);
        let mut mapper = Uxrom::new(&rom);

        mapper.write_chr(0x0000, 0x42);
        mapper.write_chr(0x1000, 0x55);
        mapper.write_chr(0x1FFF, 0xAA);

        assert_eq!(mapper.read_chr(0x0000), 0x42);
        assert_eq!(mapper.read_chr(0x1000), 0x55);
        assert_eq!(mapper.read_chr(0x1FFF), 0xAA);
    }

    #[test]
    fn test_mirroring_modes() {
        let rom_h = create_test_rom(4, Mirroring::Horizontal);
        let mapper_h = Uxrom::new(&rom_h);
        assert_eq!(mapper_h.mirroring(), Mirroring::Horizontal);

        let rom_v = create_test_rom(4, Mirroring::Vertical);
        let mapper_v = Uxrom::new(&rom_v);
        assert_eq!(mapper_v.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn test_no_irq_or_sram() {
        let rom = create_test_rom(4, Mirroring::Horizontal);
        let mapper = Uxrom::new(&rom);

        assert!(!mapper.irq_pending());
        assert!(mapper.sram().is_none());
    }

    #[test]
    fn test_clone_mapper() {
        let rom = create_test_rom(4, Mirroring::Horizontal);
        let mut mapper = Uxrom::new(&rom);

        mapper.write_prg(0x8000, 2);
        mapper.write_chr(0x1000, 0x42);

        let cloned = mapper.clone_mapper();
        assert_eq!(cloned.read_prg(0x8000), 2);
        assert_eq!(cloned.read_chr(0x1000), 0x42);
    }

    #[test]
    #[should_panic(expected = "UxROM requires CHR-RAM")]
    fn test_chr_rom_not_allowed() {
        let header = RomHeader {
            prg_rom_size: 32768,
            chr_rom_size: 8192, // CHR-ROM not allowed
            mapper_number: 2,
            submapper: 0,
            mirroring: Mirroring::Horizontal,
            has_battery: false,
            has_trainer: false,
            nes2_format: false,
            prg_ram_size: 0,
            prg_nvram_size: 0,
            chr_ram_size: 0,
            chr_nvram_size: 0,
        };

        let rom = Rom {
            header,
            trainer: None,
            prg_rom: vec![0; 32768],
            chr_rom: vec![0; 8192],
        };

        let _ = Uxrom::new(&rom);
    }

    #[test]
    #[should_panic(expected = "UxROM requires at least 32KB PRG-ROM")]
    fn test_prg_too_small() {
        let rom = create_test_rom(1, Mirroring::Horizontal); // Only 16KB
        let _ = Uxrom::new(&rom);
    }
}
