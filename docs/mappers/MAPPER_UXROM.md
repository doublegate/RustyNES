# Mapper 2: UxROM

**Table of Contents**

- [Overview](#overview)
- [Board Variants](#board-variants)
- [Memory Map](#memory-map)
- [Technical Specifications](#technical-specifications)
- [Bank Switching](#bank-switching)
- [Bus Conflicts](#bus-conflicts)
- [Implementation](#implementation)
- [Games Using UxROM](#games-using-uxrom)
- [Testing](#testing)
- [References](#references)

---

## Overview

**UxROM** (Mapper 2) is a simple discrete logic mapper providing **PRG-ROM bank switching** while using **CHR-RAM** for graphics. It was one of Nintendo's early solutions for games larger than 32KB, offering 128KB-256KB of program ROM with a straightforward banking scheme.

### Key Characteristics

- **Mapper Number**: 2
- **Board Names**: NES-UNROM, NES-UOROM, HVC-UN1ROM
- **PRG-ROM**: 64KB to 256KB (4-16 banks of 16KB)
- **CHR**: 8KB CHR-RAM (no CHR-ROM, no CHR banking)
- **Mirroring**: Fixed horizontal or vertical
- **Bank Switching**: Lower 16KB switchable, upper 16KB fixed to last bank
- **Bus Conflicts**: Yes (on original Nintendo boards)

**Coverage**: ~10.6% of licensed NES library

---

## Board Variants

### UNROM (64KB-128KB)

**PRG-ROM**: 64KB (4 banks) or 128KB (8 banks)
**Bank Select**: 3 bits (UNROM) or 4 bits (UOROM)

**Hardware**: 74HC161 (4-bit latch) + 74HC32 (OR gate)

**Games**:

- Mega Man (64KB)
- Castlevania (128KB)
- Metal Gear (128KB)

### UOROM (256KB)

**PRG-ROM**: Up to 256KB (16 banks)
**Bank Select**: 4 bits

**Extension**: Same hardware, larger PRG-ROM capacity

### UNROM-512 (Homebrew)

**PRG-ROM**: Up to 512KB (32 banks)
**Additional Features**: CHR banking (optional), one-screen mirroring
**Bank Select**: 5-8 bits

**Status**: Modern homebrew mapper (Mapper 30), not original UxROM

---

## Memory Map

### CPU Address Space

```
$6000-$7FFF: PRG-RAM (optional, 8KB, battery-backed)
$8000-$BFFF: Switchable 16KB PRG-ROM bank (banks 0 to N-2)
$C000-$FFFF: Fixed 16KB PRG-ROM bank (always last bank)
```

**Key Point**: The last bank is **fixed** at $C000-$FFFF and contains:

- Interrupt vectors ($FFFA-$FFFF)
- Reset/initialization code
- Bank switching routine

### PPU Address Space

```
$0000-$1FFF: 8KB CHR-RAM (writable, no banking)
$2000-$3FFF: VRAM (nametables, palette)
```

**Note**: UxROM always uses CHR-RAM, not CHR-ROM. Graphics are uploaded from PRG-ROM to CHR-RAM at runtime.

---

## Technical Specifications

### PRG-ROM Banking

**Switchable Bank**: $8000-$BFFF (16KB)

- Banks 0 to (N-1), where N = total number of banks
- Selected by writing to $8000-$FFFF

**Fixed Bank**: $C000-$FFFF (16KB)

- Always mapped to the last bank
- Contains reset vector and critical code

#### Bank Number Calculation

```rust
fn num_banks(&self) -> usize {
    self.prg_rom.len() / 0x4000 // 16KB per bank
}

fn last_bank(&self) -> usize {
    self.num_banks() - 1
}
```

### Registers

**Bank Select Register** (write-only):

- **Address**: Any address in $8000-$FFFF
- **Data Written**: Bank number

**iNES Mapper 2** (no bus conflicts):

```
Bits:  7654 3210
       ---- ----
       xxxx BBBB
            ||||
            ++++- Select 16KB PRG-ROM bank for $8000-$BFFF
```

**Original Hardware** (with bus conflicts):

```
Actual bank = written_value AND rom_byte_at_address
```

### CHR-RAM

**Size**: 8KB
**Writable**: Yes
**Banking**: None (all fixed)

**Usage**: Games copy tile data from PRG-ROM to CHR-RAM during initialization or when changing levels.

---

## Bank Switching

### Switching Mechanism

**To switch to bank N**:

```assembly
LDA #N              ; Load bank number
STA $8000           ; Write to any address in $8000-$FFFF
; Bank N now appears at $8000-$BFFF
```

**Example**:

```assembly
; Switch to bank 3
LDA #$03
STA $8000
```

### Fixed Bank Strategy

**Why the last bank is fixed**:

1. **Reset vector** at $FFFC-$FFFD must be accessible on power-up
2. **Interrupt vectors** (NMI, IRQ) at $FFFA-$FFFF
3. **Bank switching code** must be accessible from any bank

**Code Organization**:

```
Last Bank ($C000-$FFFF):
  - Reset handler
  - NMI handler
  - IRQ handler (if used)
  - Bank switching routine
  - Common subroutines

Other Banks ($8000-$BFFF):
  - Level-specific code
  - Graphics data
  - Music/sound data
```

### Bank Switching Routine (in Fixed Bank)

```assembly
; BankSwitch: Switch to bank in accumulator
BankSwitch:
    STA $8000           ; Select bank
    RTS

; Usage:
LDA #5                  ; Want bank 5
JSR BankSwitch          ; Call routine in fixed bank
```

---

## Bus Conflicts

UxROM boards have **bus conflicts** because the ROM output is not disabled during writes.

### Problem

When writing to $8000-$FFFF:

- **CPU** puts the bank number on the data bus
- **ROM** simultaneously outputs the byte at that address
- **Conflict** if values differ

### Solution: Lookup Table

Place bank numbers in ROM at known addresses:

```assembly
; Bank switch table in fixed bank
BankTable:
    .db 0, 1, 2, 3, 4, 5, 6, 7  ; Bank numbers 0-7

; Bank switching routine (conflict-safe)
SwitchToBank:
    TAX                         ; X = bank number
    LDA BankTable, X            ; Read bank from table
    STA BankTable, X            ; Write same value (no conflict!)
    RTS
```

**Why this works**:

- Reading `BankTable, X` loads the bank number from ROM
- Writing to the same address puts the same value on the bus
- CPU and ROM both output the same value → No conflict

### NES 2.0 Submappers

| Submapper | Behavior |
|-----------|----------|
| 0 | Default iNES (no conflicts in emulator) |
| 1 | No bus conflicts (ASIC or modified boards) |
| 2 | Bus conflicts present (original hardware) |

**Emulation**: Check submapper field to determine conflict behavior.

---

## Implementation

### Rust Structure

```rust
pub struct UxROM {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_ram: Vec<u8>,

    prg_bank: usize,    // Switchable bank at $8000-$BFFF
    num_banks: usize,
    mirroring: Mirroring,

    bus_conflicts: bool, // Set from NES 2.0 submapper
}

impl UxROM {
    pub fn new(rom: Rom, submapper: u8) -> Self {
        let num_banks = rom.prg_rom.len() / 0x4000;
        let bus_conflicts = submapper == 2;

        Self {
            prg_rom: rom.prg_rom,
            prg_ram: vec![0; 0x2000],   // 8KB PRG-RAM
            chr_ram: vec![0; 0x2000],   // 8KB CHR-RAM

            prg_bank: 0,
            num_banks,
            mirroring: rom.mirroring,
            bus_conflicts,
        }
    }
}
```

### Mapper Trait Implementation

```rust
impl Mapper for UxROM {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM
                let offset = (addr - 0x6000) as usize;
                self.prg_ram[offset % self.prg_ram.len()]
            }
            0x8000..=0xBFFF => {
                // Switchable bank
                let offset = ((addr - 0x8000) as usize) + (self.prg_bank * 0x4000);
                self.prg_rom[offset % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                // Fixed to last bank
                let last_bank = self.num_banks - 1;
                let offset = ((addr - 0xC000) as usize) + (last_bank * 0x4000);
                self.prg_rom[offset % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // PRG-RAM write
                let offset = (addr - 0x6000) as usize;
                self.prg_ram[offset % self.prg_ram.len()] = value;
            }
            0x8000..=0xFFFF => {
                // Bank select register
                let effective_value = if self.bus_conflicts {
                    value & self.read_prg(addr) // Bus conflict
                } else {
                    value
                };

                self.prg_bank = (effective_value as usize) % self.num_banks;
            }
            _ => {}
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_ram[(addr as usize) % 0x2000]
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        self.chr_ram[(addr as usize) % 0x2000] = value;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
```

### Bus Conflict Handling

```rust
fn write_with_bus_conflict(&mut self, addr: u16, value: u8) {
    let effective_value = if self.bus_conflicts {
        let rom_value = self.read_prg(addr);
        value & rom_value
    } else {
        value
    };

    self.prg_bank = (effective_value as usize) % self.num_banks;
}
```

---

## Games Using UxROM

### Notable Titles

| Game | Size | Year | Notes |
|------|------|------|-------|
| **Mega Man** | 128KB | 1987 | First Mega Man game |
| **Mega Man 2** | 128KB | 1988 | Uses bus conflict workaround |
| **Mega Man 3-6** | 128KB-256KB | 1990-1993 | Series standard |
| **Castlevania** | 128KB | 1987 | Classic action platformer |
| **Contra** | 128KB | 1988 | Run-and-gun gameplay |
| **Metal Gear** | 128KB | 1987 | Stealth action |
| **Duck Tales** | 128KB | 1989 | Capcom platformer |
| **The Legend of Zelda** | 128KB | 1986 | Actually uses MMC1, not UxROM |

**Total UxROM Games**: ~220 commercial releases (~10.6% of library)

### Common Use Cases

- **Action platformers**: Mega Man series
- **Run-and-gun**: Contra
- **Adventure**: Metal Gear
- **Sports**: Various sports titles

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bank_switching() {
        let rom = create_uxrom_test_rom(8); // 8 banks (128KB)
        let mut mapper = UxROM::new(rom, 0); // No bus conflicts

        // Switch to bank 3
        mapper.write_prg(0x8000, 0x03);
        assert_eq!(mapper.prg_bank, 3);

        // Verify reading from switchable window
        let addr = 0x8000;
        let value = mapper.read_prg(addr);
        // Should read from bank 3
    }

    #[test]
    fn test_last_bank_fixed() {
        let rom = create_uxrom_test_rom(8);
        let mut mapper = UxROM::new(rom, 0);

        // Write different banks
        mapper.write_prg(0x8000, 0x00);
        let value_bank0 = mapper.read_prg(0xC000);

        mapper.write_prg(0x8000, 0x05);
        let value_bank5 = mapper.read_prg(0xC000);

        // Upper bank should not change
        assert_eq!(value_bank0, value_bank5);
    }

    #[test]
    fn test_bus_conflicts() {
        let mut rom = create_uxrom_test_rom(8);
        // Set ROM byte at $8000 to 0x03
        rom.prg_rom[0x0000] = 0x03;

        let mut mapper = UxROM::new(rom, 2); // Submapper 2 = bus conflicts

        // Write 0xFF to $8000
        mapper.write_prg(0x8000, 0xFF);

        // With conflicts: 0xFF & 0x03 = 0x03
        assert_eq!(mapper.prg_bank, 3);
    }

    #[test]
    fn test_chr_ram_readwrite() {
        let rom = create_uxrom_test_rom(4);
        let mut mapper = UxROM::new(rom, 0);

        // CHR-RAM should be writable
        mapper.write_chr(0x0000, 0xAA);
        assert_eq!(mapper.read_chr(0x0000), 0xAA);

        mapper.write_chr(0x1FFF, 0x55);
        assert_eq!(mapper.read_chr(0x1FFF), 0x55);
    }
}
```

### Integration Tests

**Test ROM**: Run Mega Man 2 (UxROM with bus conflicts)

```rust
#[test]
fn test_mega_man_2() {
    let rom = load_rom("roms/Mega Man 2 (USA).nes");
    let mut console = Console::new(rom);

    // Run for 120 frames (2 seconds)
    for _ in 0..120 {
        console.step_frame();
    }

    // Should reach title screen
    // Verify by checking known RAM location
    let stage_select = console.read_cpu(0x001C);
    assert!(stage_select <= 8); // Valid stage
}
```

---

## References

- [NesDev Wiki: UxROM](https://www.nesdev.org/wiki/UxROM)
- [NesDev Wiki: Programming UNROM](https://www.nesdev.org/wiki/Programming_UNROM)
- [NesDev Wiki: NES 2.0 Submappers](https://www.nesdev.org/wiki/NES_2.0_submappers)
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - General mapper architecture
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Bus conflict details
- [MEMORY_MAP.md](../bus/MEMORY_MAP.md) - NES memory layout

---

**Related Documents**:

- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - Mapper introduction
- [MAPPER_NROM.md](MAPPER_NROM.md) - Simpler predecessor
- [MAPPER_MMC1.md](MAPPER_MMC1.md) - More complex successor
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Handling bus conflicts
