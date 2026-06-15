# Sprint 2 — APU put/get phase plumbing

**Phase:** 6 — v1.0.0 final
**Status:** **4 of 4 LANDED (Frame Counter IRQ whole-test PASS via iter 5 split)**
as of Session-26 (2026-05-23). The "Frame Counter IRQ" entry was
previously LANDED via iter 3 for the put/get phase axis (Test 7), with
the remaining Tests J/K/L deferred as a separate field-conflation axis.
Iter 5 resolves that conflation, flipping the AccuracyCoin catalog
entry from FAIL to PASS. The DMC [error 21] entry remains
INVESTIGATION-ONLY and Sprint 1's blocked oracle path stays the
limiting factor there.

- **Controller Strobing** — LANDED via v1.0.0-final brief Phase 3
  (`docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`).
- **Frame Counter IRQ #7** — LANDED via Session-25 Sprint 2 iter 3
  (`docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`).
  The Test 7 put/get phase axis is now closed via a Mesen2-faithful
  lazy-clear schedule on `crates/rustynes-apu/src/frame_counter.rs`.
  Internal advance from Test 7 to Test J (12 sub-tests further).
  The Test J/K/L conflation residual is closed by iter 5 below.
- **Frame Counter IRQ J/K/L** — LANDED via Session-26 Sprint 2 iter 5
  (`docs/audit/session-26-sprint2-iter5-frame-counter-irq-split-2026-05-23.md`).
  Separates `FrameCounter::irq_flag` ($4015 bit 6 visibility) from
  the new `FrameCounter::irq_line_active` (CPU IRQ source driver),
  mirroring Mesen2's `_irqFlag` vs `IRQSource::FrameCounter` split.
  The custom Frame Counter IRQ ROM result advances all the way from
  `$4E = Fail Test J` to `$01 = PASS` (Tests J/K/L/M/N/O all PASS).
  AccuracyCoin catalog entry `APU Tests :: Frame Counter IRQ`flips
  FAIL → PASS. The 4 MMC3 commercial canary ROMs (`mega_man_3`,
  `tmnt3`,`ninja_gaiden_2`,`tiny_toon_adventures_2`) that broke
  under Session-25's failed Test J refinement ALL strict-pass under
  the split. Save-state v2 → v3 with`irq_line_active = irq_flag`
  migration.
- **APU Register Activation Test 4** — LANDED via Session-26 Sprint 2
  iter 4
  (`docs/audit/session-26-sprint2-iter4-apu-reg-activation-2026-05-23.md`).
  The OAM-DMA APU-chip-select gate now mirrors the existing
  `dmc_dma_read` gate: when the 6502 address bus (parked at
  `dma_halt_addr` during DMA) is outside `$4000-$401F` and the OAM
  DMA's source page maps inside that range, reads return the
  open-bus latch with no register side-effects. Internal advance
  from Test 4 to Test 6 (Tests 4 + 5 PASS, residual at Test 6);
  the AccuracyCoin catalog-headline metric is unchanged because the
  APU Register Activation entry as a whole still fails (Test 6
  residual). Test 6 depends on the conflict-path semantics for the
  halted_addr-IN-$4000-$401F case (the Test 5 wacky JSR-$3FFE setup
  parks the 6502 bus AT `$4001`); that's a separate axis deferred
  to a future sprint.
- **DMC [error 21]** — INVESTIGATION-ONLY (also overlaps Sprint 1).

