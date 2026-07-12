# ADR 0033 — `Cpu2A03Revision` config + the DMA "unexpected read" die-revision frontier

- **Status:** Accepted
- **Date:** 2026-07-11
- **Deciders:** DoubleGate
- **Supersedes / relates to:** ADR 0002 (IRQ-timing coordination + the by-design-deferred MMC3 R1/R2 residual — the template for an *honest, unclosed* residual), ADR 0029 (one-clock / every-cycle timebase — the unified DMA engine this touches), the v2.1.7 "Hardware Revisions & DMA Frontier" plan (items P2/P3 + the 2A03 revision)

## Context

v2.1.7's DMA half is the plan's **officially-unsolved frontier**. The sub-instruction DMA machinery from the v2.0.0 "Timebase" rewrite is already mature and the committed oracle floor is green on the shipped default:

- The five `dmc_dma_during_read4` ROMs (`dma_2007_read`, `dma_2007_write`, `dma_4016_read`, `double_2007_read`, `read_write_2007`) — all status-`0`.
- Both `sprdma_and_dmc_dma` ROMs (OAM-DMA + DMC-DMA cycle-steal alignment) — both "Passed".
- `dma_timing_pin` (the AccuracyCoin `CheckDMATiming` reload-span = 4 + the `$50-$5F` DMC-during-OAM landing sweep on KEY) — green.
- `cpu_dummy_writes_oam`, `oam_read`, `oam_stress` — green.
- AccuracyCoin **141/141**, nestest **0-diff**.

The remaining frontier item is the **2A03 die-revision "unexpected DMA" extra read**: nesdev
([DMA](https://www.nesdev.org/wiki/DMA)) notes that when a DMC-DMA halt coincides with an
OAM-DMA halt (the "double-halt" overlap) some silicon inserts an **extra** re-read of the
parked 6502 address bus, and this differs between 2A03 mask revisions (RP2A03G vs RP2A03H).
The v2.1.7 plan asks for a `Cpu2A03Revision` config gating this behavior, for the games known
to be sensitive to the adjacent DMC-glitch controller-read corruption (Ultimate Stuntman,
Battletoads, Time Lord, Mig-29, Captain Planet, Paperboy).

**The hard constraint is non-negotiable:** the default build stays byte-identical (AccuracyCoin
141/141, every DMA oracle ROM `Passed`, save-state round-trip byte-identical). Per the release's
honesty gate we must model to the public oracle's limit and **document** the rest — never fake a
pass or claim accuracy we do not verify.

### What the references actually model (v2.1.7 survey)

A full survey of the vendored references (Mesen2, ares, BizHawk, TriCNES, fceux, nestopia,
GeraNES, higan) established the ground truth:

1. **DMC/OAM cycle-stealing, get/put alternation, OAM alignment, the aborted DMC-DMA path, and
   the DMC-glitch register-readout corruption on `$2007`/`$4015`/`$4016`/`$4017`** are all
   well-modeled — Mesen2 (`NesCpu.cpp` `ProcessPendingDma`/`ProcessDmaRead`, `DeltaModulationChannel.cpp`)
   is authoritative; TriCNES is the most micro-accurate state machine; ares is minimal. RustyNES's
   unified DMA engine already ports these (`unified_dma_cycle_impl`, `dmc_dma_read`,
   `raw_oam_dma_read`, `replay_dma_noop_read`), which is why the oracle floor is green.

2. **The console-type DMC-glitch distinction is real and reference-grounded** but is a *different
   axis* from the die revision: Mesen2's `isNesBehavior` (`NesCpu.cpp:349`,
   `ConsoleType != Hvc001`) clocks a controller on *every* DMA idle read on the original Famicom
   vs only the *first* on NES-001/AV-Famicom. RustyNES's default already re-clocks the held
   register on each DMC halt/put cycle (`replay_dma_noop_read`, NTSC), which is what makes
   `dma_4016_read.nes` pass — i.e. the default already sits at a self-consistent point on this
   axis. Splitting it into a user-selectable console-type knob is deferred (it is not the plan's
   requested axis and would risk the passing ROM).

3. **The 2A03 die-revision (RP2A03G vs RP2A03H) DMA difference is modeled by NO reference and
   verified by NO public test ROM.** Every tree was grepped for `RP2A03G/H`, `2A03G/H`, die
   revision, `dmcDmaGlitch`, "unexpected/extra/double/spurious read"; the only hit is a MAME
   machine-list *data* file (`BizHawk/.../mame_machines.txt`), not emulation code. None of the
   eight emulators branch DMA cycle behavior on 2A03 stepping. **This is a genuine open frontier,
   not a port.**

## Decision

1. **Ship the config surface, default byte-identical.** Add an additive
   `Cpu2A03Revision { Rp2A03G (default), Rp2A03H }` enum to `rustynes-core`, plumbed
   `Nes::set_cpu_2a03_revision` / `cpu_2a03_revision` → `Bus`. `Rp2A03G` is the default and is
   **bit-identical** to the pre-v2.1.7 core. The revision is a **config knob re-applied on load,
   NOT part of the save-state** (like the optional OAM-decay model): the only state it influences
   is fully re-derived from the deterministic timeline, so a save/restore round-trip stays
   byte-identical for a fixed revision, and no snapshot version bump / tail is required.

2. **Model the extra read at its mechanism-correct location.** The gate lives at the single cycle
   in `unified_dma_cycle_impl` where a *halted* DMC squeezes a parked-address re-read into an
   OAM-owned read cycle during a DMC+OAM overlap (`get` half, `uni_oam_active && !uni_oam_halt`,
   `in_dmc_dma`). `Rp2A03G` performs the re-read (the current behavior); `Rp2A03H` omits it. The
   suppression is deterministic and cannot desync the transfer — `replay_dma_noop_read` only
   re-triggers a register's side-effect (a `$2007` buffer advance / `$4016`-`$4017` shift /
   `$4015` IRQ-clear); it ticks no time and advances no DMA counter.

