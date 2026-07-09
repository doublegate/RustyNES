# ADR 0002 — Coordinated CPU/Bus/PPU IRQ-Sample-Timing Rework

**Status:** **CLOSED — by-design-permanent** (v2.1.0 "Fathom" F5.0, 2026-07-09).
Formerly "by-design-deferred beyond v2.0.0"; the v2.1.0 instrumentation-first
review (F5.0) closed it. 21+ documented rollbacks (the v2.0.0
one-clock/every-cycle-bus-access promote landed the R1 substrate itself, and a
dedicated two-session bounded-effort campaign on the promoted core added 4 more
falsified levers) plus the F5.0 review of the campaign ground-truth establish
that the residual is a **differential 1-dot deficit that is structurally
invariant to every global phase lever** and that no single-axis lever exists
that shifts the mapper-IRQ observation path relative to the `$2002` path without
a scheduler-substrate change — which would risk the sacred AccuracyCoin 141/141
for a bracket with **zero production-ROM impact**. The residual
(`mmc3_test_2/4` sub-test #3 + siblings) therefore stays `#[ignore]`'d
**permanently by design** (not as an open TODO). See §"Decision update
(2026-07-09, v2.1.0 Fathom F5.0)" at the end for the full closing argument.
**Date:** 2026-05-10
**Author:** RustyNES maintainers
**Numbering:** 0002. ADR 0001 (mapper dispatch) will be written after
Track B6 benches land — they share authorship.

---

## Context

As of the v0.9.0 release candidate, six integration tests in
`crates/rustynes-test-harness/tests/` are `#[ignore]`'d as expected-fail:

- `cpu_interrupts_v2/2-nmi_and_brk`
- `cpu_interrupts_v2/3-nmi_and_irq`
- `cpu_interrupts_v2/4-irq_and_dma`
- `cpu_interrupts_v2/5-branch_delays_irq` (fails on `test_jmp` sub-test)
- `mmc3_test_2/4-scanline_timing` (fails on sub-test #2: "Scanline 0
  IRQ should occur later when `$2000=$08`")
- `mmc3_test_2/6-MMC3_alt` (by-design fail: NEC rev B; project defaults
  to Sharp rev A. **Not in scope of this ADR.**)

Each of the five non-by-design failures has a companion
`*_currently_fails` probe that asserts the current failure shape; both
are part of the strict-test-suite contract established by Track A1.

Four independent code attempts have been made and all four were rolled
back as negative results. Each is documented in CHANGELOG `[Unreleased]`
→ "Investigated and rolled back". Their evidence is the design
constraint set this ADR captures.

### Rolled-back attempts (evidence, not solutions)

#### Attempt 1 — Intra-cycle CPU phase split (start/access/end)

**What:** Refactor `Cpu::step` to emit `(start_ticks, access_ticks,
end_ticks)` per memory access rather than the existing access-then-tick
model. Two split values tested: `1+2` and `2+1` PPU dots before/after
the access.

**Result:** Both split values regressed `ppu_vbl_nmi/05-nmi_timing`
(was 9/10 passing → failing). Neither flipped the target
`ppu_vbl_nmi/10-even_odd_timing` nor any `cpu_interrupts_v2` sub-ROM.

**Diagnosis:** The existing PPU's NMI-line timing and trailing-cycle
interrupt-sample model in `Cpu::step` are calibrated against the
`0+3` access ordering. Shifting the access N dots later via the split
correspondingly shifts the NMI sample N dots later, breaking
`ppu_vbl_nmi/05`'s tuned boundary. Meanwhile `ppu_vbl_nmi/10`'s
"Clock skipped too late" failure comes from upstream pre-render-line
dot 339 logic in `Ppu::advance_dot`, which detects the skip *before*
observing the BG-enable write at any phase position — both split
values still skip the dot the test expects un-skipped.

**Conclusion:** The CPU phase split alone cannot flip the target
tests. To fix test 10 the PPU's BG-enable sampling logic needs a
1-dot delay between PPUMASK write and the rendering-enabled flag
becoming visible to the dot-skip check.

#### Attempt 2 — MMC3 IRQ-pending visibility pipeline, 1 M2 delay

**What:** Inside `Mmc3`, introduce `irq_pending_visible` returned by
`Mapper::irq_pending` instead of the synchronous `irq_pending_line`.
The visible value lags the internal flag by 1 M2 (CPU) cycle, advanced
once per `notify_cpu_cycle`. `$E000` ack clears all stages synchronously.

**Result:** Identical to baseline (`mmc3_test_2/4-scanline_timing`
still `Failed #2`, status=0x2). No `cpu_interrupts_v2` sub-ROMs
flipped.

**Diagnosis:** The single cycle was insufficient to push the IRQ-poll
past the instruction window where it currently lands. The CPU's
IRQ-sample point is still at the same M2 phase relative to the A12
rising edge.

#### Attempt 3 — MMC3 IRQ-pending visibility pipeline, 2 M2 delay

**What:** Same as Attempt 2, but with a 2 M2-cycle delay.

**Result:** Advanced through sub-tests 1-8 (`Failed #2` → `Failed #9`,
status `0x2` → `0x9`), but landed on the **opposite asymmetry**:
sub-test #9 is "Scanline 0 IRQ should occur **sooner** when
`$2000=$10`" (standard layout, sprites on `$1000`). The constant
2-cycle delay over-shifts the standard-layout case once it has fixed
the reverse-layout case.

**Diagnosis:** Sub-test #2 expects `$2000=$08` (BG on `$1000`,
reverse layout) IRQ to fire LATER; sub-test #9 expects `$2000=$10`
IRQ to fire SOONER. These are **bidirectional bounds** and a single
constant pipeline cannot satisfy both, because the qualifying A12
rising edge lands on different PPU dots in the two layouts, which
fall on different M2 phases relative to the CPU's IRQ poll point.

No `cpu_interrupts_v2` sub-ROMs flipped at either delay value.

**Conclusion:** The actual fix is *not* a constant-cycle pipeline on
MMC3's `irq_pending` flag. The delay is likely M2-phase-dependent
(the IRQ flip-flop only latches on rising M2, so the effective delay
is 1-2 cycles depending on whether the A12 edge lands in the first or
second half of an M2 period), OR the PPU's A12 emission for the BG-on-
`$1000` reverse layout fires on the wrong dot relative to the
standard layout, OR the CPU's `poll_irq` sample point is itself one
M2 cycle off relative to where it needs to be — and these three
possibilities are not independent of each other.

#### Attempt 4 — `LockstepBus` access-ordering swap (access-then-tick → tick-then-access)

**What:** Swap the order of `LockstepBus::read`/`write` so the PPU
ticks happen before the access fanout instead of after.

**Result:** Byte-identical status codes pre/post-swap for all 44
status-emitting ROMs. Neither helped nor regressed the target tests.

**Diagnosis:** The bus access ordering alone is not the bottleneck.
The failures need different fixes: `mmc3_test_2/2,4` are PPU A12
emission timing during sprite-fetch dots, and `cpu_dummy_writes_ppumem`
open-bus is the bus's open-bus latch policy on RMW dummy writes to
write-only registers.

### Refined diagnosis (synthesizing the four rolled-back attempts)

All four attempts share a single structural conclusion: **the CPU
per-cycle IRQ sample point, the `LockstepBus` IRQ poll point, and the
PPU A12 emission dot are coupled.** Any one of them can be moved by
1-2 M2 cycles, but only at the cost of regressing tests calibrated
against the other two. None of them is independently the
"right answer."

The IRQ-poll-to-flag-visibility surface is, in the project's current
architecture:

```text
PPU emits A12 rising edge at dot D                  (PPU dot resolution)
    ↓
Mapper sees notify_a12(level=true) synchronously    (same dot)
    ↓
Mapper's filter decides whether to clock the
    IRQ counter, then asserts irq_pending_line      (same dot)
    ↓
LockstepBus polls Mapper::irq_pending() during
    its per-CPU-cycle update                        (3 PPU dots later)
    ↓
CPU::step samples the IRQ line at its
    per-cycle sample point                          (within those 3 dots)
    ↓
If sampled high AND I-flag clear AND not in a
    delay-after-CLI window, the interrupt
    services on the NEXT instruction boundary       (some N cycles later)
```

The four-step path means three independent timing decisions:

1. **PPU dot at which A12 emission happens** (e.g., dot 260 for the
   sprite fetch group; the exact dot for BG fetches in the reverse
   layout vs. standard layout is currently identical but real silicon
   may differ).
2. **CPU per-cycle IRQ sample point** (currently the "trailing cycle"
   of `Cpu::step`'s per-cycle bus-interleaving loop, calibrated for
   `cpu_interrupts_v2/1-cli_latency` and `ppu_vbl_nmi/05-08`).
3. **`LockstepBus` IRQ poll** (currently inside the bus's tick
   callback; the access-ordering attempt 4 confirmed swapping the
   tick order is a no-op, but the *poll location* relative to the
   per-cycle tick may not be).

The five remaining failures are all consequences of these three
points not being aligned with the same M2 phase reference.

---

## Decision

**Land a single, coordinated, designed change that adjusts all three
sample points together.** Not incremental, not isolated attempts at
the M2/PPU/Bus level.

## Decision (revised, 2026-05-13)

The four rolled-back attempts — *Attempt 1* (intra-cycle CPU phase
split start/access/end), *Attempt 2* (MMC3 IRQ-pending pipeline at
1 M2 delay), *Attempt 3* (same pipeline at 2 M2 delay), *Attempt 4*
(`LockstepBus` access-ordering swap) — collectively prove three
things:

1. The CPU's per-cycle interrupt sample point, the bus's IRQ poll
   point, and the PPU's A12 emission dot **cannot be moved
   independently**. Any single-axis move regresses something
   calibrated against the other two axes.
2. The MMC3 IRQ filter operates at 1-cycle resolution (`cpu_cycle`
   integer), but the actual flip-flop in silicon latches on rising
   M2 — so the effective delay is M2-phase-dependent (1-2 cycles
   depending on whether the A12 edge lands in the first or second
   half of an M2 period). A constant pipeline (Attempts 2 / 3)
   cannot model that asymmetry.
3. The "M2 phase" relative to PPU dots is **implicit** in the
   current scheduler. None of the four attempts could be evaluated
   against an explicit phase oracle because no such oracle existed.

### The coordinated change differs from each rolled-back attempt

- **Differs from Attempt 1** (intra-cycle CPU phase split) because
  it does *not* shift the access N dots later. The CPU memory
  access still happens at the start of the cycle (mirroring `φ1
  low` in silicon). What the coordinated change adds is an
  explicit *phase reference* and a re-derivation of the
  interrupt-sample point relative to it — not a reordering of when
  the access lands inside the 3-dot cycle. Attempt 1's split was a
  no-op at split=0 and a regression at any other split precisely
  because the phase reference was missing; we add the reference
  first, then derive the access position.
- **Differs from Attempt 2 and Attempt 3** (constant 1-cycle and
  2-cycle MMC3 IRQ-pending pipelines) because the coordinated
  change does *not* add a constant-cycle pipeline on MMC3's IRQ
  flag. Instead it gives MMC3 a precise M2-phase reference at the
  moment of the qualifying A12 edge: when the edge occurs at the
  first half of an M2 period, the latch is seen by the CPU's
  IRQ sample in the *same* CPU cycle; when it occurs at the second
  half, the latch is only visible to the *next* cycle's sample —
  the natural asymmetry the test ROM expects between sub-test #2
  (BG @ `$1000`, reverse layout) and sub-test #9 (BG @ `$0000`,
  standard layout). A constant pipeline cannot model this; an
  explicit M2-phase reference can.
