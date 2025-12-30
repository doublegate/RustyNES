//! NES Cartridge Mapper Implementations.
//!
//! This crate provides mapper implementations for NES cartridge emulation.
//! Mappers handle memory banking for PRG-ROM, CHR-ROM/RAM, and provide
//! various hardware features like IRQ generation.
//!
//! # Supported Mappers
//!
//! | Mapper | Name | Description |
//! |--------|------|-------------|
//! | 0 | NROM | No banking, simplest mapper |
//! | 1 | MMC1 | Nintendo's first bank-switching mapper |
//! | 2 | UxROM | PRG-ROM banking only |
//! | 3 | CNROM | CHR-ROM banking only |
//! | 4 | MMC3 | Most popular, fine-grained banking + IRQ |
//! | 7 | AxROM | Single-screen mirroring, 32KB PRG banking |
//! | 11 | Color Dreams | 32KB PRG + 8KB CHR banking |
//! | 34 | BNROM | 32KB PRG banking, CHR-RAM |
//! | 66 | GxROM | 32KB PRG + 8KB CHR banking |
//! | 71 | Camerica | Codemasters games, mirroring control |
//!
//! # Example
//!
//! ```no_run
//! use rustynes_mappers::{Rom, create_mapper};
//!
//! // Load ROM from file
//! let rom_data = std::fs::read("game.nes").expect("Failed to read ROM");
//! let rom = Rom::load(&rom_data).expect("Failed to parse ROM");
//!
//! // Create appropriate mapper
//! let mut mapper = create_mapper(&rom).expect("Unsupported mapper");
//!
//! // Use mapper for memory access
//! let opcode = mapper.read_prg(0x8000);
//! let tile = mapper.read_chr(0x0000);
//! ```
//!
//! # no_std Support
//!
//! This crate supports `no_std` environments with the `alloc` feature.
//! Disable the default `std` feature for embedded use.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

pub mod mapper;
pub mod rom;

mod axrom;
mod bnrom;
mod camerica;
mod cnrom;
mod color_dreams;
mod gxrom;
mod mmc1;
mod mmc3;
mod nrom;
mod uxrom;

pub use axrom::Axrom;
pub use bnrom::Bnrom;
pub use camerica::Camerica;
pub use cnrom::Cnrom;
pub use color_dreams::ColorDreams;
pub use gxrom::Gxrom;
pub use mapper::{Mapper, Mirroring};
pub use mmc1::Mmc1;
pub use mmc3::Mmc3;
pub use nrom::Nrom;
pub use rom::{Rom, RomError, RomFormat, RomHeader};
pub use uxrom::Uxrom;

/// Create a mapper instance from ROM data.
///
/// Returns the appropriate mapper implementation based on the ROM header's
/// mapper number. Returns an error if the mapper is not supported.
///
/// # Errors
///
/// Returns `RomError::UnsupportedMapper` if the mapper number is not
/// implemented in this crate.
///
/// # Example
///
/// ```no_run
/// use rustynes_mappers::{Rom, create_mapper};
///
/// let rom_data = std::fs::read("game.nes").expect("Failed to read ROM");
/// let rom = Rom::load(&rom_data).expect("Failed to parse ROM");
/// let mapper = create_mapper(&rom).expect("Unsupported mapper");
///
/// println!("Mapper: {} ({})", mapper.mapper_name(), mapper.mapper_number());
/// ```
pub fn create_mapper(rom: &Rom) -> Result<Box<dyn Mapper>, RomError> {
    match rom.header.mapper {
        0 => Ok(Box::new(Nrom::new(rom))),
        1 => Ok(Box::new(Mmc1::new(rom))),
        2 => Ok(Box::new(Uxrom::new(rom))),
        3 => Ok(Box::new(Cnrom::new(rom))),
        4 => Ok(Box::new(Mmc3::new(rom))),
        7 => Ok(Box::new(Axrom::new(rom))),
        11 => Ok(Box::new(ColorDreams::new(rom))),
        34 => Ok(Box::new(Bnrom::new(rom))),
        66 => Ok(Box::new(Gxrom::new(rom))),
        71 => Ok(Box::new(Camerica::new(rom))),
        n => Err(RomError::UnsupportedMapper(n)),
    }
}

/// Get a list of supported mapper numbers.
#[must_use]
pub fn supported_mappers() -> &'static [u16] {
    &[0, 1, 2, 3, 4, 7, 11, 34, 66, 71]
}

/// Check if a mapper number is supported.
#[must_use]
pub fn is_mapper_supported(mapper: u16) -> bool {
    supported_mappers().contains(&mapper)
}

