//! Mapper 4: MMC3 (`TxROM`)
//!
//! MMC3 is Nintendo's most advanced mapper IC featuring:
//! - Flexible PRG-ROM banking with 2 modes
//! - Flexible CHR-ROM banking with 2 modes
//! - Scanline counter IRQ for split-screen effects
//! - Mirroring control
//! - Battery-backed SRAM support
//!
//! # Hardware Details
//!
//! - **PRG-ROM**: Up to 512KB
//! - **CHR-ROM**: Up to 256KB
//! - **PRG-RAM**: 8KB battery-backed SRAM (optional)
//! - **Mirroring**: Programmable (horizontal or vertical)
//! - **IRQ**: Scanline counter triggered by PPU A12 rising edge
//!
//! # Registers
//!
//! - **$8000-$9FFF (even)**: Bank select (which bank register to update)
//! - **$8001-$9FFF (odd)**: Bank data (value to write to selected register)
//! - **$A000-$BFFF (even)**: Mirroring
//! - **$A001-$BFFF (odd)**: PRG-RAM protect
//! - **$C000-$DFFF (even)**: IRQ latch
//! - **$C001-$DFFF (odd)**: IRQ reload
//! - **$E000-$FFFF (even)**: IRQ disable
//! - **$E001-$FFFF (odd)**: IRQ enable
//!
//! # Banking Modes
//!
//! **PRG Mode 0**: $8000 swappable, $C000 fixed to second-last bank
//! **PRG Mode 1**: $C000 swappable, $8000 fixed to second-last bank
//!
//! **CHR Mode 0**: 2KB banks at $0000/$0800, 1KB banks at $1000-$1C00
//! **CHR Mode 1**: 2KB banks at $1000/$1800, 1KB banks at $0000-$0C00
//!
//! # IRQ System
//!
//! MMC3 uses PPU A12 rising edges to count scanlines:
//! - Counter decrements on each A12 rising edge
//! - When counter reaches 0, IRQ is triggered (if enabled)
//! - Counter reloads from latch value
//! - Used for split-screen effects (status bars, etc.)
//!
//! # Games
//!
//! - Super Mario Bros. 3
//! - Mega Man 3-6
//! - Kirby's Adventure
//! - Super Mario Bros. 2 (USA)
//! - Ninja Gaiden

use crate::{Mapper, Mirroring, Rom};

/// MMC3 mapper implementation (Mapper 4).
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct Mmc3 {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,

    /// CHR-ROM data (or empty for CHR-RAM).
    chr_rom: Vec<u8>,

    /// CHR-RAM (8KB if `chr_rom` is empty).
    chr_ram: Vec<u8>,

    /// Battery-backed SRAM (8KB).
    sram: Vec<u8>,

    /// Bank select register (which register to update).
    bank_select: u8,

    /// 8 bank data registers (R0-R7).
    bank_registers: [u8; 8],

    /// PRG banking mode (0 or 1).
    prg_mode: u8,

    /// CHR banking mode (0 or 1).
    chr_mode: u8,

    /// Current mirroring mode.
    mirroring: Mirroring,

    /// PRG-RAM write protection enabled.
    prg_ram_protect: bool,

    /// IRQ latch value (reload value).
    irq_latch: u8,

    /// IRQ counter.
    irq_counter: u8,

    /// IRQ enabled flag.
    irq_enable: bool,

    /// IRQ pending flag.
    irq_pending: bool,

    /// IRQ reload flag (set when counter should reload).
    irq_reload: bool,

    /// Number of 8KB PRG banks.
    prg_banks: usize,

    /// Number of 1KB CHR banks.
    chr_banks: usize,

    /// True if using CHR-RAM instead of CHR-ROM.
    has_chr_ram: bool,
}

impl Mmc3 {
    /// Create a new MMC3 mapper from a ROM.
    ///
    /// # Arguments
    ///
    /// * `rom` - Loaded ROM file
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 8192;
        let has_chr_ram = rom.chr_rom.is_empty();
        let chr_banks = if has_chr_ram {
            8 // 8KB CHR-RAM = 8 * 1KB banks
        } else {
            rom.chr_rom.len() / 1024
        };

