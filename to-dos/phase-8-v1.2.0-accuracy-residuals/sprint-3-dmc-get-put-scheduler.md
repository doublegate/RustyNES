# Sprint 3 — DMC Get/Put Scheduler Parallel Implementation

**Phase:** 8 — v1.2.0 Accuracy Residuals
**Status:** **CLOSED at v1.2.0** (released 2026-05-24).
**ADR:** [0007 — DMC DMA Get/Put Cycle Scheduler](../../docs/adr/0007-dmc-get-put-scheduler.md).

This sprint replaces RustyNES's pre-Sprint-3.2 phase-agnostic DMC
scheduler (a 2-or-3-noop loop + 4 compensating delays) with Mesen2's
canonical get/put cycle alternation model
(`NesCpu.cpp:399-447`), landed behind a default-off feature flag
(`dmc-get-put-scheduler`) using the parallel-implementation pattern.

Sprint sub-tickets and their commit references are listed below
(all merged to `main`).

## Tickets

- [x] **T-83-001 — Sprint 3.1 — feature flag + APU API scaffolding.**
  Add `dmc-get-put-scheduler` cargo feature to `rustynes-apu`, `rustynes-core`,
  `rustynes-test-harness`. Wire `Apu::dmc_need_halt` /
  `Apu::dmc_need_dummy_read` fields (always-present, not
  `#[cfg]`-gated for forward-compatibility); arm them at all 3
  `pending_dmc_dma = true` sites; clear them at the 2 completion
  paths. Add `clear_dmc_need_*` bus-facing API. **No behavior
  change** under any feature config (default-off scheduler still
  uses the old fields). Validated: 599 tests pass + 6 ignored
  preserved.
  - Commit: `e45659b` — `feat(apu): scaffold dmc-get-put-scheduler feature flag + API (v1.2 Sprint 3.1)`

- [x] **T-83-002 — Sprint 3.2 — get/put scheduler in bus.rs**
  (feature-gated, WIP landing). Add a new `service_dmc_dma` path
  under `#[cfg(feature = "dmc-get-put-scheduler")]` that
  implements Mesen2's get/put cycle alternation. The OLD scheduler
  is preserved as the `#[cfg(not(...))]` branch. **AccuracyCoin
  DMA cluster: 1/10 match baseline** at this landing
  (expected-fail per the parallel-impl pattern — the harness then
  drives convergence).
  - Commit: `67ec7db` — `feat(bus): get/put DMC scheduler initial landing — Sprint 3.2 (WIP)`

- [x] **T-83-003 — Sprint 3.3 — parallel-impl equivalence harness.**
  Two artifacts. (a) `crates/rustynes-test-harness/tests/dmc_get_put_
  equivalence.rs` — Rust CI gate; gated on
  `feature = "test-roms"` + `feature = "dmc-get-put-scheduler"`;
  one diagnostic test (`progress_signal`, no asserts) + one strict
  probe (`*_matches_baseline_under_get_put`, `#[ignore]`'d per the
  project's `*_currently_fails` pattern). (b)
  `scripts/dmc_equivalence_harness.sh` — shell-level iteration
  tool; builds `trace_dmc_dma` with both feature configs, runs
  against a ROM corpus, diffs per-CPU-cycle traces.
  - Commit: `a227d7a` — `feat(test-harness): DMC get/put parallel-impl equivalence harness — Sprint 3.3`

- [x] **T-83-004 — Sprint 3.4 iter 1 — DMC parity convention fix.**
  Flip the DMC scheduler's parity from `(cycle & 1) == 1` (the
  OAM-DMA convention) to `(cycle & 1) == 0`. The two DMA paths
  (OAM vs DMC) enter the bus at structurally different points
  relative to `tick_one_cpu_cycle`, putting the get/put phases out
  of step. **AccuracyCoin DMA cluster: 1/10 → 6/10 match.**
  - Commit: `2bc48f8` — `feat(bus): Sprint 3.4 iter 1 — DMC parity convention fix (1/10 -> 6/10)`

- [x] **T-83-005 — Sprint 3.4 iter 2 — OAM-conflict path port.**
  Port `service_dmc_dma_during_oam` to the get/put model so both
  DMC service paths share the same structural shape. Matches
  Mesen2's unified `RunDma` loop (which handles OAM-conflict
  inline rather than via a separate function). **AccuracyCoin DMA
  cluster: 6/10 preserved** (structural completeness; no new
  test flips). The OLD path is preserved as the
  `#[cfg(not(...))]` branch for default-off behaviour.
  - Commit: `b715441` — `feat(bus): Sprint 3.4 iter 2 — service_dmc_dma_during_oam under get/put`

