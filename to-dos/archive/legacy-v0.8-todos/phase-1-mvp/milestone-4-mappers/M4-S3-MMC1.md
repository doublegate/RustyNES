# [Milestone 4] Sprint 4.3: Mapper 1 (MMC1)

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week
**Assignee:** Claude Code / Developer
**Dependencies:** Sprint 4.2 (NROM) must be complete

---

## Overview

Implement Mapper 1 (MMC1/SxROM), one of the most common NES mappers used by over 680 games including The Legend of Zelda, Metroid, Mega Man 2, and Final Fantasy. MMC1 features:

- Serial shift register interface (5 writes to configure)
- Switchable PRG-ROM banks (16KB or 32KB)
- Switchable CHR-ROM banks (4KB or 8KB)
- Configurable mirroring (horizontal, vertical, single-screen)
- Optional PRG-RAM with battery backup (save games)

---

## Acceptance Criteria

- [ ] Shift register with 5-bit serial write protocol
- [ ] Four control registers (Control, CHR Bank 0, CHR Bank 1, PRG Bank)
- [ ] PRG-ROM banking modes (16KB switchable, 32KB switchable, 16KB fixed)
- [ ] CHR-ROM banking modes (4KB switchable, 8KB switchable)
- [ ] Configurable mirroring (horizontal, vertical, single-screen low, single-screen high)
- [ ] PRG-RAM enable/disable
- [ ] Write to $8000-$FFFF with bit 7 set resets shift register
- [ ] Zero unsafe code
- [ ] Unit tests for all banking modes
- [ ] Integration tests with real MMC1 games

---

## Tasks

### 4.3.1 MMC1 Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Create the Mapper1 struct with shift register, control registers, and banking state.

**Files:**

- `crates/rustynes-mappers/src/mapper1.rs` - MMC1 implementation

**Subtasks:**

- [ ] Define Mapper1 struct
- [ ] Shift register (5-bit accumulator)
- [ ] Control register ($8000-$9FFF)
- [ ] CHR bank 0 register ($A000-$BFFF)
- [ ] CHR bank 1 register ($C000-$DFFF)
- [ ] PRG bank register ($E000-$FFFF)
- [ ] PRG-ROM/CHR-ROM data storage
- [ ] PRG-RAM (8KB or 32KB with battery backup)

**Implementation:**

```rust
use crate::mapper::{Mapper, Mirroring};

pub struct Mapper1 {
    // ROM data
    prg_rom: Vec<u8>,
    chr_memory: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_is_ram: bool,

    // Shift register
    shift_register: u8,
    write_count: u8,

    // Control register ($8000-$9FFF, internal)
    control: u8,

    // CHR bank registers ($A000-$BFFF, $C000-$DFFF, internal)
    chr_bank_0: u8,
    chr_bank_1: u8,

    // PRG bank register ($E000-$FFFF, internal)
    prg_bank: u8,

    // Banking state
    prg_bank_mode: u8,     // 0-3 (from control register bits 2-3)
    chr_bank_mode: u8,     // 0-1 (from control register bit 4)
    mirroring: Mirroring,
    prg_ram_enabled: bool,
}

impl Mapper1 {
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
            prg_ram: vec![0; 0x2000], // 8KB PRG-RAM (can be 32KB on some carts)
            chr_is_ram,
            shift_register: 0,
            write_count: 0,
            control: 0x0C, // Default: 16KB CHR, last PRG bank fixed
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            prg_bank_mode: 3, // Last bank fixed at $C000
            chr_bank_mode: 0, // 8KB mode
            mirroring,
            prg_ram_enabled: true,
        }
    }

    fn reset_shift_register(&mut self) {
        self.shift_register = 0;
        self.write_count = 0;
        self.control |= 0x0C; // Reset to mode 3
        self.update_banking_mode();
    }

    fn update_banking_mode(&mut self) {
        self.prg_bank_mode = (self.control >> 2) & 0x03;
        self.chr_bank_mode = (self.control >> 4) & 0x01;
        self.mirroring = match self.control & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };
    }
}
```

---

