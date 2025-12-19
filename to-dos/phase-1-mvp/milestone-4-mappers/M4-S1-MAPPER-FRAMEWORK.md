# [Milestone 4] Sprint 4.1: Mapper Framework & Infrastructure

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1-2 weeks
**Assignee:** Claude Code / Developer

---

## Overview

Establish the mapper infrastructure including the Mapper trait, ROM format parsing (iNES and NES 2.0), mapper factory, and base implementation patterns. This sprint creates the foundation for all mapper implementations.

---

## Acceptance Criteria

- [ ] Mapper trait definition with all required methods
- [ ] iNES header parsing (16-byte format)
- [ ] NES 2.0 header parsing (extended format)
- [ ] ROM struct with PRG/CHR data
- [ ] Mapper factory pattern
- [ ] Battery-backed SRAM interface
- [ ] Mirroring mode support
- [ ] Zero unsafe code
- [ ] Comprehensive unit tests

---

## Tasks

### 4.1.1 Create Mappers Crate Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Set up the rustynes-mappers crate with initial file structure and dependencies.

**Files:**

- `crates/rustynes-mappers/Cargo.toml` - Crate manifest
- `crates/rustynes-mappers/src/lib.rs` - Public API
- `crates/rustynes-mappers/src/mapper.rs` - Mapper trait

**Subtasks:**

- [ ] Create Cargo.toml with dependencies
  - [ ] Add `thiserror = "1.0"` for error handling
  - [ ] Add `log = "0.4"` for logging
  - [ ] Add `bitflags = "2.4"` for flags
- [ ] Set up lib.rs with public exports
- [ ] Create initial module structure
- [ ] Add documentation and README

**Implementation:**

```toml
# Cargo.toml
[package]
name = "rustynes-mappers"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[dependencies]
thiserror = "1.0"
log = "0.4"
bitflags = "2.4"

[dev-dependencies]
```

---

### 4.1.2 Mapper Trait Definition

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Define the Mapper trait that all mapper implementations must implement.

**Files:**

- `crates/rustynes-mappers/src/mapper.rs` - Mapper trait

**Subtasks:**

- [ ] Define read_prg() for PRG-ROM reads
- [ ] Define write_prg() for mapper registers
- [ ] Define read_chr() for CHR reads
- [ ] Define write_chr() for CHR-RAM writes
- [ ] Define mirroring() for nametable mirroring
- [ ] Define IRQ methods (pending, clear, clock)
- [ ] Define PPU notification methods (A12 edge)
- [ ] Define SRAM access methods
- [ ] Add Send trait bound for thread safety

**Implementation:**

```rust
use crate::rom::Mirroring;

/// Mapper trait for cartridge hardware emulation
pub trait Mapper: Send {
    /// Read from PRG address space ($6000-$FFFF)
    fn read_prg(&self, addr: u16) -> u8;

    /// Write to PRG address space (for mapper registers and SRAM)
    fn write_prg(&mut self, addr: u16, value: u8);

    /// Read from CHR address space ($0000-$1FFF)
    fn read_chr(&self, addr: u16) -> u8;

    /// Write to CHR address space (for CHR-RAM)
    fn write_chr(&mut self, addr: u16, value: u8);

    /// Get current nametable mirroring mode
    fn mirroring(&self) -> Mirroring;

    /// Check if mapper IRQ is pending
    fn irq_pending(&self) -> bool {
        false
    }

    /// Clear mapper IRQ flag
    fn clear_irq(&mut self) {}

    /// Clock the mapper (for IRQ counters)
    fn clock(&mut self, _cycles: u8) {}

    /// Notify mapper of PPU A12 rising edge (for MMC3 scanline counter)
    fn ppu_a12_edge(&mut self) {}

    /// Get immutable reference to battery-backed SRAM
    fn sram(&self) -> Option<&[u8]> {
        None
    }

    /// Get mutable reference to battery-backed SRAM
    fn sram_mut(&mut self) -> Option<&mut [u8]> {
        None
    }

    /// Get mapper number
    fn mapper_number(&self) -> u16;

    /// Get submapper number (NES 2.0)
    fn submapper(&self) -> u8 {
        0
    }
}
```

