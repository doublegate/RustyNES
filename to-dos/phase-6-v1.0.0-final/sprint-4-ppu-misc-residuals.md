# Sprint 4 — PPU misc residuals (6 tests)

**Phase:** 6 — v1.0.0 final
**Status:** **INVESTIGATION-ONLY** (Session 28, 2026-05-23). All 6
candidates analysed; 0 of 6 landed. 1 candidate (`$2004 Stress`) is
on Sprint 3's deferred eval-base-from-OAMADDR axis. The remaining 5
require net-new state machines OR per-PPU-clock analog modeling
beyond current architecture, with EXTREME cascade risk into the
commercial-ROM oracle (Session-8 Cascade A re-baselined surface)
and / or the sprite-eval FSM (B8b regression surface). Permanent
regression-prevention infrastructure landed: 6 custom AccuracyCoin
sub-test ROMs at `tests/roms/AccuracyCoin/sub-tests/ppu-misc-*.nes`.
**Cascade risk:** **EXTREME** — PPU dot-precise modeling shares surface
with sprite-eval (Sprint 3) and the C1 PPU axis (Sprint 5); BG-pipeline
fixes risk re-baseline of the 60-ROM commercial oracle FNV-1a hashes.

## Target tests (6)

Per `crates/rustynes-test-harness/tests/accuracycoin.rs` diagnostic output:

- `PPU Misc :: Stale BG/Sprite Shift Regs`
- `PPU Misc :: BG Serial In`
- `PPU Misc :: Sprites On Scanline 0`
- `PPU Misc :: $2004 Stress Test`
- `PPU Misc :: $2007 Stress Test`
- `PPU Misc :: Rendering Flag Behavior` (per `docs/STATUS.md` v1.0.0
  residual table)

Estimated yield: **+1 to +3 AccuracyCoin tests** (some may share root
cause; verify per-test).

## Hypothesis (per-test)

### `Stale BG/Sprite Shift Regs`

When rendering is disabled mid-frame (`$2001` BG/sprite enable bits
cleared), the BG and sprite shift registers should retain their
last-shifted values rather than zeroing. RustyNES may be zeroing them
on disable transitions. Cross-reference Mesen2's
`Core/NES/NesPpu.cpp` rendering-disable path.

### `BG Serial In`

BG shift register serial-in timing during the cycle-9 reload. The
session-8 fix (commit `086ce4d`) shifted the reload point to canonical
cycle 9; this test may exercise a residual sub-cycle behavior of the
serial-in latch.

### `Sprites On Scanline 0`

Sprite-zero (and all sprites) rendering on scanline 0 is a special
case: per nesdev, the first visible scanline behaves slightly
differently from subsequent scanlines (sprite fetches from pre-render
scanline 261 inform scanline 0 rendering).

### `$2004 Stress Test`

OAM read/write stress via `$2004`. Session-7 commit `c230489` (`$4`-
aligned `$2004` write) closed `Address $2004 behavior` but this
sub-test exercises stress patterns that may reveal additional
sub-cycle bugs.

### `$2007 Stress Test`

PPUDATA read/write stress. The `$2007` buffer state machine has known
quirks during rendering (the session-8 fix flipped `$2007 read w/
rendering` already; this is the residual stress sub-test).

### `Rendering Flag Behavior`

`$2001` flag changes mid-frame. The 2-PPU-clock PPUMASK → dot-skip
pipeline delay is implemented; this test may exercise the sprite
visibility or BG-left-8-pixel sub-flag behavior at sub-cycle precision.

## Sprint plan

### Step 1 — Per-PPU-dot trace tooling (prerequisite)

Session-10 landed `crates/rustynes-ppu/src/state_trace.rs` and the
`ppu-state-trace` feature flag for PPU dot-precise tracing. Session-18
deferred extending this; this sprint may need it.

If a per-test investigation reveals dot-precise divergence:
1. Enable `ppu-state-trace`.
2. Run the failing AccuracyCoin sub-test and capture the RustyNES
   per-PPU-dot trace.
