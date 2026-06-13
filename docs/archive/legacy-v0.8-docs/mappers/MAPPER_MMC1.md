# Mapper 1: MMC1 (SxROM)

**Table of Contents**

- [Overview](#overview)
- [Serial Write Interface](#serial-write-interface)
- [Registers](#registers)
- [Banking Modes](#banking-modes)
- [Implementation](#implementation)
- [Games Using MMC1](#games-using-mmc1)
- [References](#references)

---

## Overview

**MMC1** is Nintendo's most widely used first-party mapper, introducing PRG-RAM support, battery-backed saves, configurable banking modes, and dynamic mirroring control. Its unique **serial write interface** reduces pin count by requiring 5 writes to configure each register.

### Key Characteristics

- **Mapper Number**: 1
- **Board Names**: SxROM (SLROM, SNROM, SOROM, SUROM, SXROM)
- **PRG-ROM**: Up to 512KB (32 banks of 16KB)
- **CHR**: Up to 128KB ROM or 32KB RAM
- **PRG-RAM**: 8KB-32KB (often battery-backed)
- **Mirroring**: Software-controlled
- **Bank Switching**: Configurable 16KB or 32KB PRG; 4KB or 8KB CHR

**Coverage**: ~27.9% of licensed NES library (most common mapper)

---

## Serial Write Interface

Unlike other mappers, MMC1 uses a **5-bit serial port** to reduce chip pins. Writes occur one bit at a time.

### Write Sequence

1. Write bit 0 of data to $8000-$FFFF (5 times total)
2. On 5th write, the accumulated value loads into the target register

### Write Protocol

```rust
// Pseudo-code for serial write
if value & 0x80 != 0 {
    // Reset: Writing $80-$FF clears shift register
    shift_register = 0;
    write_count = 0;
} else {
    // Add bit to shift register
    shift_register |= (value & 0x01) << write_count;
    write_count += 1;

    if write_count == 5 {
        // Fifth write: Load into target register
        load_register(addr, shift_register);
        shift_register = 0;
        write_count = 0;
    }
}
```

### Example Assembly Code

```assembly
; Write $15 (10101 binary) to control register
LDA #$01   ; Bit 0
STA $8000
LDA #$00   ; Bit 1
STA $8000
LDA #$01   ; Bit 2
STA $8000
LDA #$00   ; Bit 3
STA $8000
LDA #$01   ; Bit 4 (fifth write triggers load)
STA $8000
```

---

## Registers

MMC1 has 4 internal registers accessible through the serial interface:

### Control Register ($8000-$9FFF)

```
Bits:  43210
       |||||
       |||++- Mirroring: 0=one-screen A, 1=one-screen B,
       |||               2=vertical, 3=horizontal
       ||+--- PRG mode:  0/1=32KB mode, 2=fix first bank, 3=fix last bank
       ++---- CHR mode:  0=8KB mode, 1=4KB mode
```

### CHR Bank 0 ($A000-$BFFF)

```
Bits:  43210
       |||||
       +++++- Select 4KB or 8KB CHR bank at $0000
```

### CHR Bank 1 ($C000-$DFFF)

```
Bits:  43210
       |||||
       +++++- Select 4KB CHR bank at $1000 (ignored in 8KB mode)
```

### PRG Bank ($E000-$FFFF)

```
Bits:  43210
       |||||
       ||||+- PRG RAM enable (0=enabled, 1=disabled)
       ++++-- Select 16KB PRG-ROM bank
```

---

## Banking Modes

### PRG Banking Modes

**Mode 0/1: 32KB switching**

```
CPU $8000-$FFFF: Switchable 32KB bank (ignore low bit of bank number)
```

**Mode 2: Fix first bank**

```
CPU $8000-$BFFF: Fixed to bank 0
CPU $C000-$FFFF: Switchable 16KB bank
```

**Mode 3: Fix last bank (most common)**

```
CPU $8000-$BFFF: Switchable 16KB bank
CPU $C000-$FFFF: Fixed to last bank
```

### CHR Banking Modes

**Mode 0: 8KB switching**

```
PPU $0000-$1FFF: Switchable 8KB bank (ignore low bit)
```

**Mode 1: Two 4KB banks**

```
PPU $0000-$0FFF: Switchable 4KB bank (CHR Bank 0)
PPU $1000-$1FFF: Switchable 4KB bank (CHR Bank 1)
```

---

## Implementation

```rust
pub struct MMC1 {
    prg_rom: Vec<u8>,
    chr_mem: Vec<u8>,
    prg_ram: Vec<u8>,

    // Serial interface
    shift_register: u8,
    write_count: u8,

    // Internal registers
    control: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,

    // Derived state
    chr_is_ram: bool,
    prg_ram_enabled: bool,
}

impl Mapper for MMC1 {
    fn write_prg(&mut self, addr: u16, value: u8) {
        if value & 0x80 != 0 {
            // Reset
            self.shift_register = 0;
            self.write_count = 0;
            self.control |= 0x0C; // Set to mode 3 (common default)
        } else {
            // Serial write
            self.shift_register |= (value & 0x01) << self.write_count;
            self.write_count += 1;

            if self.write_count == 5 {
                self.load_register(addr, self.shift_register);
                self.shift_register = 0;
                self.write_count = 0;
            }
        }
    }

    fn load_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.control = value,
            0xA000..=0xBFFF => self.chr_bank_0 = value,
            0xC000..=0xDFFF => self.chr_bank_1 = value,
            0xE000..=0xFFFF => {
                self.prg_bank = value & 0x0F;
                self.prg_ram_enabled = (value & 0x10) == 0;
            }
            _ => {}
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled {
                    let offset = (addr - 0x6000) as usize;
                    self.prg_ram[offset % self.prg_ram.len()]
                } else {
                    0 // Open bus or disabled
                }
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                let offset = ((addr & 0x3FFF) as usize) + (bank * 0x4000);
                self.prg_rom[offset % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn get_prg_bank(&self, addr: u16) -> usize {
        let prg_mode = (self.control >> 2) & 0x03;
        let num_banks = self.prg_rom.len() / 0x4000;

        match (prg_mode, addr) {
            (0..=1, _) => {
                // 32KB mode: Ignore low bit
                ((self.prg_bank & 0xFE) as usize) % num_banks
            }
            (2, 0x8000..=0xBFFF) => 0, // Fix first
            (2, 0xC000..=0xFFFF) => (self.prg_bank as usize) % num_banks,
            (3, 0x8000..=0xBFFF) => (self.prg_bank as usize) % num_banks,
            (3, 0xC000..=0xFFFF) => num_banks - 1, // Fix last
            _ => 0,
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::SingleScreenB,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        }
    }
}
```

---

## Games Using MMC1

| Game | Board | PRG | CHR | Year |
|------|-------|-----|-----|------|
| The Legend of Zelda | SLROM | 128KB | 128KB CHR-ROM | 1986 |
| Metroid | SLROM | 128KB | 128KB CHR-ROM | 1986 |
| Kid Icarus | SLROM | 128KB | 128KB CHR-ROM | 1987 |
| Mega Man 2 | SNROM | 128KB | 8KB CHR-RAM | 1988 |
| Final Fantasy | SNROM | 256KB | 8KB CHR-RAM | 1990 |
| Dragon Warrior IV | SUROM | 512KB | 8KB CHR-RAM | 1992 |

**Total**: ~580 games (~27.9% of NES library)

---

## References

- [NesDev Wiki: MMC1](https://www.nesdev.org/wiki/Nintendo_MMC1)
- [NesDev Wiki: Programming MMC1](https://www.nesdev.org/wiki/Programming_MMC1)
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md)

---

**Related Documents**:

- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md)
- [MAPPER_MMC3.md](MAPPER_MMC3.md)
- [MEMORY_MAP.md](../bus/MEMORY_MAP.md)
