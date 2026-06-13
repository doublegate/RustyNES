# [Milestone 5] Sprint 5.4: ROM Loading

**Status:** ✅ COMPLETED
**Started:** December 19, 2025
**Completed:** December 19, 2025
**Duration:** 1 day (part of M5 integration)
**Assignee:** Claude Code / Developer

---

## Overview

Implement robust ROM loading infrastructure supporting iNES and NES 2.0 formats with validation, error handling, and mapper creation. This sprint provides the entry point for users to load and run NES games.

### Goals

- Load ROM from file path
- Load ROM from byte array
- iNES format validation
- NES 2.0 format detection and parsing
- Mapper selection and creation
- Battery-backed SRAM loading/saving
- Comprehensive error handling
- User-friendly error messages
- Zero unsafe code

---

## Acceptance Criteria

- [ ] ROM loading from file path works
- [ ] ROM loading from bytes works
- [ ] iNES header parsing complete
- [ ] NES 2.0 extended header parsing complete
- [ ] Mapper factory creates correct mapper instances
- [ ] Invalid ROM files rejected with clear errors
- [ ] Unsupported mappers reported clearly
- [ ] Battery SRAM loaded if present
- [ ] Battery SRAM saved on demand
- [ ] Comprehensive unit tests
- [ ] Zero unsafe code

---

## Tasks

### Task 1: ROM File Loading

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Implement ROM loading from filesystem and byte arrays.

**Files:**

- `crates/rustynes-core/src/rom_loader.rs` - ROM loading functions

**Subtasks:**

- [ ] Add load_rom_file function taking file path
- [ ] Add load_rom_bytes function taking byte slice
- [ ] Handle file I/O errors
- [ ] Validate file size minimums
- [ ] Read entire file into buffer

**Implementation:**

```rust
use std::fs;
use std::path::Path;
use rustynes_mappers::Rom;

/// Load ROM from file path
pub fn load_rom_file<P: AsRef<Path>>(path: P) -> Result<Rom, RomLoadError> {
    let path = path.as_ref();

    // Check if file exists
    if !path.exists() {
        return Err(RomLoadError::FileNotFound(path.to_path_buf()));
    }

    // Read file contents
    let data = fs::read(path).map_err(|e| RomLoadError::IoError {
        path: path.to_path_buf(),
        error: e,
    })?;

    // Parse ROM from bytes
    load_rom_bytes(&data)
}

/// Load ROM from byte array
pub fn load_rom_bytes(data: &[u8]) -> Result<Rom, RomLoadError> {
    // Validate minimum size (16-byte header)
    if data.len() < 16 {
        return Err(RomLoadError::FileTooSmall {
            size: data.len(),
            minimum: 16,
        });
    }

    // Parse ROM
    Rom::from_bytes(data).map_err(RomLoadError::from)
}

#[derive(Debug, thiserror::Error)]
pub enum RomLoadError {
    #[error("File not found: {0}")]
    FileNotFound(std::path::PathBuf),

    #[error("I/O error reading {path}: {error}")]
    IoError {
        path: std::path::PathBuf,
        error: std::io::Error,
    },

    #[error("File too small: {size} bytes (minimum {minimum})")]
    FileTooSmall { size: usize, minimum: usize },

    #[error("ROM parsing error: {0}")]
    Rom(#[from] rustynes_mappers::RomError),

    #[error("Mapper error: {0}")]
    Mapper(#[from] rustynes_mappers::MapperError),
}
```

---

### Task 2: iNES Header Validation

- **Status:** ⏳ Pending (already implemented in rustynes-mappers)
- **Priority:** High
- **Estimated:** 0 hours (review only)

**Description:**
Validate iNES header format and extract metadata. This is already implemented in rustynes-mappers, but we need to ensure integration.

**Files:**

- `crates/rustynes-mappers/src/rom.rs` - Header parsing (already exists)

**Subtasks:**

- [ ] Verify magic number "NES\x1A"
- [ ] Validate PRG/CHR sizes
- [ ] Parse mapper number
- [ ] Detect NES 2.0 format
- [ ] Handle corrupted headers

**Implementation Note:**
This functionality already exists in rustynes-mappers. Review to ensure:

- Clear error messages for invalid magic number
- Proper NES 2.0 detection (flags7 bits 2-3 == 10b)
- Mapper number calculation (12-bit for NES 2.0)

---

### Task 3: NES 2.0 Format Support

- **Status:** ⏳ Pending (already implemented in rustynes-mappers)
- **Priority:** Medium
- **Estimated:** 0 hours (review only)

**Description:**
Support extended NES 2.0 format features.

**Files:**

