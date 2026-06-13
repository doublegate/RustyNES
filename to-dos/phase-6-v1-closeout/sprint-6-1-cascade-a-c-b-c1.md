# Sprint 6-1 — v1.0.0 cascade closures

**Date:** 2026-05-19
**Owner:** Claude (with user authorization)
**Phase:** 6 v1.0.0 closeout
**Predecessors:** v1.0.0-rc1 tag (commit `340cdba`), Cascade B closure (commit `9b0c81c`).

## Scope (per user `/goal`)

1. **Cascade B — DMC DMA halt-cycle precision.** STATUS: **CLOSED in session 4** (commit `9b0c81c`). All 8 tests in "APU Registers and DMA tests" suite flipped. AccuracyCoin advanced 69.78% → 76.98%. No further action required for this item.

2. **Cascade A — Sprite Zero Hit cycle precision.** STATUS: investigation in progress. Empirical data from session 5 diagnostic probe (commit `3a0fce0`) rejects audit hypothesis (b). PPUMASK_COPY=0x1E at test entry (BG bit IS set); PPUSTATUS bit 6 = 0 when result byte transitions (hit never fires). The audit-narrowed bug-location list (4 candidates in `crates/rustynes-ppu/src/ppu.rs`) is the working set.

3. **T-60-001 C1 IRQ-timing breakthrough.** STATUS: multi-week, 11 prior rollbacks. ADR-0002 "Decision (revised, 2026-05-13)" has the proposed coordinated change.

## Sprint plan

### Phase 1 — Cascade A (this sprint, highest ROI)

**Goal:** Close all 16 Sprite Zero Hit cascade tests without regressing any of:
- The 78+ committed baselines (19 audio_tests + 60 commercial ROMs + 21 permissive baselines).
- The 11 blargg `sprite_hit_tests/*.nes` ROMs that currently pass.
- The strict-pass count of 510 + ignored 5.

**Acceptance:** AccuracyCoin advances 76.98% → ≥ 88% (Cascade A delivers +16 tests, putting us at ~88.5%).

#### Step 1 — Direct PPU unit test for the exact scenario

Build a deterministic unit test in `crates/rustynes-ppu/src/ppu.rs` `#[cfg(test)]` that constructs the AccuracyCoin `TEST_Sprite0Hit_Behavior` sub-test 1 scenario directly, bypassing the test ROM and CPU:

- Sprite 0: `Y=0, CHR=$FC, ATT=0, X=8` (write to PPU OAM).
- BG nametable: `vram[$2001] = $FC` (8 KiB nametable RAM is direct).
- CHR ROM: load tile `$FC` rows 0..7 with `lo=$FF / hi=$00` (fully opaque pixels) in pattern table 0 (BG + sprite default).
- `PPUMASK = $1E` (BG + SPR + BG_LEFT + grayscale).
- `PPUCTRL = $00` (both pattern tables at `$0000`).
- `v = $2000` (scroll origin, top-left of nametable).
- Tick PPU through pre-render → scanline 0 → scanline 1 → scanline 2 (minimum). Hit should fire by end of scanline 1.
- Assert `PPUSTATUS bit 6 = 1`.

**Expected outcome (per current investigation):** test FAILS — proves the bug.

#### Step 2 — Bisect the 4 candidate code paths

Once the unit test reproduces the bug, narrow with targeted instrumentation:

1. **Sprite-eval FSM Y=0 edge case** (line 1349 area):
   - Add temporary `eprintln!` at the dot-256 commit point logging `(scanline, sprite_eval_zero_found, sprite_eval_found, secondary_oam[0..4])`.
   - Expected: at end of scanline 0, `sprite_eval_zero_found = true`, `sprite_eval_found ≥ 1`, `secondary_oam[0..4] = [0, $FC, 0, 8]`.

2. **`fetch_sprite_tile` pattern load** (line 1486):
   - Log `(slot=0, scanline, y=0, tile=$FC, in_use, row, addr_lo, lo, hi)` for sprite 0 fetches.
   - Expected: at scanline 0 dot 260+ fetch (for scanline 1), `addr_lo=$0FC0`, `lo=$FF`, `hi=$00`.

