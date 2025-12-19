# Mapper 66: GxROM

Simple discrete mapper with PRG and CHR bank switching.

## Overview

| Property | Value |
|----------|-------|
| Mapper Number | 66 |
| PRG ROM | 128 KB max |
| PRG RAM | None |
| CHR ROM | 32 KB max |
| Mirroring | Fixed (hardwired) |
| Bus Conflicts | Yes |

## Boards

- GNROM
- MHROM

## Memory Map

### CPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $6000-$7FFF | 8 KB | Open bus (no PRG RAM) |
| $8000-$FFFF | 32 KB | Switchable PRG bank |

### PPU Memory

| Address | Size | Description |
|---------|------|-------------|
| $0000-$1FFF | 8 KB | Switchable CHR bank |

## Bank Register ($8000-$FFFF)

Writing to any address in PRG ROM space sets the bank register:

```
7  bit  0
---- ----
xxPP xxCC
  ||   ||
  ||   ++- CHR ROM bank select (8 KB banks)
  ++------ PRG ROM bank select (32 KB banks)
```

### Bank Switching

- **PRG**: 32 KB banks, selected by bits 4-5 (4 banks max)
- **CHR**: 8 KB banks, selected by bits 0-1 (4 banks max)

## Bus Conflicts

GxROM has bus conflicts. The written value is ANDed with the value at the write address. Games must include a lookup table in ROM to work around this.

### Conflict-Safe Write

```rust
fn safe_write(&mut self, addr: u16, value: u8) {
    // Effective value is AND of written value and ROM data
    let rom_value = self.read_prg(addr);
    let effective = value & rom_value;
    self.apply_bank_register(effective);
}
```

## Implementation

```rust
pub struct GxRom {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_bank: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl GxRom {
    pub fn new(prg_rom: &[u8], chr_rom: &[u8], mirroring: Mirroring) -> Self {
        Self {
            prg_rom: prg_rom.to_vec(),
            chr_rom: chr_rom.to_vec(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        }
    }
}

impl Mapper for GxRom {
    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank_offset = self.prg_bank * 0x8000;
                let addr_offset = (addr - 0x8000) as usize;
                let index = (bank_offset + addr_offset) % self.prg_rom.len();
                self.prg_rom[index]
            }
            _ => 0,
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            // Bus conflict: AND written value with ROM value
            let effective = value & self.read_prg(addr);

            self.prg_bank = ((effective >> 4) & 0x03) as usize;
            self.chr_bank = (effective & 0x03) as usize;
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let bank_offset = self.chr_bank * 0x2000;
        let addr_offset = addr as usize & 0x1FFF;
        let index = (bank_offset + addr_offset) % self.chr_rom.len();
        self.chr_rom[index]
    }

    fn write_chr(&mut self, _addr: u16, _value: u8) {
        // CHR ROM is read-only
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
```

## Bank Calculation

### PRG Banking Example

```
PRG ROM size: 128 KB (4 x 32 KB banks)
Register value: 0x20 (bits 4-5 = 2)

PRG bank = (0x20 >> 4) & 0x03 = 2
Bank offset = 2 * 32768 = 65536 ($10000)

$8000 maps to PRG ROM $10000
$FFFF maps to PRG ROM $17FFF
```

### CHR Banking Example

```
CHR ROM size: 32 KB (4 x 8 KB banks)
Register value: 0x02 (bits 0-1 = 2)

CHR bank = 0x02 & 0x03 = 2
Bank offset = 2 * 8192 = 16384 ($4000)

PPU $0000 maps to CHR ROM $4000
PPU $1FFF maps to CHR ROM $5FFF
```

## Compatibility Notes

### Submapper 0 (Standard GxROM)

- 4 PRG banks (128 KB max)
- 4 CHR banks (32 KB max)
- Bus conflicts present

### MHROM Variant

MHROM is similar but with different sizes:
- 64 KB PRG ROM (2 x 32 KB banks)
- 16 KB CHR ROM (2 x 8 KB banks)
- Uses only bits 4 and 0 of register

## Notable Games

- Super Mario Bros. + Duck Hunt (multicart)
- Doraemon
- Dragon Power
- Gumshoe
- Thunder & Lightning

## Power-On State

```rust
impl GxRom {
    fn power_on(&mut self) {
        // Banks default to 0 on power-on
        self.prg_bank = 0;
        self.chr_bank = 0;
    }
}
```

## Test ROMs

- Mapper 66 test (blargg)
- Holy Diver Batman

## References

- [NESdev Wiki: GxROM](https://www.nesdev.org/wiki/GxROM)
- [NESdev Wiki: GNROM](https://www.nesdev.org/wiki/GNROM)
