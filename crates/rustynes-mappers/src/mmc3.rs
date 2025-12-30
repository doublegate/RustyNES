//! MMC3 Mapper (Mapper 4).
//!
//! The most popular NES mapper, used by hundreds of games including
//! Super Mario Bros. 3, Mega Man 3-6, and Kirby's Adventure.
//!
//! Features:
//! - 8KB PRG-RAM at $6000-$7FFF (optionally battery-backed)
//! - Fine-grained PRG-ROM banking: 8KB banks
//! - Fine-grained CHR-ROM banking: 1KB and 2KB banks
//! - Mirroring control (H/V)
//! - Scanline counter IRQ for split-screen effects
//!
//! Bank Configuration:
//! - 8 bank registers (R0-R7) selected via bank select register
//! - PRG mode bit swaps $8000/$C000 banks
//! - CHR A12 inversion swaps pattern table banks

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// MMC3 mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(dead_code)] // last_a12 reserved for accurate A12 edge detection
#[allow(clippy::struct_excessive_bools)] // Hardware state requires multiple flags
pub struct Mmc3 {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-ROM/RAM data.
    chr: Vec<u8>,
    /// PRG-RAM data (8KB).
    prg_ram: Vec<u8>,
    /// Whether CHR is RAM (writable).
    chr_is_ram: bool,
    /// Number of PRG-ROM banks (8KB each).
    prg_banks: usize,
    /// Number of CHR banks (1KB each).
    chr_banks: usize,

    // Bank select register ($8000)
    /// Bank register index to update (0-7).
    bank_select: u8,
    /// PRG-ROM bank mode (0 = $8000 swappable, 1 = $C000 swappable).
    prg_mode: bool,
    /// CHR A12 inversion (0 = normal, 1 = inverted).
    chr_inversion: bool,

    // Bank registers
    /// R0: 2KB CHR bank at PPU $0000 (or $1000 if inverted).
    chr_bank_2k_0: u8,
    /// R1: 2KB CHR bank at PPU $0800 (or $1800 if inverted).
    chr_bank_2k_1: u8,
    /// R2: 1KB CHR bank at PPU $1000 (or $0000 if inverted).
    chr_bank_1k_0: u8,
    /// R3: 1KB CHR bank at PPU $1400 (or $0400 if inverted).
    chr_bank_1k_1: u8,
    /// R4: 1KB CHR bank at PPU $1800 (or $0800 if inverted).
    chr_bank_1k_2: u8,
    /// R5: 1KB CHR bank at PPU $1C00 (or $0C00 if inverted).
    chr_bank_1k_3: u8,
    /// R6: 8KB PRG bank at $8000 (or $C000 if prg_mode).
    prg_bank_0: u8,
    /// R7: 8KB PRG bank at $A000.
    prg_bank_1: u8,

    /// Nametable mirroring mode.
    mirroring: Mirroring,
    /// PRG-RAM write protection.
    prg_ram_protect: bool,
    /// PRG-RAM chip enable.
    prg_ram_enabled: bool,

    // IRQ counter
    /// IRQ counter reload value.
    irq_latch: u8,
    /// Current IRQ counter value.
    irq_counter: u8,
    /// IRQ counter reload flag.
    irq_reload: bool,
    /// IRQ enabled flag.
    irq_enabled: bool,
    /// IRQ pending flag.
    irq_pending: bool,

    /// Previous A12 state for edge detection.
    last_a12: bool,
    /// A12 filter counter (for ignoring short pulses).
    a12_filter: u8,

    /// Has battery-backed RAM.
    has_battery: bool,
}

