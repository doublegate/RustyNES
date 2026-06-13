# Sprint 3 — Sprite-eval residuals (4 tests)

**Phase:** 6 — v1.0.0 final
**Status:** **INVESTIGATION-ONLY** (Session 27, 2026-05-23). 0 of 4
targets landed; 4 of 4 deferred to v1.x with documented prior-cascade
evidence. Pass rate unchanged at 84.17%. Permanent regression-
prevention infrastructure (4 custom sub-test ROMs) landed.
**Cascade risk:** **HIGH** — sprite-eval FSM is the surface that
produced the May-2026 sacred-trio regression (B8b first-bad `63d8dea`,
fixed by `834be9e`). Any change here has commercial-ROM oracle
implications.

## Target tests (4)

- `Sprite Evaluation :: $2002 flag timing`
- `Sprite Evaluation :: Arbitrary Sprite zero`
- `Sprite Evaluation :: Misaligned OAM behavior`
- `Sprite Evaluation :: OAM Corruption`

These are post-BG-pipeline-fix residuals (session-8 `086ce4d` closed
the geometric `VerifySpriteZeroHits` step-2 puzzle; these 4 are the
sub-residuals that remain). Each exercises a specific corner of the
sprite-eval FSM.

Estimated yield: **+1 to +4 AccuracyCoin tests** (each likely 1 fix
per test, surgical).

## Hypothesis (per-test)

### `$2002 flag timing`

Sprite-zero-hit flag-clear timing on `$2002` reads. Likely the flag
clear happens 1 PPU dot off. Cross-reference with Mesen2's
`Core/NES/NesPpu.cpp` `ReadRegister(2)` path.

### `Arbitrary Sprite zero`

Per the AccuracyCoin upstream catalog, this exercises sprite-zero
detection when sprite 0 is at unusual OAM positions (not in slot 0
after OAM DMA). Sprite-eval FSM's slot-tracking for sprite 0 needs to
match: any sprite that *would-have-been* sprite 0 if OAM hadn't been
rotated must still be eligible for the sprite-zero-hit predicate.

### `Misaligned OAM behavior`

OAM access at non-`$4`-aligned byte offsets. OAMADDR can be any value,
and `$2003 → $2004` reads/writes walk in unusual patterns. Per
nesdev: misaligned OAMADDR causes the sprite-eval FSM to read
out-of-order quartets.

### `OAM Corruption`

Per nesdev: OAM contents corrupt when CPU writes to `$2003 / $2004`
during specific PPU dots of rendering. The corruption pattern depends
on the dot the write lands on (dots 1-64 vs 65-256 vs 257-320 differ).
RustyNES currently models the OAMADDR-during-rendering reset (Cascade
A `f29f7ca`) but not the full corruption-pattern table.

## Sprint plan

### Step 1 — Per-test PPU unit test reproducer

Before any production code change, add 4 unit tests in
`crates/rustynes-ppu/tests/sprite_eval.rs` (new file). Each test
deterministically constructs the AccuracyCoin scenario at the FSM-step
level (bypassing the test ROM + CPU) and asserts the failing condition.

Template: see Sprint 6-1's Cascade A `Step 1` pattern (in
`to-dos/phase-6-v1-closeout/sprint-6-1-cascade-a-c-b-c1.md`).

### Step 2 — Per-test feature flag

Each fix lands under its own feature flag:
- `accuracycoin-sprite-eval-2002-flag-timing`
- `accuracycoin-sprite-eval-arbitrary-sprite-zero`
- `accuracycoin-sprite-eval-misaligned-oam`
- `accuracycoin-sprite-eval-oam-corruption`

This prevents one fix from cascading into another's regression analysis.

### Step 3 — Per-test fix + gauntlet

Implement one fix at a time. After each:
1. Run the per-test unit test (passes).
2. Run the full validation gauntlet.
3. Verify NO commercial-ROM oracle flip (specifically: the
   `external_*` SMB / Excitebike / Kid Icarus baselines).
4. Verify NO sacred-trio regression.
5. Verify the target AccuracyCoin test flips, and no other
   AccuracyCoin test regresses.
6. Land OR roll back.

Move to next test only after the current one is closed (landed or
explicitly rolled back).

### Step 4 — Re-baseline if necessary

