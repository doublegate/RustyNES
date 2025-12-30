//! MMC1 Mapper (Mapper 1).
//!
//! One of the most common NES mappers, used by games like The Legend of Zelda,
//! Metroid, and Final Fantasy. Features:
//!
//! - Serial shift register for configuration writes
//! - PRG-ROM banking: 16KB or 32KB modes
//! - CHR-ROM/RAM banking: 4KB or 8KB modes
//! - Mirroring control (H/V/single-screen)
//! - 8KB PRG-RAM at $6000-$7FFF (often battery-backed)
//!
//! Register layout (written via serial shift register):
//! - $8000-$9FFF: Control register
//! - $A000-$BFFF: CHR bank 0
//! - $C000-$DFFF: CHR bank 1
//! - $E000-$FFFF: PRG bank

use crate::mapper::{Mapper, Mirroring};
use crate::rom::Rom;

#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, vec::Vec};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// PRG-ROM banking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
enum PrgMode {
    /// Switch 32KB at $8000, ignore low bit of bank number.
    Switch32K,
    /// Fix first bank at $8000, switch 16KB at $C000.
    FixFirst,
    /// Fix last bank at $C000, switch 16KB at $8000.
    #[default]
    FixLast,
}

/// CHR banking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
enum ChrMode {
    /// Switch 8KB at a time.
    #[default]
    Switch8K,
    /// Switch two separate 4KB banks.
    Switch4K,
}

/// MMC1 mapper implementation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Mmc1 {
    /// PRG-ROM data.
    prg_rom: Vec<u8>,
    /// CHR-ROM/RAM data.
    chr: Vec<u8>,
    /// PRG-RAM data (8KB).
    prg_ram: Vec<u8>,
    /// Whether CHR is RAM (writable).
    chr_is_ram: bool,
    /// Number of PRG-ROM banks (16KB each).
    prg_banks: usize,

    // Shift register
    /// Shift register value.
    shift_reg: u8,
    /// Number of bits written to shift register.
    shift_count: u8,

    // Control register ($8000-$9FFF)
    /// Nametable mirroring mode.
    mirroring: Mirroring,
    /// PRG banking mode.
    prg_mode: PrgMode,
    /// CHR banking mode.
    chr_mode: ChrMode,

    // Bank registers
    /// CHR bank 0 (or 8KB bank in 8K mode).
    chr_bank_0: u8,
    /// CHR bank 1 (4K mode only).
    chr_bank_1: u8,
    /// PRG bank.
    prg_bank: u8,
    /// PRG-RAM enable (active low on bit 4 of PRG bank register).
    prg_ram_enabled: bool,

    /// Has battery-backed RAM.
    has_battery: bool,
}

