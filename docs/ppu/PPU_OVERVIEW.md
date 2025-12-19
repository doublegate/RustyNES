# PPU Overview (2C02 Picture Processing Unit)

**Document Version:** 1.0.0
**Last Updated:** 2025-12-18

---

## Table of Contents

- [Introduction](#introduction)
- [PPU Specifications](#ppu-specifications)
- [Register Interface](#register-interface)
- [Memory Map](#memory-map)
- [Rendering Architecture](#rendering-architecture)
- [Coordinate Systems](#coordinate-systems)
- [Palette System](#palette-system)
- [Implementation Overview](#implementation-overview)
- [Common Pitfalls](#common-pitfalls)

---

## Introduction

The **Ricoh 2C02** (NTSC) and **2C07** (PAL) are the Picture Processing Units used in the NES. The PPU is a dedicated graphics processor that:

- Renders **256×240 pixel display** at **60 Hz** (NTSC) or **50 Hz** (PAL)
- Supports **2 layers**: Background (tile-based) and Sprites (objects)
- Uses **8×8 pixel tiles** for all graphics
- Manages a **64-color master palette** with **4-color sub-palettes**
- Operates **3× faster than the CPU** (5.37 MHz vs. 1.79 MHz)
- Provides **VBlank interrupt** for timing game logic

**Key Characteristics:**

- Dot-level rendering (341 dots per scanline, 262 scanlines per frame)
- Parallel processing (background and sprite evaluation occur simultaneously)
- Limited sprite capacity (8 sprites per scanline, 64 total)
- Hardware scrolling via internal registers

---

## PPU Specifications

### Clock and Timing

```
NTSC (2C02):
  Master Clock:   21.477272 MHz
  PPU Clock:      5.369318 MHz (÷4)
  CPU Clock:      1.789773 MHz (÷12)
  Ratio:          3 PPU dots per CPU cycle (exact)

PAL (2C07):
  Master Clock:   26.601712 MHz
  PPU Clock:      5.320342 MHz (÷5)
  CPU Clock:      1.662607 MHz (÷16)
  Ratio:          3.2 PPU dots per CPU cycle
```

### Frame Structure (NTSC)

```
Visible Scanlines:  0-239 (240 lines)
Post-Render:        240 (idle)
VBlank:             241-260 (20 lines)
Pre-Render:         261 (prepare for next frame)

Total Scanlines:    262
Dots per Line:      341 (340 on odd frames due to cycle skip)

Frame Time:         ~16.67 ms (60.0988 Hz)
VBlank Duration:    ~2.27 ms (20 scanlines)
```

### Display Resolution

```
Visible Resolution: 256×240 pixels
Overscan Adjustment: ~8 pixels on each edge (common on CRT TVs)
Safe Area:          240×224 pixels (common game rendering area)

Pixel Aspect Ratio: 8:7 (NTSC), 2.34:1 display aspect after overscan
```

---

## Register Interface

The PPU is controlled via **8 memory-mapped registers** at CPU addresses `$2000-$2007`. The registers are **mirrored** every 8 bytes through `$2000-$3FFF`.

### Register Map

| Address | Name | Access | Purpose |
|---------|------|--------|---------|
| **$2000** | PPUCTRL | Write | PPU control flags |
| **$2001** | PPUMASK | Write | Rendering enable/disable, color effects |
| **$2002** | PPUSTATUS | Read | VBlank status, sprite 0 hit, sprite overflow |
| **$2003** | OAMADDR | Write | OAM (sprite) address pointer |
| **$2004** | OAMDATA | R/W | OAM data read/write |
| **$2005** | PPUSCROLL | Write×2 | Scrolling position (X, then Y) |
| **$2006** | PPUADDR | Write×2 | VRAM address (high byte, then low byte) |
| **$2007** | PPUDATA | R/W | VRAM data read/write |

### Register Details

#### $2000 - PPUCTRL (Write)

```
7  bit  0
---- ----
VPHB SINN
|||| ||||
|||| ||++- Base nametable address
|||| ||    (0 = $2000; 1 = $2400; 2 = $2800; 3 = $2C00)
|||| |+--- VRAM address increment per CPU read/write of PPUDATA
|||| |     (0: add 1, going across; 1: add 32, going down)
|||| +---- Sprite pattern table address for 8x8 sprites
||||       (0: $0000; 1: $1000; ignored in 8x16 mode)
|||+------ Background pattern table address (0: $0000; 1: $1000)
||+------- Sprite size (0: 8x8 pixels; 1: 8x16 pixels)
|+-------- PPU master/slave select (0: master; 1: slave; not used on NES)
+--------- Generate NMI at start of VBlank (0: off; 1: on)
```

**Critical:** Writing to PPUCTRL during rendering can cause glitches. The nametable select bits directly affect the internal VRAM address register.

#### $2001 - PPUMASK (Write)

```
7  bit  0
---- ----
BGRs bMmG
|||| ||||
|||| |||+- Greyscale (0: normal color, 1: greyscale)
|||| ||+-- Show background in leftmost 8 pixels (0: hide, 1: show)
|||| |+--- Show sprites in leftmost 8 pixels (0: hide, 1: show)
|||| +---- Show background (0: hide, 1: show)
|||+------ Show sprites (0: hide, 1: show)
||+------- Emphasize red (NTSC: red, PAL: green)
|+-------- Emphasize green (NTSC: green, PAL: red)
+--------- Emphasize blue
```

**Usage:**

```rust
const SHOW_BACKGROUND: u8 = 0b0000_1000;
const SHOW_SPRITES: u8    = 0b0001_0000;
const RENDERING_ENABLED: u8 = SHOW_BACKGROUND | SHOW_SPRITES;

if (ppu_mask & RENDERING_ENABLED) != 0 {
    // Rendering is active
}
```

#### $2002 - PPUSTATUS (Read)

```
7  bit  0
---- ----
VSO. ....
|||| ||||
|||+-++++- PPU open bus (least significant bits previously written)
||+------- Sprite overflow (more than 8 sprites on a scanline)
|+-------- Sprite 0 hit (sprite 0 overlaps background)
+--------- VBlank flag (set at dot 1 of scanline 241, cleared on read)
```

**Important Side Effects:**

- Reading `$2002` clears the VBlank flag (bit 7)
- Reading `$2002` resets the `$2005`/`$2006` write latch
- Race condition: Reading on same cycle VBlank is set suppresses NMI

**Implementation:**

```rust
fn read_status(&mut self) -> u8 {
    let status = self.status_register;
    self.status_register &= 0x7F; // Clear VBlank flag
    self.write_latch = false;     // Reset address latch
    status
}
```

#### $2003 - OAMADDR (Write)

Sets the address in OAM (Object Attribute Memory) for `$2004` access:

```
OAMADDR = value
```

**Important:** Most games use `$4014` (OAM DMA) instead of manual writes to `$2004`.

#### $2004 - OAMDATA (Read/Write)

Read/write OAM data at the address specified by `$2003`:

```
Write: OAM[OAMADDR] = value; OAMADDR++
Read:  value = OAM[OAMADDR] (does not increment)
```

**Caution:** Reading during rendering returns unpredictable values. Writing during rendering corrupts OAM.

#### $2005 - PPUSCROLL (Write, ×2)

Sets scroll position. Must be written **twice** (X, then Y):

```
First write:  X scroll (0-255)
Second write: Y scroll (0-239)
```

**Implementation Detail:** PPUSCROLL writes to the internal 15-bit VRAM address register (`v` and `t`) via complex bit manipulation. See [PPU_SCROLLING.md](PPU_SCROLLING.md) for details.

#### $2006 - PPUADDR (Write, ×2)

Sets VRAM address for `$2007` access. Must be written **twice** (high byte, then low byte):

```
First write:  High byte (bits 13-8 of address)
Second write: Low byte (bits 7-0 of address)
```

**Address Range:** `$0000-$3FFF` (16 KB address space, mirrored)

**Example:**

```rust
// Set address to $2400
ppu.write(0x2006, 0x24); // High byte
ppu.write(0x2006, 0x00); // Low byte

// Now $2007 reads/writes will access $2400
```

#### $2007 - PPUDATA (Read/Write)

Read/write data from/to VRAM at the address specified by `$2006`:

```
Write: VRAM[vram_addr] = value; vram_addr += (PPUCTRL.I ? 32 : 1)
Read:  value = VRAM[vram_addr]; vram_addr += (PPUCTRL.I ? 32 : 1)
```

**Critical Quirk - Buffered Reads:**
Reading from `$2007` returns the contents of an internal buffer, NOT the current address. The buffer is updated with the current address after the read:

```
First read:  Returns buffered value (garbage)
             Buffer = VRAM[current_address]

Second read: Returns previous VRAM value
             Buffer = VRAM[current_address]
```

**Exception:** Reading palette data (`$3F00-$3FFF`) returns immediately without buffering.

**Implementation:**

```rust
fn read_ppudata(&mut self) -> u8 {
    let addr = self.vram_addr & 0x3FFF;
    let value = self.read_vram(addr);

    let result = if addr >= 0x3F00 {
        // Palette reads are immediate
        value
    } else {
        // Buffered read
        let buffered = self.read_buffer;
        self.read_buffer = value;
        buffered
    };

    self.increment_vram_addr();
    result
}
```

---

## Memory Map

The PPU has a **16 KB address space** (`$0000-$3FFF`) with mirroring:

### Full PPU Memory Map

```
$0000-$0FFF  Pattern Table 0 (4 KB, from CHR-ROM/RAM)
$1000-$1FFF  Pattern Table 1 (4 KB, from CHR-ROM/RAM)
$2000-$23FF  Nametable 0 (1 KB, internal VRAM)
$2400-$27FF  Nametable 1 (1 KB, internal VRAM)
$2800-$2BFF  Nametable 2 (1 KB, internal VRAM or mirrored)
$2C00-$2FFF  Nametable 3 (1 KB, internal VRAM or mirrored)
$3000-$3EFF  Mirrors of $2000-$2EFF
$3F00-$3F1F  Palette RAM (32 bytes)
$3F20-$3FFF  Mirrors of $3F00-$3F1F
```

### Pattern Tables (CHR-ROM/RAM)

**Pattern tables** store tile graphics. Each tile is 8×8 pixels with 2 bits per pixel (4 colors):

```
Tile Structure (16 bytes per tile):
  Bytes 0-7:  Bitplane 0 (low bit of color index)
  Bytes 8-15: Bitplane 1 (high bit of color index)

Example Tile:
  Bitplane 0:  0x41 = 01000001
  Bitplane 1:  0xC2 = 11000010

  Resulting pixels:
  %11, %01, %00, %00, %00, %00, %01, %11
   3    1    0    0    0    0    1    3
```

**Memory Layout:**

- Pattern Table 0: `$0000-$0FFF` (256 tiles)
- Pattern Table 1: `$1000-$1FFF` (256 tiles)

### Nametables (VRAM)

**Nametables** define which tiles appear on screen. The NES has **2 KB internal VRAM**, enough for 2 nametables. The other 2 are **mirrored**.

**Nametable Structure** (1024 bytes):

```
$0000-$03BF  Tile indices (32×30 = 960 bytes)
$03C0-$03FF  Attribute table (64 bytes)
```

**Mirroring Modes:**

- **Horizontal:** `$2000=$2400`, `$2800=$2C00` (vertical scrolling games)
- **Vertical:** `$2000=$2800`, `$2400=$2C00` (horizontal scrolling games)
- **Single-Screen:** All nametables mirror the same 1 KB
- **Four-Screen:** 4 KB of VRAM on cartridge (no mirroring)

### Attribute Table

The attribute table assigns one of 4 palettes to each **2×2 tile group** (16×16 pixels):

```
Each byte controls a 4×4 tile area (32×32 pixels):

  Byte layout:
  7654 3210
  |||| ||||
  |||| ||++- Palette for top-left 2×2 tiles
  |||| ++--- Palette for top-right 2×2 tiles
  ||++------ Palette for bottom-left 2×2 tiles
  ++-------- Palette for bottom-right 2×2 tiles
```

**Attribute Table Address Calculation:**

```rust
fn attribute_address(nametable_base: u16, tile_x: u8, tile_y: u8) -> u16 {
    let attr_x = tile_x / 4;
    let attr_y = tile_y / 4;
    nametable_base + 0x03C0 + (attr_y as u16 * 8) + attr_x as u16
}
```

### Palette RAM

Palette RAM stores **32 bytes** of palette data at `$3F00-$3F1F`:

```
$3F00  Universal background color
$3F01-$3F03  Background palette 0 (colors 1-3)
$3F05-$3F07  Background palette 1
$3F09-$3F0B  Background palette 2
$3F0D-$3F0F  Background palette 3

$3F10  Mirror of $3F00
$3F11-$3F13  Sprite palette 0 (colors 1-3)
$3F15-$3F17  Sprite palette 1
$3F19-$3F1B  Sprite palette 2
$3F1D-$3F1F  Sprite palette 3
```

**Special Addresses:**

- `$3F00`, `$3F04`, `$3F08`, `$3F0C`: Mirror the universal background color
- `$3F10`, `$3F14`, `$3F18`, `$3F1C`: Also mirror the universal background color
- Color `0` of each palette is transparent for sprites

---

## Rendering Architecture

### Two-Layer System

The PPU renders two independent layers:

1. **Background Layer**
   - Tile-based (8×8 pixel tiles)
   - Uses nametables to define layout
   - Hardware scrolling support
   - 4 palettes (16 colors total)

2. **Sprite Layer**
   - Up to 64 sprites (8×8 or 8×16 pixels)
   - Maximum 8 sprites per scanline
   - 4 palettes (12 colors total, color 0 is transparent)
   - Per-sprite priority and flipping

### Rendering Pipeline (Per Scanline)

```
Dots 0-255:    Fetch background tiles and render pixels
               Evaluate sprites for next scanline
               Output pixels to screen

Dots 256-320:  Fetch sprite data for next scanline
               Increment vertical scroll position

Dots 321-336:  Fetch first two tiles for next scanline

Dots 337-340:  Unused nametable fetches
```

### Sprite Evaluation

The PPU can display **up to 8 sprites per scanline**. Sprite evaluation occurs during rendering:

```
Dots 1-64:     Clear secondary OAM (8 sprite slots)
Dots 65-256:   Scan primary OAM (64 sprites)
               Copy sprites on current scanline to secondary OAM
               Set sprite overflow flag if more than 8 sprites
```

**Sprite 0 Hit:**

- Detects when sprite 0 (first sprite in OAM) overlaps non-transparent background pixel
- Used for split-screen effects and raster timing
- Set during rendering, cleared at dot 1 of pre-render scanline

---

## Coordinate Systems

### Screen Coordinates

```
X: 0-255 (256 pixels wide)
Y: 0-239 (240 pixels tall)

Origin: Top-left corner (0, 0)
```

### Tile Coordinates

```
Tile X: 0-31 (32 tiles wide)
Tile Y: 0-29 (30 tiles tall)

Tile Address = Nametable Base + (Tile Y × 32) + Tile X
```

### Fine Scroll Coordinates

```
Fine X: 0-7 (horizontal scroll within tile)
Fine Y: 0-7 (vertical scroll within tile)

Coarse X: 0-31 (tile column)
Coarse Y: 0-29 (tile row)

Total Scroll Range:
  X: 0-511 (2 nametables wide)
  Y: 0-479 (2 nametables tall)
```

---

## Palette System

### Master Palette

The NES has a **64-color master palette** (6-bit color):

```
Color Index: $00-$3F (0-63)

Bit layout: %00BBGGRR
            ||  || ||
            ||  || ++- Red intensity (0-3)
            ||  ++---- Green intensity (0-3)
            ++-------- Blue intensity (0-3)
```

**Special Colors:**

- `$0D`: True black (safe for all TVs)
- `$0E`, `$0F`: "Blacker than black" (may cause issues on some displays)
- `$1D`, `$2D`, `$3D`: Equivalent to `$0D`

### Sub-Palettes

Games select **4 colors** from the master palette for each sub-palette:

```
Background Palettes (4 total):
  Palette 0: $3F00 (universal), $3F01, $3F02, $3F03
  Palette 1: $3F00 (universal), $3F05, $3F06, $3F07
  Palette 2: $3F00 (universal), $3F09, $3F0A, $3F0B
  Palette 3: $3F00 (universal), $3F0D, $3F0E, $3F0F

Sprite Palettes (4 total):
  Palette 0: Transparent, $3F11, $3F12, $3F13
  Palette 1: Transparent, $3F15, $3F16, $3F17
  Palette 2: Transparent, $3F19, $3F1A, $3F1B
  Palette 3: Transparent, $3F1D, $3F1E, $3F1F
```

---

## Implementation Overview

### Core PPU Structure

```rust
pub struct Ppu {
    // Registers (CPU-accessible)
    ctrl: u8,          // $2000
    mask: u8,          // $2001
    status: u8,        // $2002
    oam_addr: u8,      // $2003

    // Internal state
    vram_addr: u16,    // Current VRAM address (v)
    temp_addr: u16,    // Temporary VRAM address (t)
    fine_x: u8,        // Fine X scroll (0-7)
    write_latch: bool, // First/second write toggle
    read_buffer: u8,   // PPUDATA read buffer

    // Timing
    scanline: u16,     // Current scanline (0-261)
    dot: u16,          // Current dot (0-340)
    frame_count: u64,  // Frame counter

    // Memory
    vram: [u8; 2048],  // 2 KB internal VRAM (nametables)
    palette: [u8; 32], // Palette RAM
    oam: [u8; 256],    // Object Attribute Memory (sprites)

    // Rendering state
    bg_shift_lo: u16,  // Background tile shift registers
    bg_shift_hi: u16,
    attr_shift_lo: u8, // Attribute shift registers
    attr_shift_hi: u8,
}
```

### Execution Loop

```rust
pub fn step(&mut self, cartridge: &mut Cartridge) -> bool {
    let rendering_enabled = (self.mask & 0x18) != 0;

    match (self.scanline, self.dot) {
        (0..=239, 1..=256) => {
            // Visible scanlines - render pixels
            if rendering_enabled {
                self.render_pixel();
                self.fetch_tile_data(cartridge);
            }
        }
        (241, 1) => {
            // Start of VBlank
            self.status |= 0x80; // Set VBlank flag
            if (self.ctrl & 0x80) != 0 {
                self.nmi_triggered = true; // Trigger NMI
            }
        }
        (261, 1) => {
            // Pre-render scanline - clear flags
            self.status &= 0x1F; // Clear VBlank, sprite 0, overflow
        }
        _ => {}
    }

    // Advance dot and scanline counters
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

---

## Common Pitfalls

### 1. PPUDATA Read Buffer

Forgetting to implement the read buffer causes immediate reads instead of buffered reads:

```rust
// WRONG: Immediate read
fn read_ppudata(&mut self) -> u8 {
    self.read_vram(self.vram_addr)
}

// CORRECT: Buffered read
fn read_ppudata(&mut self) -> u8 {
    let addr = self.vram_addr;
    let value = self.read_vram(addr);

    let result = if addr >= 0x3F00 {
        value // Palette reads are immediate
    } else {
        let buffered = self.read_buffer;
        self.read_buffer = value;
        buffered
    };

    self.increment_vram_addr();
    result
}
```

### 2. Write Latch Reset

Reading `$2002` must reset the write latch for `$2005`/`$2006`:

```rust
fn read_status(&mut self) -> u8 {
    let status = self.status;
    self.status &= 0x7F;      // Clear VBlank flag
    self.write_latch = false; // CRITICAL: Reset latch
    status
}
```

### 3. Palette Mirroring

Addresses `$3F10`, `$3F14`, `$3F18`, `$3F1C` mirror `$3F00` (universal background):

```rust
fn palette_addr(addr: u16) -> usize {
    let mut addr = (addr & 0x1F) as usize;

    // Mirror $3F10, $3F14, $3F18, $3F1C to $3F00
    if addr >= 16 && (addr & 0x03) == 0 {
        addr -= 16;
    }

    addr
}
```

### 4. Sprite Evaluation Timing

Sprite evaluation happens **during** rendering, not before the scanline:

```rust
// WRONG: Evaluate all sprites at once
fn evaluate_sprites(&mut self) {
    for sprite in &self.oam { /* evaluate */ }
}

// CORRECT: Evaluate during dots 65-256
fn step(&mut self) {
    if self.scanline < 240 && self.dot >= 65 && self.dot <= 256 {
        self.evaluate_sprite_for_dot();
    }
}
```

### 5. VBlank Timing Race Condition

Reading `$2002` on the exact cycle VBlank is set suppresses the NMI:

```rust
// Handle race condition at scanline 241, dot 1
if self.scanline == 241 && self.dot == 1 {
    self.status |= 0x80; // Set VBlank

    // If CPU reads $2002 on this exact cycle, NMI is suppressed
    if cpu_reading_status {
        self.nmi_triggered = false;
    } else if (self.ctrl & 0x80) != 0 {
        self.nmi_triggered = true;
    }
}
```

---

## References

- [NesDev Wiki - PPU](https://www.nesdev.org/wiki/PPU)
- [PPU Registers](https://www.nesdev.org/wiki/PPU_registers)
- [PPU Memory Map](https://www.nesdev.org/wiki/PPU_memory_map)
- [PPU Palettes](https://www.nesdev.org/wiki/PPU_palettes)
- [PPU Rendering](https://www.nesdev.org/wiki/PPU_rendering)

---

**Next:** [PPU Timing](PPU_TIMING.md) | [PPU Rendering](PPU_RENDERING.md) | [PPU Scrolling](PPU_SCROLLING.md)
