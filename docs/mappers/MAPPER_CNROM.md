# Mapper 3: CNROM

**Table of Contents**
- [Overview](#overview)
- [Memory Map](#memory-map)
- [Technical Specifications](#technical-specifications)
- [CHR Banking](#chr-banking)
- [Bus Conflicts](#bus-conflicts)
- [Implementation](#implementation)
- [Games Using CNROM](#games-using-cnrom)
- [Testing](#testing)
- [References](#references)

---

## Overview

**CNROM** (Mapper 3) is a simple discrete logic mapper providing **CHR-ROM bank switching** with fixed PRG-ROM. It allows games to have more than 512 tiles by swapping 8KB banks of CHR-ROM, while keeping program code fixed at 32KB.

### Key Characteristics

- **Mapper Number**: 3
- **PRG-ROM**: 16KB or 32KB (fixed, no banking)
- **CHR-ROM**: 32KB (4 banks of 8KB)
- **Mirroring**: Fixed horizontal or vertical
- **Bank Switching**: CHR only (8KB banks)
- **Bus Conflicts**: Yes (on original boards)

**Coverage**: ~6.3% of licensed NES library

---

## Memory Map

### CPU Address Space

```
$8000-$BFFF: First 16KB of PRG-ROM (CNROM-128) or first 16KB (CNROM-256)
$C000-$FFFF: Mirror of $8000-$BFFF (CNROM-128) or second 16KB (CNROM-256)
```

**Note**: PRG-ROM is completely fixed (like NROM)

### PPU Address Space

```
$0000-$1FFF: Switchable 8KB CHR-ROM bank (4 banks available)
```

---

## Technical Specifications

### CHR Banking Register

**Address**: Write to any address in $8000-$FFFF
**Format**:
```
Bits:  76543210
       ||||||||
       ......BA
              ||
              ++- Select 8KB CHR-ROM bank (0-3)
```

**Bank Calculation**:
```rust
fn chr_bank_address(&self, ppu_addr: u16) -> usize {
    (self.chr_bank * 0x2000) + (ppu_addr as usize)
}
```

### Security Diodes (Some Boards)

Some CNROM boards use bits 4-5 for **security diodes** to prevent unauthorized copying:

```
Bits:  76543210
       ||||||||
       ..DC..BA
         ||  ||
         ||  ++- CHR bank select (A14-A13)
         ++- Security diode outputs
```

**Emulation**: Usually ignored

---

## CHR Banking

### Switching Mechanism

**To switch to CHR bank N**:

```assembly
LDA #N              ; N = 0-3
STA $8000           ; Write to any address in $8000-$FFFF
```

### Common Pattern: Dynamic Graphics

Games use CHR banking to:
- Change graphics between levels
- Display different sprite sets
- Show animated backgrounds

**Example**:
```assembly
; Display title screen graphics
LDA #$00
STA $8000           ; CHR Bank 0: Title screen tiles

; Switch to level 1 graphics
LDA #$01
STA $8000           ; CHR Bank 1: Level 1 tiles
```

### "Poor Man's CNROM" (NROM Alternative)

Games without CNROM can simulate banking by swapping between pattern tables:

```
PPUCTRL bit 4 = 0: Use $0000-$0FFF for sprites
PPUCTRL bit 4 = 1: Use $1000-$1FFF for sprites
```

**Limitation**: Only 2 "banks" (pattern tables) vs. CNROM's 4 full banks

---

## Bus Conflicts

CNROM has **bus conflicts** because the ROM output is not disabled during writes.

### Problem

Writing to $8000-$FFFF causes:
- **CPU** outputs the CHR bank number
- **ROM** outputs the byte at that address
- **Conflict** resolved by bitwise AND

### Solution: Lookup Table

```assembly
; Bank selection table (in PRG-ROM)
CHRBankTable:
    .db $00, $01, $02, $03

; Switch to CHR bank N (N in accumulator)
SwitchCHRBank:
    TAX
    LDA CHRBankTable, X
    STA CHRBankTable, X  ; Read and write same value
    RTS
```

### NES 2.0 Submappers

| Submapper | Behavior |
|-----------|----------|
| 0 | Unknown (default to no conflicts) |
| 1 | No bus conflicts |
| 2 | Bus conflicts (bitwise AND) |

---

## Implementation

### Rust Structure

```rust
pub struct CNROM {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,

    chr_bank: usize,
    mirroring: Mirroring,

    bus_conflicts: bool,
}

impl CNROM {
    pub fn new(rom: Rom, submapper: u8) -> Self {
        Self {
            prg_rom: rom.prg_rom,
            chr_rom: rom.chr_rom,
            chr_bank: 0,
            mirroring: rom.mirroring,
            bus_conflicts: submapper == 2,
        }
    }
}
```

### Mapper Trait Implementation

```rust
impl Mapper for CNROM {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                if self.prg_rom.len() == 0x4000 {
                    // 16KB: Mirror twice
                    self.prg_rom[offset % 0x4000]
                } else {
                    // 32KB: Full range
                    self.prg_rom[offset % self.prg_rom.len()]
                }
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            let effective_value = if self.bus_conflicts {
                value & self.read_prg(addr)
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

    fn write_chr(&mut self, _addr: u16, _value: u8) {
        // CHR-ROM is read-only
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
```

---

## Games Using CNROM

### Notable Titles

| Game | PRG Size | CHR Size | Year |
|------|----------|----------|------|
| **Super Mario Bros.** | 32KB | 8KB | 1985 |
| Excitebike | 32KB | 8KB | 1985 |
| Ice Climber | 32KB | 8KB | 1985 |
| Gradius | 32KB | 32KB | 1986 |
| Paperboy | 32KB | 32KB | 1988 |
| Solomon's Key | 32KB | 32KB | 1987 |

**Note**: Super Mario Bros. uses NROM (Mapper 0), not CNROM, despite appearing similar.

---

## Testing

### Unit Tests

```rust
#[test]
fn test_chr_banking() {
    let rom = create_cnrom_rom(2, 4); // 32KB PRG, 32KB CHR (4 banks)
    let mut mapper = CNROM::new(rom, 0);

    // Fill CHR banks with identifiable data
    for bank in 0..4 {
        mapper.chr_rom[bank * 0x2000] = bank as u8;
    }

    // Switch to bank 2
    mapper.write_prg(0x8000, 0x02);
    assert_eq!(mapper.chr_bank, 2);
    assert_eq!(mapper.read_chr(0x0000), 2);

    // Switch to bank 3
    mapper.write_prg(0x8000, 0x03);
    assert_eq!(mapper.chr_bank, 3);
    assert_eq!(mapper.read_chr(0x0000), 3);
}

#[test]
fn test_bus_conflicts() {
    let mut rom = create_cnrom_rom(2, 4);
    rom.prg_rom[0x0000] = 0x01; // ROM at $8000 contains $01

    let mut mapper = CNROM::new(rom, 2); // Submapper 2 = conflicts

    // Write 0xFF to $8000
    mapper.write_prg(0x8000, 0xFF);

    // With conflicts: 0xFF & 0x01 = 0x01
    assert_eq!(mapper.chr_bank, 1);
}
```

---

## References

- [NesDev Wiki: CNROM](https://www.nesdev.org/wiki/CNROM)
- [NesDev Wiki: CNROM CHR Bank Switching](https://nesasm.com/graphics/cnrom-bank-switching/)
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - General mapper architecture
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Bus conflict handling

---

**Related Documents**:
- [MAPPER_NROM.md](MAPPER_NROM.md) - Similar fixed PRG-ROM
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md) - Mapper introduction
- [BUS_CONFLICTS.md](../bus/BUS_CONFLICTS.md) - Conflict details