3. **Record the direction of `Rp2A03H` as an unverified hypothesis.** Because no reference or ROM
   proves it, the `Rp2A03H` arm's "omit the extra read" direction is a modeled guess, flagged as
   such in the enum docs and never selected by the shipped path.

## The residual we could NOT close (the honest finding)

Direct instrumentation of the ported engine (a synthetic ROM: enable DMC looping at the fastest
byte-fetch rate, then a tight `STA $4014` / `LDA $2007` loop to stack an OAM DMA over a DMC DMA
with `$2007` in view) measured the gate's reachability precisely:

- The DMC+OAM overlap **does** occur (~300 co-active cycles), and the gated extra-read branch
  **does fire** (~75×).
- **But its parked address is NEVER a side-effect register** (`overlap_reg_fire = 0`), so
  `replay_dma_noop_read(halted_addr)` is a no-op **every** time.

The mechanism: an OAM DMA is triggered by the `$4014` *write* and drains on the **next opcode
fetch** (the first CPU *read* after the write), so during any DMC+OAM overlap the parked 6502
address is the post-`$4014` instruction fetch in PRG — never `$2002/$2007/$4015/$4016/$4017`. For
the extra read to be *observable* the CPU would have to be halted *on a register operand read*
while OAM is already in flight, which the write-halts-immediately model precludes. Consequently
**`Rp2A03H` is byte-identical to `Rp2A03G` on every committed DMA oracle ROM and on every
constructible scenario** — the die-revision extra read is *unobservable* on this engine, not
merely unverified.

This is the ADR-0002-style outcome: a mechanism-level finding, recorded rather than papered over.
Closing it would require either (a) a public test ROM that captures the RP2A03G-vs-H DMA
difference on real silicon (none exists), or (b) reworking the OAM parked-address model so a
register can be in view during the overlap — a substrate change that risks the sacred
byte-identity floor and is out of scope for an accuracy-hardening release.

### `dmc_dma_during_read4` sub-test disposition

**No `dmc_dma_during_read4` sub-test is made to fail or newly `#[ignore]`'d.** All five ROMs (plus
both `sprdma_and_dmc_dma` variants) continue to `Pass` on the default `Rp2A03G` build — they are
the verified floor, not the residual. There is no ROM for the die-revision axis to gate, so there
is nothing to mark expected-fail; the residual is the *absence* of any oracle, documented here and
pinned by the `rp2a03h_matches_rp2a03g_documented_residual` regression guard.

## Consequences

### Positive

- The requested `Cpu2A03Revision` config surface exists, is deterministic, and never perturbs the
  default (`Rp2A03G` == pre-v2.1.7, byte-identical: AccuracyCoin 141/141, nestest 0-diff, all DMA
  ROMs `Passed`, save-state round-trip byte-identical).
- The extra-read model sits at the mechanism-correct location, so it becomes live immediately if a
  future engine change ever exposes a register during the overlap.
- The frontier is documented with a reproducible measurement, not an over-claim — the honesty gate
  is satisfied.

### Negative / deferred

- `Rp2A03H` is currently behaviorally indistinguishable from `Rp2A03G` (no public oracle can drive
  it) — it is a forward-looking hook, and its direction is unverified.
- The reference-grounded **console-type** DMC-glitch axis (Mesen2 `isNesBehavior`, NES-001 vs
  Famicom controller re-clocking) is a separate, deferred knob; the default already implements a
  self-consistent point on it (dma_4016_read passes). Follow-up: `T-PS-dmc-glitch-console-type`.
- No frontend UI is wired for the revision selector this release (core + config API only).