### 4.3.2 Shift Register Interface

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement the serial shift register write protocol that loads configuration registers.

**Files:**

- `crates/rustynes-mappers/src/mapper1.rs` - Shift register logic

**Subtasks:**

- [ ] Detect write to $8000-$FFFF
- [ ] Bit 7 set = reset shift register
- [ ] Accumulate 5 bits (bit 0 of write value)
- [ ] On 5th write, update target register
- [ ] Target register determined by address range

**Implementation:**

```rust
impl Mapper for Mapper1 {
    fn write_prg(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM
                if self.prg_ram_enabled {
                    self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()] = val;
                }
            }
            0x8000..=0xFFFF => {
                // Shift register write
                if (val & 0x80) != 0 {
                    // Bit 7 set: Reset shift register
                    self.reset_shift_register();
                } else {
                    // Accumulate bit 0
                    self.shift_register |= (val & 0x01) << self.write_count;
                    self.write_count += 1;

                    if self.write_count == 5 {
                        // 5 bits written: Update target register
                        self.write_internal_register(addr, self.shift_register);
                        self.shift_register = 0;
                        self.write_count = 0;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Mapper1 {
    fn write_internal_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFF => {
                // Control register
                self.control = val;
                self.update_banking_mode();
            }
            0xA000..=0xBFFF => {
                // CHR bank 0
                self.chr_bank_0 = val;
            }
            0xC000..=0xDFFF => {
                // CHR bank 1
                self.chr_bank_1 = val;
            }
            0xE000..=0xFFFF => {
                // PRG bank
                self.prg_bank = val & 0x0F; // Only lower 4 bits
                self.prg_ram_enabled = (val & 0x10) == 0; // Bit 4 disables PRG-RAM
            }
            _ => {}
        }
    }
}
```

---

### 4.3.3 PRG-ROM Banking

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement PRG-ROM banking with 4 modes (32KB, 16KB switchable low, 16KB switchable high, 16KB fixed high).

**Files:**

- `crates/rustynes-mappers/src/mapper1.rs` - PRG banking

**Subtasks:**

- [ ] Mode 0/1: 32KB switchable (ignore low bit of bank number)
- [ ] Mode 2: Fix first bank at $8000, switch 16KB at $C000
- [ ] Mode 3: Switch 16KB at $8000, fix last bank at $C000
- [ ] Handle PRG-ROM sizes (128KB, 256KB, 512KB)

**Implementation:**

```rust
impl Mapper for Mapper1 {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM
                if self.prg_ram_enabled {
                    self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()]
                } else {
                    0 // Open bus or disabled
                }
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                let offset = (addr & 0x3FFF) as usize; // 16KB bank offset
                let prg_addr = (bank * 0x4000 + offset) % self.prg_rom.len();
                self.prg_rom[prg_addr]
            }
            _ => 0,
        }
    }
}

impl Mapper1 {
    fn get_prg_bank(&self, addr: u16) -> usize {
        let num_banks = self.prg_rom.len() / 0x4000; // Number of 16KB banks

        match self.prg_bank_mode {
            0 | 1 => {
                // 32KB mode: Ignore low bit of bank number
                let bank_32kb = (self.prg_bank & 0xFE) as usize;
                if addr < 0xC000 {
                    bank_32kb % num_banks
                } else {
                    (bank_32kb + 1) % num_banks
                }
            }
            2 => {
                // Fix first bank at $8000, switch at $C000
                if addr < 0xC000 {
                    0
                } else {
                    (self.prg_bank as usize) % num_banks
                }
            }
            3 => {
                // Switch at $8000, fix last bank at $C000
                if addr < 0xC000 {
                    (self.prg_bank as usize) % num_banks
                } else {
                    num_banks - 1
                }
            }
            _ => unreachable!(),
        }
    }
}
```

---

### 4.3.4 CHR-ROM Banking

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement CHR-ROM banking with 2 modes (4KB switchable, 8KB switchable).

**Files:**

- `crates/rustynes-mappers/src/mapper1.rs` - CHR banking

**Subtasks:**

