# [Milestone 2] Sprint 2: VRAM & Scrolling

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 2025
**Assignee:** Claude Code / Developer

---

## Overview

Implemented VRAM, palette RAM, and the Loopy scrolling model. This sprint establishes the memory system and scroll register logic required for accurate rendering.

---

## Acceptance Criteria

- [x] VRAM with all mirroring modes
- [x] Palette RAM with backdrop mirroring
- [x] Loopy scrolling registers (v, t, fine_x, w)
- [x] PPUADDR/PPUSCROLL write handling
- [x] Coarse/fine scroll support
- [x] Zero unsafe code
- [x] Unit tests for mirroring and scrolling

---

## Tasks

### Task 1: VRAM Implementation ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 3 hours
- **Actual:** ~3 hours

**Description:**
Implement VRAM with support for all nametable mirroring modes.

**Files:**

- `crates/rustynes-ppu/src/vram.rs` - VRAM implementation (377 lines)

**Subtasks:**

- [x] 2 KB internal nametable RAM
- [x] Horizontal mirroring (vertical arrangement)
- [x] Vertical mirroring (horizontal arrangement)
- [x] Four-screen mirroring (4 KB external RAM)
- [x] Single-screen mirroring
- [x] Address mirroring ($3000-$3EFF mirrors $2000-$2EFF)
- [x] Read/write methods

**Implementation:**

```rust
pub struct Vram {
    nametables: [u8; 2048],  // 2 KB internal
    mirroring: Mirroring,
}

pub enum Mirroring {
    Horizontal,   // Vertical arrangement (AB AB)
    Vertical,     // Horizontal arrangement (AA BB)
    SingleScreen, // All same nametable
    FourScreen,   // 4 KB external RAM (handled by mapper)
}

impl Vram {
    fn mirror_address(&self, addr: u16) -> usize {
        let addr = addr & 0x2FFF; // Mirror $3000-$3FFF to $2000-$2FFF
        let addr = addr & 0x0FFF; // Remove $2000 base

        match self.mirroring {
            Mirroring::Horizontal => {
                // $2000-$23FF, $2400-$27FF → first 1KB
                // $2800-$2BFF, $2C00-$2FFF → second 1KB
                if addr < 0x0800 {
                    (addr & 0x03FF) as usize
                } else {
                    (0x0400 | (addr & 0x03FF)) as usize
                }
            }
            Mirroring::Vertical => {
                // $2000-$23FF, $2800-$2BFF → first 1KB
                // $2400-$27FF, $2C00-$2FFF → second 1KB
                ((addr & 0x0400) | (addr & 0x03FF)) as usize
            }
            Mirroring::SingleScreen => {
                (addr & 0x03FF) as usize
            }
            Mirroring::FourScreen => {
                // Four-screen uses external RAM (mapper handles this)
                (addr & 0x0FFF) as usize
            }
        }
    }
}
```

---

### Task 2: Palette RAM ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Implement palette RAM with backdrop color mirroring.

**Files:**

- `crates/rustynes-ppu/src/vram.rs` - Palette RAM

**Subtasks:**

- [x] 32 bytes palette RAM ($3F00-$3F1F)
- [x] Backdrop mirroring ($3F10/$3F14/$3F18/$3F1C → $3F00/04/08/0C)
- [x] Address mirroring ($3F20-$3FFF mirrors $3F00-$3F1F)
- [x] Read/write methods
- [x] Grayscale masking

**Implementation:**

```rust
pub struct PaletteRam {
    data: [u8; 32],
}

impl PaletteRam {
    pub fn read(&self, addr: u16) -> u8 {
        let addr = self.mirror_palette_addr(addr);
        self.data[addr]
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        let addr = self.mirror_palette_addr(addr);
        self.data[addr] = value & 0x3F; // Only 6 bits used
    }

    fn mirror_palette_addr(&self, addr: u16) -> usize {
        let addr = (addr & 0x1F) as usize; // Mirror $3F20-$3FFF to $3F00-$3F1F

        // Backdrop color mirroring
        match addr {
            0x10 => 0x00,  // $3F10 → $3F00
            0x14 => 0x04,  // $3F14 → $3F04
            0x18 => 0x08,  // $3F18 → $3F08
            0x1C => 0x0C,  // $3F1C → $3F0C
            _ => addr,
        }
    }
}
```

---

### Task 3: Loopy Scrolling Registers ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 4 hours
- **Actual:** ~4 hours

**Description:**
Implement Loopy scrolling model with v, t, fine_x, and w registers.

**Files:**

- `crates/rustynes-ppu/src/scroll.rs` - Scrolling logic (357 lines)

**Subtasks:**

- [x] v register (current VRAM address, 15 bits)
- [x] t register (temporary VRAM address, 15 bits)
- [x] fine_x (fine X scroll, 3 bits)
- [x] w (write toggle, 1 bit)
- [x] Coarse X/Y extraction (5 bits each)
- [x] Fine Y extraction (3 bits)
- [x] Nametable select (2 bits)

**Implementation:**