3. Run the same sequence in Mesen2 with `scripts/mesen2_ppu_trace.lua`
   (extend Session-15's `mesen2_irq_trace.lua` template).
4. Cross-diff and locate the load-bearing divergence dot.

### Step 2 — Per-test investigation + fix

For each test, in order:
1. Add a unit test in `crates/rustynes-ppu/tests/` reproducing the bug
   deterministically.
2. Bisect candidate code paths (similar to Cascade A methodology).
3. Implement surgical fix under feature flag
   `accuracycoin-ppu-misc-<test-name>`.
4. Run validation gauntlet.
5. Land OR roll back.

### Step 3 — Validation gauntlet

Standard 10-gate gauntlet. Special attention:
- `ppu_vbl_nmi/*` (10 strict): the broad PPU regression sentinel.
  Must remain 10/10.
- `sprite_hit_tests/*` (11 strict).
- `sprite_overflow_tests/*` (5 strict).
- `vbl_race_window_2002_read_sweep` PPU unit test (Session-18 oracle).
  Must remain strict.

## Cascade-risk callouts

1. **PPU dot-precise modeling has the same cascade risk as Sprint 3
   (sacred-trio). Run `scripts/regression-bisect/bisect-real-games.sh`
   after every fix.**
2. The Session-18 `$2002` race-window unit test is the permanent
   regression oracle. Do NOT introduce a fix that breaks it (e.g., by
   shifting VBL set timing).
3. Some PPU misc fixes may interact with the C1 axis (Sprint 5).
   Specifically: anything that shifts when `$2002` returns VBL=1 will
   cascade into `cpu_interrupts_v2/{2,3}`. Run the C1 trace cross-diff
   (`scripts/irq_trace_cross_diff.py`) after each fix.

## Estimated effort + yield

- **Effort:** 1-3 days per test (6 tests × 1-3 days; some may share
  root cause and close together).
- **Yield:** +1 to +3 AccuracyCoin tests.

## References

- nesdev `PPU rendering` page (BG fetch + sprite fetch dot-precise
  pipeline)
- nesdev `PPU registers` page (`$2001` / `$2004` / `$2007` semantics
  during rendering)
- Mesen2 `Core/NES/NesPpu.cpp` (`ProcessSpriteEvaluation`,
  `LoadShifters`, `IncHorizontalScrolling`, `IncVerticalScrolling`)
- `crates/rustynes-ppu/src/ppu.rs` (RustyNES PPU implementation)
- `crates/rustynes-ppu/src/state_trace.rs` (Session-10 dot-precise trace
  infrastructure; gated on `ppu-state-trace` feature)
- `docs/adr/0005-*.md` (PPU state trace ADR if it exists per
  `docs/STATUS.md` references)

## Exit criterion

- AccuracyCoin pass rate increases (target +1 to +3 tests).
- No regressions in any of the 10 validation gauntlet gates.
- `vbl_race_window_2002_read_sweep` permanent oracle preserved.
- If pass rate reaches ≥ 90%, jump to v1.0.0 final tag. Otherwise
  proceed to Sprint 5.

## Session 28 outcome (2026-05-23)

**INVESTIGATION-ONLY.** No chip-stack code change. Pass rate
unchanged at 84.17%. Workspace unchanged at 545 strict + 5 ignored.
Commercial-ROM oracle 60/60 (no FNV-1a deltas). See
`docs/audit/session-28-sprint4-ppu-misc-residuals-2026-05-23.md`
for the per-test cascade analysis. The 6 custom AccuracyCoin
sub-test ROMs at `tests/roms/AccuracyCoin/sub-tests/ppu-misc-*.nes`
are permanent regression-prevention infrastructure for any future
PPU misc fix attempt (each reaches the target test by frame ≤ 400;
most ≤ 92).

Per-target cascade rationale:
- `Stale BG Shift Registers` Test 3 — requires net-new BG shifter
  retention + unused-NT-read state; EXTREME cascade into 60-ROM
  oracle (same surface Cascade A re-baselined under explicit user
  authorization at Session 8).
- `Stale Sprite Shift Regs` Test 3 — net-new per-sprite counter
  mode (`Halted`/`Counting`); touches FSM mid-scanline-write surface
  (B8b regression `63d8dea` lesson).
- `BG Serial In` Test 2 — net-new BG shifter serial-in + per-CPU-
  cycle PPUMASK pipelining; PPUMASK delay change cascades into
  blargg `ppu_vbl_nmi/*` 10/10 strict surface.
- `Sprites On Scanline 0` Test 2 — net-new pre-render-as-scanline-5
  in-range check + Composite vs RGB PPU config dimension + dot-340-
  odd-skip-shifter effect; touches FSM B8b surface.
- `$2004 Stress` Test 2 — **Sprint 3 deferred eval-base-from-
  OAMADDR axis** (Session 9 14-test cascade documented per
  Session 27).
- `$2007 Stress` Test 2 — net-new PPU DATA state machine with
  sub-PPU-clock granularity ALE-vs-Read analog feedback; even
  Mesen2 may not pass cleanly per upstream comment "honestly a
  real shame half the bytes are affected by analogue behavior."

Next: per `sprint-gate-conditions.md` rule 4 (pass rate 84.17% in
83-87% band), proceed to **Sprint 5** (C1 IRQ-timing rework).