- [ ] Mode 0: 8KB switchable (use chr_bank_0, ignore chr_bank_1)
- [ ] Mode 1: Two 4KB banks (chr_bank_0 at $0000, chr_bank_1 at $1000)
- [ ] Handle CHR-RAM writes

**Implementation:**

```rust
impl Mapper for Mapper1 {
    fn read_chr(&self, addr: u16) -> u8 {
        let bank = self.get_chr_bank(addr);
        let offset = (addr & 0x0FFF) as usize; // 4KB bank offset
        let chr_addr = (bank * 0x1000 + offset) % self.chr_memory.len();
        self.chr_memory[chr_addr]
    }

    fn write_chr(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram {
            let bank = self.get_chr_bank(addr);
            let offset = (addr & 0x0FFF) as usize;
            let chr_addr = (bank * 0x1000 + offset) % self.chr_memory.len();
            self.chr_memory[chr_addr] = val;
        }
    }
}

impl Mapper1 {
    fn get_chr_bank(&self, addr: u16) -> usize {
        let num_banks = self.chr_memory.len() / 0x1000; // Number of 4KB banks

        if self.chr_bank_mode == 0 {
            // 8KB mode: Use chr_bank_0 (ignore low bit), ignore chr_bank_1
            let bank_8kb = (self.chr_bank_0 & 0xFE) as usize;
            if addr < 0x1000 {
                bank_8kb % num_banks
            } else {
                (bank_8kb + 1) % num_banks
            }
        } else {
            // 4KB mode: Two separate banks
            if addr < 0x1000 {
                (self.chr_bank_0 as usize) % num_banks
            } else {
                (self.chr_bank_1 as usize) % num_banks
            }
        }
    }
}
```

---

### 4.3.5 Mirroring Control

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Implement dynamic mirroring control (horizontal, vertical, single-screen).

**Files:**

- `crates/rustynes-mappers/src/mapper1.rs` - Mirroring logic

**Subtasks:**

- [ ] Control register bits 0-1 determine mirroring
- [ ] 0 = Single-screen lower
- [ ] 1 = Single-screen upper
- [ ] 2 = Vertical
- [ ] 3 = Horizontal

**Implementation:**

```rust
impl Mapper for Mapper1 {
    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        false // MMC1 has no IRQ
    }
}
```

---

### 4.3.6 Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for MMC1 functionality.

**Files:**

- `crates/rustynes-mappers/src/mapper1.rs` - Test module

**Subtasks:**

