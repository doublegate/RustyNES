# NES PPU: Complete 2C02 Specification

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete 2C02 PPU register behavior and internal state machine

---

## Table of Contents

- [Overview](#overview)
- [Register Specifications](#register-specifications)
- [Internal Registers](#internal-registers)
- [Register Behavior Details](#register-behavior-details)
- [Internal Latches and Buffers](#internal-latches-and-buffers)
- [Power-Up and Reset](#power-up-and-reset)
- [Register Decay](#register-decay)
- [Mirroring Configuration](#mirroring-configuration)
- [Open Bus Behavior](#open-bus-behavior)
- [Implementation Guide](#implementation-guide)

---

## Overview

The **2C02** (NTSC) PPU exposes 8 memory-mapped registers to the CPU at addresses $2000-$2007. These registers are **incompletely decoded** and mirror throughout $2000-$3FFF (every 8 bytes).

### PPU Variants

| Variant | Region | Clock | Frame Rate | Scanlines |
|---------|--------|-------|------------|-----------|
| **2C02** | NTSC | 5.369318 MHz | 60.098 Hz | 262 |
| **2C07** | PAL | 5.320342 MHz | 50.007 Hz | 312 |
| **2C03** | RGB (PlayChoice/VS) | 5.369318 MHz | 60.098 Hz | 262 |
| **2C04** | RGB (VS System variants) | 5.369318 MHz | 60.098 Hz | 262 |
| **2C05** | RGB (VS System variants) | 5.369318 MHz | 60.098 Hz | 262 |

### Clock Relationship

```
NTSC Master: 21.477272 MHz
PPU Clock = Master ÷ 4 = 5.369318 MHz
1 PPU dot = 1 PPU clock = ~186.3 ns

CPU Clock = Master ÷ 12 = 1.789773 MHz
1 CPU cycle = 3 PPU dots
```

---

## Register Specifications

### Memory Map

| Address | Register | Access | Mirrored To | Description |
|---------|----------|--------|-------------|-------------|
| **$2000** | PPUCTRL | W | $2000, $2008, $2010... $3FF8 | PPU control register |
| **$2001** | PPUMASK | W | $2001, $2009, $2011... $3FF9 | PPU mask register |
| **$2002** | PPUSTATUS | R | $2002, $200A, $2012... $3FFA | PPU status register |
| **$2003** | OAMADDR | W | $2003, $200B, $2013... $3FFB | OAM address port |
| **$2004** | OAMDATA | R/W | $2004, $200C, $2014... $3FFC | OAM data port |
| **$2005** | PPUSCROLL | W×2 | $2005, $200D, $2015... $3FFD | Fine scroll position |
| **$2006** | PPUADDR | W×2 | $2006, $200E, $2016... $3FFE | PPU address register |
| **$2007** | PPUDATA | R/W | $2007, $200F, $2017... $3FFF | PPU data port |
| **$4014** | OAMDMA | W | - | OAM DMA transfer (CPU) |

### PPUCTRL ($2000) - Write Only

```
7  bit  0
---- ----
VPHB SINN
|||| ||||
|||| ||++- Base nametable address
|||| ||    (0 = $2000; 1 = $2400; 2 = $2800; 3 = $2C00)
|||| |+--- VRAM address increment per CPU read/write of PPUDATA
|||| |     (0: add 1, going across; 1: add 32, going down)
|||| +---- Sprite pattern table address for 8×8 sprites
||||       (0: $0000; 1: $1000; ignored in 8×16 mode)
|||+------ Background pattern table address (0: $0000; 1: $1000)
||+------- Sprite size (0: 8×8 pixels; 1: 8×16 pixels)
|+-------- PPU master/slave select
|          (0: read backdrop from EXT pins; 1: output color on EXT pins)
+--------- Generate an NMI at the start of vblank (0: off; 1: on)
```

**Critical Warning:** Setting bit 6 (master/slave) on stock consoles can **damage the PPU** by shorting EXT pin outputs.

### PPUMASK ($2001) - Write Only

```
7  bit  0
---- ----
BGRs bMmG
|||| ||||
|||| |||+- Greyscale (0: normal color, 1: greyscale)
|||| ||+-- 1: Show background in leftmost 8 pixels of screen, 0: Hide
|||| |+--- 1: Show sprites in leftmost 8 pixels of screen, 0: Hide
|||| +---- 1: Show background
|||+------ 1: Show sprites
||+------- Emphasize red (green on PAL/Dendy)
|+-------- Emphasize green (red on PAL/Dendy)
+--------- Emphasize blue
```

### PPUSTATUS ($2002) - Read Only

```
7  bit  0
---- ----
VSO- ----
|||| ||||
|||+-++++- PPU open bus (returns last value written to any PPU register)
||+------- Sprite overflow flag
|+-------- Sprite 0 hit flag
+--------- Vertical blank flag
           Set at dot 1 of line 241 (vblank start)
           Cleared at dot 1 of pre-render line or after reading $2002
```

**Side Effects:**

- Reading $2002 **clears bit 7 (V flag)** immediately
- Reading $2002 **clears the write latch** for $2005/$2006
- Race condition: Reading $2002 within one PPU cycle of V flag being set may clear it before NMI is triggered

### OAMADDR ($2003) - Write Only

```
7  bit  0
---- ----
AAAA AAAA
|||| ||||
++++-++++- OAM address (0-255)
```

**Behavior:**

- Sets address for OAMDATA reads/writes
- Automatically incremented after write to $2004
- **Corruption bug (2C02G):** Writing to OAMADDR corrupts OAM

### OAMDATA ($2004) - Read/Write

**Write:**

- Writes byte to OAM[OAMADDR]
- Increments OAMADDR after write
- Writing during rendering corrupts OAM (avoid!)

**Read:**

- Returns OAM[OAMADDR]
- Does **not** increment OAMADDR
- Not readable on original 2C02, only 2C02G and later

**Recommended Usage:** Use $4014 DMA instead of manual writes.

### PPUSCROLL ($2005) - Write ×2

First write (w=0): Horizontal scroll

```
7  bit  0
---- ----
XXXX XXXX
|||| ||||
++++-++++- Fine X scroll (0-255)
           Splits into coarse_x (upper 5 bits) and fine_x (lower 3 bits)
```

Second write (w=1): Vertical scroll

```
7  bit  0
---- ----
YYYY YYYY
|||| ||||
++++-++++- Fine Y scroll (0-255)
           Splits into coarse_y (upper 5 bits) and fine_y (lower 3 bits)
```

**Side Effects:**

- Toggles write latch between 0 and 1
- Affects internal registers `t` and `v`
- Read $2002 to reset latch to first write

### PPUADDR ($2006) - Write ×2

First write (w=0): High byte

```
7  bit  0
---- ----
--AA AAAA
  || ||||
  ++-++++- High 6 bits of PPU address (bits 8-13)
```

Second write (w=1): Low byte

```
7  bit  0
---- ----
AAAA AAAA
|||| ||||
++++-++++- Low 8 bits of PPU address (bits 0-7)
```

**Side Effects:**

- First write: Sets high byte of temporary VRAM address `t`
- Second write: Sets low byte of `t` and copies `t` to `v`
- Address auto-increments after $2007 access

### PPUDATA ($2007) - Read/Write

**Write:**

- Writes to PPU address space at `v`
- Increments `v` by 1 or 32 (based on PPUCTRL bit 2)

**Read:**

- Returns **buffered data** from previous read (except palette)
- Palette reads ($3F00-$3FFF) return immediately but still update buffer
- Increments `v` by 1 or 32

**Buffer Behavior:**

```
First read:  Returns garbage, loads actual data into buffer
Second read: Returns previous data, loads new data into buffer
```

**Palette Exception:**

```
Read $3F00: Returns palette value immediately
            Buffer contains mirrored nametable data from $2F00
```

---

## Internal Registers

The PPU maintains several internal registers not directly accessible via CPU:

### v - Current VRAM Address (15 bits)

```
yyy NN YYYYY XXXXX
||| || ||||| +++++- Coarse X scroll (0-31)
||| || +++++------- Coarse Y scroll (0-31)
||| ++------------- Nametable select (0-3)
+++---------------- Fine Y scroll (0-7)
```

**Usage:**

- Current read/write address for VRAM
- Updated during rendering for scrolling
- Set via $2006 writes

### t - Temporary VRAM Address (15 bits)

```
yyy NN YYYYY XXXXX
```

**Usage:**

- Holds scroll/address data before transfer to `v`
- Updated by $2005 and $2006 writes
- Copied to `v` at specific rendering times

### x - Fine X Scroll (3 bits)

```
7  bit  0
---- ----
---- -XXX
      |||
      +++- Fine X scroll (0-7)
```

**Usage:**

- Holds fine X scroll (lower 3 bits of horizontal scroll)
- Used for pixel selection from tile shift registers
- Set by first $2005 write

### w - Write Latch (1 bit)

```
0 or 1
```

**Usage:**

- Toggles between first/second write for $2005 and $2006
- Reset by reading $2002
- 0 = first write, 1 = second write

---

## Register Behavior Details

### Write to $2005 (PPUSCROLL)

#### First Write (w=0)

```rust
// $2005 first write: X scroll
t = (t & 0xFFE0) | (data >> 3);  // Coarse X = data[7:3]
x = data & 0x07;                  // Fine X = data[2:0]
w = 1;                            // Toggle latch
```

#### Second Write (w=1)

```rust
// $2005 second write: Y scroll
t = (t & 0x8FFF) | ((data & 0x07) << 12);  // Fine Y = data[2:0]
t = (t & 0xFC1F) | ((data & 0xF8) << 2);   // Coarse Y = data[7:3]
w = 0;                                      // Toggle latch
```

### Write to $2006 (PPUADDR)

#### First Write (w=0)

```rust
// $2006 first write: high byte
t = (t & 0x00FF) | ((data & 0x3F) << 8);  // High 6 bits
t = t & 0x7FFF;                            // Clear bit 14
w = 1;                                      // Toggle latch
```

#### Second Write (w=1)

```rust
// $2006 second write: low byte
t = (t & 0xFF00) | data;  // Low 8 bits
v = t;                     // Copy t to v
w = 0;                     // Toggle latch
```

### Read from $2002 (PPUSTATUS)

```rust
// Read PPUSTATUS
let value = (self.status & 0xE0) | (self.open_bus & 0x1F);
self.status &= 0x7F;  // Clear V flag (bit 7)
w = 0;                 // Reset write latch
return value;
```

### Read/Write $2007 (PPUDATA)

#### Write

```rust
// Write PPUDATA
ppu_memory[v] = data;
v += if ppuctrl & 0x04 { 32 } else { 1 };  // Increment
v &= 0x3FFF;  // Wrap to 14-bit address
```

#### Read

```rust
// Read PPUDATA
let addr = v & 0x3FFF;
let value = if addr >= 0x3F00 {
    // Palette: return immediately
    let pal_value = palette[addr & 0x1F];
    // But still fill buffer with mirrored nametable
    buffer = vram[addr & 0x2FFF];
    pal_value
} else {
    // Normal: return buffer, load new data
    let old_buffer = buffer;
    buffer = vram[addr];
    old_buffer
};
v += if ppuctrl & 0x04 { 32 } else { 1 };
v &= 0x3FFF;
return value;
```

---

## Internal Latches and Buffers

### Pattern Table Shifters (16-bit each)

```rust
struct BackgroundShifters {
    pattern_lo: u16,  // Low bitplane
    pattern_hi: u16,  // High bitplane
    attrib_lo: u16,   // Attribute low bits
    attrib_hi: u16,   // Attribute high bits
}
```

**Usage:**

- Shift left by 1 each dot (dots 2-257, 322-337)
- Reloaded every 8 dots with next tile data

### Sprite Shifters (8-bit each, 8 sprites)

```rust
struct SpriteShifters {
    pattern_lo: [u8; 8],  // Low bitplane per sprite
    pattern_hi: [u8; 8],  // High bitplane per sprite
}
```

**Usage:**

- Shift right by 1 each dot when sprite counter reaches 0
- Loaded during sprite fetch (dots 257-320)

### Internal Data Bus (8-bit)

Holds last value read/written to PPU registers for open bus behavior.

---

## Power-Up and Reset

### Power-Up State

```
PPUCTRL   = $00
PPUMASK   = $00
PPUSTATUS = $A0 (V=1, S=0, O=1)
OAMADDR   = $00
v         = $0000
t         = $0000
x         = 0
w         = 0
buffer    = $00
OAM       = Random values
```

### Write Ignored Period

After power-up, writes to PPUCTRL/PPUMASK/PPUSCROLL/PPUADDR are **ignored** for:

- **NTSC:** ~29,658 CPU cycles (~88,974 PPU dots)
- **PAL:** ~33,132 CPU cycles

This is approximately **2 frames**.

**Recommendation:** Wait for 2 VBlanks before initializing PPU.

### RESET Behavior

On console reset (not power-up):

- PPUCTRL, PPUMASK unchanged
- PPUSCROLL, PPUADDR: write latch reset
- PPUDATA: read buffer unchanged
- OAM unchanged

---

## Register Decay

### OAM Decay

OAM uses **dynamic RAM** that requires refresh. Without rendering enabled:

- **NTSC:** Decay starts after a few seconds
- **PAL:** Forced refresh occurs 24 scanlines after VBlank

**Mitigation:** Enable rendering or periodically write OAM.

### Open Bus Decay

Values returned from unimplemented bits decay after 600-800 ms.

---

## Mirroring Configuration

The PPU's address space includes 4 nametables at $2000-$2FFF. Cartridges provide 2KB of VRAM (2 nametables), requiring **mirroring**:

### Horizontal Mirroring

```
[A][A]
[B][B]

$2000-$23FF = A
$2400-$27FF = B
$2800-$2BFF = A (mirror)
$2C00-$2FFF = B (mirror)
```

Used for vertical scrolling (e.g., Super Mario Bros).

### Vertical Mirroring

```
[A][B]
[A][B]

$2000-$23FF = A
$2400-$27FF = B
$2800-$2BFF = A (mirror)
$2C00-$2FFF = B (mirror)
```

Used for horizontal scrolling (e.g., Ice Climber).

### Four-Screen

Some mappers provide full 4KB VRAM:

```
[A][B]
[C][D]

No mirroring
```

### Single-Screen

```
[A][A]
[A][A]

All nametables mirror to same RAM
```

---

## Open Bus Behavior

Unmapped or unimplemented register bits return the **last value on the PPU data bus**.

### Open Bus Sources

1. **PPUSTATUS bits 0-4:** Return last PPU write
2. **Unmapped addresses:** Return last PPU write
3. **PPUDATA read buffer:** Persists across reads

### Example

```
Write $2000 = $FF  -> Open bus = $FF
Read  $2002        -> Returns $Ex (where x = $F)
```

---

## Implementation Guide

### Recommended PPU Structure

```rust
pub struct Ppu {
    // Registers
    ppuctrl: u8,
    ppumask: u8,
    ppustatus: u8,
    oamaddr: u8,

    // Internal registers
    v: u16,        // Current VRAM address
    t: u16,        // Temporary VRAM address
    x: u8,         // Fine X scroll (3 bits)
    w: bool,       // Write latch

    // Buffers
    read_buffer: u8,
    open_bus: u8,

    // Memory
    oam: [u8; 256],
    palette: [u8; 32],

    // Timing
    scanline: u16,
    dot: u16,
}
```

### Register Write Implementation

```rust
impl Ppu {
    pub fn write_register(&mut self, addr: u16, value: u8) {
        self.open_bus = value;

        match addr & 0x2007 {
            0x2000 => {
                self.ppuctrl = value;
                // t: ....BA.. ........ = d: ......BA
                self.t = (self.t & 0xF3FF) | (((value & 0x03) as u16) << 10);
            }
            0x2001 => self.ppumask = value,
            0x2003 => self.oamaddr = value,
            0x2004 => {
                self.oam[self.oamaddr as usize] = value;
                self.oamaddr = self.oamaddr.wrapping_add(1);
            }
            0x2005 => {
                if !self.w {
                    // First write: X scroll
                    self.t = (self.t & 0xFFE0) | ((value >> 3) as u16);
                    self.x = value & 0x07;
                } else {
                    // Second write: Y scroll
                    self.t = (self.t & 0x8FFF) | (((value & 0x07) as u16) << 12);
                    self.t = (self.t & 0xFC1F) | (((value & 0xF8) as u16) << 2);
                }
                self.w = !self.w;
            }
            0x2006 => {
                if !self.w {
                    // First write: high byte
                    self.t = (self.t & 0x00FF) | (((value & 0x3F) as u16) << 8);
                } else {
                    // Second write: low byte
                    self.t = (self.t & 0xFF00) | (value as u16);
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            0x2007 => {
                self.write_vram(self.v, value);
                self.v += if self.ppuctrl & 0x04 != 0 { 32 } else { 1 };
                self.v &= 0x3FFF;
            }
            _ => {}
        }
    }
}
```

---

## Related Documentation

- [PPU_TIMING_DIAGRAM.md](PPU_TIMING_DIAGRAM.md) - Dot-by-dot rendering timing
- [PPU_SCROLLING_INTERNALS.md](PPU_SCROLLING_INTERNALS.md) - Loopy's scrolling implementation
- [PPU_RENDERING.md](PPU_RENDERING.md) - Rendering pipeline
- [PPU_OVERVIEW.md](PPU_OVERVIEW.md) - High-level PPU architecture

---

## References

- [NESdev Wiki: PPU Registers](https://www.nesdev.org/wiki/PPU_registers)
- [NESdev Wiki: PPU Programmer Reference](https://www.nesdev.org/wiki/PPU_programmer_reference)
- [NESdev Wiki: PPU Power-Up State](https://www.nesdev.org/wiki/PPU_power_up_state)
- [2C02 Technical Reference](http://nesdev.com/2C02%20technical%20reference.TXT)
- [Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02) - Transistor-level simulator

---

**Document Status:** Complete 2C02 register specification with cycle-accurate behavior.
