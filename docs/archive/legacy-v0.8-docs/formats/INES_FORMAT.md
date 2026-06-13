# iNES ROM Format Specification

## Overview

The iNES format (`.nes` extension) is the most common ROM image format for NES games. Created by Marat Fayzullin in 1996, it encapsulates the game ROM data along with metadata describing the cartridge hardware configuration.

**Format Version:** iNES 1.0
**File Extension:** `.nes`
**Magic Number:** `$4E $45 $53 $1A` ("NES" + MS-DOS EOF)

---

## Header Structure

The iNES header is exactly 16 bytes:

```
Offset  Size  Description
------  ----  -----------
0-3     4     Magic number: "NES\x1A"
4       1     PRG-ROM size in 16KB units
5       1     CHR-ROM size in 8KB units (0 = CHR-RAM)
6       1     Flags 6: Mapper low nibble, mirroring, battery, trainer
7       1     Flags 7: Mapper high nibble, VS/Playchoice, NES 2.0
8       1     Flags 8: PRG-RAM size (rarely used)
9       1     Flags 9: TV system (rarely used)
10      1     Flags 10: TV system, PRG-RAM (unofficial)
11-15   5     Padding (should be zero)
```

### Byte-by-Byte Breakdown

#### Bytes 0-3: Magic Number

```rust
const INES_MAGIC: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A]; // "NES\x1A"
```

The magic number identifies the file as an iNES ROM. The `0x1A` byte is the MS-DOS end-of-file marker, which prevents the `TYPE` command from displaying binary garbage.

#### Byte 4: PRG-ROM Size

```
PRG-ROM size = value × 16,384 bytes (16 KB)
```

| Value | PRG-ROM Size |
|-------|--------------|
| 1     | 16 KB        |
| 2     | 32 KB        |
| 4     | 64 KB        |
| 8     | 128 KB       |
| 16    | 256 KB       |
| 32    | 512 KB       |

**Maximum (iNES 1.0):** 255 × 16 KB = 4,080 KB (~4 MB)

#### Byte 5: CHR-ROM Size

```
CHR-ROM size = value × 8,192 bytes (8 KB)
```

| Value | CHR-ROM Size | Notes |
|-------|--------------|-------|
| 0     | 0 KB         | Uses CHR-RAM instead |
| 1     | 8 KB         | Single bank |
| 2     | 16 KB        | Two banks |
| 4     | 32 KB        | Common for MMC1 |
| 16    | 128 KB       | Large CHR-ROM |

**CHR-RAM:** When this byte is 0, the cartridge uses CHR-RAM (typically 8 KB) instead of CHR-ROM. Games like Super Mario Bros. 3 and Kirby's Adventure use CHR-RAM with bankswitching.

#### Byte 6: Flags 6

```
7  bit  0
---------
NNNN FTBM

N: Lower nibble of mapper number
F: Four-screen VRAM layout
T: Trainer present (512 bytes at $7000-$71FF)
B: Battery-backed PRG-RAM at $6000-$7FFF
M: Mirroring (0 = horizontal, 1 = vertical)
```

**Bit Layout:**

| Bit | Mask | Description |
|-----|------|-------------|
| 0   | 0x01 | Mirroring: 0 = horizontal, 1 = vertical |
| 1   | 0x02 | Battery-backed PRG-RAM present |
| 2   | 0x04 | 512-byte trainer at $7000-$71FF |
| 3   | 0x08 | Four-screen VRAM (ignore mirroring bit) |
| 4-7 | 0xF0 | Lower nibble of mapper number |

**Mirroring Types:**

```rust
pub enum Mirroring {
    Horizontal,    // Vertical arrangement (CIRAM A10 = PPU A11)
    Vertical,      // Horizontal arrangement (CIRAM A10 = PPU A10)
    FourScreen,    // No mirroring, requires 4KB VRAM
    SingleScreenA, // All nametables map to CIRAM $000-$3FF
    SingleScreenB, // All nametables map to CIRAM $400-$7FF
}
```

**Note:** "Horizontal mirroring" means nametables are arranged vertically (top/bottom identical). "Vertical mirroring" means nametables are arranged horizontally (left/right identical). This naming convention is historical and counterintuitive.

#### Byte 7: Flags 7

