# PPU Rendering Pipeline

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Overview](#overview)
- [Rendering Pipeline](#rendering-pipeline)
- [Background Rendering](#background-rendering)
- [Sprite Rendering](#sprite-rendering)
- [Priority and Transparency](#priority-and-transparency)
- [Sprite 0 Hit](#sprite-0-hit)
- [Sprite Overflow](#sprite-overflow)
- [Implementation Guide](#implementation-guide)
- [Test ROM Validation](#test-rom-validation)

---

## Overview

The PPU renders a **256×240 pixel display** using a two-layer system:

1. **Background Layer** - Tile-based scrollable playfield
2. **Sprite Layer** - Up to 64 movable objects (8 per scanline)

Both layers are rendered **simultaneously** during each scanline, with pixel priority determined by sprite attributes and transparency.

**Key Rendering Characteristics:**

- Dot-level rendering (one pixel per PPU cycle)
- Parallel background and sprite processing
- 8 sprites per scanline limit (hardware limitation)
- Sprite 0 hit detection for raster effects
- Attribute-based priority and transparency

---

## Rendering Pipeline

### Per-Scanline Overview

```
Dots 1-256:  Render visible pixels
             ├─ Fetch background tile data
             ├─ Evaluate sprites for current scanline
             ├─ Mix background and sprite pixels
             └─ Output final pixel to framebuffer

Dots 257-320: Fetch sprite data for next scanline

Dots 321-336: Pre-fetch first two background tiles for next scanline
```

### Pixel Output (Dot-Level)

For each visible dot (1-256), the PPU outputs a pixel by:

1. **Background Pixel Selection**
   - Shift registers produce 4-bit color index
   - Palette lookup (background palette 0-3)

2. **Sprite Pixel Selection**
   - Check up to 8 sprites for current pixel
   - Select first opaque sprite pixel
   - Palette lookup (sprite palette 0-3)

3. **Priority Multiplexing**
   - Determine which pixel to display (BG or sprite)
   - Based on sprite priority bit and transparency

4. **Palette Lookup**
   - Convert 4-bit color index to 6-bit NES color
   - Output to framebuffer

**Pixel Multiplexer Logic:**

```
if sprite_pixel.transparent && bg_pixel.transparent:
    output = universal_background_color

elif sprite_pixel.transparent:
    output = bg_pixel.color

elif bg_pixel.transparent || sprite_pixel.priority_front:
    output = sprite_pixel.color

else:
    output = bg_pixel.color
```

---

## Background Rendering

### Tile Fetching Pipeline

The PPU fetches tile data in a **repeating 8-dot cycle**:

```
Cycle 1: Fetch Nametable Byte (tile index)
Cycle 2: (unused)
Cycle 3: Fetch Attribute Table Byte (palette selection)
Cycle 4: (unused)
Cycle 5: Fetch Pattern Table Low Byte (bitplane 0)
Cycle 6: (unused)
Cycle 7: Fetch Pattern Table High Byte (bitplane 1)
Cycle 8: Increment coarse X, reload shift registers
```

**Memory Access Pattern:**

```
Nametable Address:     $2000 | (v & 0x0FFF)
Attribute Address:     $23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)
Pattern Table Address: (PPUCTRL.B × $1000) + (tile_index × 16) + fine_y
```

### Shift Registers

The PPU uses **shift registers** to serialize tile data for pixel-by-pixel output:

```rust
struct BgShiftRegisters {
    pattern_lo: u16,  // Low bitplane (16 bits, 2 tiles worth)
    pattern_hi: u16,  // High bitplane
    attr_lo: u8,      // Attribute low bit (8 bits, 1 tile)
    attr_hi: u8,      // Attribute high bit
}
```

**Operation:**

- **Load:** Every 8 dots, fetch next tile data and load into upper 8 bits
- **Shift:** Every dot, shift all registers left by 1 bit
- **Output:** Use fine X scroll to select which bit to output

**Shift and Reload:**

```rust
fn shift_registers(&mut self) {
    self.bg_shift_lo <<= 1;
    self.bg_shift_hi <<= 1;
    self.attr_shift_lo <<= 1;
    self.attr_shift_hi <<= 1;
}

fn reload_registers(&mut self) {
    // Load next tile into upper 8 bits
    self.bg_shift_lo = (self.bg_shift_lo & 0x00FF) | (self.pattern_lo_latch as u16) << 8;
    self.bg_shift_hi = (self.bg_shift_hi & 0x00FF) | (self.pattern_hi_latch as u16) << 8;

    // Replicate attribute bits for 8 pixels
    self.attr_shift_lo = (self.attr_shift_lo & 0x00) | if self.attr_latch & 0x01 != 0 { 0xFF } else { 0x00 };
    self.attr_shift_hi = (self.attr_shift_hi & 0x00) | if self.attr_latch & 0x02 != 0 { 0xFF } else { 0x00 };
}
```

### Background Pixel Extraction

```rust
fn get_background_pixel(&self) -> (u8, u8) {
    let bit_select = 0x8000 >> self.fine_x;

    let bit_0 = if (self.bg_shift_lo & bit_select) != 0 { 1 } else { 0 };
    let bit_1 = if (self.bg_shift_hi & bit_select) != 0 { 1 } else { 0 };

    let color_index = (bit_1 << 1) | bit_0;

    let attr_bit_0 = if (self.attr_shift_lo & (0x80 >> self.fine_x)) != 0 { 1 } else { 0 };
    let attr_bit_1 = if (self.attr_shift_hi & (0x80 >> self.fine_x)) != 0 { 1 } else { 0 };

    let palette = (attr_bit_1 << 1) | attr_bit_0;

    (palette, color_index)
}
```

### Attribute Table Extraction

The attribute table stores palette selection for **2×2 tile groups** (16×16 pixels):

```rust
fn get_attribute_bits(&self, tile_x: u8, tile_y: u8, attr_byte: u8) -> u8 {
    let quadrant_x = (tile_x & 0x02) >> 1;
    let quadrant_y = (tile_y & 0x02) >> 0;
    let shift = (quadrant_y * 4) + (quadrant_x * 2);

    (attr_byte >> shift) & 0x03
}
```

**Attribute Byte Layout:**

```
7654 3210
|||| ||||
|||| ||++- Palette for top-left 2×2 tiles
|||| ++--- Palette for top-right 2×2 tiles
||++------ Palette for bottom-left 2×2 tiles
++-------- Palette for bottom-right 2×2 tiles
```

---

## Sprite Rendering

### OAM (Object Attribute Memory)

OAM stores 64 sprite definitions, each **4 bytes**:

```
Byte 0: Y position (top edge, in pixels)
Byte 1: Tile index (pattern table entry)
Byte 2: Attributes
Byte 3: X position (left edge, in pixels)
```

**Attribute Byte (Byte 2):**

```
7654 3210
|||| ||||
|||| ||++- Palette (sprite palette 0-3)
|||| |+--- (unused on NES)
|||| +---- (unused on NES)
|||+------ Priority (0: front of BG, 1: behind BG)
||+------- Flip horizontally
|+-------- Flip vertically
+--------- (unused on NES)
```

### Sprite Evaluation

The PPU evaluates which sprites are on the current scanline during **dots 65-256**:

```
Dots 1-64:   Clear secondary OAM (8 sprite slots)
Dots 65-256: Scan primary OAM (64 sprites)
             ├─ Check if sprite Y overlaps current scanline
             ├─ Copy to secondary OAM if within range
             ├─ Stop after 8 sprites found
             └─ Set overflow flag if more than 8 sprites
```

**Sprite Y Range Check:**

```rust
fn sprite_in_range(&self, sprite_y: u8, scanline: u16, sprite_height: u8) -> bool {
    let sprite_top = sprite_y as u16;
    let sprite_bottom = sprite_top + (sprite_height as u16);

    scanline >= sprite_top && scanline < sprite_bottom
}
```

### Sprite Fetching (Dots 257-320)

After evaluation, the PPU fetches tile data for the 8 sprites on the next scanline:

```
For each sprite (8 total):
  Dot 257 + (n×8) + 0: Fetch sprite Y (dummy read)
  Dot 257 + (n×8) + 1: Fetch sprite tile index (dummy read)
  Dot 257 + (n×8) + 2: Fetch sprite attributes (dummy read)
  Dot 257 + (n×8) + 3: Fetch sprite X (dummy read)
  Dot 257 + (n×8) + 4: Fetch pattern table low byte
  Dot 257 + (n×8) + 5: (garbage fetch)
  Dot 257 + (n×8) + 6: Fetch pattern table high byte
  Dot 257 + (n×8) + 7: (garbage fetch)
```

### Sprite Rendering (8×8 Mode)

```rust
fn render_sprite_pixel(&self, x: u8) -> Option<SpritePixel> {
    for (i, sprite) in self.secondary_oam.iter().enumerate() {
        if x < sprite.x || x >= sprite.x + 8 {
            continue; // Sprite not at this X position
        }

        let sprite_x_offset = x - sprite.x;
        let pixel_x = if sprite.flip_h {
            7 - sprite_x_offset
        } else {
            sprite_x_offset
        };

        let bit_0 = (sprite.pattern_lo >> (7 - pixel_x)) & 0x01;
        let bit_1 = (sprite.pattern_hi >> (7 - pixel_x)) & 0x01;
        let color_index = (bit_1 << 1) | bit_0;

        if color_index == 0 {
            continue; // Transparent pixel
        }

        return Some(SpritePixel {
            color: self.palette[0x10 + (sprite.palette * 4) + color_index],
            priority: sprite.priority,
            sprite_zero: i == 0,
        });
    }

    None // No sprite pixel at this position
}
```

### Sprite Rendering (8×16 Mode)

When **PPUCTRL.5 = 1**, sprites are 8×16 pixels:

```
Tile Index:
  Bit 0: Pattern table (0: $0000, 1: $1000)
  Bits 1-7: Tile pair index

Top half:    Tile index & 0xFE
Bottom half: Tile index | 0x01
```

**Y Offset Calculation:**

```rust
fn get_8x16_tile_index(&self, tile_byte: u8, row: u8) -> (u16, u8) {
    let pattern_table = if (tile_byte & 0x01) != 0 { 0x1000 } else { 0x0000 };

    let tile_index = if row < 8 {
        (tile_byte & 0xFE) as u16
    } else {
        (tile_byte | 0x01) as u16
    };

    let tile_row = row % 8;

    (pattern_table + (tile_index * 16), tile_row)
}
```

---

## Priority and Transparency

### Pixel Multiplexer

The final pixel color is determined by a priority multiplexer:

```rust
fn multiplex_pixel(&self, bg: BgPixel, sprite: Option<SpritePixel>) -> u8 {
    match (bg.color_index, sprite) {
        // Both transparent → universal background color
        (0, None) | (0, Some(sp)) if sp.color_index == 0 => {
            self.palette[0x00] // Universal BG color
        }

        // Sprite transparent → show background
        (_, None) | (_, Some(sp)) if sp.color_index == 0 => {
            self.palette[0x00 + (bg.palette * 4) + bg.color_index]
        }

        // Background transparent → show sprite
        (0, Some(sp)) => {
            self.palette[0x10 + (sp.palette * 4) + sp.color_index]
        }

        // Both opaque → check priority
        (_, Some(sp)) => {
            if sp.priority == 0 {
                // Sprite in front
                self.palette[0x10 + (sp.palette * 4) + sp.color_index]
            } else {
                // Sprite behind background
                self.palette[0x00 + (bg.palette * 4) + bg.color_index]
            }
        }
    }
}
```

**Priority Truth Table:**

| BG Opaque | Sprite Opaque | Sprite Priority | Output |
|-----------|---------------|-----------------|--------|
| No | No | - | Universal BG |
| Yes | No | - | Background |
| No | Yes | - | Sprite |
| Yes | Yes | 0 (front) | Sprite |
| Yes | Yes | 1 (back) | Background |

---

## Sprite 0 Hit

### Definition

**Sprite 0 hit** occurs when:

1. Sprite 0 (first sprite in OAM) has an **opaque pixel** (color index ≠ 0)
2. Background has an **opaque pixel** at the same position
3. Both rendering layers are **enabled** ($2001 bits 3 and 4)
4. Pixel is **not in leftmost 8 pixels** (unless both show_left flags are set)

**Purpose:** Timing raster effects (split-screen scrolling, status bars)

### Detection Logic

```rust
fn check_sprite_0_hit(&mut self, x: u8, bg_pixel: BgPixel, sprite_pixel: Option<SpritePixel>) {
    if let Some(sp) = sprite_pixel {
        if sp.sprite_zero && bg_pixel.color_index != 0 && sp.color_index != 0 {
            // Check leftmost 8 pixels restriction
            if x < 8 {
                let show_left_bg = (self.mask & 0x02) != 0;
                let show_left_sp = (self.mask & 0x04) != 0;

                if !show_left_bg || !show_left_sp {
                    return; // Don't set hit in leftmost 8 pixels
                }
            }

            self.status |= 0x40; // Set sprite 0 hit flag
        }
    }
}
```

### Timing

- **Set:** When opaque pixels overlap during rendering (dots 1-256, scanlines 0-239)
- **Cleared:** At dot 1 of pre-render scanline (261)

**Important Edge Cases:**

- Hit does NOT occur if either pixel is transparent (color index 0)
- Hit does NOT occur in leftmost 8 pixels unless both show_left flags are set
- Hit occurs even if sprite priority places it behind background

**Race Condition:**
Games often poll `$2002` in a loop waiting for sprite 0 hit:

```assembly
@wait_sprite0:
    BIT $2002
    BVC @wait_sprite0  ; Loop until bit 6 set
```

---

## Sprite Overflow

### Definition

**Sprite overflow** flag is set when more than **8 sprites** are on the same scanline.

**Hardware Bug:** The actual sprite overflow detection has a hardware bug that causes false positives and false negatives. Most emulators implement the buggy behavior for accuracy.

### Overflow Detection (Hardware Accurate)

```rust
fn evaluate_sprites_buggy(&mut self) {
    let mut n = 0; // Primary OAM index
    let mut m = 0; // Byte within sprite (0-3)
    let mut secondary_count = 0;

    while n < 64 {
        let sprite_y = self.oam[n * 4];

        if self.sprite_in_range(sprite_y, self.scanline, self.sprite_height) {
            if secondary_count < 8 {
                // Copy sprite to secondary OAM
                self.copy_sprite_to_secondary(n);
                secondary_count += 1;
            } else {
                // Overflow! Set flag
                self.status |= 0x20;

                // Hardware bug: incorrectly increment m
                n += 1;
                m = (m + 1) % 4; // Should always be 0, but hardware increments

                // This causes false positives/negatives
                break;
            }
        }

        n += 1;
    }
}
```

**Most Emulators:** Simplify by always setting overflow when 9+ sprites are on a scanline.

---

## Implementation Guide

### Core Rendering Loop

```rust
pub fn render_pixel(&mut self) {
    if self.dot == 0 || self.dot > 256 || self.scanline >= 240 {
        return; // Not a visible pixel
    }

    let x = (self.dot - 1) as u8;
    let y = self.scanline as u8;

    // Get background pixel
    let bg_pixel = self.get_background_pixel();

    // Get sprite pixel
    let sprite_pixel = self.get_sprite_pixel(x);

    // Check sprite 0 hit
    if let Some(ref sp) = sprite_pixel {
        if sp.sprite_zero {
            self.check_sprite_0_hit(x, bg_pixel, sprite_pixel);
        }
    }

    // Multiplex and output
    let color = self.multiplex_pixel(bg_pixel, sprite_pixel);
    self.framebuffer[(y as usize * 256) + x as usize] = color;

    // Shift registers for next pixel
    self.shift_registers();
}
```

### Framebuffer Layout

```rust
pub struct Framebuffer {
    pixels: [u8; 256 * 240], // 61,440 pixels, 6-bit color indices
}

impl Framebuffer {
    pub fn set_pixel(&mut self, x: u8, y: u8, color: u8) {
        self.pixels[(y as usize * 256) + x as usize] = color;
    }

    pub fn as_rgb(&self, palette: &[u8; 64 * 3]) -> Vec<u8> {
        self.pixels.iter()
            .flat_map(|&color_index| {
                let offset = (color_index as usize) * 3;
                &palette[offset..offset + 3]
            })
            .copied()
            .collect()
    }
}
```

---

## Test ROM Validation

### Rendering Test ROMs

1. **color_test**
   - Validates palette rendering
   - Tests all 64 colors

2. **sprite_hit_tests**
   - Tests sprite 0 hit timing
   - Validates edge cases (leftmost 8 pixels, transparency)

3. **sprite_overflow_tests**
   - Tests overflow flag behavior
   - Validates hardware bug emulation

4. **sprdma_and_dmc_dma**
   - Tests sprite DMA timing
   - Validates DMC DMA conflicts

5. **full_palette**
   - Displays all 512 color combinations
   - Tests attribute table rendering

### Validation Checklist

- [ ] Background rendering uses shift registers
- [ ] Attribute table palette selection correct
- [ ] Sprite evaluation finds 8 sprites per scanline
- [ ] Sprite 0 hit detected when opaque pixels overlap
- [ ] Sprite 0 hit not detected in leftmost 8 pixels (when restricted)
- [ ] Sprite overflow flag set when 9+ sprites on scanline
- [ ] Priority multiplexer respects sprite priority attribute
- [ ] Transparent pixels (color 0) handled correctly
- [ ] 8×16 sprite mode renders correctly

---

## References

- [NesDev Wiki - PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)
- [PPU Sprite Evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation)
- [PPU Sprite Priority](https://www.nesdev.org/wiki/PPU_sprite_priority)
- [PPU OAM](https://www.nesdev.org/wiki/PPU_OAM)

---

**Next:** [PPU Scrolling](PPU_SCROLLING.md) | [Back to PPU Overview](PPU_OVERVIEW.md)
