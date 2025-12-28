# M9 Sprint 2: PPU Edge Cases

## Overview

Resolve PPU edge cases including sprite overflow, palette RAM writes during rendering, scrolling split-screen effects, and mid-scanline register access.

## Current Implementation (v0.7.1)

The desktop frontend includes PPU debug windows implemented in egui:

**Completed:**
- [x] PPU debug window with pattern table viewer
- [x] Nametable viewer (4 nametables with scroll indicator)
- [x] OAM viewer (64 sprites with attributes)
- [x] Palette viewer (background + sprite palettes)
- [x] Basic sprite rendering with 8-sprite-per-scanline limit
- [x] Palette RAM mirroring implemented
- [x] VBlank/NMI timing functional

**Location:** `crates/rustynes-desktop/src/gui/debug.rs`

## Objectives

- [x] Implement accurate sprite overflow flag behavior
- [ ] Handle palette RAM writes during rendering
- [x] Support scrolling split-screen effects (mid-scanline writes)
- [ ] Fix attribute handling edge cases
- [ ] Validate with complex games (Super Mario Bros. 3, Zelda)

## Tasks

### Task 1: Sprite Overflow Flag
- [x] Study hardware sprite evaluation (8 sprite limit per scanline)
- [x] Implement sprite overflow flag logic ($2002 bit 5)
- [x] Handle hardware quirks (overflow flag false positives/negatives)
- [x] Test with unit tests for overflow bug scenarios
- [ ] Validate with games using many sprites (Mega Man, Gradius)

**Implementation Complete (Dec 27, 2025):**
- Hardware-accurate sprite overflow bug implemented in `sprites.rs`
- When secondary OAM is full (8 sprites), PPU enters OverflowCheck mode
- Bug: Both n (sprite index) AND m (byte offset) increment together
- Causes reading wrong bytes (tile/attr/X instead of Y coordinate)
- Results in false positives and false negatives matching real hardware
- 3 new tests added: `test_sprite_overflow_bug_false_positive`, `test_sprite_overflow_bug_byte_offset_increment`, `test_sprite_overflow_bug_false_negative`

### Task 2: Palette RAM Edge Cases
- [ ] Handle writes to palette RAM during rendering
- [ ] Test palette mirroring during rendering ($3F10/$3F14/$3F18/$3F1C)
- [ ] Verify background color updates mid-frame
- [ ] Test with palette cycling effects (Battletoads, Mega Man)
- [ ] Validate color accuracy (compare to hardware screenshots)

