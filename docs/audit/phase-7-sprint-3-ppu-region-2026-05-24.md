# Phase 7 Sprint 3 — PPU residuals and region variants

**Date:** 2026-05-24 (v1.5.0)
**Scope:** close the *completable* PPU/region items (region timing validation,
PPU-variant scoping); document the AccuracyCoin PPU residuals that ride the C1 /
master-clock axis as deferred to v2.0. Additive only — AccuracyCoin 90.65%,
oracle / sacred trio / B4 byte-identical.

## Ticket disposition

### T-73-005 — PAL timing validation — **DONE**
### T-73-006 — Dendy timing validation — **DONE**

Region timing is now gated two ways:

- **`nes-ppu` unit test** `ppu_region_constants_match_hardware` pins the
  per-region table (NTSC 262/VBL@241, PAL 312/VBL@241, Dendy 312/VBL@291,
  post-reset mask 29,658 NTSC / 33,132 PAL+Dendy), and
  `odd_frame_dot_skip_is_ntsc_only` proves the pre-render dot-skip fires only on
  NTSC.
- **`region_timing.rs` integration test** builds NES 2.0 ROMs with region byte
  12 and asserts the frame *structure* through the public `Nes` API: NTSC ≈
  29,780 CPU cycles/frame, PAL/Dendy ≈ 35,464 (the 312-vs-262 scanline count),
  averaged over 64 frames to absorb instruction-overshoot slop.

**Known limitation (documented, deferred to v2.0):** the CPU:PPU clock ratio is
hardcoded 3:1; PAL hardware is 3.2:1. PAL frame *structure* (312 lines, VBL@241,
no odd-frame skip) is correct, but PAL CPU timing *relative to the PPU* is
slightly fast. The fractional ratio is naturally handled by the v2.0
master-clock refactor (12 master-clocks/CPU-cycle NTSC vs 16 PAL). See
`docs/audit/pal-dendy-validation-inventory-2026-05-24.md`. The sacred-trio gate
already exercises SMB/Excitebike/Kid Icarus in PAL as the real-game canary.

### T-73-003 — `$2004`/OAMADDR rendering closure — **DONE (landed in v1.0.0); residuals DEFERRED**

OAMADDR reset during dots 257-320 and OAMADDR-walks-during-eval landed in
v1.0.0 (commits `f29f7ca`, `c230489`). The residual AccuracyCoin sprite-eval
cases (`Misaligned OAM behavior`, `OAM Corruption`, `Arbitrary Sprite zero`) are
on the Cascade-A sub-cycle axis — see below.

### T-73-001 — Stale BG/sprite shifter modeling — **DEFERRED to v2.0**
### T-73-002 — `$2002` sub-cycle flag timing — **DEFERRED to v2.0**
### T-73-004 — `$2007` rendering-time reads/writes — **DEFERRED to v2.0**

These three are the remaining AccuracyCoin PPU residuals (Stale BG/Sprite Shift
Regs, BG Serial In, Sprites On Scanline 0, `$2004`/`$2007` Stress, `$2002` flag
timing, Arbitrary Sprite zero, Misaligned OAM, OAM Corruption). Session-27/29
established that `$2002` flag timing rides the same per-cycle CPU↔PPU phase
relationship as the C1 axis and flips with the master-clock refactor; the
stale-shifter / `$2007`-rendering cases are the Cascade-A sub-cycle residuals
that the v1.0.0 BG-pipeline fix (`086ce4d`) left after closing the geometric
root cause. Surgically chasing them risks the green-but-wrong oracle re-baseline
trap (the v1.0.0→v1.3.0 left-column regression). They are deferred to v2.0 and
remain visible in the per-CI-run AccuracyCoin diagnostic. **No re-baseline is
performed in Phase 7.**

### T-73-007 — PPU variant scoping — **DONE (doc)**

2C03 / 2C04 / 2C05 (RGB PPUs) and Vs. System palette behavior are **out of
scope** and remain load-time diagnostics, not implemented features. They use
different PPU ICs with hardware palettes and (2C05) a swapped `$2000`/`$2001`
register order — a separate platform initiative, not a stock-2C02 accuracy item.
Recorded in `docs/compatibility.md`.

## Exit-checklist status

- PPU residuals in `docs/STATUS.md` reflect current AccuracyCoin output
  (unchanged at 90.65%; the residual fixes are v2.0).
- PAL/Dendy each have at least one automated timing gate (unit + integration).
- `docs/compatibility.md` states the unsupported PPU variants.
