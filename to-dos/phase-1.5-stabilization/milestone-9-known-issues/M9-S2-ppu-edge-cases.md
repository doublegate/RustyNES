# M9 Sprint 2: PPU Edge Cases

## Overview

Resolve PPU edge cases including sprite overflow, palette RAM writes during rendering, scrolling split-screen effects, and mid-scanline register access.

## Objectives

- [ ] Implement accurate sprite overflow flag behavior
- [ ] Handle palette RAM writes during rendering
- [ ] Support scrolling split-screen effects (mid-scanline writes)
- [ ] Fix attribute handling edge cases
- [ ] Validate with complex games (Super Mario Bros. 3, Zelda)

## Tasks

### Task 1: Sprite Overflow Flag
- [ ] Study hardware sprite evaluation (8 sprite limit per scanline)
- [ ] Implement sprite overflow flag logic ($2002 bit 5)
- [ ] Handle hardware quirks (overflow flag false positives)
- [ ] Test with sprite overflow test ROMs
- [ ] Validate with games using many sprites (Mega Man, Gradius)

### Task 2: Palette RAM Edge Cases
- [ ] Handle writes to palette RAM during rendering
- [ ] Test palette mirroring during rendering ($3F10/$3F14/$3F18/$3F1C)
- [ ] Verify background color updates mid-frame
- [ ] Test with palette cycling effects (Battletoads, Mega Man)
- [ ] Validate color accuracy (compare to hardware screenshots)

### Task 3: Scrolling Split-Screen Effects
- [ ] Handle mid-scanline $2006 writes (VRAM address)
- [ ] Handle mid-scanline $2005 writes (scroll position)
- [ ] Test with Super Mario Bros. 3 (status bar split)
- [ ] Test with other split-screen games (Kirby's Adventure)
- [ ] Verify timing precision (scanline-accurate writes)

### Task 4: Attribute Handling Edge Cases
- [ ] Verify attribute byte extraction for all quadrants (v0.5.0 fix)
- [ ] Test attribute shift register reload timing
- [ ] Handle attribute fetches at tile boundaries
- [ ] Validate with attribute table test ROMs
- [ ] Test with games using complex palettes (Zelda, Mega Man)

## Implementation Details

### Sprite Overflow Flag

**Hardware Behavior:**
- PPU evaluates 64 OAM sprites during pre-render scanline
- Counts sprites on current scanline (y-coordinate check)
- If >8 sprites found, sets overflow flag ($2002 bit 5)
- **Quirk:** Hardware bug can cause false positives/negatives

**Implementation:**
```rust
fn evaluate_sprites(&mut self, scanline: u16) {
    let mut sprite_count = 0;
    self.sprite_overflow = false;

    for i in 0..64 {
        let y = self.oam[i * 4];
        let sprite_height = if self.control.sprite_size_8x16() { 16 } else { 8 };

        if scanline >= y as u16 && scanline < y as u16 + sprite_height {
            sprite_count += 1;
            if sprite_count > 8 {
                self.sprite_overflow = true;
                // Hardware bug: Continue checking with incorrect index increment
                break;
            }
        }
    }
}
```

### Palette RAM During Rendering

**Edge Case:**
- Palette RAM writes during rendering can cause visual artifacts
- NES hardware allows palette writes during rendering
- Some games use this for effects (color cycling)

**Implementation:**
```rust
fn write_palette(&mut self, addr: u16, value: u8) {
    let mirrored_addr = addr & 0x1F;

    // Mirror $3F10, $3F14, $3F18, $3F1C to $3F00, $3F04, $3F08, $3F0C
    let final_addr = if mirrored_addr >= 0x10 && (mirrored_addr & 0x03) == 0 {
        mirrored_addr - 0x10
    } else {
        mirrored_addr
    };

    self.palette_ram[final_addr as usize] = value;

    // If writing during rendering, update output immediately
    if self.rendering_enabled() && self.scanline < 240 {
        self.update_palette_output(final_addr, value);
    }
}
```

### Split-Screen Scrolling

**Technique:**
- Games use IRQs or precise timing to change scroll mid-scanline
- Super Mario Bros. 3: Status bar at top, gameplay scrolls below
- Requires scanline-accurate $2005/$2006 writes

**Implementation:**
```rust
fn write_scroll_x(&mut self, value: u8) {
    self.fine_x = value & 0x07;
    self.temp_vram_addr = (self.temp_vram_addr & !0x001F) | ((value as u16) >> 3);

    // If mid-scanline write, adjust current scroll position immediately
    if self.rendering_enabled() && self.scanline < 240 {
        self.current_scroll_x = value;
    }
}

fn write_vram_addr(&mut self, value: u8) {
    if self.write_latch {
        // Second write: low byte
        self.temp_vram_addr = (self.temp_vram_addr & 0xFF00) | value as u16;
        self.vram_addr = self.temp_vram_addr;

        // If mid-scanline write (split-screen effect), update immediately
        if self.rendering_enabled() && self.scanline < 240 {
            self.apply_split_screen_scroll();
        }
    } else {
        // First write: high byte
        self.temp_vram_addr = (self.temp_vram_addr & 0x00FF) | ((value as u16 & 0x3F) << 8);
    }
    self.write_latch = !self.write_latch;
}
```

## Test Cases

| Game | Edge Case | Expected Behavior |
|------|-----------|-------------------|
| **Super Mario Bros. 3** | Status bar split | Clean split between status bar and gameplay |
| **Kirby's Adventure** | Complex scrolling | Smooth scrolling, no glitches |
| **Mega Man 2** | Many sprites | Correct sprite overflow flag |
| **Gradius** | Many bullets | Correct sprite limit enforcement |
| **Battletoads** | Palette cycling | Smooth color transitions |
| **Zelda** | Complex palettes | Accurate colors, no artifacts |
| **Mega Man 3** | Mid-frame palette changes | Correct color updates |

## Test ROMs

| ROM | Focus | Expected Result |
|-----|-------|-----------------|
| ppu_sprite_overflow.nes | Sprite overflow flag | Flag set correctly |
| ppu_palette_ram.nes | Palette mirroring | All edge cases pass |
| ppu_scroll_glitch.nes | Scrolling edge cases | No visual glitches |
| ppu_split_screen.nes | Mid-scanline writes | Clean split |

## Acceptance Criteria

- [ ] Sprite overflow flag 100% accurate
- [ ] Palette RAM writes during rendering handled
- [ ] Split-screen effects working (SMB3, Kirby's Adventure)
- [ ] Attribute handling verified (no regressions)
- [ ] All test ROMs passing
- [ ] Tested with 5+ complex games
- [ ] No visual artifacts or glitches

## Known Issues to Fix

From v0.5.0 implementation report and test failures:

1. **Sprite Overflow Flag** - Not implemented or inaccurate
2. **Palette RAM During Rendering** - Edge cases not handled
3. **Mid-Scanline Writes** - Split-screen effects not working
4. **Attribute Handling** - Edge cases (v0.5.0 fix to verify)

## Debugging Strategy

1. **Identify Visual Glitch:**
   - Take screenshot, compare to reference emulator
   - Note scanline/dot where glitch occurs

2. **Isolate Issue:**
   - Determine if sprite, background, or palette issue
   - Check relevant PPU state at failure point

3. **Trace Execution:**
   - Enable PPU trace logging
   - Log scanline, dot, register writes

4. **Fix & Verify:**
   - Implement fix
   - Test with affected game
   - Run full PPU test suite

## Version Target

v0.8.0