**Previous status:** INVESTIGATION-ONLY iteration 1 (Session-22, 2026-05-22).
Sprint remains OPEN. Mesen2 cross-reference (`Core/NES/APU/ApuFrameCounter.h`

- `Core/Shared/BaseControlDevice.cpp`) and AccuracyCoin sub-test
architectural analysis (Controller Strobing test #102 + Frame Counter
IRQ test #97 + APU Register Activation test #101 + DMC test #100)
landed in `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md`.
The production change is **deferred on the same Mesen2 oracle wall-
time blocker** that blocks Sprint 1 Phase 1B — without a Mesen2 trace
showing the exact CPU cycle of the put/get-sensitive observable
behavior (e.g. the cycle on which `$4015` reads observably clear the
frame counter IRQ flag), proceeding to a production fix risks the same
Session-19 / 20-style cascade revert against the load-bearing
`apu_test/*` (8/8 strict), `apu_mixer/*` (4/4 strict), and
`dmc_dma_during_read4` (5/5 strict) surfaces.

The audit doc documents a precise single-axis hypothesis for the
Controller Strobing failure (Test 4: latch must fire on M2-low boundary
where strobe transitions 0→1 — RustyNES currently latches on rising
edge regardless of M2 phase). Frame Counter IRQ #7 hypothesis is less
precise without an oracle trace pinpointing the wrong cycle.

**Predecessor / shared blocker:**
`docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md` (Mesen2
wall-time blocker).

**Cascade risk:** **MEDIUM**.

## Target tests (4)

- `APU Tests :: Frame Counter IRQ #7`
- `APU Tests :: DMC [error 21]`
- `APU Tests :: APU Register Activation [error 4]`
- `APU Tests :: Controller Strobing [error 4]`

Estimated yield: **+1 to +3 AccuracyCoin tests** (with possible
side-flips into Sprint 1's Implied Dummy Reads if Sprint 1 didn't
close it cleanly).

## Hypothesis

APU register writes (`$4000-$401F`) take effect on either the "put"
cycle or the "get" cycle of the 6502 read-modify-write pair. The
distinction matters for:

- Frame counter IRQ flag clearing on `$4015` reads.
- `$4017` (frame counter mode) writes that race with frame-counter
  internal clocking.
- `$4016 / $4017` controller strobe transitions (the strobe takes
  effect on a specific bus phase).
- DMC register writes that interact with DMC DMA mid-fetch.

RustyNES currently emits APU writes at end-of-CPU-cycle uniformly.
Canonical NES behavior per nesdev (`APU page` + `Frame_Counter` page)
alternates by put/get phase: the M2 boundary divides each CPU cycle
into a "put" half (writes commit) and a "get" half (reads sample).
The B1/B2/B3 infrastructure already plumbed `M2Phase::Low/High` and
`LockstepBus::current_m2_phase()`; this sprint extends the convention
to APU register access.

## Implementation surface

- `crates/rustynes-cpu/src/cpu.rs` — classify each opcode cycle by put/get
  (already partially tracked via the M2 boundary; this sprint surfaces
  it at the bus layer).
- `crates/rustynes-core/src/bus.rs` — extend `service_dmc_dma` and the
  `cpu_write` dispatch to thread put/get through to APU. Use the
  existing `M2Phase` accessor.
- `crates/rustynes-apu/src/lib.rs` — split `cpu_write` into
  `cpu_write_put` and `cpu_write_get` (or accept an `M2Phase`
  argument). Update internal channel state machines to commit on
  put-phase only.
- `crates/rustynes-apu/src/frame_counter.rs` (if a separate module exists)
  — re-anchor the frame-counter IRQ flag clearing on the M2-phase
  boundary, not at end-of-cycle.
- `crates/rustynes-apu/src/dmc.rs` — DMC DMA fetches sample on get-phase;
  align with Sprint 1's coordinated DMC DMA scheduler change if it
  landed.

All under feature flag `apu-put-get-phase` (default off).

## Sprint plan

### Step 1 — Mesen2 cross-reference

Read Mesen2's `Core/NES/NesApu.cpp` and `Core/NES/FrameCounter.cpp` for
the put/get convention. Capture findings in
`docs/audit/sprint-2-mesen2-cross-reference.md`.

### Step 2 — Unit tests (before any production change)

- `crates/rustynes-apu/tests/` — new test exercising the `$4015` read +
  frame-counter IRQ flag clearing on put vs get cycle.
- Same for `$4017` writes during DMC DMA halt.
- Same for `$4016` controller strobe transitions.

### Step 3 — Production code change

Land the put/get split in `crates/rustynes-apu/src/lib.rs` + the bus
plumbing. Production code is gated on the feature flag until validation
passes.

### Step 4 — Validation gauntlet

Per `to-dos/phase-6-v1.0.0-final/overview.md`. Special attention:

- `apu_test/*` (8 strict): every sub-test. The frame counter
  sub-tests are especially sensitive to put/get phase.
- `apu_mixer/*` (4 strict): mixer behavior unchanged (no put/get on
  the mixer itself).
- `dmc_dma_during_read4/*` (5 strict): DMC DMA timing.
- `cpu_dummy_writes_oam` (1 strict): OAM DMA put/get sensitivity.
- The 4 target AccuracyCoin tests flip.

### Step 5 — Land OR rollback

Per Sprint 1 land/rollback discipline. Audit doc:
`docs/audit/sprint-2-apu-put-get-phase-N.md`.

## Cascade-risk callouts

1. Frame-counter IRQ timing is the load-bearing axis for both this
   sprint AND the C1 IRQ-timing axis (Sprint 5). Changes here that
   shift the frame-counter IRQ assertion cycle may cascade into the
   `cpu_interrupts_v2/{2,3,5}` tests — could be positive (flips the
   tests) or negative (regresses other surfaces). Run the full C1
   trace cross-diff (`scripts/irq_trace_cross_diff.py --svc`) after
   any change.
2. The Sprint 1 cascade (DMC DMA gating on bus-quiet cycles) shares
   surface with this sprint. If Sprint 1 closed it via option (a) or
   (b), this sprint's changes must be consistent with that choice.
3. `$4015` read open-bus behavior interacts with the cycle-2 dummy
   from Sprint 1. The reads no longer update the open-bus latch
   (Phase D3 fix) — that convention must be preserved.

## Estimated effort + yield

- **Effort:** 2-3 days (more research-heavy than Sprint 1; the put/get
  convention is poorly documented and Mesen2 is the only reliable
  oracle).
- **Yield:** +1 to +3 AccuracyCoin tests.

## References

- nesdev `APU Frame Counter` page
- nesdev `APU` page (register write semantics)
- Mesen2 `Core/NES/NesApu.cpp` + `Core/NES/FrameCounter.cpp`
- `docs/adr/0002-irq-timing-coordination.md` Phases B1/B2/B3 (the
  `M2Phase` plumbing this sprint extends)
- `crates/rustynes-cpu/src/scheduler.rs` (`M2Phase::Low/High`)
- `crates/rustynes-core/src/bus.rs` `LockstepBus::current_m2_phase()`

## Exit criterion

- AccuracyCoin pass rate increases (target +1 to +3 tests).
- No regressions in any of the 10 validation gauntlet gates.
- If pass rate reaches ≥ 90% after this sprint, jump to v1.0.0 final
  tag. Otherwise proceed to Sprint 3.
