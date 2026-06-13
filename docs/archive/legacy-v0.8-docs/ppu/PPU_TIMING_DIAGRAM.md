# PPU Timing Diagram and Scanline Reference

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Complete dot-by-dot timing for all 262 scanlines × 341 dots

---

## Table of Contents

- [Overview](#overview)
- [Frame Structure](#frame-structure)
- [Scanline Types](#scanline-types)
- [Dot-by-Dot Timeline](#dot-by-dot-timeline)
- [Memory Access Patterns](#memory-access-patterns)
- [Register Updates](#register-updates)
- [Visible Scanline Detail](#visible-scanline-detail)
- [Pre-Render Scanline](#pre-render-scanline)
- [VBlank Scanlines](#vblank-scanlines)
- [Odd/Even Frame Behavior](#oddeven-frame-behavior)

---

## Overview

The NES PPU renders at **5.369318 MHz** (NTSC), generating one pixel (dot) per clock cycle. Each frame consists of:

- **262 scanlines** (NTSC) or **312 scanlines** (PAL)
- **341 dots per scanline**
- **Total: 89,342 dots/frame** (NTSC) or **106,392 dots/frame** (PAL)

**Frame Rate:**

- NTSC: 5369318 ÷ 341 ÷ 262 = **60.0988 Hz**
- PAL: 5320342 ÷ 341 ÷ 312 = **50.0070 Hz**

### CPU/PPU Relationship

```
1 CPU cycle = 3 PPU dots (NTSC)
1 CPU cycle = 3.2 PPU dots (PAL)

341 dots per scanline = 113.667 CPU cycles (NTSC)
```

---

## Frame Structure

### NTSC Frame Layout

```
Scanline   Dot Range    Type           Description
--------   ---------    ----           -----------
0-239      0-340        Visible        Render background and sprites
240        0-340        Post-render    Idle scanline
241-260    0-340        VBlank         NMI triggered, safe VRAM access
261        0-340        Pre-render     Prepare for next frame
```

### Visual Diagram

```
     0                                                340
    ╔════════════════════════════════════════════════╗
  0 ║░░░░░░░░░░░░░ Visible Scanlines ░░░░░░░░░░░░░░║
  : ║░░░░░░░░░░░░░  (0-239)          ░░░░░░░░░░░░░░║
239 ║░░░░░░░░░░░░░                    ░░░░░░░░░░░░░░║
    ╠════════════════════════════════════════════════╣
240 ║▒▒▒▒▒▒▒▒▒▒▒▒▒ Post-Render (240) ▒▒▒▒▒▒▒▒▒▒▒▒▒║
    ╠════════════════════════════════════════════════╣
241 ║▓▓▓▓▓▓▓▓▓▓▓▓▓ VBlank (241-260)  ▓▓▓▓▓▓▓▓▓▓▓▓▓║
  : ║▓▓▓▓▓▓▓▓▓▓▓▓▓  NMI at dot 1     ▓▓▓▓▓▓▓▓▓▓▓▓▓║
260 ║▓▓▓▓▓▓▓▓▓▓▓▓▓                    ▓▓▓▓▓▓▓▓▓▓▓▓▓║
    ╠════════════════════════════════════════════════╣
261 ║▓▓▓▓▓▓▓▓▓▓▓▓▓ Pre-Render (261)  ▓▓▓▓▓▓▓▓▓▓▓▓▓║
    ╚════════════════════════════════════════════════╝
```

---

## Scanline Types

### Visible Scanlines (0-239)

**Purpose:** Render 256×240 visible picture

**Activity:**

- Dots 0-256: Fetch tiles, render pixels
- Dots 257-320: Fetch sprite data for next scanline
- Dots 321-336: Fetch first 2 tiles of next scanline
- Dots 337-340: Dummy nametable fetches

### Post-Render Scanline (240)

**Purpose:** Idle scanline, no rendering

**Activity:**

- PPU is idle
- No memory access (safe to access VRAM)
- Lasts full 341 dots

### VBlank Scanlines (241-260)

**Purpose:** Allow CPU to update VRAM/OAM

**Activity:**

- Scanline 241, dot 1: Set VBlank flag, trigger NMI (if enabled)
- No rendering or memory access
- Safe to write VRAM, OAM, palettes, scroll registers

### Pre-Render Scanline (261)

**Purpose:** Prepare for next frame

**Activity:**

- Dot 1: Clear VBlank flag, sprite 0 hit, sprite overflow
- Dots 280-304: Copy horizontal and vertical bits from t to v
- Dots 321-336: Fetch first 2 tiles of scanline 0
- Dot 339-340 (odd frames): Skip dot 340

---

## Dot-by-Dot Timeline

### Visible Scanline (0-239) Detailed Breakdown

#### Dots 0: Idle

```
Dot 0: No operation (idle cycle)
```

#### Dots 1-256: Tile Fetching and Rendering

Every 8 dots, the PPU fetches one complete tile (4 memory accesses):

```
Dot Pattern (repeats every 8 dots):
  Dot 1: Fetch nametable byte
  Dot 2: (Read finishes)
  Dot 3: Fetch attribute table byte
  Dot 4: (Read finishes)
  Dot 5: Fetch pattern table tile low
  Dot 6: (Read finishes)
  Dot 7: Fetch pattern table tile high
  Dot 8: (Read finishes), reload shift registers

Example for first tile:
  Dot 1-2:   NT fetch from $2000 + (v & 0x0FFF)
  Dot 3-4:   AT fetch from $23C0 + ((v >> 4) & 0x38) + ((v >> 2) & 0x07)
  Dot 5-6:   PT low from $0000 + (tile_id × 16) + fine_y
  Dot 7-8:   PT high from $0000 + (tile_id × 16) + fine_y + 8
```

**Shift Registers:**

- Shift left by 1 every dot (dots 2-257)
- Reload every 8 dots with new tile data

**Pixel Output:**

- Dots 1-256: Output pixel from shift registers
- Fine X scroll selects pixel within current tile

#### Dots 257-320: Sprite Fetching

Fetch sprite data for the next scanline (8 sprites, 8 bytes each):

```
For each of 8 sprites (0-7):
  Dot N+0: Garbage nametable fetch
  Dot N+1: Garbage nametable fetch
  Dot N+2: Fetch sprite tile low byte
  Dot N+3: Fetch sprite tile low byte
  Dot N+4: Fetch sprite tile high byte
  Dot N+5: Fetch sprite tile high byte
  Dot N+6: Fetch sprite tile low byte (actual)
  Dot N+7: Fetch sprite tile high byte (actual)

If fewer than 8 sprites on next scanline:
  Remaining slots: Fetch tile $FF (8 dummy fetches each)
```

**At Dot 257:**

- Copy horizontal bits from t to v: `v = (v & 0xFBE0) | (t & 0x041F)`
- Resets horizontal scroll position for next scanline

#### Dots 321-336: Next Scanline Prefetch

```
Dot 321-328: Fetch first tile of next scanline
  321-322: NT fetch
  323-324: AT fetch
  325-326: PT low
  327-328: PT high

Dot 329-336: Fetch second tile of next scanline
  329-330: NT fetch
  331-332: AT fetch
  333-334: PT low
  335-336: PT high
```

#### Dots 337-340: Dummy Nametable Fetches

```
Dot 337-338: NT fetch (purpose unknown)
Dot 339-340: NT fetch (purpose unknown)

MMC5 mapper uses these fetches for scanline counting.
```

---

## Memory Access Patterns

### Background Tile Fetch

**Nametable Address:**

```
NT addr = $2000 | (v & 0x0FFF)
Where v = yyy NN YYYYY XXXXX
```

**Attribute Table Address:**

```
AT addr = $23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)
          $23C0 + nametable_select + coarse_y/4 + coarse_x/4
```

**Pattern Table Address:**

```
PT addr = (PPUCTRL.B × $1000) + (tile_id × 16) + fine_y
PT low  = PT addr + 0
PT high = PT addr + 8
```

### Sprite Fetch

**Sprite Tile Address (8×8 mode):**

```
PT addr = (PPUCTRL.S × $1000) + (sprite_tile_id × 16) + sprite_y_offset
```

**Sprite Tile Address (8×16 mode):**

```
PT addr = ((sprite_tile_id & 0x01) × $1000) + ((sprite_tile_id & 0xFE) × 16) + sprite_y_offset
```

---

## Register Updates

### Horizontal Increment (Dots 8, 16, 24, ..., 256)

After every 8th dot during visible rendering:

```rust
fn increment_x(&mut self) {
    if (self.v & 0x001F) == 31 {
        self.v &= !0x001F;        // Coarse X = 0
        self.v ^= 0x0400;         // Switch horizontal nametable
    } else {
        self.v += 1;              // Increment coarse X
    }
}
```

### Vertical Increment (Dot 256)

At dot 256 of each visible scanline:

```rust
fn increment_y(&mut self) {
    if (self.v & 0x7000) != 0x7000 {
        self.v += 0x1000;         // Increment fine Y
    } else {
        self.v &= !0x7000;        // Fine Y = 0
        let mut y = (self.v & 0x03E0) >> 5;  // Coarse Y

        if y == 29 {
            y = 0;
            self.v ^= 0x0800;     // Switch vertical nametable
        } else if y == 31 {
            y = 0;                // Wrap without switching nametable
        } else {
            y += 1;
        }

        self.v = (self.v & !0x03E0) | (y << 5);
    }
}
```

### Horizontal Copy (Dot 257)

```rust
fn copy_horizontal(&mut self) {
    // v: ....F.. ...EDCBA = t: ....F.. ...EDCBA
    self.v = (self.v & 0xFBE0) | (self.t & 0x041F);
}
```

### Vertical Copy (Dots 280-304, Pre-Render Scanline)

```rust
fn copy_vertical(&mut self) {
    // v: IHGF.ED CBA..... = t: IHGF.ED CBA.....
    self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
}
```

---

## Visible Scanline Detail

### Example Timeline for Scanline 0

```
Dot   Action                           Memory Address       v Updates
----  ------                           --------------       ---------
0     Idle
1     Fetch NT byte (tile 0)          $2000 + (v & 0x0FFF)
2     (Read completes)
3     Fetch AT byte (tile 0)          $23C0 + ...
4     (Read completes)
5     Fetch PT low (tile 0)           $0000 + tile*16 + fine_y
6     (Read completes)
7     Fetch PT high (tile 0)          $0000 + tile*16 + fine_y + 8
8     (Read completes), reload shift  -                     inc_x()
9     Fetch NT byte (tile 1)          $2001 + (v & 0x0FFF)
...
256   (Last pixel rendered)                                 inc_y()
257   Fetch NT (sprite eval)          -                     copy_h()
...
320   (Sprite fetches done)
321   Fetch NT byte (next scanline)
...
340   Dummy NT fetch
```

---

## Pre-Render Scanline

Scanline 261 prepares for the next frame:

```
Dot   Action
----  ------
0     Idle
1     Clear VBlank flag, sprite 0 hit, sprite overflow
2-256 Same as visible scanline (fetch background tiles)
257   Copy horizontal bits from t to v
258-279 Sprite fetches (garbage data, not used)
280   Begin vertical copy
280-304 Copy vertical bits from t to v (every dot)
305-320 Sprite fetches continue (garbage)
321-336 Fetch first 2 tiles of scanline 0
337-338 Dummy NT fetch
339   Dummy NT fetch (skipped on odd frames if rendering enabled)
340   Dummy NT fetch (or skip to dot 0 of scanline 0 on odd frames)
```

### Odd Frame Skip

On odd frames with rendering enabled:

```
Even frame: Dots 0-340 (341 dots total)
Odd frame:  Dots 0-339 (340 dots total, skip dot 340)

This makes odd frames 1 PPU dot shorter (~186 ns)
```

**Implementation:**

```rust
if self.odd_frame && rendering_enabled() && self.scanline == 261 && self.dot == 339 {
    self.dot = 0;
    self.scanline = 0;
    self.odd_frame = false;
} else {
    // Normal progression
}
```

---

## VBlank Scanlines

Scanlines 241-260:

### Scanline 241, Dot 1: VBlank Start

```
Dot 0: Normal operation
Dot 1: Set VBlank flag in PPUSTATUS ($2002 bit 7)
       If PPUCTRL bit 7 is set: Trigger NMI on CPU
Dot 2-340: Continue VBlank
```

**Race Condition:** Reading $2002 on the same PPU dot that VBlank is set may prevent NMI.

### Scanlines 242-260

No special operations, PPU is idle. Safe for CPU to:

- Write to $2000-$2007 (VRAM, OAM, palettes)
- Update scroll registers
- Transfer OAM via $4014 DMA

---

## Odd/Even Frame Behavior

The PPU alternates between odd and even frames:

### Even Frame

```
Scanline 261, Dot 340 -> Scanline 0, Dot 0
Total frame: 341 × 262 = 89,342 dots
```

### Odd Frame (Rendering Enabled)

```
Scanline 261, Dot 339 -> Scanline 0, Dot 0 (skip dot 340)
Total frame: 340 + (341 × 261) = 89,341 dots
```

**Purpose:** Compensates for NTSC video timing, produces crisper image.

**Condition:** Only occurs if PPUMASK enables background OR sprites.

```rust
fn rendering_enabled(&self) -> bool {
    (self.ppumask & 0x18) != 0
}
```

---

## Implementation Example

### Minimal PPU Timing Loop

```rust
pub struct Ppu {
    scanline: u16,
    dot: u16,
    odd_frame: bool,
}

impl Ppu {
    pub fn step(&mut self) {
        // Visible scanlines (0-239)
        if self.scanline < 240 {
            match self.dot {
                0 => { /* Idle */ }
                1..=256 => self.render_pixel(),
                257 => self.copy_horizontal(),
                258..=320 => self.fetch_sprites(),
                321..=336 => self.fetch_tiles(),
                337..=340 => self.dummy_fetch(),
                _ => {}
            }

            if (self.dot >= 1 && self.dot <= 256 && self.dot % 8 == 0) {
                self.increment_x();
            }
            if self.dot == 256 {
                self.increment_y();
            }
        }
        // Post-render (240)
        else if self.scanline == 240 {
            // Idle
        }
        // VBlank (241-260)
        else if self.scanline == 241 {
            if self.dot == 1 {
                self.set_vblank();
            }
        }
        // Pre-render (261)
        else if self.scanline == 261 {
            if self.dot == 1 {
                self.clear_vblank();
            }
            if self.dot >= 280 && self.dot <= 304 {
                self.copy_vertical();
            }

            // Odd frame skip
            if self.odd_frame && self.rendering_enabled() && self.dot == 339 {
                self.dot = 340; // Will wrap to 0 below
            }
        }

        // Advance timing
        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
            }
        }
    }
}
```

---

## Related Documentation

- [PPU_2C02_SPECIFICATION.md](PPU_2C02_SPECIFICATION.md) - Register behavior
- [PPU_RENDERING.md](PPU_RENDERING.md) - Rendering pipeline details
- [PPU_SCROLLING_INTERNALS.md](PPU_SCROLLING_INTERNALS.md) - Scroll register updates
- [PPU_TIMING.md](PPU_TIMING.md) - High-level timing overview

---

## References

- [NESdev Wiki: PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [NESdev Wiki: PPU Frame Timing](https://www.nesdev.org/wiki/PPU_frame_timing)
- [NESdev Wiki: PPU Scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- Loopy's PPU Document
- Visual 2C02 Simulator

---

**Document Status:** Complete dot-by-dot timing diagram for all 262 scanlines.
