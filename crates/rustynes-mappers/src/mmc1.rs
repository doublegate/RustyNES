//! Mapper 1: MMC1 (`SxROM`)
//!
//! MMC1 is Nintendo's first mapper IC featuring:
//! - 5-bit serial shift register interface
//! - Flexible PRG-ROM banking (16KB or 32KB modes)
//! - CHR-ROM banking (4KB or 8KB modes)
//! - Switchable mirroring
//! - Battery-backed SRAM support
//!
//! # Hardware Details
//!
//! - **PRG-ROM**: Up to 512KB (32 banks of 16KB)
//! - **CHR-ROM**: Up to 128KB (32 banks of 4KB, or 16 banks of 8KB)
//! - **PRG-RAM**: 8KB battery-backed SRAM (optional)
//! - **Mirroring**: Programmable (horizontal, vertical, single-screen)
//!
//! # Write Protocol
//!
//! MMC1 uses a 5-bit serial shift register:
//! 1. Write bit 0 to any address $8000-$FFFF
//! 2. Repeat 5 times (LSB first)
//! 3. On 5th write, register is loaded and shift register resets
//! 4. Writing with bit 7 set resets the shift register immediately
//!
//! # Registers
//!
//! - **Control ($8000-$9FFF)**: Mirroring, PRG mode, CHR mode
//! - **CHR bank 0 ($A000-$BFFF)**: CHR bank selection
//! - **CHR bank 1 ($C000-$DFFF)**: CHR bank selection (4KB mode only)
//! - **PRG bank ($E000-$FFFF)**: PRG bank selection
//!
//! # Games
//!
//! - The Legend of Zelda
//! - Metroid
//! - Mega Man 2
//! - Final Fantasy
//! - Castlevania II
//! - Kid Icarus

use crate::{Mapper, Mirroring, Rom};

/// MMC1 control register flags.
#[derive(Debug, Clone, Copy)]
struct ControlRegister {
    /// Mirroring mode (bits 0-1).
    mirroring: Mirroring,
    /// PRG-ROM banking mode (bits 2-3).
    prg_mode: PrgMode,
    /// CHR-ROM banking mode (bit 4).
    chr_mode: ChrMode,
}

impl ControlRegister {
    fn from_byte(value: u8) -> Self {
        let mirroring = match value & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };

        let prg_mode = match (value >> 2) & 0x03 {
            0 | 1 => PrgMode::Switch32KB,
            2 => PrgMode::FixFirst,
            3 => PrgMode::FixLast,
            _ => unreachable!(),
        };

        let chr_mode = if (value & 0x10) != 0 {
            ChrMode::Switch4KB
        } else {
            ChrMode::Switch8KB
        };

        Self {
            mirroring,
            prg_mode,
            chr_mode,
        }
    }
}

/// PRG-ROM banking mode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PrgMode {
    /// Switch 32KB at $8000, ignoring low bit of bank number.
    Switch32KB,
    /// Fix first bank at $8000, switch 16KB bank at $C000.
    FixFirst,
    /// Switch 16KB bank at $8000, fix last bank at $C000.
    FixLast,
}

/// CHR-ROM banking mode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChrMode {
    /// Switch 8KB at a time.
    Switch8KB,
    /// Switch two separate 4KB banks.
    Switch4KB,
}

/// MMC1 mapper implementation (Mapper 1).
#[derive(Clone)]
pub struct Mmc1 {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,

    /// CHR-ROM data (or empty for CHR-RAM).
    chr_rom: Vec<u8>,

    /// CHR-RAM (8KB if `chr_rom` is empty).
    chr_ram: Vec<u8>,

    /// Battery-backed SRAM (8KB).
    sram: Vec<u8>,

    /// 5-bit shift register.
    shift_register: u8,

    /// Shift register write count (0-4).
    shift_count: u8,

    /// Control register.
    control: ControlRegister,

    /// CHR bank 0 (used for both 4KB and 8KB modes).
    chr_bank_0: u8,

    /// CHR bank 1 (used only in 4KB mode).
    chr_bank_1: u8,

    /// PRG bank select.
    prg_bank: u8,

    /// Number of 16KB PRG banks.
    prg_banks: usize,

    /// Number of 4KB CHR banks.
    chr_banks: usize,

    /// True if using CHR-RAM instead of CHR-ROM.
    has_chr_ram: bool,

    /// Consecutive write protection counter.
    /// MMC1 ignores writes that occur within 2 CPU cycles of each other.
    /// This prevents glitches from games that have back-to-back writes.
    write_just_occurred: u8,
}