```
7  bit  0
---------
NNNN xxPV

N: Upper nibble of mapper number
P: Playchoice-10 (8KB hint screen after CHR)
V: VS Unisystem
x: If bits 2-3 == 2, NES 2.0 format
```

**Bit Layout:**

| Bit | Mask | Description |
|-----|------|-------------|
| 0   | 0x01 | VS Unisystem |
| 1   | 0x02 | PlayChoice-10 (8KB hint screen data) |
| 2-3 | 0x0C | If == 2 ($08), this is NES 2.0 format |
| 4-7 | 0xF0 | Upper nibble of mapper number |

**NES 2.0 Detection:**

```rust
fn is_nes2_format(header: &[u8; 16]) -> bool {
    (header[7] & 0x0C) == 0x08
}
```

#### Byte 8: Flags 8 (PRG-RAM Size)

```
PRG-RAM size = value × 8,192 bytes (8 KB)
```

**Note:** This byte is rarely used correctly in iNES 1.0 ROMs. Most emulators assume 8 KB PRG-RAM when battery bit is set, regardless of this value. NES 2.0 provides a more reliable PRG-RAM size field.

#### Byte 9: Flags 9 (TV System)

```
7  bit  0
---------
xxxx xxxT

T: TV system (0 = NTSC, 1 = PAL)
```

**Rarely Used:** Most ROMs leave this as 0. For accurate region detection, use ROM database hashing.

#### Byte 10: Flags 10 (Unofficial)

```
7  bit  0
---------
xxBB xxPP

P: TV system (0 = NTSC, 2 = PAL, 1/3 = dual compatible)
B: PRG-RAM ($6000-$7FFF) (0 = present, 1 = not present, 2 = present with conflicts)
```

**Note:** This byte is part of an unofficial extension and should be ignored by modern emulators. Use ROM database for accurate information.

#### Bytes 11-15: Padding

These bytes should be zero. Non-zero values may indicate:

- Corrupted header
- Unofficial extensions
- "DiskDude!" watermark (common corruption)

**DiskDude! Detection:**

```rust
fn has_diskdude_corruption(header: &[u8; 16]) -> bool {
    &header[7..16] == b"DiskDude!" || &header[7..15] == b"DiskDude"
}
```

---

## File Layout

```
+------------------+
|   Header (16B)   |  Bytes 0-15
+------------------+
| Trainer (512B)?  |  Optional, if Flags 6 bit 2 set
+------------------+
|    PRG-ROM       |  Size = Header[4] × 16KB
+------------------+
|    CHR-ROM       |  Size = Header[5] × 8KB (optional)
+------------------+
| PlayChoice (8KB)?|  Rare, if Flags 7 bit 1 set
+------------------+
```

### Calculating Offsets

```rust
fn calculate_offsets(header: &[u8; 16]) -> RomOffsets {
    let has_trainer = (header[6] & 0x04) != 0;
    let prg_size = header[4] as usize * 16384;
    let chr_size = header[5] as usize * 8192;

    let trainer_offset = 16;
    let prg_offset = 16 + if has_trainer { 512 } else { 0 };
    let chr_offset = prg_offset + prg_size;

    RomOffsets {
        trainer: if has_trainer { Some(trainer_offset) } else { None },
        prg_rom: prg_offset,
        chr_rom: if chr_size > 0 { Some(chr_offset) } else { None },
        prg_size,
        chr_size,
    }
}
```

---

## Mapper Number Calculation

The mapper number is split across Flags 6 and Flags 7:

```rust
fn get_mapper_number(header: &[u8; 16]) -> u8 {
    let low_nibble = (header[6] & 0xF0) >> 4;
    let high_nibble = header[7] & 0xF0;
    high_nibble | low_nibble
}
```

### Common Mappers

| Mapper | Name | Games | Notes |
|--------|------|-------|-------|
| 0 | NROM | ~247 | No mapper, 32KB PRG + 8KB CHR max |
| 1 | MMC1 | ~680 | SxROM, serial register |
| 2 | UxROM | ~270 | PRG switching, bus conflicts |
| 3 | CNROM | ~155 | CHR switching, bus conflicts |
| 4 | MMC3 | ~600 | TxROM, scanline counter |
| 7 | AxROM | ~75 | Single-screen mirroring |
| 9 | MMC2 | 1 | Punch-Out!!, latch-based CHR |
| 10 | MMC4 | 3 | Fire Emblem, latch-based CHR |
| 11 | Color Dreams | ~35 | Unlicensed, simple banking |
| 66 | GxROM | ~17 | Simple PRG+CHR banking |
| 71 | Camerica | ~17 | Codemasters games |
| 206 | DxROM | ~20 | Namco/Tengen |

