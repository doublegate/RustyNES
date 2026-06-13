# Session-29 — Option (a) PPU Re-baseline EMPIRICAL FALSIFICATION

**Date**: 2026-05-23
**Status**: Critical finding — Option (a) "comprehensive PPU re-baseline" empirically
attempted and **does NOT close the C1 IRQ-timing axis**. User authorization for the
re-baseline was obtained, the shift was applied, the gauntlet was run with
`--include-ignored` against the strict C1 probes, and the target tests **STILL FAIL**.

This document supersedes the Session-29 final-conclusion's recommendation of Option
(a) as the v1.0.0 path. The empirical evidence reframes the available options.

---

## Hypothesis tested

Per the Session-29 final-conclusion + Session-17 boot-trace finding, Mesen2's
post-reset PPU position is `(scanline=0, dot=25)` while RustyNES's is
`(scanline=0, dot=23)` — a +2 PPU dot deficit rooted in Mesen2's pre-loop
master-clock advance (`_masterClock += cpuDivider + cpuOffset;` line 158 of
`Core/NES/NesCpu.cpp`).

The hypothesis: shift RustyNES's PPU init from `(prerender_line, dot=340)` to
`(scanline=0, dot=1)` — putting our PPU 2 dots ahead at power-up so the
post-power-on position matches Mesen2's. This was option (a) of the v1.0.0 C1
closure choice.

---

## Implementation

`crates/nes-ppu/src/ppu.rs:317-318` — single-line change:

```rust
// Before:
dot: 340,
scanline: region.prerender_line(),

// After:
dot: 1,
scanline: 0,
```

User-authorized the comprehensive re-baseline (60-ROM commercial oracle insta
snapshots, 81-PNG visual corpus, audio_db FNV-1a hashes, Cascade A unit test
expectations, vbl_race_window oracle) on 2026-05-23.

---

## Result — TARGET TESTS STILL FAIL

Gauntlet with shift applied (`cargo test --workspace --features test-roms --release
--no-fail-fast`):

* **24 snapshot regressions** (as expected): all in audio_db, m22, visual_regression,
  Cascade A — these are the "expected cosmetic shifts" that re-baseline would absorb.
* **`cpu_interrupts_v2/{2,3,5}_strict` probes**: ran with `--include-ignored` →
  **ALL THREE STILL FAILED**.
* **`mmc3_test_2/4` sub-test #3**: still ignored, not run separately, but expected
  to STILL FAIL on the orthogonal CPU `T_last - 1` axis.

The empirical result is unambiguous:

```
test cpu_interrupts_v2_2_nmi_and_brk_strict ... FAILED
test cpu_interrupts_v2_3_nmi_and_irq_strict ... FAILED
test cpu_interrupts_v2_5_branch_delays_irq_strict ... FAILED
test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 5 filtered out
```

---

## Why Option (a) cannot work

The +2 dot shift moves **everything uniformly**:

* VBL is set when PPU reaches `(scanline=241, dot=1)` — its position is determined
  by the PPU's own state, not by the CPU's read position.
* The CPU's BIT $2002 inside `sync_vbl` lands at some PPU dot determined by the
  cumulative PPU position at the CPU's read instant.

With the +2 dot shift, BOTH positions shift by +2 dots simultaneously:

* VBL set: now happens at `(scanline=241, dot=1)` of the SHIFTED PPU frame timeline.
* BIT $2002 read: lands at `(prior_dot + 2)` of the SHIFTED PPU frame timeline.

The **relative position** between the VBL set event and the read event is unchanged.
The race window relationship is preserved. Therefore the read still lands on the
pre-VBL-set side, and the target tests still fail.

To actually close the C1 axis, we need to change the **phase relationship** between
the CPU and PPU per cycle — i.e., the number of PPU dots that advance during the
"pre-access" part of a CPU cycle (in master-clock terms, between `StartCpuCycle`
and `MemoryRead`). This is what Option (b) (master-clock-precise scheduling) does.

A global PPU init shift cannot change the per-cycle phase relationship; it only
changes the absolute starting position.

---

## Reframed conclusion

The C1 IRQ-timing axis cannot be closed by:

* (a) Comprehensive PPU re-baseline — **empirically falsified** by this session's
  attempt. The cascade WOULD have been absorbed by the re-baseline, but the target
  tests STILL FAIL with the shift applied.
* (b) Master-clock-precise scheduling refactor — **the only remaining technical path**.
  Multi-week architectural change. Replaces our integer-PPU-dot per-CPU-cycle model
  with Mesen2's fractional 12-master-clocks-per-CPU-cycle model. This is what would
  actually close C1 because it changes the per-cycle CPU/PPU phase relationship.
* (c) Documented v2.0 deferral — same as Session-29 final-conclusion path (c), but
  now framed as v2.0 (when Option b ships) rather than open-ended v1.x.

**The user-chosen "(a) now, (b) later for v2.0" path is now (c) defer to v2.0**
because (a) has been empirically falsified. Option (b) is the path forward when
the v2.0 master-clock refactor lands.

---

## Status of v1.0.0 release

Unchanged from Session-29 final-conclusion:

* Workspace strict: **545** + 5 ignored
* Commercial-ROM oracle: **60/60**
* AccuracyCoin: **90.65%** (126/139 — gate cleared)
* Sacred trio + B4 invariant preserved
* All gauntlet gates green

The v1.0.0 final tag protocol's strict requirement of "all 4 C1 IRQ-timing
residuals flipped" can only be met by the v2.0 master-clock refactor. v1.0.0
should proceed with the 4 C1 residuals documented as **v2.0-deferred** (not
"v1.x" — v2.0 specifically, when the master-clock refactor lands).

---

## Recommendation for v1.0.0 final tag

Proceed with the v1.0.0 final tag protocol per Session-19 "Option B" precedent
+ Session-29 final-conclusion path (c):

1. AccuracyCoin ≥ 90% — **MET** (90.65%)
2. All validation gates green — **MET**
3. ~~All 4 C1 residuals flipped~~ → **DEFERRED TO v2.0** with documented
   architectural rationale (master-clock refactor required; integer-PPU-dot
   model cannot close the per-cycle phase relationship).

The v1.0.0 tag commit message should explicitly reference this audit doc and
the Session-29 final-conclusion doc as the canonical record of why the 4 C1
residuals are deferred and what closure requires.

---

## Permanent infrastructure landed during the C1 investigation series

Retained as scaffolding for v2.0's master-clock refactor:

* `cpu-c1-attempt-17-access-reorder` cargo feature (φ1/φ2 split scaffold)
* `crates/nes-core::irq_trace` (irq-timing-trace feature) + 6 golden traces
* `crates/nes-cpu::M2Phase` enum + per-phase IRQ snapshots
* `crates/nes-ppu::vbl_race_window_2002_read_sweep` permanent oracle
* `scripts/cpu_boot_trace_pc_align.py` + `cpu_boot_trace_diff` + Mesen2
  per-CPU-instruction trace generator
* Mesen2 source patch: `EventType::PpuCycle` for per-cycle Lua callbacks

These tools will let v2.0 validate the master-clock refactor against Mesen2 at
multiple granularities (per-CPU-instruction, per-CPU-cycle, per-PPU-cycle).
