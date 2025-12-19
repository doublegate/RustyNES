# PPU Scrolling Internals (Loopy's Implementation)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete implementation of Loopy's PPU scrolling document

---

## Table of Contents

- [Overview](#overview)
- [Internal Registers](#internal-registers)
- [Register Layout](#register-layout)
- [PPUSCROLL Writes](#ppuscroll-writes)
- [PPUADDR Writes](#ppuaddr-writes)
- [Rendering Updates](#rendering-updates)
- [Mid-Frame Updates](#mid-frame-updates)
- [Split Screen Scrolling](#split-screen-scrolling)
- [Implementation Guide](#implementation-guide)

---

## Overview

PPU scrolling is controlled by **two internal 15-bit registers** (v and t) and a **3-bit fine X register**. This implementation is based on **Loopy's PPU scrolling document**, the authoritative reference.

### Key Concepts

- **v**: Current VRAM address (what the PPU is currently reading/writing)
- **t**: Temporary VRAM address (holds target address before transfer to v)
- **x**: Fine X scroll (3-bit pixel offset within tile)
- **w**: Write toggle (determines first vs second write to $2005/$2006)

---

## Internal Registers

### v - Current VRAM Address (15 bits)

```
yyy NN YYYYY XXXXX
||| || ||||| +++++-- Coarse X scroll (tile column: 0-31)
||| || +++++-------- Coarse Y scroll (tile row: 0-31)
||| ++-------------- Nametable select (0-3)
+++----------------- Fine Y scroll (pixel row within tile: 0-7)
```

**Address Mapping:**

```
Bits 14-13: Nametable select
  00 = $2000 (top-left)
  01 = $2400 (top-right)
  10 = $2800 (bottom-left)
  11 = $2C00 (bottom-right)

Bits 12-10: Fine Y (0-7)
Bits 9-5:   Coarse Y (0-31, but 30-31 trigger nametable switch)
Bits 4-0:   Coarse X (0-31)
```

### t - Temporary VRAM Address (15 bits)

Same bit layout as `v`. Used to hold scroll/address data before copying to `v`.

### x - Fine X Scroll (3 bits)

```
7  bit  0
---- ----
---- -XXX
      +++-- Fine X scroll (0-7)
```

Pixel offset within the current tile (used to select pixel from shift registers).

### w - Write Toggle (1 bit)

```
0 = Next write to $2005/$2006 is first write
1 = Next write to $2005/$2006 is second write
```

Reset by reading $2002.

---

## Register Layout

### Relationship Between Scroll Position and v/t

```
Scroll Position (X, Y) mapped to v/t:

X scroll = (Nametable X bit << 8) | (Coarse X << 3) | Fine X
Y scroll = (Nametable Y bit << 8) | (Coarse Y << 3) | Fine Y

Example:
X = 123 pixels:
  Nametable X = 0
  Coarse X = 123 / 8 = 15
  Fine X = 123 % 8 = 3

Y = 89 pixels:
  Nametable Y = 0
  Coarse Y = 89 / 8 = 11
  Fine Y = 89 % 8 = 1
```

### Extracting Scroll Components

```rust
fn get_scroll_x(&self) -> u16 {
    ((self.v & 0x0400) >> 2) |  // Nametable X bit (bit 10)
    ((self.v & 0x001F) << 3) |  // Coarse X (bits 0-4)
    self.x as u16               // Fine X
}

fn get_scroll_y(&self) -> u16 {
    ((self.v & 0x0800) >> 3) |  // Nametable Y bit (bit 11)
    ((self.v & 0x03E0) >> 2) |  // Coarse Y (bits 5-9)
    ((self.v & 0x7000) >> 12)   // Fine Y (bits 12-14)
}
```

---

## PPUSCROLL Writes

### First Write ($2005 w=0): Horizontal Scroll

```
7  bit  0
---- ----
XXXX XXXX
|||| ||||
++++-++++-- Horizontal scroll position

Split into:
  Bits 7-3: Coarse X (5 bits)
  Bits 2-0: Fine X (3 bits)
```

**Register Updates:**

```rust
// First write: X scroll
t = (t & 0xFFE0) | (data >> 3);  // t: ........ ...XXXXX = data >> 3
x = data & 0x07;                  // x:       XXX = data & 0x07
w = 1;                            // Toggle write latch
```

**Example:**

```
Write $2005 = $7D (scroll X = 125)
  Coarse X = $7D >> 3 = 15 (0x0F)
  Fine X = $7D & 7 = 5

  t = (t & 0xFFE0) | 0x0F  // Set coarse X to 15
  x = 5                     // Set fine X to 5
```

### Second Write ($2005 w=1): Vertical Scroll

```
7  bit  0
---- ----
YYYY YYYY
|||| ||||
++++-++++-- Vertical scroll position

Split into:
  Bits 7-3: Coarse Y (5 bits)
  Bits 2-0: Fine Y (3 bits)
```

**Register Updates:**

```rust
// Second write: Y scroll
t = (t & 0x8FFF) | ((data & 0x07) << 12);  // t: .YYY.... ........ = data[2:0]
t = (t & 0xFC1F) | ((data & 0xF8) << 2);   // t: ........ YYYYY... = data[7:3]
w = 0;                                      // Toggle write latch
```

**Example:**

```
Write $2005 = $5E (scroll Y = 94)
  Coarse Y = $5E >> 3 = 11 (0x0B)
  Fine Y = $5E & 7 = 6

  t = (t & 0x8FFF) | (6 << 12)   // Set fine Y to 6
  t = (t & 0xFC1F) | (11 << 5)   // Set coarse Y to 11
```

---

## PPUADDR Writes

### First Write ($2006 w=0): High Byte

```
7  bit  0
---- ----
--AA AAAA
  || ||||
  ++-++++-- High 6 bits of address (bits 8-13)
```

**Register Updates:**

```rust
// First write: high byte
t = (t & 0x00FF) | ((data & 0x3F) << 8);  // t: ..AAAAAA ........ = data[5:0]
t = t & 0x7FFF;                            // t: .0...... ........ (clear bit 14)
w = 1;                                     // Toggle write latch
```

**Example:**

```
Write $2006 = $3F (set high byte)
  t = (t & 0x00FF) | 0x3F00
  t = t & 0x7FFF  // Ensure bit 14 is clear
```

### Second Write ($2006 w=1): Low Byte

```
7  bit  0
---- ----
AAAA AAAA
|||| ||||
++++-++++-- Low 8 bits of address (bits 0-7)
```

**Register Updates:**

```rust
// Second write: low byte
t = (t & 0xFF00) | data;  // t: ........ AAAAAAAA = data
v = t;                     // v = t (copy temp to current)
w = 0;                     // Toggle write latch
```

**Example:**

```
Write $2006 = $00 (set low byte, finalize address)
  t = (t & 0xFF00) | 0x00
  v = t  // Transfer to v
```

---

## Rendering Updates

During rendering, `v` is automatically updated by the PPU:

### Coarse X Increment (Every 8 Dots)

Called at dots 8, 16, 24, ..., 256 during visible scanlines:

```rust
fn increment_x(&mut self) {
    if (self.v & 0x001F) == 31 {
        // Coarse X = 31, wrap to 0 and switch horizontal nametable
        self.v &= !0x001F;   // Coarse X = 0
        self.v ^= 0x0400;    // Switch horizontal nametable
    } else {
        // Increment coarse X
        self.v += 1;
    }
}
```

**Behavior:**

```
v = $2000 (coarse X = 0) -> $2001 (coarse X = 1)
v = $201F (coarse X = 31) -> $2400 (coarse X = 0, switch to right nametable)
v = $241F (coarse X = 31) -> $2000 (coarse X = 0, switch to left nametable)
```

### Fine Y Increment (Dot 256)

Called at dot 256 of each visible scanline:

```rust
fn increment_y(&mut self) {
    if (self.v & 0x7000) != 0x7000 {
        // Increment fine Y if not at max (7)
        self.v += 0x1000;
    } else {
        // Fine Y = 7, wrap to 0 and increment coarse Y
        self.v &= !0x7000;                     // Fine Y = 0
        let mut y = (self.v & 0x03E0) >> 5;    // Get coarse Y

        if y == 29 {
            // Coarse Y = 29 (last row of nametable), wrap and switch vertical nametable
            y = 0;
            self.v ^= 0x0800;  // Switch vertical nametable
        } else if y == 31 {
            // Coarse Y = 31 (out of bounds), wrap without switching nametable
            y = 0;
        } else {
            // Increment coarse Y
            y += 1;
        }

        self.v = (self.v & !0x03E0) | (y << 5);
    }
}
```

**Behavior:**

```
Fine Y 0-6: Increment fine Y
Fine Y = 7, Coarse Y = 0-28: Wrap fine Y, increment coarse Y
Fine Y = 7, Coarse Y = 29: Wrap fine Y and coarse Y, switch vertical nametable
Fine Y = 7, Coarse Y = 31: Wrap fine Y and coarse Y (no nametable switch)
```

### Horizontal Copy (Dot 257)

At dot 257 of each visible scanline, copy horizontal bits from `t` to `v`:

```rust
fn copy_horizontal(&mut self) {
    // v: ....F.. ...EDCBA = t: ....F.. ...EDCBA
    self.v = (self.v & 0xFBE0) | (self.t & 0x041F);
}
```

**Copies:**

- Bit 10: Horizontal nametable
- Bits 0-4: Coarse X

**Effect:** Resets horizontal scroll position to start of scanline.

### Vertical Copy (Dots 280-304 of Pre-Render)

During pre-render scanline (261), dots 280-304, copy vertical bits from `t` to `v`:

```rust
fn copy_vertical(&mut self) {
    // v: IHGF.ED CBA..... = t: IHGF.ED CBA.....
    self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
}
```

**Copies:**

- Bit 11: Vertical nametable
- Bits 5-9: Coarse Y
- Bits 12-14: Fine Y

**Effect:** Resets vertical scroll position for next frame.

---

## Mid-Frame Updates

### Changing Scroll Mid-Frame

To create effects like status bars or parallax scrolling:

```rust
// Wait for specific scanline
while ppu.scanline != 8 { }

// Write new scroll position
bus.write(0x2005, new_x);  // First write: X
bus.write(0x2005, new_y);  // Second write: Y
```

**Timing Requirement:**

- Must write during HBlank or VBlank
- Recommended: Write at dot 257-320 (sprite fetch)
- Affects rendering starting next scanline

### Changing Nametable Only

To switch nametables without changing scroll:

```rust
// PPUCTRL bit 0-1 control nametable base
let ppuctrl = (ppuctrl & 0xFC) | nametable_select;
bus.write(0x2000, ppuctrl);
```

**Effect on t:**

```rust
// PPUCTRL write updates t nametable bits
t = (t & 0xF3FF) | ((value & 0x03) << 10);
```

---

## Split Screen Scrolling

### Status Bar (No Scroll)

```
Scanline 0-7:   v = t (normal scrolling)
Scanline 8:     Write $2006 twice to reset v to $2000
                v = $2000 (scroll = 0,0)
Scanline 9-239: No scroll (status bar)
```

**Implementation:**

```rust
// During scanline 8 HBlank
if ppu.scanline == 8 && ppu.dot == 257 {
    bus.write(0x2006, 0x20);  // Set high byte
    bus.write(0x2006, 0x00);  // Set low byte, v = $2000
}
```

### Parallax Layers

```
Scanline 0-99:   v from t (layer 1 scroll)
Scanline 100:    Update t with new scroll
                 copy_horizontal(), copy_vertical()
Scanline 101-239: v from new t (layer 2 scroll)
```

---

## Implementation Guide

### Complete PPU Scroll State

```rust
pub struct PpuScroll {
    v: u16,       // Current VRAM address
    t: u16,       // Temporary VRAM address
    x: u8,        // Fine X scroll (3 bits)
    w: bool,      // Write toggle
}

impl PpuScroll {
    pub fn new() -> Self {
        Self {
            v: 0,
            t: 0,
            x: 0,
            w: false,
        }
    }

    pub fn write_ppuctrl(&mut self, value: u8) {
        // t: ....BA.. ........ = d: ......BA
        self.t = (self.t & 0xF3FF) | (((value & 0x03) as u16) << 10);
    }

    pub fn write_ppuscroll(&mut self, value: u8) {
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

    pub fn write_ppuaddr(&mut self, value: u8) {
        if !self.w {
            // First write: high byte
            self.t = (self.t & 0x00FF) | (((value & 0x3F) as u16) << 8);
            self.t &= 0x7FFF;
        } else {
            // Second write: low byte
            self.t = (self.t & 0xFF00) | (value as u16);
            self.v = self.t;
        }
        self.w = !self.w;
    }

    pub fn read_ppustatus(&mut self) {
        self.w = false;  // Reset write toggle
    }

    pub fn increment_x(&mut self) {
        if (self.v & 0x001F) == 31 {
            self.v &= !0x001F;
            self.v ^= 0x0400;
        } else {
            self.v += 1;
        }
    }

    pub fn increment_y(&mut self) {
        if (self.v & 0x7000) != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5;

            if y == 29 {
                y = 0;
                self.v ^= 0x0800;
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }

            self.v = (self.v & !0x03E0) | (y << 5);
        }
    }

    pub fn copy_horizontal(&mut self) {
        self.v = (self.v & 0xFBE0) | (self.t & 0x041F);
    }

    pub fn copy_vertical(&mut self) {
        self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
    }
}
```

---

## Related Documentation

- [PPU_2C02_SPECIFICATION.md](PPU_2C02_SPECIFICATION.md) - Register specifications
- [PPU_TIMING_DIAGRAM.md](PPU_TIMING_DIAGRAM.md) - Dot-by-dot timing
- [PPU_RENDERING.md](PPU_RENDERING.md) - Rendering pipeline
- [PPU_OVERVIEW.md](PPU_OVERVIEW.md) - High-level PPU architecture

---

## References

- [Loopy's PPU Scrolling Document](https://wiki.nesdev.org/w/index.php/PPU_scrolling)
- [NESdev Wiki: PPU Scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- [NESdev Wiki: PPU Registers](https://www.nesdev.org/wiki/PPU_registers)
- [2C02 Technical Reference](http://nesdev.com/2C02%20technical%20reference.TXT)

---

**Document Status:** Complete implementation of Loopy's PPU scrolling specification.