        let chr_ram = if has_chr_ram {
            vec![0; 8192]
        } else {
            Vec::new()
        };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr_rom: rom.chr_rom.clone(),
            chr_ram,
            sram: vec![0; 8192],
            bank_select: 0,
            bank_registers: [0; 8],
            prg_mode: 0,
            chr_mode: 0,
            mirroring: rom.header.mirroring,
            prg_ram_protect: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_enable: false,
            irq_pending: false,
            irq_reload: false,
            prg_banks,
            chr_banks,
            has_chr_ram,
        }
    }

    /// Get PRG bank for a given address.
    fn get_prg_bank(&self, addr: u16) -> usize {
        let bank = match addr {
            0x8000..=0x9FFF => {
                // 8KB bank
                if self.prg_mode == 0 {
                    self.bank_registers[6] as usize
                } else {
                    self.prg_banks - 2 // Second-last bank
                }
            }
            0xA000..=0xBFFF => {
                // 8KB bank (always R7)
                self.bank_registers[7] as usize
            }
            0xC000..=0xDFFF => {
                // 8KB bank
                if self.prg_mode == 0 {
                    self.prg_banks - 2 // Second-last bank
                } else {
                    self.bank_registers[6] as usize
                }
            }
            0xE000..=0xFFFF => {
                // 8KB bank (always last bank)
                self.prg_banks - 1
            }
            _ => 0,
        };

        bank % self.prg_banks
    }

    /// Get CHR bank for a given address.
    fn get_chr_bank(&self, addr: u16) -> usize {
        let bank = if self.chr_mode == 0 {
            // CHR Mode 0: 2KB banks at $0000/$0800, 1KB banks at $1000-$1C00
            match addr {
                0x0000..=0x07FF => (self.bank_registers[0] & 0xFE) as usize,
                0x0800..=0x0FFF => (self.bank_registers[0] | 0x01) as usize,
                0x1000..=0x13FF => self.bank_registers[2] as usize,
                0x1400..=0x17FF => self.bank_registers[3] as usize,
                0x1800..=0x1BFF => self.bank_registers[4] as usize,
                0x1C00..=0x1FFF => self.bank_registers[5] as usize,
                _ => 0,
            }
        } else {
            // CHR Mode 1: 2KB banks at $1000/$1800, 1KB banks at $0000-$0C00
            match addr {
                0x0000..=0x03FF => self.bank_registers[2] as usize,
                0x0400..=0x07FF => self.bank_registers[3] as usize,
                0x0800..=0x0BFF => self.bank_registers[4] as usize,
                0x0C00..=0x0FFF => self.bank_registers[5] as usize,
                0x1000..=0x17FF => (self.bank_registers[0] & 0xFE) as usize,
                0x1800..=0x1FFF => (self.bank_registers[0] | 0x01) as usize,
                _ => 0,
            }
        };

        bank % self.chr_banks
    }

    /// Clock the IRQ counter (called on PPU A12 rising edge).
    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enable {
            self.irq_pending = true;
        }
    }
}

