//! NES cartridge mapper implementations for `RustyNES` emulator.
//!
//! This crate provides implementations of NES mappers (cartridge hardware) that enable
//! bank switching, extended ROM sizes, and special features like IRQ generation.
//!
//! # Overview
//!
//! The NES has a limited address space:
//! - CPU: 64KB address space, but only 32KB available for cartridge ROM ($8000-$FFFF)
//! - PPU: 16KB address space, with 8KB for pattern tables ($0000-$1FFF)
//!
//! Mappers solve this limitation by providing bank switching - dynamically mapping
//! different ROM banks into the same address space.
//!
//! # Supported Mappers
//!
//! This crate currently implements 5 essential mappers covering 77.7% of licensed NES games:
//!
//! - **Mapper 0 (NROM)**: Simple passthrough, no banking (9.5% of games)
//! - **Mapper 1 (MMC1)**: 5-bit shift register, flexible banking (27.9% of games)
//! - **Mapper 2 (`UxROM`)**: Switchable + fixed PRG banks (10.6% of games)
//! - **Mapper 3 (CNROM)**: Simple CHR banking (6.3% of games)
//! - **Mapper 4 (MMC3)**: Complex banking with scanline IRQ (23.4% of games)
//!
//! # Usage
//!
//! ```ignore
//! use rustynes_mappers::{Rom, create_mapper};
//! use std::fs;
//!
//! // Load ROM file
//! let rom_data = fs::read("game.nes")?;
//! let rom = Rom::load(&rom_data)?;
//!
//! // Create appropriate mapper
//! let mut mapper = create_mapper(&rom)?;
//!
//! // Use mapper for CPU/PPU memory access
//! let byte = mapper.read_prg(0x8000);
//! mapper.write_prg(0x8000, 0x42);
//!
//! let chr_byte = mapper.read_chr(0x0000);
//! mapper.write_chr(0x0000, 0x55);
//!
//! // Check mirroring
//! let mirroring = mapper.mirroring();
//!
//! // Check for IRQ (MMC3, etc.)
//! if mapper.irq_pending() {
//!     // Trigger CPU interrupt
//!     mapper.clear_irq();
//! }
//! ```
//!
//! # Architecture
//!
//! - [`Mapper`]: Core trait defining the mapper interface
//! - [`Mirroring`]: Nametable mirroring modes
//! - [`Rom`]: ROM file parsing and loading
//! - [`RomHeader`]: iNES/NES 2.0 header information
//!
//! # Safety
//!
//! This crate uses **zero unsafe code**. All mapper implementations are safe Rust.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod mapper;
mod mirroring;
mod rom;

// Mapper implementations
mod cnrom;
mod mmc1;
mod mmc3;
mod nrom;
mod uxrom;

// Re-exports
pub use mapper::Mapper;
pub use mirroring::Mirroring;
pub use rom::{Rom, RomError, RomHeader};

// Mapper implementations
pub use cnrom::Cnrom;
pub use mmc1::Mmc1;
pub use mmc3::Mmc3;
pub use nrom::Nrom;
pub use uxrom::Uxrom;

/// Mapper creation error.
#[derive(Debug, thiserror::Error)]
pub enum MapperError {
    /// Unsupported mapper number.
    #[error("Unsupported mapper {mapper} (submapper {submapper})")]
    UnsupportedMapper {
        /// Mapper number from ROM header.
        mapper: u16,
        /// Submapper number from ROM header (NES 2.0 only).
        submapper: u8,
    },

    /// Invalid ROM configuration for mapper.
    #[error("Invalid ROM configuration for mapper {mapper}: {reason}")]
    InvalidConfiguration {
        /// Mapper number.
        mapper: u16,
        /// Reason for rejection.
        reason: String,
    },
}

/// Create a mapper instance from a loaded ROM.
///
/// This factory function selects and instantiates the appropriate mapper based on
/// the mapper number in the ROM header.
///
/// # Arguments
///
/// * `rom` - Loaded ROM file
///
/// # Returns
///
/// Boxed mapper instance or error if mapper is unsupported.
///
/// # Errors
///
/// Returns `MapperError::UnsupportedMapper` if the mapper number is not implemented.
///
/// # Examples
///
/// ```ignore
/// use rustynes_mappers::{Rom, create_mapper};
///
/// let rom = Rom::load(&rom_data)?;
/// let mapper = create_mapper(&rom)?;
///
/// println!("Created mapper {}", mapper.mapper_number());
/// ```
pub fn create_mapper(rom: &Rom) -> Result<Box<dyn Mapper>, MapperError> {
    let mapper_num = rom.header.mapper_number;
    let submapper = rom.header.submapper;

    match mapper_num {
        0 => Ok(Box::new(Nrom::new(rom))),
        1 => Ok(Box::new(Mmc1::new(rom))),
        2 => Ok(Box::new(Uxrom::new(rom))),
        3 => Ok(Box::new(Cnrom::new(rom))),
        4 => Ok(Box::new(Mmc3::new(rom))),
        _ => Err(MapperError::UnsupportedMapper {
            mapper: mapper_num,
            submapper,
        }),
    }
}

/// Check if a mapper number is supported.
///
/// # Arguments
///
/// * `mapper` - Mapper number to check
///
/// # Returns
///
/// `true` if the mapper is implemented, `false` otherwise.
///
/// # Examples
///
/// ```
/// use rustynes_mappers::is_mapper_supported;
///
/// assert!(is_mapper_supported(0));  // NROM
/// assert!(is_mapper_supported(1));  // MMC1
/// assert!(is_mapper_supported(4));  // MMC3
/// assert!(!is_mapper_supported(5)); // MMC5 (not implemented)
/// ```
#[must_use]
pub fn is_mapper_supported(mapper: u16) -> bool {
    matches!(mapper, 0..=4)
}

/// Get a human-readable name for a mapper number.
///
/// # Arguments
///
/// * `mapper` - Mapper number
///
/// # Returns
///
/// Mapper name or "Unknown" if not recognized.
///
/// # Examples
///
/// ```
/// use rustynes_mappers::mapper_name;
///
/// assert_eq!(mapper_name(0), "NROM");
/// assert_eq!(mapper_name(1), "MMC1 (SxROM)");
/// assert_eq!(mapper_name(4), "MMC3 (TxROM)");
/// assert_eq!(mapper_name(999), "Unknown");
/// ```
#[must_use]
pub fn mapper_name(mapper: u16) -> &'static str {
    match mapper {
        0 => "NROM",
        1 => "MMC1 (SxROM)",
        2 => "UxROM",
        3 => "CNROM",
        4 => "MMC3 (TxROM)",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_mapper_supported() {
        assert!(is_mapper_supported(0));
        assert!(is_mapper_supported(1));
        assert!(is_mapper_supported(2));
        assert!(is_mapper_supported(3));
        assert!(is_mapper_supported(4));
        assert!(!is_mapper_supported(5));
        assert!(!is_mapper_supported(999));
    }

    #[test]
    fn test_mapper_name() {
        assert_eq!(mapper_name(0), "NROM");
        assert_eq!(mapper_name(1), "MMC1 (SxROM)");
        assert_eq!(mapper_name(2), "UxROM");
        assert_eq!(mapper_name(3), "CNROM");
        assert_eq!(mapper_name(4), "MMC3 (TxROM)");
        assert_eq!(mapper_name(999), "Unknown");
    }
}
