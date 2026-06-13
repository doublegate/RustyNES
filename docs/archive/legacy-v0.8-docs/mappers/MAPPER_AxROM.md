# Mapper 7: AxROM

Simple discrete mapper with single-screen mirroring control.

## Overview

| Property | Value |
|----------|-------|
| Mapper Number | 7 |
| PRG ROM | 256 KB max |
| PRG RAM | None |
| CHR | 8 KB RAM |
| Mirroring | Switchable single-screen |
| Bus Conflicts | Yes (on some boards) |

## Boards

- ANROM
- AOROM
- AN1ROM
- AMROM

## Memory Map

### CPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $6000-$7FFF | 8 KB | Open bus (no PRG RAM) |
| $8000-$FFFF | 32 KB | Switchable PRG bank |

### PPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $0000-$1FFF | 8 KB | CHR RAM (pattern tables) |

## Bank Register ($8000-$FFFF)

Writing to any address in PRG ROM space sets the bank register:

```
7  bit  0
---- ----
xxxM xPPP
   |  |||
   |  +++- PRG ROM bank select (32 KB banks)
   +------ Nametable select (0 = $2000, 1 = $2400)
```

### Bank Switching

- **PRG**: 32 KB banks, selected by bits 0-2
- **Maximum banks**: 8 (256 KB)

### Mirroring

Single-screen mirroring:

- Bit 4 = 0: All nametables map to CIRAM $000-$3FF
- Bit 4 = 1: All nametables map to CIRAM $400-$7FF

## Bus Conflicts

Some AxROM boards have bus conflicts:

| Board | Bus Conflicts |
|-------|---------------|
| ANROM | Yes |
| AOROM | No |
| AN1ROM | No |
| AMROM | No |

For boards with bus conflicts, the written value is ANDed with the value at the write address. ROMs designed for these boards include a lookup table to avoid conflicts.

## Implementation

```rust
pub struct AxRom {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    prg_bank: usize,
    mirroring: Mirroring,
    has_bus_conflicts: bool,
}

impl AxRom {
    pub fn new(rom: &[u8], has_bus_conflicts: bool) -> Self {
        Self {
            prg_rom: rom.to_vec(),
            chr_ram: [0; 8192],
            prg_bank: 0,
            mirroring: Mirroring::SingleScreenLower,
            has_bus_conflicts,
        }
    }
}

impl Mapper for AxRom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank_offset = self.prg_bank * 0x8000;
                let addr_offset = (addr - 0x8000) as usize;
                self.prg_rom[bank_offset + addr_offset]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            let effective_value = if self.has_bus_conflicts {
                value & self.read_prg(addr)
            } else {
                value
            };

            self.prg_bank = (effective_value & 0x07) as usize;
            self.mirroring = if effective_value & 0x10 != 0 {
                Mirroring::SingleScreenUpper
            } else {
                Mirroring::SingleScreenLower
            };
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        self.chr_ram[addr as usize & 0x1FFF]
    }

    fn write_chr(&mut self, addr: u16, value: u8) {
        self.chr_ram[addr as usize & 0x1FFF] = value;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
```

## Notable Games

- Battletoads
- Marble Madness
- Wizards & Warriors
- R.C. Pro-Am
- A Boy and His Blob
- Captain Skyhawk
- Solstice

## Test ROMs

- Holy Diver Batman (AxROM test)
- Mapper 7 test (NESdev)

## References

- [NESdev Wiki: AxROM](https://www.nesdev.org/wiki/AxROM)
- [NESdev Wiki: Bus conflict](https://www.nesdev.org/wiki/Bus_conflict)