---

## Rust Implementation

### Header Structure

```rust
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TvSystem {
    Ntsc,
    Pal,
    DualCompatible,
}

#[derive(Debug, Clone)]
pub struct INesHeader {
    pub prg_rom_size: usize,      // In bytes
    pub chr_rom_size: usize,      // In bytes (0 = CHR-RAM)
    pub mapper: u8,
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub is_vs_unisystem: bool,
    pub is_playchoice: bool,
    pub tv_system: TvSystem,
    pub prg_ram_size: usize,      // In bytes
}

#[derive(Debug, Error)]
pub enum INesError {
    #[error("Invalid magic number: expected 'NES\\x1A', got {0:02X?}")]
    InvalidMagic([u8; 4]),

    #[error("File too small: expected at least {expected} bytes, got {actual}")]
    FileTooSmall { expected: usize, actual: usize },

    #[error("PRG-ROM size mismatch: header specifies {expected} bytes, file has {actual}")]
    PrgSizeMismatch { expected: usize, actual: usize },

    #[error("CHR-ROM size mismatch: header specifies {expected} bytes, file has {actual}")]
    ChrSizeMismatch { expected: usize, actual: usize },

    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u8),

    #[error("Corrupted header: {0}")]
    CorruptedHeader(String),
}
```

### Header Parser

```rust
impl INesHeader {
    pub fn parse(data: &[u8]) -> Result<Self, INesError> {
        // Validate minimum size
        if data.len() < 16 {
            return Err(INesError::FileTooSmall {
                expected: 16,
                actual: data.len(),
            });
        }

        // Validate magic number
        let magic: [u8; 4] = data[0..4].try_into().unwrap();
        if magic != [0x4E, 0x45, 0x53, 0x1A] {
            return Err(INesError::InvalidMagic(magic));
        }

        // Check for NES 2.0 format
        if (data[7] & 0x0C) == 0x08 {
            // This is NES 2.0, should use different parser
            // For now, parse as iNES 1.0 with warning
        }

        // Check for DiskDude! corruption
        if Self::has_diskdude_corruption(data) {
            // Zero out corrupted bytes 7-15
            let mut clean_data = data.to_vec();
            clean_data[7..16].fill(0);
            return Self::parse_clean(&clean_data);
        }

        Self::parse_clean(data)
    }

    fn parse_clean(data: &[u8]) -> Result<Self, INesError> {
        let flags6 = data[6];
        let flags7 = data[7];
        let flags9 = data[9];

        // Calculate sizes
        let prg_rom_size = data[4] as usize * 16384;
        let chr_rom_size = data[5] as usize * 8192;
        let has_trainer = (flags6 & 0x04) != 0;

        // Validate file size
        let expected_size = 16
            + if has_trainer { 512 } else { 0 }
            + prg_rom_size
            + chr_rom_size;

        if data.len() < expected_size {
            return Err(INesError::FileTooSmall {
                expected: expected_size,
                actual: data.len(),
            });
        }

        // Calculate mapper number
        let mapper = ((flags6 & 0xF0) >> 4) | (flags7 & 0xF0);

        // Determine mirroring
        let mirroring = if (flags6 & 0x08) != 0 {
            Mirroring::FourScreen
        } else if (flags6 & 0x01) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        // Determine TV system
        let tv_system = match flags9 & 0x01 {
            0 => TvSystem::Ntsc,
            _ => TvSystem::Pal,
        };

        // PRG-RAM size (use default 8KB if battery present but size is 0)
        let prg_ram_size = if data[8] > 0 {
            data[8] as usize * 8192
        } else if (flags6 & 0x02) != 0 {
            8192  // Default 8KB for battery-backed games
        } else {
            0
        };

        Ok(INesHeader {
            prg_rom_size,
            chr_rom_size,
            mapper,
            mirroring,
            has_battery: (flags6 & 0x02) != 0,
            has_trainer,
            is_vs_unisystem: (flags7 & 0x01) != 0,
            is_playchoice: (flags7 & 0x02) != 0,
            tv_system,
            prg_ram_size,
        })
    }

    fn has_diskdude_corruption(data: &[u8]) -> bool {
        data.len() >= 16 && (
            &data[7..16] == b"DiskDude!" ||
            &data[7..15] == b"DiskDude"
        )
    }
}
```