3. **`spr_x` decrement timing** (lines 1235-1242):
   - Log `(scanline=1, pixel_x, spr_x[0], spr_shift_lo[0])` for dots 1..16.
   - Expected: at dot 9 (pixel_x=8), `spr_x[0] = 0`, `spr_shift_lo[0] & 0x80 = 0x80`.

4. **Hit predicate** (line 1205-1212):
   - Log `(scanline, pixel_x, spr_zero_pixel, bg_idx, spr_idx)` when both opaque.
   - Expected: at scanline 1 pixel_x = 8, `spr_zero_pixel=true, bg_idx=1, spr_idx=1`.

#### Step 3 — Surgical fix

Once the failing axis is identified, apply the narrowest possible fix. The Codex 2026-05-19 PPU change broke 78+ baselines because it changed sprite-eval emit ordering (a broad architectural change). The fix here must be a single-condition adjustment, not a flow restructure.

#### Step 4 — Regression validation

Mandatory, in order:
1. New PPU unit test — passes.
2. `cargo test --workspace --features test-roms` — 510 strict pass + 5 ignored (no count change).
3. `cargo test --workspace --features test-roms,commercial-roms` — 60 commercial ROMs, all snapshots stable.
4. `cargo test --workspace --features test-roms --release -p rustynes-test-harness --test accuracycoin` — AccuracyCoin RAM pass rate ≥ 88%.
5. `scripts/regression-bisect/bisect-real-games.sh` against SMB / Excitebike / Kid Icarus PAL (matches what session 4's audit ran via Codex's worktree).

Only if all 5 gates pass, commit + push the fix.

### Phase 2 — C1 IRQ-timing axis (deferred to subsequent sprint)

Given:
- 11 prior rollback attempts across multiple sessions.
- Multi-week investigation time per the project documentation.
- This sprint's primary focus is Cascade A.

C1 work in this session is **research-only**. Document any findings in ADR-0002 but do not attempt a code-change rollback risk. The structural work item remains open for v1.0.0 final.

### Phase 3 — Final gates

After Cascade A closes:
- Update `CHANGELOG.md` `[Unreleased]` with the Cascade A closure narrative.
- Update `docs/STATUS.md` AccuracyCoin pass rate (76.98% → new value).
- Update `docs/audit/accuracycoin-readme-analysis-2026-05-17.md` with the closing addendum.
- Update `to-dos/ROADMAP.md` T-60-002 trajectory.
- Push to `origin/main`.
- Verify CI green.

If AccuracyCoin reaches ≥ 90% (Cascade A + any other closures), promote `v1.0.0-rc1` → `v1.0.0`. If not, the next sprint addresses the C1 axis explicitly.

## Risks

| Risk | Mitigation |
|------|-----------|
| PPU fix regresses the 78+ baselines | The bisect tooling at `scripts/regression-bisect/` + 60-ROM commercial corpus auto-detect. Run before commit. |
| Fix flips Cascade A tests but breaks blargg sprite_hit_tests | `cargo test --workspace --features test-roms` includes all 11; CI gate catches it. |
| Investigation extends beyond session budget | Sprint can be split: land the unit test (proves the bug) + targeted instrumentation as one commit; defer the fix to next session. |
| Cascade A and C1 axes interact (e.g., shared sprite-eval timing) | Document any cross-axis coupling found; do not modify C1 surface in this sprint. |

## Out of scope

- C1 IRQ-timing breakthrough (multi-week, deferred to subsequent sprint).
- VRC7 FM audio (ADR-0004 deferral to v1.x).
- Internal-bus model rework (Open Bus + SH* cluster, deferred to v1.x).
- Frame Counter IRQ + DMC + APU activation (5 APU tests; deferred to v1.x).

## Session-7 progress (2026-05-19, post-Cascade-A-partial)

**Cascade B status:** fully verified complete. Session-7 review of
Codex's DMC DMA scheduler work (commit `9b0c81c`) plus the follow-on
extension in session 2 (`Bus::replay_readout_bug` covering `$2002` /
`$4016` / `$4017`) found no remaining optimization targets — the
8-test "APU Registers and DMA tests" cluster is closed, the 3 net
side-benefit flips elsewhere are stable, and the `dmc_dma_during_read4`
regression guard stays 5/5 strict.

