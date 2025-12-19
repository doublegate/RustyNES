//! iNES ROM format loader.
//!
//! This module provides basic iNES ROM loading functionality for testing purposes.
//! For the full emulator, a more comprehensive ROM loader will be in rustynes-core.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// iNES ROM header (16 bytes).
#[derive(Debug, Clone)]
pub struct INesHeader {
    /// PRG-ROM size in 16 KB units
    pub prg_rom_size: u8,
    /// CHR-ROM size in 8 KB units (0 means CHR-RAM)
    pub chr_rom_size: u8,
    /// Mapper number
    pub mapper: u8,
    /// Mirroring type (0 = horizontal, 1 = vertical)
    pub mirroring: u8,
    /// Has battery-backed RAM
    pub battery: bool,
    /// Has trainer
    pub trainer: bool,
    /// Four-screen VRAM
    pub four_screen: bool,
}

impl INesHeader {
    /// Parse iNES header from bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the header does not contain the iNES magic number ("NES\x1A").
    pub fn parse(header: &[u8; 16]) -> Result<Self, String> {
        // Check magic number "NES\x1A"
        if header[0] != b'N' || header[1] != b'E' || header[2] != b'S' || header[3] != 0x1A {
            return Err("Invalid iNES header magic number".to_string());
        }

        let prg_rom_size = header[4];
        let chr_rom_size = header[5];
        let flags6 = header[6];
        let flags7 = header[7];

        // Extract mapper number
        let mapper_lo = (flags6 & 0xF0) >> 4;
        let mapper_hi = flags7 & 0xF0;
        let mapper = mapper_hi | mapper_lo;

        // Extract flags
        let mirroring = flags6 & 0x01;
        let battery = (flags6 & 0x02) != 0;
        let trainer = (flags6 & 0x04) != 0;
        let four_screen = (flags6 & 0x08) != 0;

        Ok(Self {
            prg_rom_size,
            chr_rom_size,
            mapper,
            mirroring,
            battery,
            trainer,
            four_screen,
        })
    }
}

/// iNES ROM file.
#[derive(Debug, Clone)]
pub struct INesRom {
    /// ROM header
    pub header: INesHeader,
    /// PRG-ROM data (program code)
    pub prg_rom: Vec<u8>,
    /// CHR-ROM data (graphics)
    pub chr_rom: Vec<u8>,
}

impl INesRom {
    /// Load an iNES ROM file from disk.
    ///
    /// # Errors
    ///
    /// Returns an IO error if the file cannot be opened or read, or if the ROM data is invalid.
    pub fn load<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        Self::from_bytes(&buffer).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Parse iNES ROM from bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the data is too small, contains an invalid iNES header,
    /// or has invalid PRG-ROM or CHR-ROM sizes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 16 {
            return Err("ROM file too small".to_string());
        }

        // Parse header
        let mut header_bytes = [0u8; 16];
        header_bytes.copy_from_slice(&data[0..16]);
        let header = INesHeader::parse(&header_bytes)?;

        let mut offset = 16;

        // Skip trainer if present (512 bytes)
        if header.trainer {
            offset += 512;
        }

        // Read PRG-ROM
        let prg_rom_bytes = (header.prg_rom_size as usize) * 16384;
        if data.len() < offset + prg_rom_bytes {
            return Err("Invalid PRG-ROM size".to_string());
        }
        let prg_rom = data[offset..offset + prg_rom_bytes].to_vec();
        offset += prg_rom_bytes;

        // Read CHR-ROM
        let chr_rom_bytes = (header.chr_rom_size as usize) * 8192;
        let chr_rom = if chr_rom_bytes > 0 {
            if data.len() < offset + chr_rom_bytes {
                return Err("Invalid CHR-ROM size".to_string());
            }
            data[offset..offset + chr_rom_bytes].to_vec()
        } else {
            // CHR-RAM
            vec![0; 8192]
        };

        Ok(Self {
            header,
            prg_rom,
            chr_rom,
        })
    }

    /// Get PRG-ROM size in bytes.
    pub fn prg_rom_size(&self) -> usize {
        self.prg_rom.len()
    }

    /// Get CHR-ROM size in bytes.
    pub fn chr_rom_size(&self) -> usize {
        self.chr_rom.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ines_header_parse() {
        let header_bytes = [
            b'N', b'E', b'S', 0x1A, // Magic
            0x02, // 2 * 16KB PRG-ROM
            0x01, // 1 * 8KB CHR-ROM
            0x00, // Flags 6: Mapper 0, horizontal mirroring
            0x00, // Flags 7
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let header = INesHeader::parse(&header_bytes).unwrap();
        assert_eq!(header.prg_rom_size, 2);
        assert_eq!(header.chr_rom_size, 1);
        assert_eq!(header.mapper, 0);
        assert_eq!(header.mirroring, 0);
        assert!(!header.battery);
        assert!(!header.trainer);
    }

    #[test]
    fn test_ines_header_invalid_magic() {
        let header_bytes = [
            b'N', b'E', b'X', 0x1A, // Invalid magic
            0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        assert!(INesHeader::parse(&header_bytes).is_err());
    }
}
