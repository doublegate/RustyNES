# Sprint gate conditions + early-exit policy

**Phase:** 6 — v1.0.0 final.
**Authority:** user (Option-B mandate, 2026-05-22). The 90%
AccuracyCoin target is preserved as the v1.0.0 final gate. This
document defines the conditions for advancing sprint-to-sprint, for
declaring v1.0.0 final, and for escalating back to the user.

## Per-sprint gate

After each sprint (1 → 6, in order):

1. Re-measure AccuracyCoin via
   `cargo test -p rustynes-test-harness --features test-roms accuracycoin
   --release -- --nocapture`.
2. If pass rate ≥ 90% (≥ 126 of 139 assigned tests):
   - **STOP the sprint backlog.** Sprints subsequent to the current
     one are not attempted.
   - Proceed to **v1.0.0 final tag protocol** (see below).
3. If pass rate < 90% but ≥ 87% (≥ 121 / 139):
   - Re-evaluate whether the next sprint's estimated yield can close
     the remaining gap. If yes, proceed. If no, escalate.
4. If pass rate is in the 83-87% band:
   - Proceed to next sprint per the priority order.
5. If pass rate regresses below the rc2 baseline (82.73%) for any
   reason:
   - Roll back the regressing change. Investigate via the standard
     `git bisect` + `scripts/regression-bisect/` flow. Do NOT proceed
     to the next sprint until the regression is closed.

## v1.0.0 final tag protocol

Triggered when AccuracyCoin ≥ 90% AND all validation gates green AND
all 4 C1 IRQ-timing residuals flipped (Sprint 5 closure).

1. `Cargo.toml` workspace version: `1.0.0-rc2` → `1.0.0`.
2. `CHANGELOG.md`: cut `[1.0.0]` section from `[Unreleased]`; date it
   the tag day.
3. `docs/STATUS.md`: bump version line; mark v1.0.0 final.
4. `README.md`: update version reference; update compatibility
   status; remove v1.0.0-rc2 release-candidate framing.
5. `to-dos/ROADMAP.md`: close Phase 6; mark T-60-001 + T-60-002 +
   T-60-005 as CLOSED.
6. Pre-tag gauntlet (Phase 3.1 of the rc2 tag session): all 10 gates
   green.
7. Commit: `chore(release): v1.0.0 final`.
8. Tag: `git tag -a v1.0.0 -m "..."`.
9. Push: `git push origin main && git push origin v1.0.0`.

## Escalation conditions

Stop sprint backlog and escalate to user when:

1. **All 6 sprints completed and pass rate < 90%.** The 90% bar may
   need v1.x reframing or additional unbudgeted sprints. Document the
   per-sprint trajectory and propose options:
   - Option A: continue with additional non-canonical fixes (riskier
     surface; specifically test-specific patches that don't
     generalize to hardware accuracy).
   - Option B: defer the 90% gate to v1.x; tag v1.0.0 final at the
     achieved rate (e.g., 87% if 5 of 6 sprints succeeded).
   - Option C: re-investigate one or more of the rolled-back C1
     attempts on a fresh empirical oracle (e.g., a physical NES dump
     of the failing test cycle).

2. **A sprint's validation gauntlet fails irrecoverably** (i.e., the
   change cascades into ≥ 5 regressions that cannot be untangled
   without rolling back additional historical commits). Document the
   cascade structure and request user decision.

3. **The sacred trio regresses** (SMB / Excitebike / Kid Icarus PAL
   boot-and-play breaks). This is a hard stop: roll back the
   regressing change immediately, run the bisect harness, and request
   user input before any further work.

4. **The 60-ROM commercial-ROM oracle requires re-baselining for
   reasons other than Sprint 5's expected access-ordering shift.** Any
   audio FNV-1a or cumulative cycle-count invariant break is a hard
   stop (those should remain byte-identical across non-C1-axis fixes).

## Per-sprint commit + push discipline

Per `to-dos/phase-6-v1.0.0-final/overview.md` "Commit + push
discipline" section. Specifically:

- Each sprint commits ONLY when its validation gauntlet is green.
- Sprints that cascade-revert land their diagnostic + unit-test +
  audit-doc artifacts as a separate audit-only commit. The CHANGELOG
  `[Unreleased]` section gains an "Investigated and rolled back —
  Sprint N" subsection.
