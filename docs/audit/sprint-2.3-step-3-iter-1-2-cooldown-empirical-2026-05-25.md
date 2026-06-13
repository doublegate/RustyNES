# Sprint 2.3 Step 3 — Iter 1+2: dmc_dma_cooldown empirical adjustment

**Date:** 2026-05-25 (post-Step-1+2 landing under feature flag)
**Target:** Flip `APU Registers and DMA tests :: Implicit DMA Abort
[error 2]` (the cascade sentinel) back to PASS under
`--features cpu-implied-dummy-reads`, ideally also flipping
`CPU Behavior 2 :: Implied Dummy Reads [error 3]` simultaneously.
**Outcome:** Both single-axis adjustments (`dmc_dma_cooldown ±1`)
attempted — **neither sufficient**. The cascade is multi-axis per
Session-20's diagnosis; single-delay recalibration cannot close it.

---

## What was tried

The DMC scheduler in `crates/nes-apu/src/apu.rs` has two
`dmc_dma_cooldown` setters that fire after a DMC DMA completes:

* Line 343 (post-load complete): `dmc_dma_cooldown = 4`
* Line 377 (post-get-tick complete): `dmc_dma_cooldown = 5`

Per Session-20 Finding 3, these "compensating delays" were tuned
to RustyNES's pre-Sprint-2.3 bus-quiet implied-cycle-2 baseline.
Adding the implied dummy reads converges to canonical bus-active
patterns, but the cooldowns don't auto-recalibrate.

### Iter 1: `-1` (cooldown 4→3, 5→4)

Hypothesis: dummy read makes DMA halt land 1 cycle earlier;
shortening cooldown by 1 keeps the canonical post-DMA timing.

Result with `--features cpu-implied-dummy-reads`:
- `Implicit DMA Abort [error 2]`: **STILL FAILING** (cascade
  unchanged)
- `Implied Dummy Reads [error 3]`: STILL FAILING (target
  unchanged, as expected — DMC scheduler is the cascade axis,
  not the target axis)
- 14 total failures (vs 13 at v1.1.0 baseline). Net: no
  improvement.

### Iter 2: `+1` (cooldown 4→5, 5→6)

Hypothesis: the +1 cycle from dummy read pushes the DMA halt
later; lengthening the cooldown maintains the canonical
"next-DMA-eligible" gap.

Result with `--features cpu-implied-dummy-reads`:
- `Implicit DMA Abort [error 2]`: STILL FAILING (cascade
  unchanged)
- `Implied Dummy Reads [error 3]`: STILL FAILING
- 14 total failures. Same as Iter 1.

## Both iterations reverted

`apu.rs` restored to baseline. Verified `--features test-roms`
alone (no implied-dummy feature) still produces the 13-failure
baseline: AccuracyCoin 90.65% (126/139) preserved.

## What Session-20 said would happen

Session-20's Finding 3 (2026-05-22):

> RustyNES's DMC DMA scheduler has **multiple compensating
> delays** (`dmc_dma_cooldown = 4` after a load, `5` after an
> early-deliver get; `dmc_abort_delay_for(2) = 2`, `(3) = 3`)
> that were also tuned to the non-canonical baseline. The
> combined system is now off-by-one in some bus-phase corner
> cases.

The phrasing "multiple compensating delays" and "combined system"
makes it explicit: **no single-delay adjustment can close the
cascade**. The fix requires recalibrating multiple delays together,
ideally driven by a Mesen2 cycle-precise trace cross-comparison
to identify which specific corner cases shift by which exact
amounts.

## What Session-20 also said about the target test

Beyond the cascade, the `Implied Dummy Reads` test itself doesn't
flip with just the 21-opcode wire-up:

> The target test ALSO did not flip because:
> - The test sequence is `JSR $4011 → DMC DMA → PHA → LDY <$A4 →
>   LDA <$A5 → [opcode] → fetch from $4015`.
> - The DMC DMA halt currently inserts 3-4 cycles in our scheduler
>   AFTER `JSR $4011` (between JSR cycle 6 and PHA cycle 1).
> - For the test to flip, the opcode's cycle-2 dummy read MUST land
>   on `$4015` …
> - The naive fix correctly places the dummy at PC, but the test's
>   earlier `JMP $400F` (line 11860) does some PC arithmetic that
>   the dummy read at PC alone may not satisfy. Need deeper
>   cycle-trace.

