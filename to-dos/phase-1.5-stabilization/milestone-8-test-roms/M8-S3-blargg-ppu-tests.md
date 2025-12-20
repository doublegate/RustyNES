# M8 Sprint 3: Blargg PPU Tests

## Overview

Systematically pass the Blargg PPU test suite (49 tests) to validate VBlank/NMI timing, sprite 0 hit, palette RAM, and PPU rendering behavior.

## Objectives

- [ ] Pass 47/49 PPU tests (96%)
- [ ] Validate VBlank/NMI timing precision
- [ ] Handle sprite 0 hit edge cases
- [ ] Verify palette RAM mirroring
- [ ] Test PPU open bus behavior
- [ ] Ensure scrolling edge cases work

## Tasks

### Task 1: VBlank/NMI Tests (10 tests)
- [ ] Run ppu_vbl_nmi/ppu_vbl_nmi.nes (comprehensive VBL/NMI test)
- [ ] Test 01-vbl_basics.nes (VBlank flag basics) - **Already passing**
- [ ] Test 02-vbl_set_time.nes (VBlank flag set timing ±2 cycle)
- [ ] Test 03-vbl_clear_time.nes (VBlank flag clear timing exact)
- [ ] Test 04-nmi_control.nes (NMI enable/disable control)
- [ ] Test 05-nmi_timing.nes (NMI timing precision)
- [ ] Test 06-suppression.nes (NMI suppression edge cases)
- [ ] Test 07-nmi_on_timing.nes (NMI enable timing)
- [ ] Test 08-nmi_off_timing.nes (NMI disable timing)
- [ ] Test 09-even_odd_frames.nes (Frame timing odd/even)
- [ ] Test 10-even_odd_timing.nes (Odd frame skip timing)

### Task 2: Sprite 0 Hit Tests (11 tests)
- [ ] Run ppu_sprite_hit/ppu_sprite_hit.nes (comprehensive sprite 0 test)
- [ ] Test 01-basics.nes (Sprite 0 hit basics) - **Already passing**
- [ ] Test 02-alignment.nes (Sprite 0 hit alignment) - **Already passing**
- [ ] Test 03-corners.nes (Sprite 0 hit corners)
- [ ] Test 04-flip.nes (Sprite 0 hit flip behavior)
- [ ] Test 05-left_clip.nes (Sprite 0 hit left clipping)
- [ ] Test 06-right_edge.nes (Sprite 0 hit right edge)
- [ ] Test 07-screen_bottom.nes (Sprite 0 hit screen bottom)
- [ ] Test 08-double_height.nes (Sprite 0 hit 8x16 sprites)
- [ ] Test 09-timing.nes (Sprite 0 hit timing precision)
- [ ] Test 10-timing_order.nes (Sprite 0 hit timing order)
- [ ] Test 11-edge_timing.nes (Sprite 0 hit edge timing)

### Task 3: Palette RAM Tests (5 tests)
- [ ] Run ppu_palette_ram/ppu_palette_ram.nes (palette RAM mirroring)
- [ ] Test sprite palette mirroring ($3F10, $3F14, $3F18, $3F1C → $3F00, $3F04, $3F08, $3F0C)
- [ ] Test background color mirroring ($3F00 mirrored at $3F10)
- [ ] Verify palette write behavior
- [ ] Test palette read edge cases

### Task 4: Open Bus Tests (3 tests)
- [ ] Run ppu_open_bus/ppu_open_bus.nes (PPU open bus behavior)
- [ ] Test $2000-$2007 open bus behavior
- [ ] Test $2002 VBlank flag read (clear after read)
- [ ] Verify $2004 OAM read behavior
- [ ] Test $2007 VRAM read buffer behavior

