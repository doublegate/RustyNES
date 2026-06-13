# Phase 8 — v1.2.0 Accuracy Residuals

**Goal:** Close the v1.1.0 → v1.2.0 accuracy gap per the v2.0.0 release plan
(`/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md`)'s
v1.2.0 milestone, with the DMC scheduler refactor as the centrepiece.

**Exit criterion:** v1.2.0 tag landed with the `dmc-get-put-scheduler`
parallel-implementation in place (default-off), the equivalence harness
shipped, and the AccuracyCoin DMA cluster matching v1.1.0 baseline at
>= 6/10 under `--features dmc-get-put-scheduler`. Workspace tests
preserved (599+6 ignored default; 600+7 ignored feature-on). No
regression to 60-ROM commercial oracle, sacred trio, or B4 invariant.

**Status:** **CLOSED.** v1.2.0 tag landed 2026-05-24 closing Sprint
3.1 → 3.2 → 3.3 → 3.4 iter 1+2 → 3.5 → iter 3 across 10 commits.
All 10 validation gauntlet gates green.

## Sprint Index

- [Sprint 3 — DMC get/put scheduler parallel implementation](sprint-3-dmc-get-put-scheduler.md)

## Workstreams

### v1.2 DMC scheduler refactor

The v1.1.0 → v1.2.0 release plan called for "accuracy residuals
(everything except C1)" with a target trajectory of 90.65% → 97%
AccuracyCoin. The Sprint 2.3 Step 3 audit (`docs/audit/sprint-2.3-
step-3-iter-1-2-cooldown-empirical-2026-05-25.md`) empirically
established that the planned DMC compensating-delay recalibration is
structurally insufficient — the scheduler model itself needs
refactoring to Mesen2's canonical get/put cycle alternation
(`NesCpu.cpp:399-447`).

ADR 0007 documents the resulting decision: parallel-implementation
pattern, feature flag default-off, promotion deferred to v1.6
(fallback) or v2.0 (planned). This phase scopes the v1.2 deliverables
of that ADR.

### Cross-phase dependencies

- ADR 0007 [DMC get/put scheduler](../../docs/adr/0007-dmc-get-put-scheduler.md)
  is the architectural contract for this phase.
- Phase 7 (Nesdev accuracy hardening) continues with its Sprint 2
  ticket T-72-004 ("DMC DMA side-effect bracket audit") which this
  phase materially advances.
- v2.0.0 Sprint A (master-clock-precise scheduling refactor) will
  promote `dmc-get-put-scheduler` to default-on AND fold the parity
  source into master-clock derivation, removing the feature flag.

### Out of scope

- The 4 remaining AccuracyCoin DMA-cluster tests (`DMA + $4015 Read`,
  `DMC DMA + OAM DMA`, `Explicit DMA Abort`, `Implicit DMA Abort`)
  are deferred to Sprint 3 iter 3+ (v1.2.x patch series) OR
  absorbed into v2.0 Sprint A. They require either the DMC abort
  path port to the get/put model OR the master-clock refactor's
  natural absorption.
- The C1 IRQ-timing residuals remain v2.0 work per ADR 0002.
- Other v1.2 accuracy residuals (sprite-eval, PPU misc, etc.) are
  out of scope for this phase — they get separate phases as they
  are tackled.

## Dependencies

- v1.1.0 tag landed (2026-05-25).
- ADR 0007 written (this phase, Sprint 3.5).
- `docs/STATUS.md` remains the source of truth for current pass
  counts.