**Cascade A status:** partial closure landed across sessions 5-7.
The geometric residual (`VerifySpriteZeroHits` step-2) is the open
work item. Characterisation reproducer committed in `b629ace`
(`test(ppu): VerifySpriteZeroHits step-2 characterisation reproducer`)
documents the open puzzle in a `crates/rustynes-ppu/src/ppu.rs` `#[cfg(test)]`
direct-PPU unit test. Next step: Mesen2 reference trace of the
identical scanline / dot range to localise the divergence point.

**Session-7 fixes landed (chronological)**:

1. `f29f7ca` — PPU OAMADDR reset during dots 257-320 (sprite-tile-
   loading interval). +2 AccuracyCoin tests; pass rate 76.98% → 78.42%.
2. `6c2664e` — `$2004` reads return `$FF` during dots 1-64 secondary-
   OAM clear. Internal-sub-test advancement only; pass rate stays at
   78.42%.
3. `b629ace` — characterisation reproducer for the `VerifySpriteZeroHits`
   step-2 residual.
4. `c230489` — OAMADDR walks during sprite-eval dots 65-256 + `$2004`
   reads return `$FF` during dots 257-320 (extends `6c2664e`) + `$2004`
   write during rendering uses `(OAMADDR + 4) & 0xFC` realignment.
   +1 AccuracyCoin test (`Address $2004 behavior` PASS with code 16);
   pass rate 78.42% → 79.14%.
5. `32d5b18` — 6502 RMW ABS,X / ABS,Y always-dummy at unfixed address
   for 18 RMW opcodes (12 ABS,X + 6 ABS,Y). +1 AccuracyCoin test
   (`Controller Clocking` FAIL → PASS); `Implied Dummy Reads` advances
   error 2→3 + `Frame Counter IRQ` advances error 6→7 via the
   SLO $4015,X bracket. Pass rate 79.14% → 79.86%.

**Investigated + reverted (session 7):** always-defer `$4015`-read
frame IRQ-clear path. Would have flipped AccuracyCoin
`Frame Counter IRQ` sub-test 7 but regressed kevtris
`apu_test/6-irq_flag_timing` sub-test 4. Preserving the kevtris
baseline trumps a single AccuracyCoin sub-test flip; the
deferred-clear semantic needs cycle-precise modeling of the
bus<->APU read ordering that is out of scope for this session.

**T-60-001 C1 IRQ-timing axis:** unchanged from sprint plan. 11 prior
rollback attempts; the canonical CPU `T_last - 1` IRQ-sample-point
rework on the `cpu_interrupts_v2` axis remains a multi-week
investigation gated on a new empirical finding. No session-7 attempt.