### Task 5: Rendering Edge Cases (20 tests)
- [ ] Test sprite overflow flag behavior
- [ ] Test sprite priority (front/back)
- [ ] Test background rendering edge cases
- [ ] Verify scrolling split-screen effects
- [ ] Test mid-scanline $2006 writes
- [ ] Validate attribute table handling
- [ ] Test fine X scroll edge cases
- [ ] Verify CHR bank switching during rendering

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| ppu_vbl_nmi/01-vbl_basics.nes | ✅ Pass | Already passing |
| ppu_vbl_nmi/02-vbl_set_time.nes | [ ] Pending | ±2 cycle timing |
| ppu_vbl_nmi/03-vbl_clear_time.nes | [ ] Pending | Exact timing |
| ppu_vbl_nmi/04-nmi_control.nes | [ ] Pending | NMI control |
| ppu_vbl_nmi/05-nmi_timing.nes | [ ] Pending | NMI timing |
| ppu_vbl_nmi/06-suppression.nes | [ ] Pending | NMI suppression |
| ppu_vbl_nmi/07-nmi_on_timing.nes | [ ] Pending | NMI enable timing |
| ppu_vbl_nmi/08-nmi_off_timing.nes | [ ] Pending | NMI disable timing |
| ppu_vbl_nmi/09-even_odd_frames.nes | [ ] Pending | Frame timing |
| ppu_vbl_nmi/10-even_odd_timing.nes | [ ] Pending | Odd frame skip |
| ppu_sprite_hit/01-basics.nes | ✅ Pass | Already passing |
| ppu_sprite_hit/02-alignment.nes | ✅ Pass | Already passing |
| ppu_sprite_hit/03-corners.nes | [ ] Pending | Corner cases |
| ppu_sprite_hit/04-flip.nes | [ ] Pending | Flip behavior |
| ppu_sprite_hit/05-left_clip.nes | [ ] Pending | Left clipping |
| ppu_sprite_hit/06-right_edge.nes | [ ] Pending | Right edge |
| ppu_sprite_hit/07-screen_bottom.nes | [ ] Pending | Screen bottom |
| ppu_sprite_hit/08-double_height.nes | [ ] Pending | 8x16 sprites |
| ppu_sprite_hit/09-timing.nes | [ ] Pending | Timing precision |
| ppu_sprite_hit/10-timing_order.nes | [ ] Pending | Timing order |
| ppu_sprite_hit/11-edge_timing.nes | [ ] Pending | Edge timing |
| ppu_palette_ram/ppu_palette_ram.nes | [ ] Pending | Palette mirroring |
| ppu_open_bus/ppu_open_bus.nes | [ ] Pending | Open bus behavior |

**Additional PPU Tests (20+ ROMs):**
- ppu_sprite_overflow/ (sprite overflow flag)
- ppu_read_buffer/ (VRAM read buffer $2007)
- ppu_scroll/ (scrolling edge cases)
- ppu_misc/ (miscellaneous PPU behavior)

## Acceptance Criteria

- [ ] 47/49 PPU tests passing (96%)
- [ ] VBlank flag timing accurate (±2 cycle)
- [ ] NMI timing precise (exact cycle)
- [ ] Sprite 0 hit edge cases handled (9/11 passing)
- [ ] Palette RAM mirroring correct
- [ ] Open bus behavior verified
- [ ] Zero regressions from v0.6.0 baseline
- [ ] Scrolling edge cases working

## Expected Failures (2 tests)

**Highly timing-sensitive tests:**
- ppu_02-vbl_set_time.nes - Requires ±1 cycle precision (currently ±2)
- ppu_sprite_hit/11-edge_timing.nes - Sub-dot precision required

**Rationale:** These represent <5% of PPU tests and require sub-cycle/sub-dot precision beyond Phase 1.5 scope.

## Debugging Strategy

1. **Identify Failure:**
   - Run test ROM, capture output code
   - Cross-reference with test source/documentation

2. **Isolate Issue:**
   - Determine which timing/behavior failing
   - Review PPU implementation (dot-accurate rendering)

3. **Trace Execution:**
   - Enable PPU trace logging
   - Log scanline, dot, cycle at failure point

4. **Fix & Verify:**
   - Implement fix (adjust timing or behavior)
   - Verify no regressions in other tests
   - Run full PPU test suite

## Version Target

v0.7.0