---

### 4.1.3 Mirroring Modes

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Define nametable mirroring modes.

**Files:**

- `crates/rustynes-mappers/src/rom.rs` - Mirroring enum

**Subtasks:**

- [ ] Define Mirroring enum
- [ ] Horizontal (vertical arrangement)
- [ ] Vertical (horizontal arrangement)
- [ ] SingleScreen (all one nametable)
- [ ] FourScreen (4KB external VRAM)

**Implementation:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    /// Horizontal mirroring (vertical arrangement)
    /// Nametables: A A
    ///             B B
    Horizontal,

    /// Vertical mirroring (horizontal arrangement)
    /// Nametables: A B
    ///             A B
    Vertical,

    /// Single-screen mirroring (all same nametable)
    /// Nametables: A A
    ///             A A
    SingleScreen,

    /// Four-screen mirroring (4KB external VRAM on cartridge)
    /// Nametables: A B
    ///             C D
    FourScreen,
}

impl Mirroring {
    /// Map logical nametable address to physical address
    pub fn map_address(&self, addr: u16) -> usize {
        let addr = addr & 0x0FFF; // Remove $2000 base and mirror $3000

        match self {
            Mirroring::Horizontal => {
                // $2000-$23FF, $2400-$27FF → first 1KB
                // $2800-$2BFF, $2C00-$2FFF → second 1KB
                if addr < 0x0800 {
                    (addr & 0x03FF) as usize
                } else {
                    (0x0400 | (addr & 0x03FF)) as usize
                }
            }
            Mirroring::Vertical => {
                // $2000-$23FF, $2800-$2BFF → first 1KB
                // $2400-$27FF, $2C00-$2FFF → second 1KB
                ((addr & 0x0400) | (addr & 0x03FF)) as usize
            }
            Mirroring::SingleScreen => {
                (addr & 0x03FF) as usize
            }
            Mirroring::FourScreen => {
                // Four-screen uses full 4KB
                (addr & 0x0FFF) as usize
            }
        }
    }
}
```

---

### 4.1.4 iNES Header Parsing

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement iNES (.nes) file format parsing.

**Files:**

- `crates/rustynes-mappers/src/rom.rs` - ROM and header parsing

**Subtasks:**

- [ ] Parse 16-byte iNES header
- [ ] Extract magic number ("NES" + $1A)
- [ ] Extract PRG-ROM size (16KB units)
- [ ] Extract CHR-ROM size (8KB units)
- [ ] Parse flags 6 (mapper low, mirroring, battery, trainer)
- [ ] Parse flags 7 (mapper high, VS System, PlayChoice)
- [ ] Calculate mapper number
- [ ] Handle trainer (512 bytes at $7000)
- [ ] Load PRG-ROM and CHR-ROM data

**Implementation:**

```rust
use std::io::{self, Read};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RomError {
    #[error("Invalid iNES magic number")]
    InvalidMagic,

    #[error("Unsupported ROM format")]
    UnsupportedFormat,

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid ROM size: PRG={0} CHR={1}")]
    InvalidSize(usize, usize),
}

pub struct INesHeader {
    pub prg_rom_size: usize,      // In 16KB units
    pub chr_rom_size: usize,      // In 8KB units (0 = CHR-RAM)
    pub mapper_number: u16,
    pub submapper: u8,
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub four_screen: bool,
    pub nes2_format: bool,
}

