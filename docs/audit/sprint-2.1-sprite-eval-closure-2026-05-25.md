# Sprint 2.1 (Sprite-eval residuals) — Closure Audit

**Date:** 2026-05-25 (post-v1.1.0 tag)
**Sprint scope (per v2.0.0 plan):** Close residual sprite-eval
AccuracyCoin failures: `$2002 flag timing`, `Misaligned OAM behavior`,
`OAM Corruption`.
**Outcome:** **CLOSED — 3 of 4 targets already passing at v1.1.0 baseline;
the 1 remaining is C1-axis (v2.0 Sprint A).**

---

## Empirical state at v1.1.0 (this commit)

Per `cargo test -p nes-test-harness --features test-roms --release
accuracycoin_pass_rate -- --nocapture`:

```
AccuracyCoin (RAM): total=144 pass=116 pass_with_code=10
                    fail=13 skipped=0 not_run=5
AccuracyCoin (RAM): pass rate = 90.65% over 139 assigned tests
```

All 13 failing tests are **outside Sprint 2.1's scope**:

| Suite | Test | Sprint |
|---|---|---|
| Sprite Evaluation | `$2002 flag timing` [error 1] | **v2.0 Sprint A** (C1 axis per Session-27) |
| CPU Interrupts | 3 tests (Interrupt flag latency, NMI Overlap BRK, NMI Overlap IRQ) | **v2.0 Sprint A** (C1 axis — same architectural surface as `cpu_interrupts_v2`) |
| APU Tests | 2 tests (DMC, APU Register Activation) | **Sprint 2.4** (APU edge cases) |
| PPU Misc. | 6 tests (Stale BG/Sprite Shift Regs, BG Serial In, Sprites On Scanline 0, $2004 Stress, $2007 Stress) | **Sprint 2.2** (PPU misc residuals) |
| CPU Behavior 2 | Implied Dummy Reads [error 3] | **Sprint 2.3** (DMC DMA scheduler) |

The 3 tests Sprint 2.1's plan listed as "residuals to close" are
**already passing** on the v1.1.0 baseline:

| Test | Result address | Value | Status |
|---|---|---:|---|
| `Sprite Evaluation :: Arbitrary Sprite zero` | `0x0458` | `0x01` | PASS (code 0) |
| `Sprite Evaluation :: Misaligned OAM behavior` | `0x045A` | `0x01` | PASS (code 0) |
| `Sprite Evaluation :: OAM Corruption` | `0x047B` | `0x01` | PASS (code 0) |

These were closed during v1.0.0-final's Phase 3a (sprite-eval base
from OAMADDR, commit `e837afa`) and Phase 3b (OAM-corruption row
tracking, commit `941d448`). The plan's "residual" framing was
based on the in-flight v1.0.0-rc state where these tests partially
passed; by v1.0.0 final tag they were fully closed, and v1.1.0
preserves them.

## Why `$2002 flag timing` is deferred to v2.0 Sprint A

Per [Session-27's per-test
tractability table](session-27-sprint3-sprite-eval-residuals-2026-05-23.md),
`$2002 flag timing` Test 1 brackets the **1.875-PPU-cycle M2-low-vs-
M2-high asymmetry** between the VBL-flag clear (bit 7) and the
sprite-flag clear (bits 5 + 6) on `$2002` reads. This is the same
sub-CPU-cycle phase axis that gates the 3 `cpu_interrupts_v2`
strict-ignored tests + `mmc3_test_2/4` sub-test #3 — the v2.0
master-clock-precise scheduling refactor's load-bearing surface.

Per the v2.0.0 release plan (`/home/parobek/.claude/plans/
generate-a-new-plan-snug-starlight.md`) Sprint A7:

> Side-benefit AccuracyCoin closures from the master-clock
> refactor: `$2002 flag timing` (per Session-27 audit, on the
> C1 axis) + any latent dot-precision tests that improve under
> fractional scheduling. Target: AccuracyCoin ≥ 99% (138/139),
> realistically 100% bar the one hardest edge case.

The Sprint 2.1 plan listed `$2002 flag timing` as a target "IF Phase
2's CPU-vs-PPU access-ordering rework flipped this test already";
since v1.2.0 does NOT include that rework, the test stays failing
and rolls over into v2.0 Sprint A's natural closure surface.

## v2.0.0 plan implications

Sprint 2.1 has **0 actionable tests at v1.1.0 → v1.2.0**.
v1.2.0's effective sprite-eval scope is **null**. The plan's
estimated +3 AccuracyCoin tests for Sprint 2.1 is already captured
in v1.0.0's 90.65% baseline (those 3 tests were counted there).

Path forward:
- **Officially close Sprint 2.1** (this audit doc).
- **Move to Sprint 2.2** (PPU Misc residuals — 6 failing tests, the
  largest remaining cluster at v1.1.0).
- **`$2002 flag timing` carries forward** to v2.0 Sprint A7 (a
  side-benefit closure rather than the primary fix surface).
- AccuracyCoin pass-rate target for v1.2.0 stays at ≥ 97% per the
  plan, since the +3 Sprint 2.1 tests are already in the 90.65%
  baseline.

## What to verify before declaring Sprint 2.2 in-flight

- [ ] Confirm Sprint 2.2's 6 failing tests via per-test catalog
      address (this audit's table above lists the 6 — addresses
      can be looked up in `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`).
- [ ] Re-baseline authorisation: the v2.0.0 plan flagged Sprint 2.2
      as a Cascade A re-baseline candidate. Need user authorisation
      up-front per Session-8 `086ce4d` precedent.
- [ ] Sprint 2.2's net-new state machines (BG-shifter retention,
      per-sprite counter mode) carry FSM-mid-cycle-clobber audit
      risk per `memory/feedback_emulator_fsm_mid_cycle_clobber.md`.

## Conclusion

Sprint 2.1 of v1.2.0 is **CLOSED** at v1.1.0's release commit. No
production code changes are required for this milestone closure;
this audit doc serves as the closure record. The next actionable
sprint is **Sprint 2.2 (PPU Misc residuals)**, which is the largest
single cluster of failing tests at v1.1.0 (6 out of 13).
