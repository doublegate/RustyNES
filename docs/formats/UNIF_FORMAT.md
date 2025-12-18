# UNIF (Universal NES Image Format) Specification

## Overview

UNIF (Universal NES Image Format) is an alternative ROM format that uses a chunk-based structure to describe NES cartridge hardware. Unlike iNES which uses mapper numbers, UNIF identifies boards by name.

**File Extension:** `.unf`, `.unif`
**Magic Number:** `UNIF` (4 bytes)
**Chunk-Based:** Yes (similar to IFF/RIFF)
**Status:** Legacy format (NES 2.0 preferred for new ROMs)

---

## File Structure

```
+------------------+
|   Header (32B)   |  Magic + version + padding
+------------------+
|   Chunk 1        |  FourCC + size + data
+------------------+
|   Chunk 2        |
+------------------+
|      ...         |
+------------------+
|   Chunk N        |
+------------------+
```

---

## Header Format

```
Offset  Size  Description
------  ----  -----------
$00     4     Magic: "UNIF"
$04     4     Minimum version (little-endian)
$08     24    Reserved (zeros)
```

### Version History

| Version | Changes |
|---------|---------|
| 1-6 | Original implementations |
| 7 | Added MIRR chunk mirroring types |
| 8+ | Various board additions |

---

## Chunk Structure

Each chunk follows this format:

```
Offset  Size  Description
------  ----  -----------
$00     4     Chunk ID (ASCII FourCC)
$04     4     Data length (little-endian)
$08     N     Chunk data (N = length)
```

Chunks are not required to be aligned.

---

## Standard Chunks

### MAPR - Board Name (Required)

Identifies the cartridge board by name:

```
Data: Null-terminated ASCII string
Example: "NES-NROM-256\0"
```

**Common Board Names:**

| Board | Description | Equivalent Mapper |
|-------|-------------|-------------------|
| NES-NROM-128 | 16KB PRG, 8KB CHR | 0 |
| NES-NROM-256 | 32KB PRG, 8KB CHR | 0 |
| NES-CNROM | CHR banking | 3 |
| NES-UNROM | PRG banking | 2 |
| NES-SLROM | MMC1 | 1 |
| NES-SNROM | MMC1 + WRAM | 1 |
| NES-TLROM | MMC3 | 4 |
| NES-TSROM | MMC3 + WRAM | 4 |
| NES-TKROM | MMC3 + battery | 4 |
| HVC-EKROM | MMC5 | 5 |

### PRG0-PRGF - PRG-ROM Chunks

PRG-ROM data in up to 16 chunks:

```
Chunk ID: PRG0, PRG1, ..., PRGF
Data: Raw PRG-ROM bytes
```

Chunks are concatenated in order (PRG0 first).

### CHR0-CHRF - CHR-ROM Chunks

CHR-ROM data in up to 16 chunks:

```
Chunk ID: CHR0, CHR1, ..., CHRF
Data: Raw CHR-ROM bytes
```

### NAME - Game Name

```
Data: Null-terminated UTF-8 string
```

### READ - Comment/README

```
Data: Null-terminated text
```

### DINF - Dumper Info

```
Offset  Size  Description
------  ----  -----------
$00     100   Dumper name (null-terminated)
$64     1     Day dumped
$65     1     Month dumped
$66     2     Year dumped (little-endian)
$68     100   Dumper agent (null-terminated)
```

### TVCI - TV System

```
Data: 1 byte
  $00 = NTSC
  $01 = PAL
  $02 = Dual compatible
```

### CTRL - Controller Info

```
Data: 1 byte (bitfield)
  Bit 0: Standard controller
  Bit 1: Zapper
  Bit 2: R.O.B.
  Bit 3: Arkanoid controller
  Bit 4: Power Pad
  Bit 5: Four-Score adapter
  Bits 6-7: Reserved
```

### BATR - Battery Present

```
Data: 1 byte
  $00 = No battery
  $01 = Battery present
```

### VROR - CHR-RAM Only

Indicates CHR-RAM instead of CHR-ROM:

```
Data: 1 byte
  $01 = Uses CHR-RAM
```