### Task 3: Scrolling Split-Screen Effects
- [x] Handle mid-scanline $2006 writes (VRAM address)
- [x] Handle mid-scanline $2005 writes (scroll position)
- [ ] Test with Super Mario Bros. 3 (status bar split)
- [ ] Test with other split-screen games (Kirby's Adventure)
- [x] Verify timing precision (scanline-accurate writes)

**Implementation Complete (Dec 27, 2025):**
- Added mid-scanline tracking to `scroll.rs`:
  - `last_v_before_update: u16` - preserves v before mid-scanline update
  - `mid_scanline_write_detected: bool` - flags mid-scanline writes this frame
  - `start_frame()` - resets detection flag at frame start
  - `record_mid_scanline_write()` - records mid-scanline write event
- Added public getters in `scroll.rs`: `temp_vram_addr()`, `write_toggle()`, `mid_scanline_write_detected()`, `last_v_before_update()`
- Added helper `is_visible_rendering_position()` in `ppu.rs`
- Mid-scanline detection triggers when $2005/$2006 written during:
  - Visible scanlines (0-239)
  - After dot 0 (rendering has started)
  - Rendering enabled
- Added 6 new tests for mid-scanline tracking
- Exposed scroll state via PPU getters: `vram_addr()`, `temp_vram_addr()`, `fine_x()`, `coarse_x()`, `coarse_y()`, `fine_y()`, `mid_scanline_write_detected()`, `last_v_before_update()`

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

- [x] Sprite overflow flag 100% accurate (hardware bug implemented)
- [ ] Palette RAM writes during rendering handled
- [x] Split-screen effects detection working (mid-scanline tracking)
- [ ] Attribute handling verified (no regressions)
- [ ] All test ROMs passing
- [ ] Tested with 5+ complex games
- [ ] No visual artifacts or glitches

## Known Issues to Fix

From v0.5.0 implementation report and test failures:

1. **Sprite Overflow Flag** - ~~Not implemented or inaccurate~~ **FIXED** (Dec 27, 2025)
2. **Palette RAM During Rendering** - Edge cases not handled
3. **Mid-Scanline Writes** - ~~Split-screen effects not working~~ **FIXED** (Dec 27, 2025)
4. **Attribute Handling** - Edge cases (v0.5.0 fix to verify)

## egui Debug Window Integration

### PPU Debug Window Enhancement

The existing egui PPU debug window can be extended for edge case debugging:

```rust
// crates/rustynes-desktop/src/gui/debug.rs
pub fn ppu_debug_window(
    ctx: &egui::Context,
    open: &mut bool,
    console: &Console,
) {
    egui::Window::new("PPU Debug")
        .open(open)
        .default_size([800.0, 600.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut tab, PpuTab::PatternTables, "Pattern Tables");
                ui.selectable_value(&mut tab, PpuTab::Nametables, "Nametables");
                ui.selectable_value(&mut tab, PpuTab::Sprites, "Sprites");
                ui.selectable_value(&mut tab, PpuTab::Palette, "Palette");
                ui.selectable_value(&mut tab, PpuTab::Scanline, "Scanline"); // New tab
            });

            match tab {
                PpuTab::Sprites => render_sprite_debug(ui, console),
                PpuTab::Scanline => render_scanline_debug(ui, console),
                // ...
            }
        });
}

fn render_sprite_debug(ui: &mut egui::Ui, console: &Console) {
    let ppu = console.ppu();

    // Sprite overflow indicator
    ui.horizontal(|ui| {
        ui.label("Sprite Overflow:");
        let overflow = ppu.status() & 0x20 != 0;
        ui.colored_label(
            if overflow { egui::Color32::RED } else { egui::Color32::GREEN },
            if overflow { "SET" } else { "CLEAR" }
        );
    });

    // Current scanline sprite count
    ui.label(format!("Sprites on scanline {}: {}",
        ppu.scanline(), ppu.sprites_on_current_scanline()));

    // OAM table with visual highlighting for sprites on current scanline
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("oam_grid").show(ui, |ui| {
            for i in 0..64 {
                let sprite = ppu.oam_sprite(i);
                let on_scanline = sprite.is_on_scanline(ppu.scanline());
                ui.colored_label(
                    if on_scanline { egui::Color32::YELLOW } else { egui::Color32::WHITE },
                    format!("{}: Y={} X={} Tile={:02X}",
                        i, sprite.y, sprite.x, sprite.tile)
                );
                if (i + 1) % 4 == 0 { ui.end_row(); }
            }
        });
    });
}

fn render_scanline_debug(ui: &mut egui::Ui, console: &Console) {
    let ppu = console.ppu();

    // Current rendering state
    ui.label(format!("Scanline: {} / Dot: {}", ppu.scanline(), ppu.dot()));

    // VRAM address visualization
    ui.label(format!("VRAM Addr: ${:04X} (t=${:04X})",
        ppu.vram_addr(), ppu.temp_vram_addr()));

    // Scroll position visualization
    ui.label(format!("Fine X: {} | Coarse X: {} | Coarse Y: {}",
        ppu.fine_x(), ppu.coarse_x(), ppu.coarse_y()));

    // Split-screen detection
    if ppu.mid_scanline_write_detected() {
        ui.colored_label(egui::Color32::YELLOW, "Mid-scanline write detected!");
    }
}
```

### Palette RAM Debug Visualization

```rust
fn render_palette_debug(ui: &mut egui::Ui, console: &Console) {
    let ppu = console.ppu();

    ui.heading("Palette RAM");

    // Background palettes
    ui.label("Background:");
    ui.horizontal(|ui| {
        for i in 0..16 {
            let color = ppu.palette_ram(i);
            let rgb = NES_PALETTE[color as usize];
            let rect = ui.allocate_space(egui::vec2(24.0, 24.0));
            ui.painter().rect_filled(rect.1, 0.0, rgb_to_color32(rgb));

            // Highlight mirrored addresses
            if i == 0 || i == 4 || i == 8 || i == 12 {
                ui.painter().rect_stroke(rect.1, 0.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE));
            }
        }
    });

    // Sprite palettes
    ui.label("Sprites:");
    ui.horizontal(|ui| {
        for i in 16..32 {
            let color = ppu.palette_ram(i);
            let rgb = NES_PALETTE[color as usize];
            let rect = ui.allocate_space(egui::vec2(24.0, 24.0));
            ui.painter().rect_filled(rect.1, 0.0, rgb_to_color32(rgb));

            // Highlight mirrored addresses ($3F10, $3F14, $3F18, $3F1C)
            if i == 16 || i == 20 || i == 24 || i == 28 {
                ui.painter().rect_stroke(rect.1, 0.0,
                    egui::Stroke::new(2.0, egui::Color32::YELLOW));
            }
        }
    });

    // Live palette write monitoring
    if let Some(write) = ppu.last_palette_write() {
        ui.colored_label(egui::Color32::YELLOW,
            format!("Last write: ${:04X} = ${:02X}", write.addr, write.value));
    }
}
```

## Debugging Strategy

1. **Identify Visual Glitch:**
   - Take screenshot, compare to reference emulator
   - Note scanline/dot where glitch occurs
   - Use egui PPU debug window to inspect state

2. **Isolate Issue:**
   - Determine if sprite, background, or palette issue
   - Check relevant PPU state at failure point
   - Use Scanline tab to monitor mid-scanline writes

3. **Trace Execution:**
   - Enable PPU trace logging
   - Log scanline, dot, register writes
   - Monitor sprite overflow flag in debug window

4. **Fix & Verify:**
   - Implement fix
   - Test with affected game
   - Run full PPU test suite
   - Verify fix in egui debug window

## Version Target

v0.8.0
