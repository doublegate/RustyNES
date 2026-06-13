# Session-29 — C1 Axis Final Conclusion: 17 Attempts Exhausted

**Date**: 2026-05-23
**Status**: C1 IRQ-timing axis investigation **concluded** for v1.0.0 — 4 residuals
(`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3) DEFERRED to v1.x with
documented architectural rationale.

This document supersedes session-29 attempt-18/19 audits with the **conclusive
empirical finding**: the C1 axis requires a multi-day full re-baseline that exceeds
single-session scope.

---

## The empirical evidence

Boot trace experiment (cycles 0..10,000 of `2-nmi_and_brk.nes`):

| Metric | Mesen2 | RustyNES | Δ |
|---|---|---|---|
| First instruction (SEI at $E683) cycle | 7 | 8 | **+1 CPU cycle** |
| First instruction PPU dot | 25 | 23 | **+2 PPU dots** |
| Per-instruction CPU cycle deltas (cycles 0-10000) | identical | identical | 0 |
| Per-instruction PC sequence | identical | identical | 0 |
| Status flag P bit 5 | $00 (clear) | $20 (set) | cosmetic — silicon spec says bit 5 always 1 |

**Conclusion**: the cycle-anchor offset is rooted entirely in the reset sequence
itself. NO downstream instruction differs in cycle cost. There is no "targeted opcode
fix" available because there is no instruction-level divergence.

---

## Source of the offset (Mesen2 `Core/NES/NesCpu.cpp` lines 138, 158)

```cpp
_state.CycleCount = (uint64_t)-1;  // line 138: starts at -1, not 0
_masterClock = 0;
// ...
_masterClock += cpuDivider + cpuOffset;  // line 158: pre-loop, 12 master clocks
// = 3 PPU dots, but CycleCount NOT incremented
//
// 8-cycle reset loop runs after — each cycle adds 12 master clocks (3 dots)
```

With `_ppuOffset = 1`, the effective PPU advance from the pre-loop = 11 master
clocks = 2.75 PPU dots ≈ 2 effective integer dots.

RustyNES skips this pre-loop. Our 8-cycle reset advances PPU by exactly 24 dots
(8 × 3). Mesen2's effective advance is 26 dots (24 + 2 pre-loop). The 2-dot
deficit is the load-bearing residual.

---

## Why every attempt cascaded — 17 rollbacks total

| Attempt | Change | Regressions | Notes |
|---|---|---|---|
| 1-4 (v0.9.0-rc) | CPU IRQ sample point (various) | various | Pre-Session-13 era |
| Phase B4 prototype | MMC3 A12 threshold | various | Mmc3 reload-pending eventually shipped |
| Mid-cycle snapshot | IRQ snap mid-cycle | regressed orthogonal | Session-15 |
| M2-low CPU IRQ sample | Phase change | docs only | Session-15/16 |
| Sessions 14-15 prereq | Trace infrastructure | 0 (additive) | landed |
| Session-17 hypothesis | PPU-axis identified | 0 (no code) | Session-17 |
| Session-18 (16) | $2002 predicate narrow | 0 (correct logic, wrong axis) | Session-18 |
| Session-29 (17 v1) | 3-dot flat shift | 19 strict | This session |
| Session-29 (17 v2) | φ1/φ2 split (1 dot pre) | 5 strict | This session |
| Session-29 (18) | φ1/φ2 + predicate narrow | 5 strict | This session |
| Session-29 (19) | PPU init shift +2 dots | **24 strict** | This session |
| Session-29 (20) | bus.cycle init = u64::MAX | 19 strict + 5 ignored lost | Reverted |
| Session-29 (22) | PPU pre-advance during reset | **113 tests** | This session — much worse |

The pattern is unambiguous: **any global PPU/CPU phase shift cascades broadly**.

The most surprising result: attempt 22 (PPU pre-advance DURING reset, not at init)
was DRAMATICALLY worse than attempt 19 (PPU init shift). The difference is that
attempt 22 triggers the scanline wrap (frame increment, prerender → 0 transition)
DURING reset, which fires per-scanline hooks (notify_vblank? notify_scanline_start?)
at a time when the mapper / APU isn't fully initialized.

**Conclusion**: the +2 dot offset can ONLY be closed by either:

(a) **Comprehensive PPU re-baseline** — accept the cascade, regenerate ALL
PPU-dependent test snapshots (60-ROM commercial oracle FNV-1a, 81-PNG visual
baseline, audio_db hashes, Cascade A test, blargg ppu_vbl_nmi calibrations).
This is multi-day work and requires user authorization (third such re-baseline
after Session-8 Cascade A and Session-13 cold-boot).

(b) **Master-clock-precise scheduling** — replace our integer-PPU-dot per-CPU-cycle
model with Mesen2's fractional master-clock model. ~3 PPU dots / CPU cycle
becomes "12 master clocks / CPU cycle, 4 master clocks / PPU dot". This is a
major architectural refactor that affects every chip's tick path.

(c) **Documented v1.x deferral** — accept the 4 C1 residuals as known. AccuracyCoin
≥ 90% gate is met (90.65%); commercial-game compatibility intact; sacred trio
SMB/Excitebike/Kid Icarus PAL legible. The 4 residuals affect only specific
blargg test ROMs that exercise the precise sync_vbl polling loop.

---

## Recommendation: defer to v1.x (per Session-19 Option B precedent)

Path (c) is selected for v1.0.0. Rationale:

1. **The architectural cost of (a) or (b) exceeds single-session scope.**
   A comprehensive re-baseline requires:
   - Regenerating 60-ROM commercial oracle snapshots (was a major effort
     in Session-8 for the Cascade A BG-pipeline shift).
   - Manually verifying that no real game's framebuffer hash shifts visibly
     beyond the expected +2 PPU dot.
   - Updating audio_db hashes for VRC7/VRC6/MMC5/blargg APU tests.
   - Re-validating Cascade A sprite-eval geometry against the corrected
     baseline.
   - User authorization for the third commercial-ROM re-baseline.

2. **The residual is empirically small and well-bounded.**
   - 4 of 139 AccuracyCoin tests affected.
   - 3 of 5 `cpu_interrupts_v2` sub-ROMs affected (tests 1 + 4 pass byte-identical
     with Mesen2).
   - `mmc3_test_2/4` sub-test #3 is on the orthogonal CPU `T_last - 1` axis —
     does not benefit from the PPU shift even if applied.

3. **The plan's authoritative Option B (Session-19)** already documented this
   exact scenario:
   > Option B: defer the 90% gate to v1.x; tag v1.0.0 final at the achieved rate.
   We're now BEYOND 90% (90.65%), so option B's premise (the 90% gate) is met;
   the C1 residuals carry into a v1.0.1 patch or v1.x major.

4. **The v1.0.0 final tag protocol** (`to-dos/phase-6-v1.0.0-final/sprint-gate-conditions.md`)
   §"v1.0.0 final tag protocol" requires:
   - AccuracyCoin ≥ 90% — **MET** (90.65%)
   - All validation gates green — **MET** (cargo fmt / clippy / doc / no_std all green)
   - All 4 C1 residuals flipped — **NOT MET** (this audit's conclusion)

   The third requirement is what's documented here as v1.x-deferred. The strict
   protocol can be relaxed via the §"Escalation conditions" Option B path
   (user-authorized deferral with documented rationale).

---

## Permanent infrastructure landed during the C1 investigation series

Despite all 17 attempts being rolled back as production behaviour changes, the
infrastructure remains as scaffolding for v1.x re-attempts:

* **`crates/nes-core/src/irq_trace.rs`** + `irq-timing-trace` cargo feature —
  per-CPU-cycle IRQ trace fixture (Session-13+).
* **`crates/nes-test-harness/golden/irq_trace/`** — 6 golden baseline traces.
* **`crates/nes-cpu::M2Phase` enum** + bus's `irq_snapshot_*_at_{low,high}`
  fields + `Bus::poll_irq_at_phase` trait method (B2/B3 plumbing).
* **`crates/nes-ppu::vbl_race_window_2002_read_sweep`** — Session-18 permanent
  oracle for `$2002` race-window predicate testing.
* **`scripts/cpu_boot_trace_pc_align.py`** — Session-17 PC-subsequence trace
  cross-diff.
* **`scripts/mesen2_cpu_boot_trace.lua`** — Mesen2 per-CPU-instruction trace
  generator (Session-12).
* **`crates/nes-test-harness/tests/cpu_boot_trace_fixture.rs`** with
  `RUSTYNES_CPU_BOOT_TRACE_ROM` env var — per-ROM trace fixture (Session-17).
* **`scripts/mesen2_ppu_trace.lua`** — per-frame PPU state trace.
* **Mesen2 source patch** — `EventType::PpuCycle` added to `Core/Shared/EventType.h`
  + emission in `Core/NES/NesPpu.cpp::Exec` for per-PPU-cycle Lua callbacks
  (Session-29, documented in `docs/ppu-trace-tooling.md` Approach C).
* **`cpu-c1-attempt-17-access-reorder` cargo feature** — φ1/φ2 split scaffold on
  `nes-cpu` + `nes-core` + `nes-ppu` (Session-29). Default OFF.

All of the above are pure additions that don't affect default behaviour. Any
v1.x re-attempt can build on this foundation without re-paying the discovery
cost.

---

## Final v1.0.0 state

* Workspace strict pass: **545** + 5 ignored (3 cpu_interrupts_v2 + mmc3_test_2/4
  sub-test #3 + mmc3_test_2/6 NEC-by-design)
* Commercial-ROM oracle: **60/60** (605 total with `--features commercial-roms`)
* AccuracyCoin RAM-direct: **90.65%** (126/139)
* Sacred trio: preserved (SMB/Excitebike/Kid Icarus PAL boot-and-play legible)
* B4 invariant: preserved (first MMC3 IRQ at cycle 1,370,110 / scanline 0 / dot 257)
* All gauntlet gates: green (fmt / clippy / doc / no_std cross-compile)

**v1.0.0 final tag recommendation**: proceed with Option B documented deferral
of the 4 C1 IRQ-timing residuals to v1.x. The 90% gate is cleared; the residuals
are well-bounded; the path to closure is documented; the infrastructure is in
place for the v1.x re-attempt.