### MIRR - Mirroring

```
Data: 1 byte
  $00 = Horizontal
  $01 = Vertical
  $02 = Single-screen $2000
  $03 = Single-screen $2400
  $04 = Four-screen
  $05 = Mapper-controlled
```

### PCK0-PCKF - PRG CRC32

CRC32 checksums for PRG chunks:

```
Data: 4 bytes (little-endian CRC32)
```

### CCK0-CCKF - CHR CRC32

CRC32 checksums for CHR chunks:

```
Data: 4 bytes (little-endian CRC32)
```

### WRTR - WRAM Present

```
Data: 1 byte array (sizes for each bank)
```

### WRAM - WRAM Size

```
Data: 4 bytes (total size, little-endian)
```

---

## Rust Implementation

### Data Structures

```rust
use thiserror::Error;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifMirroring {
    Horizontal,
    Vertical,
    SingleScreen0,
    SingleScreen1,
    FourScreen,
    MapperControlled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifTvSystem {
    Ntsc,
    Pal,
    Dual,
}

#[derive(Debug, Clone)]
pub struct DumperInfo {
    pub name: String,
    pub day: u8,
    pub month: u8,
    pub year: u16,
    pub agent: String,
}

#[derive(Debug, Clone)]
pub struct UnifRom {
    pub version: u32,
    pub board: String,
    pub name: Option<String>,
    pub comment: Option<String>,
    pub tv_system: Option<UnifTvSystem>,
    pub mirroring: Option<UnifMirroring>,
    pub has_battery: bool,
    pub uses_chr_ram: bool,
    pub controllers: u8,
    pub dumper: Option<DumperInfo>,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub prg_crc: Vec<u32>,
    pub chr_crc: Vec<u32>,
    pub wram_size: u32,
}

#[derive(Debug, Error)]
pub enum UnifError {
    #[error("Invalid UNIF magic number")]
    InvalidMagic,

    #[error("Missing required MAPR chunk")]
    MissingBoard,

    #[error("Missing PRG-ROM data")]
    MissingPrg,

    #[error("Invalid chunk: {0}")]
    InvalidChunk(String),

    #[error("File too small")]
    FileTooSmall,

    #[error("CRC mismatch for {chunk}: expected {expected:08X}, got {actual:08X}")]
    CrcMismatch {
        chunk: String,
        expected: u32,
        actual: u32,
    },
}
```

### Parser Implementation

