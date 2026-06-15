# Sprint 2 - CPU, DMA, And Internal Bus Closure

**Goal:** close the CPU/Bus/APU timing residuals called out by the Nesdev audit
and AccuracyCoin.

## Tickets

- [x] **T-72-001 - C1 IRQ-sample-timing bundle.** Revisit the coordinated
  CPU/Bus/PPU IRQ sample-point work as a single change. Target
  `cpu_interrupts_v2/{2,3,5}` and `mmc3_test_2/4-scanline_timing` sub-test #3.
- [x] **T-72-002 - NMI hijack and BRK vector evidence.** Add trace-backed tests
  for NMI hijacking BRK/IRQ without regressing stack-pushed B-flag semantics.
- [x] **T-72-003 - Internal-vs-external bus model.** Separate CPU internal data
  bus effects from CPU external open bus, PPU `_io_db`, APU `$4015`, controller
  reads, and mapper bus conflicts. Target SH*/TAS/LAS/XAA AccuracyCoin
  residuals.
- [x] **T-72-004 - DMC DMA side-effect bracket audit.** Revalidate load vs
  reload DMA get/put timing and repeated reads of `$2007`, `$4015`, `$4016`,
  and `$4017`. Keep blargg `dmc_dma_during_read4` strict green.
- [x] **T-72-005 - Power-on randomization mode.** Add a developer option that
  randomizes RAM, relevant latches, CPU/PPU phase, and DMA get/put phase while
  preserving deterministic seeded mode for CI and save-state tests.
- [x] **T-72-006 - `$4015` open-bus semantics.** Document and test that `$4015`
  is internal to the CPU/APU package and should not blindly refresh external
  open bus.

## Exit Checklist

- [x] `cpu_interrupts_v2` residuals have updated trace evidence (C1 axis,
  deferred to v2.0 master-clock — `docs/audit/phase-7-sprint-2-cpu-dma-bus-2026-05-24.md`
  - Session-29 conclusion).
- [x] AccuracyCoin CPU/APU/internal-bus residual list documented; the
  completable items landed as coverage/guards (the residual *fixes* are the v2.0
  master-clock/internal-bus rework). AccuracyCoin held at 90.65%.
- [x] No commercial-ROM oracle regressions (additive tests + opt-in constructor).

**Sprint 2 outcome (v1.5.0):** T-72-005 power-on randomization (opt-in seeded),
T-72-002 NMI/IRQ B-flag + vector tests, T-72-006 `$4015` open-bus guard landed.
T-72-001 (C1), T-72-003 (SH\* internal-bus fix), T-72-004 (DMC get/put
completion) documented as deferred to v2.0. See the Sprint 2 audit doc.
