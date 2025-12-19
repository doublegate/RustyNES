//! Mapper trait definition.
//!
//! The Mapper trait defines the interface for all NES cartridge mapper implementations.
//! Mappers handle bank switching, mirroring control, and special features like IRQ generation.

use crate::Mirroring;

/// NES cartridge mapper interface.
///
/// This trait defines the contract for all mapper implementations. Mappers are responsible for:
///
/// - **PRG-ROM Banking**: Mapping CPU address space ($8000-$FFFF) to PRG-ROM banks
/// - **CHR Banking**: Mapping PPU address space ($0000-$1FFF) to CHR-ROM/RAM banks
/// - **Mirroring Control**: Determining nametable mirroring mode
/// - **IRQ Generation**: Triggering interrupts for scanline effects (MMC3, MMC5, etc.)
/// - **Battery-Backed RAM**: Providing save game storage (SRAM)
///
/// # Safety
///
/// All mapper implementations must be `Send` to allow safe transfer between threads.
/// Mappers should avoid internal mutability beyond what's necessary for register writes.
///
/// # Examples
///
/// ```ignore
/// use rustynes_mappers::{Mapper, Mirroring};
///
/// // Simple passthrough mapper (NROM)
/// struct Nrom {
///     prg_rom: Vec<u8>,
///     chr_rom: Vec<u8>,
///     mirroring: Mirroring,
/// }
///
/// impl Mapper for Nrom {
///     fn read_prg(&self, addr: u16) -> u8 {
///         let offset = (addr - 0x8000) as usize % self.prg_rom.len();
///         self.prg_rom[offset]
///     }
///
///     fn write_prg(&mut self, _addr: u16, _value: u8) {
///         // NROM has no writable registers
///     }
///
///     fn read_chr(&self, addr: u16) -> u8 {
///         self.chr_rom[addr as usize]
///     }
///
///     fn write_chr(&mut self, _addr: u16, _value: u8) {
///         // NROM has CHR-ROM (not writable)
///     }
///
///     fn mirroring(&self) -> Mirroring {
///         self.mirroring
///     }
///
///     fn mapper_number(&self) -> u16 {
///         0
///     }
/// }
/// ```
pub trait Mapper: Send {
    /// Read from PRG-ROM address space ($8000-$FFFF).
    ///
    /// # Arguments
    ///
    /// * `addr` - CPU address in range $8000-$FFFF
    ///
    /// # Returns
    ///
    /// Byte value at the given address after bank mapping
    ///
    /// # Panics
    ///
    /// Implementations may panic if `addr < 0x8000` (invalid PRG address).
    fn read_prg(&self, addr: u16) -> u8;

    /// Write to PRG address space ($8000-$FFFF).
    ///
    /// This is primarily used for writing to mapper registers. Writes may:
    /// - Change PRG/CHR banking
    /// - Update mirroring mode
    /// - Configure IRQ settings
    /// - Write to PRG-RAM (if present and not write-protected)
    ///
    /// # Arguments
    ///
    /// * `addr` - CPU address in range $8000-$FFFF
    /// * `value` - Byte value to write
    ///
    /// # Panics
    ///
    /// Implementations may panic if `addr < 0x8000` (invalid PRG address).
    fn write_prg(&mut self, addr: u16, value: u8);

    /// Read from CHR address space ($0000-$1FFF).
    ///
    /// # Arguments
    ///
    /// * `addr` - PPU address in range $0000-$1FFF
    ///
    /// # Returns
    ///
    /// Byte value at the given address after bank mapping
    ///
    /// # Panics
    ///
    /// Implementations may panic if `addr > 0x1FFF` (invalid CHR address).
    fn read_chr(&self, addr: u16) -> u8;

    /// Write to CHR address space ($0000-$1FFF).
    ///
    /// Only functional for CHR-RAM. CHR-ROM cartridges typically ignore writes.
    ///
    /// # Arguments
    ///
    /// * `addr` - PPU address in range $0000-$1FFF
    /// * `value` - Byte value to write
    ///
    /// # Panics
    ///
    /// Implementations may panic if `addr > 0x1FFF` (invalid CHR address).
    fn write_chr(&mut self, addr: u16, value: u8);

    /// Get current nametable mirroring mode.
    ///
    /// # Returns
    ///
    /// Current mirroring mode. May change dynamically for mappers with mirroring control.
    fn mirroring(&self) -> Mirroring;

    /// Check if an IRQ is pending.
    ///
    /// Used by mappers with IRQ support (MMC3, MMC5, VRC, etc.) to signal the CPU.
    ///
    /// # Returns
    ///
    /// `true` if the mapper is asserting IRQ, `false` otherwise.
    ///
    /// # Default Implementation
    ///
    /// Returns `false` for mappers without IRQ support.
    fn irq_pending(&self) -> bool {
        false
    }

