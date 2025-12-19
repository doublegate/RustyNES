# [Milestone 4] Sprint 4.4: Mappers 2 & 3 (UxROM, CNROM)

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~4 days
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 4.3 (MMC1) must be complete

---

## Overview

Implement Mapper 2 (UxROM) and Mapper 3 (CNROM), two simple but popular NES mappers:

- **Mapper 2 (UxROM)**: Switchable 16KB PRG-ROM banks, fixed 8KB CHR-ROM/RAM, no mirroring control. Used by Mega Man, Castlevania, Contra.
- **Mapper 3 (CNROM)**: Fixed 16KB or 32KB PRG-ROM, switchable 8KB CHR-ROM banks. Used by Solomon's Key, Arkanoid, Cybernoid.

Both mappers are simpler than MMC1 but widely used. UxROM has bus conflicts (important for accuracy).

---

## Acceptance Criteria

### Mapper 2 (UxROM)
- [ ] Switchable 16KB PRG-ROM banks at $8000-$BFFF
- [ ] Fixed last 16KB PRG-ROM bank at $C000-$FFFF
- [ ] 8KB CHR-ROM or CHR-RAM (no banking)
- [ ] Bus conflicts (write value must match ROM data)
- [ ] Horizontal or vertical mirroring (fixed by hardware)

### Mapper 3 (CNROM)
- [ ] Fixed PRG-ROM (16KB or 32KB)
- [ ] Switchable 8KB CHR-ROM banks
- [ ] Bank select via any write to $8000-$FFFF
- [ ] Horizontal or vertical mirroring (fixed by hardware)

### Both
- [ ] Zero unsafe code
- [ ] Unit tests for all features
- [ ] Integration tests with real games

---

## Tasks

### 4.4.1 UxROM (Mapper 2) Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Create the Mapper2 struct for UxROM with switchable PRG banks and fixed CHR memory.

**Files:**

- `crates/rustynes-mappers/src/mapper2.rs` - UxROM implementation

**Subtasks:**

- [ ] Define Mapper2 struct
- [ ] Store PRG-ROM data (128KB, 256KB typical)
- [ ] Store CHR-ROM or CHR-RAM (8KB, no banking)
- [ ] Current PRG bank register
- [ ] Fixed mirroring mode

**Implementation:**

```rust
use crate::mapper::{Mapper, Mirroring};

pub struct Mapper2 {
    prg_rom: Vec<u8>,
    chr_memory: Vec<u8>,
    chr_is_ram: bool,
    prg_bank: u8,        // Current switchable bank at $8000
    mirroring: Mirroring,
}

impl Mapper2 {
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
            chr_is_ram,
            prg_bank: 0,
            mirroring,
        }
    }
}
```

---

### 4.4.2 UxROM PRG Banking

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement PRG-ROM banking with switchable low bank and fixed high bank.

**Files:**

- `crates/rustynes-mappers/src/mapper2.rs` - PRG read/write

**Subtasks:**

- [ ] $8000-$BFFF: Switchable 16KB bank
- [ ] $C000-$FFFF: Fixed to last 16KB bank
- [ ] Write to $8000-$FFFF sets bank number
- [ ] Bus conflicts: Write value must match ROM data at address

**Implementation:**

```rust
impl Mapper for Mapper2 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                // Switchable bank
                let bank = self.prg_bank as usize;
                let num_banks = self.prg_rom.len() / 0x4000;
                let offset = (addr - 0x8000) as usize;
                let prg_addr = (bank * 0x4000 + offset) % self.prg_rom.len();
                self.prg_rom[prg_addr]
            }
            0xC000..=0xFFFF => {
                // Fixed last bank
                let num_banks = self.prg_rom.len() / 0x4000;
                let last_bank = num_banks - 1;
                let offset = (addr - 0xC000) as usize;
                let prg_addr = last_bank * 0x4000 + offset;
                self.prg_rom[prg_addr]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflicts: Check that write value matches ROM
            let rom_val = self.read_prg(addr);
            if val == rom_val || cfg!(not(feature = "strict_bus_conflicts")) {
                self.prg_bank = val;
            }
            // If values don't match, write is ignored (bus conflict)
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_memory[(addr & 0x1FFF) as usize]
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram {
            self.chr_memory[(addr & 0x1FFF) as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        false
    }
}
```

---

### 4.4.3 CNROM (Mapper 3) Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Create the Mapper3 struct for CNROM with fixed PRG-ROM and switchable CHR-ROM banks.