impl Mmc3 {
    /// Create a new MMC3 mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 8192;
        let chr_is_ram = rom.chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 8192]
        } else {
            rom.chr_rom.clone()
        };
        let chr_banks = (chr.len() / 1024).max(1);

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr,
            prg_ram: vec![0u8; 8192],
            chr_is_ram,
            prg_banks,
            chr_banks,
            bank_select: 0,
            prg_mode: false,
            chr_inversion: false,
            chr_bank_2k_0: 0,
            chr_bank_2k_1: 2,
            chr_bank_1k_0: 4,
            chr_bank_1k_1: 5,
            chr_bank_1k_2: 6,
            chr_bank_1k_3: 7,
            prg_bank_0: 0,
            prg_bank_1: 1,
            mirroring: rom.header.mirroring,
            prg_ram_protect: false,
            prg_ram_enabled: true,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
            last_a12: false,
            a12_filter: 0,
            has_battery: rom.header.has_battery,
        }
    }

    /// Get PRG-ROM address for a CPU address.
    fn prg_addr(&self, addr: u16) -> usize {
        let bank = match addr {
            0x8000..=0x9FFF => {
                if self.prg_mode {
                    self.prg_banks.saturating_sub(2) // Fixed second-to-last
                } else {
                    self.prg_bank_0 as usize
                }
            }
            0xA000..=0xBFFF => self.prg_bank_1 as usize,
            0xC000..=0xDFFF => {
                if self.prg_mode {
                    self.prg_bank_0 as usize
                } else {
                    self.prg_banks.saturating_sub(2) // Fixed second-to-last
                }
            }
            0xE000..=0xFFFF => self.prg_banks.saturating_sub(1), // Fixed last
            _ => 0,
        };

        let bank = bank % self.prg_banks.max(1);
        let offset = (addr & 0x1FFF) as usize;
        bank * 8192 + offset
    }

    /// Get CHR address for a PPU address.
    fn chr_addr(&self, addr: u16) -> usize {
        let addr = addr & 0x1FFF;

        // Determine which bank register to use based on A12 inversion
        let bank = if self.chr_inversion {
            match addr {
                0x0000..=0x03FF => self.chr_bank_1k_0,
                0x0400..=0x07FF => self.chr_bank_1k_1,
                0x0800..=0x0BFF => self.chr_bank_1k_2,
                0x0C00..=0x0FFF => self.chr_bank_1k_3,
                0x1000..=0x17FF => self.chr_bank_2k_0 & 0xFE, // 2KB aligned
                0x1800..=0x1FFF => self.chr_bank_2k_1 & 0xFE, // 2KB aligned
                _ => 0,
            }
        } else {
            match addr {
                0x0000..=0x07FF => self.chr_bank_2k_0 & 0xFE, // 2KB aligned
                0x0800..=0x0FFF => self.chr_bank_2k_1 & 0xFE, // 2KB aligned
                0x1000..=0x13FF => self.chr_bank_1k_0,
                0x1400..=0x17FF => self.chr_bank_1k_1,
                0x1800..=0x1BFF => self.chr_bank_1k_2,
                0x1C00..=0x1FFF => self.chr_bank_1k_3,
                _ => 0,
            }
        };

        // Adjust bank based on whether it's 1KB or 2KB
        let (bank_size, offset_mask) = if self.chr_inversion {
            match addr {
                0x0000..=0x0FFF => (1024, 0x03FF),
                _ => (2048, 0x07FF),
            }
        } else {
            match addr {
                0x0000..=0x0FFF => (2048, 0x07FF),
                _ => (1024, 0x03FF),
            }
        };

        let bank = (bank as usize) % self.chr_banks;
        let offset = (addr & offset_mask) as usize;

        if bank_size == 2048 {
            // 2KB bank
            (bank / 2 * 2) * 1024 + offset
        } else {
            // 1KB bank
            bank * 1024 + offset
        }
    }

    /// Clock the IRQ counter (called on A12 rising edge).
    fn clock_irq(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter = self.irq_counter.saturating_sub(1);
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }
}