```rust
impl UnifRom {
    pub fn load(data: &[u8]) -> Result<Self, UnifError> {
        if data.len() < 32 {
            return Err(UnifError::FileTooSmall);
        }

        // Validate magic
        if &data[0..4] != b"UNIF" {
            return Err(UnifError::InvalidMagic);
        }

        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

        // Parse chunks
        let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
        let mut offset = 32;

        while offset + 8 <= data.len() {
            let id = String::from_utf8_lossy(&data[offset..offset + 4]).to_string();
            let length = u32::from_le_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]) as usize;

            offset += 8;

            if offset + length > data.len() {
                break;
            }

            chunks.insert(id, data[offset..offset + length].to_vec());
            offset += length;
        }

        // Required: Board name
        let board = chunks.get("MAPR")
            .map(|d| Self::parse_string(d))
            .ok_or(UnifError::MissingBoard)?;

        // Collect PRG-ROM
        let mut prg_rom = Vec::new();
        let mut prg_crc = Vec::new();
        for i in 0..16 {
            let chunk_id = format!("PRG{:X}", i);
            if let Some(data) = chunks.get(&chunk_id) {
                prg_rom.extend_from_slice(data);
            }
            let crc_id = format!("PCK{:X}", i);
            if let Some(data) = chunks.get(&crc_id) {
                if data.len() >= 4 {
                    prg_crc.push(u32::from_le_bytes([data[0], data[1], data[2], data[3]]));
                }
            }
        }

        if prg_rom.is_empty() {
            return Err(UnifError::MissingPrg);
        }

        // Collect CHR-ROM
        let mut chr_rom = Vec::new();
        let mut chr_crc = Vec::new();
        for i in 0..16 {
            let chunk_id = format!("CHR{:X}", i);
            if let Some(data) = chunks.get(&chunk_id) {
                chr_rom.extend_from_slice(data);
            }
            let crc_id = format!("CCK{:X}", i);
            if let Some(data) = chunks.get(&crc_id) {
                if data.len() >= 4 {
                    chr_crc.push(u32::from_le_bytes([data[0], data[1], data[2], data[3]]));
                }
            }
        }

        // Optional fields
        let name = chunks.get("NAME").map(|d| Self::parse_string(d));
        let comment = chunks.get("READ").map(|d| Self::parse_string(d));

        let tv_system = chunks.get("TVCI").map(|d| {
            match d.first() {
                Some(0) => UnifTvSystem::Ntsc,
                Some(1) => UnifTvSystem::Pal,
                Some(2) => UnifTvSystem::Dual,
                _ => UnifTvSystem::Ntsc,
            }
        });

        let mirroring = chunks.get("MIRR").map(|d| {
            match d.first() {
                Some(0) => UnifMirroring::Horizontal,
                Some(1) => UnifMirroring::Vertical,
                Some(2) => UnifMirroring::SingleScreen0,
                Some(3) => UnifMirroring::SingleScreen1,
                Some(4) => UnifMirroring::FourScreen,
                Some(5) => UnifMirroring::MapperControlled,
                _ => UnifMirroring::Horizontal,
            }
        });

        let has_battery = chunks.get("BATR")
            .and_then(|d| d.first())
            .map(|&b| b != 0)
            .unwrap_or(false);

        let uses_chr_ram = chunks.get("VROR")
            .and_then(|d| d.first())
            .map(|&b| b != 0)
            .unwrap_or(chr_rom.is_empty());

        let controllers = chunks.get("CTRL")
            .and_then(|d| d.first())
            .copied()
            .unwrap_or(0x01);

        let dumper = chunks.get("DINF").and_then(|d| Self::parse_dumper(d));

        let wram_size = chunks.get("WRAM")
            .filter(|d| d.len() >= 4)
            .map(|d| u32::from_le_bytes([d[0], d[1], d[2], d[3]]))
            .unwrap_or(0);

        Ok(UnifRom {
            version,
            board,
            name,
            comment,
            tv_system,
            mirroring,
            has_battery,
            uses_chr_ram,
            controllers,
            dumper,
            prg_rom,
            chr_rom,
            prg_crc,
            chr_crc,
            wram_size,
        })
    }

    fn parse_string(data: &[u8]) -> String {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        String::from_utf8_lossy(&data[..end]).to_string()
    }

    fn parse_dumper(data: &[u8]) -> Option<DumperInfo> {
        if data.len() < 204 {
            return None;
        }

        Some(DumperInfo {
            name: Self::parse_string(&data[0..100]),
            day: data[100],
            month: data[101],
            year: u16::from_le_bytes([data[102], data[103]]),
            agent: Self::parse_string(&data[104..204]),
        })
    }

    /// Verify CRC checksums
    pub fn verify_checksums(&self) -> Result<(), UnifError> {
        // Verify PRG CRCs
        let mut prg_offset = 0;
        for (i, &expected_crc) in self.prg_crc.iter().enumerate() {
            // Find the chunk size (assume uniform chunks for simplicity)
            let chunk_size = self.prg_rom.len() / self.prg_crc.len().max(1);
            if prg_offset + chunk_size <= self.prg_rom.len() {
                let actual_crc = crc32fast::hash(&self.prg_rom[prg_offset..prg_offset + chunk_size]);
                if actual_crc != expected_crc {
                    return Err(UnifError::CrcMismatch {
                        chunk: format!("PRG{:X}", i),
                        expected: expected_crc,
                        actual: actual_crc,
                    });
                }
                prg_offset += chunk_size;
            }
        }

        Ok(())
    }
}
```

### Board Name to Mapper Conversion