**Files:**

- `crates/rustynes-mappers/src/mapper3.rs` - CNROM implementation

**Subtasks:**

- [ ] Define Mapper3 struct
- [ ] Store PRG-ROM data (16KB or 32KB)
- [ ] Store CHR-ROM data (16KB, 32KB typical)
- [ ] Current CHR bank register
- [ ] Fixed mirroring mode

**Implementation:**

```rust
use crate::mapper::{Mapper, Mirroring};

pub struct Mapper3 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Mapper3 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_rom,
            chr_bank: 0,
            mirroring,
        }
    }
}
```

---

### 4.4.4 CNROM CHR Banking

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement CHR-ROM banking with switchable 8KB banks.

**Files:**

- `crates/rustynes-mappers/src/mapper3.rs` - CHR read/write

**Subtasks:**

- [ ] Fixed PRG-ROM (16KB mirrored or 32KB)
- [ ] Switchable CHR-ROM (8KB banks)
- [ ] Write to $8000-$FFFF sets CHR bank
- [ ] No bus conflicts (CNROM uses discrete logic)

**Implementation:**

```rust
impl Mapper for Mapper3 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                if self.prg_rom.len() == 0x4000 {
                    // 16KB: Mirror at $C000
                    self.prg_rom[offset & 0x3FFF]
                } else {
                    // 32KB: Direct access
                    self.prg_rom[offset & 0x7FFF]
                }
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, val: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Lower 2 bits (or more) select CHR bank
            self.chr_bank = val & 0x03; // Some carts use more bits
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let bank = self.chr_bank as usize;
        let num_banks = self.chr_rom.len() / 0x2000;
        let offset = (addr & 0x1FFF) as usize;
        let chr_addr = (bank * 0x2000 + offset) % self.chr_rom.len();
        self.chr_rom[chr_addr]
    }

    fn write_chr(&mut self, _addr: u16, _val: u8) {
        // CNROM uses CHR-ROM only (read-only)
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        false
    }
}
```

---

### 4.4.5 Mapper Registration

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 30 minutes

**Description:**
Register Mappers 2 and 3 in the mapper factory.

**Files:**

- `crates/rustynes-mappers/src/lib.rs` - Factory registration
- `crates/rustynes-mappers/src/mapper2.rs` - Module declaration
- `crates/rustynes-mappers/src/mapper3.rs` - Module declaration

**Subtasks:**

- [ ] Declare mapper2 and mapper3 modules
- [ ] Add to mapper factory match statement
- [ ] Export Mapper2 and Mapper3 structs

**Implementation:**

```rust
// crates/rustynes-mappers/src/lib.rs

mod mapper;
mod mapper0;
mod mapper1;
mod mapper2;
mod mapper3;

pub use mapper::{Mapper, Mirroring};
pub use mapper0::Mapper0;
pub use mapper1::Mapper1;
pub use mapper2::Mapper2;
pub use mapper3::Mapper3;

use crate::rom::Rom;

pub fn create_mapper(rom: Rom) -> Result<Box<dyn Mapper>, String> {
    match rom.mapper_number {
        0 => Ok(Box::new(Mapper0::new(
            rom.prg_rom,
            rom.chr_rom,
            rom.mirroring,
        ))),
        1 => Ok(Box::new(Mapper1::new(
            rom.prg_rom,
            rom.chr_rom,
            rom.mirroring,
        ))),
        2 => Ok(Box::new(Mapper2::new(
            rom.prg_rom,
            rom.chr_rom,
            rom.mirroring,
        ))),
        3 => Ok(Box::new(Mapper3::new(
            rom.prg_rom,
            rom.chr_rom,
            rom.mirroring,
        ))),
        _ => Err(format!("Unsupported mapper: {}", rom.mapper_number)),
    }
}
```

---

### 4.4.6 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for both mappers.

**Files:**

- `crates/rustynes-mappers/src/mapper2.rs` - UxROM tests
- `crates/rustynes-mappers/src/mapper3.rs` - CNROM tests

**Subtasks:**

- [ ] Test UxROM bank switching
- [ ] Test UxROM fixed last bank
- [ ] Test UxROM bus conflicts
- [ ] Test CNROM CHR bank switching
- [ ] Test CNROM PRG mirroring (16KB)

**Tests:**