impl INesHeader {
    pub fn parse(header: &[u8; 16]) -> Result<Self, RomError> {
        // Check magic number
        if &header[0..4] != b"NES\x1A" {
            return Err(RomError::InvalidMagic);
        }

        let prg_rom_size = header[4] as usize;
        let chr_rom_size = header[5] as usize;

        // Flags 6
        let flags6 = header[6];
        let mirroring = if (flags6 & 0x01) == 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
        let has_battery = (flags6 & 0x02) != 0;
        let has_trainer = (flags6 & 0x04) != 0;
        let four_screen = (flags6 & 0x08) != 0;
        let mapper_low = (flags6 >> 4) & 0x0F;

        // Flags 7
        let flags7 = header[7];
        let mapper_high = flags7 & 0xF0;
        let mapper_number = (mapper_high | mapper_low) as u16;

        // Check for NES 2.0 format
        let nes2_format = (flags7 & 0x0C) == 0x08;

        let submapper = if nes2_format {
            (header[8] >> 4) & 0x0F
        } else {
            0
        };

        // Override mirroring if four-screen
        let mirroring = if four_screen {
            Mirroring::FourScreen
        } else {
            mirroring
        };

        Ok(Self {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
            submapper,
            mirroring,
            has_battery,
            has_trainer,
            four_screen,
            nes2_format,
        })
    }
}

pub struct Rom {
    pub header: INesHeader,
    pub trainer: Option<Vec<u8>>,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
}

