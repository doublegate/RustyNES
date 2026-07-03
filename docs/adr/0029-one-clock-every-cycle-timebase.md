# ADR 0029 — The One-Clock, Every-Cycle-Bus-Access Timebase

**Status:** Accepted. Shipped as the sole scheduler model since v2.0.0
beta.4 (PR #220); this ADR formalizes it as the canonical architecture
description, superseding the dot-lockstep (`tick_one_dot`) framing that
`docs/architecture.md` and (until this ADR's companion doc pass) the older
sections of `docs/scheduler.md`, `docs/cpu-6502.md`, and `docs/apu-2a03.md`
described as current.
**Date:** 2026-07-03
**Author:** RustyNES maintainers
**Supersedes:** The dot-lockstep scheduling description in
`docs/architecture.md` §"The master clock is the PPU dot" and the
equivalent framing in `docs/scheduler.md`'s pre-v2.0.0 primer sections
(both kept as explicitly-marked historical content, not deleted — per this
project's "never rewrite a superseded description in place" documentation
convention, see `master-core/modules/40-docs-and-adrs.md`).

## Context

RustyNES's original (v0.9.0 → v1.10.0) scheduler advanced the PPU one dot
at a time (`tick_one_dot`), with the CPU ticking on every third dot (NTSC)
and five separate counters independently tracking cycle position across the
CPU, PPU, APU, and bus:

- `Cpu::master_clock` — introduced pre-v2.0.0 (W3-Stage-4, 2026-06-10) as
  inert parallel bookkeeping, not yet load-bearing.
- `Cpu::cycles` — the CPU's own independently-incremented cycle counter.
- `LockstepBus::cycle` — the bus's own independently-incremented counter.
- `LockstepBus::ppu_clock` — the PPU catch-up loop's progress marker.
- `Apu::cpu_cycle` — the APU's own independently-incremented counter.

All five were kept in sync by construction (matching increments at every
relevant call site), not by derivation — a correct but fragile invariant.
`docs/adr/0002-irq-timing-coordination.md` documents 17+ rollback attempts
across the pre-v2.0.0 IRQ-timing investigation, several of which were
structurally blocked by this fragility: Session-18 (2026-05-22) specifically
diagnosed that RustyNES's `Cpu::read1` reads BEFORE the cycle's PPU ticks
(`bus.cpu_read(addr); self.idle_tick(bus)`), while Mesen2 (the reference
implementation) splits the PPU advance AROUND the CPU access
(`StartCpuCycle(); Read(); EndCpuCycle()`) — a genuine per-dot interleaving
difference that the five-counter model could not cleanly express without
risking the counters drifting out of sync.

The v2.0.0 "Timebase" plan (`to-dos/plans/v2.0.0-master-clock-plan.md`)
committed to collapsing this substrate. The work landed across four betas:

- **beta.1** (PR #217): the one-clock counter collapse behind a default-off
  feature flag. `Cpu::cycles` began being ASSIGNED from the canonical
  counter rather than independently incremented; the four other counters
  became DERIVED. AccuracyCoin held 100% (139/139) flag-on.
- **beta.2** (PR #218): every-cycle-bus-access — every CPU instruction
  cycle became a real bus access (no "busless" filler cycles), the
  STOP-OR-GO gate the plan defined as make-or-break for the whole
  rewrite. Passed clean: AccuracyCoin 139/139, the `cpu_interrupts_v2`
  trio, and the R5 DMC-DMA-span pin all held simultaneously.
- **beta.3** (PR #219): the cycle-accurate warm reset, closing residuals R3
  (reclassified as a harness bug) and R4 (the `apu_reset`/`4017_written`
  bracket).
- **beta.4** (PR #220, "THE PROMOTE"): the `mc-one-clock-v2` feature flag
  was DELETED and the beta.1–beta.3 substrate became the ONLY scheduler
  path, unconditionally. This is the release's designated breaking-behavior
  change per the plan's own ADR-0003-MAJOR-boundary tier.

The result, verified structurally by the `one_clock_invariants` regression
pin (`crates/rustynes-test-harness/tests/one_clock_invariants.rs`): the
residues `(master_clock - divisor * cycles, bus.cycle - cpu.cycles,
apu.cpu_cycle - cpu.cycles)` are frame-over-frame CONSTANT — `(12, 0, 0)` on
the shipped default — across both nestest (CPU/branch-heavy) and the
AccuracyCoin DMC+OAM DMA window (the historical 17-rollback drift surface).

A follow-up bounded-effort investigation on the promoted core (two sessions,
2026-07-02, recorded in `docs/adr/0002-irq-timing-coordination.md`'s
"Decision update (2026-07-02..." section) discovered that the promoted
`Cpu::start_cycle`/`Cpu::end_cycle` + `Bus::run_ppu_to` pattern already IS,
structurally, the per-dot split-around-the-access model Session-18
identified as the missing piece — it shipped as an emergent consequence of
the one-clock collapse, without anyone having explicitly connected it to
that diagnosis until this follow-up investigation. The residual MMC3
IRQ-timing bracket (R1/R2) that motivated Session-18's original diagnosis
did NOT close on the promoted core, and the follow-up investigation's
mechanism finding (the residual is a *differential* measurement invariant to
any consistent batch re-phasing) suggests the true fix needs finer-than-CPU-
cycle granularity than even this model provides — an axis explicitly
deferred beyond v2.0.0, not something this ADR claims to have solved.

## Decision

**The one-clock, every-cycle-bus-access model is the canonical scheduler
architecture, effective immediately and permanently (no flag, no
alternative path).** Concretely, for any future contributor or doc reader:

1. **One counter.** `LockstepBus::cycle` is the single per-cycle authority.
   `Cpu::cycles`, `Apu::cpu_cycle`, and any other cycle-position reads are
   assigned FROM it (directly or via a fixed offset/divisor relationship),
   never incremented independently. `Cpu::master_clock` remains the CPU's
   own master-clock-unit counter (NTSC: 12 per CPU cycle; PAL 16; Dendy
   15), related to `cycles` by the pinned residue invariant.
2. **Every cycle is a real bus access.** No CPU instruction cycle is
   "busless" — every cycle performs (or explicitly no-ops as a genuine
   silicon-faithful dummy read/write) a real `Bus::cpu_read`/`cpu_write`
   call, matching real 6502 behavior at every cycle rather than only at
   cycles where the emulator's dispatch loop happened to need a value.
3. **Split-around-the-access PPU catch-up.** `Cpu::start_cycle` advances
   `master_clock` by the PRE (φ1) split and catches the PPU up to
   `master_clock - PPU_OFFSET` via `Bus::run_ppu_to` BEFORE the actual bus
   access; `Cpu::end_cycle` advances by the POST (φ2) split and catches the
   PPU up again AFTER. `run_ppu_to` ticks the PPU one whole dot per
   iteration in a loop, not in a coarse multi-dot batch. This mirrors
   Mesen2's `StartCpuCycle → Read → EndCpuCycle` structure (see Context)
   rather than the old read-then-advance-3-dots pattern.
4. **DMA is unified and interleaved**, not a separate stepping mode: every
   DMA cycle (OAM, DMC load/reload) is a first-class `start_cycle`/
   `end_cycle` pair on the same one-clock path, not a parallel bookkeeping
   fold. (The old `dma_mc_consumed`-based coherence fold this replaced was
   deleted in beta.4; the still-live `dma_mc_consumed` field on
   `LockstepBus` serves a distinct, narrower DMA-span accounting purpose —
   see the field's own doc comment, not this ADR.)
5. **Reset is a clocked sequence**, not an instantaneous state reset: the
   warm-reset path advances through its 8-cycle delay on the same one-clock
   timeline, enabling the cycle-accurate `$4017` re-write (closing R4).
6. **The old dot-lockstep description is historical, not current.**
   `docs/architecture.md` §"The master clock is the PPU dot" and any
   remaining pre-v2.0.0 primer sections in `docs/scheduler.md` describe the
   ORIGINAL v0.9.0–v1.10.0 model. They are kept (never rewritten in place,
   per this project's documentation convention) but must be read as
   engine-lineage history, with a clear banner pointing to this ADR and the
   "v2.0.0 Timebase" model as the CURRENT architecture — mirroring the
   treatment `docs/cpu-6502.md` and `docs/apu-2a03.md` already received
   during beta.2/beta.4.

## Consequences

### Positive

- Eliminates an entire historical class of bug (the five counters drifting
  out of sync under an edge case some call site's increment missed) by
  construction — there is only one counter to get wrong now, not five to
  keep synchronized.
- Enables the split-around-the-access model, closing R3/R4 and providing
  the structural foundation the 2026-07-02 follow-up investigation used to
  test (and, honestly, fail to close) the R1/R2 residual — a foundation
  that did not exist before beta.1-4.
- `one_clock_invariants` is now a permanent, cheap regression guard against
  any future re-introduction of counter drift.
- AccuracyCoin held 100% (139/139) at every gate across all four
  landing betas — the collapse introduced zero net accuracy regression
  while closing two residuals (R3, R4) outright.

### Negative / Costs

- **Breaking change**, by design: this ADR's companion, ADR 0028, documents
  the save-state format break this required (see that ADR for the full
  save-state/movie decision).
- The every-cycle-bus-access model adds a dummy read on cycles that were
  previously "busless" — measured as a small, sub-1%-of-frame-cost
  addition (a single `mapper.cpu_read` per formerly-busless cycle),
  well within the project's performance budget (re-verified in beta.4's
  perf re-baseline: both configurations clear the 16.639 ms NTSC deadline
  with wide margin).
- Contributors investigating cycle-timing bugs must now reason about the
  split-around-the-access model (`start_cycle`/`end_cycle` pairs) rather
  than the simpler-sounding-but-fragile "read then advance 3 dots" mental
  model. This is judged a net improvement (it matches real silicon more
  closely and is the model Mesen2, the project's primary accuracy
  reference, already uses) but is a genuine learning-curve cost for anyone
  who internalized the old model from the pre-v2.0.0 docs.

### Neutral

- The R1/R2 MMC3 IRQ-timing residual remains open, now with a
  mechanism-level explanation (see ADR 0002's 2026-07-02 update) rather
  than closed. This ADR does not claim victory on that axis — it documents
  the scheduler model the residual was tested against, honestly, including
  the negative result.

## Alternatives considered

1. **Keep the five-counter model, patch the specific IRQ-timing bugs
   ad hoc.** Rejected: this was the status quo for 17+ documented rollback
   attempts (`docs/adr/0002-irq-timing-coordination.md`) — the structural
   fragility of parallel-incremented counters was itself identified as a
   contributing cause of why isolated point-fixes kept failing to stick or
   regressing something else.
2. **A full per-dot interleaved scheduler with sub-CPU-cycle mapper
   visibility** (going further than the split-around-the-access model to
   give mappers real M2-phase-conditional behavior). Attempted as part of
   the 2026-07-02 follow-up investigation (a default-off `mmc3-m2-phase-irq`
   feature); found to work correctly in isolation but to have zero
   differential effect on the R1/R2 target ROM (no qualifying A12 rise ever
   lands in the relevant half-cycle for that specific test). Kept as
   default-off tested infrastructure for whoever picks up the falling-edge
   `gap >= 3` low-time-accounting axis next, per that investigation's own
   disposition — not promoted to the default path by this ADR.
3. **Ship the promote silently, without a dedicated ADR.** Rejected: this
   is the single largest architectural change in the project's history
   (touching the CPU/PPU/APU/bus/DMA/reset scheduling core simultaneously)
   and the save-state break it necessitates (ADR 0028) needs a canonical
   architecture reference to cite. `docs/architecture.md`'s planned
   re-baseline (tracked alongside this ADR) needs exactly this document to
   point readers at.
