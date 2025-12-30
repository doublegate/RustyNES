//! ROM Loading and Parsing.
//!
//! This module handles loading NES ROM files in iNES and NES 2.0 formats.
//! It parses the header and extracts PRG-ROM, CHR-ROM, and mapper information.

use crate::mapper::Mirroring;

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// ROM format type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RomFormat {
    /// Original iNES format.
    INes,
    /// NES 2.0 extended format.
    Nes20,
}

/// Parsed ROM header information.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RomHeader {
    /// ROM format.
    pub format: RomFormat,
    /// Mapper number.
    pub mapper: u16,
    /// PRG-ROM size in 16KB units.
    pub prg_rom_size: u16,
    /// CHR-ROM size in 8KB units (0 = CHR-RAM).
    pub chr_rom_size: u16,
    /// PRG-RAM size in bytes.
    pub prg_ram_size: u32,
    /// CHR-RAM size in bytes.
    pub chr_ram_size: u32,
    /// Nametable mirroring.
    pub mirroring: Mirroring,
    /// Has battery-backed PRG-RAM.
    pub has_battery: bool,
    /// Has trainer (512 bytes before PRG-ROM).
    pub has_trainer: bool,
    /// TV system (0 = NTSC, 1 = PAL).
    pub tv_system: u8,
}

/// Loaded ROM data.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Rom {
    /// Header information.
    pub header: RomHeader,
    /// PRG-ROM data.
    pub prg_rom: Vec<u8>,
    /// CHR-ROM data (empty if CHR-RAM).
    pub chr_rom: Vec<u8>,
    /// Trainer data (if present).
    pub trainer: Option<Vec<u8>>,
}

/// ROM loading error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RomError {
    /// File is too small.
    FileTooSmall,
    /// Invalid header magic.
    InvalidMagic,
    /// Unsupported mapper.
    UnsupportedMapper(u16),
    /// Invalid ROM size.
    InvalidSize,
    /// Invalid header values.
    InvalidHeader(String),
}