impl Mmc1 {
    /// Create a new MMC1 mapper from a ROM.
    ///
    /// # Arguments
    ///
    /// * `rom` - Loaded ROM file
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 16384;
        let has_chr_ram = rom.chr_rom.is_empty();
        let chr_banks = if has_chr_ram {
            2 // 8KB CHR-RAM = 2 * 4KB banks
        } else {
            rom.chr_rom.len() / 4096
        };

        let chr_ram = if has_chr_ram {
            vec![0; 8192]
        } else {
            Vec::new()
        };

        // MMC1 defaults: fix last bank, 8KB CHR mode, horizontal mirroring
        let control = ControlRegister {
            mirroring: Mirroring::Horizontal,
            prg_mode: PrgMode::FixLast,
            chr_mode: ChrMode::Switch8KB,
        };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_rom: rom.chr_rom.clone(),
            chr_ram,
            sram: vec![0; 8192],
            shift_register: 0,
            shift_count: 0,
            control,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            prg_banks,
            chr_banks,
            has_chr_ram,
            write_just_occurred: 0,
        }
    }

    /// Write to shift register.
    fn write_register(&mut self, addr: u16, value: u8) {
        // Consecutive write protection: ignore writes that occur within 2 CPU cycles
        // This is a hardware quirk that some games rely on
        if self.write_just_occurred > 0 {
            return;
        }

        // Mark that a write just occurred (will be decremented by clock())
        self.write_just_occurred = 2;

        // Bit 7 set = reset shift register
        if (value & 0x80) != 0 {
            self.shift_register = 0;
            self.shift_count = 0;
            // Also reset control to fix last bank mode
            self.control.prg_mode = PrgMode::FixLast;
            return;
        }

        // Load bit 0 into shift register
        self.shift_register |= (value & 0x01) << self.shift_count;
        self.shift_count += 1;

        // After 5 writes, load target register
        if self.shift_count == 5 {
            self.load_register(addr, self.shift_register);
            self.shift_register = 0;
            self.shift_count = 0;
        }
    }

    /// Load a register from the shift register value.
    fn load_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                // Control register
                self.control = ControlRegister::from_byte(value);
            }
            0xA000..=0xBFFF => {
                // CHR bank 0
                self.chr_bank_0 = value;
            }
            0xC000..=0xDFFF => {
                // CHR bank 1
                self.chr_bank_1 = value;
            }
            0xE000..=0xFFFF => {
                // PRG bank
                self.prg_bank = value & 0x0F;
            }
            _ => {}
        }
    }

    /// Get PRG bank number for a given address.
    fn get_prg_bank(&self, addr: u16) -> usize {
        let bank = match self.control.prg_mode {
            PrgMode::Switch32KB => {
                // 32KB mode: ignore low bit, each bank is 32KB
                if addr < 0xC000 {
                    (self.prg_bank & 0xFE) as usize
                } else {
                    (self.prg_bank | 0x01) as usize
                }
            }
            PrgMode::FixFirst => {
                // Fix first bank at $8000, switch at $C000
                if addr < 0xC000 {
                    0
                } else {
                    self.prg_bank as usize
                }
            }
            PrgMode::FixLast => {
                // Switch at $8000, fix last bank at $C000
                if addr < 0xC000 {
                    self.prg_bank as usize
                } else {
                    self.prg_banks - 1
                }
            }
        };

        bank % self.prg_banks
    }

    /// Get CHR bank number for a given address.
    fn get_chr_bank(&self, addr: u16) -> usize {
        let bank = match self.control.chr_mode {
            ChrMode::Switch8KB => {
                // 8KB mode: ignore low bit
                (self.chr_bank_0 & 0xFE) as usize + (addr / 0x1000) as usize
            }
            ChrMode::Switch4KB => {
                // 4KB mode: use separate banks
                if addr < 0x1000 {
                    self.chr_bank_0 as usize
                } else {
                    self.chr_bank_1 as usize
                }
            }
        };

        bank % self.chr_banks
    }
}

