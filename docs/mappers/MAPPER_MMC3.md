# Mapper 4: MMC3 (TxROM)

**Table of Contents**
- [Overview](#overview)
- [Register Map](#register-map)
- [Banking Modes](#banking-modes)
- [Scanline IRQ Counter](#scanline-irq-counter)
- [Implementation](#implementation)
- [Games Using MMC3](#games-using-mmc3)
- [References](#references)

---

## Overview

**MMC3** is Nintendo's second most popular mapper, featuring fine-grained banking (2KB/8KB banks), dynamic mirroring, and a **scanline IRQ counter** enabling split-screen effects. Used in many flagship titles.

### Key Characteristics

- **Mapper Number**: 4
- **Board Names**: TxROM (TKROM, TLROM, TR1ROM, TSROM, TVROM)
- **PRG-ROM**: Up to 512KB (64 banks of 8KB)
- **CHR**: Up to 256KB (256 banks of 1KB/2KB)
- **PRG-RAM**: 8KB (battery-backed optional)
- **Mirroring**: Software-controlled H/V
- **IRQ**: Scanline counter (PPU A12 rising edge)

**Coverage**: ~23.4% of NES library (second most common)

---

## Register Map

MMC3 uses **register pairs** at even/odd addresses:

| Address Range | Even ($x000) | Odd ($x001) |
|---------------|--------------|-------------|
| $8000-$9FFF | Bank select | Bank data |
| $A000-$BFFF | Mirroring | PRG-RAM protect |
| $C000-$DFFF | IRQ latch | IRQ reload |
| $E000-$FFFF | IRQ disable | IRQ enable |

### Bank Select ($8000)

```
Bits:  76543210
       ||||||||
       ||||||++- Bank register to update on next write to $8001
       |||||||   0: R0 (2KB CHR @ $0000)
       |||||||   1: R1 (2KB CHR @ $0800)
       |||||||   2: R2 (1KB CHR @ $1000)
       |||||||   3: R3 (1KB CHR @ $1400)
       |||||||   4: R4 (1KB CHR @ $1800)
       |||||||   5: R5 (1KB CHR @ $1C00)
       |||||||   6: R6 (8KB PRG @ $8000 or $C000)
       |||||||   7: R7 (8KB PRG @ $A000)
       |||||+--- PRG banking mode (0=$8000 switchable, 1=$C000 switchable)
       +++++---- CHR A12 inversion (affects CHR bank mapping)
```

### Bank Data ($8001)

Write the bank number for the register selected in $8000.

### Mirroring ($A000)

```
Bit 0: 0 = Vertical, 1 = Horizontal
```

### IRQ Latch ($C000)

Specifies the IRQ counter reload value.

### IRQ Reload ($C001)

Writing any value reloads the counter on the next scanline.

### IRQ Disable/Enable ($E000/$E001)

$E000 = Disable IRQ, $E001 = Enable IRQ

---

## Banking Modes

### PRG Banking

**Mode 0 (PRG Mode = 0)**:
```
CPU $8000-$9FFF: Switchable 8KB bank (R6)
CPU $A000-$BFFF: Switchable 8KB bank (R7)
CPU $C000-$DFFF: Fixed to second-last bank
CPU $E000-$FFFF: Fixed to last bank
```

**Mode 1 (PRG Mode = 1)**:
```
CPU $8000-$9FFF: Fixed to second-last bank
CPU $A000-$BFFF: Switchable 8KB bank (R7)
CPU $C000-$DFFF: Switchable 8KB bank (R6)
CPU $E000-$FFFF: Fixed to last bank
```

### CHR Banking

**Normal (CHR A12 = 0)**:
```
PPU $0000-$07FF: Switchable 2KB bank (R0)
PPU $0800-$0FFF: Switchable 2KB bank (R1)
PPU $1000-$13FF: Switchable 1KB bank (R2)
PPU $1400-$17FF: Switchable 1KB bank (R3)
PPU $1800-$1BFF: Switchable 1KB bank (R4)
PPU $1C00-$1FFF: Switchable 1KB bank (R5)
```

**Inverted (CHR A12 = 1)**: Swap $0000 and $1000 regions

---

## Scanline IRQ Counter

The MMC3's signature feature is a **scanline counter** that generates IRQs at specific screen positions.

### How It Works

1. **PPU A12 Monitoring**: Counts rising edges of PPU A12 line
2. **Automatic Counting**: Increments during rendering (background/sprites active)
3. **IRQ Trigger**: Fires when counter reaches 0

### Register Usage

**Setup**:
```assembly
LDA #239      ; Trigger at scanline 240 (VBlank start)
STA $C000     ; Set IRQ latch
STA $C001     ; Reload counter
LDA #$01
STA $E001     ; Enable IRQ
```

**IRQ Handler**:
```assembly
IRQHandler:
    ; Acknowledge IRQ
    LDA $E000     ; Disable IRQ
    LDA $E001     ; Re-enable IRQ

    ; Perform mid-frame update (scroll change, bank switch, etc.)
    LDA #new_scroll
    STA $2005
    STA $2005

    RTI
```

### Use Cases

- **Status bars**: Super Mario Bros. 3 bottom UI
- **Split-screen**: Different scroll regions
- **Raster effects**: Per-scanline palette changes

### Revision Differences

| Revision | IRQ Latch = 0 Behavior |
|----------|------------------------|
| Sharp MMC3 | IRQ every scanline |
| NEC MMC3 | No IRQs |

**Games relying on Sharp**: Star Trek 25th Anniversary

---

## Implementation

```rust
pub struct MMC3 {
    prg_rom: Vec<u8>,
    chr_mem: Vec<u8>,
    prg_ram: Vec<u8>,

    // Banking registers
    bank_select: u8,
    bank_registers: [u8; 8],

    // IRQ counter
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,

    // State
    mirroring: Mirroring,
    prg_ram_protect: bool,
    chr_is_ram: bool,
}

impl Mapper for MMC3 {
    fn write_prg(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if addr & 0x01 == 0 {
                    self.bank_select = value; // Even: Bank select
                } else {
                    // Odd: Bank data
                    let reg = (self.bank_select & 0x07) as usize;
                    self.bank_registers[reg] = value;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 0x01 == 0 {
                    // Mirroring
                    self.mirroring = if value & 0x01 == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                } else {
                    // PRG-RAM protect
                    self.prg_ram_protect = (value & 0x80) != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 0x01 == 0 {
                    self.irq_latch = value; // IRQ latch
                } else {
                    self.irq_reload = true; // IRQ reload
                }
            }
            0xE000..=0xFFFF => {
                if addr & 0x01 == 0 {
                    self.irq_enabled = false; // Disable IRQ
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true; // Enable IRQ
                }
            }
            _ => {}
        }
    }

    fn notify_scanline(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let bank = self.get_prg_bank(addr);
        let offset = ((addr & 0x1FFF) as usize) + (bank * 0x2000);
        self.prg_rom[offset % self.prg_rom.len()]
    }

    fn get_prg_bank(&self, addr: u16) -> usize {
        let prg_mode = (self.bank_select >> 6) & 0x01;
        let num_banks = self.prg_rom.len() / 0x2000;

        match (addr, prg_mode) {
            (0x8000..=0x9FFF, 0) => self.bank_registers[6] as usize,
            (0x8000..=0x9FFF, 1) => num_banks - 2,
            (0xA000..=0xBFFF, _) => self.bank_registers[7] as usize,
            (0xC000..=0xDFFF, 0) => num_banks - 2,
            (0xC000..=0xDFFF, 1) => self.bank_registers[6] as usize,
            (0xE000..=0xFFFF, _) => num_banks - 1,
            _ => 0,
        }
    }
}
```

---

## Games Using MMC3

| Game | Year | Notable Use |
|------|------|-------------|
| **Super Mario Bros. 3** | 1990 | Status bar (scanline IRQ) |
| **Mega Man 3-6** | 1990-93 | Standard usage |
| **Kirby's Adventure** | 1993 | Advanced CHR banking |
| **Contra** | 1988 | Split-screen effects |
| **TMNT II: The Arcade Game** | 1990 | Smooth scrolling |
| **Star Trek 25th Anniversary** | 1992 | Requires Sharp MMC3 |

**Total**: ~485 games (~23.4% of NES library)

---

## References

- [NesDev Wiki: MMC3](https://www.nesdev.org/wiki/MMC3)
- [NesDev Wiki: TxROM](https://www.nesdev.org/wiki/TxROM)
- [Kevtris MMC3 Documentation](http://kevtris.org/mappers/mmc3/index.html)
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md)

---

**Related Documents**:
- [MAPPER_OVERVIEW.md](MAPPER_OVERVIEW.md)
- [MAPPER_MMC1.md](MAPPER_MMC1.md)
- [PPU_TIMING.md](../ppu/PPU_TIMING.md) - Scanline timing