### ROM Loader

```rust
pub struct INesRom {
    pub header: INesHeader,
    pub trainer: Option<Vec<u8>>,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub chr_ram: Option<Vec<u8>>,
}

impl INesRom {
    pub fn load(data: &[u8]) -> Result<Self, INesError> {
        let header = INesHeader::parse(data)?;

        let mut offset = 16;

        // Read trainer if present
        let trainer = if header.has_trainer {
            let trainer_data = data[offset..offset + 512].to_vec();
            offset += 512;
            Some(trainer_data)
        } else {
            None
        };

        // Read PRG-ROM
        let prg_end = offset + header.prg_rom_size;
        if data.len() < prg_end {
            return Err(INesError::PrgSizeMismatch {
                expected: header.prg_rom_size,
                actual: data.len() - offset,
            });
        }
        let prg_rom = data[offset..prg_end].to_vec();
        offset = prg_end;

        // Read CHR-ROM or allocate CHR-RAM
        let (chr_rom, chr_ram) = if header.chr_rom_size > 0 {
            let chr_end = offset + header.chr_rom_size;
            if data.len() < chr_end {
                return Err(INesError::ChrSizeMismatch {
                    expected: header.chr_rom_size,
                    actual: data.len() - offset,
                });
            }
            (data[offset..chr_end].to_vec(), None)
        } else {
            // CHR-RAM: typically 8KB, some games use more
            (Vec::new(), Some(vec![0u8; 8192]))
        };

        Ok(INesRom {
            header,
            trainer,
            prg_rom,
            chr_rom,
            chr_ram,
        })
    }

    /// Check if this ROM uses CHR-RAM instead of CHR-ROM
    pub fn uses_chr_ram(&self) -> bool {
        self.chr_ram.is_some()
    }

    /// Get the effective CHR data (ROM or RAM)
    pub fn chr_data(&self) -> &[u8] {
        if let Some(ref ram) = self.chr_ram {
            ram
        } else {
            &self.chr_rom
        }
    }

    /// Get mutable CHR-RAM (returns None if using CHR-ROM)
    pub fn chr_ram_mut(&mut self) -> Option<&mut [u8]> {
        self.chr_ram.as_mut().map(|v| v.as_mut_slice())
    }
}
```

---

## Validation Heuristics

Many iNES ROMs have incorrect or corrupted headers. Use these heuristics:

### Header Sanity Checks

```rust
impl INesHeader {
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        // PRG-ROM must be at least 16KB
        if self.prg_rom_size < 16384 {
            warnings.push("PRG-ROM size less than 16KB is invalid".to_string());
        }

        // PRG-ROM should be power of 2 for most mappers
        if !self.prg_rom_size.is_power_of_two() && self.mapper != 5 {
            warnings.push(format!(
                "PRG-ROM size {} is not a power of 2",
                self.prg_rom_size
            ));
        }

        // CHR-ROM/RAM should be power of 2
        if self.chr_rom_size > 0 && !self.chr_rom_size.is_power_of_two() {
            warnings.push(format!(
                "CHR-ROM size {} is not a power of 2",
                self.chr_rom_size
            ));
        }

        // Mapper 0 (NROM) constraints
        if self.mapper == 0 {
            if self.prg_rom_size > 32768 {
                warnings.push("NROM (mapper 0) supports max 32KB PRG-ROM".to_string());
            }
            if self.chr_rom_size > 8192 {
                warnings.push("NROM (mapper 0) supports max 8KB CHR-ROM".to_string());
            }
        }

        warnings
    }
}
```

### ROM Database Matching

For accurate header information, use CRC32 or SHA-1 hashing to match against a ROM database:

