//! NES ROM file format parsing (iNES and NES 2.0).
//!
//! This module handles loading and parsing NES ROM files in both the legacy iNES format
//! and the extended NES 2.0 format.

use crate::Mirroring;

/// Errors that can occur when parsing NES ROM files.
#[derive(Debug, thiserror::Error)]
pub enum RomError {
    /// ROM file is too small to contain a valid header.
    #[error("ROM file too small: expected at least 16 bytes, got {0}")]
    FileTooSmall(usize),

    /// Invalid iNES magic number in header.
    #[error("Invalid iNES magic number: expected [4E 45 53 1A], got {0:02X?}")]
    InvalidMagic([u8; 4]),

    /// Invalid PRG-ROM size.
    #[error("Invalid PRG-ROM size: {0}")]
    InvalidPrgSize(String),

    /// Invalid CHR-ROM size.
    #[error("Invalid CHR-ROM size: {0}")]
    InvalidChrSize(String),

    /// ROM file size doesn't match header specifications.
    #[error("ROM file size mismatch: expected {expected} bytes, got {actual} bytes")]
    SizeMismatch {
        /// Expected file size in bytes.
        expected: usize,
        /// Actual file size in bytes.
        actual: usize,
    },

    /// Unsupported ROM format variant.
    #[error("Unsupported ROM format: {0}")]
    UnsupportedFormat(String),
}

/// iNES/NES 2.0 ROM header.
///
/// Represents the 16-byte header found at the start of all iNES format ROM files.
///
/// # Format
///
/// ```text
/// Byte 0-3:   Magic number "NES" followed by MS-DOS EOF (0x4E 0x45 0x53 0x1A)
/// Byte 4:     PRG-ROM size in 16KB units (or LSB in NES 2.0)
/// Byte 5:     CHR-ROM size in 8KB units (or LSB in NES 2.0)
/// Byte 6:     Flags 6 (mirroring, battery, trainer, four-screen, mapper low nibble)
/// Byte 7:     Flags 7 (VS System, PlayChoice-10, NES 2.0 identifier, mapper high nibble)
/// Byte 8:     Flags 8 (mapper MSB and submapper in NES 2.0, or PRG-RAM size in iNES)
/// Byte 9:     Flags 9 (PRG-ROM MSB and CHR-ROM MSB in NES 2.0)
/// Byte 10:    Flags 10 (PRG-RAM and PRG-NVRAM size in NES 2.0)
/// Byte 11:    Flags 11 (CHR-RAM and CHR-NVRAM size in NES 2.0)
/// Byte 12:    Flags 12 (CPU/PPU timing in NES 2.0)
/// Byte 13:    Flags 13 (system type in NES 2.0)
/// Byte 14:    Miscellaneous ROMs count in NES 2.0
/// Byte 15:    Default expansion device in NES 2.0
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomHeader {
    /// PRG-ROM size in bytes.
    pub prg_rom_size: usize,

    /// CHR-ROM size in bytes (0 indicates CHR-RAM).
    pub chr_rom_size: usize,

    /// Mapper number (0-4095 for NES 2.0, 0-255 for iNES 1.0).
    pub mapper_number: u16,

    /// Submapper number (0-15, NES 2.0 only).
    pub submapper: u8,

    /// Nametable mirroring mode.
    pub mirroring: Mirroring,

    /// Battery-backed PRG-RAM present.
    pub has_battery: bool,

    /// 512-byte trainer present before PRG-ROM.
    pub has_trainer: bool,

    /// True if this is NES 2.0 format.
    pub nes2_format: bool,

    /// PRG-RAM size in bytes (for battery-backed save data).
    pub prg_ram_size: usize,

    /// PRG-NVRAM size in bytes (NES 2.0 only).
    pub prg_nvram_size: usize,

    /// CHR-RAM size in bytes (when `chr_rom_size` is 0).
    pub chr_ram_size: usize,

    /// CHR-NVRAM size in bytes (NES 2.0 only).
    pub chr_nvram_size: usize,
}