impl Mapper for Mmc3 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled {
                    let offset = (addr - 0x6000) as usize;
                    self.prg_ram.get(offset).copied().unwrap_or(0)
                } else {
                    0 // Open bus
                }
            }
            0x8000..=0xFFFF => {
                let offset = self.prg_addr(addr);
                self.prg_rom.get(offset).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled && !self.prg_ram_protect {
                    let offset = (addr - 0x6000) as usize;
                    if let Some(byte) = self.prg_ram.get_mut(offset) {
                        *byte = val;
                    }
                }
            }
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    // Bank select ($8000)
                    self.bank_select = val & 0x07;
                    self.prg_mode = val & 0x40 != 0;
                    self.chr_inversion = val & 0x80 != 0;
                } else {
                    // Bank data ($8001)
                    match self.bank_select {
                        0 => self.chr_bank_2k_0 = val,
                        1 => self.chr_bank_2k_1 = val,
                        2 => self.chr_bank_1k_0 = val,
                        3 => self.chr_bank_1k_1 = val,
                        4 => self.chr_bank_1k_2 = val,
                        5 => self.chr_bank_1k_3 = val,
                        6 => self.prg_bank_0 = val & 0x3F,
                        7 => self.prg_bank_1 = val & 0x3F,
                        _ => {}
                    }
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    // Mirroring ($A000)
                    self.mirroring = if val & 1 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                } else {
                    // PRG-RAM protect ($A001)
                    self.prg_ram_enabled = val & 0x80 != 0;
                    self.prg_ram_protect = val & 0x40 != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    // IRQ latch ($C000)
                    self.irq_latch = val;
                } else {
                    // IRQ reload ($C001)
                    self.irq_counter = 0;
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    // IRQ disable ($E000)
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    // IRQ enable ($E001)
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let offset = self.chr_addr(addr);
        self.chr.get(offset).copied().unwrap_or(0)
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram {
            let offset = self.chr_addr(addr);
            if let Some(byte) = self.chr.get_mut(offset) {
                *byte = val;
            }
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn scanline(&mut self) {
        // Called by PPU at end of visible scanline
        // This is a simplified version; accurate MMC3 uses A12 detection
        self.clock_irq();
    }

    fn ppu_a12_rising(&mut self) {
        // More accurate A12-based clocking
        // Filter rapid toggling (must be low for ~2 M2 cycles)
        if self.a12_filter > 0 {
            self.a12_filter -= 1;
        } else {
            self.clock_irq();
            self.a12_filter = 2;
        }
    }

    fn mapper_number(&self) -> u16 {
        4
    }

    fn mapper_name(&self) -> &'static str {
        "MMC3"
    }

    fn has_battery(&self) -> bool {
        self.has_battery
    }

    fn battery_ram(&self) -> Option<&[u8]> {
        if self.has_battery {
            Some(&self.prg_ram)
        } else {
            None
        }
    }

    fn set_battery_ram(&mut self, data: &[u8]) {
        let len = data.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&data[..len]);
    }

    fn reset(&mut self) {
        self.bank_select = 0;
        self.prg_mode = false;
        self.chr_inversion = false;
        self.chr_bank_2k_0 = 0;
        self.chr_bank_2k_1 = 2;
        self.chr_bank_1k_0 = 4;
        self.chr_bank_1k_1 = 5;
        self.chr_bank_1k_2 = 6;
        self.chr_bank_1k_3 = 7;
        self.prg_bank_0 = 0;
        self.prg_bank_1 = 1;
        self.irq_latch = 0;
        self.irq_counter = 0;
        self.irq_reload = false;
        self.irq_enabled = false;
        self.irq_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{RomFormat, RomHeader};

    fn create_test_rom(prg_banks: u8, chr_banks: u8) -> Rom {
        let prg_size = prg_banks as usize * 8192;
        let chr_size = chr_banks as usize * 1024;

        // Fill each PRG bank with its bank number
        let mut prg_rom = vec![0u8; prg_size];
        for bank in 0..prg_banks as usize {
            for i in 0..8192 {
                prg_rom[bank * 8192 + i] = bank as u8;
            }
        }

        // Fill each CHR bank with its bank number
        let mut chr_rom = vec![0u8; chr_size];
        for bank in 0..chr_banks as usize {
            for i in 0..1024 {
                chr_rom[bank * 1024 + i] = bank as u8;
            }
        }

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 4,
                prg_rom_size: (prg_banks / 2) as u16,
                chr_rom_size: (chr_banks / 8) as u16,
                prg_ram_size: 8192,
                chr_ram_size: 0,
                mirroring: Mirroring::Vertical,
                has_battery: true,
                has_trainer: false,
                tv_system: 0,
            },
            prg_rom,
            chr_rom,
            trainer: None,
        }
    }

    #[test]
    fn test_mmc3_initial_prg_banks() {
        let rom = create_test_rom(32, 32); // 256KB PRG, 32KB CHR
        let mapper = Mmc3::new(&rom);

        // $8000 = bank 0, $A000 = bank 1, $C000 = bank 30, $E000 = bank 31
        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.read_prg(0xA000), 1);
        assert_eq!(mapper.read_prg(0xC000), 30);
        assert_eq!(mapper.read_prg(0xE000), 31);
    }

    #[test]
    fn test_mmc3_prg_bank_switching() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        // Select bank register 6 (PRG bank 0)
        mapper.write_prg(0x8000, 6);
        // Write bank number 5
        mapper.write_prg(0x8001, 5);

        // $8000 should now be bank 5
        assert_eq!(mapper.read_prg(0x8000), 5);
    }

    #[test]
    fn test_mmc3_prg_mode_swap() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        // Set PRG bank 0 to 5
        mapper.write_prg(0x8000, 6);
        mapper.write_prg(0x8001, 5);

        // Verify normal mode: $8000 = bank 5
        assert_eq!(mapper.read_prg(0x8000), 5);
        assert_eq!(mapper.read_prg(0xC000), 30);

        // Switch to mode 1 (swap $8000 and $C000)
        mapper.write_prg(0x8000, 0x46); // bit 6 set = mode 1

        // Now $8000 = fixed second-to-last, $C000 = bank 5
        assert_eq!(mapper.read_prg(0x8000), 30);
        assert_eq!(mapper.read_prg(0xC000), 5);
    }

    #[test]
    fn test_mmc3_mirroring_control() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        // Initial should be vertical (from ROM header)
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        // Set horizontal
        mapper.write_prg(0xA000, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        // Set vertical
        mapper.write_prg(0xA000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn test_mmc3_irq() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        // Set IRQ latch to 3
        mapper.write_prg(0xC000, 3);
        // Reload counter
        mapper.write_prg(0xC001, 0);
        // Enable IRQ
        mapper.write_prg(0xE001, 0);

        assert!(!mapper.irq_pending());

        // Clock 3 times - should trigger on 4th
        mapper.scanline();
        assert!(!mapper.irq_pending());
        mapper.scanline();
        assert!(!mapper.irq_pending());
        mapper.scanline();
        assert!(!mapper.irq_pending());
        mapper.scanline();
        assert!(mapper.irq_pending());

        // Acknowledge
        mapper.irq_acknowledge();
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_mmc3_irq_disable() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        // Set IRQ latch to 1
        mapper.write_prg(0xC000, 1);
        mapper.write_prg(0xC001, 0);
        mapper.write_prg(0xE001, 0); // Enable

        mapper.scanline();
        mapper.scanline();
        assert!(mapper.irq_pending());

        // Disable clears pending
        mapper.write_prg(0xE000, 0);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn test_mmc3_prg_ram() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        // PRG-RAM should be enabled by default
        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        // Disable PRG-RAM
        mapper.write_prg(0xA001, 0x00); // Disable bit 7
        assert_eq!(mapper.read_prg(0x6000), 0); // Open bus

        // Re-enable with write protect
        mapper.write_prg(0xA001, 0xC0); // Enable + protect
        assert_eq!(mapper.read_prg(0x6000), 0x42); // Can still read

        // Write should be ignored
        mapper.write_prg(0x6000, 0xFF);
        assert_eq!(mapper.read_prg(0x6000), 0x42); // Unchanged
    }

    #[test]
    fn test_mmc3_battery_ram() {
        let rom = create_test_rom(32, 32);
        let mut mapper = Mmc3::new(&rom);

        assert!(mapper.has_battery());

        mapper.write_prg(0x6000, 0xAB);
        mapper.write_prg(0x6001, 0xCD);

        let save = mapper.battery_ram().unwrap();
        assert_eq!(save[0], 0xAB);
        assert_eq!(save[1], 0xCD);
    }
}