```rust
use std::collections::HashMap;

pub struct RomDatabase {
    entries: HashMap<u32, RomEntry>,
}

pub struct RomEntry {
    pub name: String,
    pub mapper: u8,
    pub submapper: u8,
    pub mirroring: Mirroring,
    pub prg_rom_size: usize,
    pub chr_rom_size: usize,
    pub prg_ram_size: usize,
    pub chr_ram_size: usize,
    pub has_battery: bool,
    pub region: TvSystem,
}

impl RomDatabase {
    /// Calculate CRC32 of PRG+CHR ROM data (excluding header)
    pub fn calculate_crc32(rom: &INesRom) -> u32 {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&rom.prg_rom);
        hasher.update(&rom.chr_rom);
        hasher.finalize()
    }

    /// Look up ROM in database and return corrected header
    pub fn lookup(&self, crc32: u32) -> Option<&RomEntry> {
        self.entries.get(&crc32)
    }

    /// Apply database corrections to header
    pub fn apply_corrections(&self, rom: &mut INesRom) -> bool {
        let crc32 = Self::calculate_crc32(rom);
        if let Some(entry) = self.lookup(crc32) {
            rom.header.mapper = entry.mapper;
            rom.header.mirroring = entry.mirroring;
            rom.header.has_battery = entry.has_battery;
            rom.header.tv_system = entry.region;
            true
        } else {
            false
        }
    }
}
```

---

## Common Issues and Solutions

### 1. DiskDude! Corruption