impl core::fmt::Display for RomError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FileTooSmall => write!(f, "File is too small to be a valid NES ROM"),
            Self::InvalidMagic => write!(f, "Invalid NES ROM header (missing NES magic)"),
            Self::UnsupportedMapper(m) => write!(f, "Unsupported mapper: {m}"),
            Self::InvalidSize => write!(f, "Invalid ROM size"),
            Self::InvalidHeader(msg) => write!(f, "Invalid header: {msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for RomError {}

impl Rom {
    /// Load a ROM from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the ROM data is invalid or uses an unsupported format.
    pub fn load(data: &[u8]) -> Result<Self, RomError> {
        // Minimum size: 16-byte header + at least some PRG-ROM
        if data.len() < 16 {
            return Err(RomError::FileTooSmall);
        }

        // Check magic bytes "NES\x1A"
        if &data[0..4] != b"NES\x1a" {
            return Err(RomError::InvalidMagic);
        }

        // Determine format
        let format = if (data[7] & 0x0C) == 0x08 {
            RomFormat::Nes20
        } else {
            RomFormat::INes
        };

        let header = Self::parse_header(data, format)?;

        // Calculate offsets
        let trainer_offset = 16;
        let trainer_size = if header.has_trainer { 512 } else { 0 };
        let prg_offset = trainer_offset + trainer_size;
        let prg_size = usize::from(header.prg_rom_size) * 16 * 1024;
        let chr_offset = prg_offset + prg_size;
        let chr_size = usize::from(header.chr_rom_size) * 8 * 1024;

        // Validate size
        let expected_size = chr_offset + chr_size;
        if data.len() < expected_size {
            return Err(RomError::InvalidSize);
        }

        // Extract data
        let trainer = if header.has_trainer {
            Some(data[trainer_offset..prg_offset].to_vec())
        } else {
            None
        };

        let prg_rom = data[prg_offset..prg_offset + prg_size].to_vec();
        let chr_rom = if chr_size > 0 {
            data[chr_offset..chr_offset + chr_size].to_vec()
        } else {
            Vec::new()
        };

        Ok(Self {
            header,
            prg_rom,
            chr_rom,
            trainer,
        })
    }

    /// Parse the ROM header.
    #[allow(clippy::similar_names)] // ROM/RAM size variables are naturally similar
    fn parse_header(data: &[u8], format: RomFormat) -> Result<RomHeader, RomError> {
        let prg_rom_size;
        let chr_rom_size;
        let mapper;
        let prg_ram_size;
        let chr_ram_size;

        match format {
            RomFormat::INes => {
                prg_rom_size = u16::from(data[4]);
                chr_rom_size = u16::from(data[5]);
                mapper = u16::from((data[6] >> 4) | (data[7] & 0xF0));
                prg_ram_size = if data[8] == 0 {
                    8192
                } else {
                    u32::from(data[8]) * 8192
                };
                chr_ram_size = if chr_rom_size == 0 { 8192 } else { 0 };
            }
            RomFormat::Nes20 => {
                // Extended PRG-ROM size
                let prg_lsb = u16::from(data[4]);
                let prg_msb = u16::from(data[9] & 0x0F);
                prg_rom_size = prg_lsb | (prg_msb << 8);

                // Extended CHR-ROM size
                let chr_lsb = u16::from(data[5]);
                let chr_msb = u16::from((data[9] >> 4) & 0x0F);
                chr_rom_size = chr_lsb | (chr_msb << 8);

                // Extended mapper number
                mapper =
                    u16::from((data[6] >> 4) | (data[7] & 0xF0)) | (u16::from(data[8] & 0x0F) << 8);

                // PRG-RAM size (NES 2.0 uses shift count)
                let prg_ram_shift = data[10] & 0x0F;
                prg_ram_size = if prg_ram_shift == 0 {
                    0
                } else {
                    64 << prg_ram_shift
                };

                // CHR-RAM size (NES 2.0 uses shift count)
                let chr_ram_shift = data[11] & 0x0F;
                chr_ram_size = if chr_ram_shift == 0 {
                    if chr_rom_size == 0 { 8192 } else { 0 }
                } else {
                    64 << chr_ram_shift
                };
            }
        }

        // Mirroring
        let mirroring = if data[6] & 0x08 != 0 {
            Mirroring::FourScreen
        } else if data[6] & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_battery = data[6] & 0x02 != 0;
        let has_trainer = data[6] & 0x04 != 0;
        let tv_system = data[9] & 0x01;

        // Validate PRG-ROM size
        if prg_rom_size == 0 {
            return Err(RomError::InvalidHeader("PRG-ROM size is 0".into()));
        }

        Ok(RomHeader {
            format,
            mapper,
            prg_rom_size,
            chr_rom_size,
            prg_ram_size,
            chr_ram_size,
            mirroring,
            has_battery,
            has_trainer,
            tv_system,
        })
    }

    /// Check if the ROM uses CHR-RAM instead of CHR-ROM.
    #[must_use]
    pub fn has_chr_ram(&self) -> bool {
        self.chr_rom.is_empty()
    }

    /// Get the PRG-ROM size in bytes.
    #[must_use]
    pub fn prg_rom_size(&self) -> usize {
        self.prg_rom.len()
    }

    /// Get the CHR-ROM size in bytes.
    #[must_use]
    pub fn chr_rom_size(&self) -> usize {
        self.chr_rom.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_rom(prg_banks: u8, chr_banks: u8, mapper: u8, flags6: u8) -> Vec<u8> {
        let prg_size = usize::from(prg_banks) * 16 * 1024;
        let chr_size = usize::from(chr_banks) * 8 * 1024;
        let mut data = vec![0u8; 16 + prg_size + chr_size];

        // Header
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = prg_banks;
        data[5] = chr_banks;
        data[6] = (mapper << 4) | flags6;
        data[7] = mapper & 0xF0;

        // Fill PRG-ROM with test pattern
        for (i, byte) in data[16..16 + prg_size].iter_mut().enumerate() {
            *byte = (i & 0xFF) as u8;
        }

        data
    }

    #[test]
    fn test_load_valid_rom() {
        let data = create_test_rom(1, 1, 0, 0);
        let rom = Rom::load(&data).unwrap();

        assert_eq!(rom.header.format, RomFormat::INes);
        assert_eq!(rom.header.mapper, 0);
        assert_eq!(rom.header.prg_rom_size, 1);
        assert_eq!(rom.header.chr_rom_size, 1);
        assert_eq!(rom.prg_rom.len(), 16384);
        assert_eq!(rom.chr_rom.len(), 8192);
    }

    #[test]
    fn test_load_vertical_mirroring() {
        let data = create_test_rom(1, 1, 0, 0x01);
        let rom = Rom::load(&data).unwrap();
        assert_eq!(rom.header.mirroring, Mirroring::Vertical);
    }

    #[test]
    fn test_load_horizontal_mirroring() {
        let data = create_test_rom(1, 1, 0, 0x00);
        let rom = Rom::load(&data).unwrap();
        assert_eq!(rom.header.mirroring, Mirroring::Horizontal);
    }

    #[test]
    fn test_load_battery() {
        let data = create_test_rom(1, 1, 0, 0x02);
        let rom = Rom::load(&data).unwrap();
        assert!(rom.header.has_battery);
    }

    #[test]
    fn test_load_chr_ram() {
        let data = create_test_rom(1, 0, 0, 0);
        let rom = Rom::load(&data).unwrap();
        assert!(rom.has_chr_ram());
        assert_eq!(rom.header.chr_ram_size, 8192);
    }

    #[test]
    fn test_invalid_magic() {
        let data = vec![0u8; 32];
        let result = Rom::load(&data);
        assert_eq!(result.unwrap_err(), RomError::InvalidMagic);
    }

    #[test]
    fn test_file_too_small() {
        let data = vec![0u8; 10];
        let result = Rom::load(&data);
        assert_eq!(result.unwrap_err(), RomError::FileTooSmall);
    }

    #[test]
    fn test_mapper_number() {
        // Test mapper 1 (MMC1)
        let data = create_test_rom(2, 1, 1, 0);
        let rom = Rom::load(&data).unwrap();
        assert_eq!(rom.header.mapper, 1);

        // Test mapper 4 (MMC3)
        let data = create_test_rom(2, 1, 4, 0);
        let rom = Rom::load(&data).unwrap();
        assert_eq!(rom.header.mapper, 4);
    }
}
