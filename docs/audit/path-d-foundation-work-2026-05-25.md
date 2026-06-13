# Path D Foundation — Sprint 2.2 Step 1 dive + v1.2.0 Honest Trajectory

**Date:** 2026-05-25 (after Sprint 2.3 Step 3 iter 1+2 audit; after
Sprint 2.5 + 2.1 closure audits)
**Path:** D — Sprint 2.2 (PPU misc + Cascade-A re-baseline auth) +
Sprint 2.3 Step 3 (Mesen2 DMC trace pairing) combined.
**Outcome:** Diagnostic + reconnaissance + foundation-work audit.
Sub-test ROM reproducer confirmed for the highest-tractability
Sprint 2.2 target (`Stale BG Shift Registers` T3); code-path
inspection identifies the missing "unused NT-read at 241-248"
semantic. **Single-session attempts on this surface are too
speculative for the Cascade-A risk profile**: any change to BG
shifter persistence is observable in the 60-ROM commercial oracle.
Multi-day Mesen2 BG-pipeline trace pairing is the next step.

---

## Sprint 2.2 Step 1 — sub-test ROM reproducer confirmed

Ran `tests/roms/AccuracyCoin/sub-tests/ppu-misc-stale-bg-shift-regs.nes`
against `Nes::run_frame` via `validate_sub_test_rom`:

```
rom=…/ppu-misc-stale-bg-shift-regs.nes  addr=$0483
final=0x0E  first_set_frame=Some(71)  interpretation=Fail
```

Matches Session-28's documented `Fail at Test 3 (first set frame 71)`
empirical state exactly. The sub-test ROM is a **clean oracle** —
boots to the target test deterministically within 71 frames.

## Code-path inspection — what the test expects vs what we model

Per Session-28's `Stale BG Shift Registers Test 3` description
(audit lines 164-198):

The test:
1. Sets sprite zero at `Y=$06, tile=$C8, attr=$03, X=$00`.
2. Sets BG tile `$C8` (solid white) at `$2C00`.
3. Syncs to scanline 0 dot 1.
4. Stalls 767 + 3 nops → arrives at scanline 6 mid-HBlank.
5. `LDA #0 / STA $2001` — disables rendering.
6. Stalls until HBlank ends (~211 PPU cycles).
7. `LDA #$18 / STA $2001` — re-enables rendering.
8. Expects sprite-zero hit because both (a) the sprite zero
   shifter was preserved across the disabled window AND (b) the
   BG shift registers retained `11111111 00000000` from the
   unused NT read at dot 241-248 of the previous scanline.

Inspection of `crates/nes-ppu/src/ppu.rs`:

* **BG-shifter freeze on disable IS modeled** (line 1273-1278):
  ```rust
  if visible && (1..=256).contains(&self.dot) {
      self.emit_pixel();
      if render_line && rendering {
          self.shift_bg();   // only shifts when rendering enabled
      }
  }
  ```
  When `$2001` disables rendering (`rendering = false`), the shift
  is gated off. The shifter freezes at its current value. ✓
* **Unused NT-read at dot 241-248 is NOT modeled.** The audit
  describes the test relying on the BG-shifter latching
  `11111111 00000000` from a specific dot-window NT read. Our
  fetch loop at lines 1192-1230 runs the canonical 8-cycle fetch
  group at dots 1..=256 (and dots 321..=336 pre-fetch), but does
  NOT have a separate "unused NT-read at 241-248" code path. The
  description "unused NT-read at the previous scanline's dot 241-
  248" is ambiguous: dots 241-248 ARE within the visible-region
  fetch window, so fetches DO happen there, but the test expects
  a SPECIFIC bit pattern in the shifter that our normal-fetch
  output doesn't produce.

The two flaws "partially compensate" per Session-28 — fixing either
alone diverges; both need coordinated modeling.

## Why this can't be a single-session empirical fix

Per Session-28's tractability rationale (the "EXTREME — touches
the same BG-pipeline surface that Cascade A (commit `086ce4d`)
just rebaselined the 60-ROM commercial oracle for" warning):