Some ROM dumping tools (like NESticle's ROM tool) wrote "DiskDude!" into bytes 7-15, corrupting the mapper number.

**Detection:**

```rust
fn is_diskdude_corrupted(header: &[u8; 16]) -> bool {
    header[7..].starts_with(b"DiskDude")
}
```

**Fix:** Zero out bytes 7-15 and use ROM database for correct mapper.

### 2. Incorrect Mapper Numbers

Many early ROM dumps have incorrect mapper assignments.

**Solution:** Use ROM database with CRC32 lookup to get correct mapper.

### 3. Missing PRG-RAM Size

iNES 1.0 has poor PRG-RAM size support.

**Solution:**

- Default to 8KB when battery bit is set
- Use ROM database for games requiring larger PRG-RAM
- Migrate to NES 2.0 format

### 4. Oversized Files

Some ROMs include extra data after CHR-ROM (PlayChoice hint screens, copier headers).

**Solution:** Only read the data sizes specified in header.

### 5. Trainer Data

The trainer section ($7000-$71FF) was used by copiers for cheat codes.

**Solution:** Load trainer data to $7000-$71FF before PRG-ROM banks if trainer bit is set.

---

## File Size Calculation

```rust
impl INesRom {
    /// Calculate expected file size from header
    pub fn expected_file_size(&self) -> usize {
        16  // Header
        + if self.header.has_trainer { 512 } else { 0 }
        + self.header.prg_rom_size
        + self.header.chr_rom_size
    }

    /// Validate actual file matches expected size
    pub fn validate_size(&self, actual_size: usize) -> Result<(), INesError> {
        let expected = self.expected_file_size();
        if actual_size < expected {
            Err(INesError::FileTooSmall {
                expected,
                actual: actual_size,
            })
        } else if actual_size > expected {
            // Warn but don't fail - might have PlayChoice data
            log::warn!(
                "File size {} exceeds expected {} (extra {} bytes)",
                actual_size,
                expected,
                actual_size - expected
            );
            Ok(())
        } else {
            Ok(())
        }
    }
}
```

---

## Integration Example

```rust
use std::fs;
use std::path::Path;

fn load_nes_rom<P: AsRef<Path>>(path: P) -> Result<INesRom, Box<dyn std::error::Error>> {
    let data = fs::read(path)?;
    let mut rom = INesRom::load(&data)?;

    // Validate header
    for warning in rom.header.validate() {
        log::warn!("Header warning: {}", warning);
    }

    // Apply database corrections if available
    let db = RomDatabase::load_default()?;
    if db.apply_corrections(&mut rom) {
        log::info!("Applied database corrections for known ROM");
    }

    log::info!(
        "Loaded ROM: {}KB PRG, {}KB CHR, Mapper {}, {:?}",
        rom.header.prg_rom_size / 1024,
        rom.header.chr_rom_size / 1024,
        rom.header.mapper,
        rom.header.mirroring
    );

    Ok(rom)
}
```

---

## Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_valid_header() -> [u8; 16] {
        [
            0x4E, 0x45, 0x53, 0x1A,  // Magic
            0x02,                      // 2 × 16KB = 32KB PRG
            0x01,                      // 1 × 8KB CHR
            0x01,                      // Vertical mirroring, mapper 0 low
            0x00,                      // Mapper 0 high
            0x00, 0x00, 0x00,         // Flags 8-10
            0x00, 0x00, 0x00, 0x00, 0x00,  // Padding
        ]
    }

    #[test]
    fn test_parse_valid_header() {
        let header = create_valid_header();
        let result = INesHeader::parse(&header);
        assert!(result.is_ok());

        let h = result.unwrap();
        assert_eq!(h.prg_rom_size, 32768);
        assert_eq!(h.chr_rom_size, 8192);
        assert_eq!(h.mapper, 0);
        assert_eq!(h.mirroring, Mirroring::Vertical);
        assert!(!h.has_battery);
        assert!(!h.has_trainer);
    }

    #[test]
    fn test_invalid_magic() {
        let mut header = create_valid_header();
        header[0] = 0x00;

        let result = INesHeader::parse(&header);
        assert!(matches!(result, Err(INesError::InvalidMagic(_))));
    }

    #[test]
    fn test_mapper_number() {
        let mut header = create_valid_header();
        header[6] = 0x10;  // Low nibble = 1
        header[7] = 0x20;  // High nibble = 2

        let h = INesHeader::parse(&header).unwrap();
        assert_eq!(h.mapper, 0x21);  // 33
    }

    #[test]
    fn test_battery_flag() {
        let mut header = create_valid_header();
        header[6] |= 0x02;  // Set battery bit

        let h = INesHeader::parse(&header).unwrap();
        assert!(h.has_battery);
        assert_eq!(h.prg_ram_size, 8192);  // Default 8KB
    }

    #[test]
    fn test_trainer_flag() {
        let mut header = create_valid_header();
        header[6] |= 0x04;  // Set trainer bit

        let h = INesHeader::parse(&header).unwrap();
        assert!(h.has_trainer);
    }

    #[test]
    fn test_four_screen_mirroring() {
        let mut header = create_valid_header();
        header[6] |= 0x08;  // Set four-screen bit

        let h = INesHeader::parse(&header).unwrap();
        assert_eq!(h.mirroring, Mirroring::FourScreen);
    }

    #[test]
    fn test_diskdude_detection() {
        let mut header = create_valid_header();
        header[7..16].copy_from_slice(b"DiskDude!");

        assert!(INesHeader::has_diskdude_corruption(&header));
    }

    #[test]
    fn test_chr_ram() {
        let mut header = create_valid_header();
        header[5] = 0;  // No CHR-ROM

        let h = INesHeader::parse(&header).unwrap();
        assert_eq!(h.chr_rom_size, 0);
    }

    #[test]
    fn test_rom_load() {
        let mut data = vec![0u8; 16 + 32768 + 8192];
        data[0..4].copy_from_slice(&[0x4E, 0x45, 0x53, 0x1A]);
        data[4] = 0x02;  // 32KB PRG
        data[5] = 0x01;  // 8KB CHR

        let rom = INesRom::load(&data).unwrap();
        assert_eq!(rom.prg_rom.len(), 32768);
        assert_eq!(rom.chr_rom.len(), 8192);
        assert!(rom.chr_ram.is_none());
    }

    #[test]
    fn test_chr_ram_allocation() {
        let mut data = vec![0u8; 16 + 32768];
        data[0..4].copy_from_slice(&[0x4E, 0x45, 0x53, 0x1A]);
        data[4] = 0x02;  // 32KB PRG
        data[5] = 0x00;  // No CHR-ROM (use RAM)

        let rom = INesRom::load(&data).unwrap();
        assert!(rom.uses_chr_ram());
        assert!(rom.chr_ram.is_some());
        assert_eq!(rom.chr_ram.as_ref().unwrap().len(), 8192);
    }
}
```

---

## References

- [NESdev Wiki: iNES](https://www.nesdev.org/wiki/INES)
- [NESdev Wiki: Mapper List](https://www.nesdev.org/wiki/Mapper)
- [NESdev Wiki: NES 2.0](https://www.nesdev.org/wiki/NES_2.0)
- [NEScartDB](https://nescartdb.com/) - ROM database

---

## See Also

- [NES20_FORMAT.md](NES20_FORMAT.md) - Extended NES 2.0 format
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mapper implementations
- [ROM_LOADING.md](ROM_LOADING.md) - Complete ROM loading pipeline