**Pass-rate trajectory recap**: `76.98% → 78.42% → 79.14% → 79.86%`
across sessions 5/6/7. Within 0.14pp of the v0.9.x 80% target;
~10pp from the v1.0.0 90% gate. Remaining 29 failing tests cluster
into the Cascade A geometric residual (6 sprite-eval + 7 PPU misc
gated on the same step-2 puzzle), the C1 IRQ-timing axis (3 +
`mmc3_test_2/4` #3), the internal-bus model (5 SH* + 1 Open Bus #9
+ 1 Implied Dummy Reads #3), 1 PPU `$2007` read w/ rendering, and 5
APU residuals.

## Session 8 — 2026-05-20 — Cascade A BG-pipeline cycle-9 reload

**Authorized** by user explicitly (2026-05-19) after the
`docs/audit/cascade-a-investigation-2026-05-19.md` investigation
identified the BG shift-register pipeline as the load-bearing root
cause of the `VerifySpriteZeroHits` step-2 puzzle. The fix had
been prototyped, demonstrated to regress 60 commercial-ROM
snapshots + 3 visual snapshots due to a 1-column BG shift (the
CORRECT alignment per Mesen2 + nesdev wiki, not a functional
regression), and rolled back pending user authorisation to
re-baseline. Authorisation granted; the BG fix landed on `main`
together with the re-baseline.

**Code commit:** `086ce4d` — `fix(ppu): BG shift-register cycle-9
reload + post-emit shift`. Three conceptual edits in
`crates/rustynes-ppu/src/ppu.rs::Ppu::tick`:

1. Move `reload_bg_shift_regs()` from phase 7 (cycle 8) to phase 0
   (cycle 9 = first cycle of new fetch group, per Mesen2's
   `LoadTileInfo()` `case 1` and nesdev wiki "shifters are reloaded
   during ticks 9, 17, 25, …, 257").
2. Move `shift_bg()` from BEFORE the BG fetch block to AFTER
   `emit_pixel()`. Shifts only happen on visible scanlines, not
   pre-render (matches Mesen2's `if(_scanline >= 0)` guard in
   `ProcessScanlineImpl()` lines 881-884).
3. Add explicit `bg_shift_lo <<= 8` + `bg_shift_hi <<= 8` at phase
   7 of the pre-fetch region (dots 328 and 336) per Mesen2
   `ProcessScanlineImpl()` lines 941-944 — substitutes for the
   missing per-cycle shifts during 321-336 and clears bits 0-7
   ahead of the next reload.

Plus: flip `cascade_a_verify_sprite_zero_hits_step2`
characterisation probe assertion from `assert!(!hit)` to
`assert!(hit)` — sprite-zero hit now correctly fires.

**Re-baseline commit:** `f79e44c` — `test(snapshots): re-baseline
visual snapshots for BG-pipeline fix`. Scope:
- 60 commercial-ROM `external_real_games` framebuffer snapshots
  (audio FNV-1a hashes + cumulative cycle counts byte-identical
  across the suite; only framebuffer hashes changed).
- 3 visual snapshots (`m22_vrc2a_chr_banking_0_127`,
  `mmc1_a12_non_mmc3_a12_is_inert`,
  `instr_test_basics_frame_120`).
- 68 PNGs under `screenshots/external/` regenerated with the
  current `sanitize()` flat-layout naming
  (`mapper_NNN_<mapper>_<rom>_fNN.png`); legacy per-mapper-subdir
  PNGs (from a pre-`6b3a818` dump function) simultaneously cleaned
  up. 4 uniform-gray PNGs (Lagrange Point + 3 others) detected by
  git as pure renames since their bytes did not change.
- 2 PNGs at `screenshots/m22/0_127_f240.png` +
  `screenshots/mmc1_a12/mmc1_a12_f240.png` re-captured.

**Sacred trio visually verified:** SMB / Excitebike / Kid Icarus
PNGs all render legibly with the corrected BG alignment — title
screens, menus, and gameplay backgrounds all coherent.

**Tests flipped FAIL → PASS (4 tests):**
1. `Sprite Evaluation :: Suddenly Resize Sprite [error 1]`
2. `Sprite Evaluation :: Sprite 0 Hit behavior`
3. `Sprite Evaluation :: Sprite overflow behavior`
4. `PPU Behavior :: $2007 read w/ rendering [error 1]`

**Pass rate:** 79.86% → **82.73%** (+4 tests, +2.87pp — the
largest single-commit pass-rate jump since Cascade B's DMC DMA
scheduler at session 5). **v0.9.x 80% target CLEARED by 2.73pp.**

**Workspace tests:** 537 strict + 5 ignored (unchanged at session
8 — the BG-pipeline fix flips existing characterisation polarity
on the `cascade_a_verify_sprite_zero_hits_step2` probe rather
than adding new unit tests). With `--features
test-roms,commercial-roms`: **597 strict + 5 ignored** (all 60
commercial-ROM snapshots re-baselined and pass).

**Pass-rate trajectory recap (post-session-8):** `76.98% → 78.42%
→ 79.14% → 79.86% → 82.73%` across sessions 5/6/7/8. **v0.9.x 80%
target CLEARED**; ~7.27pp from the v1.0.0 90% gate. Remaining 24
failing tests cluster as: 4 sprite-eval residuals (post-BG-fix
$2002 flag timing + Arbitrary Sprite zero + Misaligned OAM
behavior + OAM Corruption), 6 PPU misc residuals (post-BG-fix
Stale BG/Sprite Shift Regs + BG Serial In + Sprites On Scanline 0
+ $2004/$2007 Stress Tests), the C1 IRQ-timing axis (3 +
`mmc3_test_2/4` #3, 11 prior rollbacks), the internal-bus model
(5 SH* + 1 Open Bus #9 + 1 Implied Dummy Reads #3), and 4 APU
residuals.

**Cascade A status post-session-8:** PARTIALLY CLOSED at the
architectural level. The geometric root cause
(`VerifySpriteZeroHits` step-2 1-column BG misalignment) is
resolved per `docs/audit/cascade-a-investigation-2026-05-19.md`'s
RESOLUTION section. Remaining sprite-eval / PPU-misc residuals
on the Cascade-A-adjacent surface are now empirically subtler-
cycle-precision issues (stale shift register modeling, post-B8
sprite FSM interactions, sub-cycle $2002 flag timing) rather than
geometric-pipeline issues. Further session work can address these
without the 60-ROM-commercial-oracle blast radius that the
BG-pipeline fix carried.

### Session 10 (2026-05-20) — per-PPU-dot observability tooling

Infrastructure-only landing. The Session-9 sprite-eval cascade
rollback (`sprite_eval_base_from_OAMADDR` across 3 variants, all
flipping the targeted tests but cascading 14 regressions) showed
the load-bearing failure is intermediate-state corruption rather
than the dirty-flag gating itself. The next investigation needs
runtime-state visibility comparable to Mesen2's debugger, so this
session lands the per-PPU-dot state-trace fixture (the PPU
analogue of the per-CPU-cycle `irq_trace` fixture that
empirically unblocked Phase B4 of Track C1).

**What landed (no behavior changes; all gated on the
`ppu-state-trace` cargo feature, off by default):**

* `crates/rustynes-ppu/src/state_trace.rs`: `PpuStateRecord` schema
  (111 bytes packed, schema v1), `PpuTraceConfig` with
  `all` / `visible_only` / `sprite_eval_window` presets,
  `PpuStateTrace` linear buffer with capacity cap + overflow
  counter, binary (LE) + CSV emitters, binary parser with
  magic / version / alignment validation, FNV-1a-64 hash for
  primary OAM digest.
* `crates/rustynes-ppu/src/ppu.rs`: optional `state_trace: Option<...>`
  field on `Ppu` (`#[cfg]`-gated), `enable_state_trace` /
  `take_state_trace` / `state_trace` / `build_state_record`
  API, per-dot recording hook at end of `Ppu::tick`. Read-only;
  preserves determinism contract.
* `crates/rustynes-test-harness/src/bin/ppu_trace_diff.rs`: CLI
  binary that aligns two traces by record index and reports
  first or all per-field divergences;
  `--skip-fields <CSV>` opts out of fields the reference side
  doesn't populate. Exit codes: 0 = equivalent, 1 =
  divergence, 2 = parse error.
* `crates/rustynes-test-harness/tests/ppu_state_trace_fixture.rs`:
  AccuracyCoin-driver integration test. Env-var override for
  start/end frames + output path. Roundtrip-asserts the
  binary parses back to identical records.
* `scripts/mesen2_ppu_trace.lua`: Mesen2 Lua-script
  reference-trace emitter. Per-scanline granularity (the
  finest the published Mesen2 Lua API exposes). Documented
  with the recommended `--skip-fields` invocation in
  `docs/ppu-trace-tooling.md`.
* `docs/adr/0005-ppu-state-trace.md` — design rationale.
* `docs/ppu-trace-tooling.md` — operator's guide.
* CHANGELOG `[Unreleased]` entry + STATUS feature-flag matrix
  row + audit session 10 addendum.

**Verification:**

* `cargo test -p rustynes-ppu --features ppu-state-trace`: 12 new
  unit tests pass.
* `cargo test -p rustynes-test-harness --release --features
  test-roms,ppu-state-trace --test ppu_state_trace_fixture`
  with 2-frame window: 163,680 records, zero overflow, binary
  roundtrips.
* `./target/debug/ppu_trace_diff` on identical traces: exit 0.
  On synthetic 3-byte mutation: exit 1, correct field names
  reported (`ctrl`, `v`, `spr_shift_lo`).
* AccuracyCoin pass rate UNCHANGED at 82.73% (this is tooling,
  not a fix). Workspace tests UNCHANGED at 537 strict + 5
  ignored. Commercial-ROM oracle UNCHANGED at 60 green.

**Cascade A status post-session-10:** UNCHANGED from session-8
(partial-closure at architectural level). The Session-11
investigation has the runtime-state visibility it needs to diff
against a Mesen2 reference and isolate the load-bearing
intermediate-state corruption.