```rust
// Mapper 2 (UxROM) Tests
#[cfg(test)]
mod tests_mapper2 {
    use super::*;

    #[test]
    fn test_uxrom_prg_switching() {
        let mut prg_rom = vec![0; 0x20000]; // 128KB = 8 banks
        for i in 0..8 {
            prg_rom[i * 0x4000] = i as u8; // Mark each bank
        }
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper2::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Switch to bank 3
        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.read_prg(0x8000), 3);

        // Last bank should be fixed
        assert_eq!(mapper.read_prg(0xC000), 7);
    }

    #[test]
    fn test_uxrom_fixed_last_bank() {
        let mut prg_rom = vec![0; 0x10000]; // 64KB = 4 banks
        for i in 0..4 {
            prg_rom[i * 0x4000] = i as u8;
        }
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper2::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Switch lower bank
        mapper.write_prg(0x8000, 0);
        assert_eq!(mapper.read_prg(0x8000), 0);
        assert_eq!(mapper.read_prg(0xC000), 3); // Always bank 3

        mapper.write_prg(0x8000, 2);
        assert_eq!(mapper.read_prg(0x8000), 2);
        assert_eq!(mapper.read_prg(0xC000), 3); // Still bank 3
    }

    #[test]
    #[cfg(feature = "strict_bus_conflicts")]
    fn test_uxrom_bus_conflicts() {
        let mut prg_rom = vec![0; 0x8000]; // 32KB = 2 banks
        prg_rom[0] = 0x01; // Bank switch value at $8000
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper2::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Valid write (value matches ROM)
        mapper.write_prg(0x8000, 0x01);
        assert_eq!(mapper.prg_bank, 0x01);

        // Invalid write (value doesn't match ROM)
        mapper.write_prg(0x8000, 0x02);
        assert_eq!(mapper.prg_bank, 0x01); // Ignored due to bus conflict
    }

    #[test]
    fn test_uxrom_chr_ram() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![]; // CHR-RAM
        let mut mapper = Mapper2::new(prg_rom, chr_rom, Mirroring::Horizontal);

        mapper.write_chr(0x0000, 0x42);
        assert_eq!(mapper.read_chr(0x0000), 0x42);
    }
}

// Mapper 3 (CNROM) Tests
#[cfg(test)]
mod tests_mapper3 {
    use super::*;

    #[test]
    fn test_cnrom_chr_switching() {
        let prg_rom = vec![0; 0x8000]; // 32KB
        let mut chr_rom = vec![0; 0x8000]; // 32KB = 4 banks
        for i in 0..4 {
            chr_rom[i * 0x2000] = i as u8;
        }
        let mut mapper = Mapper3::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Switch to bank 0
        mapper.write_prg(0x8000, 0);
        assert_eq!(mapper.read_chr(0x0000), 0);

        // Switch to bank 2
        mapper.write_prg(0x8000, 2);
        assert_eq!(mapper.read_chr(0x0000), 2);

        // Switch to bank 3
        mapper.write_prg(0x8000, 3);
        assert_eq!(mapper.read_chr(0x0000), 3);
    }

    #[test]
    fn test_cnrom_prg_16kb_mirror() {
        let mut prg_rom = vec![0; 0x4000]; // 16KB
        prg_rom[0] = 0x42;
        prg_rom[0x3FFF] = 0xFF;
        let chr_rom = vec![0; 0x2000];
        let mapper = Mapper3::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // $8000-$BFFF
        assert_eq!(mapper.read_prg(0x8000), 0x42);
        assert_eq!(mapper.read_prg(0xBFFF), 0xFF);

        // $C000-$FFFF (mirrored)
        assert_eq!(mapper.read_prg(0xC000), 0x42);
        assert_eq!(mapper.read_prg(0xFFFF), 0xFF);
    }

    #[test]
    fn test_cnrom_prg_32kb() {
        let mut prg_rom = vec![0; 0x8000]; // 32KB
        prg_rom[0] = 0x01;
        prg_rom[0x7FFF] = 0xFF;
        let chr_rom = vec![0; 0x2000];
        let mapper = Mapper3::new(prg_rom, chr_rom, Mirroring::Horizontal);

        assert_eq!(mapper.read_prg(0x8000), 0x01);
        assert_eq!(mapper.read_prg(0xFFFF), 0xFF);
    }

    #[test]
    fn test_cnrom_chr_readonly() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0x55; 0x2000];
        let mut mapper = Mapper3::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Writes should be ignored
        mapper.write_chr(0x0000, 0xAA);
        assert_eq!(mapper.read_chr(0x0000), 0x55);
    }
}
```

---