- No force-pushes to `main`. No `--amend` after pre-commit hook
  failure (create a NEW commit).
- Pull commercial-ROM oracle re-baseline ONLY with explicit user
  authorization.

## Sprint progress tracking

Each sprint logs its outcome here as it completes. Format:

```
- Sprint N (YYYY-MM-DD): {LANDED|ROLLED-BACK|MIXED}
  - Tests flipped: <list> (+N)
  - Tests regressed: <list> (-N if any)
  - Pass rate: PRE → POST
  - Commit: <hash> (or "no commits — rollback")
  - Audit doc: docs/audit/sprint-N-<topic>.md
```

- Sprint 1 (2026-05-22, iteration 1): ROLLED-BACK
  - Tests flipped: none (target `Implied Dummy Reads` did NOT flip)
  - Tests regressed (feature-ON only, reverted): `Implicit DMA Abort` (-1)
  - Pass rate: 82.73% → 82.73% (post-revert; no commits with chip-stack diff)
  - Commits: `7d55367` (Phase 1 investigation doc) +
    Phase 2 rollback addendum commit (this session, audit-only)
  - Audit doc: docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md
  - Next: Sprint 1 iteration 2 (multi-session DMC scheduler
    audit) OR Sprint 2 (APU put/get phase plumbing). Recommendation
    in audit doc: skip to Sprint 2; defer Sprint 1 iteration 2 to
    after DMC trace tooling is built.

- Sprint 1 (2026-05-22, iteration 2 Phase B): INVESTIGATION-ONLY
  - Phase 1A landed: Mesen2 Lua extended with AccuracyCoin protocol +
    autostart Start-press + per-sub-test RAM watchdog + exec-callback
    throughput knob.
  - Phase 1B deferred: wall-time blocker. Mesen2's Lua exec callback
    cannot sustain throughput to reach the DMC sub-tests within
    reasonable session budget (~5-7 effective FPS under xvfb; Mesen2's
    testRunner pauses around frame 1589, short of test #141).
  - No chip-stack code changed; pass rate unchanged.
  - Pass rate: 82.73% → 82.73%
  - Audit doc: docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md
  - Next: Sprint 2 (APU put/get phase plumbing). Phase B re-attempt
    deferred to a focused future session that either (a) builds Mesen2
    from source with native C++ debug hooks, or (b) compiles a custom
    AccuracyCoin sub-test ROM that jumps directly to
    `TEST_ImpliedDummyRead`.

- Sprint 2 (2026-05-22, iteration 1): INVESTIGATION-ONLY
  - Mesen2 cross-reference landed:
    `Core/NES/APU/ApuFrameCounter.h` (`WriteRam` + `Run` + `GetIrqFlag`)
    and `Core/Shared/BaseControlDevice.cpp` (`StrobeProcessWrite`).
  - AccuracyCoin sub-test architectural model landed for the 4
    targets:
    - Controller Strobing #102 (test #4 fails): latch must fire on
      M2-low boundary; RustyNES currently latches on rising edge
      regardless of M2 phase.
    - Frame Counter IRQ #97 (test #7 fails): two hypotheses
      (semantic value of `apu_aligned` argument; OR
      `pending_irq_clear` clear-on-current-cycle vs always-defer model).
    - APU Register Activation #101 (test #4 fails): put/get axis
      candidate, not confirmed.
    - DMC #100 (error 21 = deep into the sub-test list): unconfirmed
      without an oracle.
  - Production fix deferred: same Mesen2 oracle wall-time blocker as
    Sprint 1 Phase 1B. The Sprint 2 spec itself notes Mesen2 is the
    only reliable oracle for the put/get convention (line 116-118 of
    `sprint-2-apu-put-get-phase.md`).
  - No chip-stack code changed; pass rate unchanged.
  - Pass rate: 82.73% → 82.73%
  - Audit doc: docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md
  - Next: Sprint 3 (sprite-eval residuals) OR the shared oracle
    unblock that lets BOTH Sprint 1 Phase 1B AND Sprint 2 production
    fixes proceed with empirical evidence. The Sprint 2 hypothesis
    set (Controller Strobing M2-low latch axis especially) is precise
    enough that a successful oracle trace in a future session could
    drive a single-axis fix within one session.

- Session 28 Sprint 4 (2026-05-23): **INVESTIGATION-ONLY** — PPU misc residuals (6 tests)
  - Tests flipped: none.
  - Tests regressed: none.
  - Pass rate: 84.17% → 84.17% (unchanged).
  - Workspace `--features test-roms`: **545 strict + 5 ignored** (unchanged).
  - Commercial-ROM oracle: **60/60** (unchanged).
  - Sacred trio (SMB / Excitebike / Kid Icarus PAL): preserved (no
    chip-stack code change).
  - B4 invariants: preserved.
  - Outcome: per-test tractability table shows 0 of 6 are tractable
    within the brief's constraints. 1 candidate (`$2004 Stress`) is
    on Sprint 3's deferred eval-base-from-OAMADDR axis (Session-9
    14-test cascade). The remaining 5 require net-new state machines
    OR per-PPU-clock analog modeling beyond current architecture,
    with EXTREME cascade risk into the commercial-ROM oracle
    (Session-8 Cascade A re-baselined surface) and / or the
    sprite-eval FSM (B8b regression surface).
  - Permanent infrastructure landed: 6 custom AccuracyCoin sub-test
    ROMs at `tests/roms/AccuracyCoin/sub-tests/ppu-misc-*.nes`
    (40976 B each, suite-index 18, test-indices 2/3/4/5/6/7) — boot
    to target test by frame ≤ 400 (most ≤ 92); usable as one-shot
    oracle inputs by `validate_sub_test_rom` + Mesen2 `testRunner`
    for any future PPU misc fix attempt.
  - Audit doc: `docs/audit/session-28-sprint4-ppu-misc-residuals-2026-05-23.md`.
  - Sprint 4 status: 0 of 6 landed (6 targets deferred to v1.x).
  - **Next**: per Per-sprint-gate rule 4 (pass rate in 83-87% band) —
    proceed to **Sprint 5 (C1 IRQ-timing rework)** OR strategic
    re-evaluation. Sprint 3 + Sprint 4 both closed INVESTIGATION-ONLY;
    the residual gap of `90% - 84.17% = 5.83pp ≈ 8 tests` is
    concentrated on architectural surfaces with documented
    multi-session cascade history (C1 axis: 13 prior rollbacks;
    eval-base-from-OAMADDR axis: Session-9 14-test cascade) or
    analog hardware-precision modeling exceeding the current
    architecture (PPU DATA state machine, BG shifter serial-in,
    sprite counter mode, pre-render-as-scanline-5).

