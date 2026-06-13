# Session-14 — C1 Attempt 12: IRQ-Trace Regeneration on Session-13 Mesen2-Aligned Foundation

**Date**: 2026-05-22
**Status**: Infrastructure-landed-only (no chip-stack code change). 6 golden IRQ traces regenerated on the post-Session-13 boot-aligned foundation; all 6 shifted vs the pre-Session-13 baselines, invalidating a key ADR-0002 claim and providing a clean empirical surface for attempt 13.
**Predecessor**: `session-13-cpu-boot-fix-2026-05-21.md` (CPU SP=$00 + 8-cycle reset + PPU dot=340 power-up alignment with Mesen2).
**Branch / commit**: `main`, this session's commits build on `4027e0e` (Part 1 STATUS doc refresh).

---

## Summary

Session-13 closed the +344-dot PPU power-up offset + the SP-divergent stack-write
confound that had contaminated every prior C1 attempt's trace measurements. This
session is the first opportunity to regenerate the per-CPU-cycle IRQ trace golden
baselines under the clean foundation.

The 6 IRQ traces at `crates/nes-test-harness/golden/irq_trace/*.csv` were
regenerated unconditionally via `cargo test --features test-roms,irq-timing-trace
--test irq_trace_fixture`. **All 6 shifted vs the pre-Session-13 baselines**:

- `cpu_interrupts_v2_1_cli_latency.csv` (control, only strict-pass): single-row
  shift, `cpu_cycle=268141 dot=5` → `cpu_cycle=268028 dot=6` (−113 CPU cycles,
  +1 PPU dot within scanline). The first APU IRQ assertion in this baseline.
- `cpu_interrupts_v2_2_nmi_and_brk.csv`: ~745 rows, 1492 diff lines (substantially
  rewritten).
- `cpu_interrupts_v2_3_nmi_and_irq.csv`: ~767 rows, 1536 diff lines (substantially
  rewritten).
- `cpu_interrupts_v2_4_irq_and_dma.csv` (currently strict-pass): 75 rows,
  148 diff lines (substantially rewritten despite the test passing).
- `cpu_interrupts_v2_5_branch_delays_irq.csv`: 206 rows, 413 diff lines
  (substantially rewritten).
- `mmc3_test_2_4_scanline_timing.csv`: 2918 rows, 5838 diff lines (substantially
  rewritten). The first MMC3 IRQ assertion shifted from cycle 1,370,111 (post-B4
  baseline) to cycle 1,369,997 (post-Session-13). The architectural property the
  B4 fix introduced — first MMC3 IRQ lands on scanline 0 dot 260, not scanline 261
  pre-render — is **preserved** through Session-13; only the absolute cycle count
  shifts.

This session lands the regenerated traces, an ADR-0002 update documenting the
empirical finding, and a CHANGELOG entry. No chip-stack code was modified.

The C1 attempt 12 is **infrastructure-landed-only**. The new golden traces are
the authoritative baseline against which attempt 13 (or any later coordinated
CPU/Bus/PPU IRQ-sample-timing rework) must be diffed. The prior ADR claim that
"the 5 `cpu_interrupts_v2_*.csv` baselines are byte-identical pre-B4 vs post-B4"
no longer holds across the Session-13 boundary, but the B4 architectural
property is empirically preserved.

---

## Why no code change

