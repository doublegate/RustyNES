//! Mapper 3: CNROM
//!
//! CNROM is a simple discrete logic mapper featuring:
//! - Fixed 16KB or 32KB PRG-ROM (no banking)
//! - Switchable 8KB CHR-ROM banks
//! - Single register write to any address $8000-$FFFF
//!
//! # Hardware Details
//!
//! - **PRG-ROM**: 16KB or 32KB (fixed, no banking)
//! - **CHR-ROM**: Up to 256KB (32 banks of 8KB)
//! - **Mirroring**: Fixed horizontal or vertical (hardware)
//! - **Bus Conflicts**: Yes (write value must match ROM data)
//!
//! # Games
//!
//! - Arkanoid
//! - Paperboy
//! - Solomon's Key
//! - Gradius
//! - Cybernoid

use crate::{Mapper, Mirroring, Rom};

/// CNROM mapper implementation (Mapper 3).
#[derive(Clone)]
pub struct Cnrom {
    /// PRG-ROM data (16KB or 32KB).
    prg_rom: Vec<u8>,

    /// CHR-ROM data.
    chr_rom: Vec<u8>,

    /// Nametable mirroring mode (fixed by hardware).
    mirroring: Mirroring,

    /// Currently selected CHR bank.
    chr_bank: u8,

    /// Number of 8KB CHR banks.
    chr_banks: usize,
}

impl Cnrom {
    /// Create a new CNROM mapper from a ROM.
    ///
    /// # Arguments
    ///
    /// * `rom` - Loaded ROM file
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - PRG-ROM size is not 16KB or 32KB
    /// - CHR-ROM is empty (CNROM requires CHR-ROM, not CHR-RAM)
    /// - CHR-ROM size is not a multiple of 8KB
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        // Validate PRG-ROM size
        assert!(
            rom.prg_rom.len() == 16384 || rom.prg_rom.len() == 32768,
            "CNROM requires 16KB or 32KB PRG-ROM, got {} bytes",
            rom.prg_rom.len()
        );

        // CNROM requires CHR-ROM
        assert!(
            !rom.chr_rom.is_empty(),
            "CNROM requires CHR-ROM (got CHR-RAM)"
        );

        // Validate CHR-ROM size
        assert_eq!(
            rom.chr_rom.len() % 8192,
            0,
            "CHR-ROM size must be a multiple of 8KB"
        );

        let chr_banks = rom.chr_rom.len() / 8192;

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_rom: rom.chr_rom.clone(),
            mirroring: rom.header.mirroring,
            chr_bank: 0,
            chr_banks,
        }
    }
}

impl Mapper for Cnrom {
    fn read_prg(&self, addr: u16) -> u8 {
        debug_assert!(addr >= 0x8000, "Invalid PRG address: ${addr:04X}");

        let offset = (addr - 0x8000) as usize;

        // Handle mirroring for 16KB PRG-ROM
        let masked_offset = if self.prg_rom.len() == 16384 {
            offset & 0x3FFF // Mirror 16KB to fill 32KB space
        } else {
            offset
        };

        self.prg_rom[masked_offset]
    }

    fn write_prg(&mut self, _addr: u16, value: u8) {
        // CHR bank select register
        // In real hardware, this would AND with ROM data (bus conflicts)
        // Only the lower 2 bits are used for bank selection (max 4 banks = 32KB)
        // But some games have more, so we mask to the actual number of banks
        self.chr_bank = value;
    }

    fn read_chr(&self, addr: u16) -> u8 {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");

        let bank = (self.chr_bank as usize) % self.chr_banks;
        let offset = addr as usize;
        self.chr_rom[bank * 8192 + offset]
    }