- [ ] Test shift register writes (5-bit accumulation)
- [ ] Test shift register reset (bit 7)
- [ ] Test all PRG banking modes
- [ ] Test all CHR banking modes
- [ ] Test mirroring modes
- [ ] Test PRG-RAM enable/disable

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shift_register_accumulation() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Write 5 bits to control register: 0b11010
        mapper.write_prg(0x8000, 0x00); // Bit 0 = 0
        mapper.write_prg(0x8000, 0x01); // Bit 0 = 1
        mapper.write_prg(0x8000, 0x00); // Bit 0 = 0
        mapper.write_prg(0x8000, 0x01); // Bit 0 = 1
        mapper.write_prg(0x8000, 0x01); // Bit 0 = 1

        // Control register should now be 0b11010 = 26
        assert_eq!(mapper.control, 0b11010);
    }

    #[test]
    fn test_shift_register_reset() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Start writing
        mapper.write_prg(0x8000, 0x01);
        mapper.write_prg(0x8000, 0x01);

        // Reset with bit 7
        mapper.write_prg(0x8000, 0x80);

        assert_eq!(mapper.shift_register, 0);
        assert_eq!(mapper.write_count, 0);
        assert_eq!(mapper.control & 0x0C, 0x0C); // Mode 3
    }

    #[test]
    fn test_prg_bank_mode_3() {
        // Mode 3: Switch at $8000, fix last bank at $C000
        let mut prg_rom = vec![0; 0x20000]; // 128KB = 8 banks
        for i in 0..8 {
            prg_rom[i * 0x4000] = i as u8; // Mark each bank
        }

        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set mode 3 (default)
        mapper.control = 0x0C;
        mapper.update_banking_mode();

        // Switch to bank 2 at $8000
        mapper.prg_bank = 2;

        assert_eq!(mapper.read_prg(0x8000), 2); // Bank 2
        assert_eq!(mapper.read_prg(0xC000), 7); // Last bank (fixed)
    }

    #[test]
    fn test_prg_bank_mode_2() {
        // Mode 2: Fix first bank at $8000, switch at $C000
        let mut prg_rom = vec![0; 0x20000]; // 128KB = 8 banks
        for i in 0..8 {
            prg_rom[i * 0x4000] = i as u8;
        }

        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set mode 2
        mapper.control = 0x08;
        mapper.update_banking_mode();

        // Switch to bank 5 at $C000
        mapper.prg_bank = 5;

        assert_eq!(mapper.read_prg(0x8000), 0); // First bank (fixed)
        assert_eq!(mapper.read_prg(0xC000), 5); // Bank 5
    }

    #[test]
    fn test_prg_bank_mode_01_32kb() {
        // Mode 0/1: 32KB switchable
        let mut prg_rom = vec![0; 0x40000]; // 256KB = 16 banks
        for i in 0..16 {
            prg_rom[i * 0x4000] = i as u8;
        }

        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set mode 0
        mapper.control = 0x00;
        mapper.update_banking_mode();

        // Switch to bank 4 (32KB = banks 4-5)
        mapper.prg_bank = 4;

        assert_eq!(mapper.read_prg(0x8000), 4); // Bank 4
        assert_eq!(mapper.read_prg(0xC000), 5); // Bank 5
    }

    #[test]
    fn test_chr_bank_mode_1() {
        // Mode 1: Two 4KB banks
        let prg_rom = vec![0; 0x8000];
        let mut chr_rom = vec![0; 0x8000]; // 32KB = 8 banks
        for i in 0..8 {
            chr_rom[i * 0x1000] = i as u8;
        }

        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set mode 1 (4KB banks)
        mapper.control = 0x10;
        mapper.update_banking_mode();

        mapper.chr_bank_0 = 2;
        mapper.chr_bank_1 = 5;

        assert_eq!(mapper.read_chr(0x0000), 2); // Bank 2
        assert_eq!(mapper.read_chr(0x1000), 5); // Bank 5
    }

    #[test]
    fn test_chr_bank_mode_0() {
        // Mode 0: 8KB bank
        let prg_rom = vec![0; 0x8000];
        let mut chr_rom = vec![0; 0x8000]; // 32KB = 8 banks
        for i in 0..8 {
            chr_rom[i * 0x1000] = i as u8;
        }

        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Set mode 0 (8KB banks)
        mapper.control = 0x00;
        mapper.update_banking_mode();

        mapper.chr_bank_0 = 4; // 8KB = banks 4-5

        assert_eq!(mapper.read_chr(0x0000), 4); // Bank 4
        assert_eq!(mapper.read_chr(0x1000), 5); // Bank 5
    }

    #[test]
    fn test_mirroring_control() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // Test all mirroring modes
        mapper.control = 0x00; // Single-screen lower
        mapper.update_banking_mode();
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);

        mapper.control = 0x01; // Single-screen upper
        mapper.update_banking_mode();
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);

        mapper.control = 0x02; // Vertical
        mapper.update_banking_mode();
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        mapper.control = 0x03; // Horizontal
        mapper.update_banking_mode();
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn test_prg_ram_disable() {
        let prg_rom = vec![0; 0x8000];
        let chr_rom = vec![0; 0x2000];
        let mut mapper = Mapper1::new(prg_rom, chr_rom, Mirroring::Horizontal);

        // PRG-RAM enabled by default
        mapper.write_prg(0x6000, 0x42);
        assert_eq!(mapper.read_prg(0x6000), 0x42);

        // Disable PRG-RAM (bit 4 of PRG bank register)
        mapper.prg_bank = 0x10;
        mapper.prg_ram_enabled = false;

        // Reads return 0, writes ignored
        mapper.write_prg(0x6000, 0xFF);
        assert_eq!(mapper.read_prg(0x6000), 0);
    }
}
```

---

## Dependencies

**Required:**

- Sprint 4.2 complete (NROM implementation)

**Blocks:**

- Sprint 4.4: UxROM/CNROM implementation
- Sprint 4.5: MMC3 implementation

---

## Related Documentation

- [Mapper Overview](../../../docs/mappers/MAPPER_OVERVIEW.md)
- [MMC1 Specification](../../../docs/mappers/MAPPER_MMC1.md)
- [NESdev Wiki - MMC1](https://www.nesdev.org/wiki/MMC1)

---

## Technical Notes

### Shift Register Protocol

- 5 consecutive writes to $8000-$FFFF with bit 0 = data bit
- Write with bit 7 set resets shift register to initial state
- After 5 writes, accumulated value written to internal register based on address

### Internal Register Map

| Address Range | Register | Description |
|---------------|----------|-------------|
| $8000-$9FFF | Control | Mirroring, PRG mode, CHR mode |
| $A000-$BFFF | CHR Bank 0 | CHR bank for $0000-$0FFF (or $0000-$1FFF in 8KB mode) |
| $C000-$DFFF | CHR Bank 1 | CHR bank for $1000-$1FFF (4KB mode only) |
| $E000-$FFFF | PRG Bank | PRG bank number + PRG-RAM enable |

### Control Register (Internal)

```
7  bit  0
---- ----
CPPMM
|||||
|||++- Mirroring (0=one-screen lower, 1=one-screen upper, 2=vertical, 3=horizontal)
|++--- PRG ROM bank mode (0/1=32KB, 2=fix first 16KB, 3=fix last 16KB)
+----- CHR ROM bank mode (0=8KB, 1=4KB)
```

### PRG Bank Register (Internal)

```
7  bit  0
---- ----
RPPPP
|||||
|++++- PRG ROM bank select
+----- PRG RAM chip enable (0=enabled, 1=disabled)
```

### Power-Up State

- Control register: $0C (mode 3, horizontal mirroring)
- CHR bank 0/1: $00
- PRG bank: $00
- Shift register: empty

### MMC1 Variants

- **SUROM**: 512KB PRG-ROM (extra bit in PRG bank register)
- **SOROM**: 256KB PRG-ROM + 16KB PRG-RAM (dual 8KB chips)
- **SXROM**: Standard 256KB PRG-ROM + 8KB PRG-RAM

---

## Test Requirements

- [ ] Unit tests for shift register accumulation
- [ ] Unit tests for shift register reset
- [ ] Unit tests for all PRG banking modes (0/1/2/3)
- [ ] Unit tests for all CHR banking modes (0/1)
- [ ] Unit tests for mirroring control
- [ ] Unit tests for PRG-RAM enable/disable
- [ ] Integration tests with real MMC1 games

---

## Performance Targets

- Shift register write: <20 ns
- Bank calculation: <10 ns
- PRG read: <10 ns
- Memory: <100KB overhead

---

## Success Criteria

- [ ] Shift register protocol works correctly
- [ ] All 4 PRG banking modes function
- [ ] Both CHR banking modes function
- [ ] Dynamic mirroring control works
- [ ] PRG-RAM enable/disable works
- [ ] All unit tests pass
- [ ] Integration tests with real games pass (Zelda, Metroid, Mega Man 2)
- [ ] Zero unsafe code
- [ ] Documentation complete

---

## Known MMC1 Games

| Game | PRG Size | CHR Size | Special Features |
|------|----------|----------|------------------|
| The Legend of Zelda | 128KB | 128KB | Battery PRG-RAM (save) |
| Metroid | 128KB | 128KB | Battery PRG-RAM (save) |
| Mega Man 2 | 128KB | 128KB | - |
| Final Fantasy | 256KB | 128KB | Battery PRG-RAM (save) |
| Castlevania II | 128KB | 128KB | - |
| Blaster Master | 128KB | 128KB | - |

---

**Previous Sprint:** [Sprint 4.2: Mapper 0 (NROM)](M4-S2-NROM.md)
**Next Sprint:** [Sprint 4.4: Mappers 2 & 3 (UxROM, CNROM)](M4-S4-UXROM-CNROM.md)