* The BG-shifter pipeline is the load-bearing 1-pixel-position
  surface in the 60-ROM oracle. Cascade A (commit `086ce4d`,
  Session-8) shifted framebuffer FNV-1a hashes 1 column right on
  ALL 60 ROMs — a documented, user-authorized re-baseline.
* A follow-on tweak here is GUARANTEED to shift the framebuffer
  hashes again, requiring re-baseline #2 to the same oracle.
* The new BG-shifter behavior must be EMPIRICALLY VALIDATED
  against Mesen2's BG-pipeline at sub-PPU-dot precision via the
  `ppu-state-trace` infrastructure (ADR-0005) + the Mesen2
  `EventType::PpuCycle` Lua callback (v1.0.0 Phase 0).

Without a Mesen2 BG-pipeline trace, any code change to
`reload_bg_shift_regs` / `shift_bg` / the fetch loop is GUESSWORK
at the empirical level. Per Sprint 2.3 Step 3's iter 1+2 audit
above, single-axis guess-and-check on a multi-axis problem
produces no progress — that finding generalizes.

## What Path D actually requires

### Sprint 2.2 (PPU misc, 5 of 6 attackable; `$2004 Stress` excluded)

Per Session-28's tractability table, all 5 attackable targets need
either:

* **Net-new state machines** (BG-shifter retention, per-sprite
  counter mode, pre-render-as-scanline-5 logic, PPU DATA latency
  state machine) — each is 2-3 days of careful surgery with
  per-write-site FSM-mid-cycle-clobber audits
  (`memory/feedback_emulator_fsm_mid_cycle_clobber.md`)
* **Per-PPU-cycle PPUMASK pipelining** (for `BG Serial In`) —
  cascades into `ppu_vbl_nmi/*` 10/10 strict-load-bearing
* **Cascade-A re-baseline** of the 60-ROM commercial oracle on
  every commit (insta snapshot regen + screenshot regen)

Estimated time: 2-3 weeks of focused work. Each sub-step lands
under its own `accuracycoin-*` feature flag and gauntlet-validates
before the next.

### Sprint 2.3 Step 3 (DMC scheduler trace pairing)

Per Session-20 + this session's iter 1+2 audit, this needs:

* Build a per-cycle DMC-DMA trace fixture (similar to
  `irq_trace_fixture.rs` template)
* Cross-compare against Mesen2 running the `Implicit DMA Abort`
  test ROM via `scripts/mesen2_*_trace.lua` Lua hooks
* Identify which specific cycle-offsets Mesen2 lands DMA on vs
  RustyNES for EACH `Key1/Key2/Key3` X-iteration of the test loop
* Adjust the 4 compensating delays based on trace diff, not
  guess-and-check
* AND: investigate the `JMP $400F` PC-arithmetic surface that
  the target test also depends on (Session-20 Finding 3 epilogue)

Estimated time: 2-4 focused sessions.

### Combined estimate for Path D

3-5 weeks of focused work (Session-20 + Session-28 cross-confirmed
estimates), with **two independent re-baselines** of the 60-ROM
commercial oracle, on the BG-pipeline surface (already once-
re-baselined under Cascade A) and the DMC scheduler surface.

## Honest single-session contribution

This session's productive contribution:

1. **Sub-test ROM reproducer confirmed** for `Stale BG Shift Regs`
   — produces `Fail (code 3) at frame 71`, matching Session-28's
   documented state. Confirms the cascade hasn't drifted since
   2026-05-23.
2. **Code-path inspection** identifies that BG-shifter freeze IS
   modeled but unused-NT-read semantics ARE NOT.
3. **Sprint 2.3 Step 3 cooldown experiments** (iter 1+2 in the
   prior audit) confirm single-axis guess-and-check is insufficient
   for the DMC scheduler cascade either.
4. **Cross-confirmation** that the v2.0.0 plan's Sprint 2.2 + 2.3
   AccuracyCoin estimates (+1 to +7) require multi-day Mesen2-
   trace-pairing work that exceeds single-session scope.