impl Mmc1 {
    /// Create a new MMC1 mapper from ROM data.
    #[must_use]
    pub fn new(rom: &Rom) -> Self {
        let prg_banks = rom.prg_rom.len() / 16384;
        let chr_is_ram = rom.chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 8192]
        } else {
            rom.chr_rom.clone()
        };

        Self {
            prg_rom: rom.prg_rom.clone(),
            chr,
            prg_ram: vec![0u8; 8192],
            chr_is_ram,
            prg_banks,
            shift_reg: 0,
            shift_count: 0,
            mirroring: rom.header.mirroring,
            prg_mode: PrgMode::FixLast,
            chr_mode: ChrMode::Switch8K,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            prg_ram_enabled: true,
            has_battery: rom.header.has_battery,
        }
    }

    /// Write to the shift register.
    fn write_shift(&mut self, addr: u16, val: u8) {
        // Bit 7 set = reset shift register
        if val & 0x80 != 0 {
            self.shift_reg = 0;
            self.shift_count = 0;
            // Reset also sets PRG mode to 3 (fix last bank)
            self.prg_mode = PrgMode::FixLast;
            return;
        }

        // Shift in bit 0 of value
        self.shift_reg |= (val & 1) << self.shift_count;
        self.shift_count += 1;

        // After 5 writes, commit to the appropriate register
        if self.shift_count == 5 {
            let register = (addr >> 13) & 0x03;
            match register {
                0 => self.write_control(self.shift_reg),
                1 => self.chr_bank_0 = self.shift_reg,
                2 => self.chr_bank_1 = self.shift_reg,
                3 => self.write_prg_bank(self.shift_reg),
                _ => unreachable!(),
            }
            self.shift_reg = 0;
            self.shift_count = 0;
        }
    }

    /// Write control register.
    fn write_control(&mut self, val: u8) {
        // Bits 0-1: Mirroring
        self.mirroring = match val & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };

        // Bits 2-3: PRG mode
        self.prg_mode = match (val >> 2) & 0x03 {
            0 | 1 => PrgMode::Switch32K,
            2 => PrgMode::FixFirst,
            3 => PrgMode::FixLast,
            _ => unreachable!(),
        };

        // Bit 4: CHR mode
        self.chr_mode = if val & 0x10 != 0 {
            ChrMode::Switch4K
        } else {
            ChrMode::Switch8K
        };
    }

    /// Write PRG bank register.
    fn write_prg_bank(&mut self, val: u8) {
        self.prg_bank = val & 0x0F;
        self.prg_ram_enabled = val & 0x10 == 0;
    }

    /// Get the PRG-ROM address for a CPU address.
    fn prg_addr(&self, addr: u16) -> usize {
        let bank = match self.prg_mode {
            PrgMode::Switch32K => {
                // 32KB mode: ignore low bit of bank number
                let base = (self.prg_bank & 0x0E) as usize;
                if addr < 0xC000 { base } else { base + 1 }
            }
            PrgMode::FixFirst => {
                if addr < 0xC000 {
                    0 // Fixed first bank
                } else {
                    (self.prg_bank & 0x0F) as usize
                }
            }
            PrgMode::FixLast => {
                if addr < 0xC000 {
                    (self.prg_bank & 0x0F) as usize
                } else {
                    self.prg_banks.saturating_sub(1) // Fixed last bank
                }
            }
        };

        let bank = bank % self.prg_banks.max(1);
        let offset = (addr & 0x3FFF) as usize;
        bank * 16384 + offset
    }

    /// Get the CHR address for a PPU address.
    fn chr_addr(&self, addr: u16) -> usize {
        let chr_banks = (self.chr.len() / 4096).max(1);

        match self.chr_mode {
            ChrMode::Switch8K => {
                // 8KB mode: use chr_bank_0, ignore low bit
                let bank = (self.chr_bank_0 & 0x1E) as usize;
                let offset = (addr & 0x1FFF) as usize;
                (bank * 4096 + offset) % self.chr.len().max(1)
            }
            ChrMode::Switch4K => {
                let (bank, offset) = if addr < 0x1000 {
                    (self.chr_bank_0 as usize, (addr & 0x0FFF) as usize)
                } else {
                    (self.chr_bank_1 as usize, (addr & 0x0FFF) as usize)
                };
                let bank = bank % chr_banks;
                bank * 4096 + offset
            }
        }
    }
}

