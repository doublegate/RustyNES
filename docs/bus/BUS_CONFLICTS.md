# NES Bus Conflicts

**Table of Contents**
- [Overview](#overview)
- [What is a Bus Conflict?](#what-is-a-bus-conflict)
- [When Bus Conflicts Occur](#when-bus-conflicts-occur)
- [Hardware Behavior](#hardware-behavior)
- [Avoiding Bus Conflicts in ROM Code](#avoiding-bus-conflicts-in-rom-code)
- [Mapper-Specific Behavior](#mapper-specific-behavior)
  - [UxROM (Mapper 2)](#uxrom-mapper-2)
  - [CNROM (Mapper 3)](#cnrom-mapper-3)
  - [AxROM (Mapper 7)](#axrom-mapper-7)
  - [Other Affected Mappers](#other-affected-mappers)
- [Emulation Implementation](#emulation-implementation)
- [NES 2.0 Submapper Specification](#nes-20-submapper-specification)
- [Testing Bus Conflict Handling](#testing-bus-conflict-handling)
- [References](#references)

---

## Overview

**Bus conflicts** are a hardware phenomenon that occurs on certain NES cartridges when the CPU attempts to write to a mapper register located in ROM address space. Understanding and correctly emulating bus conflicts is essential for compatibility with many commercial games, particularly those using discrete logic mappers like UxROM and CNROM.

### Key Points

- Bus conflicts occur when **two devices drive the same bus line** with different values
- Common on **discrete logic mappers** (UxROM, CNROM, AxROM)
- **ASIC mappers** (MMC1, MMC3, etc.) typically prevent conflicts with output disable logic
- Games must avoid conflicts by writing values that match ROM contents
- Emulators should handle conflicts by **ANDing** the CPU and ROM values

---

## What is a Bus Conflict?

A **bus conflict** occurs when two logic devices attempt to output different values on the same bus line simultaneously. In the context of the NES:

1. The **CPU** writes a value to an address in PRG-ROM space ($8000-$FFFF)
2. The **ROM chip** simultaneously outputs the byte stored at that address
3. If the values differ, they **conflict** on the PRG data bus

### Electrical Behavior

When two signals are asserted at different logic levels on the same wire:
- The signal with **less impedance** (stronger drive) typically wins
- In the NES, both CPU and mask ROMs drive **0 more strongly than 1**
- The effective result is the **bitwise AND** of the two values

**Example**:
```
CPU writes:     10110101 (0xB5)
ROM contains:   11001100 (0xCC)
Resulting AND:  10000100 (0x84)
```

The mapper register receives **0x84**, not the intended **0xB5**.

---

## When Bus Conflicts Occur

Bus conflicts are specific to mapper implementations. They occur when:

1. **Mapper registers are mapped to ROM space** ($8000-$FFFF)
2. **ROM output is not disabled during writes**
3. The **written value differs from ROM contents** at that address

### Why Some Mappers Have Conflicts

**Discrete Logic Mappers** (UxROM, CNROM):
- Use simple 74-series logic chips
- ROM chip-select tied to **PRG A15** (active when address ≥ $8000)
- **No write signal** to disable ROM output during CPU writes
- Saves components but causes conflicts

**ASIC Mappers** (MMC1, MMC3, MMC5):
- Custom integrated circuits with sophisticated logic
- Include **output enable control** that disables ROM during writes
- **No bus conflicts** due to proper tri-state management

---

## Hardware Behavior

Through hardware testing, the following behavior has been confirmed:

### Conflict Resolution

Both the CPU and mask ROMs in the NES era:
- Drive **logic 0 more strongly than logic 1**
- Result: Conflicts resolve to **bitwise AND**

**Implementation Rule**:
```
effective_value = cpu_written_value & rom_byte_at_address
```

### Important Caveat

**Programmers must not rely on this behavior.** The specific conflict resolution is undefined in the NES specification and may vary:
- Different ROM chip manufacturers
- Different board revisions
- Clone consoles with different electrical characteristics

**Proper practice**: Always avoid conflicts by writing matching values.

---

## Avoiding Bus Conflicts in ROM Code

Game developers use several techniques to prevent bus conflicts:

### Method 1: Lookup Table (Most Common)

Place a **table of bank numbers** in ROM and write to the corresponding offset:

```assembly
; Bank switching routine
SwitchBank:
    TAX                     ; X = bank number
    LDA BankTable, X        ; Read bank number from table
    STA BankTable, X        ; Write same value back (no conflict!)
    RTS

BankTable:
    .db 0, 1, 2, 3, 4, 5, 6, 7   ; Bank numbers 0-7
```

**Why this works**:
- `LDA BankTable, X` reads the bank number from ROM
- `STA BankTable, X` writes the **same value** to the **same address**
- CPU writes (e.g., 5) and ROM outputs (5) → **5 AND 5 = 5** (no conflict)

### Method 2: Dedicated Conflict-Free Regions

Some boards include small regions where conflicts cannot occur:
- **AxROM boards**: Some have a small ROM region with hardwired conflict-free bytes
- **Custom boards**: May include registers in non-ROM space ($6000-$7FFF)

**Example** (theoretical):
```assembly
; If ROM at $8000 is guaranteed to be $FF
SwitchBank:
    STA $8000   ; Write to $8000 where ROM = $FF
                ; ANY_VALUE & $FF = ANY_VALUE (safe)
    RTS
```

### Method 3: Pre-Computed Match Addresses

Calculate addresses where ROM naturally contains the desired value:

```assembly
; Suppose we want to switch to bank 3
; Search ROM for any location containing $03
SwitchToBank3:
    LDA #3
    STA FoundThree   ; Address where ROM contains $03
    RTS
```

**Limitation**: Requires knowing ROM contents at compile time.

---

## Mapper-Specific Behavior

### UxROM (Mapper 2)

**Description**: Simple discrete logic mapper for PRG-ROM bank switching

**Registers**:
- **Bank select**: Write to $8000-$FFFF
- **Size**: 3-4 bits (8-16 banks of 16KB)
- **Fixed bank**: Last bank always at $C000-$FFFF

**Bus Conflicts**: **YES** (on original Nintendo boards)

**Standard Implementation** (with conflicts):
```rust
fn write_uxrom_with_conflicts(&mut self, addr: u16, value: u8) {
    if addr >= 0x8000 {
        let rom_value = self.prg_rom[self.map_prg_addr(addr)];
        let effective_value = value & rom_value; // Bus conflict
        self.prg_bank = (effective_value & 0x0F) as usize;
    }
}
```

**NES 2.0 Submappers**:
- **Submapper 0**: Default iNES behavior (no conflicts in emulator)
- **Submapper 1**: Explicit no conflicts
- **Submapper 2**: Explicit conflicts (AND behavior)

**Games Using UxROM**:
- Mega Man (1-6)
- Castlevania
- Contra
- Duck Tales

### CNROM (Mapper 3)

**Description**: Simple CHR-ROM bank switching

**Registers**:
- **CHR bank select**: Write to $8000-$FFFF
- **Size**: 2 bits (4 banks of 8KB CHR)

**Bus Conflicts**: **YES**

**Implementation**:
```rust
fn write_cnrom_with_conflicts(&mut self, addr: u16, value: u8) {
    if addr >= 0x8000 {
        let rom_value = self.prg_rom[self.map_prg_addr(addr)];
        let effective_value = value & rom_value;
        self.chr_bank = (effective_value & 0x03) as usize;
    }
}
```

**Games Using CNROM**:
- Super Mario Bros.
- Excitebike
- Donkey Kong (many versions)

### AxROM (Mapper 7)

**Description**: Bank switching with single-screen mirroring control

**Registers**:
- **Write to $8000-$FFFF**:
  - Bits 0-2: PRG bank (8 banks of 32KB)
  - Bit 4: Nametable select (single-screen mirroring)

**Bus Conflicts**: **YES**

**Implementation**:
```rust
fn write_axrom_with_conflicts(&mut self, addr: u16, value: u8) {
    if addr >= 0x8000 {
        let rom_value = self.prg_rom[self.map_prg_addr(addr)];
        let effective_value = value & rom_value;

        self.prg_bank = (effective_value & 0x07) as usize;
        self.nametable_select = (effective_value >> 4) & 0x01;
    }
}
```

**Games Using AxROM**:
- Battletoads
- Marble Madness
- Jeopardy!

### Other Affected Mappers

Several other mappers exhibit bus conflicts:

| Mapper | Name | Conflict Behavior |
|--------|------|-------------------|
| 0 | NROM | No registers (irrelevant) |
| 30 | UNROM 512 | Optional (homebrew) |
| 34 | BNROM/NINA-001 | Yes (discrete logic variants) |
| 66 | GxROM | Yes |
| 152 | Taito TC0690 | Yes |
| 180 | Crazy Climber | Yes (inverted UxROM) |

**ASIC Mappers (No Conflicts)**:
- Mapper 1 (MMC1)
- Mapper 4 (MMC3)
- Mapper 5 (MMC5)
- Mapper 9 (MMC2)
- Mapper 10 (MMC4)
- All VRC mappers

---

## Emulation Implementation

### Standard Approach (NES 2.0 Aware)

```rust
pub enum BusConflictMode {
    None,           // No conflicts (ASIC mappers or iNES default)
    BitwiseAnd,     // Conflicts occur, AND the values
}

impl Mapper {
    fn write_with_bus_conflict(
        &mut self,
        addr: u16,
        value: u8,
        conflict_mode: BusConflictMode,
    ) {
        let effective_value = match conflict_mode {
            BusConflictMode::None => value,
            BusConflictMode::BitwiseAnd => {
                let rom_value = self.read_prg(addr);
                value & rom_value
            }
        };

        self.write_register(addr, effective_value);
    }
}
```

### UxROM Example

```rust
pub struct UxROM {
    prg_rom: Vec<u8>,
    prg_bank: usize,
    num_banks: usize,
    bus_conflicts: bool, // Set from NES 2.0 submapper
}

impl Mapper for UxROM {
    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            let effective_value = if self.bus_conflicts {
                let rom_addr = self.map_prg_addr(addr);
                value & self.prg_rom[rom_addr]
            } else {
                value
            };

            self.prg_bank = (effective_value as usize) % self.num_banks;
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let rom_addr = self.map_prg_addr(addr);
        self.prg_rom[rom_addr]
    }

    fn map_prg_addr(&self, addr: u16) -> usize {
        match addr {
            0x8000..=0xBFFF => {
                // Switchable bank
                (self.prg_bank * 0x4000) + ((addr & 0x3FFF) as usize)
            }
            0xC000..=0xFFFF => {
                // Fixed to last bank
                let last_bank = self.num_banks - 1;
                (last_bank * 0x4000) + ((addr & 0x3FFF) as usize)
            }
            _ => 0, // Should not happen
        }
    }
}
```

### CNROM Example

```rust
pub struct CNROM {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_bank: usize,
    bus_conflicts: bool,
}

impl Mapper for CNROM {
    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            let effective_value = if self.bus_conflicts {
                let rom_addr = (addr & 0x7FFF) as usize;
                value & self.prg_rom[rom_addr % self.prg_rom.len()]
            } else {
                value
            };

            self.chr_bank = (effective_value & 0x03) as usize;
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let offset = (self.chr_bank * 0x2000) + (addr as usize);
        self.chr_rom[offset % self.chr_rom.len()]
    }
}
```

### Performance Considerations

Bus conflict checking adds overhead:
1. **Extra ROM read** to get the conflicting value
2. **Bitwise AND** operation

**Optimization**:
- Only perform conflict checking for mappers that need it
- Use compile-time feature flags for accuracy vs. speed

```rust
#[cfg(feature = "accurate_bus_conflicts")]
fn get_effective_value(&self, addr: u16, value: u8) -> u8 {
    value & self.read_prg(addr)
}

#[cfg(not(feature = "accurate_bus_conflicts"))]
fn get_effective_value(&self, _addr: u16, value: u8) -> u8 {
    value
}
```

---

## NES 2.0 Submapper Specification

The **NES 2.0 header format** includes a submapper field to disambiguate bus conflict behavior.

### Submapper Values (Mappers 2, 3, 7)

| Submapper | Behavior | Description |
|-----------|----------|-------------|
| **0** | Default | iNES behavior (typically no conflicts for compatibility) |
| **1** | No conflicts | Explicit specification (ASIC variants) |
| **2** | Conflicts (AND) | Explicit bus conflicts with AND resolution |

### Reading Submapper from Header

```rust
pub fn parse_nes20_header(header: &[u8; 16]) -> MapperInfo {
    let mapper_low = header[6] >> 4;
    let mapper_high = header[7] & 0xF0;
    let submapper = (header[8] & 0x0F) >> 0;

    let mapper_number = (mapper_high as u16) | (mapper_low as u16);

    let bus_conflicts = match (mapper_number, submapper) {
        (2, 2) | (3, 2) | (7, 2) => BusConflictMode::BitwiseAnd,
        (2, 1) | (3, 1) | (7, 1) => BusConflictMode::None,
        _ => BusConflictMode::None, // Default for iNES compatibility
    };

    MapperInfo {
        mapper_number,
        submapper,
        bus_conflicts,
    }
}
```

### Handling iNES 1.0 (No Submapper)

For iNES 1.0 ROMs (without NES 2.0 header):
- **Default to no conflicts** for maximum compatibility
- Many unlicensed games require this behavior
- Optionally allow user override in configuration

---

## Testing Bus Conflict Handling

### Test ROM Approach

Create a test ROM that:
1. Writes to mapper registers with **mismatched values**
2. Verifies the **resulting bank** matches AND behavior
3. Tests with both matching and conflicting writes

**Pseudo-code**:
```assembly
; Test 1: Matching write (should always work)
LDA BankTable + 5   ; Load 5 from ROM
STA BankTable + 5   ; Write 5 to same location
; Verify bank 5 is selected

; Test 2: Conflicting write (tests bus conflict)
LDA #$FF            ; Load all 1s
STA BankTable + 3   ; Write to location containing 3
; With conflicts: $FF AND $03 = $03 (bank 3)
; Without conflicts: $FF (undefined bank, likely bank 15)
```

### Known Commercial Test Cases

| Game | Mapper | Requires Conflicts? |
|------|--------|---------------------|
| Mega Man 2 | UxROM | Yes |
| Castlevania | UxROM | Yes |
| Super Mario Bros. | CNROM | Yes |
| Battletoads | AxROM | Yes |

**Testing Method**: Run these games with both conflict modes and verify correct operation.

### Automated Testing

```rust
#[test]
fn test_uxrom_bus_conflicts() {
    let mut mapper = UxROM::new_with_conflicts(
        vec![0x00; 0x40000], // 256KB PRG-ROM
        true, // Enable bus conflicts
    );

    // Set up ROM to contain specific values
    mapper.prg_rom[0x0000] = 0x03; // Bank 3

    // Write 0xFF to $8000 (which contains $03)
    mapper.write_prg(0x8000, 0xFF);

    // With conflicts: 0xFF & 0x03 = 0x03
    assert_eq!(mapper.prg_bank, 3);

    // Without conflicts, bank would be 15
}

#[test]
fn test_cnrom_no_conflicts() {
    let mut mapper = CNROM::new_without_conflicts(
        vec![0x00; 0x8000],  // 32KB PRG-ROM
        vec![0x00; 0x8000],  // 32KB CHR-ROM
        false, // Disable bus conflicts
    );

    // Write 0xFF to select CHR bank
    mapper.write_prg(0x8000, 0xFF);

    // Without conflicts: bank = 0xFF & 0x03 = 3
    assert_eq!(mapper.chr_bank, 3);
}
```

---

## References

- [NesDev Wiki: Bus Conflict](https://www.nesdev.org/wiki/Bus_conflict)
- [NesDev Wiki: UxROM](https://www.nesdev.org/wiki/UxROM)
- [NesDev Wiki: CNROM](https://www.nesdev.org/wiki/NROM)
- [NesDev Wiki: NES 2.0 Submappers](https://www.nesdev.org/wiki/NES_2.0_submappers)
- [NesDev Forums: UxROM Bus Conflicts](https://forums.nesdev.org/viewtopic.php?t=13191)
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - Mapper architecture
- [MEMORY_MAP.md](MEMORY_MAP.md) - NES memory layout

---

**Related Documents**:
- [MEMORY_MAP.md](MEMORY_MAP.md) - NES memory architecture
- [MAPPER_UXROM.md](../mappers/MAPPER_UXROM.md) - UxROM implementation
- [MAPPER_CNROM.md](../mappers/MAPPER_CNROM.md) - CNROM implementation
- [MAPPER_OVERVIEW.md](../mappers/MAPPER_OVERVIEW.md) - General mapper architecture
