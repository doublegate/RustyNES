# PPU Sprite Evaluation and Rendering

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18
**Scope:** Secondary OAM population, sprite overflow bug, sprite 0 hit timing

---

## Table of Contents

- [Overview](#overview)
- [Sprite Evaluation Process](#sprite-evaluation-process)
- [Secondary OAM](#secondary-oam)
- [Sprite Overflow Bug](#sprite-overflow-bug)
- [Sprite 0 Hit](#sprite-0-hit)
- [Sprite Fetch Timing](#sprite-fetch-timing)
- [Priority and Transparency](#priority-and-transparency)
- [Implementation Guide](#implementation-guide)

---

## Overview

The NES PPU can display **64 sprites** in OAM but only **8 sprites per scanline**. Sprite evaluation occurs during rendering to determine which 8 sprites to display.

### Key Specifications

```
Primary OAM:   256 bytes (64 sprites × 4 bytes each)
Secondary OAM: 32 bytes (8 sprites × 4 bytes each)
Sprite Limit:  8 sprites per scanline (hardware limitation)
Sprite Size:   8×8 or 8×16 pixels
```

### Sprite Data Format

Each sprite in OAM consists of 4 bytes:

```
Byte 0: Y position (scanline - 1)
Byte 1: Tile index
Byte 2: Attributes (VHPPPPPP)
        V = Vertical flip
        H = Horizontal flip
        P = Palette (0-3)
Byte 3: X position
```

---

## Sprite Evaluation Process

### Timeline

```
Dot 0:     Idle
Dot 1-64:  Clear secondary OAM to $FF
Dot 65-256: Sprite evaluation
Dot 257-320: Sprite fetches
```

### Dot 1-64: Secondary OAM Clear

```rust
// Dots 1-64: Write $FF to all 32 bytes of secondary OAM
if dot >= 1 && dot <= 64 {
    let index = ((dot - 1) / 2) as usize;
    secondary_oam[index] = 0xFF;
}
```

**Timing:** 2 dots per byte (64 dots total for 32 bytes)

### Dot 65-256: Sprite Evaluation

The PPU searches OAM for sprites on the next scanline:

```
n = 0  // OAM sprite index (0-63)
m = 0  // Byte within sprite (0-3)
found = 0  // Number of sprites found (0-8)

For dots 65-256:
    If found < 8:
        Read OAM[n*4 + m]

        If m == 0:  // Reading Y position
            If sprite is on next scanline:
                Copy all 4 bytes to secondary OAM
                found++
                n++
                m = 0
            Else:
                n++  // Skip this sprite
                m = 0
        Else:
            // Copying remaining bytes
            Copy byte to secondary OAM
            m++
            If m == 4:
                m = 0  // Sprite fully copied

    Elif found == 8:
        // 8 sprites found, check for overflow
        Sprite overflow check (with hardware bug)
```

---

## Secondary OAM

### Structure

```
Secondary OAM Layout (32 bytes):

Offset  Sprite
------  ------
$00-03  Sprite 0
$04-07  Sprite 1
$08-0B  Sprite 2
$0C-0F  Sprite 3
$10-13  Sprite 4
$14-17  Sprite 5
$18-1B  Sprite 6
$1C-1F  Sprite 7
```

### Population Algorithm

```rust
fn evaluate_sprites(&mut self) {
    let next_scanline = self.scanline + 1;
    let sprite_height = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

    self.sprite_count = 0;
    self.sprite_0_on_next_line = false;

    for n in 0..64 {
        let oam_addr = n * 4;
        let y = self.oam[oam_addr];

        // Check if sprite is on next scanline
        let diff = next_scanline.wrapping_sub(y as u16);
        if diff < sprite_height {
            // Sprite is on next scanline
            if self.sprite_count < 8 {
                // Copy to secondary OAM
                let sec_addr = self.sprite_count * 4;
                self.secondary_oam[sec_addr] = self.oam[oam_addr];        // Y
                self.secondary_oam[sec_addr + 1] = self.oam[oam_addr + 1]; // Tile
                self.secondary_oam[sec_addr + 2] = self.oam[oam_addr + 2]; // Attr
                self.secondary_oam[sec_addr + 3] = self.oam[oam_addr + 3]; // X

                if n == 0 {
                    self.sprite_0_on_next_line = true;
                }

                self.sprite_count += 1;
            } else {
                // 9th sprite found: set overflow flag
                self.ppustatus |= 0x20;
                break;
            }
        }
    }
}
```

---

## Sprite Overflow Bug

The actual hardware has a **bug** in sprite overflow detection:

### Hardware Bug Behavior

```
When 8 sprites are found and checking for 9th:
    Read Y position of sprite n
    If sprite is on scanline:
        Set overflow flag
        Increment both n AND m (BUG!)
    Else:
        Increment both n AND m (BUG!)

Result: m increments incorrectly, checking wrong bytes
        Most sprites beyond 8th are missed
        Overflow flag may not be set reliably
```

### Accurate Overflow Emulation

```rust
fn sprite_evaluation_with_overflow_bug(&mut self) {
    let mut n = 0;  // Sprite index
    let mut m = 0;  // Byte offset
    let mut found = 0;

    for dot in 65..=256 {
        if found < 8 {
            let oam_addr = (n * 4 + m) & 0xFF;  // 8-bit wrap
            let byte = self.oam[oam_addr as usize];

            if m == 0 {
                // Reading Y position
                let diff = (self.scanline + 1).wrapping_sub(byte as u16);
                let sprite_height = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

                if diff < sprite_height {
                    // Sprite on next scanline: copy all 4 bytes
                    for i in 0..4 {
                        let src = ((n * 4 + i) & 0xFF) as usize;
                        let dst = (found * 4 + i) as usize;
                        self.secondary_oam[dst] = self.oam[src];
                    }

                    found += 1;
                    if n == 0 {
                        self.sprite_0_on_next_line = true;
                    }
                }

                n += 1;
                if n >= 64 {
                    break;
                }
            }
        } else {
            // 8 sprites found: check for overflow (with bug)
            let oam_addr = (n * 4 + m) & 0xFF;
            let byte = self.oam[oam_addr as usize];

            if m == 0 {
                let diff = (self.scanline + 1).wrapping_sub(byte as u16);
                let sprite_height = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

                if diff < sprite_height {
                    self.ppustatus |= 0x20;  // Set overflow flag
                }
            }

            // BUG: Increment both n and m
            n += 1;
            m = (m + 1) & 0x03;  // Wrap at 4

            if n >= 64 {
                break;
            }
        }
    }
}
```

---

## Sprite 0 Hit

The **sprite 0 hit flag** (PPUSTATUS bit 6) is set when:

1. Non-transparent pixel of sprite 0 overlaps non-transparent background pixel
2. Hit occurs at X position 1-255 (not 0, not 256+)
3. Rendering is enabled (background and/or sprites)

### Timing

Sprite 0 hit is checked **every dot during rendering** (dots 1-256):

```rust
fn check_sprite_0_hit(&mut self, bg_pixel: u8, sprite_pixel: u8, x: u16) {
    if !self.sprite_0_on_current_line {
        return;
    }

    if x == 0 || x >= 255 {
        return;  // Sprite 0 hit doesn't occur at X=0 or X>=255
    }

    if bg_pixel == 0 || sprite_pixel == 0 {
        return;  // Both must be non-transparent
    }

    // Set sprite 0 hit flag
    self.ppustatus |= 0x40;
}
```

### Practical Usage

Sprite 0 hit is commonly used for:

- **Split-screen scrolling** (status bar at top)
- **Scanline detection** (trigger IRQ at specific scanline)
- **Parallax effects**

**Example:**

```rust
// Wait for sprite 0 hit
while (bus.read(0x2002) & 0x40) == 0 { }

// Hit occurred: change scroll
bus.write(0x2005, new_x);
bus.write(0x2005, new_y);
```

---

## Sprite Fetch Timing

### Dots 257-320: Sprite Pattern Fetches

For each of the 8 sprites in secondary OAM:

```
Dot Pattern (8 dots per sprite):
  Dot N+0: Garbage NT fetch
  Dot N+1: Garbage NT fetch
  Dot N+2: Garbage AT fetch
  Dot N+3: Garbage AT fetch
  Dot N+4: Pattern low fetch
  Dot N+5: Pattern low fetch
  Dot N+6: Pattern high fetch
  Dot N+7: Pattern high fetch
```

### Pattern Table Address Calculation

#### 8×8 Sprites

```rust
fn get_sprite_pattern_addr(&self, tile_index: u8, row: u8) -> u16 {
    let base = if self.ppuctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
    base + (tile_index as u16 * 16) + row as u16
}
```

#### 8×16 Sprites

```rust
fn get_sprite_pattern_addr_8x16(&self, tile_index: u8, row: u8) -> u16 {
    let bank = if tile_index & 0x01 != 0 { 0x1000 } else { 0x0000 };
    let tile = (tile_index & 0xFE) as u16;

    if row < 8 {
        // Top half
        bank + (tile * 16) + row as u16
    } else {
        // Bottom half
        bank + ((tile + 1) * 16) + (row - 8) as u16
    }
}
```

---

## Priority and Transparency

### Sprite Priority

Sprites are rendered **back-to-front** (sprite 0 has highest priority):

```
If multiple sprites overlap at same pixel:
    Display sprite with lowest index (0-7 in secondary OAM)
```

### Background vs Sprite Priority

Sprite attribute byte bit 5 controls priority:

```
Priority bit = 0: Sprite in front of background
Priority bit = 1: Sprite behind background

If sprite priority = 1 and bg_pixel != 0:
    Display background
Else:
    Display sprite (if sprite_pixel != 0)
```

### Full Priority Logic

```rust
fn get_pixel(&self, bg_pixel: u8, sprite_pixel: u8, sprite_priority: bool) -> u8 {
    match (bg_pixel, sprite_pixel) {
        (0, 0) => self.backdrop_color(),      // Both transparent
        (bg, 0) => bg,                         // Only background
        (0, sp) => sp,                         // Only sprite
        (bg, sp) => {
            if sprite_priority {
                bg  // Sprite behind background
            } else {
                sp  // Sprite in front
            }
        }
    }
}
```

---

## Implementation Guide

### Complete Sprite Rendering

```rust
pub struct SpriteRenderer {
    secondary_oam: [u8; 32],
    sprite_count: usize,
    sprite_0_on_current_line: bool,

    // Sprite shifters (8 sprites)
    pattern_lo: [u8; 8],
    pattern_hi: [u8; 8],
    attribute: [u8; 8],
    x_counter: [u8; 8],
}

impl SpriteRenderer {
    pub fn evaluate_sprites(&mut self, scanline: u16, oam: &[u8; 256], ppuctrl: u8) {
        self.sprite_count = 0;
        self.sprite_0_on_current_line = false;

        let next_scanline = scanline.wrapping_add(1);
        let sprite_height = if ppuctrl & 0x20 != 0 { 16 } else { 8 };

        for n in 0..64 {
            let y = oam[n * 4];
            let diff = next_scanline.wrapping_sub(y as u16);

            if diff < sprite_height && self.sprite_count < 8 {
                let base = self.sprite_count * 4;
                self.secondary_oam[base] = oam[n * 4];
                self.secondary_oam[base + 1] = oam[n * 4 + 1];
                self.secondary_oam[base + 2] = oam[n * 4 + 2];
                self.secondary_oam[base + 3] = oam[n * 4 + 3];

                if n == 0 {
                    self.sprite_0_on_current_line = true;
                }

                self.sprite_count += 1;
            }
        }
    }

    pub fn load_sprites(&mut self, scanline: u16, vram: &impl VramRead, ppuctrl: u8) {
        for i in 0..8 {
            if i < self.sprite_count {
                let y = self.secondary_oam[i * 4];
                let tile = self.secondary_oam[i * 4 + 1];
                let attr = self.secondary_oam[i * 4 + 2];
                let x = self.secondary_oam[i * 4 + 3];

                let row = scanline.wrapping_sub(y as u16);
                let addr = self.get_pattern_addr(tile, row, ppuctrl);

                let mut lo = vram.read(addr);
                let mut hi = vram.read(addr + 8);

                // Horizontal flip
                if attr & 0x40 != 0 {
                    lo = self.reverse_bits(lo);
                    hi = self.reverse_bits(hi);
                }

                self.pattern_lo[i] = lo;
                self.pattern_hi[i] = hi;
                self.attribute[i] = attr;
                self.x_counter[i] = x;
            } else {
                self.pattern_lo[i] = 0;
                self.pattern_hi[i] = 0;
                self.x_counter[i] = 0xFF;
            }
        }
    }

    pub fn tick(&mut self) {
        for i in 0..8 {
            if self.x_counter[i] == 0 {
                // Shift sprite pattern
                self.pattern_lo[i] >>= 1;
                self.pattern_hi[i] >>= 1;
            } else if self.x_counter[i] < 0xFF {
                self.x_counter[i] -= 1;
            }
        }
    }

    pub fn get_pixel(&self) -> (u8, u8, bool) {
        for i in 0..8 {
            if self.x_counter[i] == 0 {
                let lo_bit = self.pattern_lo[i] & 0x01;
                let hi_bit = self.pattern_hi[i] & 0x01;
                let pixel = (hi_bit << 1) | lo_bit;

                if pixel != 0 {
                    let palette = (self.attribute[i] & 0x03) + 4;
                    let priority = (self.attribute[i] & 0x20) != 0;
                    return (pixel, palette, priority);
                }
            }
        }

        (0, 0, false)  // Transparent
    }

    fn reverse_bits(&self, byte: u8) -> u8 {
        let mut result = 0u8;
        for i in 0..8 {
            result |= ((byte >> i) & 1) << (7 - i);
        }
        result
    }
}
```

---

## Related Documentation

- [PPU_2C02_SPECIFICATION.md](PPU_2C02_SPECIFICATION.md) - PPU registers
- [PPU_RENDERING.md](PPU_RENDERING.md) - Complete rendering pipeline
- [PPU_TIMING_DIAGRAM.md](PPU_TIMING_DIAGRAM.md) - Dot-by-dot timing
- [PPU_OVERVIEW.md](PPU_OVERVIEW.md) - High-level architecture

---

## References

- [NESdev Wiki: PPU Sprite Evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation)
- [NESdev Wiki: PPU OAM](https://www.nesdev.org/wiki/PPU_OAM)
- [NESdev Wiki: Sprite 0 Hit](https://www.nesdev.org/wiki/PPU_OAM#Sprite_zero_hits)
- sprite_hit_tests_2005.nes test ROM
- sprite_overflow_tests.nes test ROM

---

**Document Status:** Complete sprite evaluation, overflow bug, and sprite 0 hit specification.