impl RomHeader {
    /// iNES magic number: "NES" followed by MS-DOS EOF.
    const MAGIC: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A];

    /// Parse ROM header from raw bytes.
    ///
    /// # Arguments
    ///
    /// * `data` - Raw header bytes (at least 16 bytes)
    ///
    /// # Returns
    ///
    /// Parsed ROM header or error.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Data is less than 16 bytes
    /// - Magic number is incorrect
    /// - Header contains invalid size values
    #[allow(clippy::similar_names)]
    pub fn parse(data: &[u8]) -> Result<Self, RomError> {
        if data.len() < 16 {
            return Err(RomError::FileTooSmall(data.len()));
        }

        // Verify magic number
        let magic = [data[0], data[1], data[2], data[3]];
        if magic != Self::MAGIC {
            return Err(RomError::InvalidMagic(magic));
        }

        // Check for NES 2.0 format
        let nes2_format = (data[7] & 0x0C) == 0x08;

        let (mapper_number, submapper) = if nes2_format {
            Self::parse_nes2_mapper(data)
        } else {
            (Self::parse_ines_mapper(data), 0)
        };

        let (prg_rom_size, chr_rom_size) = if nes2_format {
            Self::parse_nes2_sizes(data)?
        } else {
            Self::parse_ines_sizes(data)?
        };

        let mirroring = Self::parse_mirroring(data);
        let has_battery = (data[6] & 0x02) != 0;
        let has_trainer = (data[6] & 0x04) != 0;

        let (prg_ram_size, prg_nvram_size, chr_ram_size, chr_nvram_size) = if nes2_format {
            Self::parse_nes2_ram_sizes(data)
        } else {
            let prg_ram = if data[8] == 0 {
                8192
            } else {
                data[8] as usize * 8192
            };
            (prg_ram, 0, if chr_rom_size == 0 { 8192 } else { 0 }, 0)
        };

        Ok(Self {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
            submapper,
            mirroring,
            has_battery,
            has_trainer,
            nes2_format,
            prg_ram_size,
            prg_nvram_size,
            chr_ram_size,
            chr_nvram_size,
        })
    }

    /// Parse iNES 1.0 mapper number (8 bits).
    fn parse_ines_mapper(data: &[u8]) -> u16 {
        let low = (data[6] & 0xF0) >> 4;
        let high = data[7] & 0xF0;
        u16::from(high | low)
    }

    /// Parse NES 2.0 mapper number (12 bits) and submapper (4 bits).
    fn parse_nes2_mapper(data: &[u8]) -> (u16, u8) {
        let low = (data[6] & 0xF0) >> 4;
        let mid = data[7] & 0xF0;
        let high = data[8] & 0x0F;
        let mapper = u16::from(high) << 8 | u16::from(mid | low);
        let submapper = (data[8] & 0xF0) >> 4;
        (mapper, submapper)
    }

    /// Parse iNES 1.0 ROM sizes.
    fn parse_ines_sizes(data: &[u8]) -> Result<(usize, usize), RomError> {
        let prg_size = data[4] as usize * 16384; // 16KB units
        let chr_size = data[5] as usize * 8192; // 8KB units

        if prg_size == 0 {
            return Err(RomError::InvalidPrgSize(
                "PRG-ROM size cannot be 0".to_string(),
            ));
        }

        Ok((prg_size, chr_size))
    }

    /// Parse NES 2.0 ROM sizes with MSB support.
    #[allow(clippy::similar_names)]
    fn parse_nes2_sizes(data: &[u8]) -> Result<(usize, usize), RomError> {
        let prg_lsb = data[4] as usize;
        let chr_lsb = data[5] as usize;
        let prg_msb = (data[9] & 0x0F) as usize;
        let chr_msb = ((data[9] & 0xF0) >> 4) as usize;

        // If MSB is 0xF, use exponent-multiplier format
        let prg_size = if prg_msb == 0x0F {
            let exponent = (prg_lsb & 0xFC) >> 2;
            let multiplier = (prg_lsb & 0x03) * 2 + 1;
            multiplier * (1 << exponent)
        } else {
            (prg_msb << 8 | prg_lsb) * 16384
        };

        let chr_size = if chr_msb == 0x0F {
            let exponent = (chr_lsb & 0xFC) >> 2;
            let multiplier = (chr_lsb & 0x03) * 2 + 1;
            multiplier * (1 << exponent)
        } else {
            (chr_msb << 8 | chr_lsb) * 8192
        };

        if prg_size == 0 {
            return Err(RomError::InvalidPrgSize(
                "PRG-ROM size cannot be 0".to_string(),
            ));
        }

        Ok((prg_size, chr_size))
    }

    /// Parse mirroring mode from flags.
    fn parse_mirroring(data: &[u8]) -> Mirroring {
        if (data[6] & 0x08) != 0 {
            // Four-screen VRAM
            Mirroring::FourScreen
        } else if (data[6] & 0x01) != 0 {
            // Vertical mirroring
            Mirroring::Vertical
        } else {
            // Horizontal mirroring
            Mirroring::Horizontal
        }
    }

    /// Parse NES 2.0 RAM sizes.
    fn parse_nes2_ram_sizes(data: &[u8]) -> (usize, usize, usize, usize) {
        let prg_ram = Self::parse_ram_size(data[10] & 0x0F);
        let prg_nvram = Self::parse_ram_size((data[10] & 0xF0) >> 4);
        let chr_ram = Self::parse_ram_size(data[11] & 0x0F);
        let chr_nvram = Self::parse_ram_size((data[11] & 0xF0) >> 4);
        (prg_ram, prg_nvram, chr_ram, chr_nvram)
    }

    /// Parse RAM size from 4-bit field (NES 2.0 format).
    fn parse_ram_size(field: u8) -> usize {
        if field == 0 { 0 } else { 64 << field }
    }
}