    fn write_chr(&mut self, _addr: u16, _value: u8) {
        // CHR-ROM is not writable
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_number(&self) -> u16 {
        3
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

    fn create_test_rom(prg_size: usize, chr_banks: usize, mirroring: Mirroring) -> Rom {
        let header = RomHeader {
            prg_rom_size: prg_size,
            chr_rom_size: chr_banks * 8192,
            mapper_number: 3,
            submapper: 0,
            mirroring,
            has_battery: false,
            has_trainer: false,
            nes2_format: false,
            prg_ram_size: 0,
            prg_nvram_size: 0,
            chr_ram_size: 0,
            chr_nvram_size: 0,
        };

        Rom {
            header,
            trainer: None,
            prg_rom: vec![0; prg_size],
            chr_rom: (0..chr_banks * 8192).map(|i| (i / 8192) as u8).collect(),
        }
    }

    #[test]
    fn test_cnrom_creation() {
        let rom = create_test_rom(32768, 4, Mirroring::Horizontal);
        let mapper = Cnrom::new(&rom);

        assert_eq!(mapper.mapper_number(), 3);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert_eq!(mapper.chr_banks, 4);
    }

    #[test]
    fn test_cnrom_16kb_prg() {
        let rom = create_test_rom(16384, 2, Mirroring::Vertical);
        let mapper = Cnrom::new(&rom);

        assert_eq!(mapper.prg_rom.len(), 16384);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn test_prg_read_32kb() {
        let mut rom = create_test_rom(32768, 2, Mirroring::Horizontal);
        rom.prg_rom[0x0000] = 0x42;
        rom.prg_rom[0x7FFF] = 0x55;

        let mapper = Cnrom::new(&rom);

        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0x55);
    }

    #[test]
    fn test_prg_read_16kb_mirroring() {
        let mut rom = create_test_rom(16384, 2, Mirroring::Horizontal);
        rom.prg_rom[0x0000] = 0x42;
        rom.prg_rom[0x3FFF] = 0x55;

        let mapper = Cnrom::new(&rom);

        // First 16KB
        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xBFFF), 0x55);

        // Mirrored second 16KB
        assert_eq!(mapper.read_prg(0xC000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0x55);
    }

    #[test]
    fn test_chr_bank_switching() {
        let rom = create_test_rom(32768, 4, Mirroring::Horizontal);
        let mut mapper = Cnrom::new(&rom);

        // Initial bank is 0
        assert_eq!(mapper.chr_bank, 0);
        assert_eq!(mapper.read_chr(0x0000), 0);

        // Switch to bank 1
        mapper.write_prg(0x8000, 1);
        assert_eq!(mapper.chr_bank, 1);
        assert_eq!(mapper.read_chr(0x0000), 1);

        // Switch to bank 2
        mapper.write_prg(0x8000, 2);
        assert_eq!(mapper.chr_bank, 2);
        assert_eq!(mapper.read_chr(0x0000), 2);

        // Switch to bank 3
        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.chr_bank, 3);
        assert_eq!(mapper.read_chr(0x0000), 3);
    }

    #[test]
    fn test_chr_bank_wrapping() {
        let rom = create_test_rom(32768, 4, Mirroring::Horizontal);
        let mut mapper = Cnrom::new(&rom);

        // Writing bank 7 should wrap to bank 3 (7 % 4)
        mapper.write_prg(0x8000, 7);
        assert_eq!(mapper.read_chr(0x0000), 3);
    }

    #[test]
    fn test_chr_read_full_range() {
        let rom = create_test_rom(32768, 2, Mirroring::Horizontal);
        let mut mapper = Cnrom::new(&rom);

        // Set bank 1
        mapper.write_prg(0x8000, 1);

        // Bank 1 data starts at byte 8192
        assert_eq!(mapper.read_chr(0x0000), 1);
        assert_eq!(mapper.read_chr(0x1000), 1);
        assert_eq!(mapper.read_chr(0x1FFF), 1);
    }

    #[test]
    fn test_chr_write_ignored() {
        let rom = create_test_rom(32768, 2, Mirroring::Horizontal);
        let mut mapper = Cnrom::new(&rom);

        // CHR-ROM writes should be ignored
        mapper.write_chr(0x0000, 0x42);
        assert_eq!(mapper.read_chr(0x0000), 0);
    }

    #[test]
    fn test_mirroring_modes() {
        let rom_h = create_test_rom(32768, 2, Mirroring::Horizontal);
        let mapper_h = Cnrom::new(&rom_h);
        assert_eq!(mapper_h.mirroring(), Mirroring::Horizontal);

        let rom_v = create_test_rom(32768, 2, Mirroring::Vertical);
        let mapper_v = Cnrom::new(&rom_v);
        assert_eq!(mapper_v.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn test_no_irq_or_sram() {
        let rom = create_test_rom(32768, 2, Mirroring::Horizontal);
        let mapper = Cnrom::new(&rom);

        assert!(!mapper.irq_pending());
        assert!(mapper.sram().is_none());
    }

    #[test]
    fn test_clone_mapper() {
        let rom = create_test_rom(32768, 4, Mirroring::Horizontal);
        let mut mapper = Cnrom::new(&rom);

        mapper.write_prg(0x8000, 2);

        let cloned = mapper.clone_mapper();
        assert_eq!(cloned.read_chr(0x0000), 2);
    }

    #[test]
    #[should_panic(expected = "CNROM requires 16KB or 32KB PRG-ROM")]
    fn test_invalid_prg_size() {
        let rom = create_test_rom(8192, 2, Mirroring::Horizontal);
        let _ = Cnrom::new(&rom);
    }

    #[test]
    #[should_panic(expected = "CNROM requires CHR-ROM")]
    fn test_chr_ram_not_allowed() {
        let header = RomHeader {
            prg_rom_size: 32768,
            chr_rom_size: 0, // CHR-RAM not allowed
            mapper_number: 3,
            submapper: 0,
            mirroring: Mirroring::Horizontal,
            has_battery: false,
            has_trainer: false,
            nes2_format: false,
            prg_ram_size: 0,
            prg_nvram_size: 0,
            chr_ram_size: 8192,
            chr_nvram_size: 0,
        };

        let rom = Rom {
            header,
            trainer: None,
            prg_rom: vec![0; 32768],
            chr_rom: Vec::new(),
        };

        let _ = Cnrom::new(&rom);
    }
}