## Recommended v1.2.0 closure

Given:
- Sprint 2.1 CLOSED at v1.1.0 baseline (already-passing)
- Sprint 2.2 NOT TRACTABLE at single-session scope; needs
  Mesen2 BG-pipeline trace pairing + 2-3 weeks of cascade-
  careful surgery + Cascade-A re-baseline #2
- Sprint 2.3 Step 1+2 LANDED behind feature flag (default-off,
  no regression)
- Sprint 2.3 Step 3 NOT TRACTABLE at single-session scope;
  needs Mesen2 DMC trace pairing + multi-axis recalibration
- Sprint 2.4 v2.0 work (C1 axis)
- Sprint 2.5 CLOSED at v1.1.0 baseline (already-un-ignored)

**Recommended v1.2.0 tag at AccuracyCoin 90.65%** — consolidates
the audit + scaffolding work without invoking the multi-day
trace-pairing investigations. The v1.2.0 release notes can
honestly describe:

- 4 of 5 sprints reconnaissance-closed via audit
- Sprint 2.3 Steps 1+2 feature-flagged for future Step 3 close
- AccuracyCoin baseline preserved at 90.65% (no regression)
- 6 comprehensive `docs/audit/sprint-2.*.md` audit docs added
  for future-session foundation
- v1.1.0 → v1.2.0 mainly documentation + scaffolding milestone

The two re-baseline-gated investments (Sprint 2.2 Mesen2 trace +
Sprint 2.3 Step 3 DMC trace) can be scheduled for **v1.3.0** as
a focused accuracy-pass milestone, or rolled into v2.0 alongside
the master-clock refactor (which closes them naturally via the
C1 axis closure).

## Cross-references

- Sprint 2.1 closure: `sprint-2.1-sprite-eval-closure-2026-05-25.md`
- Sprint 2.2 recon: `sprint-2.2-ppu-misc-recon-2026-05-25.md`
- Sprint 2.3 recon: `sprint-2.3-implied-dummy-dmc-recon-2026-05-25.md`
- Sprint 2.3 Step 3 iter 1+2:
  `sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`
- Sprint 2.4 iter 1 rollback:
  `sprint-2.4-iter1-oam-dma-conflict-mirror-rollback-2026-05-25.md`
- Sprint 2.5 closure: `sprint-2.5-commercial-rom-closure-2026-05-25.md`
- Predecessor for Sprint 2.2 surface: Session-28
  (`session-28-sprint4-ppu-misc-residuals-2026-05-23.md`)
- Predecessor for Sprint 2.3 surface: Session-20
  (`session-20-sprint1-dmc-abort-investigation-2026-05-22.md`)
- Predecessor for BG-pipeline Cascade A:
  `cascade-a-investigation-2026-05-19.md`
- Sprint 2.3 Steps 1+2 commit: `1e1d2cf` on `origin/main`
- Existing PPU-state-trace infra: `crates/nes-ppu/src/state_trace.rs`
  + `ppu-state-trace` cargo feature
- Existing IRQ trace infra: `crates/nes-core/src/irq_trace.rs` +
  `irq-timing-trace` cargo feature
- Mesen2 BG-pipeline reference: `Core/NES/NesPpu.cpp::LoadTileInfo`
  + `ProcessScanlineImpl`
- Mesen2 DMC reference: `Core/NES/NesApu.cpp` DMC fetch path

## Workspace state at end of this session

- 599 strict pass + 5 ignored + 6 ignored-doctest = preserved
  from v1.1.0
- AccuracyCoin 90.65% (126/139) preserved
- 60-ROM commercial oracle 60/60 preserved (with
  `--features test-roms,commercial-roms`)
- Sacred trio SMB / Excitebike / Kid Icarus PAL preserved
- B4 invariant preserved (first MMC3 IRQ at cycle 1,370,110 /
  scanline 0 / dot 257)
- All 10 v1.1.0 validation gates green
