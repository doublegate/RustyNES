# [Milestone 4] Sprint 4.2: Mapper 0 (NROM)

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~3 days
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 4.1 (Mapper Framework) must be complete

---

## Overview

Implement Mapper 0 (NROM), the simplest NES cartridge mapper with no bank switching. NROM carts have either 16KB or 32KB of PRG-ROM and up to 8KB of CHR-ROM (or CHR-RAM). This mapper is used by early games like Donkey Kong, Mario Bros., and Balloon Fight.

NROM serves as the reference implementation for all other mappers and validates the mapper framework.

---

## Acceptance Criteria

- [ ] Mapper 0 registered in mapper factory
- [ ] 16KB PRG-ROM mirrored at $C000-$FFFF
- [ ] 32KB PRG-ROM fills $8000-$FFFF
- [ ] CHR-ROM (8KB) at $0000-$1FFF
- [ ] CHR-RAM support (when ROM has no CHR-ROM)
- [ ] Horizontal and vertical mirroring
- [ ] Family BASIC keyboard support (optional)
- [ ] Zero unsafe code
- [ ] Unit tests for all configurations
- [ ] Integration tests with real NROM games

---

## Tasks

### 4.2.1 NROM Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Create the Mapper0 struct implementing the Mapper trait with PRG-ROM and CHR-ROM/RAM storage.

**Files:**

- `crates/rustynes-mappers/src/mapper0.rs` - NROM implementation

**Subtasks:**

- [ ] Define Mapper0 struct
- [ ] Store PRG-ROM data (16KB or 32KB)
- [ ] Store CHR-ROM or CHR-RAM (8KB)
- [ ] Store mirroring mode
- [ ] Implement Mapper trait methods

**Implementation:**

```rust
use crate::mapper::{Mapper, Mirroring};

pub struct Mapper0 {
    prg_rom: Vec<u8>,
    chr_memory: Vec<u8>,
    mirroring: Mirroring,
    prg_ram: [u8; 0x2000], // $6000-$7FFF (8KB, optional)
    chr_is_ram: bool,
}

impl Mapper0 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr_memory = if chr_is_ram {
            vec![0; 0x2000] // 8KB CHR-RAM
        } else {
            chr_rom
        };

        Self {
            prg_rom,
            chr_memory,
            mirroring,
            prg_ram: [0; 0x2000],
            chr_is_ram,
        }
    }
}
```

---

### 4.2.2 PRG-ROM Access

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Implement PRG-ROM reading with automatic mirroring for 16KB ROMs.

**Files:**

- `crates/rustynes-mappers/src/mapper0.rs` - PRG read/write

**Subtasks:**

- [ ] Handle $6000-$7FFF PRG-RAM
- [ ] Handle $8000-$BFFF (first 16KB)
- [ ] Handle $C000-$FFFF (mirror or second 16KB)
- [ ] No-op writes to PRG-ROM

**Implementation:**

```rust
impl Mapper for Mapper0 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM (optional, Family BASIC)
                self.prg_ram[(addr - 0x6000) as usize]
            }
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                if self.prg_rom.len() == 0x4000 {
                    // 16KB: Mirror at $C000-$FFFF
                    self.prg_rom[offset & 0x3FFF]
                } else {
                    // 32KB: Direct access
                    self.prg_rom[offset & 0x7FFF]
                }
            }
            _ => 0, // Open bus
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            // PRG-RAM is writable
            self.prg_ram[(addr - 0x6000) as usize] = val;
        }
        // Writes to PRG-ROM are ignored (no bus conflicts on NROM)
    }
}
```

---

### 4.2.3 CHR Memory Access

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Implement CHR-ROM/RAM access with write support for CHR-RAM.

**Files:**

- `crates/rustynes-mappers/src/mapper0.rs` - CHR read/write

**Subtasks:**

- [ ] Read from CHR memory (ROM or RAM)
- [ ] Write to CHR-RAM only (when applicable)
- [ ] Ignore writes to CHR-ROM

**Implementation:**