    /// Clear the IRQ flag.
    ///
    /// Called when the CPU acknowledges the IRQ (typically after reading $FFFC/FFFD).
    ///
    /// # Default Implementation
    ///
    /// No-op for mappers without IRQ support.
    fn clear_irq(&mut self) {}

    /// Clock the mapper for a number of CPU cycles.
    ///
    /// Used by mappers with cycle-based timers (MMC3 alternate IRQ mode, FDS, etc.).
    ///
    /// # Arguments
    ///
    /// * `cycles` - Number of CPU cycles elapsed
    ///
    /// # Default Implementation
    ///
    /// No-op for mappers without cycle-based features.
    fn clock(&mut self, _cycles: u8) {}

    /// Notify mapper of PPU A12 rising edge.
    ///
    /// Used by MMC3 and compatible mappers for scanline IRQ counting.
    /// Called when PPU address bit 12 transitions from 0 to 1.
    ///
    /// # Default Implementation
    ///
    /// No-op for mappers without A12-based IRQ.
    fn ppu_a12_edge(&mut self) {}

    /// Get reference to battery-backed SRAM.
    ///
    /// # Returns
    ///
    /// `Some(&[u8])` if the mapper has battery-backed SRAM, `None` otherwise.
    ///
    /// # Default Implementation
    ///
    /// Returns `None` for mappers without SRAM.
    fn sram(&self) -> Option<&[u8]> {
        None
    }

    /// Get mutable reference to battery-backed SRAM.
    ///
    /// # Returns
    ///
    /// `Some(&mut [u8])` if the mapper has battery-backed SRAM, `None` otherwise.
    ///
    /// # Default Implementation
    ///
    /// Returns `None` for mappers without SRAM.
    fn sram_mut(&mut self) -> Option<&mut [u8]> {
        None
    }

    /// Get the mapper number (iNES/NES 2.0 format).
    ///
    /// # Returns
    ///
    /// iNES mapper number (0-4095 for NES 2.0, 0-255 for iNES 1.0)
    fn mapper_number(&self) -> u16;

    /// Get the submapper number (NES 2.0 format only).
    ///
    /// # Returns
    ///
    /// Submapper number (0-15), or 0 if not applicable.
    ///
    /// # Default Implementation
    ///
    /// Returns 0 (no submapper).
    fn submapper(&self) -> u8 {
        0
    }

    /// Clone the mapper state.
    ///
    /// This is used for save states and debugging. Implementations should return
    /// a boxed clone of the current mapper state.
    ///
    /// # Returns
    ///
    /// Boxed clone of the mapper
    fn clone_mapper(&self) -> Box<dyn Mapper>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test mapper to verify trait implementation
    struct TestMapper {
        prg_rom: Vec<u8>,
        chr_ram: Vec<u8>,
        mirroring: Mirroring,
    }

    impl Mapper for TestMapper {
        fn read_prg(&self, addr: u16) -> u8 {
            assert!(addr >= 0x8000, "Invalid PRG address");
            let offset = (addr - 0x8000) as usize;
            self.prg_rom[offset % self.prg_rom.len()]
        }

        fn write_prg(&mut self, addr: u16, _value: u8) {
            assert!(addr >= 0x8000, "Invalid PRG address");
        }

        fn read_chr(&self, addr: u16) -> u8 {
            assert!(addr <= 0x1FFF, "Invalid CHR address");
            self.chr_ram[addr as usize]
        }

        fn write_chr(&mut self, addr: u16, value: u8) {
            assert!(addr <= 0x1FFF, "Invalid CHR address");
            self.chr_ram[addr as usize] = value;
        }

        fn mirroring(&self) -> Mirroring {
            self.mirroring
        }

        fn mapper_number(&self) -> u16 {
            0
        }

        fn clone_mapper(&self) -> Box<dyn Mapper> {
            Box::new(TestMapper {
                prg_rom: self.prg_rom.clone(),
                chr_ram: self.chr_ram.clone(),
                mirroring: self.mirroring,
            })
        }
    }

    #[test]
    fn test_mapper_trait_implementation() {
        let mut mapper = TestMapper {
            prg_rom: vec![0x42; 0x8000],
            chr_ram: vec![0; 0x2000],
            mirroring: Mirroring::Horizontal,
        };

        // Test PRG read
        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0x42);

        // Test PRG write (should not panic)
        mapper.write_prg(0x8000, 0xFF);

        // Test CHR read/write
        mapper.write_chr(0x1000, 0x55);
        assert_eq!(mapper.read_chr(0x1000), 0x55);

        // Test mirroring
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        // Test default implementations
        assert!(!mapper.irq_pending());
        mapper.clear_irq();
        mapper.clock(1);
        mapper.ppu_a12_edge();
        assert!(mapper.sram().is_none());
        assert!(mapper.sram_mut().is_none());
        assert_eq!(mapper.submapper(), 0);
    }

    #[test]
    fn test_mapper_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Box<dyn Mapper>>();
    }
}
