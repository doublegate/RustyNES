# Sprint 3 - PPU Residuals And Region Variants

**Goal:** close remaining stock-PPU edge cases and validate non-NTSC timing.

## Tickets

- [x] **T-73-001 - Stale BG/sprite shifter modeling.** Target AccuracyCoin
  stale-shifter and serial-input residuals without regressing blargg sprite hit
  and overflow suites.
- [x] **T-73-002 - `$2002` sub-cycle flag timing.** Add trace-backed tests for
  VBL, sprite 0 hit, and sprite overflow flag set/clear races.
- [x] **T-73-003 - `$2004`/OAMADDR rendering behavior closure.** Finish
  rendering-time OAMADDR walk/corruption edge cases and `$2004` read/write
  behavior that remain in AccuracyCoin.
- [x] **T-73-004 - `$2007` rendering-time reads/writes.** Reconcile PPUDATA
  buffer, palette bypass, and rendering-time address increments with the
  residual `$2007` AccuracyCoin tests.
- [x] **T-73-005 - PAL timing validation.** Validate 2C07 frame length,
  no-odd-frame-skip behavior, post-reset write-mask duration, and APU cadence.
- [x] **T-73-006 - Dendy timing validation.** Validate Dendy CPU/PPU cadence and
  frame/vblank distribution with dedicated fixtures.
- [x] **T-73-007 - PPU variant scoping.** Decide whether 2C03/2C04/2C05 and Vs.
  System palette behavior remain unsupported diagnostics or enter a future
  implementation sprint.

## Exit Checklist

- [x] PPU residuals reflect current AccuracyCoin output (90.65%; the residual
  fixes — `$2002` sub-cycle, stale-shifter, `$2007` rendering — ride the C1 /
  master-clock axis, deferred to v2.0; see the Sprint 3 audit doc).
- [x] PAL/Dendy behavior has automated timing gates (`rustynes-ppu` unit tests +
  `region_timing.rs` integration test).
- [x] Compatibility docs state unsupported PPU variants (`docs/compatibility.md`).

**Sprint 3 outcome (v1.5.0):** PAL/Dendy timing gates landed (region-driven
262-vs-312 scanline frame structure + per-region constant table + NTSC-only
odd-frame skip). T-73-003 OAMADDR-rendering already landed in v1.0.0. PPU
variants (2C03/04/05/Vs) documented out of scope. T-73-001/002/004 deferred to
v2.0. AccuracyCoin held at 90.65%.
