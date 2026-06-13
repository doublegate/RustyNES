# Sprint 2.2 (PPU Misc residuals) — Reconnaissance + v1.2.0 Priority

**Date:** 2026-05-25 (post-v1.1.0 tag, post-Sprint-2.1 closure)
**Sprint scope (per v2.0.0 plan):** Close the 6 PPU Misc.
AccuracyCoin residuals: `Stale BG Shift Registers`,
`Stale Sprite Shift Regs`, `BG Serial In`, `Sprites On Scanline 0`,
`$2004 Stress`, `$2007 Stress`.
**Outcome of this session:** Reconnaissance only. **Sprint 2.2 is the
hardest single cluster of remaining v1.2.0 work** — all 6 targets are
EXTREME-cascade per Session-28. No code change this session.

---

## Current state (v1.1.0 baseline)

AccuracyCoin (RAM) reports 13 failing tests, clustered:

| Cluster | Tests | Sprint scope |
|---|---:|---|
| C1 axis (CPU Interrupts + `$2002 flag timing`) | 4 | v2.0 Sprint A |
| **PPU Misc.** | **6** | **Sprint 2.2 (this audit)** |
| APU edge cases | 2 | Sprint 2.4 |
| CPU Behavior 2 :: Implied Dummy Reads | 1 | Sprint 2.3 |

## Per-test status against Session-28's tractability analysis

Session-28 (2026-05-23, pre-v1.0.0-rc2) classified all 6 PPU Misc tests
as **EXTREME-cascade**. The classification is still valid at v1.1.0:

| Test | Error | Hardware behavior tested | Cascade risk | Tractability score (Session-28) |
|---|---|---|---|---:|
| Stale BG Shift Regs | T3 (`$0E`) | BG-shifter retention across render-disable mid-HBlank | **EXTREME** (Cascade A surface; `086ce4d` precedent) | 2.75 |
| Stale Sprite Shift Regs | T3 (`$0E`) | Per-sprite counter mode (`Halted`/`Counting`) preserved across render-disable | **EXTREME** (sprite-eval FSM; B8b regression `63d8dea` clobbered sacred trio) | 2.00 |
| BG Serial In | T2 (`$0A`) | 2-5 PPU-cycle alignment-dependent PPUMASK delay + BG shifter serial-in semantics | **EXTREME** (cascades into `ppu_vbl_nmi/*` 10/10) | 2.00 |
| Sprites On Scanline 0 | T2 (`$0A`) | Pre-render line treated as scanline 5 for sprite-eval; dot-340-odd-skip composite/RGB | **EXTREME** (sprite-eval FSM; B8b surface) | 1.75 |
| $2004 Stress | T2 (`$0A`) | Per-PPU-dot OAMADDR walk via `$2004` reads | **EXCLUDED** (Sprint-3-axis: Session-9 documented 14-test cascade) | 2.50 |
| $2007 Stress | T2 (`$0A`) | PPU DATA 3-cycle latency + analog ALE-vs-Read feedback | **EXTREME** (Mesen2 itself acknowledges "half the bytes are affected by analogue behavior") | 1.75 |

Tractability score breakdown (higher = safer): the 6 tests scored
1.75-2.75 on a 0-5 scale where Sprint 1 / Sprint 2 surgical fixes
average 4.0+. Session-28 concluded "Sprint 4 INVESTIGATION-ONLY"
with all 6 candidates deferred.

## What changed between Session-28 and now

Nothing on the PPU Misc surface. v1.0.0-final shipped the 3 sprite-eval
flips (Phase 3a/3b) which closed `Arbitrary Sprite zero` +
`Misaligned OAM behavior` + `OAM Corruption` — these are
**different tests** from Session-28's 6 PPU Misc targets.

v1.1.0 added VRC7 OPLL audio (orthogonal to PPU Misc).

The 6 PPU Misc residuals are precisely the same as Session-28
identified, with the same error codes and same diagnostic shapes.

## Recommended Sprint 2.2 execution order

If Sprint 2.2 is attacked, the recommended order (lowest cascade
risk first, highest tractability score):

1. **`Stale BG Shift Registers`** (score 2.75) — **REQUIRES Cascade A
   re-baseline authorization up-front**. The fix shape per the v2.0.0
   plan Step 3c is: add `bg_shift_frozen_at_disable: bool` field that
   latches on `$2001` BG-enable 1→0 during HBlank, snapshot shifter,
   continue updating from the unused NT-read path (currently absent
   at dots 241-248), restore on re-enable. Re-baselines the 60-ROM
   commercial oracle insta snapshots (the Cascade A `086ce4d`
   precedent).
2. **`Stale Sprite Shift Regs`** (score 2.00) — **REQUIRES net-new
   state machine** (`spr_mode: [SpriteCounterMode; 8]`). Touches
   the same FSM surface B8b regression `63d8dea` clobbered SMB /
   Excitebike / Kid Icarus PAL. Risk-mitigation: per-write-site
   audit per `feedback_emulator_fsm_mid_cycle_clobber.md`, sacred-
   trio bisect after every sub-step.