- [x] **T-83-006 — Sprint 3.5 — ADR + sprint docs.**
  - [ADR 0007](../../docs/adr/0007-dmc-get-put-scheduler.md): full
    context + decision + alternatives + consequences.
  - This sprint file + phase-8 overview.
  - ROADMAP.md updated to reflect v1.2 milestone.
  - CHANGELOG entries for 3.1 through 3.4 iter 2.

- [x] **T-83-007 — Sprint 3 iter 3 — DMC abort path port.**
  Port the abort path to Mesen2's cancel-instead-of-continue
  semantics. New `Apu::cancel_dmc_dma()` clears all DMC state
  atomically (matches Mesen2 `processCycle::if(_abortDmcDma)`,
  `NesCpu.cpp:386-390`). Abort detection moved INSIDE both
  `service_dmc_dma` and `service_dmc_dma_during_oam` loops
  (checked at the start of each iteration). `drain_dma` updated
  to skip the pre-service abort call under the feature flag.
  OLD `service_dmc_abort` is preserved as
  `#[cfg(not(...))]`-gated.
  - **AccuracyCoin DMA cluster: 6/10 preserved** (no new test
    flips). The sub-agent research predicted 3/3 flips on the
    assumption that the abort scheduling was reachable for the
    failing tests — empirically that assumption was wrong:
    `schedule_explicit_dmc_abort_if_needed`'s gating
    (`bits_remaining == 1 && sample_buffer.is_some()`) is too
    narrow for these test scenarios.
  - The structural-correctness landing is still the right call:
    the abort path now matches Mesen2 semantics under the new
    model, so any future test that DOES trigger abort under
    `dmc-get-put-scheduler` will behave canonically.
  - Workspace tests preserved: 599+6 / 600+7.
  - Commit reference: see git log for iter 3 commit.

## Deferred to Sprint 3 iter 3+ (or v2.0 absorption)

These 4 AccuracyCoin DMA-cluster tests remain FAILing under
`--features dmc-get-put-scheduler` after Sprint 3.4 iter 2:

| Test | Result | Root cause area |
|---|---|---|
| DMA + $4015 Read | 0x0A (FAIL #2) | $4015 open-bus interaction during DMC service |
| DMC DMA + OAM DMA | 0x06 (FAIL #1) | OAM/DMC interleaving (iter 2 didn't close it; deeper than parity) |
| Explicit DMA Abort | 0x06 (FAIL #1) | DMC abort path (`service_dmc_abort`) NOT yet ported |
| Implicit DMA Abort | 0x06 (FAIL #1) | DMC abort path + abort-completion timing |

Three of the four involve the DMC abort path. The natural next
iteration is **Sprint 3 iter 3**: port `service_dmc_abort` to the
get/put model. The fourth (`DMA + $4015 Read`) needs a focused
investigation into the `$4015` read open-bus latch behaviour
during DMC halt cycles.

**Decision deferred to maintainer at v1.2 tag-time**:
- Option A: ship v1.2 with the 4 deferred and tag the remaining
  work as a v1.2.x patch series.
- Option B: spend one more session on iter 3 (abort path port)
  to attempt closing 7-8/10 before v1.2 ships.
- Option C: leave the harness in place and absorb the closure into
  v2.0 Sprint A's master-clock refactor, which structurally
  refactors the entire scheduler.

The v2.0.0 release plan's v1.2 milestone target was AccuracyCoin
≥ 97%. With the DMC cluster's 4 remaining failures + the C1 axis
(deferred to v2.0) + sprite-eval + PPU misc residuals, that
target was always going to be tight for v1.2; ADR 0007 documents
the realistic v1.2 expectation as "the get/put model lands;
specific test closure is a v1.2.x patch series question."

## Exit Checklist

- [x] ADR 0007 written with full context + alternatives + consequences.
- [x] Equivalence harness in place (Rust CI gate + shell tool).
- [x] AccuracyCoin DMA cluster: 6/10 match baseline under flag.
- [x] Workspace tests preserved: 599+6 (default), 600+7 (feature-on).
- [x] 60-ROM commercial oracle preserved.
- [x] Sacred trio + B4 invariant preserved.
- [x] CHANGELOG entries for each landed iteration.
- [x] **v1.2.0 release tag** (2026-05-24). All 5 release-protocol
  steps completed; CI release-workflow body-clobber bug fixed
  structurally in commit `51ef94a` (removed `body:` and
  `generate_release_notes:` from `softprops/action-gh-release`).

## Cross-references

- [ADR 0007](../../docs/adr/0007-dmc-get-put-scheduler.md)
- [Audit thread](../../docs/audit/) — `path-beta-*.md`,
  `sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`
- [v2.0.0 release plan](/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md)
  — Sprint 2 of the plan (v1.2.0 milestone)
- Mesen2 reference: `Core/NES/NesCpu.cpp::RunDma` (lines 399-447)
  + `processCycle` lambda (lines 384-397). GPL-3.0 structural
  reference only.