## Dependencies

**Required:**

- Sprint 4.3 complete (MMC1 implementation)

**Blocks:**

- Sprint 4.5: MMC3 implementation

---

## Related Documentation

- [Mapper Overview](../../docs/mappers/MAPPER_OVERVIEW.md)
- [UxROM Specification](../../docs/mappers/MAPPER_UXROM.md)
- [CNROM Specification](../../docs/mappers/MAPPER_CNROM.md)
- [NESdev Wiki - UxROM](https://www.nesdev.org/wiki/UxROM)
- [NESdev Wiki - CNROM](https://www.nesdev.org/wiki/CNROM)

---

## Technical Notes

### Mapper 2 (UxROM)

**Memory Map:**
```
$8000-$BFFF: Switchable 16KB PRG-ROM bank
$C000-$FFFF: Fixed to last 16KB PRG-ROM bank
$0000-$1FFF: 8KB CHR-ROM or CHR-RAM (no banking)
```

**Bank Switching:**
- Any write to $8000-$FFFF sets PRG bank number
- Only lower bits used (depends on ROM size: 128KB = 3 bits, 256KB = 4 bits)

**Bus Conflicts:**
- UxROM uses discrete logic with no bus conflict avoidance
- Write value must match ROM data at write address
- Games carefully place bank switch values in ROM at correct addresses
- Example: To switch to bank 3, write 3 to address containing 3

### Mapper 3 (CNROM)

**Memory Map:**
```
$8000-$FFFF: Fixed 16KB or 32KB PRG-ROM (16KB mirrored)
$0000-$1FFF: Switchable 8KB CHR-ROM bank
```

**Bank Switching:**
- Any write to $8000-$FFFF sets CHR bank number
- Lower 2 bits = bank number (some carts use more bits)
- No bus conflicts (discrete logic avoids this)

**Variants:**
- Some CNROM variants support up to 4 CHR banks (2 bits)
- Security chip variants exist but are rare

---

## Test Requirements

### Mapper 2 (UxROM)
- [ ] Unit tests for PRG bank switching
- [ ] Unit tests for fixed last bank
- [ ] Unit tests for bus conflicts (strict mode)
- [ ] Unit tests for CHR-RAM support
- [ ] Integration test with Mega Man, Castlevania, Contra

### Mapper 3 (CNROM)
- [ ] Unit tests for CHR bank switching
- [ ] Unit tests for 16KB PRG mirroring
- [ ] Unit tests for 32KB PRG
- [ ] Unit tests for CHR-ROM read-only
- [ ] Integration test with Solomon's Key, Arkanoid

---

## Performance Targets

- Bank switch: <10 ns
- PRG read: <5 ns
- CHR read: <5 ns
- Memory: <100KB overhead per mapper

---

## Success Criteria

### Mapper 2 (UxROM)
- [ ] PRG bank switching works
- [ ] Last PRG bank fixed at $C000
- [ ] Bus conflicts handled (optional strict mode)
- [ ] CHR-RAM support works
- [ ] All unit tests pass
- [ ] Real games work (Mega Man, Castlevania)

### Mapper 3 (CNROM)
- [ ] CHR bank switching works
- [ ] PRG-ROM mirroring correct (16KB)
- [ ] 32KB PRG-ROM works
- [ ] All unit tests pass
- [ ] Real games work (Solomon's Key, Arkanoid)

### Both
- [ ] Zero unsafe code
- [ ] Documentation complete

---

## Known Games

### Mapper 2 (UxROM) Games

| Game | PRG Size | CHR Type |
|------|----------|----------|
| Mega Man | 128KB | 8KB RAM |
| Castlevania | 128KB | 8KB RAM |
| Contra | 128KB | 8KB RAM |
| Duck Tales | 256KB | 8KB RAM |
| Ghosts 'n Goblins | 128KB | 8KB RAM |

### Mapper 3 (CNROM) Games

| Game | PRG Size | CHR Size |
|------|----------|----------|
| Solomon's Key | 32KB | 32KB ROM |
| Arkanoid | 16KB | 32KB ROM |
| Cybernoid | 32KB | 32KB ROM |
| Paperboy | 32KB | 32KB ROM |
| Gradius | 32KB | 16KB ROM |

---

**Previous Sprint:** [Sprint 4.3: Mapper 1 (MMC1)](M4-S3-MMC1.md)
**Next Sprint:** [Sprint 4.5: Mapper 4 (MMC3)](M4-S5-MMC3.md)