If a sprite-eval fix causes a commercial-ROM oracle framebuffer hash
shift (similar to session-8's 1-column-right BG shift), re-baseline ONLY
with explicit user authorization. Audio + cumulative cycle invariants
must remain byte-identical regardless.

## Cascade-risk callouts

1. **Sacred-trio regression risk is HIGHEST in this sprint.** The B8b
   commit `63d8dea` regressed SMB / Excitebike / Kid Icarus PAL via a
   mid-scanline FSM clobber. The recovery commit `834be9e` is the
   reference for "do NOT touch FSM step-functions mid-scanline".
   Audit `feedback_emulator_fsm_mid_cycle_clobber.md` in user memory
   bank before any FSM change.
2. **Permanent regression-prevention infrastructure** at
   `scripts/regression-bisect/` is the safety net. Run it before
   commit on every fix.
3. The 60-ROM commercial-ROM oracle is the long-tail regression
   detector. Visual inspection of the 81-PNG corpus at `screenshots/`
   is the human-readable cross-check.

## Estimated effort + yield

- **Effort:** 1-2 days per test (4 tests × 1-2 days = 4-8 days
  worst-case if all attempted).
- **Yield:** +1 to +4 AccuracyCoin tests.

## References

- nesdev `PPU OAM` page (sprite-eval FSM specification)
- nesdev `PPU sprite evaluation` page (dot-precise FSM behavior)
- Mesen2 `Core/NES/NesPpu.cpp` (`EvaluateSprites` family of methods)
- `crates/rustynes-ppu/src/ppu.rs` (RustyNES sprite-eval FSM
  implementation; line ~1349 dot-256 commit point)
- `docs/audit/cascade-a-investigation-2026-05-19.md` (methodology
  template for sprite-eval reproducer + bisect)
- `docs/audit/accuracycoin-readme-analysis-2026-05-17.md` (cluster
  diagnosis — the 4 sprite-eval residuals are the "Cascade A
  residuals" post-BG-pipeline-fix sub-cluster)
- AccuracyCoin source `AccuracyCoin.asm` (test sequences in the
  `Sprite Evaluation` suite block)

## Exit criterion

- AccuracyCoin pass rate increases (target +1 to +4 tests).
- No regressions in any of the 10 validation gauntlet gates.
- Sacred trio PAL boots cleanly.
- If pass rate reaches ≥ 90% after this sprint, jump to v1.0.0 final
  tag. Otherwise proceed to Sprint 4.

## Session 27 outcome (2026-05-23) — INVESTIGATION-ONLY

Per `docs/audit/session-27-sprint3-sprite-eval-residuals-2026-05-23.md`:

**4 of 4 target tests deferred to v1.x.** Rationale per target:

- **`$2002 flag timing` Test 1** — fails because the sprite flags
  (bits 5 + 6) clear-on-$2002-read timing is atomic in RustyNES, but
  the spec expects sprite flags to be sampled ~1.875 PPU cycles
  AFTER the vblank-flag latch (M2-low vs M2-high asymmetry). This is
  the **C1 axis** which the v1.0.0-final brief explicitly excludes
  (13+ rolled-back attempts on adjacent surfaces). Two answer-key
  pattern compares `[$E0, $E0, $80, $00]` (primary) and
  `[$E0, $80, $80, $00]` (alt) encode the M2-low-vs-M2-high
  asymmetry the C1 axis would close.

- **`Arbitrary Sprite zero` Test 2** + **`Misaligned OAM behavior`
  Test 1** — share root cause: when CPU writes `$2003` to a non-zero
  value just before sprite-eval, the first read at PPU dot 65 must
  come from `OAM[OAMADDR..]`, not `OAM[0..]`. Cycle 66's in-range
  decision then flips the sprite-zero-in-line latch based on whether
  that first read was in range — regardless of physical OAM index.
  **Session-9 (2026-05-20) attempted exactly this fix** and the
  targeted observables flipped, but **14 OTHER TESTS CASCADE-
  REGRESSED** including all 8 of `PPU Misc.`, all 5 of
  `CPU Behavior 2`, and all 5 of `Power On State`. A narrow-gate
  variant ("only honor the eval base on the FIRST eval pass after a
  non-zero `$2003` write") DID NOT eliminate the cascade. Decision:
  ROLLED BACK + deferred. The cascade mechanism is unresolved.

- **`OAM Corruption` Test 2** — requires modeling a NEW state
  machine: Secondary OAM Address tracking + per-PPU-dot OAM-row
  corruption seed + 8-byte row replacement on the next render-
  enable transition. The state machine has cycle-by-cycle
  behavior across cycles 1-64 (incrementing), 65-256 (FSM-driven),
  and 257-320 (8-cycle-loop pattern). This is multi-session work
  that touches the same FSM mid-scanline write surface that B8b
  regressed.

**What landed**:

1. Audit document at
   `docs/audit/session-27-sprint3-sprite-eval-residuals-2026-05-23.md`.
2. 4 custom AccuracyCoin sub-test ROMs (permanent regression-
   prevention infrastructure) at:
   - `tests/roms/AccuracyCoin/sub-tests/sprite-eval-2002-flag-timing.nes`
   - `tests/roms/AccuracyCoin/sub-tests/sprite-eval-arbitrary-sprite-zero.nes`
   - `tests/roms/AccuracyCoin/sub-tests/sprite-eval-misaligned-oam.nes`
   - `tests/roms/AccuracyCoin/sub-tests/sprite-eval-oam-corruption.nes`
3. Sprint 3 + sprint-gate-conditions.md status updates.
4. CHANGELOG `[Unreleased]` Session-27 entry.

**Workspace test count unchanged**: 545 strict + 5 ignored across 34
suites with `--features test-roms`.
**AccuracyCoin pass rate unchanged**: 84.17% (117/139 assigned).
**Commercial-ROM oracle unchanged**: 60/60 strict pass.
**Sacred trio preservation**: confirmed (no chip-stack code changed).
**B4 invariants**: confirmed (no chip-stack code changed).

**Next sprint**: per `sprint-gate-conditions.md` §"Per-sprint gate"
rule 4 (pass rate in 83-87% band), proceed to **Sprint 4 (PPU misc
residuals)** which has higher tractability — several PPU Misc tests
(`Stale BG/Sprite Shift Registers`, `BG Serial In`,
`$2007 Stress Test`) are on the BG-pipeline surface that session-8's
`086ce4d` Cascade A resolution touched cleanly.