impl Rom {
    pub fn load<R: Read>(mut reader: R) -> Result<Self, RomError> {
        // Read header
        let mut header_bytes = [0u8; 16];
        reader.read_exact(&mut header_bytes)?;

        let header = INesHeader::parse(&header_bytes)?;

        // Read trainer if present
        let trainer = if header.has_trainer {
            let mut trainer_data = vec![0u8; 512];
            reader.read_exact(&mut trainer_data)?;
            Some(trainer_data)
        } else {
            None
        };

        // Read PRG-ROM
        let prg_size = header.prg_rom_size * 16384;
        let mut prg_rom = vec![0u8; prg_size];
        reader.read_exact(&mut prg_rom)?;

        // Read CHR-ROM (or allocate CHR-RAM)
        let chr_rom = if header.chr_rom_size > 0 {
            let chr_size = header.chr_rom_size * 8192;
            let mut chr_rom = vec![0u8; chr_size];
            reader.read_exact(&mut chr_rom)?;
            chr_rom
        } else {
            // CHR-RAM: 8KB
            vec![0u8; 8192]
        };

        Ok(Self {
            header,
            trainer,
            prg_rom,
            chr_rom,
        })
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, RomError> {
        use std::io::Cursor;
        Self::load(Cursor::new(data))
    }
}
```

---

### 4.1.5 NES 2.0 Format Support

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Extend iNES parser to support NES 2.0 format features.

**Files:**

- `crates/rustynes-mappers/src/rom.rs` - NES 2.0 parsing

**Subtasks:**

- [ ] Detect NES 2.0 format (flags7 bits 2-3 == 10b)
- [ ] Parse extended mapper number (12 bits)
- [ ] Parse submapper (4 bits)
- [ ] Parse extended PRG/CHR sizes
- [ ] Parse RAM sizes (PRG-RAM, PRG-NVRAM, CHR-RAM, CHR-NVRAM)
- [ ] Parse region (NTSC/PAL/dual)

**Implementation:**

```rust
impl INesHeader {
    fn parse_nes20(header: &[u8; 16]) -> Self {
        // Extended mapper number (12 bits)
        let mapper_low = (header[6] >> 4) & 0x0F;
        let mapper_mid = header[7] & 0xF0;
        let mapper_high = (header[8] & 0x0F) << 8;
        let mapper_number = (mapper_high | mapper_mid as u16 | mapper_low as u16);

        // Submapper (4 bits)
        let submapper = (header[8] >> 4) & 0x0F;

        // Extended PRG/CHR sizes
        let prg_rom_size = if (header[9] & 0x0F) == 0x0F {
            // Exponent-multiplier notation (for huge ROMs)
            let exponent = (header[4] >> 2) & 0x3F;
            let multiplier = (header[4] & 0x03) * 2 + 1;
            ((1 << exponent) * multiplier as usize) / 16384
        } else {
            let lsb = header[4] as usize;
            let msb = (header[9] & 0x0F) as usize;
            (msb << 8) | lsb
        };

        let chr_rom_size = if (header[9] & 0xF0) == 0xF0 {
            let exponent = (header[5] >> 2) & 0x3F;
            let multiplier = (header[5] & 0x03) * 2 + 1;
            ((1 << exponent) * multiplier as usize) / 8192
        } else {
            let lsb = header[5] as usize;
            let msb = ((header[9] >> 4) & 0x0F) as usize;
            (msb << 8) | lsb
        };

        // ... rest of parsing ...

        Self {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
            submapper,
            // ...
            nes2_format: true,
            // ...
        }
    }
}
```

---

### 4.1.6 Mapper Factory

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement mapper factory pattern to create appropriate mapper from ROM.

**Files:**

- `crates/rustynes-mappers/src/factory.rs` - Mapper factory

**Subtasks:**

- [ ] Create mapper based on mapper number
- [ ] Handle unsupported mappers gracefully
- [ ] Pass ROM data to mapper constructor
- [ ] Return boxed Mapper trait object

**Implementation:**

```rust
use crate::mapper::Mapper;
use crate::rom::{Rom, RomError};

// Import mapper implementations
use crate::mapper000::NROM;
use crate::mapper001::MMC1;
use crate::mapper002::UxROM;
use crate::mapper003::CNROM;
use crate::mapper004::MMC3;

pub fn create_mapper(rom: Rom) -> Result<Box<dyn Mapper>, MapperError> {
    let mapper_number = rom.header.mapper_number;
    let submapper = rom.header.submapper;

    match mapper_number {
        0 => Ok(Box::new(NROM::new(rom))),
        1 => Ok(Box::new(MMC1::new(rom, submapper))),
        2 => Ok(Box::new(UxROM::new(rom, submapper))),
        3 => Ok(Box::new(CNROM::new(rom))),
        4 => Ok(Box::new(MMC3::new(rom, submapper))),
        _ => Err(MapperError::UnsupportedMapper(mapper_number)),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MapperError {
    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u16),

    #[error("ROM error: {0}")]
    Rom(#[from] RomError),
}
```

---

### 4.1.7 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for ROM parsing and mapper infrastructure.

**Files:**

- `crates/rustynes-mappers/src/rom.rs` - ROM tests
- `crates/rustynes-mappers/src/factory.rs` - Factory tests

**Subtasks:**

- [ ] Test iNES magic number validation
- [ ] Test iNES header parsing
- [ ] Test NES 2.0 detection
- [ ] Test mapper number calculation
- [ ] Test mirroring mode detection
- [ ] Test trainer handling
- [ ] Test PRG/CHR size calculation
- [ ] Test factory pattern

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_ines_header(
        prg: u8,
        chr: u8,
        mapper: u8,
        flags6: u8,
    ) -> [u8; 16] {
        let mut header = [0u8; 16];
        header[0..4].copy_from_slice(b"NES\x1A");
        header[4] = prg;
        header[5] = chr;
        header[6] = flags6 | ((mapper & 0x0F) << 4);
        header[7] = mapper & 0xF0;
        header
    }

    #[test]
    fn test_valid_ines_header() {
        let header = create_ines_header(2, 1, 0, 0x00);
        let parsed = INesHeader::parse(&header).unwrap();

        assert_eq!(parsed.prg_rom_size, 2);
        assert_eq!(parsed.chr_rom_size, 1);
        assert_eq!(parsed.mapper_number, 0);
        assert_eq!(parsed.mirroring, Mirroring::Horizontal);
    }

    #[test]
    fn test_invalid_magic() {
        let mut header = create_ines_header(2, 1, 0, 0x00);
        header[0] = b'X';

        assert!(INesHeader::parse(&header).is_err());
    }

    #[test]
    fn test_mapper_number_calculation() {
        // Mapper 1 (MMC1)
        let header = create_ines_header(8, 0, 1, 0x00);
        let parsed = INesHeader::parse(&header).unwrap();
        assert_eq!(parsed.mapper_number, 1);

        // Mapper 4 (MMC3)
        let header = create_ines_header(16, 8, 4, 0x00);
        let parsed = INesHeader::parse(&header).unwrap();
        assert_eq!(parsed.mapper_number, 4);
    }

    #[test]
    fn test_mirroring_modes() {
        // Horizontal
        let header = create_ines_header(2, 1, 0, 0x00);
        let parsed = INesHeader::parse(&header).unwrap();
        assert_eq!(parsed.mirroring, Mirroring::Horizontal);

        // Vertical
        let header = create_ines_header(2, 1, 0, 0x01);
        let parsed = INesHeader::parse(&header).unwrap();
        assert_eq!(parsed.mirroring, Mirroring::Vertical);

        // Four-screen
        let header = create_ines_header(2, 1, 0, 0x08);
        let parsed = INesHeader::parse(&header).unwrap();
        assert_eq!(parsed.mirroring, Mirroring::FourScreen);
    }

    #[test]
    fn test_battery_flag() {
        let header = create_ines_header(2, 1, 0, 0x02);
        let parsed = INesHeader::parse(&header).unwrap();
        assert!(parsed.has_battery);
    }

    #[test]
    fn test_trainer_flag() {
        let header = create_ines_header(2, 1, 0, 0x04);
        let parsed = INesHeader::parse(&header).unwrap();
        assert!(parsed.has_trainer);
    }

    #[test]
    fn test_nes20_detection() {
        let mut header = create_ines_header(2, 1, 0, 0x00);
        header[7] = (header[7] & 0xF3) | 0x08; // Set NES 2.0 bits

        let parsed = INesHeader::parse(&header).unwrap();
        assert!(parsed.nes2_format);
    }
}
```

---

## Dependencies

**Required:**

- Rust 1.75+ toolchain
- thiserror = "1.0"
- log = "0.4"
- bitflags = "2.4"

**Blocks:**

- Sprint 4.2: Mapper 0 (NROM) - needs trait and ROM parsing
- All subsequent mapper implementations

---

## Related Documentation

- [Mapper Overview](../../../docs/mappers/MAPPER_OVERVIEW.md)
- [iNES Format](../../../docs/formats/INES_FORMAT.md)
- [NES 2.0 Format](../../../docs/formats/NES20_FORMAT.md)
- [NESdev Wiki - Mapper](https://www.nesdev.org/wiki/Mapper)
- [NESdev Wiki - iNES](https://www.nesdev.org/wiki/INES)
- [NESdev Wiki - NES 2.0](https://www.nesdev.org/wiki/NES_2.0)

---

## Technical Notes

### Mapper Number Calculation

iNES format stores mapper number split across two nibbles:
- Flags 6 bits 4-7: Lower nibble
- Flags 7 bits 4-7: Upper nibble

Combined: `(flags7 & 0xF0) | (flags6 >> 4)`

### PRG/CHR Size Units

- PRG-ROM: Measured in 16KB units
- CHR-ROM: Measured in 8KB units
- CHR-ROM size 0: Indicates CHR-RAM (allocate 8KB)

### Battery-Backed RAM

If battery flag is set, mapper should provide SRAM that can be saved/loaded.

### Mirroring Priority

Four-screen flag overrides basic mirroring flag. Some mappers (MMC1, MMC3) can dynamically control mirroring.

---

## Test Requirements

- [ ] Unit tests for iNES header parsing
- [ ] Unit tests for NES 2.0 detection
- [ ] Unit tests for mapper number calculation
- [ ] Unit tests for mirroring modes
- [ ] Unit tests for ROM loading
- [ ] Unit tests for factory pattern
- [ ] Integration test: Load real ROM file

---

## Performance Targets

- ROM parsing: <1ms for typical ROMs
- Header parsing: <10 μs
- Factory creation: <100 μs
- Memory: <overhead of ROM data

---

## Success Criteria

- [ ] Mapper trait compiles and is usable
- [ ] iNES header parsing works for all test cases
- [ ] NES 2.0 detection works correctly
- [ ] Mapper number calculation is accurate
- [ ] Mirroring modes are correct
- [ ] ROM loading handles all edge cases
- [ ] Factory pattern creates correct mapper types
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Next Sprint:** [Sprint 4.2: Mapper 0 (NROM)](M4-S2-NROM.md)