3. **`BG Serial In`** (score 2.00) — **REQUIRES per-PPU-cycle
   PPUMASK pipelining** (2-5 cycle alignment-dependent delay). The
   PPUMASK delay change alone would cascade into every blargg
   `ppu_vbl_nmi/*` test; must verify all 10 stay strict-pass.
4. **`Sprites On Scanline 0`** (score 1.75) — **REQUIRES pre-render-
   as-scanline-5 sprite-eval logic + composite/RGB PPU config
   dimension + dot-340-odd-skip shifter pipe**. Net-new logic
   parallel to the existing `mask_skip_pipe1`.
5. **`$2007 Stress`** (score 1.75) — **REQUIRES PPU DATA state
   machine + sub-PPU-clock analog ALE-vs-Read model**. The lockstep
   scheduler does not currently provide sub-PPU-clock granularity;
   even Mesen2 acknowledges its own analog limits. Likely impossible
   to fully close without v2.0's master-clock-precise scheduling.
6. **`$2004 Stress`** — **EXCLUDED**. Session-9's documented 14-test
   cascade on the eval-base-from-OAMADDR axis means this test cannot
   safely be attempted without re-baselining the full 60-ROM oracle
   AND risking the same cascade Sprint 3 deferred from v1.0.0.

## Re-baseline authorization required

Per the v2.0.0 plan Step 3c:

> The plan flagged Sprint 2.2 as a Cascade A re-baseline candidate.

The v1.0.0 release executed one such re-baseline (Session-8 commit
`086ce4d`). Each re-baseline shifts the 60-ROM commercial-ROM oracle
insta snapshots' framebuffer FNV-1a hashes; the audio FNV-1a +
cumulative-cycle invariants must stay byte-identical. The user
must authorize the re-baseline **before** Sprint 2.2 Step 1
(`Stale BG Shift Registers`) lands.

## Realistic v1.2.0 scope

Per the v2.0.0 plan's original budget (5-7 weeks for v1.2.0 total),
realistic v1.2.0 deliverables given Session-28's tractability:

| Sprint | Effort estimate | Risk |
|---|---|---|
| **2.1 (sprite-eval residuals)** | **CLOSED at v1.1.0** | — |
| 2.2 (PPU misc) | 2-3 weeks for Tests 1+2+3 (Stale BG / Sprite / BG Serial); Tests 4-6 may not close in v1.2.0 | EXTREME cascade for each; re-baseline required |
| 2.3 (Implied Dummy Reads + DMC) | 1 week — Session-19's documented cascade target | EXTREME — `Implicit DMA Abort` regression sentinel |
| 2.4 (APU edge cases) | 1-2 weeks — 2 tests (`DMC`, `APU Reg Activation`) | Moderate — Session-26 made progress on APU Reg Activation |
| 2.5 (6 ignored commercial ROMs) | 1-2 weeks for mapper-026 VRC6b pair structural fix; the other 4 are per-ROM debugging | Moderate — sacred-trio surface |

**Conservative v1.2.0 target: AccuracyCoin 92-94%** (4-6 of 13
remaining failures closed). The v2.0.0 plan's 97% target assumes ALL
6 Sprint 2.2 tests close, which Session-28 categorized as EXTREME-
cascade work potentially exceeding the v1.2.0 budget.

**Recommendation:** Begin Sprint 2.2 with the user authorising the
Cascade A re-baseline up-front, attack `Stale BG Shift Registers`
first (highest tractability score), and bail to Sprint 2.3 / 2.4 if
the cascade blast radius proves larger than budget.

## Next-session work items

1. **User decision required:** Sprint priority for v1.2.0:
   (a) Begin Sprint 2.2 Step 1 with re-baseline authorization
       (highest cascade risk, biggest payoff per AccuracyCoin
       point);
   (b) Skip to Sprint 2.3 (Implied Dummy Reads + DMC coordinated;
       Session-19 documented surface);
   (c) Skip to Sprint 2.4 (APU edge cases — 2 tests, lower
       cascade risk);
   (d) Skip to Sprint 2.5 (6 ignored commercial ROMs — fundamentally
       different surface from PPU/CPU accuracy).
2. **Permanent regression-prevention infrastructure already exists**:
   6 custom AccuracyCoin sub-test ROMs at
   `tests/roms/AccuracyCoin/sub-tests/ppu-misc-*.nes` (per
   Session-28's "Custom-ROM test capture" table), 60-ROM commercial
   oracle insta snapshots, sacred-trio bisect tooling at
   `scripts/regression-bisect/`. All ready for Sprint 2.2 execution.
3. **Sprint 2.2 acceptance criterion** if attacked: at least 3 of 6
   PPU Misc tests flip strict-pass without regressing sacred trio
   (SMB / Excitebike / Kid Icarus PAL); Cascade A 60-ROM oracle
   re-baseline applied with audio + cycle invariants byte-identical.

## Conclusion

Sprint 2.2 is **reconnaissance-complete** as of this audit. The 6
target tests are unchanged from Session-28's analysis, the
tractability landscape is unchanged, and the cascade risk is
unchanged. The next step is user-level prioritization: which
sprint to spend v1.2.0's calendar budget on. The audit doc above
gives the per-sprint cost / payoff trade-off for that decision.