So even if Step 3 closes the `Implicit DMA Abort` cascade, the
+1 target gain on `Implied Dummy Reads` is NOT automatic — the
target test brackets PC-arithmetic semantics our simple
`read1(bus, self.pc)` may not satisfy.

## Conclusion + next steps

**Sprint 2.3 Step 3 requires multi-axis recalibration**, not the
single-cooldown empirical adjustment this session attempted.
Specifically:

1. Build a per-cycle DMC-DMA trace fixture (similar to the
   existing `irq_trace_fixture.rs`) that captures the cycle-by-
   cycle DMA halt + service state with the feature ON.
2. Cross-compare against Mesen2 running the same test ROM
   (`tests/roms/AccuracyCoin/sub-tests/apu-implicit-dma-abort.nes`
   if it exists, or the full AccuracyCoin battery's
   `Implicit DMA Abort` window).
3. Identify which specific cycle-offset Mesen2 lands the DMA on
   vs RustyNES, for EACH `Key1/Key2/Key3` X-iteration of the test
   loop.
4. Adjust the multiple compensating delays based on the trace
   diff, not by guess-and-check.

This is **multi-day work** (estimated 1-2 focused sessions on the
Mesen2 trace pairing alone, then 1-2 sessions on the actual
recalibration). Not single-session-tractable.

**Sprint 2.3 status**: Steps 1+2 LANDED (feature flag default-off,
ready for trace-driven Step 3). The +1 AccuracyCoin gain the
v2.0.0 plan estimated for Sprint 2.3 is **not closable** at this
session's effort level. Honest status: deferred to a focused
trace-pairing session.

## v1.2.0 trajectory at session end

| Sprint | Status | AccuracyCoin Δ |
|---|---|---:|
| 2.1 sprite-eval | CLOSED audit | +0 |
| 2.2 PPU misc | NOT YET ATTACKED (EXTREME cascade; re-baseline gate) | TBD |
| 2.3 implied dummy + DMC | Steps 1+2 LANDED behind flag; Step 3 needs trace pairing | +0 (this session) |
| 2.4 APU edge cases | v2.0 work | +0 |
| 2.5 commercial ROMs | CLOSED audit | +0 |

**Honest v1.2.0 closure recommendation:** ship as a smaller-scope
release that consolidates the v1.1.0 + audit work + Steps 1+2
scaffolding, **at AccuracyCoin 90.65% (unchanged from v1.1.0).**
The +1 to +6 test flips the plan budgeted require either:
- Sprint 2.2 (with Cascade-A re-baseline authorization, +1 to +6)
- Sprint 2.3 Step 3 (with deep Mesen2-trace pairing, +1)

Both are multi-day. Pure v1.1.x patch tag closing the audit work
+ committing what's done is the lowest-risk near-term shipment.

## Cross-references

- Sprint 2.3 commit history: `1e1d2cf` (Steps 1+2 feature-flag
  landing) on `origin/main`
- This audit's predecessor: Session-19 (2026-05-22 cascade-revert)
  + Session-20 (2026-05-22 Option A selection)
- The DMC scheduler code: `crates/nes-apu/src/apu.rs`
  - line 343 (`dmc_dma_cooldown = 4` post-load)
  - line 377 (`dmc_dma_cooldown = 5` post-get-tick)
  - lines 103-108 (`dmc_abort_delay_for(2) = 2`, `(3) = 3`)
  - lines 549-561 (`dmc_dma_delay` calibration from `$4015` write
    enable path)
- Mesen2 reference: `Core/NES/NesApu.cpp` DMC fetch path
- Per-cycle trace infrastructure (would-be Step 3 prerequisite):
  `crates/nes-test-harness/src/bin/trace_apu_reg_activation.rs`
  (existing template for similar trace tooling)
