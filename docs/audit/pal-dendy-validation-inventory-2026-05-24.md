# PAL / Dendy validation inventory (T-71-006, Phase 7 Sprint 1)

**Date:** 2026-05-24
**Purpose:** identify what can validate PAL (2C07) and Dendy timing **without
reusing NTSC expectations**, feeding the automated gates in Sprint 3
(T-73-005 PAL, T-73-006 Dendy).

## What the core already models (region-correct)

All in `crates/nes-ppu/src/ppu.rs` (`PpuRegion`) and `crates/nes-core/src/bus.rs`:

| Behavior | NTSC | PAL | Dendy | Code |
|---|---|---|---|---|
| Scanlines / frame | 262 | 312 | 312 | `PpuRegion::prerender_line` (261 / 311 / 311) |
| Pre-render line | 261 | 311 | 311 | same |
| VBL start line | 241 | 241 | **291** | `PpuRegion::vblank_start_line` |
| Last visible line | 239 | 239 | 239 | `PpuRegion::last_visible_line` |
| Post-reset write-mask | 29,658 | 33,132 | 33,132 | `PpuRegion::post_reset_mask_cycles` |
| Odd-frame dot-skip | yes | **no** | **no** | `advance_dot` (`region == Ntsc` guard) |
| Wall-clock frame duration | 16.639 ms | 19.997 ms | 19.997 ms | `nes_core::FRAME_DURATION_*` |
| APU frame-counter region | NTSC | PAL | Dendy | `nes_apu::ApuRegion` |

The cartridge header region flows through `nes_mappers::Region` →
`PpuRegion` / `ApuRegion` in `LockstepBus::new` (bus.rs ~267-274).

## Known limitation (deferred to v2.0)

The CPU:PPU clock ratio is **hardcoded 3:1** (`for sub_dot in 0..3` in
`bus.rs` ~921/938). True PAL hardware is **3.2:1** (the 2C07 PPU runs 3.2 dots
per CPU cycle; see the bus.rs comment at ~845 "PAL would be 3.2"). The integer
3:1 model means PAL *frame structure* (312 lines, VBL@241, no odd-frame skip)
is correct, but PAL CPU timing *relative to the PPU* is slightly fast. Closing
this requires fractional master-clock scheduling — the v2.0 refactor handles
3.2:1 naturally (12 master-clocks/CPU-cycle NTSC vs 16 PAL). Dendy uses
NTSC-style CPU timing (3:1), so Dendy CPU:PPU cadence is already exact.

## Validation approach (Sprint 3)

No region-specific commercial ROM is required (and none would be committable).
The region constants are deterministic and the frame structure is observable
from the public `Nes` API, so the gates are **construct-and-assert**:

1. **`PpuRegion` constant unit tests** — assert the scanline/VBL/mask/skip
   table above per region (pins the table against accidental edits).
2. **PAL frame-structure integration test** — build a PAL-region `Nes` (PAL
   iNES/NES 2.0 header or region override), run N frames, assert: 312-line
   cadence (frame cycle count consistent with no odd-frame skip — PAL frames
   never vary in length), VBL flag sets at scanline 241, the post-reset mask
   window lasts 33,132 cycles.
3. **Dendy frame-structure integration test** — same, but VBL@291 and 312
   lines with NTSC-style CPU cadence.
4. **Existing PAL coverage retained** — the sacred-trio gate already asserts
   **SMB / Excitebike / Kid Icarus run in PAL and stay legible**
   (`scripts/regression-bisect/`), which is the real-game PAL canary. The new
   gates add structural-timing assertions on top.

The 3.2:1 CPU:PPU ratio gap is asserted as a documented `// DEFERRED (v2.0)`
note in the PAL test, not as a failing expectation.

## References

- nesdev wiki: [PPU frame timing](https://www.nesdev.org/wiki/PPU_frame_timing),
  [Clock rate](https://www.nesdev.org/wiki/Clock_rate),
  [Cycle reference chart](https://www.nesdev.org/wiki/Cycle_reference_chart).
- Local: `ref-docs/nesdev-wiki-technical-report.md` (region timing section).