/// Parsed NES ROM file.
#[derive(Debug, Clone)]
pub struct Rom {
    /// ROM header information.
    pub header: RomHeader,

    /// 512-byte trainer data (if present).
    pub trainer: Option<Vec<u8>>,

    /// PRG-ROM data (program code).
    pub prg_rom: Vec<u8>,

    /// CHR-ROM data (graphics), or empty if CHR-RAM.
    pub chr_rom: Vec<u8>,
}

impl Rom {
    /// Load a ROM from raw file bytes.
    ///
    /// # Arguments
    ///
    /// * `data` - Complete ROM file contents
    ///
    /// # Returns
    ///
    /// Parsed ROM or error.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Header is invalid
    /// - File size doesn't match header
    /// - Data is truncated
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::fs;
    /// use rustynes_mappers::Rom;
    ///
    /// let data = fs::read("game.nes")?;
    /// let rom = Rom::load(&data)?;
    /// println!("Mapper: {}", rom.header.mapper_number);
    /// ```
    pub fn load(data: &[u8]) -> Result<Self, RomError> {
        let header = RomHeader::parse(data)?;

        let mut offset = 16; // Header size

        // Load trainer if present
        let trainer = if header.has_trainer {
            if data.len() < offset + 512 {
                return Err(RomError::FileTooSmall(data.len()));
            }
            let trainer_data = data[offset..offset + 512].to_vec();
            offset += 512;
            Some(trainer_data)
        } else {
            None
        };

        // Load PRG-ROM
        if data.len() < offset + header.prg_rom_size {
            return Err(RomError::SizeMismatch {
                expected: offset + header.prg_rom_size,
                actual: data.len(),
            });
        }
        let prg_rom = data[offset..offset + header.prg_rom_size].to_vec();
        offset += header.prg_rom_size;

        // Load CHR-ROM (if present)
        let chr_rom = if header.chr_rom_size > 0 {
            if data.len() < offset + header.chr_rom_size {
                return Err(RomError::SizeMismatch {
                    expected: offset + header.chr_rom_size,
                    actual: data.len(),
                });
            }

            data[offset..offset + header.chr_rom_size].to_vec()
        } else {
            Vec::new()
        };

        Ok(Self {
            header,
            trainer,
            prg_rom,
            chr_rom,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_header(
        prg_size: u8,
        chr_size: u8,
        mapper: u8,
        mirroring: u8,
        battery: bool,
    ) -> Vec<u8> {
        let mut header = vec![0x4E, 0x45, 0x53, 0x1A]; // Magic
        header.push(prg_size);
        header.push(chr_size);
        header.push(((mapper & 0x0F) << 4) | mirroring | if battery { 0x02 } else { 0x00 });
        header.push(mapper & 0xF0);
        header.extend_from_slice(&[0; 8]); // Padding
        header
    }

    #[test]
    fn test_valid_ines_header() {
        let header = create_test_header(2, 1, 0, 0, false);
        let result = RomHeader::parse(&header);
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.prg_rom_size, 32768); // 2 * 16KB
        assert_eq!(parsed.chr_rom_size, 8192); // 1 * 8KB
        assert_eq!(parsed.mapper_number, 0);
        assert_eq!(parsed.mirroring, Mirroring::Horizontal);
        assert!(!parsed.has_battery);
        assert!(!parsed.nes2_format);
    }

    #[test]
    fn test_invalid_magic() {
        let mut header = create_test_header(1, 1, 0, 0, false);
        header[0] = 0x00; // Corrupt magic
        let result = RomHeader::parse(&header);
        assert!(matches!(result, Err(RomError::InvalidMagic(_))));
    }

    #[test]
    fn test_file_too_small() {
        let result = RomHeader::parse(&[0x4E, 0x45, 0x53]);
        assert!(matches!(result, Err(RomError::FileTooSmall(3))));
    }

    #[test]
    fn test_mapper_number_parsing() {
        let header = create_test_header(1, 1, 0x42, 0, false);
        let parsed = RomHeader::parse(&header).unwrap();
        assert_eq!(parsed.mapper_number, 0x42);
    }

    #[test]
    fn test_mirroring_modes() {
        // Horizontal
        let header = create_test_header(1, 1, 0, 0x00, false);
        assert_eq!(
            RomHeader::parse(&header).unwrap().mirroring,
            Mirroring::Horizontal
        );

        // Vertical
        let header = create_test_header(1, 1, 0, 0x01, false);
        assert_eq!(
            RomHeader::parse(&header).unwrap().mirroring,
            Mirroring::Vertical
        );

        // Four-screen
        let header = create_test_header(1, 1, 0, 0x08, false);
        assert_eq!(
            RomHeader::parse(&header).unwrap().mirroring,
            Mirroring::FourScreen
        );
    }

    #[test]
    fn test_battery_flag() {
        let header = create_test_header(1, 1, 0, 0, true);
        let parsed = RomHeader::parse(&header).unwrap();
        assert!(parsed.has_battery);
    }

    #[test]
    fn test_rom_loading() {
        let mut rom_data = create_test_header(1, 1, 0, 0, false);
        rom_data.extend_from_slice(&[0x42; 16384]); // PRG-ROM
        rom_data.extend_from_slice(&[0x55; 8192]); // CHR-ROM

        let rom = Rom::load(&rom_data).unwrap();
        assert_eq!(rom.header.prg_rom_size, 16384);
        assert_eq!(rom.header.chr_rom_size, 8192);
        assert_eq!(rom.prg_rom.len(), 16384);
        assert_eq!(rom.chr_rom.len(), 8192);
        assert_eq!(rom.prg_rom[0], 0x42);
        assert_eq!(rom.chr_rom[0], 0x55);
    }

    #[test]
    fn test_rom_with_trainer() {
        let mut header = create_test_header(1, 0, 0, 0, false);
        header[6] |= 0x04; // Set trainer flag

        let mut rom_data = header;
        rom_data.extend_from_slice(&[0xFF; 512]); // Trainer
        rom_data.extend_from_slice(&[0x42; 16384]); // PRG-ROM

        let rom = Rom::load(&rom_data).unwrap();
        assert!(rom.header.has_trainer);
        assert!(rom.trainer.is_some());
        assert_eq!(rom.trainer.unwrap().len(), 512);
    }

    #[test]
    fn test_rom_size_mismatch() {
        let mut rom_data = create_test_header(2, 1, 0, 0, false);
        rom_data.extend_from_slice(&[0x42; 1024]); // Too small
        let result = Rom::load(&rom_data);
        assert!(matches!(result, Err(RomError::SizeMismatch { .. })));
    }
}