impl Mapper for Mmc3 {
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
                let offset = (addr & 0x1FFF) as usize;
                self.prg_rom[bank * 8192 + offset]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // SRAM (if not write-protected)
                if !self.prg_ram_protect {
                    let offset = (addr - 0x6000) as usize;
                    self.sram[offset] = value;
                }
            }
            0x8000..=0x9FFF => {
                if addr & 0x01 == 0 {
                    // Even: Bank select
                    self.bank_select = value & 0x07;
                    self.prg_mode = (value >> 6) & 0x01;
                    self.chr_mode = (value >> 7) & 0x01;
                } else {
                    // Odd: Bank data
                    let reg = self.bank_select as usize;
                    self.bank_registers[reg] = value;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 0x01 == 0 {
                    // Even: Mirroring
                    self.mirroring = if (value & 0x01) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                } else {
                    // Odd: PRG-RAM protect
                    self.prg_ram_protect = (value & 0x40) != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 0x01 == 0 {
                    // Even: IRQ latch
                    self.irq_latch = value;
                } else {
                    // Odd: IRQ reload
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 0x01 == 0 {
                    // Even: IRQ disable
                    self.irq_enable = false;
                    self.irq_pending = false;
                } else {
                    // Odd: IRQ enable
                    self.irq_enable = true;
                }
            }
            _ => {}
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        debug_assert!(addr <= 0x1FFF, "Invalid CHR address: ${addr:04X}");

        if self.has_chr_ram {
            self.chr_ram[addr as usize]
        } else {
            let bank = self.get_chr_bank(addr);
            let offset = (addr & 0x03FF) as usize;
            self.chr_rom[bank * 1024 + offset]
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

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    fn ppu_a12_edge(&mut self) {
        // Detect rising edge on A12
        // In real hardware, this triggers on PPU address changes
        // For simplicity, we assume the caller detects the edge
        self.clock_irq_counter();
    }

    fn sram(&self) -> Option<&[u8]> {
        Some(&self.sram)
    }

    fn sram_mut(&mut self) -> Option<&mut [u8]> {
        Some(&mut self.sram)
    }

    fn mapper_number(&self) -> u16 {
        4
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

    fn create_test_rom(prg_banks: usize, chr_banks: usize) -> Rom {
        let header = RomHeader {
            prg_rom_size: prg_banks * 8192,
            chr_rom_size: chr_banks * 1024,
            mapper_number: 4,
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
            prg_rom: (0..prg_banks * 8192).map(|i| (i / 8192) as u8).collect(),
            chr_rom: if chr_banks > 0 {
                (0..chr_banks * 1024).map(|i| (i / 1024) as u8).collect()
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn test_mmc3_creation() {
        let rom = create_test_rom(32, 128);
        let mapper = Mmc3::new(&rom);

        assert_eq!(mapper.mapper_number(), 4);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        assert!(mapper.sram().is_some());
    }

    #[test]
    fn test_prg_mode_0() {
        let rom = create_test_rom(32, 8);
        let mut mapper = Mmc3::new(&rom);

        // Set PRG mode 0 (bit 6 = 0)
        mapper.write_prg(0x8000, 0x06); // Select R6
        mapper.write_prg(0x8001, 5); // Set R6 = 5

        // Mode 0: $8000 = R6, $C000 = second-last
        assert_eq!(mapper.get_prg_bank(0x8000), 5);
        assert_eq!(mapper.get_prg_bank(0xC000), 30); // Second-last of 32
    }

    #[test]
    fn test_prg_mode_1() {
        let rom = create_test_rom(32, 8);
        let mut mapper = Mmc3::new(&rom);

        // Set PRG mode 1 (bit 6 = 1)
        mapper.write_prg(0x8000, 0x46); // Select R6, PRG mode 1
        mapper.write_prg(0x8001, 5); // Set R6 = 5

        // Mode 1: $8000 = second-last, $C000 = R6
        assert_eq!(mapper.get_prg_bank(0x8000), 30); // Second-last of 32
        assert_eq!(mapper.get_prg_bank(0xC000), 5);
    }

    #[test]
    fn test_chr_mode_0() {
        let rom = create_test_rom(8, 128);
        let mut mapper = Mmc3::new(&rom);

        // Set CHR mode 0 (bit 7 = 0)
        mapper.write_prg(0x8000, 0x00); // Select R0, CHR mode 0
        mapper.write_prg(0x8001, 10); // Set R0 = 10 (2KB bank)

        // Mode 0: $0000-$0FFF uses R0 (2KB)
        assert_eq!(mapper.get_chr_bank(0x0000), 10);
        assert_eq!(mapper.get_chr_bank(0x0800), 11);
    }

    #[test]
    fn test_chr_mode_1() {
        let rom = create_test_rom(8, 128);
        let mut mapper = Mmc3::new(&rom);

        // Set CHR mode 1 (bit 7 = 1)
        mapper.write_prg(0x8000, 0x80); // Select R0, CHR mode 1
        mapper.write_prg(0x8001, 10); // Set R0 = 10 (2KB bank)

        // Mode 1: $1000-$1FFF uses R0 (2KB)
        assert_eq!(mapper.get_chr_bank(0x1000), 10);
        assert_eq!(mapper.get_chr_bank(0x1800), 11);
    }

    #[test]
    fn test_mirroring_control() {
        let rom = create_test_rom(8, 8);
        let mut mapper = Mmc3::new(&rom);

        // Set vertical mirroring (bit 0 = 0)
        mapper.write_prg(0xA000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        // Set horizontal mirroring (bit 0 = 1)
        mapper.write_prg(0xA000, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn test_prg_ram_protect() {
        let rom = create_test_rom(8, 8);
        let mut mapper = Mmc3::new(&rom);

        // Initially not protected
        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        // Enable protection
        mapper.write_prg(0xA001, 0x40);
        mapper.write_prg(0x6000, 0x55);
        assert_eq!(mapper.read_prg(0x6000), 0x42); // Write ignored
    }

    #[test]
    fn test_irq_latch_and_counter() {
        let rom = create_test_rom(8, 8);
        let mut mapper = Mmc3::new(&rom);

        // Set latch value
        mapper.write_prg(0xC000, 10);
        assert_eq!(mapper.irq_latch, 10);

        // Trigger reload
        mapper.write_prg(0xC001, 0);
        assert!(mapper.irq_reload);
    }

    #[test]
    fn test_irq_generation() {
        let rom = create_test_rom(8, 8);
        let mut mapper = Mmc3::new(&rom);

        // Set latch to 2
        mapper.write_prg(0xC000, 2);
        // Reload counter
        mapper.write_prg(0xC001, 0);
        // Enable IRQ
        mapper.write_prg(0xE001, 0);

        assert!(!mapper.irq_pending());

        // Clock 1: counter = 2 -> 1
        mapper.ppu_a12_edge();
        assert!(!mapper.irq_pending());

        // Clock 2: counter = 1 -> 0, IRQ triggered
        mapper.ppu_a12_edge();
        assert!(!mapper.irq_pending()); // Not yet

        // Clock 3: counter = 0, IRQ triggered
        mapper.ppu_a12_edge();
        assert!(mapper.irq_pending());

        // Clear IRQ
        mapper.clear_irq();
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_irq_disable() {
        let rom = create_test_rom(8, 8);
        let mut mapper = Mmc3::new(&rom);

        // Set latch and enable
        mapper.write_prg(0xC000, 0);
        mapper.write_prg(0xE001, 0);

        // Clock to trigger IRQ
        mapper.ppu_a12_edge();
        mapper.ppu_a12_edge();

        // Disable IRQ
        mapper.write_prg(0xE000, 0);
        assert!(!mapper.irq_pending());
        assert!(!mapper.irq_enable);
    }

    #[test]
    fn test_chr_ram() {
        let rom = create_test_rom(8, 0); // No CHR-ROM = CHR-RAM
        let mut mapper = Mmc3::new(&rom);

        mapper.write_chr(0x0000, 0xAA);
        mapper.write_chr(0x1FFF, 0xBB);

        assert_eq!(mapper.read_chr(0x0000), 0xAA);
        assert_eq!(mapper.read_chr(0x1FFF), 0xBB);
    }

    #[test]
    fn test_sram_access() {
        let rom = create_test_rom(8, 8);
        let mut mapper = Mmc3::new(&rom);

        mapper.write_prg(0x6000, 0x42);
        mapper.write_prg(0x7FFF, 0x55);

        assert_eq!(mapper.read_prg(0x6000), 0x42);
        assert_eq!(mapper.read_prg(0x7FFF), 0x55);
    }
}