```rust
impl Mapper for Mapper0 {
    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_memory[(addr & 0x1FFF) as usize]
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram {
            self.chr_memory[(addr & 0x1FFF) as usize] = val;
        }
        // Writes to CHR-ROM are ignored
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        false // NROM has no IRQ
    }
}
```

---

### 4.2.4 Mapper Registration

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 30 minutes

**Description:**
Register Mapper 0 in the mapper factory.

**Files:**

- `crates/rustynes-mappers/src/lib.rs` - Factory registration
- `crates/rustynes-mappers/src/mapper0.rs` - Module declaration

**Subtasks:**

- [ ] Declare mapper0 module
- [ ] Add to mapper factory match statement
- [ ] Export Mapper0 struct

**Implementation:**

```rust
// crates/rustynes-mappers/src/lib.rs

mod mapper;
mod mapper0;

pub use mapper::{Mapper, Mirroring};
pub use mapper0::Mapper0;

use crate::rom::Rom;

pub fn create_mapper(rom: Rom) -> Result<Box<dyn Mapper>, String> {
    match rom.mapper_number {
        0 => Ok(Box::new(Mapper0::new(
            rom.prg_rom,
            rom.chr_rom,
            rom.mirroring,
        ))),
        _ => Err(format!("Unsupported mapper: {}", rom.mapper_number)),
    }
}
```

---

### 4.2.5 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Create comprehensive unit tests for NROM functionality.

**Files:**

- `crates/rustynes-mappers/src/mapper0.rs` - Test module

**Subtasks:**

- [ ] Test 16KB PRG-ROM mirroring
- [ ] Test 32KB PRG-ROM direct access
- [ ] Test CHR-ROM read-only
- [ ] Test CHR-RAM read/write
- [ ] Test PRG-RAM read/write
- [ ] Test mirroring modes

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_16kb_prg_rom_mirroring() {
        let prg_rom = vec![0x42; 0x4000]; // 16KB
        let chr_rom = vec![0; 0x2000];    // 8KB CHR-ROM
        let mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // $8000-$BFFF maps to first 16KB
        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xBFFF), 0x42);

        // $C000-$FFFF mirrors to first 16KB
        assert_eq!(mapper.read_prg(0xC000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0x42);
    }

    #[test]
    fn test_32kb_prg_rom() {
        let mut prg_rom = vec![0; 0x8000]; // 32KB
        prg_rom[0] = 0x01;           // $8000
        prg_rom[0x7FFF] = 0xFF;      // $FFFF
        let chr_rom = vec![0; 0x2000];
        let mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        assert_eq!(mapper.read_prg(0x8000), 0x01);
        assert_eq!(mapper.read_prg(0xFFFF), 0xFF);
    }

    #[test]
    fn test_chr_ram_write() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![]; // Empty = CHR-RAM
        let mut mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        mapper.write_chr(0x0000, 0x42);
        assert_eq!(mapper.read_chr(0x0000), 0x42);

        mapper.write_chr(0x1FFF, 0xFF);
        assert_eq!(mapper.read_chr(0x1FFF), 0xFF);
    }

    #[test]
    fn test_chr_rom_readonly() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![0x55; 0x2000]; // 8KB CHR-ROM
        let mut mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        assert_eq!(mapper.read_chr(0x0000), 0x55);

        // Write should be ignored
        mapper.write_chr(0x0000, 0xAA);
        assert_eq!(mapper.read_chr(0x0000), 0x55);
    }

    #[test]
    fn test_prg_ram() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        mapper.write_prg(0x7FFF, 0xFF);
        assert_eq!(mapper.read_prg(0x7FFF), 0xFF);
    }

    #[test]
    fn test_prg_rom_write_ignored() {
        let mut prg_rom = vec![0; 0x4000];
        prg_rom[0] = 0x42;
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        assert_eq!(mapper.read_prg(0x8000), 0x42);

        // Write should be ignored
        mapper.write_prg(0x8000, 0xFF);
        assert_eq!(mapper.read_prg(0x8000), 0x42);
    }

    #[test]
    fn test_mirroring() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![0; 0x2000];

        let mapper_h = Mapper0::new(prg_rom.clone(), chr_rom.clone(), Mirroring::Horizontal);
        assert_eq!(mapper_h.mirroring(), Mirroring::Horizontal);

        let mapper_v = Mapper0::new(prg_rom, chr_rom, Mirroring::Vertical);
        assert_eq!(mapper_v.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn test_no_irq() {
        let prg_rom = vec![0; 0x4000];
        let chr_rom = vec![0; 0x2000];
        let mapper = Mapper0::new(prg_rom, chr_rom, Mirroring::Horizontal);

        assert!(!mapper.irq_pending());
    }
}
```

---

### 4.2.6 Integration Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Test NROM with real game ROMs and validate correct operation.

**Files:**

- `crates/rustynes-mappers/tests/mapper0_integration.rs` - Integration tests

**Subtasks:**

- [ ] Test loading NROM ROM files
- [ ] Verify PRG-ROM size detection
- [ ] Verify CHR-ROM vs CHR-RAM detection
- [ ] Test with known NROM games (Donkey Kong, Mario Bros)
- [ ] Validate memory map correctness

**Tests:**

```rust
#[cfg(test)]
mod integration_tests {
    use rustynes_mappers::{create_mapper, Rom};
    use std::fs;