- **Differs from Attempt 4** (bus access-ordering swap) because it
  does *not* swap `tick_one_cpu_cycle` from access-then-tick to
  tick-then-access. Attempt 4 was a no-op because the *poll
  location* of the IRQ relative to the per-cycle tick was
  undefined; the coordinated change *defines* it: the bus's
  `poll_irq` snapshots the IRQ line at the M2-falling boundary
  (end of the 3rd PPU dot of the cycle) into a `irq_sampled_at_m2_fall`
  bool, and the CPU's `idle_tick` reads from that snapshot rather
  than calling `poll_irq` directly. This gives us a single,
  well-defined point at which all three sample axes observe the
  same bus state.

### M2-phase reference

Concretely the reference will be a `core::scheduler::M2Phase` enum
on the bus's per-cycle state:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum M2Phase {
    /// First half of the CPU cycle: M2 low (φ1). Memory accesses
    /// happen here in silicon; in our model `cpu_read`/`cpu_write`
    /// land at the *start* of `tick_one_cpu_cycle` which is this
    /// phase by definition.
    Low,
    /// Second half of the CPU cycle: M2 high (φ2). IRQ/NMI latches
    /// in silicon sample HERE; in our model this corresponds to
    /// "after PPU dot 2 of the 3-dot tick has completed but before
    /// `notify_cpu_cycle` increments the mapper's cycle counter."
    High,
}
```

The reference is advanced at the boundary between PPU dot 2 and
PPU dot 3 of every CPU cycle (the second of the 3 PPU dots in a
CPU cycle is approximately the M2-low → M2-high transition on
silicon; the third dot is M2-high; the cycle boundary is the
M2-high → M2-low fall).

- **CPU reference**: `idle_tick` reads `bus.poll_irq_at_phase(M2Phase::High)`
  rather than the existing `bus.poll_irq()`. The phase-aware poll
  returns the IRQ line state as snapshotted at the most recent M2
  rising boundary inside `tick_one_cpu_cycle`.
- **Bus reference**: `tick_one_cpu_cycle` snapshots `mapper.irq_pending()
  || apu.irq_line()` into `irq_snapshot_m2_high` between PPU dot 2
  and PPU dot 3 (the M2-rising boundary). `poll_irq_at_phase` reads
  from the snapshot.
- **PPU reference**: A12 emission still happens inside `Ppu::tick`,
  but the bus's `PpuBusAdapter::notify_a12` now also records the
  `(cpu_cycle, m2_phase_at_emission)` tuple into a new mapper-side
  field. MMC3's filter consults *phase*, not just cycle count, to
  decide visibility on the next IRQ poll.

### Test fixture

The M2-phase tracing fixture (landed pre-work; see commit
`test(harness): M2-phase IRQ tracing fixture (C1 pre-work)`)
records `(cpu_cycle, ppu_scanline, ppu_dot, m2_phase, a12_level,
irq_pending_mapper, irq_pending_apu, irq_sampled_by_cpu)` per CPU
cycle. The fixture's baseline trace (pre-change) is committed at
`crates/rustynes-test-harness/golden/irq_trace_baseline_*.csv`. Each
coordinated-change commit must produce a trace that differs from
the baseline ONLY at the cycles where IRQ-line visibility changed
between them — never at cycles where the trace is identical to the
no-IRQ control flow. This is the empirical oracle the four
rolled-back attempts lacked.

### Stop conditions (mandatory)

Per the plan:

1. **Any pre-existing strict test regresses.** Rollback and document
   as the 5th designed-out approach in this ADR.
2. **Some but not all 5 target tests flip.** Partial fixes are
   designed-out: the trace fixture should reveal why partial before
   any code lands.
3. **The proposed change reaches the same diagnosis as one of the
   four rolled-back attempts.** Stop. A 5th rollback is worse than
   no change.

### Proposed approach (concrete enough to plan, abstract enough to refine)

1. **Define an M2-phase reference** — a single canonical assertion
   inside the lockstep scheduler that says "this PPU dot is M2 rising"
   vs. "this PPU dot is M2 falling." Today this is implicit in the
   CPU's third-dot tick alignment; make it explicit.
2. **Re-derive each of the three sample points relative to that
   reference**:
   - PPU A12 emission: still during the pattern-fetch dot, but
     **document the M2 phase** at which it lands for each fetch type
     (BG NT byte, BG AT byte, BG pattern lo/hi, sprite pattern lo/hi).
     The "reverse layout fires on a different M2 phase than the
     standard layout" hypothesis is the falsifiable claim here.
   - CPU IRQ sample: move from "trailing cycle of `Cpu::step`" to
     "the second M2 falling edge inside each instruction" (matching
     the documented 6502 IRQ-sample silicon model). This was
     explicitly warned against in Attempt 1 because it would regress
     `cpu_interrupts_v2/1-cli_latency` — so the coordinated change
     must include a corresponding adjustment to the I-flag delay
     snapshot in `Cpu::step` so `/1` keeps passing.
   - LockstepBus IRQ poll: move to "M2 falling edge of every CPU
     cycle" rather than the current implicit ordering. The access-
     ordering swap from Attempt 4 is a no-op precisely because the
     poll location relative to the per-cycle tick isn't currently
     defined; this step *defines* it.
3. **Verify against the M2-phase test fixture.** Add a probe in
   `rustynes-core` that records, for every CPU cycle, the `(ppu_dot,
   m2_phase, a12_level, irq_pending, irq_sampled)` tuple. Run it
   against `mmc3_test_2/4-scanline_timing` and `cpu_interrupts_v2/*`
   and produce a side-by-side trace that the rolled-back attempts
   could not be evaluated against (because the M2 phase was implicit
   in each).

### Expected test surface to flip

When the coordinated change lands correctly:

- `cpu_interrupts_v2/2-nmi_and_brk` — `*_currently_fails` probe
  flips from FAIL → PASS; flip the `#[ignore]` `_strict` test from
  `#[ignore]` to non-ignored, delete the `_currently_fails` companion.
- `cpu_interrupts_v2/3-nmi_and_irq` — same.
- `cpu_interrupts_v2/4-irq_and_dma` — same.
- `cpu_interrupts_v2/5-branch_delays_irq` — `test_jmp` sub-test
  passes; same flip pattern.
- `mmc3_test_2/4-scanline_timing` — sub-test #2 passes;
  `_currently_fails` probe in `tests/mmc3.rs` flips loud.
- **Possibly** `cpu_dummy_writes_ppumem` open-bus sub-test (currently
  passes strictly; may re-orient if the bus's open-bus latch policy
  needs adjusting to keep the test happy under the new ordering).

### Tests that must NOT regress

- `nestest` (8,991-instruction zero-diff against Nintendulator log).
- All 10 `ppu_vbl_nmi/*` sub-ROMs.
- All 5 `cpu_interrupts_v2/{1}` (currently the only one passing).
- All 4 `mmc3_test_2/{1,2,3,5}` currently passing.
- All 8 `apu_test/*`, 4 `apu_mixer/*`, 5 `dmc_dma_during_read4/*`,
  5 `sprite_overflow_tests/*`, 11 `sprite_hit_tests/*`.
- The PPU-internal A12 invariant
  `a12_rising_edges_match_241_per_ntsc_frame_standard_layout` in
  `crates/rustynes-ppu/`.

### Success criteria

The `*_currently_fails` probes in `tests/cpu_interrupts_v2.rs` and
`tests/mmc3.rs` (lines noted in CHANGELOG) flip green (i.e., loudly
fail with the "unexpectedly PASSES — please flip the `_strict` test
to non-ignored and delete this probe" message). At that point:

1. Move the `#[ignore]`'d `_strict` tests to non-ignored, deleting
   the `_currently_fails` probes per the test contract.
2. Update `docs/STATUS.md` "Test ROMs" table:
   - `cpu_interrupts_v2`: 1 → 5 strict pass; 4 → 0 `#[ignore]`.
   - `mmc3_test_2`: 4 → 5 strict pass; 2 → 1 `#[ignore]`
     (the by-design `6-MMC3_alt` stays).
3. CHANGELOG `[0.X.0]` entry describing what the coordinated change
   did and citing this ADR.
4. Bump workspace version 0.9.x → 1.0.0 per the version policy in
   `docs/STATUS.md`.

---

## Consequences

### Positive

- All five remaining IRQ-timing failures resolve in one coherent
  step rather than playing whack-a-mole.
- The M2-phase reference becomes an explicit, testable invariant
  of the scheduler — useful for any future bus-timing investigation.
- The project's "lockstep" claim (CLAUDE.md §"The PPU is the master
  clock") is documented at the per-cycle phase level, not just the
  per-dot level.

### Negative

- This is a large, designed change touching three crates (`rustynes-cpu`,
  `rustynes-mappers`, `rustynes-core`'s `LockstepBus`). Expected effort: 1-3
  weeks of work plus 1 week of regression hunting.
- The "I-flag delay snapshot" mentioned in step 2 is itself a 6502
  subtle behavior; getting it right under the new IRQ-sample point
  may require additional unit tests on `Cpu::step`.
- Save-state format may need a per-section version bump if the
  scheduler exposes new state via the M2-phase reference.

### Neutral

- The four rolled-back attempts can be re-introduced as foundation
  for this work if their plumbing is useful. Attempt 1's
  start/access/end CPU phase split refactor is particularly likely to
  be re-used — it's clean plumbing that's just a no-op until paired
  with the M2 reference.

---

## Open questions

- Does the project want to also resolve `cpu_dummy_writes_ppumem`
  open-bus sub-test 5-error issue as part of this ADR, or as a
  follow-up?
- Should the M2-phase reference be a runtime field on the scheduler
  (`u8` enum) or a const generic on `Cpu::step`'s helpers?
- Does the visual-only `mmc3_irq_tests/*` corpus need a re-pass once
  the IRQ-poll is coordinated, or are the 6 smoke gates sufficient?

---

## References

- CHANGELOG `[Unreleased]` → "Investigated and rolled back"
  (CHANGELOG.md lines 137-141 + 150-152).
- Track C1 of `/home/parobek/.claude/plans/linked-puzzling-sutherland.md`.
- `docs/STATUS.md` → "Known residuals (v0.9.0 → v1.0.0)".
- nesdev wiki §IRQ-and-NMI timing, §MMC3 IRQ filter:
  - <https://www.nesdev.org/wiki/CPU_interrupts>
  - <https://www.nesdev.org/wiki/MMC3>
- blargg's `cpu_interrupts_v2` and `mmc3_test_2` ROM corpora documented
  in `tests/roms/LICENSES.md`.

---

## Empirical refinement (2026-05-14, post-Phase-A + Phase-B4-attempt)

Two work sessions after the 2026-05-13 "Decision (revised)" landed,
the infrastructure for the coordinated change is in place (Phases A +
B1 + B2/B3, see CHANGELOG `[Unreleased]` and commits `d7d4c98` /
`12949c3` / `c8b7ce6`). A fifth code attempt — **Phase B4: sub_dot-
aware MMC3 A12 filter threshold** — was prototyped in two iterations
and rolled back (commit `df07ae3` lands only a diagnostic CHANGELOG
entry; no `.rs` / `Cargo.toml` changes shipped).

The empirical evidence from Phase A and Phase B4 refines two specific
hypotheses in the original "Decision (revised, 2026-05-13)" section.
The hypotheses are not falsified outright, but their implementation
surface has moved.

### Phase A finding — bus-snapshot phase is not the observable axis

Phase A rewrote the M2-phase tracing fixture's `CycleRecord` from a
single `M2Phase::High`-hardcoded IRQ snapshot into a two-phase
`(_at_low, _at_high)` pair. `LockstepBus::tick_one_cpu_cycle` samples
`(mapper.irq_pending, apu.irq_line)` after PPU sub_dot 0 (M2-low
convention) and again after PPU sub_dot 2 (M2-high convention). All
6 baseline trace CSVs at `crates/rustynes-test-harness/golden/irq_trace/`
were regenerated with the new 4-column schema.

**Across all 6 regenerated baselines, every single row shows
`irq_pending_*_at_low` == `irq_pending_*_at_high`.** The two-phase
snapshots are byte-identical because `notify_a12` is called
synchronously inside `Ppu::tick` — by the time the bus samples at
M2-low (post-sub_dot-0), the mapper's `irq_pending` is already set
or already cleared, and re-sampling at M2-high after sub_dot 2
captures the same state.

**Implication for the original "Decision (revised, 2026-05-13)"
proposal** (lines 226-235 above, "Bus reference"): the proposed
`irq_snapshot_m2_high` field on the bus is now landed (Phase B2 / B3,
unconditionally — see `LockstepBus::irq_snapshot_{mapper,apu}_at_high`).
`Bus::poll_irq_at_phase(M2Phase::High)` reads from it. **But that
single phase-aware snapshot cannot, in isolation, expose any
asymmetry the test ROMs are sensitive to — because the snapshot
inputs are identical between phases.** The bus-side phase-aware poll
is therefore necessary plumbing for the coordinated change, but not
sufficient.

The M2-phase asymmetry that `mmc3_test_2/4` sub-test #2 demands must
live in one of:

- **(a)** the mapper's filter response to the sub_dot of the A12 rise
  (Phase B4 hypothesis — empirically falsified below);
- **(b)** PPU-side A12 emission-dot differences between pre-render
  scanline 261 and visible scanlines (unexplored);
- **(c)** the CPU IRQ-sample-point change itself (also unexplored —
  `Cpu::idle_tick` now reads via `poll_irq_at_phase(M2Phase::High)`,
  but the deeper rework of `Cpu::step`'s mid-instruction IRQ-sample
  ordering proposed in step 2 of the original "Proposed approach" is
  still future work).

### Phase B4 finding — MMC3 filter threshold is not the load-bearing axis

Phase B4 prototyped two iterations of a sub_dot-aware MMC3 A12 filter
threshold, motivated by the original "Decision (revised, 2026-05-13)"
diagnosis (lines 213-224 above):

- **Iteration 1**: sub_dot 0 / 1 require gap >= 4, sub_dot 2 requires
  gap >= 3 (M2-low stricter).
- **Iteration 2**: inverted (sub_dot 2 requires gap >= 4, sub_dot 0 /
  1 require gap >= 3, M2-high stricter).

Neither iteration flipped `mmc3_test_2/4` sub-test #2.

**Trace-derived evidence** (from the v0.9.0 baseline at
`crates/rustynes-test-harness/golden/irq_trace/mmc3_test_2-4-scanline_timing.csv`):
the MMC3 IRQ line first asserts at **cycle 1,369,997, frame 46,
scanline 261, dot 259, sub_dot 0** (pre-render sprite fetch). The
prior A12 transition in the trace is at cycle 474,850 — roughly
**900,000 CPU cycles earlier**, during the test-setup phase when
rendering is disabled. The gap between the qualifying rise and the
prior A12 fall is enormous; **any reasonable threshold (3, 4, 5,
100) accepts this rise identically**.

The *next* A12 rise after the failure is at cycle 1,370,110
(scanline 0, dot 257, sub_dot 2) — exactly 113 cycles later, one
NTSC scanline. To flip sub-test #2 the filter would have to **reject
the cycle 1,369,997 rise and accept the cycle 1,370,110 rise**, but
both rises have identical large gaps from the prior fall. The
threshold dimension cannot produce that discrimination; the
discriminator must be a property of the **rise itself**, not the
gap.

**Implication for the original "Decision (revised, 2026-05-13)"
proposal** (lines 213-224 above, "Differs from Attempt 2 and Attempt
3"): the hypothesis that the asymmetry is captured by a single
M2-phase reference applied *uniformly* to the MMC3 filter is too
weak. The sub_dot-of-the-A12-rise is now exposed on the bus (Phases
A + B1 + B2/B3 landed the plumbing), and `MapperFrameEvents` already
carries A12 transitions — but the MMC3 filter consuming that
plumbing with a phase-aware threshold rule is the surface that Phase
B4 falsified.

### Refined direction for the next attempt

Two falsifiable hypotheses, in priority order:

- **Option A — PPU-side pre-render A12 emission audit.** Does
  `Ppu::tick` emit a clockable A12 rise for the pre-render scanline
  261's first sprite-fetch dot that real silicon does not? The
  trace's cycle 1,369,997 rise lands on scanline 261 dot 259 (the
  first sprite-tile fetch of pre-render). Silicon-accurate references
  (Mesen2's PPU, the nesdev `Ppu_frame_timing.png` reference)
  document that the pre-render line's sprite fetches do happen,
  but whether the MMC3 *should* see the rise on dot 259 of scanline
  261 specifically — or whether the rendering-just-re-enabled
  transition gates it for one extra cycle — is the open question.
  If the PPU should NOT clock A12 on scanline 261's first sprite
  fetch, the fix is in `Ppu::tick`'s A12 notify logic, not in MMC3.
- **Option B — Sub_dot-aware *counter-clock* pipeline on MMC3.**
  Structurally distinct from Attempts 2 and 3 (which delayed the
  *visibility* flag `irq_pending_visible`, not the *counter clock*):
  this option would defer the MMC3 internal IRQ-counter decrement
  by one CPU cycle when the qualifying A12 rise lands at sub_dot 0
  / 1 (M2-low half of the cycle), while clocking immediately when
  the rise lands at sub_dot 2 (M2-high half). Because the *counter*
  is the load-bearing state for "scanline N IRQ fires now vs. next
  scanline," delaying its clock — not its visibility — produces a
  fundamentally different shape of asymmetry than Attempts 2 / 3.
  The hypothesis is that pre-render scanline 261's first sprite
  fetch produces a sub_dot-0 rise, which queues for one CPU cycle
  and clocks on the next `notify_cpu_cycle` — effectively shifting
  the counter increment from scanline 261 to scanline 0, which is
  what sub-test #2 ("Scanline 0 IRQ should occur LATER when
  `$2000=$08`") expects.

Both options must be evaluated against the trace fixture before any
code lands. The trace fixture (now with two-phase IRQ snapshots from
Phase A) is the empirical oracle. Per the stop-condition discipline
in the original "Decision (revised, 2026-05-13)" subsection, a 6th
rollback would be the worst possible outcome — design analysis
against the trace data is mandatory before the next attempt.

### What stays valid in "Decision (revised, 2026-05-13)"

The structural claim of the original Decision section — that **the
CPU per-cycle IRQ sample point, the bus's IRQ poll point, and the
PPU's A12 emission dot cannot be moved independently** — is
strengthened, not weakened, by Phases A and B4. Phase A confirms the
bus's per-phase snapshots are byte-identical; Phase B4 confirms the
MMC3 filter threshold cannot expose the asymmetry alone. The single
remaining unconstrained axis in the proposal is the **PPU A12
emission dot** (Option A) and/or the **MMC3 counter clock** (Option
B), and the next iteration must couple one of those to the bus /
CPU plumbing now landed.

The M2-phase reference enum, the bus's per-phase snapshots, and
`Bus::poll_irq_at_phase` are all *necessary* infrastructure — they
just need a load-bearing consumer.

### Empirical refinement (post-step-B4 success, 2026-05-14)

**Phase B4 landed successfully** as a third-axis structural fix
distinct from Options A and B above. The successful fix was NOT
filter-threshold change, NOT counter-clock pipeline, NOT PPU A12
emission timing — but rather a **cycle-precise discriminator on the
`$C001`-induced reload-pending path**.

#### The successful axis

The v0.8.x `Mmc3::clock_irq` collapsed two distinct counter-zero
events into a single "assert if Sharp" branch:

1. **`was_zero` reload** — counter naturally reached 0 (via decrement
   on a prior rise), and the next rise re-reloads from `reload_value`.
   Sharp asserts; NEC does not. This is the path
   `mmc3_test_2/5-MMC3.nes` exercises.
2. **`reload_pending` reload** — `$C001` was written, forcing
   `counter = 0` and `reload_pending = true`. The next rise consumes
   the pending flag.

The new step B4 implementation distinguishes path 2 further:

- **`$C001` while counter was NON-ZERO** — a "fresh clear". The next
  rise reloads, and Sharp asserts if `reload_value == 0`. This is the
  path `mmc3_test_2/2-details` sub-test #7 ("IRQ should be set when
  non-zero and reloading to 0 after clear") exercises.
- **`$C001` while counter was ZERO** — a "no-op clear". The next rise
  reloads silently regardless of `reload_value`. This is the path
  `mmc3_test_2/4-scanline_timing` sub-test #2 ("Scanline 0 IRQ should
  occur LATER when `$2000=$08`") relies on: the test runs
  `begin_mmc3_tests` (multiple `$C001`s, counter ends at 0), then
  inside `begin_` writes one more `$C001` — which sees counter
  already at 0 and so the FIRST sprite-fetch A12 rise on pre-render
  scanline 261 reloads silently. The IRQ then asserts on the
  follow-up rise on scanline 0 via path 1 (natural `was_zero`).

#### Why this differs from all 5 prior attempts

| Attempt    | Axis                                      | Prior result |
|------------|-------------------------------------------|--------------|
| 1          | (unspecified — bus access ordering swap)  | Rolled back  |
| 2          | Constant 1-cycle pipeline on `irq_pending_visible` | Rolled back |
| 3          | Constant 2-cycle pipeline on `irq_pending_visible` | Rolled back |
| 4          | Bus access-ordering swap (different from #1) | Rolled back |
| B4 attempt | Sub_dot-aware MMC3 filter threshold       | Rolled back  |
| **B4 success** | **`$C001`-prior-counter-state latch on `reload_pending` assertion path** | **Landed** |

The successful axis is in `clock_irq`'s **assertion semantics**, not
its **timing**. It does not delay any event. It does not modify
filter thresholds. It does not modify PPU emission dots. It modifies
**which path in `clock_irq` is taken**, by latching a single bit at
each `$C001` write and consuming it on the next filtered A12 rise.

#### What's still open

- **`cpu_interrupts_v2/{2..5}`** — these 4 `#[ignore]`'d sub-ROMs do
  not involve MMC3; the step B4 landing does not affect them. They
  remain on the same architectural surface as the open
  `mmc3_test_2/4-scanline_timing` sub-test #3 residual: CPU per-cycle
  IRQ sample-point / bus IRQ-poll location.
- **`mmc3_test_2/4-scanline_timing` sub-test #3** — a 1-CPU-cycle
  bracket residual. The trace shows our second A12 rise (on scanline
  0) at cycle 1,370,110 sub_dot 2; the test's calibrated expectation
  brackets the IRQ assertion to 1 CPU cycle of fine grain. We pass
  sub-test #2 (expects $22 "LATER", we now give $22 — correct) but fail
  sub-test #3 (expects $21 "SOONER" with 1 more delay cycle, we still
  give $22). This means we fire ≤ 1 CPU cycle later than real silicon
  at the same A12 rise. The next axis to explore is the CPU IRQ
  sample-point timing inside the cycle (M2-high vs M2-low; see
  `nes_cpu::Cpu::idle_tick`'s `poll_irq_at_phase(M2Phase::High)` call
  — possibly the sample needs to happen at sub_dot 0 / 1 of the
  following cycle rather than at sub_dot 2 / end-of-cycle).
- **AccuracyCoin pass-rate** — currently 75.93%; v1.0.0 gate is 90%.
  The same CPU/Bus surface above is the likely lever.

#### What stays valid in "Decision (revised, 2026-05-13)" — updated

The Decision section's structural claim — *the CPU per-cycle IRQ
sample point, the bus's IRQ poll point, and the PPU's A12 emission
dot cannot be moved independently* — remains valid for the
`cpu_interrupts_v2/{2..5}` + `mmc3_test_2/4` sub-test #3 residuals.
Step B4's success demonstrates that **MMC3's assertion semantics**
is a *fourth* independent axis that was not enumerated in the
original Decision section. The full v1.0.0 fix likely needs both
step B4's `$C001`-prior-state discriminator (now landed) AND the
coordinated CPU/Bus axis from the original Decision (still open).

### Per-cycle CPU instrumentation analysis (post-step-B4, 2026-05-15)

Downloaded blargg's `cpu_interrupts_v2/source/*.s` from the
`christopherpow/nes-test-roms` GitHub mirror and the AccuracyCoin
upstream. The four failing sub-ROMs (`2-nmi_and_brk`,
`3-nmi_and_irq`, `4-irq_and_dma`, `5-branch_delays_irq`) all
**cycle-precise** measure CPU-side IRQ/NMI sample-point timing
under specific instruction-sequence conditions (BRK / hijack /
DMA / branch / JMP). Test-source-anchored diagnoses:

**`2-nmi_and_brk`** — expected hijack rows 4-8 (5 rows of NMI-
during-BRK). Our output hijacks rows 6-9 (4 rows shifted ~2 cycles
later, plus 1 extra row 9 over-hijack and row 5 producing an
anomalous `$02`). Suggests our NMI hijack window is shifted +2 CPU
cycles and the hijack cutoff at end of the BRK sequence is +1
cycle wide. Documented in nesdev wiki: "If NMI is asserted between
the second cycle of BRK and the cycle where the vector is read,
the NMI vector is used instead." The cycle-numbering inside our
`service_interrupt` does not align with this strictly: a tested
cutoff of `nmi_first_tick <= 4` for IRQ/NMI and `<= 3` for BRK
produced no observable test-outcome change (still hijacks rows
6-9), suggesting `nmi_first_tick` is set later than expected for
the early-BRK case. The fix likely requires re-deriving when
`bus.poll_nmi()` returns true during `service_interrupt`'s own
internal cycles, or tracking the post-NMI-hijack vector latch as
a separate state from `nmi_first_tick`.

**`5-branch_delays_irq` (test_jmp)** — expected output shows
`(T+, CK, PC)` triples where CK = cycles-remaining-in-current-
instruction at the moment IRQ fires. Our impl reports `CK=00` for
every 3rd row (rows 0, 2, 5, 7); real silicon reports CK=2 or 3
at those rows. The CK metric is computed by the IRQ handler's
`bit $4015 / bvc loop` (APU frame-IRQ wait); the +2/+3 difference
maps directly to a 2-3 CPU cycle slew between our CPU's
post-instruction IRQ-armed point and real silicon's mid-
instruction IRQ-sample point. This is the same `T_last - 1`
canonical sample-point issue: our impl samples at every cycle
(including `T_last`), real silicon only at `T_last - 1`.

**Common root cause**: our CPU's `idle_tick` samples IRQ on
*every* cycle (including the last cycle of each instruction), so
`irq_first_tick` is potentially set 1 cycle later than real
silicon would. The `promote_post_step_interrupts` then transfers
to `pending_irq` if `irq_first_tick == last_tick`, which DOES
defer by 1 instruction — matching real silicon for the IRQ-arming
decision. But the **timing-reporting** path (the IRQ handler's
view of when in an instruction the IRQ fired, via stack-pushed PC
and cycle counters) is what `cpu_interrupts_v2/{2..5}` measure,
and that path sees the per-cycle sample timing directly.

**Why a single point-fix is unsafe**: changing `idle_tick` to
sample IRQ only at `T_last - 1` (matching real silicon strictly)
would alter the irq_first_tick recorded value for many tests,
including the 80+ CPU tests that currently pass. The 1-cycle-
later IRQ visibility in our impl IS a calibration choice baked
into many test passes. A coordinated rework is needed:

- Restructure `idle_tick` to record `T_last - 1` as the canonical
  sample point (not every cycle).
- Re-run all CPU-cycle-sensitive tests (nestest, ppu_vbl_nmi,
  cpu_interrupts_v2/1, apu_test) to ensure no regressions.
- Then re-validate `cpu_interrupts_v2/{2..5}` to see whether the
  sample-point fix flips them.

This is a focused but high-risk session: one structural change
plus comprehensive test-suite re-validation. Out of scope for the
current C1 plan's commit boundary.

### Sub-dot plumbing landed (preparatory infrastructure, 2026-05-14)

A new optional `Mapper::notify_a12_at_sub_dot(level, sub_dot)` trait
method is now available, with a default impl that forwards to the
existing `notify_a12(level)` so no mapper compiles differently. The
`PpuBusAdapter` in `crates/rustynes-core/src/bus.rs` tracks the current
sub-dot (0 / 1 / 2) and calls the new method instead of plain
`notify_a12`, threading the M2-phase reference through to the
mapper at the point of every A12 transition. **No behavior change
this commit** — but the infrastructure is in place so the next
attempt at fixing `mmc3_test_2/4-scanline_timing` sub-test #3 (the
1-CPU-cycle residual) can override `notify_a12_at_sub_dot` in MMC3
to implement an M2-phase-aware IRQ-propagation pipeline without
further bus-side plumbing.

#### Empirical motivation (post-step-B4 trace, 2026-05-14)

Re-running the trace fixture after step B4 captures both surviving
IRQ assertions in `mmc3_test_2/4-scanline_timing`:

| Sub-test | CPU cycle | `ppu_dot_start` | sub_dot | M2 phase |
|----------|-----------|-----------------|---------|----------|
| #2 (passes) | 1,370,110 | 257 | 2 | High (φ2) |
| #3 (fails) | 2,203,969 | 258 | 1 | Low (φ1) |

The two A12 rises land at PPU dot 260 in both cases (same sprite-fetch
emission point) but at **different sub-dots within the host CPU
cycle**. The test's `delay_ppu_even` macro intentionally rotates the
PPU-CPU phase by 1 PPU dot between sub-tests #2 and #3, exposing the
silicon's M2-phase-dependent IRQ-output latency:

- **M2-high A12 rise (sub_dot 2)**: real silicon's MMC3 latches A12
  late in cycle X; the IRQ output goes high at φ2 of cycle X+1
  (one cycle of propagation delay).
- **M2-low A12 rise (sub_dot 0 / 1)**: MMC3 latches A12 during
  cycle X's φ1 half; the IRQ output is already high by φ2 of the
  **same** cycle (no propagation delay).

Our implementation currently treats both phases identically: the
mapper's `irq_pending_line` is set synchronously inside `notify_a12`,
and the bus's M2-high snapshot at end-of-cycle catches both at the
**same** cycle X. This puts our impl 1 CPU cycle ahead of real
silicon for M2-high rises (test #2's case), but the test passes
because the absolute timing happens to land us inside `inc irq_flag`'s
sample window anyway. For M2-low rises (test #3's case), we agree
with real silicon's visibility cycle (both at cycle X) — but the
CPU instruction stream's position relative to that cycle is
**different** between our impl and real silicon, putting our IRQ
during `inc irq_flag` while real catches it earlier (during `nop2`,
producing $21 instead of $22).

#### Why a naive "defer M2-high by 1 cycle" doesn't suffice

Implementing the M2-phase-aware IRQ-output delay in MMC3 — via the
new `notify_a12_at_sub_dot` plumbing — does NOT close sub-test #3
on its own: it would shift sub-test #2's IRQ visibility 1 cycle
later (could still land inside `inc`, still pass) but it leaves
sub-test #3 unchanged. The residual is in the relative position of
our CPU instruction stream vs. real silicon's at the moment the
M2-low IRQ becomes visible. Closing sub-test #3 requires either:

- (a) shifting our CPU instruction-cycle counts to match real
      silicon's per-instruction phasing (likely requires
      cycle-precise modeling of one or more instructions whose
      timing currently differs); or
- (b) re-examining the bus M2-low snapshot point and CPU
      `poll_irq_at_phase` to expose the M2-low assertion as visible
      at the CPU's mid-cycle sample window, advancing the IRQ
      visibility by a fraction of a CPU cycle.

Both directions need cycle-instrumented analysis against the trace
fixture before any commit. Per the stop-condition discipline above,
this is the next session's work — the plumbing landed here unblocks
that work without committing to a specific direction.

### Trace regeneration after B4 + Phase D3 fixes (2026-05-15)

After Phase B4 (MMC3 reload-pending Sharp discriminator) and the
Phase D3 NOP / open-bus fixes landed, the six baseline IRQ traces
at `crates/rustynes-test-harness/golden/irq_trace/*.csv` were regenerated
via `cargo test --features test-roms,irq-timing-trace --test irq_trace_fixture`.
The diff shape is the key diagnostic:

- **`mmc3_test_2_4_scanline_timing.csv`**: changed substantially
  (2898 inserts / 2875 deletes across 5773 lines). The first MMC3
  IRQ assertion line moved from cycle 1,369,997 / scanline 261
  (pre-render) dot 259 to cycle 1,370,110 / scanline 0 dot 257 —
  exactly the architectural fix B4 was designed to produce
  (sub-test #2 "Scanline 0 IRQ should occur later when `$2000=$08`"
  now passes). The second MMC3 IRQ assertion at cycle 2,203,969 /
  scanline 0 / dot 258 is the next sub-test phase's assertion.

- **`cpu_interrupts_v2_{1,2,3,4,5}_*.csv`**: ALL FIVE byte-identical
  to the pre-B4 baselines. `git diff --stat` reports zero changes.
  This is the empirical proof that the B4 fix is MMC3-localized: it
  changes nothing on the pure APU/CPU IRQ-flow axis that the
  `cpu_interrupts_v2` ROMs exercise. The residual `cpu_interrupts_v2`
  failures are on a distinct axis — the canonical 6502 IRQ
  sample-point timing (`T_last - 1` rule from the nesdev wiki "6502
  CPU" page) — that no MMC3-only change can address.

### Phase D3 NOP / open-bus fix impact on traces (2026-05-15)

The two Phase D3 fixes that landed concurrently with the trace
regeneration above:

- `crates/rustynes-cpu/src/cpu.rs` unofficial NOP `$04/$44/$64/$14/$34/$54/$74/$D4/$F4/$0C` dummy reads.
- `crates/rustynes-mappers/src/mapper.rs` + `crates/rustynes-core/src/bus.rs` `$4020-$5FFF` open-bus latch.

Neither fix changed any baseline trace. Both are CPU-bus-side
behavior changes that don't touch the per-cycle IRQ snapshot
pipeline. This is the expected outcome and confirms the trace
fixture is a precise oracle for IRQ-pipeline changes specifically.

### nesdev wiki research on MMC3 A12 / IRQ timing (2026-05-15)

Direct quotes from `https://www.nesdev.org/wiki/MMC3` confirming
the canonical timing assumptions our PPU + MMC3 use:

> "When using 8x8 sprites, if the BG uses $0000, and the sprites use
> $1000, the IRQ counter should decrement on PPU cycle 260."

Our `crates/rustynes-ppu/src/ppu.rs:867` uses `if (260..=316).contains(&self.dot)`
to drive sprite tile fetches (and the A12 rise inside them), so our
PPU emits A12 at PPU dot 260 — canonically correct.

> "The MMC3 scanline counter is based entirely on PPU A12, triggered on
> a rising edge after the line has remained low for three falling
> edges of M2."

Our `crates/rustynes-mappers/src/mmc3.rs::notify_a12_at_sub_dot` enforces
`gap >= 3` against `cpu_cycle - a12_low_cycle` (equivalent to the
three-falling-edges-of-M2 rule because each CPU cycle has exactly
one M2 falling edge). Canonical.

> Sharp revision: "generates IRQs when the scanline counter is *equal*
> to 0." NEC revision: "generates IRQs when the scanline counter is
> *decremented* to 0."

Phase B4's `irq_reload_pending_with_nonzero_clear` flag encodes
exactly this distinction. The Sharp branch asserts on reload-to-0
ONLY when the prior `$C001` clear was from a non-zero counter (the
"explicit re-set" pattern); the NEC branch never asserts on reload-
to-0. The zero-to-zero `$C001` clear (back-to-back writes, or after
power-on) is silent on Sharp too — which is what makes
`mmc3_test_2/4-scanline_timing` sub-test #2 pass.

**The wiki does NOT document** the electrical latency between MMC3's
internal /IRQ assertion and when the 6502 samples it. This is the
exact undocumented surface that sub-test #3 brackets (a 1-CPU-cycle
window). Lidnariq's hardware tests would be the authoritative
oracle here; without them, the bracket has to be reconciled against
the test by trial against the bus snapshot policy + CPU sample
point. None of the four pre-B4 rollbacks succeeded against this
bracket either — it's the same residual surface they could not
close.

The conservative path: keep the residual `#[ignore]`-tagged with
the empirically refined failure-shape ("IRQ should occur SOONER
when `$2000=$08`" per `_currently_fails` probe), document the
1-CPU-cycle bracket in this ADR, and treat it as a v1.0+ work
item rather than risking a 6th rollback by guessing at the bus /
CPU sample-point combination.

### Trace regeneration after Session-13 Mesen2 alignment (2026-05-22)

Session-13 (commits `ea3cc4c` + `eb37ff8`) landed the coordinated
cold-boot alignment with Mesen2: `Cpu::power_on()` seeds `S=$00` and
the reset path runs 8 cycles total (matching Mesen2's "8 cycles before
it starts executing the ROM's code" reference); the PPU power-up
position moves from `(scanline=261, dot=0)` to `(scanline=261, dot=340)`
(matching Mesen2's `(scanline=-1, cycle=340)`). The two-axis change
closes the +344-dot PPU offset that Session-12 instrumented and
Session-13 Phase C empirically proved against the load-bearing
AccuracyCoin `LDA $2002 / BPL VblLoop` poll at cycle 27,389.

Session-14 (this subsection, 2026-05-22) is the first trace-regen
opportunity after the Session-13 alignment lands on `main`. The 6
golden IRQ traces at `crates/rustynes-test-harness/golden/irq_trace/*.csv`
were regenerated unconditionally.

#### Empirical finding — all 6 traces shifted

The control ROM `1-cli_latency.csv` (a 1-row filtered trace, the only
strict-pass `cpu_interrupts_v2` ROM at the time of B4) provides the
cleanest shift signature:

| Side | `cpu_cycle` | `ppu_frame` | `ppu_scanline` | `ppu_dot` | irq_apu | nmi |
|------|-------------|-------------|----------------|-----------|---------|-----|
| Pre-Session-13 (post-B4 baseline) | 268,141 | 10 | 0 | 5 | 1 | 0 |
| Post-Session-13 (this regen) | 268,028 | 10 | 0 | 6 | 1 | 0 |

The delta is `−113` CPU cycles (`−339` PPU dots) with `+1` PPU dot
within scanline. The −339 / +1 sum is `−338` PPU dots = `−113` CPU
cycles, consistent with the Session-13 +344-dot architectural shift
modulo a small sampling jitter from where the filter records the IRQ
transition. The frame index and scanline are preserved (the shift
absorbs entirely into the per-scanline dot count, not the per-frame
scanline count).

For the larger traces:

| Trace | Lines | Diff lines | First-event cycle shift |
|-------|-------|------------|--------------------------|
| `cpu_interrupts_v2_2_nmi_and_brk.csv` | ~745 | 1,492 | varies |
| `cpu_interrupts_v2_3_nmi_and_irq.csv` | ~767 | 1,536 | varies |
| `cpu_interrupts_v2_4_irq_and_dma.csv` | 75 | 148 | varies |
| `cpu_interrupts_v2_5_branch_delays_irq.csv` | 206 | 413 | −39 cycles, +Δdot rotation |
| `mmc3_test_2_4_scanline_timing.csv` | 2,918 | 5,838 | −114 cycles, +0 dot, same scanline |

The MMC3 first-IRQ event is informative: its position moved from
**cycle 1,370,111 / frame 47 / scanline 0 / dot 260** (pre-Session-13)
to **cycle 1,369,997 / frame 47 / scanline 0 / dot 260** (post). The
scanline = 0 invariant the B4 fix established is preserved; only the
absolute cycle count shifts. Sub-test #2 ("Scanline 0 IRQ should occur
LATER when `$2000=$08`") continues to PASS at strict-test level — the
test brackets scanline assignment, not absolute cycle count.

#### What this invalidates in the prior ADR text

The "Trace regeneration after B4 + Phase D3 fixes (2026-05-15)"
subsection above stated:

> `cpu_interrupts_v2_{1,2,3,4,5}_*.csv`: ALL FIVE byte-identical to
> the pre-B4 baselines.

That was true at the time it was written. It is **no longer true
across the Session-13 boundary**: all 5 `cpu_interrupts_v2_*.csv`
traces are substantially rewritten by the +344-dot boot alignment
even though no C1-axis code change has landed since the 9th attempt
(`090671b`, drop opcode-fetch IRQ sample on taken branches, 2026-05-17).

The structural conclusion the B4 trace regen was used to support —
"the B4 fix is MMC3-localized and changes nothing on the pure APU/CPU
IRQ-flow axis" — remains valid as a B4-vs-pre-B4 statement of B4's
isolation. What changes post-Session-13 is the absolute cycle count
of IRQ events on ALL the traced ROMs, because the boot alignment moved
the test-program-counter's relationship to PPU position by a multi-frame
phase shift.

#### What this validates in the original ADR

The structural claim of the "Decision (revised, 2026-05-13)" section —
that the CPU per-cycle IRQ sample point, the bus's IRQ poll point, and
the PPU's A12 emission dot cannot be moved independently — is
strengthened by this regen. The +344-dot Session-13 shift is itself a
PPU-axis change that materially perturbs IRQ-line-state cycle positions
in every traced ROM, including the strict-pass `1-cli_latency` and
`4-irq_and_dma` controls. The trace-fixture diff confirms PPU-axis
moves propagate through the per-cycle IRQ snapshot pipeline regardless
of whether the CPU IRQ sample point or the bus IRQ poll moved.

#### What this implies for attempt 13

Eleven prior C1-axis attempts have been rolled back; the 12th attempt
this session (Session-14) is **infrastructure-landed-only** — no
chip-stack code change. The regenerated traces are the new
authoritative baselines against which any code attempt 13 must be
diffed. The prior contaminated baselines (committed up through
`c8b7ce6`) were used by attempts 5-11 to evaluate hypothesis support;
their interpretations should be re-checked against the Session-13
post-alignment trace before being cited as evidence for any new code
direction.

Per the original "Stop conditions" subsection: a partial-flip or
no-flip outcome from attempt 13 is acceptable, but a regression of
the 540 strict-pass count is not. The Session-13 boot alignment did
NOT regress any strict tests (the audit doc
`docs/audit/session-13-cpu-boot-fix-2026-05-21.md` Section "Acceptance
gauntlet" documents this); attempt 13 must clear the same bar.

The recommended sequence (documented in this session's audit doc at
`docs/audit/session-14-c1-attempt12-trace-regen-2026-05-22.md`) is:

1. Cycle-instrument Mesen2 IRQ-line-state into a per-CPU-cycle trace.
   The existing `scripts/mesen2_cpu_boot_trace.lua` script is the
   template. Cross-diff against the post-Session-13 RustyNES traces.
2. Form one falsifiable hypothesis from the Mesen2 cross-diff. The
   ADR-0002 §"Per-cycle CPU instrumentation analysis" subsection's
   `T_last - 1` axis is the leading candidate; the NMI-hijack-window
   sample-point inside `service_interrupt` is the second candidate.
3. Implement behind a feature flag. Regenerate traces with and without
   the flag. Diff against the Mesen2 oracle. Land only if the diff
   matches the oracle pattern AND the strict-pass count is preserved.

### Mesen2 IRQ-cycle oracle landed (2026-05-22, Session-15, attempt 13)

The Session-14 prerequisite item #1 ("Cycle-instrument Mesen2 IRQ-line-state
into a per-CPU-cycle trace") is now landed. New infrastructure:

- `scripts/mesen2_irq_trace.lua` — Mesen2 Lua script emitting a CSV trace
  of `irq_svc` / `nmi_svc` / `apu_set` / `apu_clr` / `init` events. Capability
  matrix in `docs/audit/session-15-c1-attempt13-mesen2-irq-oracle-2026-05-22.md`
  §"Phase 1A".
- `crates/rustynes-test-harness/golden/irq_trace/mesen2/*.csv` — 6 reference
  baselines for the 5 target ROMs + 1 control.
- `scripts/irq_trace_cross_diff.py` — cross-diff tool emitting per-event
  delta histograms + first-event-cycle comparison.

#### Critical Mesen2 Lua API capability gap

Mesen2's Lua API does **NOT** expose per-CPU-cycle IRQ-line state. Exec
callbacks fire only at opcode fetch (per-instruction boundary). The
`emu.eventType.irq` / `emu.eventType.nmi` callbacks fire at the cycle the
CPU services the interrupt (= vector fetch), not the cycle the line was
asserted. APU IRQ flag transitions are detectable at instruction-boundary
granularity via `apu.frameCounter.irqFlag` polling. Mapper IRQ state is
NOT directly exposed.

This asymmetry with the RustyNES per-CPU-cycle fixture makes a precise
cycle-by-cycle cross-diff impossible from Lua alone. The cross-diff
implemented in `scripts/irq_trace_cross_diff.py` works at event-cycle
granularity, which is the limit of what Mesen2 from Lua provides.

#### Multi-axis divergence revealed by the cross-diff

The oracle reveals three independent divergence axes between RustyNES and
Mesen2:

1. **Boot/test-anchor cycle offset** — RustyNES vs Mesen2 reach the test's
   measurement loop at different absolute cycle counts despite the
   Session-13 boot alignment. For `cpu_interrupts_v2/4-irq_and_dma`
   (strict-pass), the constant offset is +89,343 ± 4 CPU cycles for 24 of
   32 matched events. This is NOT a CPU IRQ-sample-point divergence — it
   is an instruction-stream phasing divergence in the test's setup code.

2. **Mesen2 MMC3 default revision ambiguity** — Mesen2's settings.json
   shows `"Revision": "Compatibility"`, which the Mesen2 source heuristically
   resolves per-ROM. For `mmc3_test_2/4-scanline_timing.nes` (no NES 2.0
   submapper), the resolution is likely NEC, opposite of RustyNES's default
   Sharp. The first-MMC3-IRQ-event cross-diff shows RustyNES at scanline 0
   (B4 invariant) and Mesen2 at scanline -1 (pre-render) — consistent with
   NEC-on-Mesen2 vs Sharp-on-RustyNES. Cannot interpret as silicon-vs-impl
   divergence until Mesen2 is forced to Sharp.

3. **Per-instruction-vs-per-cycle event-detection asymmetry** — Mesen2's
   `apu_set` / `nmi_set` events are detected at the NEXT instruction fetch
   after the actual line transition. A constant 1-30 cycle delta is
   therefore expected for ALL matched events even if the IRQ pipeline is
   silicon-perfect on both sides. Falsifiable: extend the RustyNES fixture
   to ALSO emit instruction-boundary edge-detection records for direct
   apples-to-apples comparison.

None of these three axes is the CPU-side IRQ-sample-point axis
(`T_last - 1` / NMI-hijack-window) that the failing tests target. The
Mesen2 oracle as currently captured does **NOT** directly probe that
specific pipeline.

#### Attempt 13 is oracle-only — no code change landed

Per ADR-0002 Stop Condition #3, attempt 13 ships the oracle infrastructure
only. The cross-diff data is preserved as permanent infrastructure for
attempts 14+. Recommended Session-16 sequence:

1. Re-run Mesen2 baselines with MMC3 forced to Sharp rev A (via per-ROM
   Mesen2 settings override) to resolve the revision ambiguity.
2. Augment the RustyNES `IrqTrace` fixture with `irq_svc` / `nmi_svc`
   event records (analogous to Mesen2's) — logging the cycle of every
   read from `$FFFE`/`$FFFF`. This collapses the per-cycle vs
   per-instruction asymmetry to a direct apples-to-apples comparison.
3. Per-CPU-instruction divergence trace in the IRQ-arming window (not
   boot). Use `scripts/mesen2_cpu_boot_trace.lua` with
   `MESEN2_CPU_BOOT_TRACE_START_CYCLE=250000` and
   `_END_CYCLE=500000` to capture each failing test's IRQ-arming loop.
   Cross-diff against an in-tree RustyNES per-instruction trace. Locate
   the first PC divergence; that PC is the load-bearing instruction the
   prior 12 attempts couldn't isolate.
4. Only AFTER 1-3 yield a single-axis hypothesis, implement attempt 14
   behind feature flag `cpu-c1-attempt-14`, regenerate traces with and
   without the flag, diff against the (revised) Mesen2 oracle, land
   only if the diff matches AND the 540 strict-pass count is preserved.

Full per-ROM diff outputs + capability matrix at
`docs/audit/session-15-c1-attempt13-mesen2-irq-oracle-2026-05-22.md`.

### Decision update (2026-05-22, Session-16) — C1 attempt 14 prereq infrastructure landed; Phase 2 NOT attempted

Session-16 executed the Session-15 follow-up plan items 1, 2, and 3 (the
three prereqs for a clean hypothesis). The Phase 1 outcome:

- **Confound 1 (MMC3 revision)** is empirically resolved. Forcing
  Mesen2 to MMC3A via a `MesenNesDB.txt` override (CRC `0x8AD8A602`
  Chip-field = `MMC3A`, file marked `chmod 0444` to prevent Mesen2's
  per-run rewrite from embedded resource) produces a **byte-identical**
  trace for `mmc3_test_2/4-scanline_timing.nes` compared to the
  Compatibility default. **Mesen2's MMC3 revision is NOT the cause of
  the scanline -1 vs scanline 0 mismatch.** The first-IRQ scanline
  divergence is real, not a revision artifact.

- **Confound 3 (schema asymmetry)** is closed. `IrqTrace` now records
  `ServiceEvent`s emitted from `Cpu::service_interrupt` via the new
  `Bus::notify_irq_service` trait method (default no-op; LockstepBus
  override gated on `irq-timing-trace`). The 6 baselines now have a
  `*.svc.csv` sidecar with one row per IRQ or NMI vector fetch, directly
  comparable to Mesen2's `irq_svc` / `nmi_svc` event rows.

- **Confound 2 (boot/anchor offset)** is plumbed but functionally a
  no-op for these specific ROMs (`BOOT_FRAMES=10` already advances past
  cycle 250,000). Both sides accept `START_CYCLE` env var
  (`MESEN2_IRQ_TRACE_START_CYCLE` / `RUSTYNES_IRQ_TRACE_START_CYCLE`)
  as durable infrastructure for future investigations where the
  measurement window starts later than frame 10.

**Cleaned cross-diff outcome (post all 3 prereqs):** the data is still
multi-axis, NOT a single-cycle pipeline delay. The signal on the
PASSING test (`4-irq_and_dma`) is a constant ~+89,343 cycle delta
(= 3 NTSC frames; boot-anchor artifact from Mesen2's `BOOT_FRAMES=10`
skipping RustyNES's first IRQ at cycle 326,024 ≈ frame 11) plus a
constant Δdot = +10 PPU dots offset. The FAILING tests show **divergent
instruction-stream execution**: cycle deltas jitter ±250k cycles
across consecutive events, indicating Mesen2 and RustyNES walk
different code paths after the first IRQ service. `mmc3_test_2/4` has
Mesen2 firing the first MMC3 IRQ at cycle 1,220,992 / scanline -1 vs
RustyNES at cycle 1,370,004 / scanline 0 — a -149,012-cycle delta
(= 5 frames) AND a scanline mismatch, both of which result from the
B4 fix's intentional divergence from Mesen2's pre-render IRQ to close
sub-test #2.

**Phase 2 decision**: Per ADR-0002 Stop Condition #3, attempt 14 is
**oracle-only-prereqs-landed** outcome — no code change to chip crates.
The 13th C1-axis rollback would be predictable. The next session's
work item is the per-CPU-instruction divergence trace (Recommended #1
in the Session-16 audit doc): locate the FIRST in-test-loop PC
divergence, disassemble the test ROM around it, and only then propose
attempt 15 with a falsifiable single-axis hypothesis.

Full per-ROM diff outputs, validation gauntlet, and reproduction
instructions for the Mesen2 MMC3A override at
`docs/audit/session-16-c1-attempt14-prereq-infrastructure-2026-05-22.md`.

### Decision update (2026-05-22, Session-17) — C1 attempt 15 prereq infrastructure landed; Phase 2 hypothesis on a NON-CPU axis

Session-17 executed Recommended #1 from Session-16 (per-instruction
divergence trace in the post-boot in-test-loop window). The outcome is
the **highest-leverage empirical reframe in the entire C1 series**:

- The PASSING `cpu_interrupts_v2/4-irq_and_dma` is byte-identical to
  Mesen2 at every cycle-aligned common record (36,102 records, 100%
  PC-equal). The Session-16 "+10 dot constant" finding was a frame-1
  false-positive that aligns away under proper anchoring. There is no
  load-bearing dot-offset residual on PASSING tests.
- The FAILING `cpu_interrupts_v2/{2-nmi_and_brk, 3-nmi_and_irq}` first
  diverge at a `BIT $2002` instruction inside blargg's `sync_vbl`
  precise-synchronization loop. Mesen2's `$2002` read returns VBL=1;
  RustyNES's returns VBL=0. The two emulators are on opposite sides
  of the documented nesdev "$2002 race window" (`PPU_registers` wiki:
  "Reading the flag on the dot before it is set (scanline 241 dot 0)
  causes it to read as 0 and be cleared"). **This is a PPU-axis
  divergence, NOT a CPU IRQ-sample-point issue.**
- The FAILING `cpu_interrupts_v2/5-branch_delays_irq` exhibits the
  same pattern but downstream — first 19,465 parallel-walk records
  match, then divergence at an IRQ service event whose timing is
  determined by the earlier PPU $2002 race.
- The FAILING `mmc3_test_2/4-scanline_timing` exhibits ZERO PC
  divergence in the entire 250 k-350 k cycle window — both emulators
  execute identical instruction sequences. The sub-test #3 failure is
  exclusively in the out-of-window IRQ assertion at cycle ~2,203,969,
  already characterized post-B4 as a 1-CPU-cycle bracket on the
  canonical `T_last - 1` axis.

#### Implication for the prior 12 rollbacks

All 12 prior C1-axis attempts (Attempts 1-4 + B4 threshold + post-B4
mid-cycle snapshot + Sessions 5-12 follow-ups) targeted CPU IRQ-sample-
point timing axes (`T_last - 1`, NMI-hijack window, M2-phase poll,
$C001 reload-pending discriminator, etc.). Session-17 demonstrates that
**those axes are correct for `mmc3_test_2/4` sub-test #3 but WRONG for
the three `cpu_interrupts_v2/{2,3,5}` failures**. The prior 12 attempts
were exhaustively bisecting the wrong axis for 3 of the 4 failing tests.

This is consistent with — and post-hoc explained by — the empirical
record: NO prior attempt flipped any cpu_interrupts_v2 sub-ROM to
strict pass even when other CPU IRQ-pipeline behavior changed
substantially. The PPU `$2002` race window is independent of every
CPU IRQ axis the 12 attempts touched.

#### Why Phase 3 (code change) is not attempted this session

The PPU VBL-set-dot axis touches the same machinery Session-13 already
moved by +344 PPU dots. Naive shifts in either direction (earlier VBL-
set, or shifted $2002 register-read latch within the 4-cycle `BIT abs`)
predictably regress orthogonal surfaces: `ppu_vbl_nmi/*` (10/10 strict
pass currently), NMI service timing on the entire CPU-IRQ suite, the
60-ROM commercial oracle baselines, and potentially the AccuracyCoin
PPU-behavior sub-tests recently flipped by the session-8 BG-pipeline
cycle-9 reload fix.

A clean Phase 3 requires:

1. Per-PPU-dot precision oracle on the cpu_interrupts_v2/2 BIT $2002
   divergence point (Session-10's `mesen2_ppu_trace.lua` infrastructure
   re-run on a tight cycle window).
2. A `$2002`-race-window unit test in `crates/rustynes-ppu/tests/` that
   exercises the race deterministically — bisect RustyNES's actual
   behavior against Mesen2's and the wiki's documented semantics.
3. ONLY after those two yield a falsifiable 1-line code change with a
   predicted trace-fixture diff: implement under feature flag.

#### What stays valid in prior Decision sections

The structural claim — that the CPU per-cycle IRQ sample point, the
bus's IRQ poll point, and the PPU's A12 emission dot cannot be moved
independently — REMAINS valid for `mmc3_test_2/4` sub-test #3.
Session-17 does not invalidate that claim; it ADDS a **distinct
load-bearing axis** (PPU `$2002` race window) for the
`cpu_interrupts_v2/{2,3,5}` cohort that is orthogonal to the MMC3 #3
axis.

The full v1.0.0 IRQ-timing rework therefore needs TWO axes closed:

1. **PPU `$2002` race window**: the Session-17 finding. Flips
   `cpu_interrupts_v2/{2,3,5}` (3 of 4 failing tests).
2. **CPU `T_last - 1` IRQ sample point**: the canonical Session-13/14
   target. Flips `mmc3_test_2/4` sub-test #3.

Both are independent. They can land in separate sessions and the
4-tests-to-flip count is additive: PPU axis = 3 flips, CPU axis = 1
flip, both = 4 flips (all `#[ignore]`'d tests cleared).

#### Recommended Session-18 sequence

1. Per-PPU-dot trace on `2-nmi_and_brk` cycles 295,400-295,430 using
   `scripts/mesen2_ppu_trace.lua` (Session-10 infra) + the RustyNES
   `ppu-state-trace` feature gate. Cross-diff dot-by-dot.
2. Per-PPU-cycle unit test for $2002 race window in
   `crates/rustynes-ppu/tests/`. Sweep the read timing N cycles before /
   after scanline 241 dot 1; tabulate RustyNES's actual returned
   value at each N; compare against the wiki spec.
3. Visual6502 / PPU netlist consult (if available in `ref-docs/`).
4. Feature-flagged $2002 race-window fix under `cpu-c1-attempt-16`
   (if the per-dot trace + unit test yield a single-axis hypothesis).
5. Cycle the `T_last - 1` axis SEPARATELY in a later attempt.

Full per-ROM diff outputs at
`docs/audit/session-17-c1-attempt15-per-instruction-divergence-2026-05-22.md`.

### Decision update (2026-05-22, Session-18) — Attempt 16 PPU-axis predicate narrowing investigated and rolled back

Session-18 executed Recommended #2 from Session-17 (the `$2002`
race-window unit test) AND Recommended #4 (the feature-flagged
predicate-narrowing fix). The Phase 2 unit test landed as permanent
regression-guard infrastructure
(`crates/rustynes-ppu/src/ppu.rs::tests::vbl_race_window_2002_read_sweep`);
the Phase 5 implementation was reverted because target test
failure-shape was byte-identical between flag states.

#### Unit-test empirical truth-record

The `vbl_race_window_2002_read_sweep` test sweeps `$2002` reads across
the boundary scanline 240/dot 339 through scanline 242/dot 1 and
tabulates: read value, bit 7, `suppress_vbl_this_frame` after read,
`PpuStatus::VBLANK` after read.

| sl | dot | read | bit7 | suppress? | PPU.VBLANK? |
|----|-----|------|------|-----------|-------------|
| 240 | 339 | 0x00 | 0 | false | false |
| 240 | 340 | 0x00 | 0 | false | false |
| **241** | **0** | **0x00** | **0** | **true** | **false** |
| **241** | **1** | **0x80** | **1** | **true** | **false** |
| 241 | 2 | 0x80 | 1 | false | false |
| 241 | 3+ | 0x80 | 1 | false | false |

The dot-0 row is the documented nesdev "read returns 0 and clears the
flag" case. The dot-1 row is the documented "race window" but the
wiki + Mesen2 + RustyNES disagree on the value of bit 7 at exact
dot 1 (wiki: ambiguous; Mesen2 source: post-`Exec` set is ordered
AFTER the cycle counter increment so cycle 1 reads see VBL=1 + no
suppression; RustyNES: post-`tick` set is ordered BEFORE the read
issue so the read at dot 1 sees VBL=1 AND suppresses).

#### Why the predicate change does not flip the target tests

The failing `BIT $2002` reads in blargg's `sync_vbl` loop land on
**scanline 241 dot 0** in RustyNES — NOT dot 1. The Session-17 trace
"Mesen2 reads $80, RustyNES reads $00" finding was correct in both
halves but the two emulators are reading at *different PPU dot
positions*, not at the same dot with different predicates.

The predicate change `dot <= 1` → `dot == 0` only differs at dot 1.
At dot 0, BOTH predicates produce identical behavior (suppression
latches, read returns 0). The failing tests read at dot 0, so the
predicate cannot move them.

#### The actual load-bearing axis

The 1-PPU-dot offset between Mesen2 and RustyNES at the failing read
is **structural**: it comes from the per-cycle CPU-vs-PPU access
interleaving order, not from the suppression predicate's literal dot
range.

RustyNES `crates/rustynes-cpu/src/cpu.rs::read1`:

```rust
fn read1<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
    let v = bus.cpu_read(addr);      // PPU at end-of-prior-cycle
    self.idle_tick(bus);             // PPU ticks 3 dots
    v
}
```

Mesen2 `Core/NES/NesCpu.cpp::MemoryRead`:

```cpp
uint8_t NesCpu::MemoryRead(uint16_t addr, ...) {
    ProcessPendingDma(addr, operationType);
    StartCpuCycle(true);             // PPU advances to mid-cycle
    uint8_t value = _memoryManager->Read(addr, operationType);
    EndCpuCycle(true);               // PPU advances to end-of-cycle
    return value;
}
```

RustyNES reads BEFORE the cycle's PPU ticks; Mesen2 reads AFTER part
of them. Same `_startClockCount + _endClockCount` cycle length;
different ordering. The structural fix would mirror Mesen2's
`StartCpuCycle → Read → EndCpuCycle` order in RustyNES — but that is
the same axis Attempts 1 (intra-cycle CPU phase split) and 4 (bus
access-ordering swap) tried and rolled back at. It is NOT a 1-line
fix; the calibration impact spans `ppu_vbl_nmi/*`, sprite-hit timing,
DMC DMA-during-read events, and the AccuracyCoin `Sprite 0 Hit` / `$2007 read w/
rendering` sub-tests.

#### Recommended Session-19+ direction

The next attempt must target the access-interleaving axis with the
full coordinated-rework discipline — NOT another predicate-axis
attempt. Required prereqs:

1. Per-PPU-dot trace at the divergent `BIT $2002` reads to CONFIRM
   the predicted 1-PPU-dot offset (`scripts/mesen2_ppu_trace.lua`
   - `ppu-state-trace` feature).
2. Quantify the offset distribution at every in-window read across
   all 3 failing tests.
3. Per-CPU-cycle bus-call instrumentation to verify the offset is
   constant +1 or jitters.
4. Single-axis structural rework of `Cpu::read1` / `Cpu::write1` /
   `LockstepBus::tick_one_cpu_cycle` under feature flag
   `cpu-c1-attempt-17`. ANY pre-existing strict test regression
   (especially in `ppu_vbl_nmi/*`) = rollback per ADR-0002
   §"Stop conditions".

The 4 remaining failing tests now have two distinct axes:

- `cpu_interrupts_v2/{2,3,5}`: access-interleaving axis (Session-18+).
- `mmc3_test_2/4` sub-test #3: canonical CPU `T_last - 1` axis
  (still open from prior 12 CPU-axis rollbacks).

Both are independent and can land in separate attempts.

Full per-ROM diff outputs + validation gauntlet at
`docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md`.

### Decision update (2026-07-02, v2.0.0 beta.5) — R1/R2 bounded-effort campaign: two sessions, four new falsified levers, axis by-design-deferred beyond v2.0.0

The v2.0.0 "Timebase" beta.1→beta.4 promote (the one-clock, every-cycle-
bus-access substrate — see `docs/scheduler.md`) landed the **R1 access-
interleaving axis** Session-18 (above) diagnosed as the load-bearing gap:
every CPU cycle now runs `Cpu::start_cycle` (PPU catch-up to the PRE
split) → the actual bus access → `Cpu::end_cycle` (PPU catch-up to the
POST split), with `Bus::run_ppu_to` ticking the PPU one whole dot per
iteration rather than in a coarse batch — structurally the Mesen2
`StartCpuCycle → Read → EndCpuCycle` split this ADR's Session-18 called
for. This closed R4 (`apu_reset/4017_written`) and R3 (reclassified as a
harness bug) by beta.3/beta.4, but did **not**, on its own, close R1/R2
(`mmc3_test_2/4-scanline_timing` sub-test #3 and its `mmc3_test_v1/{4,5,6}`
siblings) — the beta.3 plan escape hatch (Risks #3) deferred a dedicated
closure attempt to a bounded post-promote campaign, which the maintainer
authorized explicitly for beta.5. Two independent sessions ran that
campaign on 2026-07-02, on the fully-promoted core. Both are clean
falsifications — no regression, no partial flip, all sacred gates held —
and both are now added to the DO-NOT-RETRY list alongside the prior 17.

#### Session A — batch-boundary re-phasing (`docs/audit/r1r2-closure-campaign-2026-07-02.md`)

**Ground truth** (fresh `irq_trace` on the promoted core; the previously-
committed goldens were stale pre-promote and were regenerated as part of
this session — see `crates/rustynes-test-harness/golden/irq_trace/
mmc3_test_2_4_scanline_timing.{csv,dmc.csv,svc.csv}`): the mapper's
`irq_pending` asserts at (frame 43, scanline 0, dot 260) and (frame 71,
scanline 0, dot 261); service happens at dots 279/280 (the 7-cycle
sequence + `T_last-1` recognition — the *service* leg is exact).
Sub-test #2 passes ("not yet" at window 6975); sub-test #3 fails ("taken"
at 6976) — the implementation fires ≥1 dot later than real silicon,
end-to-end.

- **Hypothesis 1 (falsified) — sprite-fetch A12 emission dot.** Mesen2's
  `LoadSpriteTileInfo` code computes `(cycle-257)%8==4` → dot 261, despite
  its own comment claiming 260 (a labeling fencepost in the reference
  implementation itself). Shifting RustyNES's emission window 260..=316 →
  259..=315 left sub-test #3's failure shape unchanged — the 1-dot shift
  is absorbed by CPU-cycle batch quantization (dots 259/260 land in the
  same `run_ppu_to` call in nearly all phases).
- **Hypothesis 2 (falsified) — the catch-up boundary fencepost.** Mesen2's
  `NesPpu::Run` is a `do`-`while` (always ticks at least once — the exact-
  boundary dot executes in the *current* half-cycle); RustyNES's
  `run_ppu_to` was check-first (the boundary dot waits for the *next*
  call). Converting to execute-then-check held AccuracyCoin 139/139,
  `cpu_interrupts_v2` 5/5, nestest 0-diff, and every previously-passing
  `mmc3` test — but left all four target brackets **unchanged**. Reverted
  (gratuitous divergence from the calibrated model, zero benefit).
- **The mechanism finding**: the ROM measures a *differential* interval
  between two same-timeline observations — the `$2002` VBL-flag read (its
  sync reference) and the IRQ-taken window. Any *consistent* re-phasing of
  the PPU-vs-CPU batch boundary shifts BOTH observations together, so the
  measured bracket is invariant to it. This retroactively explains why
  15+ prior sample-point/order/phase levers (Attempts 1-16 above) were all
  absorbed without effect. The residual is **differential**, not
  positional: something delays the mapper-IRQ observation path relative
  to the `$2002` path *specifically*. The two candidates that survive this
  analysis both require sub-batch (per-dot) visibility within the 3-dot
  window — which Session B (below) discovered had *already shipped*.

#### Session B — real M2-phase-conditional MMC3 visibility (`docs/audit/r1r2-per-dot-scheduler-attempt-2026-07-02.md`)

This session was chartered to implement "a genuine per-dot interleaved
CPU/PPU scheduler" as the logical next attempt. **Its first finding was
that the charter's premise was stale**: the beta.4 promote had already
shipped exactly that model (see above) — there was no coarse-batch
scheduler left to replace. The session redirected to the one concrete,
still-untested consequence: can MMC3 react *differently* to a qualifying
A12 rise depending on which of `run_ppu_to`'s two per-cycle calls
(pre-access / M2-low vs post-access / M2-high) produced it?

- **A previously-undocumented dead-plumbing bug, found and fixed
  (default-off).** This ADR's 2026-05-14 "Sub-dot plumbing landed" entry
  describes `Mapper::notify_a12_at_sub_dot(level, sub_dot)` as carrying a
  real 0/1/2 M2-phase value. On the *live* R1 per-instruction path it did
  not: `LockstepBus::run_ppu_to`'s `sub_dot` counter was local to each
  call and almost always read `0` on both the pre- and post-access calls,
  carrying no phase information at all (the genuinely-valued sub-dot
  counter only existed on the DMA-burst-only legacy path). This was fixed
  under a new default-off `mmc3-m2-phase-irq` feature (`rustynes-core` +
  `rustynes-mappers`, forwarded through `rustynes-test-harness`):
  `run_ppu_to` now seeds `sub_dot` from the real call-site phase, and
  `Mmc3::notify_a12_at_sub_dot` defers a post-access (M2-high) qualifying
  rise's `irq_pending_line` assertion by exactly one `notify_cpu_cycle`
  boundary (a pre-access/M2-low rise still asserts synchronously, matching
  today's unconditional behavior). 3 new unit tests confirm the deferral
  mechanism fires correctly in isolation (21/21 `rustynes-mappers` unit
  suite green under the feature). The default build is confirmed
  byte-for-byte unaffected (feature-gated, `sub_dot` starts at `0`
  unconditionally when off, identical source and behavior to before this
  session).
- **Result: mechanism-verified, zero effect on the target bracket.** With
  the feature ON, `mmc3_test_2_4_scanline_timing_currently_fails` and
  `mmc3_test_v1_4_scanline_timing_currently_fails` (the fail-loud probes)
  both still correctly detect the unflipped bracket, and regenerating the
  `irq_trace_fixture` with the feature ON vs OFF produces a byte-for-byte
  **identical** run (`kept=2203768` records, final `$6000=$03`, both
  configurations). This is a stronger result than "didn't flip" — it
  means **no qualifying A12 rise this ROM's actual execution produces
  ever lands in the post-access half of a CPU cycle**, so the
  phase-conditional lever never engages at all for this bracket, not just
  fails to help it.
- **This is not a re-derivation of a prior attempt.** It differs from
  Attempts 2/3 (a constant N-cycle pipeline applied uniformly — this is
  conditional and exactly one boundary) and from the Phase-B4 sub-dot
  *filter-threshold* attempt (which gated *acceptance*, not
  *visibility*-after-acceptance). It also isn't dismissible by Session A's
  "shifts both observations together" argument, since it is local to
  MMC3's own IRQ line, not a global batch re-phasing — it was tested on
  its own distinct merits and falsified on its own distinct grounds.
- **Disposition: the code is kept on the branch, default-off.** Unlike
  Session A's two reverted experiments (which had zero remaining value
  once falsified), this session's code fixes a genuine, previously-
  undocumented bug (the dead M2-phase plumbing) and leaves real,
  unit-tested infrastructure behind for whoever pursues the open question
  in the next section — matching the precedent Sessions 13-16 set by
  landing oracle/plumbing infrastructure on inconclusive attempts rather
  than reverting to nothing.

#### Consolidated disposition and the next candidate axis

Four new levers join the DO-NOT-RETRY list from 2026-07-02 (on top of the
17 already documented above): the sprite-fetch A12 emission-dot shift, the
`run_ppu_to` do-while catch-up-boundary conversion, and — from Session
B — both directions of the M2-phase-conditional MMC3 visibility deferral
(the mechanism works but never engages for this ROM). **Do not re-attempt
any of these four**, or any constant-pipeline / global-rephasing variant
of them, without new evidence that specifically distinguishes it from
what was already tested here.

**One genuinely new, untested axis remains, explicitly flagged for a
future dedicated session (not squeezed into v2.0.0):** the MMC3 filter's
`gap >= 3` low-time acceptance test currently operates at CPU-cycle
(integer) granularity. Real silicon's three-falling-edges-of-M2 rule is a
*falling*-edge, elapsed-low-time question — a different axis from the
*rising*-edge phase-conditional visibility Session B tested. Session B's
own finding (no qualifying rise ever lands post-access for this ROM) may
mean this axis is also moot for this specific bracket, or may not — it
concerns a structurally distinct property of the same A12 line and was
not tested by either 2026-07-02 session. Also open: whether "no
qualifying rise ever lands post-access" is specific to this ROM's phase
alignment or a structural property of NTSC MMC3 A12 timing generally (if
the latter, the entire phase-conditional branch of the search space is
dead, not just this session's specific lever — see
`docs/audit/r1r2-per-dot-scheduler-attempt-2026-07-02.md` §5.2 for the
falsifiable framing).

Per the stop-condition discipline this ADR has followed since 2026-05-13:
21+ rollbacks (17 historical + 4 from 2026-07-02, across two dedicated
same-day bounded-effort sessions run specifically to attempt closure) is
the empirical signal to stop spending v2.0.0 release budget on this axis.
The four target brackets (`mmc3_test_2/4` sub-test #3, `mmc3_test_v1/4`
sub-test #3, `mmc3_test_v1/{5,6}` sub-test #2) ship `#[ignore]`'d with
zero production-ROM impact, unchanged from the beta.3 escape hatch. Full
evidence trails: `docs/audit/r1r2-closure-campaign-2026-07-02.md` and
`docs/audit/r1r2-per-dot-scheduler-attempt-2026-07-02.md` (both local/
gitignored per this project's `docs/audit/` convention, matching every
prior session audit doc this ADR cites).

---

## Decision update (2026-07-09, v2.1.0 "Fathom" F5.0) — CLOSED

The v2.1.0 "Fathom" accuracy-remediation line put this residual through the
maintainer-agreed **instrumentation-first (F5.0)** gate: run the study *first*,
and attempt the flagged M2-half-cycle axis-B fix *only if* the study proves that
axis live. The study reviewed the two dedicated 2026-07-02 campaign audits'
ground-truth traces rather than re-deriving them (the re-open bar explicitly
forbids a re-derivation of anything on the DO-NOT-RETRY list).

**The falsifiable question F5.0 had to answer:** is there any single-axis lever
that shifts the *mapper-IRQ observation path* relative to the *`$2002` sync
path* — the residual is a **differential** 1-dot deficit, so only a
differential change can move it — without a scheduler-substrate rewrite?

**Finding — the axis is NOT live:**

1. **The deficit is differential and structurally invariant to global
   re-phasings.** The ROM measures the interval between two observations on one
   CPU timeline (`$2002` VBL read vs the IRQ-taken window); any consistent
   PPU-vs-CPU batch re-phasing shifts *both* together. This is why 15+ historical
   sample-point/order/phase levers **and** the two 2026-07-02 experiments (the
   sprite-fetch A12 emission-dot shift, and the `run_ppu_to`
   check-first→do-while catch-up-boundary fencepost) were all absorbed with the
   failure shape unchanged.
2. **The `gap >= 3` filter threshold is provably not the discriminator.** The
   qualifying sprite-fetch A12 rise's gap from the prior A12 fall is ~900k
   cycles, so any threshold (3/4/5/100) accepts it identically — tightening or
   M2-edge-refining the *threshold* cannot move the bracket.
3. **The two surviving candidates both require sub-batch (per-dot) CPU
   visibility of the A12→pending edge** — i.e. a genuine per-dot interleaved
   scheduler change, not a filter or sample-point tweak. Pursuing that risks a
   22nd rollback of the sacred **AccuracyCoin 141/141** (and the 540+ strict
   suite) for a bracket with **zero production-ROM impact** — precisely the
   trade the v2.0.0 plan's Risks #3 escape hatch names as the one place
   "aggressive yields to sacred."

**Decision:** the residual is **CLOSED as by-design-permanent**, not deferred.
The four pins (`mmc3_test_2/4` sub-test #3, `mmc3_test_v1/4` sub-test #3,
`mmc3_test_v1/5` sub-test #2, `mmc3_test_v1/6` sub-test #2) stay `#[ignore]`'d
permanently with their fail-loud `*_currently_fails` companions intact. This is
a well-understood architectural boundary of the one-clock batched-catch-up
model, fully explained by the differential-mechanism finding — it is **not an
accuracy gap** and not an open TODO. Re-opening would require a genuinely new,
falsifiable single-axis hypothesis that specifically distinguishes itself from
everything on the DO-NOT-RETRY list — none survived F5.0.