- `crates/rustynes-mappers/src/rom.rs` - NES 2.0 parsing (already exists)

**Subtasks:**

- [ ] Parse 12-bit mapper number
- [ ] Parse 4-bit submapper
- [ ] Parse extended PRG/CHR sizes
- [ ] Parse RAM sizes (PRG-RAM, PRG-NVRAM, CHR-RAM, CHR-NVRAM)
- [ ] Parse region (NTSC/PAL/dual)

**Implementation Note:**
Already implemented. Verify integration with Console creation.

---

### Task 4: Mapper Factory Integration

- **Status:** ⏳ Pending (already implemented in rustynes-mappers)
- **Priority:** High
- **Estimated:** 1 hour (integration work)

**Description:**
Integrate mapper factory to create appropriate mapper from ROM.

**Files:**

- `crates/rustynes-core/src/console.rs` - Mapper creation
- `crates/rustynes-mappers/src/factory.rs` - Factory (already exists)

**Subtasks:**

- [ ] Call create_mapper from Rom
- [ ] Handle unsupported mapper errors
- [ ] Provide helpful error messages
- [ ] Log mapper selection

**Implementation:**

```rust
use rustynes_mappers::create_mapper;

impl Console {
    pub fn from_rom(rom: Rom) -> Result<Self, ConsoleError> {
        // Create mapper
        let mapper = create_mapper(rom).map_err(|e| match e {
            MapperError::UnsupportedMapper(num) => {
                ConsoleError::UnsupportedMapper {
                    mapper: num,
                    message: format!(
                        "Mapper {} is not yet implemented. \
                         Supported mappers: 0 (NROM), 1 (MMC1), \
                         2 (UxROM), 3 (CNROM), 4 (MMC3)",
                        num
                    ),
                }
            }
            e => ConsoleError::Mapper(e),
        })?;

        let mirroring = mapper.mirroring();

        // Initialize subsystems
        let ppu = Ppu::new(mirroring);
        let apu = Apu::new();
        let bus = NesBus::new(ppu, apu, mapper);
        let cpu = Cpu::new();

        // Create console
        let mut console = Self {
            cpu,
            bus,
            master_clock: 0,
            frame_count: 0,
            region: Region::Ntsc,
            frame_cycles: 0,
        };

        console.power_on();

        Ok(console)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConsoleError {
    #[error("ROM loading error: {0}")]
    RomLoad(#[from] RomLoadError),

    #[error("Unsupported mapper {mapper}: {message}")]
    UnsupportedMapper { mapper: u16, message: String },

    #[error("Mapper error: {0}")]
    Mapper(#[from] rustynes_mappers::MapperError),
}
```

---

### Task 5: Battery-Backed SRAM Loading

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Load battery-backed SRAM from .sav files if present.

**Files:**

- `crates/rustynes-core/src/sram.rs` - SRAM management

**Subtasks:**

- [ ] Define SRAM save file format
- [ ] Implement load_sram_file
- [ ] Implement save_sram_file
- [ ] Generate .sav filename from ROM path
- [ ] Handle missing SRAM files gracefully
- [ ] Validate SRAM size matches mapper

**Implementation:**

```rust
use std::path::{Path, PathBuf};

/// SRAM manager for battery-backed save files
pub struct SramManager {
    rom_path: Option<PathBuf>,
    save_path: Option<PathBuf>,
}

impl SramManager {
    pub fn new() -> Self {
        Self {
            rom_path: None,
            save_path: None,
        }
    }

    /// Set ROM path and derive save file path
    pub fn set_rom_path<P: AsRef<Path>>(&mut self, rom_path: P) {
        let rom_path = rom_path.as_ref();
        self.rom_path = Some(rom_path.to_path_buf());

        // Generate .sav path (replace .nes with .sav)
        let save_path = rom_path.with_extension("sav");
        self.save_path = Some(save_path);
    }

    /// Load SRAM from .sav file if present
    pub fn load_sram(&self) -> Result<Option<Vec<u8>>, std::io::Error> {
        let save_path = match &self.save_path {
            Some(path) => path,
            None => return Ok(None),
        };

        if !save_path.exists() {
            return Ok(None);
        }

        let data = fs::read(save_path)?;
        Ok(Some(data))
    }

    /// Save SRAM to .sav file
    pub fn save_sram(&self, data: &[u8]) -> Result<(), std::io::Error> {
        let save_path = match &self.save_path {
            Some(path) => path,
            None => return Ok(()), // No path set, skip save
        };

        fs::write(save_path, data)?;
        Ok(())
    }
}

impl Console {
    /// Load battery-backed SRAM if present
    pub fn load_battery_sram(&mut self) -> Result<(), std::io::Error> {
        if let Some(sram_data) = self.sram_manager.load_sram()? {
            if let Some(sram) = self.bus.cartridge.sram_mut() {
                if sram.len() == sram_data.len() {
                    sram.copy_from_slice(&sram_data);
                    log::info!("Loaded battery SRAM: {} bytes", sram_data.len());
                } else {
                    log::warn!(
                        "SRAM size mismatch: expected {}, got {}",
                        sram.len(),
                        sram_data.len()
                    );
                }
            }
        }
        Ok(())
    }

    /// Save battery-backed SRAM
    pub fn save_battery_sram(&self) -> Result<(), std::io::Error> {
        if let Some(sram) = self.bus.cartridge.sram() {
            self.sram_manager.save_sram(sram)?;
            log::info!("Saved battery SRAM: {} bytes", sram.len());
        }
        Ok(())
    }
}
```