    #[test]
    fn test_load_nrom_128() {
        // 16KB PRG-ROM, 8KB CHR-ROM (iNES format)
        let ines_header = [
            0x4E, 0x45, 0x53, 0x1A, // "NES\x1A"
            0x01, // 1 × 16KB PRG-ROM
            0x01, // 1 × 8KB CHR-ROM
            0x00, // Horizontal mirroring, no trainer
            0x00, // Mapper 0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let prg_rom = vec![0x42; 0x4000]; // 16KB
        let chr_rom = vec![0x55; 0x2000]; // 8KB

        let mut rom_data = Vec::new();
        rom_data.extend_from_slice(&ines_header);
        rom_data.extend_from_slice(&prg_rom);
        rom_data.extend_from_slice(&chr_rom);

        let rom = Rom::from_bytes(&rom_data).unwrap();
        let mapper = create_mapper(rom).unwrap();

        // Verify PRG-ROM
        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xC000), 0x42); // Mirrored

        // Verify CHR-ROM
        assert_eq!(mapper.read_chr(0x0000), 0x55);
    }

    #[test]
    fn test_load_nrom_256() {
        // 32KB PRG-ROM, 8KB CHR-ROM
        let ines_header = [
            0x4E, 0x45, 0x53, 0x1A,
            0x02, // 2 × 16KB PRG-ROM = 32KB
            0x01, // 1 × 8KB CHR-ROM
            0x01, // Vertical mirroring
            0x00, // Mapper 0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let prg_rom = vec![0xFF; 0x8000]; // 32KB
        let chr_rom = vec![0xAA; 0x2000]; // 8KB

        let mut rom_data = Vec::new();
        rom_data.extend_from_slice(&ines_header);
        rom_data.extend_from_slice(&prg_rom);
        rom_data.extend_from_slice(&chr_rom);

        let rom = Rom::from_bytes(&rom_data).unwrap();
        let mapper = create_mapper(rom).unwrap();

        assert_eq!(mapper.read_prg(0x8000), 0xFF);
        assert_eq!(mapper.read_prg(0xFFFF), 0xFF);
        assert_eq!(mapper.read_chr(0x0000), 0xAA);
    }

    #[test]
    fn test_nrom_with_chr_ram() {
        // 16KB PRG-ROM, no CHR-ROM (CHR-RAM)
        let ines_header = [
            0x4E, 0x45, 0x53, 0x1A,
            0x01, // 1 × 16KB PRG-ROM
            0x00, // 0 × 8KB CHR-ROM (use CHR-RAM)
            0x00,
            0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let prg_rom = vec![0x42; 0x4000];

        let mut rom_data = Vec::new();
        rom_data.extend_from_slice(&ines_header);
        rom_data.extend_from_slice(&prg_rom);

        let rom = Rom::from_bytes(&rom_data).unwrap();
        let mut mapper = create_mapper(rom).unwrap();

        // CHR-RAM should be writable
        mapper.write_chr(0x0000, 0x99);
        assert_eq!(mapper.read_chr(0x0000), 0x99);
    }
}
```

---

## Dependencies

**Required:**

- Sprint 4.1 complete (Mapper trait, ROM parsing, factory pattern)

**Blocks:**

- Sprint 4.3: MMC1 implementation
- Sprint 4.4: UxROM/CNROM implementation
- All subsequent mapper implementations

---

## Related Documentation

- [Mapper Overview](../../docs/mappers/MAPPER_OVERVIEW.md)
- [NROM Specification](../../docs/mappers/MAPPER_NROM.md)
- [iNES Format](../../docs/formats/INES_FORMAT.md)
- [NESdev Wiki - NROM](https://www.nesdev.org/wiki/NROM)

---

## Technical Notes

### NROM Variants

- **NROM-128**: 16KB PRG-ROM ($8000-$BFFF), mirrored at $C000-$FFFF
- **NROM-256**: 32KB PRG-ROM ($8000-$FFFF), no mirroring

### Memory Map

```
$0000-$1FFF: CHR-ROM or CHR-RAM (8KB, PPU memory)
$6000-$7FFF: PRG-RAM (optional, 8KB, for Family BASIC)
$8000-$BFFF: First 16KB of PRG-ROM
$C000-$FFFF: Last 16KB of PRG-ROM (or mirrored first 16KB)
```

### CHR-ROM vs CHR-RAM

- If iNES header specifies 0 CHR-ROM banks, use 8KB CHR-RAM
- CHR-ROM is read-only; CHR-RAM is read/write
- Some games (Balloon Fight) use CHR-RAM for dynamic graphics

### Family BASIC

- Japanese cartridge with keyboard support
- Uses PRG-RAM at $6000-$7FFF for keyboard input buffer
- Not commonly emulated, but should be supported

### Bus Conflicts

NROM has no bus conflicts because:
- PRG-ROM writes are simply ignored
- No bank switching logic that could conflict

---

## Test Requirements

- [ ] Unit tests for 16KB and 32KB PRG-ROM
- [ ] Unit tests for CHR-ROM (read-only)
- [ ] Unit tests for CHR-RAM (read/write)
- [ ] Unit tests for PRG-RAM (Family BASIC)
- [ ] Unit tests for mirroring modes
- [ ] Integration test with real NROM ROMs
- [ ] Validation with known games (Donkey Kong, Mario Bros, Balloon Fight)

---

## Performance Targets

- PRG-ROM read: <5 ns
- CHR-ROM read: <5 ns
- Memory: <50KB overhead (ROM data not counted)

---

## Success Criteria

- [ ] Mapper 0 correctly identified from iNES header
- [ ] 16KB PRG-ROM mirrored correctly
- [ ] 32KB PRG-ROM accessed without mirroring
- [ ] CHR-ROM read-only
- [ ] CHR-RAM read/write when applicable
- [ ] Horizontal and vertical mirroring work
- [ ] All unit tests pass
- [ ] Integration tests with real ROMs pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

## Known NROM Games

| Game | PRG Size | CHR Type | Mirroring |
|------|----------|----------|-----------|
| Donkey Kong | 16KB | 8KB ROM | Vertical |
| Mario Bros | 16KB | 8KB ROM | Vertical |
| Balloon Fight | 16KB | 8KB RAM | Horizontal |
| Excitebike | 16KB | 8KB ROM | Vertical |
| Ice Climber | 32KB | 8KB ROM | Vertical |
| Duck Hunt | 16KB | 8KB ROM | Horizontal |

---

**Previous Sprint:** [Sprint 4.1: Mapper Framework](M4-S1-MAPPER-FRAMEWORK.md)
**Next Sprint:** [Sprint 4.3: Mapper 1 (MMC1)](M4-S3-MMC1.md)