- Session 27 Sprint 3 (2026-05-23): **INVESTIGATION-ONLY** — Sprite-eval residuals (4 tests)
  - Tests flipped: none.
  - Tests regressed: none.
  - Pass rate: 84.17% → 84.17% (unchanged).
  - Workspace `--features test-roms`: **545 strict + 5 ignored** (unchanged).
  - Commercial-ROM oracle: **60/60** (unchanged).
  - Sacred trio (SMB / Excitebike / Kid Icarus PAL): preserved (no
    chip-stack code change).
  - B4 invariants: preserved.
  - Outcome: per-test tractability table shows all 4 targets are DEEP
    with documented cascade history. `$2002 flag timing` Test 1 axis
    IS the C1 axis the brief explicitly excludes (M2-low-vs-M2-high
    ~1.875 PPU cycles asymmetry). `Arbitrary Sprite zero` Test 2 +
    `Misaligned OAM behavior` Test 1 = Session-9 (2026-05-20)
    documented 14-test cascade on the eval-base-from-OAMADDR axis,
    NOT eliminated by a narrow gate. `OAM Corruption` Test 2 requires
    a NEW state machine (Secondary OAM Address tracking + corruption
    seed + 8-byte row replacement) that touches the same FSM
    mid-scanline write surface that B8b regressed.
  - Permanent infrastructure landed: 4 custom AccuracyCoin sub-test
    ROMs at `tests/roms/AccuracyCoin/sub-tests/sprite-eval-*.nes`
    (40976 B each, suite-index 17, test-indices 2/4/5/7) — boot to
    target test by frame ≤ 66; usable as one-shot oracle inputs by
    `validate_sub_test_rom` + Mesen2 `testRunner` for any future
    sprite-eval fix attempt.
  - Audit doc: `docs/audit/session-27-sprint3-sprite-eval-residuals-2026-05-23.md`.
  - Sprint 3 status: 0 of 4 landed (4 targets deferred to v1.x).
  - **Next**: per Per-sprint-gate rule 4 (pass rate in 83-87% band) —
    proceed to **Sprint 4 (PPU misc residuals)** which has higher
    tractability than Sprint 3's surface.