impl Mapper for Mmc1 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled {
                    let offset = (addr - 0x6000) as usize;
                    self.prg_ram.get(offset).copied().unwrap_or(0)
                } else {
                    0 // Open bus when disabled
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
                if self.prg_ram_enabled {
                    let offset = (addr - 0x6000) as usize;
                    if let Some(byte) = self.prg_ram.get_mut(offset) {
                        *byte = val;
                    }
                }
            }
            0x8000..=0xFFFF => {
                self.write_shift(addr, val);
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

    fn mapper_number(&self) -> u16 {
        1
    }

    fn mapper_name(&self) -> &'static str {
        "MMC1"
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
        self.shift_reg = 0;
        self.shift_count = 0;
        self.prg_mode = PrgMode::FixLast;
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
        let chr_rom: Vec<u8> = (0..chr_size).map(|i| ((i + 128) & 0xFF) as u8).collect();

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 1,
                prg_rom_size: prg_banks as u16,
                chr_rom_size: chr_banks as u16,
                prg_ram_size: 8192,
                chr_ram_size: if chr_banks == 0 { 8192 } else { 0 },
                mirroring: Mirroring::Horizontal,
                has_battery: true,
                has_trainer: false,
                tv_system: 0,
            },
            prg_rom,
            chr_rom,
            trainer: None,
        }
    }

    fn write_serial(mapper: &mut Mmc1, addr: u16, val: u8) {
        // Write 5 bits serially
        for i in 0..5 {
            mapper.write_prg(addr, (val >> i) & 1);
        }
    }

    #[test]
    fn test_mmc1_shift_reset() {
        let rom = create_test_rom(8, 4);
        let mut mapper = Mmc1::new(&rom);

        // Write some bits
        mapper.write_prg(0x8000, 0x00);
        mapper.write_prg(0x8000, 0x01);
        assert_eq!(mapper.shift_count, 2);

        // Reset with bit 7
        mapper.write_prg(0x8000, 0x80);
        assert_eq!(mapper.shift_count, 0);
        assert_eq!(mapper.shift_reg, 0);
    }

    #[test]
    fn test_mmc1_prg_banking() {
        let rom = create_test_rom(8, 4); // 128KB PRG
        let mut mapper = Mmc1::new(&rom);

        // Set PRG mode to fix last bank (default)
        write_serial(&mut mapper, 0x8000, 0x0C); // Control: fix last

        // Switch to bank 2 at $8000-$BFFF
        write_serial(&mut mapper, 0xE000, 0x02);

        // Read from $8000 should be from bank 2
        let val = mapper.read_prg(0x8000);
        assert_eq!(val, 0x00); // Bank 2, offset 0

        // Read from $C000 should be from last bank (7)
        let val = mapper.read_prg(0xC000);
        assert_eq!(val, 0x00); // Bank 7, offset 0
    }

    #[test]
    fn test_mmc1_mirroring_control() {
        let rom = create_test_rom(8, 4);
        let mut mapper = Mmc1::new(&rom);

        // Set vertical mirroring
        write_serial(&mut mapper, 0x8000, 0x02);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        // Set horizontal mirroring
        write_serial(&mut mapper, 0x8000, 0x03);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        // Set single screen lower
        write_serial(&mut mapper, 0x8000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn test_mmc1_prg_ram() {
        let rom = create_test_rom(8, 4);
        let mut mapper = Mmc1::new(&rom);

        // PRG-RAM should be enabled by default
        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        // Disable PRG-RAM
        write_serial(&mut mapper, 0xE000, 0x10);
        assert_eq!(mapper.read_prg(0x6000), 0); // Disabled, returns 0
    }

    #[test]
    fn test_mmc1_battery_ram() {
        let rom = create_test_rom(8, 4);
        let mut mapper = Mmc1::new(&rom);

        assert!(mapper.has_battery());

        mapper.write_prg(0x6000, 0xAB);
        mapper.write_prg(0x6001, 0xCD);

        let save = mapper.battery_ram().unwrap();
        assert_eq!(save[0], 0xAB);
        assert_eq!(save[1], 0xCD);

        // Load save
        let mut mapper2 = Mmc1::new(&rom);
        mapper2.set_battery_ram(&[0x12, 0x34]);
        assert_eq!(mapper2.read_prg(0x6000), 0x12);
        assert_eq!(mapper2.read_prg(0x6001), 0x34);
    }

    #[test]
    fn test_mmc1_chr_banking() {
        let rom = create_test_rom(8, 4); // 32KB CHR
        let mut mapper = Mmc1::new(&rom);

        // Set 4KB CHR mode
        write_serial(&mut mapper, 0x8000, 0x10);

        // Set CHR bank 0 to bank 2
        write_serial(&mut mapper, 0xA000, 0x02);

        // Set CHR bank 1 to bank 5
        write_serial(&mut mapper, 0xC000, 0x05);

        // Verify CHR mode is set
        assert_eq!(mapper.chr_mode, ChrMode::Switch4K);
    }
}
