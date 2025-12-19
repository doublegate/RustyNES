# PPU Timing and Scanline Execution

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Clock Specifications](#clock-specifications)
- [Frame Structure](#frame-structure)
- [Scanline Breakdown](#scanline-breakdown)
- [Critical Timing Points](#critical-timing-points)
- [Dot-by-Dot Operations](#dot-by-dot-operations)
- [Odd Frame Skip](#odd-frame-skip)
- [Implementation Guide](#implementation-guide)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

The PPU operates on a **dot-level timing model** where each dot represents one PPU clock cycle. Understanding PPU timing is essential for:

- **Accurate VBlank Timing** - NMI trigger point
- **Sprite 0 Hit Detection** - Timing-based raster effects
- **Scrolling Updates** - Loopy's scrolling model
- **Mapper IRQ Timing** - MMC3 scanline counter
- **Mid-Frame Register Updates** - Split-screen effects

**Key Concept:** The PPU performs specific operations on specific dots of specific scanlines. Emulating this precisely is required for cycle-accurate emulation.

---

## Clock Specifications

### Master Clock Relationship

```
NTSC (2C02):
  Master Clock:   21.477272 MHz
  PPU Clock:      5.369318 MHz (master ÷ 4)
  CPU Clock:      1.789773 MHz (master ÷ 12)

  PPU:CPU Ratio:  3:1 (exactly)
  → 3 PPU dots per CPU cycle

PAL (2C07):
  Master Clock:   26.601712 MHz
  PPU Clock:      5.320342 MHz (master ÷ 5)
  CPU Clock:      1.662607 MHz (master ÷ 16)

  PPU:CPU Ratio:  16:5 = 3.2:1
  → 3.2 PPU dots per CPU cycle (requires fractional tracking)
```

**Implementation Note:** For NTSC, the 3:1 ratio means every CPU cycle advances the PPU by exactly 3 dots. For PAL, fractional accumulation is needed.

---

## Frame Structure

### NTSC Frame Timing

```
Scanlines 0-239:    Visible scanlines (rendering)
Scanline 240:       Post-render scanline (idle)
Scanlines 241-260:  VBlank period (20 scanlines)
Scanline 261:       Pre-render scanline (prepare next frame)

Total Scanlines:    262
Dots per Scanline:  341 (even frames)
                    340 (odd frames - skip cycle at scanline 0, dot 0)

Total Dots:         89,342 (even frames)
                    89,341 (odd frames)
```

### Frame Time Calculation

```
Even Frame:  89,342 dots ÷ 5.369318 MHz = 16.639 ms
Odd Frame:   89,341 dots ÷ 5.369318 MHz = 16.639 ms

Frame Rate:  60.0988 Hz (not exactly 60 Hz)
VBlank Time: 20 scanlines × 341 dots ÷ 5.369318 MHz = 1.27 ms
```

**CPU Cycles per Frame:**

```
Even Frame:  89,342 dots ÷ 3 = 29,780.67 CPU cycles
Odd Frame:   89,341 dots ÷ 3 = 29,780.33 CPU cycles

Average:     29,780.5 CPU cycles per frame
```

---

## Scanline Breakdown

### Scanline Types

#### 1. Visible Scanlines (0-239)

These scanlines render the 256×240 visible display:

```
Dots 0:          Idle
Dots 1-256:      Render pixels, fetch tile data
Dots 257-320:    Fetch sprite data for next scanline
Dots 321-336:    Fetch first two tiles for next scanline
Dots 337-340:    Unused nametable fetches
```

**Rendering Pipeline:**

- Background tile fetching (every 8 dots)
- Sprite evaluation (dots 1-256)
- Pixel output (dots 1-256)
- Scroll position updates (specific dots)

#### 2. Post-Render Scanline (240)

Idle scanline where PPU does nothing:

```
Dots 0-340:  Idle (no rendering or fetches)
```

**Purpose:** Provides a buffer between visible rendering and VBlank.

#### 3. VBlank Scanlines (241-260)

VBlank period where CPU can safely access VRAM:

```
Scanline 241, Dot 1:  VBlank flag set, NMI triggered
Scanlines 241-260:    VBlank period (safe for VRAM access)
```

**Duration:** 20 scanlines = 6,820 dots = ~1.27 ms

**CPU Cycles Available:**

```
6,820 dots ÷ 3 = 2,273 CPU cycles during VBlank
```

#### 4. Pre-Render Scanline (261)

Prepares for next frame by clearing flags and initializing state:

```
Dot 1:       Clear VBlank flag, Sprite 0 hit, Sprite overflow
Dots 1-256:  Same as visible scanlines (fetch tiles, but don't render)
Dots 257-320: Fetch sprite data
Dots 280-304: Copy vertical scroll position (if rendering enabled)
Dots 321-336: Fetch first two tiles
Dots 337-340: Unused fetches
```

---

## Critical Timing Points

### VBlank Flag Set (Scanline 241, Dot 1)

```
Scanline 241, Dot 1:
  - Set VBlank flag (bit 7 of $2002)
  - Trigger NMI (if enabled in $2000, bit 7)
```

**Race Condition:**
If the CPU reads `$2002` on the **exact cycle** the VBlank flag is set:

- VBlank flag is cleared (as expected)
- NMI is **suppressed** (unexpected side effect)

**Implementation:**

```rust
if self.scanline == 241 && self.dot == 1 {
    self.status |= 0x80; // Set VBlank

    // Check if CPU is reading $2002 this cycle
    if cpu_reading_status {
        self.nmi_suppressed = true;
    } else if (self.ctrl & 0x80) != 0 {
        self.nmi_triggered = true;
    }
}
```

### VBlank Flag Clear (Scanline 261, Dot 1)

```
Scanline 261, Dot 1:
  - Clear VBlank flag (bit 7 of $2002)
  - Clear Sprite 0 hit (bit 6 of $2002)
  - Clear Sprite overflow (bit 5 of $2002)
```

**Implementation:**

```rust
if self.scanline == 261 && self.dot == 1 {
    self.status &= 0x1F; // Clear bits 7, 6, 5
    self.nmi_triggered = false;
}
```

### Horizontal Scroll Copy (Dot 257)

```
Visible Scanlines, Dot 257:
  - Copy horizontal scroll position from t to v
  - v = (v & 0xFBE0) | (t & 0x041F)
```

**Purpose:** Reset horizontal scroll at the end of each scanline to prepare for the next scanline.

**Implementation:**

```rust
if self.dot == 257 && self.scanline < 240 && rendering_enabled {
    // Copy horizontal scroll: coarse X, nametable X
    self.vram_addr = (self.vram_addr & 0xFBE0) | (self.temp_addr & 0x041F);
}
```

### Vertical Scroll Copy (Dots 280-304, Scanline 261)

```
Pre-Render Scanline, Dots 280-304:
  - Copy vertical scroll position from t to v
  - v = (v & 0x841F) | (t & 0x7BE0)
```

**Purpose:** Initialize vertical scroll at the start of each frame.

**Implementation:**

```rust
if self.scanline == 261 && self.dot >= 280 && self.dot <= 304 && rendering_enabled {
    // Copy vertical scroll: coarse Y, nametable Y, fine Y
    self.vram_addr = (self.vram_addr & 0x841F) | (self.temp_addr & 0x7BE0);
}
```

### Increment Coarse X (Every 8 Dots, Dots 1-256 and 321-336)

```
During Rendering:
  - Increment coarse X position every 8 dots
  - Wrap around nametable boundaries
```

**Implementation:**

```rust
fn increment_coarse_x(&mut self) {
    if (self.vram_addr & 0x001F) == 31 {
        // Wrap around to next nametable
        self.vram_addr &= !0x001F;
        self.vram_addr ^= 0x0400; // Switch horizontal nametable
    } else {
        self.vram_addr += 1;
    }
}
```

### Increment Fine Y (Dot 256)

```
Visible Scanlines, Dot 256:
  - Increment fine Y (vertical scroll within tile)
  - If fine Y overflows, increment coarse Y
```

**Implementation:**

```rust
fn increment_y(&mut self) {
    if (self.vram_addr & 0x7000) != 0x7000 {
        // Increment fine Y
        self.vram_addr += 0x1000;
    } else {
        // Fine Y overflows, reset and increment coarse Y
        self.vram_addr &= !0x7000;
        let mut coarse_y = (self.vram_addr & 0x03E0) >> 5;

        if coarse_y == 29 {
            coarse_y = 0;
            self.vram_addr ^= 0x0800; // Switch vertical nametable
        } else if coarse_y == 31 {
            coarse_y = 0; // Out of bounds, wrap without switching nametable
        } else {
            coarse_y += 1;
        }

        self.vram_addr = (self.vram_addr & !0x03E0) | (coarse_y << 5);
    }
}
```

---

## Dot-by-Dot Operations

### Visible Scanlines (0-239)

| Dot Range | Operation | Description |
|-----------|-----------|-------------|
| **0** | Idle | No operation |
| **1-256** | Render + Fetch | Output pixel, fetch background tiles |
| **257** | Fetch | Fetch sprite Y position, copy horizontal scroll |
| **258-320** | Fetch Sprites | Load sprite data for next scanline |
| **321-336** | Fetch BG | Fetch first two tiles for next scanline |
| **337-340** | Dummy Fetch | Unused nametable fetches |

#### Fetch Cycle (Every 8 Dots)

Background tile fetching repeats every 8 dots:

```
Dot 1 (of 8): Fetch nametable byte (tile index)
Dot 3 (of 8): Fetch attribute table byte (palette)
Dot 5 (of 8): Fetch pattern table low byte (bitplane 0)
Dot 7 (of 8): Fetch pattern table high byte (bitplane 1)
Dot 0 (of 8): Increment coarse X
```

**Example Timeline (Dots 1-8):**

```
Dot 1: Fetch NT byte
Dot 2: -
Dot 3: Fetch AT byte
Dot 4: -
Dot 5: Fetch PT low
Dot 6: -
Dot 7: Fetch PT high
Dot 8: Inc coarse X, reload shift registers
```

### Pre-Render Scanline (261)

| Dot Range | Operation | Description |
|-----------|-----------|-------------|
| **1** | Clear Flags | Clear VBlank, Sprite 0, Sprite overflow |
| **1-256** | Fetch BG | Same as visible scanlines |
| **257** | Copy H Scroll | Copy horizontal scroll from t to v |
| **280-304** | Copy V Scroll | Copy vertical scroll from t to v |
| **321-336** | Fetch BG | Fetch first two tiles |
| **337-340** | Dummy Fetch | Unused fetches |

---

## Odd Frame Skip

### The Extra Cycle Skip

On **odd frames** when rendering is enabled, the PPU skips dot 0 of scanline 0:

```
Even frames:  Scanline 0 starts at dot 0 (341 dots total)
Odd frames:   Scanline 0 starts at dot 1 (340 dots total, skip dot 0)
```

**Purpose:** Aligns the PPU frame timing to avoid NTSC color artifacts.

**Condition:** Skip occurs ONLY if:

- Frame count is odd
- Background or sprite rendering is enabled ($2001 bits 3 or 4 set)

**Implementation:**

```rust
pub fn step(&mut self) {
    // Skip cycle 0 of scanline 0 on odd frames when rendering
    if self.scanline == 0 && self.dot == 0 {
        let rendering_enabled = (self.mask & 0x18) != 0;
        if (self.frame_count & 1) == 1 && rendering_enabled {
            self.dot = 1; // Skip to dot 1
        }
    }

    // ... rest of PPU step logic

    // Increment dot/scanline
    self.dot += 1;
    if self.dot > 340 {
        self.dot = 0;
        self.scanline += 1;
        if self.scanline > 261 {
            self.scanline = 0;
            self.frame_count += 1;
        }
    }
}
```

**Effect on Frame Timing:**

```
Even frame: 89,342 dots = 29,780.67 CPU cycles
Odd frame:  89,341 dots = 29,780.33 CPU cycles

Average:    29,780.5 CPU cycles per frame
```

---

## Implementation Guide

### Core Timing Variables

```rust
pub struct Ppu {
    scanline: u16,    // Current scanline (0-261)
    dot: u16,         // Current dot (0-340)
    frame_count: u64, // Frame counter (for odd/even detection)

    // Other PPU state...
}
```

### Master Step Function

```rust
pub fn step(&mut self, cartridge: &mut Cartridge) -> bool {
    let rendering_enabled = (self.mask & 0x18) != 0;

    // Odd frame skip
    if self.scanline == 0 && self.dot == 0 {
        if (self.frame_count & 1) == 1 && rendering_enabled {
            self.dot = 1;
        }
    }

    // Scanline/dot specific operations
    match (self.scanline, self.dot) {
        // VBlank start
        (241, 1) => {
            self.status |= 0x80;
            if (self.ctrl & 0x80) != 0 {
                self.nmi_triggered = true;
            }
        }

        // Pre-render scanline - clear flags
        (261, 1) => {
            self.status &= 0x1F;
        }

        // Visible scanlines - render
        (0..=239, 1..=256) if rendering_enabled => {
            self.render_pixel();
            self.fetch_tile_data(cartridge);
        }

        // Horizontal scroll copy
        (0..=239, 257) if rendering_enabled => {
            self.copy_horizontal_scroll();
        }

        // Vertical scroll copy
        (261, 280..=304) if rendering_enabled => {
            self.copy_vertical_scroll();
        }

        // Increment Y at end of scanline
        (0..=239, 256) if rendering_enabled => {
            self.increment_y();
        }

        _ => {}
    }

    // Increment dot and scanline
    self.dot += 1;
    if self.dot > 340 {
        self.dot = 0;
        self.scanline += 1;

        if self.scanline > 261 {
            self.scanline = 0;
            self.frame_count += 1;
            return true; // Frame complete
        }
    }

    false
}
```

### Timing-Sensitive Operations

#### Sprite Evaluation

```rust
// Dots 65-256 of visible scanlines
if self.scanline < 240 && self.dot >= 65 && self.dot <= 256 {
    self.evaluate_sprites();
}
```

#### Background Tile Fetches

```rust
// Every 8 dots during rendering
if rendering_enabled && (self.dot % 8) == 0 {
    self.increment_coarse_x();
}

match self.dot % 8 {
    1 => self.fetch_nametable_byte(),
    3 => self.fetch_attribute_byte(),
    5 => self.fetch_pattern_low(),
    7 => self.fetch_pattern_high(),
    0 => self.reload_shift_registers(),
    _ => {}
}
```

---

## Test ROM Validation

### Timing Test ROMs

1. **ppu_vbl_nmi**
   - Tests VBlank flag timing
   - Validates NMI trigger point
   - Checks VBlank flag clear timing

2. **ppu_sprite_hit**
   - Tests sprite 0 hit timing
   - Validates hit detection edge cases

3. **scanline**
   - Tests scanline counter accuracy
   - Validates PPU cycle counts

4. **oam_read**
   - Tests OAM access during rendering
   - Validates sprite evaluation timing

5. **odd_frame_skip**
   - Tests the cycle skip on odd frames
   - Validates rendering enable check

### Validation Checklist

- [ ] VBlank flag set at scanline 241, dot 1
- [ ] VBlank flag cleared at scanline 261, dot 1
- [ ] NMI triggered when VBlank flag set and PPUCTRL.7 = 1
- [ ] Reading $2002 clears VBlank flag
- [ ] Reading $2002 on VBlank set cycle suppresses NMI
- [ ] Horizontal scroll copied at dot 257
- [ ] Vertical scroll copied at dots 280-304 of scanline 261
- [ ] Odd frame skips dot 0 of scanline 0 when rendering
- [ ] Even frames run 341 dots per scanline
- [ ] Total frame time matches hardware (29,780.5 CPU cycles average)

---

## References

- [NesDev Wiki - PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [PPU Frame Timing](https://www.nesdev.org/wiki/PPU_frame_timing)
- [NTSC Video](https://www.nesdev.org/wiki/NTSC_video)
- [PPU Scrolling](https://www.nesdev.org/wiki/PPU_scrolling)

---

**Next:** [PPU Rendering](PPU_RENDERING.md) | [PPU Scrolling](PPU_SCROLLING.md) | [Back to PPU Overview](PPU_OVERVIEW.md)