---

### Task 6: ROM Information API

- **Status:** ⏳ Pending
- **Priority:** Low
- **Estimated:** 1 hour

**Description:**
Provide API to query ROM metadata for display in UI.

**Files:**

- `crates/rustynes-core/src/console.rs` - ROM info methods

**Subtasks:**

- [ ] Add get_rom_info method
- [ ] Define RomInfo struct
- [ ] Extract PRG/CHR sizes
- [ ] Extract mapper number and name
- [ ] Extract mirroring mode
- [ ] Extract battery flag

**Implementation:**

```rust
#[derive(Debug, Clone)]
pub struct RomInfo {
    pub prg_rom_size: usize,    // In bytes
    pub chr_rom_size: usize,    // In bytes
    pub prg_ram_size: usize,    // In bytes
    pub mapper_number: u16,
    pub submapper: u8,
    pub mapper_name: String,
    pub mirroring: String,
    pub has_battery: bool,
    pub nes2_format: bool,
}

impl Console {
    /// Get ROM metadata
    pub fn rom_info(&self) -> RomInfo {
        let mapper = &self.bus.cartridge;

        RomInfo {
            prg_rom_size: 0, // Get from mapper
            chr_rom_size: 0, // Get from mapper
            prg_ram_size: 0, // Get from mapper
            mapper_number: mapper.mapper_number(),
            submapper: mapper.submapper(),
            mapper_name: Self::mapper_name(mapper.mapper_number()),
            mirroring: format!("{:?}", mapper.mirroring()),
            has_battery: mapper.has_battery(),
            nes2_format: false, // Get from header
        }
    }

    fn mapper_name(mapper_num: u16) -> String {
        match mapper_num {
            0 => "NROM".to_string(),
            1 => "MMC1 (SxROM)".to_string(),
            2 => "UxROM".to_string(),
            3 => "CNROM".to_string(),
            4 => "MMC3 (TxROM)".to_string(),
            n => format!("Mapper {}", n),
        }
    }
}
```

---

### Task 7: Error Messages

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 1 hour

**Description:**
Provide user-friendly error messages for common ROM loading issues.

**Files:**

- `crates/rustynes-core/src/rom_loader.rs` - Error handling

**Subtasks:**

- [ ] File not found error
- [ ] Invalid magic number error
- [ ] Corrupted header error
- [ ] Unsupported mapper error
- [ ] File too small error
- [ ] Mapper-specific error messages

**Implementation:**

```rust
impl fmt::Display for RomLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RomLoadError::FileNotFound(path) => {
                write!(
                    f,
                    "ROM file not found: {}\n\nPlease check that:\n\
                     1. The file path is correct\n\
                     2. The file exists\n\
                     3. You have read permissions",
                    path.display()
                )
            }
            RomLoadError::IoError { path, error } => {
                write!(
                    f,
                    "Failed to read ROM file: {}\nError: {}",
                    path.display(),
                    error
                )
            }
            RomLoadError::FileTooSmall { size, minimum } => {
                write!(
                    f,
                    "ROM file too small: {} bytes (minimum {})\n\
                     This may be a corrupted or incomplete ROM file.",
                    size, minimum
                )
            }
            RomLoadError::Rom(e) => {
                write!(f, "Invalid ROM format: {}", e)
            }
            RomLoadError::Mapper(e) => {
                write!(f, "Mapper error: {}", e)
            }
        }
    }
}
```

---

### Task 8: Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Create comprehensive tests for ROM loading.

**Files:**

- `crates/rustynes-core/src/rom_loader.rs` - Test module

**Subtasks:**

