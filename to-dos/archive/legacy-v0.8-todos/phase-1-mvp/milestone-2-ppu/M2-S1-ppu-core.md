# [Milestone 2] Sprint 1: PPU Core & Registers

**Status:** ✅ COMPLETED
**Started:** December 2025
**Completed:** December 2025
**Assignee:** Claude Code / Developer

---

## Overview

Implemented the foundational PPU structure including all registers, timing system, and VBlank/NMI generation. This sprint establishes the skeleton for all PPU rendering operations.

---

## Acceptance Criteria

- [x] PPU registers (PPUCTRL, PPUMASK, PPUSTATUS, OAMADDR)
- [x] Register bitflags using bitflags crate
- [x] Dot/scanline timing (341×262 NTSC)
- [x] VBlank flag and NMI generation
- [x] Frame synchronization
- [x] Zero unsafe code
- [x] Unit tests for register behavior

---

## Tasks

### Task 1: PPU Register Structure ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 3 hours
- **Actual:** ~3 hours

**Description:**
Create PPU register structures using bitflags for type-safe manipulation.

**Files:**

- `crates/rustynes-ppu/src/registers.rs` - Register definitions (320 lines)
- `crates/rustynes-ppu/src/lib.rs` - Public exports

**Subtasks:**

- [x] Define PPUCTRL bitflags (nametable, increment, sprite/bg tables, sprite size, NMI)
- [x] Define PPUMASK bitflags (greyscale, show bg/sprites, emphasize RGB)
- [x] Define PPUSTATUS bitflags (sprite overflow, sprite 0 hit, vblank)
- [x] Implement register read/write methods
- [x] Handle write-only/read-only register behavior
- [x] Open bus behavior for unused bits

**Implementation:**

```rust
bitflags! {
    pub struct PpuCtrl: u8 {
        const NAMETABLE_X        = 0b0000_0001;
        const NAMETABLE_Y        = 0b0000_0010;
        const INCREMENT_MODE     = 0b0000_0100;
        const SPRITE_PATTERN     = 0b0000_1000;
        const BG_PATTERN         = 0b0001_0000;
        const SPRITE_SIZE        = 0b0010_0000;
        const MASTER_SLAVE       = 0b0100_0000;
        const NMI_ENABLE         = 0b1000_0000;
    }
}

bitflags! {
    pub struct PpuMask: u8 {
        const GREYSCALE          = 0b0000_0001;
        const SHOW_BG_LEFT       = 0b0000_0010;
        const SHOW_SPRITES_LEFT  = 0b0000_0100;
        const SHOW_BG            = 0b0000_1000;
        const SHOW_SPRITES       = 0b0001_0000;
        const EMPHASIZE_RED      = 0b0010_0000;
        const EMPHASIZE_GREEN    = 0b0100_0000;
        const EMPHASIZE_BLUE     = 0b1000_0000;
    }
}

bitflags! {
    pub struct PpuStatus: u8 {
        const SPRITE_OVERFLOW    = 0b0010_0000;
        const SPRITE_0_HIT       = 0b0100_0000;
        const VBLANK             = 0b1000_0000;
    }
}
```

---

### Task 2: Timing System ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 4 hours
- **Actual:** ~4 hours

**Description:**
Implement dot-accurate timing system for PPU rendering cycles.

**Files:**

- `crates/rustynes-ppu/src/timing.rs` - Timing logic (418 lines)

**Subtasks:**

- [x] Dot counter (0-340)
- [x] Scanline counter (0-261)
- [x] Frame counter
- [x] Rendering scanlines (0-239)
- [x] Post-render scanline (240)
- [x] VBlank scanlines (241-260)
- [x] Pre-render scanline (261)
- [x] Odd frame skip (pre-render scanline shortened by 1 dot)

**Implementation:**

```rust
pub struct Timing {
    pub dot: u16,           // 0-340
    pub scanline: u16,      // 0-261
    pub frame: u64,         // Frame counter
    pub odd_frame: bool,    // Odd/even frame toggle
}

impl Timing {
    pub fn tick(&mut self) {
        self.dot += 1;

        // Handle end of scanline
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;

            // Handle end of frame
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame += 1;
                self.odd_frame = !self.odd_frame;
            }
        }

        // Odd frame skip: pre-render scanline is 1 dot shorter
        if self.scanline == 261 && self.dot == 339 && self.odd_frame {
            self.dot = 0;
            self.scanline = 0;
            self.frame += 1;
            self.odd_frame = false;
        }
    }

    pub fn is_visible_scanline(&self) -> bool {
        self.scanline < 240
    }

    pub fn is_pre_render_scanline(&self) -> bool {
        self.scanline == 261
    }

    pub fn is_vblank(&self) -> bool {
        self.scanline >= 241 && self.scanline <= 260
    }
}
```

