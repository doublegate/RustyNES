# PPU Scrolling (Loopy's Model)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Internal Registers](#internal-registers)
- [VRAM Address Register Structure](#vram-address-register-structure)
- [Register Writes](#register-writes)
- [Scrolling During Rendering](#scrolling-during-rendering)
- [Nametable Mirroring](#nametable-mirroring)
- [Common Scrolling Patterns](#common-scrolling-patterns)
- [Implementation Guide](#implementation-guide)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

The PPU implements **hardware scrolling** via a complex internal register system discovered and documented by **Loopy** (Brad Taylor). Understanding this system is critical for accurate PPU emulation.

**Key Concepts:**

- **Two 15-bit internal registers**: `v` (current VRAM address) and `t` (temporary address)
- **Fine X scroll**: 3-bit register for horizontal fine scrolling
- **Write latch**: Toggle for $2005/$2006 two-write sequence
- **Automatic updates**: Registers update during rendering at specific dots

**Scrolling Range:**

- **Horizontal**: 0-511 pixels (2 nametables wide)
- **Vertical**: 0-479 pixels (2 nametables tall)

---

## Internal Registers

### The Five Scrolling Components

```rust
pub struct ScrollRegisters {
    v: u16,           // Current VRAM address (15 bits)
    t: u16,           // Temporary VRAM address (15 bits)
    fine_x: u8,       // Fine X scroll (3 bits, 0-7)
    write_latch: bool, // First/second write toggle
}
```

**Register Purposes:**

| Register | Name | Purpose |
|----------|------|---------|
| **v** | Current VRAM Address | Active address during rendering |
| **t** | Temporary Address | Holds values written to $2005/$2006 until copied to v |
| **fine_x** | Fine X Scroll | Horizontal fine scroll (0-7 pixels) |
| **write_latch** | Write Toggle | Tracks first/second write to $2005/$2006 |

---

## VRAM Address Register Structure

Both `v` and `t` are **15-bit registers** with the following bit layout:

```
yyy NN YYYYY XXXXX
||| || ||||| +++++- Coarse X scroll (tile column, 0-31)
||| || +++++------- Coarse Y scroll (tile row, 0-29)
||| ++------------- Nametable select (0-3)
+++---------------- Fine Y scroll (pixel row within tile, 0-7)

Bit positions:
14 13 12 11 10 09 08 07 06 05 04 03 02 01 00
y  y  y  N  N  Y  Y  Y  Y  Y  X  X  X  X  X
```

**Detailed Breakdown:**

| Bits | Name | Range | Description |
|------|------|-------|-------------|
| **14-12** | Fine Y | 0-7 | Vertical scroll within tile (pixel row) |
| **11-10** | Nametable | 0-3 | Nametable selection (NN = YX) |
| **9-5** | Coarse Y | 0-31 | Tile row (0-29 valid, 30-31 attribute table) |
| **4-0** | Coarse X | 0-31 | Tile column |

**Bit Masks:**

```rust
const COARSE_X_MASK: u16 = 0x001F;  // Bits 0-4
const COARSE_Y_MASK: u16 = 0x03E0;  // Bits 5-9
const NAMETABLE_X: u16   = 0x0400;  // Bit 10
const NAMETABLE_Y: u16   = 0x0800;  // Bit 11
const FINE_Y_MASK: u16   = 0x7000;  // Bits 12-14
```

### Extracting Components

```rust
impl ScrollRegisters {
    fn coarse_x(&self) -> u8 {
        (self.v & 0x001F) as u8
    }

    fn coarse_y(&self) -> u8 {
        ((self.v & 0x03E0) >> 5) as u8
    }

    fn nametable_x(&self) -> u8 {
        ((self.v & 0x0400) >> 10) as u8
    }

    fn nametable_y(&self) -> u8 {
        ((self.v & 0x0800) >> 11) as u8
    }

    fn fine_y(&self) -> u8 {
        ((self.v & 0x7000) >> 12) as u8
    }
}
```

---

## Register Writes

### $2005 - PPUSCROLL (Write ×2)

PPUSCROLL must be written **twice** - first for X scroll, then for Y scroll.

#### First Write (X Scroll)

```rust
fn write_scroll_x(&mut self, value: u8) {
    // Coarse X = value[7:3]
    self.t = (self.t & 0xFFE0) | ((value as u16) >> 3);

    // Fine X = value[2:0]
    self.fine_x = value & 0x07;

    // Toggle latch
    self.write_latch = true;
}
```

**Bit Assignment:**

```
Value:    HGFEDCBA
          |||||+++- Fine X scroll (3 bits) → fine_x register
          +++++---- Coarse X scroll (5 bits) → t[4:0]
```

#### Second Write (Y Scroll)

```rust
fn write_scroll_y(&mut self, value: u8) {
    // Coarse Y = value[7:3]
    self.t = (self.t & 0xFC1F) | (((value as u16) & 0xF8) << 2);

    // Fine Y = value[2:0]
    self.t = (self.t & 0x8FFF) | (((value as u16) & 0x07) << 12);

    // Toggle latch
    self.write_latch = false;
}
```

**Bit Assignment:**

```
Value:    HGFEDCBA
          |||||+++- Fine Y scroll (3 bits) → t[14:12]
          +++++---- Coarse Y scroll (5 bits) → t[9:5]
```

### $2006 - PPUADDR (Write ×2)

PPUADDR must be written **twice** - first for high byte, then for low byte.

#### First Write (High Byte)

```rust
fn write_addr_high(&mut self, value: u8) {
    // t[13:8] = value[5:0]
    self.t = (self.t & 0x00FF) | (((value as u16) & 0x3F) << 8);

    // Clear bit 14
    self.t &= 0x3FFF;

    // Toggle latch
    self.write_latch = true;
}
```

**Bit Assignment:**

```
Value:    ..FEDCBA (only low 6 bits used)
            ||||||
            ++++++-- VRAM address high byte → t[13:8]
```

#### Second Write (Low Byte)

```rust
fn write_addr_low(&mut self, value: u8) {
    // t[7:0] = value[7:0]
    self.t = (self.t & 0xFF00) | (value as u16);

    // Copy t to v
    self.v = self.t;

    // Toggle latch
    self.write_latch = false;
}
```

**Important:** The second write to $2006 **immediately copies `t` to `v`**, making the address active for $2007 reads/writes.

### $2002 - PPUSTATUS (Read)

Reading PPUSTATUS has a critical side effect:

```rust
fn read_status(&mut self) -> u8 {
    let status = self.status;

    // Clear VBlank flag
    self.status &= 0x7F;

    // Reset write latch
    self.write_latch = false;

    status
}
```

**Critical:** Resetting the write latch means the next write to $2005/$2006 will be treated as the first write.

---

## Scrolling During Rendering

The PPU automatically updates the scroll registers during rendering at specific times.

### Coarse X Increment (Every 8 Dots)

After fetching a tile (every 8 dots), increment coarse X:

```rust
fn increment_coarse_x(&mut self) {
    if (self.v & 0x001F) == 31 {
        // Coarse X wraps, switch horizontal nametable
        self.v &= !0x001F;        // Reset coarse X to 0
        self.v ^= 0x0400;         // Toggle nametable X bit
    } else {
        self.v += 1;              // Increment coarse X
    }
}
```

**When:** Dots 8, 16, 24, ..., 248, 256 (and 328, 336 for pre-fetch)

### Fine Y Increment (Dot 256)

At the end of each scanline, increment the Y position:

```rust
fn increment_y(&mut self) {
    if (self.v & 0x7000) != 0x7000 {
        // Fine Y < 7, just increment
        self.v += 0x1000;
    } else {
        // Fine Y wraps, increment coarse Y
        self.v &= !0x7000;  // Reset fine Y to 0

        let mut coarse_y = (self.v & 0x03E0) >> 5;

        match coarse_y {
            29 => {
                // Wrap to next nametable
                coarse_y = 0;
                self.v ^= 0x0800;  // Toggle nametable Y bit
            }
            31 => {
                // Out of bounds (attribute table area)
                coarse_y = 0;  // Wrap without toggling nametable
            }
            _ => {
                coarse_y += 1;
            }
        }

        self.v = (self.v & !0x03E0) | (coarse_y << 5);
    }
}
```

**When:** Dot 256 of scanlines 0-239 and 261 (if rendering enabled)

### Horizontal Scroll Copy (Dot 257)

At the end of each scanline, copy horizontal scroll from `t` to `v`:

```rust
fn copy_horizontal_scroll(&mut self) {
    // v[4:0] = t[4:0] (coarse X)
    // v[10] = t[10] (nametable X)
    self.v = (self.v & 0xFBE0) | (self.t & 0x041F);
}
```

**When:** Dot 257 of scanlines 0-239 and 261 (if rendering enabled)

**Effect:** Resets horizontal position to the value written to $2005, ensuring each scanline starts at the same X position.

### Vertical Scroll Copy (Dots 280-304, Scanline 261)

During pre-render scanline, copy vertical scroll from `t` to `v`:

```rust
fn copy_vertical_scroll(&mut self) {
    // v[14:12] = t[14:12] (fine Y)
    // v[11] = t[11] (nametable Y)
    // v[9:5] = t[9:5] (coarse Y)
    self.v = (self.v & 0x841F) | (self.t & 0x7BE0);
}
```

**When:** Dots 280-304 of scanline 261 (if rendering enabled)

**Effect:** Resets vertical position to the value written to $2005, preparing for the next frame.

---

## Nametable Mirroring

The NES has only **2 KB of internal VRAM**, enough for 2 nametables. The other 2 are **mirrored** based on cartridge wiring.

### Mirroring Modes

#### Horizontal Mirroring

```
Physical VRAM:  [ A ] [ B ]
Logical Layout:

    [ A ] [ B ]   ← Nametables 0, 1
    [ A ] [ B ]   ← Nametables 2, 3 (mirrors)
```

**Address Mapping:**

```rust
fn horizontal_mirror(addr: u16) -> u16 {
    match (addr >> 10) & 0x03 {
        0 => 0x0000, // $2000 → VRAM A
        1 => 0x0400, // $2400 → VRAM B
        2 => 0x0000, // $2800 → VRAM A (mirror)
        3 => 0x0400, // $2C00 → VRAM B (mirror)
        _ => unreachable!(),
    }
}
```

**Use Case:** Vertical scrolling games (Super Mario Bros., Mega Man)

#### Vertical Mirroring

```
Physical VRAM:  [ A ] [ B ]
Logical Layout:

    [ A ] [ A ]   ← Nametables 0, 2
    [ B ] [ B ]   ← Nametables 1, 3
```

**Address Mapping:**

```rust
fn vertical_mirror(addr: u16) -> u16 {
    match (addr >> 10) & 0x03 {
        0 => 0x0000, // $2000 → VRAM A
        1 => 0x0400, // $2400 → VRAM B
        2 => 0x0000, // $2800 → VRAM A (mirror)
        3 => 0x0400, // $2C00 → VRAM B (mirror)
        _ => unreachable!(),
    }
}
```

**Use Case:** Horizontal scrolling games (Metroid, Zelda)

#### Single-Screen Mirroring

All nametables mirror the same 1 KB:

```
Physical VRAM:  [ A ]
Logical Layout:

    [ A ] [ A ]
    [ A ] [ A ]
```

**Use Case:** Fixed-screen games or games with custom mirroring logic

#### Four-Screen Mirroring

Cartridge provides 4 KB of VRAM (no mirroring):

```
Physical VRAM:  [ A ] [ B ] [ C ] [ D ]
Logical Layout:

    [ A ] [ B ]
    [ C ] [ D ]
```

**Use Case:** Games with advanced scrolling (Gauntlet, Rad Racer II)

---

## Common Scrolling Patterns

### 1. Vertical Scrolling (Full Screen)

```assembly
LDA #$00
STA $2005  ; X scroll = 0
LDA scroll_y
STA $2005  ; Y scroll = scroll_y
```

**Result:** Vertical scrolling with fixed horizontal position.

### 2. Horizontal Scrolling (Full Screen)

```assembly
LDA scroll_x
STA $2005  ; X scroll = scroll_x
LDA #$00
STA $2005  ; Y scroll = 0
```

**Result:** Horizontal scrolling with fixed vertical position.

### 3. Split-Screen Scrolling

Using sprite 0 hit to detect mid-frame:

```assembly
@wait_sprite0:
    BIT $2002
    BVC @wait_sprite0

; Sprite 0 hit! Change scroll position
LDA status_x
STA $2005
LDA status_y
STA $2005
```

**Result:** Status bar at top with different scroll position than playfield.

### 4. Full 2D Scrolling

```assembly
LDA scroll_x
STA $2005
LDA scroll_y
STA $2005
```

**Result:** Free scrolling in all directions (within 512×480 scroll range).

---

## Implementation Guide

### Core Scroll State

```rust
pub struct Ppu {
    // Loopy registers
    vram_addr: u16,      // v register
    temp_addr: u16,      // t register
    fine_x: u8,          // Fine X scroll (0-7)
    write_latch: bool,   // First/second write toggle

    // ... other PPU state
}
```

### Register Write Handlers

```rust
impl Ppu {
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x07 {
            0x05 => self.write_scroll(value),
            0x06 => self.write_addr(value),
            _ => { /* other registers */ }
        }
    }

    fn write_scroll(&mut self, value: u8) {
        if !self.write_latch {
            // First write - X scroll
            self.temp_addr = (self.temp_addr & 0xFFE0) | ((value as u16) >> 3);
            self.fine_x = value & 0x07;
        } else {
            // Second write - Y scroll
            self.temp_addr = (self.temp_addr & 0xFC1F) | (((value as u16) & 0xF8) << 2);
            self.temp_addr = (self.temp_addr & 0x8FFF) | (((value as u16) & 0x07) << 12);
        }

        self.write_latch = !self.write_latch;
    }

    fn write_addr(&mut self, value: u8) {
        if !self.write_latch {
            // First write - high byte
            self.temp_addr = (self.temp_addr & 0x00FF) | (((value as u16) & 0x3F) << 8);
            self.temp_addr &= 0x3FFF;
        } else {
            // Second write - low byte
            self.temp_addr = (self.temp_addr & 0xFF00) | (value as u16);
            self.vram_addr = self.temp_addr; // Copy t to v
        }

        self.write_latch = !self.write_latch;
    }
}
```

### Rendering Updates

```rust
impl Ppu {
    pub fn step(&mut self) {
        let rendering_enabled = (self.mask & 0x18) != 0;

        match (self.scanline, self.dot) {
            // Coarse X increment every 8 dots
            (0..=239, dot) | (261, dot) if dot % 8 == 0 && dot <= 256 && rendering_enabled => {
                self.increment_coarse_x();
            }

            // Fine Y increment at end of scanline
            (0..=239, 256) | (261, 256) if rendering_enabled => {
                self.increment_y();
            }

            // Horizontal scroll copy
            (0..=239, 257) | (261, 257) if rendering_enabled => {
                self.copy_horizontal_scroll();
            }

            // Vertical scroll copy
            (261, 280..=304) if rendering_enabled => {
                self.copy_vertical_scroll();
            }

            _ => {}
        }
    }
}
```

---

## Test ROM Validation

### Scrolling Test ROMs

1. **scroll_test**
   - Tests basic scrolling functionality
   - Validates register updates

2. **scanline**
   - Tests scanline counter accuracy
   - Validates scroll position updates during rendering

3. **sprite_hit_tests**
   - Tests sprite 0 hit with scrolling
   - Validates split-screen effects

4. **vbl_nmi_timing**
   - Tests VBlank timing with scrolling
   - Validates register state preservation

### Validation Checklist

- [ ] $2005 first write sets coarse X and fine X
- [ ] $2005 second write sets coarse Y and fine Y
- [ ] $2006 writes update t register correctly
- [ ] Second $2006 write copies t to v
- [ ] Reading $2002 resets write latch
- [ ] Coarse X increments every 8 dots during rendering
- [ ] Fine Y increments at dot 256
- [ ] Horizontal scroll copied at dot 257
- [ ] Vertical scroll copied at dots 280-304 of scanline 261
- [ ] Nametable wrapping handled correctly

---

## References

- [NesDev Wiki - PPU Scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- [Loopy's Document](https://www.nesdev.org/loopyppu.txt)
- [PPU Registers](https://www.nesdev.org/wiki/PPU_registers)
- [PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)

---

**Back to:** [PPU Overview](PPU_OVERVIEW.md) | [PPU Timing](PPU_TIMING.md) | [PPU Rendering](PPU_RENDERING.md)