- Session 26 Sprint 2 iter 5 (2026-05-23): **LANDED** — Frame Counter `irq_flag` vs CPU IRQ-line split (Tests J/K/L/M/N/O)
  - Tests flipped: `APU Tests :: Frame Counter IRQ` (from `[error 19]`
    to PASS); custom Frame Counter IRQ ROM advances from `$4E = Fail
    Test J` to `$01 = PASS` (Tests J, K, L, M, N, O all flipped).
  - Tests regressed: none.
  - Workspace `--features test-roms`: **545 strict + 5 ignored** (unchanged).
  - Targeted regression-preserved: `apu_test` 8/8 (including the blargg
    frame-counter timing tests that Session-25's failed Test J attempt
    cascaded against), `apu_mixer` 4/4, `dmc_dma` 5/5, `mmc3` 12+2 (B4
    invariant preserved), Controller Strobing custom ROM PASS, custom
    APU Register Activation `$1A` (iter 4 preserved).
  - **THE 4 MMC3 COMMERCIAL CANARY ROMs**: `external_mmc3_mega_man_3`,
    `external_mmc3_tmnt3`, `external_mmc3_ninja_gaiden_2`,
    `external_mmc3_tiny_toon_adventures_2` ALL strict-pass under the
    split (the Session-25 cascade does NOT recur). Full 60-ROM oracle:
    60/60.
  - AccuracyCoin RAM: 83.45% → **84.17%** (+1 net flip).
  - AccuracyCoin framebuffer: 89.83% → **90.68%** (+1pp).
  - Production change: `crates/rustynes-apu/src/frame_counter.rs` adds new
    `irq_line_active: bool` field separate from `irq_flag`; FC steps
    29828/29829 set `irq_flag` unconditionally; step 29830 conditionally
    (cleared if inhibited per Test L); `irq_line_active` set only when
    not inhibited. `Apu::irq_line()` reads `irq_line_active` instead of
    `irq_flag`. `$4015` read and `$4017` inhibit-set clear both fields.
    `APU_SNAPSHOT_VERSION` bumped 2 → 3 with v2 → v3 migration
    (`irq_line_active = irq_flag` since v1/v2 conflated them).
  - Audit doc: `docs/audit/session-26-sprint2-iter5-frame-counter-irq-split-2026-05-23.md`.
  - Sprint 2 status: 4 of 4 targets LANDED (Controller Strobing +
    Frame Counter IRQ Test 7 + APU Register Activation Test 4 + Frame
    Counter IRQ J/K/L); only DMC [error 21] remains INVESTIGATION-ONLY.