---

### Task 3: VBlank and NMI Generation ✅

- **Status:** ✅ Done
- **Priority:** High
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Implement VBlank flag timing and NMI generation for CPU interrupt.

**Files:**

- `crates/rustynes-ppu/src/ppu.rs` - VBlank logic

**Subtasks:**

- [x] VBlank flag set at scanline 241, dot 1
- [x] VBlank flag cleared at pre-render scanline
- [x] NMI edge detection (rising edge of vblank & nmi_enable)
- [x] PPUSTATUS read clears VBlank flag
- [x] Suppress NMI if flag read during set cycle

**Implementation:**

```rust
pub fn tick(&mut self) -> bool {
    let prev_nmi = self.nmi_pending();

    self.timing.tick();

    // Set VBlank flag at scanline 241, dot 1
    if self.timing.scanline == 241 && self.timing.dot == 1 {
        self.status.insert(PpuStatus::VBLANK);
    }

    // Clear VBlank and sprite flags on pre-render scanline
    if self.timing.scanline == 261 && self.timing.dot == 1 {
        self.status.remove(PpuStatus::VBLANK);
        self.status.remove(PpuStatus::SPRITE_0_HIT);
        self.status.remove(PpuStatus::SPRITE_OVERFLOW);
    }

    // NMI edge detection
    let current_nmi = self.nmi_pending();
    !prev_nmi && current_nmi
}

fn nmi_pending(&self) -> bool {
    self.ctrl.contains(PpuCtrl::NMI_ENABLE) &&
    self.status.contains(PpuStatus::VBLANK)
}
```

---

### Task 4: Frame Buffer ✅

- **Status:** ✅ Done
- **Priority:** Medium
- **Estimated:** 1 hour
- **Actual:** ~1 hour

**Description:**
Create frame buffer for rendered output.

**Files:**

- `crates/rustynes-ppu/src/ppu.rs` - Frame buffer

**Subtasks:**

- [x] 256×240 pixel buffer
- [x] RGB888 format (3 bytes per pixel)
- [x] Clear on frame start
- [x] Pixel write interface

**Implementation:**

```rust
pub struct Ppu {
    // ... registers ...
    pub framebuffer: [u8; 256 * 240 * 3], // RGB888
    // ...
}

impl Ppu {
    fn write_pixel(&mut self, x: u8, y: u8, palette_index: u8) {
        if x >= 256 || y >= 240 {
            return;
        }

        let rgb = PALETTE_RGB[palette_index as usize];
        let offset = ((y as usize * 256) + x as usize) * 3;

        self.framebuffer[offset] = rgb[0];     // R
        self.framebuffer[offset + 1] = rgb[1]; // G
        self.framebuffer[offset + 2] = rgb[2]; // B
    }
}
```

---

### Task 5: Unit Tests ✅

- **Status:** ✅ Done
- **Priority:** Medium
- **Estimated:** 2 hours
- **Actual:** ~2 hours

**Description:**
Create unit tests for register behavior and timing.

**Files:**

- `crates/rustynes-ppu/src/lib.rs` - Test module

**Subtasks:**

- [x] Test PPUCTRL write/read behavior
- [x] Test PPUMASK write/read behavior
- [x] Test PPUSTATUS read clears VBlank
- [x] Test VBlank timing (set at 241:1, clear at 261:1)
- [x] Test NMI generation
- [x] Test odd frame skip

**Tests Created:**

- `test_ppuctrl_write`
- `test_ppustatus_vblank_clear`
- `test_vblank_timing`
- `test_nmi_generation`
- `test_odd_frame_skip`

---

## Dependencies

**Required:**

- Rust 1.75+ toolchain
- bitflags = "2.4" crate
- log = "0.4" crate (optional, for debugging)

**Blocks:**

- Sprint 2: VRAM & Scrolling (needs register interface)
- Sprint 3: Background Rendering (needs timing)

---

## Related Documentation

- [PPU 2C02 Specification](../../../docs/ppu/PPU_2C02_SPECIFICATION.md)
- [PPU Timing Diagram](../../../docs/ppu/PPU_TIMING_DIAGRAM.md)
- [PPU Registers](https://www.nesdev.org/wiki/PPU_registers)

---

## Commits

- `02e76b9` - feat(ppu): implement complete cycle-accurate 2C02 PPU emulation

---

## Retrospective

### What Went Well

- bitflags made register handling clean and type-safe
- Timing system accurately matches hardware
- VBlank/NMI edge detection working correctly

### What Could Be Improved

- Could add more edge case tests
- Performance profiling for timing tick

### Lessons Learned

- PPU timing is critical for accurate emulation
- VBlank flag timing has subtle edge cases
- NMI suppression requires careful implementation