- [ ] Test loading valid iNES ROM
- [ ] Test loading NES 2.0 ROM
- [ ] Test file not found error
- [ ] Test invalid magic number
- [ ] Test unsupported mapper
- [ ] Test corrupted header
- [ ] Test SRAM loading/saving

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_rom(
        prg_size: u8,
        chr_size: u8,
        mapper: u8,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 16]; // Header
        data[0..4].copy_from_slice(b"NES\x1A");
        data[4] = prg_size;
        data[5] = chr_size;
        data[6] = (mapper & 0x0F) << 4; // Mapper low nibble
        data[7] = mapper & 0xF0;        // Mapper high nibble

        // Add PRG-ROM
        data.extend(vec![0u8; (prg_size as usize) * 16384]);

        // Add CHR-ROM
        if chr_size > 0 {
            data.extend(vec![0u8; (chr_size as usize) * 8192]);
        }

        data
    }

    #[test]
    fn test_load_valid_rom() {
        let rom_data = create_test_rom(2, 1, 0); // 32KB PRG, 8KB CHR, NROM
        let result = load_rom_bytes(&rom_data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_magic() {
        let mut rom_data = create_test_rom(1, 1, 0);
        rom_data[0] = b'X'; // Corrupt magic number

        let result = load_rom_bytes(&rom_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_too_small() {
        let rom_data = vec![0u8; 8]; // Less than 16 bytes
        let result = load_rom_bytes(&rom_data);
        assert!(matches!(result, Err(RomLoadError::FileTooSmall { .. })));
    }

    #[test]
    fn test_mapper_creation() {
        // Test all supported mappers
        for mapper_num in [0, 1, 2, 3, 4] {
            let rom_data = create_test_rom(2, 1, mapper_num);
            let rom = load_rom_bytes(&rom_data).unwrap();
            let console = Console::from_rom(rom);
            assert!(console.is_ok());
        }
    }

    #[test]
    fn test_unsupported_mapper() {
        let rom_data = create_test_rom(2, 1, 99); // Unsupported mapper
        let rom = load_rom_bytes(&rom_data).unwrap();
        let result = Console::from_rom(rom);
        assert!(matches!(
            result,
            Err(ConsoleError::UnsupportedMapper { .. })
        ));
    }
}
```

---

## Dependencies

**Required:**

- rustynes-mappers (ROM parsing, mapper factory)
- Sprint 5.3: Console Coordinator (needs Console)
- std::fs (file I/O)
- thiserror = "1.0" (error handling)
- log = "0.4" (logging)

**Blocks:**

- Milestone 6: Desktop GUI (needs ROM loading)
- All game testing

---

## Related Documentation

- [iNES Format](../../../docs/formats/INES_FORMAT.md)
- [NES 2.0 Format](../../../docs/formats/NES20_FORMAT.md)
- [Mapper Overview](../../../docs/mappers/MAPPER_OVERVIEW.md)
- [Core API](../../../docs/api/CORE_API.md)

---

## Technical Notes

### iNES vs NES 2.0

**iNES 1.0:**

- 16-byte header
- 8-bit mapper number
- Basic PRG/CHR sizes (16KB/8KB units)
- Limited to 256 mappers

**NES 2.0:**

- Extended 16-byte header (backwards compatible)
- 12-bit mapper number (4096 mappers)
- 4-bit submapper
- Extended PRG/CHR sizes (exponent-multiplier notation)
- RAM size specifications

### Battery-Backed SRAM

Cartridges with battery-backed SRAM (flag 6 bit 1 = 1) save game progress. The .sav file format is simply raw SRAM contents (typically 8KB).

**Save Path Convention:**

- `game.nes` → `game.sav`
- Stored in same directory as ROM by default
- Configurable via settings

### Mapper Selection

Mapper number determines cartridge hardware:

- 0: NROM (no mapper)
- 1: MMC1 (most common)
- 2: UxROM (Mega Man, Castlevania)
- 3: CNROM (simple CHR banking)
- 4: MMC3 (Super Mario Bros. 3)

---

## Performance Targets

- **ROM loading**: <100 ms for typical ROMs
- **Header parsing**: <1 ms
- **SRAM loading**: <10 ms
- **Mapper creation**: <1 ms

---

## Success Criteria

- [ ] Can load ROM from file path
- [ ] Can load ROM from byte array
- [ ] iNES header parsed correctly
- [ ] NES 2.0 format detected and parsed
- [ ] Mapper created for ROM
- [ ] Invalid ROMs rejected with clear errors
- [ ] Unsupported mappers reported clearly
- [ ] Battery SRAM loaded if present
- [ ] Battery SRAM saved on demand
- [ ] ROM metadata accessible
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] User-friendly error messages
- [ ] Documentation complete

---

**Next Sprint:** [Sprint 5.5: Save States](M5-S5-save-states.md)