- Session 26 Sprint 2 iter 4 (2026-05-23): **LANDED** — APU Register Activation OAM-DMA chip-select gate (Test 4)
  - Tests flipped: none in the catalog headline (the APU Register
    Activation entry remains FAIL); internal advance from Test 4 to
    Test 6 (Tests 4 + 5 PASS under the fix).
  - Tests regressed: none.
  - Workspace `--features test-roms`: **545 strict + 5 ignored** (unchanged).
  - Targeted regression-preserved: `apu_test` 8/8, `apu_mixer` 4/4,
    `dmc_dma` 5/5, `mmc3` 12+2 (B4 invariant), Controller Strobing +
    Frame Counter IRQ custom ROMs PASS/preserved-shape.
  - 60-ROM commercial oracle: 60/60 (incl. the 4 MMC3 canaries).
  - AccuracyCoin RAM: 83.45% (unchanged headline; per-suite "APU
    Tests" 6 pass / 3 fail unchanged).
  - Production change: `crates/rustynes-core/src/bus.rs` adds
    `raw_oam_dma_read(src_addr)` helper mirroring the existing
    `dmc_dma_read` chip-select gate: when 6502 bus parked outside
    `$4000-$401F` and OAM DMA source in `$4000-$40FF`, the read
    returns open-bus without firing register side-effects. No
    save-state version bump (pure-dispatch).
  - Audit doc: `docs/audit/session-26-sprint2-iter4-apu-reg-activation-2026-05-23.md`.

- Session 25 Sprint 2 iter 3 (2026-05-23): **LANDED** — Frame Counter IRQ put/get phase axis (Test 7)
  - Tests flipped: AccuracyCoin Frame Counter IRQ internally advances from
    error 7 to error 19 (Tests 7-18 now PASS, residual at Test J).
  - Tests regressed: none.
  - Workspace `--features test-roms`: 541+5 → **545 strict + 5 ignored**
    (+4 new unit tests on the lazy-clear contract).
  - Targeted regression-preserved: `apu_test` 8/8, `apu_mixer` 4/4,
    `dmc_dma` 5/5, `mmc3` 12+2 (B4 invariant), Controller Strobing PASS,
    60-ROM commercial-ROM oracle 60/60.
  - AccuracyCoin RAM: 83.45% UNCHANGED (the Frame Counter IRQ catalog
    entry as a whole still fails — Test J residual — so the per-suite
    "APU Tests: 6 pass / 3 fail" count is preserved; the internal Test 7
    → Test J advance of 12 sub-tests is invisible to the headline).
  - AccuracyCoin framebuffer: 89.83% UNCHANGED.
  - Production change: `crates/rustynes-apu/src/frame_counter.rs` replaces
    `pending_irq_clear: bool` with `irq_flag_clear_cycle: u64` (Mesen2-
    faithful lazy-clear with INVERTED parity vs Mesen2's `(clock & 0x01)
    ? 2 : 1` because RustyNES's `apu_phase` polarity is opposite).
    `crates/rustynes-apu/src/apu.rs` updated to pass `self.cpu_cycle` to the
    frame counter. `APU_SNAPSHOT_VERSION` bumped 1 → 2 with v1 → v2
    migration (per ADR-0003).
  - Test J refinement investigated and rolled back: unconditional flag
    set at FC steps 3-5 broke 4 MMC3 commercial ROMs (`mega_man_3`,
    `tmnt3`, `ninja_gaiden_2`, `tiny_toon_adventures_2`) because
    RustyNES conflates `$4015` bit 6 visibility with the CPU IRQ source
    line. Lifting that conflation is deferred to a future sprint.
  - Audit doc: `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`.
  - Sprint 2 status: 2 of 4 targets LANDED (Controller Strobing + Frame
    Counter IRQ Test 7); 2 INVESTIGATION-ONLY (APU Register Activation +
    DMC).

- Session 24 Phase 4 (2026-05-23): **INVESTIGATION-ONLY** — Implied Dummy + DMC oracle blocked by custom-ROM dependency chain
  - Phase 4 target (`Implied Dummy Reads`): custom ROM `implied-dummy-reads.nes` produces `$0E` (Fail at Test 3, the
    `result_DMADMASync_PreTest` dependency check) on RustyNES; Mesen2 produces `$8A` (Fail at Test 34). The two emulators are
    failing at completely different sub-test depths because the custom ROM has an internal dependency chain on prior
    full-battery tests that the custom-ROM patch can't easily replicate. Wrapper pre-seed of `<$12 = $01` attempted but
    doesn't unblock the chain. Per Decision gate 4A: no clean single-axis hypothesis emerges; deferred.
  - Tests flipped: none. Tests regressed: none.
  - Pass rate: **83.45%** (unchanged post-Phase-3; no chip-stack code change).
  - Workspace `--features test-roms`: **541 strict + 5 ignored** (unchanged).
  - Alternate oracle targets identified for future Phase-4-shape work:
    `Frame Counter IRQ` Test 7 (clean oracle: Mesen2 `$01` PASS vs RustyNES `$1E` Fail Test 7) and
    `APU Register Activation` Test 4 (clean oracle: Mesen2 `$09` PassWithCode(2) vs RustyNES `$12` Fail Test 4).
    Both are in the put/get phase plumbing family. Sprint 1 remains OPEN.
  - Audit doc: `docs/audit/session-24-phase4-implied-dummy-dmc-2026-05-23.md`.

- Session 24 Phase 3 (2026-05-23): **LANDED** — Controller Strobing M2-low-defer write
  - Tests flipped: `APU Tests :: Controller Strobing` (from `[error 4]` to PASS) (+1)
  - Tests regressed: none
  - Pass rate: 82.73% → **83.45%** (108 pass → 109 pass + 7 pass-with-code of 139 assigned)
  - Workspace `--features test-roms`: **541 strict + 5 ignored** (unchanged)
  - Commercial-ROM oracle: 60/60 strict pass (no FNV-1a deltas)
  - B4 invariant preserved (`mmc3_test_2/4` sub-test #2 strict)
  - Commits: oracle infrastructure (Phase 3 Commit 1) +
    fix landing (Phase 3 Commit 2 — `feat(bus): deferred $4016 strobe commit`)
  - Audit doc: docs/audit/session-24-phase3-controller-strobing-2026-05-23.md
  - Production change: `crates/rustynes-core/src/bus.rs` adds
    `controller_write_pending` + `controller_write_value` fields to
    `LockstepBus`; `cpu_write` for `$4016` now buffers the value with
    pending=(1 if even-cycle, 2 if odd-cycle); commit happens at the
    start of `tick_one_cpu_cycle` when pending reaches 0. Mirrors
    Mesen2's `NesControlManager` deferred-write semantics.

- Session 23 Phase 1 + Phase 2 (2026-05-22): ORACLE-UNBLOCK INFRASTRUCTURE
  - Phase 1 (source audit): zero SHALLOW free-win candidates found among
    the 24 failing tests. All v1.0.0-final-eligible failing tests
    (Controller Strobing, Implied Dummy Reads, Frame Counter IRQ #7, APU
    Reg Activation, DMC) categorised MEDIUM-needs-oracle. Phase 1.5
    SKIPPED. Audit doc: `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md`.
    Commit `f2b51c1`.
  - Phase 2 (custom AccuracyCoin sub-test ROMs): SUCCESS. Built 4 custom
    .nes files under `tests/roms/AccuracyCoin/sub-tests/` via
    `scripts/accuracycoin-build/build_sub_test_rom.py` (Python driver
    using upstream NESASM via `wine`). Each ROM boots directly into its
    target test by frame ~30 (vs. ~3000+ in the full battery), bypassing
    the menu and the spinning-loop blocker that paused Mesen2's
    testRunner at frame 1589. Confirmed Mesen2 can now trace these ROMs:
    `controller-strobing.nes` produced a 12-row CSV in ~3 minutes
    walltime (vs. unreachable in the full-battery path). Audit doc:
    `docs/audit/session-23-custom-accuracycoin-sub-test-roms-2026-05-22.md`.
    Commit `1626c7c`.
  - No chip-stack code changed; pass rate unchanged.
  - Pass rate: 82.73% → 82.73%
  - Next: Sprint 1 Phase 1B (Implied Dummy + DMC oracle audit) and
    Sprint 2 Phase 2 (Controller Strobing oracle audit + M2-low-latch
    implementation) now unblocked. Future session picks one of:
    (a) full Mesen2 trace pass of `controller-strobing.nes` +
        cross-diff vs RustyNES + implement the M2-low-latch fix
        under `m2-low-latch` cargo feature flag (Phase 3 of the
        linked-puzzling-sutherland brief);
    (b) full Mesen2 trace of `implied-dummy-reads.nes` + cross-diff
        DMC sidecar + coordinated `cpu-implied-dummy-coordinated`
        feature-flag implementation (Phase 4 of the brief).
    The custom ROMs landed in this session are permanent regression-
    prevention infrastructure — any future AccuracyCoin sub-test fix
    can build a focused ROM via the same script with a single
    `--suite N --test M` invocation.

## Reference

- `to-dos/phase-6-v1.0.0-final/overview.md` — sprint priority +
  validation gauntlet definition.
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` — the
  cascade analysis that informed this backlog.
- `to-dos/ROADMAP.md` — overall project phase plan; T-60-005 is the
  v1.0.0 final tag ticket.