```rust
impl UnifRom {
    /// Convert UNIF board name to iNES mapper number
    pub fn to_mapper(&self) -> Option<u16> {
        Some(match self.board.as_str() {
            // Nintendo boards
            "NES-NROM-128" | "NES-NROM-256" | "HVC-NROM" => 0,
            "NES-CNROM" | "HVC-CNROM" => 3,
            "NES-UNROM" | "HVC-UNROM" | "NES-UOROM" => 2,
            "NES-SLROM" | "NES-SNROM" | "NES-SGROM" |
            "NES-SKROM" | "NES-SOROM" | "NES-SUROM" |
            "HVC-SLROM" | "HVC-SNROM" => 1,
            "NES-TLROM" | "NES-TSROM" | "NES-TKROM" |
            "NES-TGROM" | "NES-TR1ROM" | "NES-TVROM" |
            "HVC-TLROM" | "HVC-TSROM" => 4,
            "NES-EKROM" | "NES-ELROM" | "NES-ETROM" |
            "NES-EWROM" | "HVC-EKROM" => 5,
            "NES-AOROM" | "HVC-AOROM" => 7,
            "NES-PNROM" | "NES-PEEOROM" => 9,
            "NES-FKROM" => 10,

            // Konami
            "KONAMI-VRC-1" => 75,
            "KONAMI-VRC-2" => 22,
            "KONAMI-VRC-3" => 73,
            "KONAMI-VRC-4" => 21,
            "KONAMI-VRC-6" => 24,
            "KONAMI-VRC-7" => 85,

            // Namco
            "NAMCOT-163" | "NAMCOT-175" | "NAMCOT-340" => 19,

            // Sunsoft
            "SUNSOFT-1" => 184,
            "SUNSOFT-2" => 93,
            "SUNSOFT-3" => 67,
            "SUNSOFT-4" => 68,
            "SUNSOFT-5B" | "SUNSOFT-FME-07" => 69,

            // Jaleco
            "JALECO-JF-01" | "JALECO-JF-02" => 0,
            "JALECO-JF-05" | "JALECO-JF-06" | "JALECO-JF-07" => 87,
            "JALECO-JF-11" | "JALECO-JF-12" | "JALECO-JF-14" => 140,

            // Bandai
            "BANDAI-FCG-1" | "BANDAI-FCG-2" => 16,
            "BANDAI-74*161/161/32" => 70,

            // Irem
            "IREM-G101" => 32,
            "IREM-H3001" => 65,
            "IREM-LROG017" => 77,

            // Taito
            "TAITO-TC0190FMC" | "TAITO-TC0350FMR" => 33,
            "TAITO-TC0690" => 48,
            "TAITO-X1-005" => 80,
            "TAITO-X1-017" => 82,

            // Color Dreams
            "COLORDREAMS-74*377" => 11,

            // NINA
            "AVE-NINA-01" => 34,
            "AVE-NINA-03" | "AVE-NINA-06" => 79,

            // Camerica
            "CAMERICA-BF9093" | "CAMERICA-BF9097" => 71,
            "CAMERICA-GOLDENFIVE" => 104,

            // Other unlicensed
            "AGCI-47516" => 234,
            "SACHEN-8259A" => 141,
            "SACHEN-8259B" => 138,
            "SACHEN-8259C" => 139,
            "SACHEN-8259D" => 137,
            "TENGEN-800032" => 64,
            "TENGEN-800037" => 158,

            _ => return None,
        })
    }

    /// Get submapper based on board variant
    pub fn to_submapper(&self) -> u8 {
        match self.board.as_str() {
            // MMC1 variants
            "NES-SNROM" => 0,
            "NES-SOROM" => 2,
            "NES-SUROM" => 1,
            "NES-SXROM" => 3,

            // MMC3 variants
            "NES-TSROM" => 0,
            "NES-TKROM" => 0,
            "NES-TQROM" => 0,
            "NES-HKROM" => 1,  // MMC6

            // VRC variants
            "KONAMI-VRC-2A" => 0,
            "KONAMI-VRC-2B" => 1,
            "KONAMI-VRC-4A" => 0,
            "KONAMI-VRC-4B" => 1,
            "KONAMI-VRC-4C" => 2,
            "KONAMI-VRC-4D" => 3,
            "KONAMI-VRC-4E" => 4,
            "KONAMI-VRC-4F" => 5,

            _ => 0,
        }
    }
}
```