impl Mapper for Mmc1 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // SRAM
                let offset = (addr - 0x6000) as usize;
                self.sram[offset]
            }
            0x8000..=0xFFFF => {
                // PRG-ROM
                let bank = self.get_prg_bank(addr);
                let offset = (addr & 0x3FFF) as usize;
                self.prg_rom[bank * 16384 + offset]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // SRAM
                let offset = (addr - 0x6000) as usize;
                self.sram[offset] = value;
            }
            0x8000..=0xFFFF => {
                // Shift register write
                self.write_register(addr, value);
            }
            _ => {}
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");

        // Apply CHR banking to both CHR-ROM and CHR-RAM
        // This is critical for games like Kid Icarus that use CHR-RAM with banking
        let bank = self.get_chr_bank(addr);
        let offset = (addr & 0x0FFF) as usize;

        if self.has_chr_ram {
            self.chr_ram[bank * 4096 + offset]
        } else {
            self.chr_rom[bank * 4096 + offset]
        }
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");

        if self.has_chr_ram {
            // Apply CHR banking to CHR-RAM writes
            let bank = self.get_chr_bank(addr);
            let offset = (addr & 0x0FFF) as usize;
            self.chr_ram[bank * 4096 + offset] = value;
        }
        // CHR-ROM writes are ignored
    }

    fn mirroring(&self) -> Mirroring {
        self.control.mirroring
    }

    fn sram(&self) -> Option<&[u8]> {
        Some(&self.sram)
    }

    fn sram_mut(&mut self) -> Option<&mut [u8]> {
        Some(&mut self.sram)
    }

    fn mapper_number(&self) -> u16 {
        1
    }

    fn clone_mapper(&self) -> Box<dyn Mapper> {
        Box::new(self.clone())
    }

    fn clock(&mut self, cycles: u8) {
        // Decrement consecutive write protection counter by the number of cycles
        // This prevents back-to-back writes from corrupting the shift register
        if self.write_just_occurred > 0 {
            self.write_just_occurred = self.write_just_occurred.saturating_sub(cycles);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RomHeader;

    fn create_test_rom(prg_banks: usize, chr_banks: usize) -> Rom {
        let header = RomHeader {
            prg_rom_size: prg_banks * 16384,
            chr_rom_size: chr_banks * 4096,
            mapper_number: 1,
            submapper: 0,
            mirroring: Mirroring::Horizontal,
            has_battery: true,
            has_trainer: false,
            nes2_format: false,
            prg_ram_size: 8192,
            prg_nvram_size: 0,
            chr_ram_size: if chr_banks == 0 { 8192 } else { 0 },
            chr_nvram_size: 0,
        };

        Rom {
            header,
            trainer: None,
            prg_rom: vec![0; prg_banks * 16384],
            chr_rom: if chr_banks > 0 {
                vec![0; chr_banks * 4096]
            } else {
                Vec::new()
            },
        }
    }

    fn write_mmc1_register(mapper: &mut Mmc1, addr: u16, value: u8) {
        for i in 0..5 {
            mapper.write_prg(addr, (value >> i) & 0x01);
            // Simulate time passing between writes (each write takes ~3+ cycles)
            mapper.clock(3);
        }
    }

    #[test]
    fn test_mmc1_creation() {
        let rom = create_test_rom(16, 32);
        let mapper = Mmc1::new(&rom);

        assert_eq!(mapper.mapper_number(), 1);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert!(mapper.sram().is_some());
    }

    #[test]
    fn test_shift_register_5_writes() {
        let rom = create_test_rom(2, 2);
        let mut mapper = Mmc1::new(&rom);

        // Write 5 bits: 0b10101 (with proper timing between writes)
        mapper.write_prg(0xE000, 1); // Bit 0
        mapper.clock(3);
        mapper.write_prg(0xE000, 0); // Bit 1
        mapper.clock(3);
        mapper.write_prg(0xE000, 1); // Bit 2
        mapper.clock(3);
        mapper.write_prg(0xE000, 0); // Bit 3
        mapper.clock(3);
        mapper.write_prg(0xE000, 1); // Bit 4

        // Value should be 0b10101 = 21
        assert_eq!(mapper.prg_bank, 21 & 0x0F);
    }

    #[test]
    fn test_shift_register_reset() {
        let rom = create_test_rom(2, 2);
        let mut mapper = Mmc1::new(&rom);

        // Write 3 bits then reset (with proper timing between writes)
        mapper.write_prg(0xE000, 1);
        mapper.clock(3);
        mapper.write_prg(0xE000, 1);
        mapper.clock(3);
        mapper.write_prg(0xE000, 1);
        mapper.clock(3);
        mapper.write_prg(0xE000, 0x80); // Reset

        assert_eq!(mapper.shift_count, 0);
        assert_eq!(mapper.shift_register, 0);
    }

    #[test]
    fn test_control_register_mirroring() {
        let rom = create_test_rom(2, 2);
        let mut mapper = Mmc1::new(&rom);

        // Set vertical mirroring (bits 0-1 = 0b10)
        write_mmc1_register(&mut mapper, 0x8000, 0b00010);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        // Set horizontal mirroring (bits 0-1 = 0b11)
        write_mmc1_register(&mut mapper, 0x8000, 0b00011);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        // Set single-screen lower (bits 0-1 = 0b00)
        write_mmc1_register(&mut mapper, 0x8000, 0b00000);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_prg_mode_switch_32kb() {
        let rom = create_test_rom(4, 2);
        let mut mapper = Mmc1::new(&rom);

        // Set 32KB mode (bits 2-3 = 0b00 or 0b01)
        write_mmc1_register(&mut mapper, 0x8000, 0b00000);
        assert_eq!(mapper.control.prg_mode, PrgMode::Switch32KB);

        // Set bank 2 (should use banks 2 and 3)
        write_mmc1_register(&mut mapper, 0xE000, 0b00010);

        assert_eq!(mapper.get_prg_bank(0x8000), 2);
        assert_eq!(mapper.get_prg_bank(0xC000), 3);
    }

    #[test]
    fn test_prg_mode_fix_first() {
        let rom = create_test_rom(4, 2);
        let mut mapper = Mmc1::new(&rom);

        // Set fix first mode (bits 2-3 = 0b10)
        write_mmc1_register(&mut mapper, 0x8000, 0b01000);
        assert_eq!(mapper.control.prg_mode, PrgMode::FixFirst);

        // Set bank 2
        write_mmc1_register(&mut mapper, 0xE000, 0b00010);

        assert_eq!(mapper.get_prg_bank(0x8000), 0); // First bank fixed
        assert_eq!(mapper.get_prg_bank(0xC000), 2); // Bank 2 switchable
    }

    #[test]
    fn test_prg_mode_fix_last() {
        let rom = create_test_rom(4, 2);
        let mut mapper = Mmc1::new(&rom);

        // Set fix last mode (bits 2-3 = 0b11)
        write_mmc1_register(&mut mapper, 0x8000, 0b01100);
        assert_eq!(mapper.control.prg_mode, PrgMode::FixLast);

        // Set bank 1
        write_mmc1_register(&mut mapper, 0xE000, 0b00001);

        assert_eq!(mapper.get_prg_bank(0x8000), 1); // Bank 1 switchable
        assert_eq!(mapper.get_prg_bank(0xC000), 3); // Last bank fixed
    }

    #[test]
    fn test_chr_mode_8kb() {
        let rom = create_test_rom(2, 4);
        let mut mapper = Mmc1::new(&rom);

        // Set 8KB CHR mode (bit 4 = 0)
        write_mmc1_register(&mut mapper, 0x8000, 0b00000);
        assert_eq!(mapper.control.chr_mode, ChrMode::Switch8KB);

        // Set CHR bank 2 (uses banks 2 and 3)
        write_mmc1_register(&mut mapper, 0xA000, 0b00010);

        assert_eq!(mapper.get_chr_bank(0x0000), 2);
        assert_eq!(mapper.get_chr_bank(0x1000), 3);
    }

    #[test]
    fn test_chr_mode_4kb() {
        let rom = create_test_rom(2, 4);
        let mut mapper = Mmc1::new(&rom);

        // Set 4KB CHR mode (bit 4 = 1)
        write_mmc1_register(&mut mapper, 0x8000, 0b10000);
        assert_eq!(mapper.control.chr_mode, ChrMode::Switch4KB);

        // Set CHR bank 0
        write_mmc1_register(&mut mapper, 0xA000, 0b00001);
        // Set CHR bank 1
        write_mmc1_register(&mut mapper, 0xC000, 0b00011);

        assert_eq!(mapper.get_chr_bank(0x0000), 1);
        assert_eq!(mapper.get_chr_bank(0x1000), 3);
    }

    #[test]
    fn test_sram_read_write() {
        let rom = create_test_rom(2, 2);
        let mut mapper = Mmc1::new(&rom);

        mapper.write_prg(0x6000, 0x42);
        mapper.write_prg(0x7FFF, 0x55);

        assert_eq!(mapper.read_prg(0x6000), 0x42);
        assert_eq!(mapper.read_prg(0x7FFF), 0x55);
    }

    #[test]
    fn test_chr_ram() {
        let rom = create_test_rom(2, 0); // No CHR-ROM = CHR-RAM
        let mut mapper = Mmc1::new(&rom);

        mapper.write_chr(0x0000, 0xAA);
        mapper.write_chr(0x1FFF, 0xBB);

        assert_eq!(mapper.read_chr(0x0000), 0xAA);
        assert_eq!(mapper.read_chr(0x1FFF), 0xBB);
    }
}
