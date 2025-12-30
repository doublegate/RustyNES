//! Mapper Trait Definition.
//!
//! This module defines the core `Mapper` trait that all NES cartridge mappers
//! must implement. Mappers handle memory banking for PRG-ROM, CHR-ROM/RAM,
//! and provide mirroring control.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Nametable mirroring mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Mirroring {
    /// Horizontal mirroring (vertical arrangement).
    #[default]
    Horizontal,
    /// Vertical mirroring (horizontal arrangement).
    Vertical,
    /// Single-screen, lower bank.
    SingleScreenLower,
    /// Single-screen, upper bank.
    SingleScreenUpper,
    /// Four-screen (uses extra VRAM).
    FourScreen,
}

/// Mapper trait.
///
/// All NES cartridge mappers must implement this trait. The mapper handles:
/// - PRG-ROM/RAM memory access (CPU $8000-$FFFF, optionally $6000-$7FFF)
/// - CHR-ROM/RAM memory access (PPU $0000-$1FFF)
/// - Nametable mirroring control
/// - Optional IRQ generation
/// - Optional scanline counting
pub trait Mapper: Send + Sync {
    /// Read a byte from PRG memory (CPU address space).
    ///
    /// Address range: $6000-$FFFF
    /// - $6000-$7FFF: PRG-RAM (battery-backed or work RAM)
    /// - $8000-$FFFF: PRG-ROM (banked)
    fn read_prg(&self, addr: u16) -> u8;

    /// Write a byte to PRG memory (CPU address space).
    ///
    /// Address range: $6000-$FFFF
    /// - $6000-$7FFF: PRG-RAM writes (if present)
    /// - $8000-$FFFF: Mapper register writes
    fn write_prg(&mut self, addr: u16, val: u8);

    /// Read a byte from CHR memory (PPU address space).
    ///
    /// Address range: $0000-$1FFF
    fn read_chr(&self, addr: u16) -> u8;

    /// Write a byte to CHR memory (PPU address space).
    ///
    /// Only works if the cartridge has CHR-RAM instead of CHR-ROM.
    fn write_chr(&mut self, addr: u16, val: u8);

    /// Get the current nametable mirroring mode.
    fn mirroring(&self) -> Mirroring;

    /// Check if the mapper has a pending IRQ.
    fn irq_pending(&self) -> bool {
        false
    }

    /// Acknowledge/clear the IRQ.
    fn irq_acknowledge(&mut self) {}

    /// Clock the mapper (called every CPU cycle).
    ///
    /// Some mappers (like MMC3) count CPU cycles for IRQ timing.
    fn clock(&mut self, _cycles: u8) {}

    /// Notify the mapper of a scanline (called every PPU scanline).
    ///
    /// Some mappers (like MMC3) count scanlines for IRQ timing.
    fn scanline(&mut self) {}

    /// Notify the mapper of PPU A12 rising edge.
    ///
    /// MMC3 uses A12 for IRQ timing.
    fn ppu_a12_rising(&mut self) {}

    /// Get the mapper number (iNES mapper ID).
    fn mapper_number(&self) -> u16;

    /// Get the mapper name.
    fn mapper_name(&self) -> &'static str;

    /// Check if the mapper has battery-backed RAM.
    fn has_battery(&self) -> bool {
        false
    }

    /// Get a reference to the battery-backed RAM for saving.
    fn battery_ram(&self) -> Option<&[u8]> {
        None
    }

    /// Set the battery-backed RAM content (for loading saves).
    fn set_battery_ram(&mut self, _data: &[u8]) {}

    /// Reset the mapper to its initial state.
    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirroring_default() {
        let mirroring = Mirroring::default();
        assert_eq!(mirroring, Mirroring::Horizontal);
    }
}