---

## Writing UNIF Files

```rust
impl UnifRom {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Header
        data.extend_from_slice(b"UNIF");
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&[0u8; 24]);  // Reserved

        // MAPR chunk
        Self::write_chunk(&mut data, b"MAPR", self.board.as_bytes());

        // NAME chunk
        if let Some(ref name) = self.name {
            Self::write_chunk(&mut data, b"NAME", name.as_bytes());
        }

        // PRG chunks
        let prg_chunk_size = 0x4000;  // 16KB chunks
        let prg_chunks = (self.prg_rom.len() + prg_chunk_size - 1) / prg_chunk_size;
        for i in 0..prg_chunks.min(16) {
            let start = i * prg_chunk_size;
            let end = ((i + 1) * prg_chunk_size).min(self.prg_rom.len());
            let chunk_id = format!("PRG{:X}", i);
            Self::write_chunk(&mut data, chunk_id.as_bytes(), &self.prg_rom[start..end]);
        }

        // CHR chunks
        if !self.chr_rom.is_empty() {
            let chr_chunk_size = 0x2000;  // 8KB chunks
            let chr_chunks = (self.chr_rom.len() + chr_chunk_size - 1) / chr_chunk_size;
            for i in 0..chr_chunks.min(16) {
                let start = i * chr_chunk_size;
                let end = ((i + 1) * chr_chunk_size).min(self.chr_rom.len());
                let chunk_id = format!("CHR{:X}", i);
                Self::write_chunk(&mut data, chunk_id.as_bytes(), &self.chr_rom[start..end]);
            }
        }

        // MIRR chunk
        if let Some(mirr) = self.mirroring {
            let mirr_byte = match mirr {
                UnifMirroring::Horizontal => 0,
                UnifMirroring::Vertical => 1,
                UnifMirroring::SingleScreen0 => 2,
                UnifMirroring::SingleScreen1 => 3,
                UnifMirroring::FourScreen => 4,
                UnifMirroring::MapperControlled => 5,
            };
            Self::write_chunk(&mut data, b"MIRR", &[mirr_byte]);
        }

        // BATR chunk
        if self.has_battery {
            Self::write_chunk(&mut data, b"BATR", &[1]);
        }

        // TVCI chunk
        if let Some(tv) = self.tv_system {
            let tv_byte = match tv {
                UnifTvSystem::Ntsc => 0,
                UnifTvSystem::Pal => 1,
                UnifTvSystem::Dual => 2,
            };
            Self::write_chunk(&mut data, b"TVCI", &[tv_byte]);
        }

        // VROR chunk (CHR-RAM)
        if self.uses_chr_ram {
            Self::write_chunk(&mut data, b"VROR", &[1]);
        }

        data
    }

    fn write_chunk(data: &mut Vec<u8>, id: &[u8], content: &[u8]) {
        let mut chunk_id = [0u8; 4];
        chunk_id[..id.len().min(4)].copy_from_slice(&id[..id.len().min(4)]);
        data.extend_from_slice(&chunk_id);
        data.extend_from_slice(&(content.len() as u32).to_le_bytes());
        data.extend_from_slice(content);
    }
}
```

---

## Converting Between Formats

### UNIF to iNES