/// Get the name of a mapper by number.
#[must_use]
pub fn mapper_name(mapper: u16) -> Option<&'static str> {
    match mapper {
        0 => Some("NROM"),
        1 => Some("MMC1"),
        2 => Some("UxROM"),
        3 => Some("CNROM"),
        4 => Some("MMC3"),
        7 => Some("AxROM"),
        11 => Some("Color Dreams"),
        34 => Some("BNROM"),
        66 => Some("GxROM"),
        71 => Some("Camerica"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::RomFormat;

    fn create_test_rom(mapper: u16) -> Rom {
        let prg_rom: Vec<u8> = (0..32768).map(|i| (i & 0xFF) as u8).collect();
        let chr_rom: Vec<u8> = (0..8192).map(|i| (i & 0xFF) as u8).collect();

        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper,
                prg_rom_size: 2,
                chr_rom_size: 1,
                prg_ram_size: 8192,
                chr_ram_size: 0,
                mirroring: Mirroring::Vertical,
                has_battery: false,
                has_trainer: false,
                tv_system: 0,
            },
            prg_rom,
            chr_rom,
            trainer: None,
        }
    }

    #[test]
    fn test_create_mapper_nrom() {
        let rom = create_test_rom(0);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 0);
        assert_eq!(mapper.mapper_name(), "NROM");
    }

    #[test]
    fn test_create_mapper_mmc1() {
        let rom = create_test_rom(1);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 1);
        assert_eq!(mapper.mapper_name(), "MMC1");
    }

    #[test]
    fn test_create_mapper_uxrom() {
        let rom = create_test_rom(2);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 2);
        assert_eq!(mapper.mapper_name(), "UxROM");
    }

    #[test]
    fn test_create_mapper_cnrom() {
        let rom = create_test_rom(3);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 3);
        assert_eq!(mapper.mapper_name(), "CNROM");
    }

    #[test]
    fn test_create_mapper_mmc3() {
        let rom = create_test_rom(4);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 4);
        assert_eq!(mapper.mapper_name(), "MMC3");
    }

    #[test]
    fn test_create_mapper_axrom() {
        let rom = create_test_rom(7);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 7);
        assert_eq!(mapper.mapper_name(), "AxROM");
    }

    #[test]
    fn test_create_mapper_color_dreams() {
        let rom = create_test_rom(11);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 11);
        assert_eq!(mapper.mapper_name(), "Color Dreams");
    }

    #[test]
    fn test_create_mapper_bnrom() {
        let rom = create_test_rom(34);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 34);
        assert_eq!(mapper.mapper_name(), "BNROM");
    }

    #[test]
    fn test_create_mapper_gxrom() {
        let rom = create_test_rom(66);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 66);
        assert_eq!(mapper.mapper_name(), "GxROM");
    }

    #[test]
    fn test_create_mapper_camerica() {
        let rom = create_test_rom(71);
        let mapper = create_mapper(&rom).unwrap();
        assert_eq!(mapper.mapper_number(), 71);
        assert_eq!(mapper.mapper_name(), "Camerica");
    }

    #[test]
    fn test_create_mapper_unsupported() {
        let rom = create_test_rom(100);
        let result = create_mapper(&rom);
        assert!(matches!(result, Err(RomError::UnsupportedMapper(100))));
    }

    #[test]
    fn test_supported_mappers() {
        let mappers = supported_mappers();
        assert_eq!(mappers, &[0, 1, 2, 3, 4, 7, 11, 34, 66, 71]);
    }

    #[test]
    fn test_is_mapper_supported() {
        assert!(is_mapper_supported(0));
        assert!(is_mapper_supported(4));
        assert!(is_mapper_supported(7));
        assert!(is_mapper_supported(11));
        assert!(is_mapper_supported(34));
        assert!(is_mapper_supported(66));
        assert!(is_mapper_supported(71));
        assert!(!is_mapper_supported(100));
    }

    #[test]
    fn test_mapper_name() {
        assert_eq!(mapper_name(0), Some("NROM"));
        assert_eq!(mapper_name(1), Some("MMC1"));
        assert_eq!(mapper_name(4), Some("MMC3"));
        assert_eq!(mapper_name(7), Some("AxROM"));
        assert_eq!(mapper_name(11), Some("Color Dreams"));
        assert_eq!(mapper_name(34), Some("BNROM"));
        assert_eq!(mapper_name(66), Some("GxROM"));
        assert_eq!(mapper_name(71), Some("Camerica"));
        assert_eq!(mapper_name(100), None);
    }

    #[test]
    fn test_mapper_trait_read_write() {
        let rom = create_test_rom(0);
        let mut mapper = create_mapper(&rom).unwrap();

        // Read PRG-ROM
        let val = mapper.read_prg(0x8000);
        assert_eq!(val, 0); // First byte of PRG-ROM

        // Write has no effect on NROM
        mapper.write_prg(0x8000, 0xFF);
        assert_eq!(mapper.read_prg(0x8000), 0);
    }
}