Eleven prior C1-axis attempts have been rolled back (Attempts 1-4 + B4
threshold prototype + post-B4 mid-cycle-snapshot + 7th-attempt M2-low IRQ +
8th-attempt landed M2-low IRQ as Phase 1 with no test flips + 9th-attempt
drop-opcode-fetch-IRQ-on-taken-branches + two earlier T_last-1 + branch-
page-cross experiments documented in the task tracker). The 6 ADR-0002 stop
conditions and the per-cycle CPU instrumentation analysis at ADR-0002
lines 690-755 jointly establish that the remaining 4 target tests
(`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3) sit on a
coordinated multi-axis architectural surface — not a single-line `idle_tick`
or `branch` change.

Three findings make a code change THIS session a high-risk gamble:

1. **The prior trace baselines were contaminated.** Every C1 attempt's
   trace-fixture diagnosis from 2026-05-13 through 2026-05-17 was anchored
   against IRQ-cycle positions that included the +344-dot PPU drift + the
   SP-divergent stack writes. The 7th-attempt rollback's "M2-low IRQ axis IS
   load-bearing" conclusion ($cpu\_interrupts\_v2/5$ `test_jmp` CK values
   flipping) was made against a contaminated baseline. The 8th attempt landed
   that change unconditionally with no test flip; the regenerated traces
   confirm the cycles those tests now land at are different by 60-113 CPU
   cycles from where the diagnosis was anchored.

2. **The current test failure shapes look correct in shape but wrong in cycle
   count.** `cpu_interrupts_v2/5-branch_delays_irq`'s `test_jmp` and
   `test_branch_not_taken` sub-tests pass their CK pattern checks (output
   shape matches silicon); the failure is in `test_branch_taken_pagecross`
   where CK 04 → 06 → 06 → 05 → ... shows a 2-cycle-too-many tail. The CK
   pattern is consistent with a 1-cycle CPU instruction-stream mismatch
   relative to the test's hardware-calibrated expectations. The same surface
   `cpu_interrupts_v2/3-nmi_and_irq` first row matches silicon (`23 00`) but
   subsequent rows diverge — a per-instruction sample-point delta. Both shapes
   match the canonical `T_last - 1` story documented at ADR-0002 lines 720-755,
   AND the Session-13 boot-alignment trace-shift evidence. Without
   cycle-instrumented Mesen2 cross-reference, choosing between "fix this with
   a `T_last - 1` rework" and "fix this with one of N other axes" is guessing.

3. **No Mesen2 cycle-instrumented IRQ trace was captured this session.** The
   project has a working `cpu-boot-trace` fixture (cycle-aligned CPU trace,
   used in Session-12/13) but no equivalent IRQ-line-state trace from Mesen2.
   Without a Mesen2 IRQ-cycle oracle, any new C1 code attempt would be
   guessing at which axis to move first. That is exactly what produced the 11
   prior rollbacks.

The conservative path per ADR-0002's "Stop conditions" subsection: land the
trace-regen + ADR update + CHANGELOG entry, defer code attempt 13 to a session
that pairs trace-regen with Mesen2 IRQ instrumentation.

---

## Validation gauntlet

All four quality gates green:

```
env -u RUSTC_WRAPPER cargo fmt --all --check                                                       PASS
env -u RUSTC_WRAPPER cargo clippy --workspace --all-targets --features test-roms -- -D warnings   PASS
env -u RUSTC_WRAPPER RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps                      PASS
env -u RUSTC_WRAPPER cargo build --workspace                                                       (implied by clippy --all-targets)
```

Test counts:

| Feature combo | Strict pass | `#[ignore]` | Pre-Session-14 | Delta |
|---|---|---|---|---|
| `--features test-roms` | **540** | 5 | 540 + 5 | 0 |

Per-target ignored test status (unchanged from Session-13):

| Test | Failure shape | Status |
|------|---------------|--------|
| `cpu_interrupts_v2/2-nmi_and_brk_strict` | Hijack rows shifted, anomalous `$02` at row 5 (CRC `85498C19`) | `#[ignore]` (unchanged) |
| `cpu_interrupts_v2/3-nmi_and_irq_strict` | First row matches silicon, later rows diverge (CRC `11030CA2`) | `#[ignore]` (unchanged) |
| `cpu_interrupts_v2/5-branch_delays_irq_strict` | `test_jmp` PASS, `test_branch_not_taken` PASS, `test_branch_taken_pagecross` FAIL (CK 04→06 anomaly) (CRC `AB1A8F0A`) | `#[ignore]` (unchanged) |
| `mmc3_test_2/4-scanline_timing_strict` | Sub-test #2 PASS (B4 preserved), #3 FAIL ("should occur SOONER", 1-CPU-cycle bracket) | `#[ignore]` (unchanged) |
| `mmc3_test_2/6-mmc3_alt_strict` | By-design FAIL (NEC rev B, project defaults to Sharp) | `#[ignore]` (unchanged) |

AccuracyCoin RAM-direct pass rate: **82.73%** (108 pass + 7 pass_with_code of
139 assigned tests). Unchanged. Same 24-test failing list as Session-13.

AccuracyCoin framebuffer pass rate: **88.98%** (118 assigned cells). Unchanged.

---

## Empirical findings

### Finding 1 — All 6 IRQ traces shifted post Session-13

The pre-Session-13 baselines (committed up to `eb37ff8`, the
`feat(cpu,ppu): coordinated CPU/PPU power-up alignment matches Mesen2` commit)
were the post-B4 + post-D3 baselines + Session-13 boot alignment. Trace
regeneration shows that the boot alignment moved IRQ-cycle landing positions
in all 6 tracked ROMs.

The shift signature (per the cli_latency control):
- `cpu_cycle` delta: **−113** CPU cycles (= +8 reset cycles − cycle accounting
  realignment from PPU dot=340 start, approximately).
- `(scanline, dot)` delta: **+1 PPU dot within scanline** (cli_latency: dot 5
  → 6).

For the larger traces, individual events shift by 60-114 CPU cycles with PPU
position rotations consistent with the boot-alignment delta. The shifts are
not uniform across the trace because each test ROM's branch / loop structure
diverges from a hardware-calibrated cycle count once the boot alignment lands
on a different `LDA $2002 / BPL` exit cycle (the same load-bearing instruction
Session-13 Phase C identified empirically against Mesen2).

### Finding 2 — The B4 architectural property is preserved through Session-13

The post-B4 success fix (commit `48b5983` predecessor, Phase B4 reload-pending
discriminator) established that the first MMC3 IRQ assertion lands on
scanline 0 (one NTSC scanline LATER than the pre-B4 baseline's scanline 261
dot 259 pre-render fetch). This was the architectural fix sub-test #2 brackets.

Post-Session-13 trace: first MMC3 IRQ lands at **cycle 1,369,997 / frame 47 /
scanline 0 / dot 260**. The scanline = 0 invariant is preserved; only the
cycle count shifts by -114 from the pre-Session-13 cycle 1,370,111. Sub-test #2
"Scanline 0 IRQ should occur LATER" continues to PASS — the test brackets
scanline assignment, not absolute cycle.

The B4 fix is therefore demonstrably durable across the Session-13 boot
alignment. No regression introduced.

### Finding 3 — The post-B4 ADR claim "cpu_interrupts_v2 baselines byte-identical" is now FALSE

ADR-0002 §"Trace regeneration after B4 + Phase D3 fixes (2026-05-15)" stated:

> `cpu_interrupts_v2_{1,2,3,4,5}_*.csv`: ALL FIVE byte-identical to the pre-B4
> baselines. `git diff --stat` reports zero changes. This is the empirical
> proof that the B4 fix is MMC3-localized: it changes nothing on the pure
> APU/CPU IRQ-flow axis that the `cpu_interrupts_v2` ROMs exercise.

This was true at the time the B4 fix landed (the Phase D3 + B4 + post-tag
window). It is **no longer true** post-Session-13 — the boot alignment
materially changed the IRQ-line-state cycle distribution in all 5
`cpu_interrupts_v2_*.csv` traces and in the `mmc3_test_2_4_scanline_timing.csv`
trace. The fundamental causation chain is intact (B4 is still MMC3-localized;
Session-13 is what shifted all 6) but ADR-0002's prior verbatim claim now
overstates the trace-byte-identity invariant.

ADR-0002 is updated in this session's commit to add a new subsection
"Trace regeneration after Session-13 Mesen2 alignment (2026-05-22)" that
documents the shift signature and identifies the new authoritative golden
baselines.

### Finding 4 — Per-test failure shapes are consistent with the pre-Session-13 diagnosis

The 4 remaining target tests retain the same failure shapes documented at
ADR-0002 lines 690-755:

- `2-nmi_and_brk` — NMI hijack window shifted vs silicon (anomalous `02` row
  in the output).
- `3-nmi_and_irq` — first row matches, subsequent rows diverge.
- `5-branch_delays_irq` — `test_jmp` + `test_branch_not_taken` PASS, fails at
  `test_branch_taken_pagecross` (a 1-cycle CPU instruction-stream slew).
- `mmc3_test_2/4` sub-test #3 — "should occur SOONER" 1-CPU-cycle bracket
  (post-B4 residual, cross-cycle physics).

All 4 are consistent with the canonical 6502 `T_last - 1` IRQ-sample-point
hypothesis at ADR-0002 line 723. None are flipped or regressed by the
Session-13 boot alignment.

---

## What remains untried (recommended next attempts, in priority order)

These options are derived from the empirical evidence in this session's trace
regen plus ADR-0002's "Refined direction" subsection:

1. **Mesen2 IRQ-line-state cross-reference** — instrument Mesen2 to emit
   per-CPU-cycle IRQ trace records analogous to the RustyNES
   `crates/nes-core/src/irq_trace.rs` fixture. Run the 6 target ROMs through
   both emulators. Cross-diff IRQ-assertion cycle positions. This produces the
   silicon-cycle reference no prior C1 attempt has had. Estimated effort: 2-4
   hours (Mesen2 has Lua scripting + existing `scripts/mesen2_cpu_boot_trace.lua`
   as a template).

2. **`T_last - 1` CPU IRQ-sample-point rework as a feature-flagged
   experiment** — the ADR-0002 §"Per-cycle CPU instrumentation analysis"
   subsection identifies this as the load-bearing axis but warns that an
   unguarded change would regress 80+ CPU tests. Wrap behind
   `cargo features = ["cpu-irq-sample-at-tlast-minus-1"]`, regenerate traces
   with and without the flag, cross-diff against a Mesen2 oracle (Option 1
   first). DO NOT land the change without the Mesen2 oracle in hand.

3. **NMI-hijack-window cycle-level audit** — `cpu_interrupts_v2/2-nmi_and_brk`
   measures the BRK-hijack-by-NMI window with 1-cycle resolution. The current
   anomalous `$02` row at row 5 suggests our `service_interrupt` BRK path
   takes 1 cycle too many before it samples NMI. Read `crates/nes-cpu/src/cpu.rs::
   service_interrupt` against the test's expected hijack rows 4-8; the gap is
   a single conditional sample-point shift inside `service_interrupt`. Same
   feature-flag discipline as Option 2.

4. **`mmc3_test_2/4` sub-test #3 cross-cycle pipelining** — ADR-0002's
   post-B4 mid-cycle-snapshot rollback (the 11th attempt) proved this CAN'T be
   solved at the mapper alone. The fix surface is a CPU-side IRQ-visibility
   delay that LANDS on the same cycle as the A12 rise (sub-test #3's brackets
   require ≤ 1 CPU cycle from emission to CPU visibility). Likely couples with
   Option 2.

---

## Files modified by this session

- `crates/nes-test-harness/golden/irq_trace/*.csv` (6 files regenerated)
- `docs/adr/0002-irq-timing-coordination.md` (new subsection documenting the
  Session-13 trace shift)
- `CHANGELOG.md` `[Unreleased]` (new "C1 attempt 12 — infrastructure landed"
  entry)
- `docs/audit/session-14-c1-attempt12-trace-regen-2026-05-22.md` (this file)

## Files NOT modified

- All chip crates (`crates/nes-{cpu,ppu,apu,mappers,core}/src/*`). The
  production code path is unchanged.
- All other documentation (the v0.9.0 row in `docs/STATUS.md` was refreshed
  in this session's Part 1 commit `4027e0e`, separately from this Part 2
  audit).

## Invariants validated (Session-14 close)

| Invariant | Pre-Session-14 | Post-Session-14 | Status |
|-----------|----------------|------------------|--------|
| Workspace tests `--features test-roms` | 540 strict + 5 ignored | 540 strict + 5 ignored | OK |
| AccuracyCoin RAM pass rate | 82.73% | 82.73% (unchanged) | OK |
| AccuracyCoin framebuffer pass rate | 88.98% | 88.98% (unchanged) | OK |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible | legible (no source change) | OK |
| `cargo fmt --all --check` | clean | clean | OK |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | clean | clean | OK |
| `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps` | clean | clean | OK |
| 6 IRQ trace golden baselines | pre-Session-13 cycle counts | post-Session-13 cycle counts | UPDATED |

Net change: pure-additive trace regeneration + documentation. Zero production
code modified. The C1 attempt 12 is the **infrastructure-landed-only**
outcome per the Part 2 spec; the regenerated traces unblock attempt 13 by
providing an uncontaminated empirical surface.