```rust
use super::ines::{INesHeader, INesRom, Mirroring};

impl UnifRom {
    /// Convert UNIF to iNES format
    pub fn to_ines(&self) -> Result<INesRom, UnifError> {
        let mapper = self.to_mapper().ok_or(
            UnifError::InvalidChunk(format!("Unknown board: {}", self.board))
        )?;

        let mirroring = match self.mirroring {
            Some(UnifMirroring::Horizontal) => Mirroring::Horizontal,
            Some(UnifMirroring::Vertical) => Mirroring::Vertical,
            Some(UnifMirroring::FourScreen) => Mirroring::FourScreen,
            _ => Mirroring::Horizontal,
        };

        let header = INesHeader {
            prg_rom_size: self.prg_rom.len(),
            chr_rom_size: self.chr_rom.len(),
            mapper: mapper as u8,
            mirroring,
            has_battery: self.has_battery,
            has_trainer: false,
            is_vs_unisystem: false,
            is_playchoice: false,
            tv_system: match self.tv_system {
                Some(UnifTvSystem::Pal) => super::ines::TvSystem::Pal,
                Some(UnifTvSystem::Dual) => super::ines::TvSystem::DualCompatible,
                _ => super::ines::TvSystem::Ntsc,
            },
            prg_ram_size: self.wram_size as usize,
        };

        let chr_ram = if self.uses_chr_ram || self.chr_rom.is_empty() {
            Some(vec![0u8; 8192])
        } else {
            None
        };

        Ok(INesRom {
            header,
            trainer: None,
            prg_rom: self.prg_rom.clone(),
            chr_rom: self.chr_rom.clone(),
            chr_ram,
        })
    }
}
```

---

## Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_minimal_unif() -> Vec<u8> {
        let mut data = Vec::new();

        // Header
        data.extend_from_slice(b"UNIF");
        data.extend_from_slice(&7u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 24]);

        // MAPR chunk
        data.extend_from_slice(b"MAPR");
        data.extend_from_slice(&12u32.to_le_bytes());
        data.extend_from_slice(b"NES-NROM-256");

        // PRG0 chunk (32KB)
        data.extend_from_slice(b"PRG0");
        data.extend_from_slice(&0x8000u32.to_le_bytes());
        data.extend_from_slice(&vec![0xEA; 0x8000]);  // NOP sled

        // CHR0 chunk (8KB)
        data.extend_from_slice(b"CHR0");
        data.extend_from_slice(&0x2000u32.to_le_bytes());
        data.extend_from_slice(&vec![0x00; 0x2000]);

        data
    }

    #[test]
    fn test_parse_unif() {
        let data = create_minimal_unif();
        let rom = UnifRom::load(&data).unwrap();

        assert_eq!(rom.version, 7);
        assert_eq!(rom.board, "NES-NROM-256");
        assert_eq!(rom.prg_rom.len(), 0x8000);
        assert_eq!(rom.chr_rom.len(), 0x2000);
    }

    #[test]
    fn test_mapper_conversion() {
        let data = create_minimal_unif();
        let rom = UnifRom::load(&data).unwrap();

        assert_eq!(rom.to_mapper(), Some(0));
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = create_minimal_unif();
        data[0] = 0x00;

        assert!(matches!(UnifRom::load(&data), Err(UnifError::InvalidMagic)));
    }

    #[test]
    fn test_missing_mapr() {
        let mut data = Vec::new();
        data.extend_from_slice(b"UNIF");
        data.extend_from_slice(&7u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 24]);
        data.extend_from_slice(b"PRG0");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 16]);

        assert!(matches!(UnifRom::load(&data), Err(UnifError::MissingBoard)));
    }

    #[test]
    fn test_roundtrip() {
        let original = create_minimal_unif();
        let rom = UnifRom::load(&original).unwrap();
        let written = rom.to_bytes();
        let reparsed = UnifRom::load(&written).unwrap();

        assert_eq!(rom.board, reparsed.board);
        assert_eq!(rom.prg_rom.len(), reparsed.prg_rom.len());
    }

    #[test]
    fn test_mmc1_board() {
        let mut data = create_minimal_unif();
        // Change board name
        data[36..48].copy_from_slice(b"NES-SNROM\0\0\0");

        let rom = UnifRom::load(&data).unwrap();
        assert_eq!(rom.to_mapper(), Some(1));
    }
}
```

---

## References

- [NESdev Wiki: UNIF](https://www.nesdev.org/wiki/UNIF)
- [UNIF Specification (original)](http://www.romhacking.net/documents/[469]unif.txt)
- [Board Name Database](https://www.nesdev.org/wiki/UNIF#Known_Boards)

---

## See Also

- [INES_FORMAT.md](INES_FORMAT.md) - iNES format (preferred)
- [NES20_FORMAT.md](NES20_FORMAT.md) - NES 2.0 format (preferred for modern use)
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mapper implementations