```rust
pub struct ScrollRegisters {
    pub v: u16,       // Current VRAM address (15 bits)
    pub t: u16,       // Temporary VRAM address (15 bits)
    pub fine_x: u8,   // Fine X scroll (3 bits)
    pub w: bool,      // Write toggle (false = first write, true = second)
}

impl ScrollRegisters {
    // Extract components from v register
    pub fn coarse_x(&self) -> u8 {
        (self.v & 0x001F) as u8
    }

    pub fn coarse_y(&self) -> u8 {
        ((self.v >> 5) & 0x001F) as u8
    }

    pub fn fine_y(&self) -> u8 {
        ((self.v >> 12) & 0x0007) as u8
    }

    pub fn nametable(&self) -> u8 {
        ((self.v >> 10) & 0x0003) as u8
    }

    // Set components in v register
    pub fn set_coarse_x(&mut self, value: u8) {
        self.v = (self.v & 0xFFE0) | (value as u16 & 0x001F);
    }

    pub fn set_coarse_y(&mut self, value: u8) {
        self.v = (self.v & 0xFC1F) | ((value as u16 & 0x001F) << 5);
    }

    pub fn set_fine_y(&mut self, value: u8) {
        self.v = (self.v & 0x8FFF) | ((value as u16 & 0x0007) << 12);
    }

    pub fn set_nametable(&mut self, value: u8) {
        self.v = (self.v & 0xF3FF) | ((value as u16 & 0x0003) << 10);
    }
}
```

---

### Task 4: PPUADDR/PPUSCROLL Writes ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 3 hours
- **Actual:** ~3 hours

**Description:**
Implement PPUADDR and PPUSCROLL register write behavior.

**Files:**

- `crates/rustynes-ppu/src/scroll.rs` - Write handlers

**Subtasks:**

- [x] PPUSCROLL first write (coarse X, fine X)
- [x] PPUSCROLL second write (coarse Y, fine Y)
- [x] PPUADDR first write (high byte)
- [x] PPUADDR second write (low byte, copy t → v)
- [x] Write toggle (w) management
- [x] PPUSTATUS read resets w

**Implementation:**

```rust
pub fn write_scroll(&mut self, value: u8) {
    if !self.w {
        // First write: coarse X and fine X
        self.t = (self.t & 0xFFE0) | ((value as u16) >> 3);
        self.fine_x = value & 0x07;
        self.w = true;
    } else {
        // Second write: coarse Y and fine Y
        self.t = (self.t & 0x8FFF) | (((value as u16) & 0x07) << 12);
        self.t = (self.t & 0xFC1F) | (((value as u16) & 0xF8) << 2);
        self.w = false;
    }
}

pub fn write_addr(&mut self, value: u8) {
    if !self.w {
        // First write: high byte
        self.t = (self.t & 0x00FF) | (((value as u16) & 0x3F) << 8);
        self.w = true;
    } else {
        // Second write: low byte and copy t → v
        self.t = (self.t & 0xFF00) | (value as u16);
        self.v = self.t;
        self.w = false;
    }
}

pub fn read_status(&mut self) -> u8 {
    self.w = false; // Reset write toggle
    // ... return status ...
}
```

---

### Task 5: Scroll Increment Logic ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Implement horizontal and vertical scroll increment logic.

**Files:**

- `crates/rustynes-ppu/src/scroll.rs` - Increment methods

**Subtasks:**

- [x] Increment coarse X with wraparound and nametable switch
- [x] Increment fine Y with wraparound to coarse Y
- [x] Increment coarse Y with wraparound and nametable switch
- [x] Copy horizontal scroll (t → v)
- [x] Copy vertical scroll (t → v)

**Implementation:**

```rust
pub fn increment_x(&mut self) {
    if (self.v & 0x001F) == 31 {
        // Wraparound coarse X and switch horizontal nametable
        self.v &= !0x001F;
        self.v ^= 0x0400;
    } else {
        self.v += 1;
    }
}

pub fn increment_y(&mut self) {
    if (self.v & 0x7000) != 0x7000 {
        // Increment fine Y
        self.v += 0x1000;
    } else {
        // Fine Y overflow, reset and increment coarse Y
        self.v &= !0x7000;
        let mut y = (self.v & 0x03E0) >> 5;

        if y == 29 {
            // Wraparound and switch vertical nametable
            y = 0;
            self.v ^= 0x0800;
        } else if y == 31 {
            // Out of bounds, wrap without nametable switch
            y = 0;
        } else {
            y += 1;
        }

        self.v = (self.v & !0x03E0) | (y << 5);
    }
}
```

---

### Task 6: Unit Tests ✅

- **Status:** ✅ Done
- **Priority:** Medium
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Create unit tests for VRAM mirroring and scrolling logic.

**Files:**

- `crates/rustynes-ppu/src/lib.rs` - Test module

**Subtasks:**

- [x] Test horizontal mirroring
- [x] Test vertical mirroring
- [x] Test palette mirroring
- [x] Test PPUSCROLL writes
- [x] Test PPUADDR writes
- [x] Test scroll increment X/Y

**Tests Created:**

- `test_vram_horizontal_mirroring`
- `test_vram_vertical_mirroring`
- `test_palette_backdrop_mirroring`
- `test_ppuscroll_writes`
- `test_ppuaddr_writes`
- `test_scroll_increment_x`
- `test_scroll_increment_y`

---

## Dependencies

**Required:**

- Sprint 1 complete (PPU registers)

**Blocks:**

- Sprint 3: Background Rendering (needs VRAM and scroll)

---

## Related Documentation

- [PPU Scrolling](https://www.nesdev.org/wiki/PPU_scrolling)
- [PPU Memory Map](https://www.nesdev.org/wiki/PPU_memory_map)
- [Mirroring](https://www.nesdev.org/wiki/Mirroring)
- [Loopy's Document](https://www.nesdev.org/loopydocs/ppu.txt)

---

## Commits

- `02e76b9` - feat(ppu): implement complete cycle-accurate 2C02 PPU emulation

---

## Retrospective

### What Went Well

- Loopy scrolling model is well-documented
- Mirroring logic is straightforward
- Unit tests verify complex behavior

### What Could Be Improved

- Could add property-based tests for scroll increments
- Performance profiling for VRAM access

### Lessons Learned

- Loopy scrolling is elegant but subtle
- Palette mirroring has special cases
- Write toggle (w) requires careful management
