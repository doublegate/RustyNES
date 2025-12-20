# M7 Sprint 2: PPU Accuracy

## Overview

Improve PPU dot-accurate rendering to achieve ±2 cycle VBlank timing precision and handle sprite 0 hit edge cases.

## Objectives

- [ ] Achieve VBlank timing precision (±51 cycle → ±2 cycle)
- [ ] Fix sprite 0 hit edge cases
- [ ] Verify attribute shift register behavior
- [ ] Handle palette RAM mirroring edge cases
- [ ] Improve scanline/dot timing accuracy

## Tasks

### Task 1: VBlank Timing Precision
- [ ] Study ppu_02-vbl_set_time.nes requirements (±51 cycle → ±2 cycle)
- [ ] Study ppu_03-vbl_clear_time.nes requirements (±10 cycle → exact)
- [ ] Implement precise VBlank flag set timing (scanline 241, dot 1)
- [ ] Implement precise VBlank flag clear timing
- [ ] Test with ppu_vbl_nmi test suite

### Task 2: Sprite 0 Hit Edge Cases
- [ ] Test sprite 0 hit with ppu_spr_hit_* suite (11 tests)
- [ ] Handle corner cases (ppu_03.corners.nes)
- [ ] Verify flip behavior (ppu_04.flip.nes)
- [ ] Test edge timing (ppu_spr_hit_edge_timing.nes)
- [ ] Validate timing order (ppu_spr_hit_timing_order.nes)

### Task 3: Attribute Handling Verification
- [ ] Verify attribute shift register behavior (fixed in v0.5.0)
- [ ] Test attribute byte extraction for all quadrants
- [ ] Validate shift register reload timing
- [ ] Test with various background patterns

### Task 4: Palette RAM Mirroring
- [ ] Test palette mirroring at $3F10, $3F14, $3F18, $3F1C
- [ ] Verify background color mirroring
- [ ] Test with ppu_palette_ram.nes
- [ ] Handle edge cases in palette writes

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| ppu_01-vbl_basics.nes | ✅ Pass | Already passing |
| ppu_02-vbl_set_time.nes | ⏸️ Ignored | ±51 cycle → target ±2 cycle |
| ppu_03-vbl_clear_time.nes | ⏸️ Ignored | ±10 cycle → target exact |
| ppu_04-nmi_control.nes | [ ] Pending | NMI control timing |
| ppu_05-nmi_timing.nes | [ ] Pending | NMI timing precision |
| ppu_01.basics.nes | ✅ Pass | Sprite 0 hit basics (passing) |
| ppu_02.alignment.nes | ✅ Pass | Sprite 0 hit alignment (passing) |
| ppu_03.corners.nes | [ ] Pending | Sprite 0 hit corners |
| ppu_04.flip.nes | [ ] Pending | Sprite 0 hit flip |

## Acceptance Criteria

- [ ] ppu_02-vbl_set_time.nes passes (±2 cycle accuracy)
- [ ] ppu_03-vbl_clear_time.nes passes (exact timing)
- [ ] All sprite 0 hit tests pass (11/11)
- [ ] Attribute handling verified (no regressions)
- [ ] Palette mirroring edge cases handled
- [ ] No visual regressions in Super Mario Bros.

## Version Target

v0.6.0
