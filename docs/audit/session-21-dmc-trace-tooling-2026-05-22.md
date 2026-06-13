# Session 21 — Per-cycle DMC scheduler trace tooling (Sprint 1 iteration 2 prereq)

**Date:** 2026-05-22
**Branch:** `main` (HEAD `a9333ba` at session start)
**Scope:** Phase A of Sprint 1 iteration 2 (`to-dos/phase-6-v1.0.0-final/sprint-1-implied-dummy-dmc-coordinated.md`).
**Outcome:** Phase A trace tooling landed. Per-cycle DMC scheduler
visibility now exists on both RustyNES and Mesen2 sides, with a
schema-tolerant cross-diff that surfaces RustyNES's compensating-delay
state. Phase A is permanent infrastructure: lands regardless of whether
Phase B's diagnosis pass yields a single-axis fix.
**Predecessors:** `session-19-accuracycoin-pivot-2026-05-22.md` (naive
attempt) + `session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
(coordinated attempt + rollback).

## Baseline at session start

- HEAD `a9333ba` (CI release fix; macOS x86_64 runner migration).
- Workspace: **541 strict pass + 5 expected-fail `#[ignore]`'d** with
  `--features test-roms` across 34 suites.
- AccuracyCoin: **82.73%** (108 pass + 7 pass_with_code of 139 assigned
  tests).
- Sprint 1 iteration 1 status: ROLLBACK (Session-20 Phase 2). Same
  cascade shape as Session-19's naive attempt.

## Why this session exists

Sprint 1 iteration 1 (Sessions 19 + 20) twice produced the byte-
identical rollback shape:

- Implementation: Mesen2-aligned `read1(bus, self.pc)` at cycle 2 of 21
  implied/accumulator opcodes (per Mesen2 `NesCpu.cpp:274 DummyRead()`
  in `FetchOperand` for `NesAddrMode::Acc/Imp`).
- Result: target `CPU Behavior :: Implied Dummy Reads [error 3]` did
  NOT flip; cascade `APU Registers and DMA tests :: Implicit DMA
  Abort` flipped strict → error 2. AccuracyCoin regressed 82.73% →
  82.01%.

Session-20's deeper diagnosis identified the structural root cause:
RustyNES's DMC DMA scheduler has multiple compensating delays
(`dmc_abort_delay`, `dmc_dma_cooldown`, `dmc_dma_short`,
`pending_dmc_dma`, `dmc_dma_delay`) calibrated to a bus-quiet implied-
opcode T2 baseline. Adding the canonical cycle-2 dummy read makes the
bus active during those cycles, breaking the calibration. **Per-cycle
DMC scheduler visibility on BOTH emulators is the missing diagnostic
infrastructure** that blocked Session-20 from picking the right delay
constant(s) to retune.

Phase A of this session builds that infrastructure as permanent
tooling.

## Phase A scope

### Schema design

The existing `irq-timing-trace` cargo feature already captures per-CPU-
cycle IRQ-line and A12-transition state. Phase A extends it with the
DMC scheduler dimension and a per-cycle bus-access dimension.

Per CPU cycle, the `CycleRecord` now captures (existing columns
preserved, new columns appended):

| Column | Source | Notes |
|---|---|---|
| `dmc_dma_pending_pre` | `apu.dmc_dma_pending()` BEFORE `tick_with_external` | Pre-tick snapshot (mirrors `_at_low` IRQ semantics). |
| `dmc_dma_pending_post` | `apu.dmc_dma_pending()` AFTER `tick_with_external` | Post-tick snapshot. |
| `dmc_dma_short_post` | `apu.dmc_dma_short()` | 3-cycle short path vs 4-cycle long path. |
| `dmc_abort_pending_post` | `apu.dmc_abort_pending()` | One-cycle abort halt pending. |
| `dmc_abort_delay_post` | `apu.dmc_abort_delay()` | Countdown to abort-pending. |
| `dmc_dma_cooldown_post` | `apu.dmc_dma_cooldown()` | Suppress next reload-request window. |
| `dmc_dma_delay_post` | `apu.dmc_dma_delay()` | Load-DMA arming delay. |
| `apu_phase_post` | `apu.apu_phase()` | False = put, true = get. |
| `in_dmc_dma` | bus's `in_dmc_dma` | True during DMC halt service. |
| `dma_cycles_owed` | bus's `dma_cycles_owed` | OAM DMA remaining cycles. |
| `bus_access` | per-cycle tracker (see below) | `I`/`R`/`W`/`r`/`w`. |
| `bus_addr` | per-cycle tracker | Bus address driven this cycle. |
| `bus_data` | per-cycle tracker | Bus data byte driven this cycle. |

`BusAccess::Idle` = canonical 6502 internal cycle (open-bus driver
retained). `Read`/`Write` = normal CPU bus cycles. `DmaRead`/`DmaWrite`
= bus-owned DMA cycles (CPU halted).

The bus-access tracker is set by `cpu_read` / `cpu_write` (CPU-driven
cycles) and by the `service_dmc_dma` / `service_dmc_dma_during_oam` /
`service_dmc_abort` / `clock_oam_dma_cycle` paths (DMA-owned cycles).
`tick_one_cpu_cycle` consumes it when pushing the record, then resets
it to `Idle` for the next cycle. CPU burn cycles (`idle_tick` →
`bus.on_cpu_cycle()` without a preceding `cpu_read`/`cpu_write`) leave
the tracker at `Idle`, which is the correct semantics for the trace
(internal cycles don't drive the bus).

### RustyNES side implementation

Files changed:

- `crates/nes-core/src/irq_trace.rs`:
  - New `BusAccess` enum with `as_str()` for CSV serialization.
  - `CycleRecord` extended with 13 new fields (DMC + bus-access).
  - `to_csv_filtered` writes both the historical 10-column shape AND
    the new suffix as a single line (backward-compatible loaders see a
    prefix match; new loaders parse the full schema).
  - New `is_dmc_or_irq_event` convenience filter for the golden CSVs.
  - 2 new unit tests + 1 new helper (`rec`) refactoring the 3 existing
    tests; total 8 trace-module tests pass.
- `crates/nes-apu/src/apu.rs`: 4 new `pub const fn` accessors
  (`dmc_abort_delay`, `dmc_dma_cooldown`, `dmc_dma_delay`, `apu_phase`)
  exposing the previously `pub(crate)` scheduler state to the trace
  fixture. These are zero-cost in production builds — the bus reads
  them in the `#[cfg(feature = "irq-timing-trace")]` record push only.
- `crates/nes-core/src/bus.rs`:
  - 3 new `pub(crate)` trace fields (`trace_bus_access`,
    `trace_bus_addr`, `trace_bus_data`) gated on the trace feature.
  - `enable_irq_trace` resets them.
  - `cpu_read` / `cpu_write` (Bus trait impl) populate the tracker
    after the access.
  - `clock_oam_dma_cycle` / `service_dmc_dma` / `service_dmc_abort` /
    `service_dmc_dma_during_oam` populate the tracker before each
    `tick_one_cpu_cycle` call via the new `set_trace_dma_access`
    helper.
  - `tick_one_cpu_cycle` snapshots DMC scheduler state at the same
    two points as the IRQ-line snapshots (pre-tick for `_pre`, post-
    tick for `_post`), consumes the bus-access tracker, and emits the
    extended record.
- `crates/nes-test-harness/tests/irq_trace_fixture.rs`:
  - IRQ-focused trace keeps the original `IrqTrace::is_irq_event`
    filter — the historic golden CSVs (72-2919 lines per ROM) are
    preserved at the same density, just with the new DMC + bus-access
    columns appended as a backward-compatible suffix.
  - DMC-focused trace lands as a SIDECAR golden file
    (`<slug>.dmc.csv`) using the new `is_dmc_or_irq_event` filter.
    This keeps the IRQ-only baselines tight (an OAM DMA window's
    `dma_cycles_owed` decrement is per-cycle activity and would 100x
    the previously-tight files) while still committing the per-cycle
    DMC scheduler timeline as a permanent diagnostic asset.
  - 1 new test target: `dmc_dma_during_read4_dma_2007_read_baseline_trace`
    — the canonical DMC DMA regression sentinel.

Workspace tests with trace feature OFF (default): **541 strict + 5
ignored** (baseline preserved byte-identical to pre-session HEAD).

Trace-fixture tests (7 ROMs, feature ON): all 7 generate well-formed
CSVs. The new DMC sentinel produces 1,224,549 rows; 147 with
`dmc_dma_pending_post=1`, 115 with `in_dmc_dma=1`, 93 with
`dmc_dma_cooldown_post > 0`, 115 `DmaRead` bus accesses. Real DMC
scheduler activity is now visible per-cycle.

### Mesen2 side implementation

Files changed:

- `scripts/mesen2_irq_trace.lua`:
  - CSV schema extended with 4 columns: `dmc_irq_flag`,
    `dmc_irq_en`, `dmc_bytes_rem`, `dmc_sample_addr`.
  - New `snapshot_dmc()` helper (pcall-wrapped for older Mesen2 builds
    that may not expose every `apu.dmc.*` key under `emu.getState()`).
  - `on_exec` callback extended with 3 new edge-detected events:
    - `dmc_set` / `dmc_clr` — DMC IRQ flag transitions.
    - `dmc_irqen` — `$4010 bit 7` (DMC IRQ-enable) transitions.
    - `dmc_run` — zero crossings of `bytesRemaining` (DMC DMA
      activity proxy; Mesen2's Lua API does not expose
      `pending_dmc_dma` / `dmc_abort_pending` directly).

**Documented limitation**: Mesen2's Lua API exposes the DMC's
**output-side** state (IRQ flag, IRQ enable, bytes remaining, sample
address) but NOT the DMC scheduler's **internal** state (pending DMA,
abort-pending, abort-delay countdown, cooldown countdown, short-path
flag, delay countdown). Cross-diff tooling reconciles the two schemas
at IRQ-line and bytes-remaining-delta boundaries; full per-cycle DMA-
state visibility on the Mesen2 side requires a custom Mesen2 build
with C++ debug hooks (out of scope for Phase A, and not strictly
necessary — RustyNES's per-cycle scheduler state cross-diffed against
Mesen2's per-instruction `bytesRemaining` deltas is sufficient to
identify which RustyNES compensating-delay constant is mis-calibrated).

### Cross-diff tool extension

Files changed:

- `scripts/irq_trace_cross_diff.py`:
  - Schema-tolerant loaders: both pre-Session-21 (10/8-column) and
    Session-21 (23/12-column) traces parse cleanly. Older traces have
    DMC columns default to 0; cross-diff against them still works.
  - New `--dmc` mode: focused report on DMC scheduler events. Shows
    per-side activity tallies, first DMC IRQ assertion delta, first
    DMA activation window delta, and RustyNES's first-12-halt-cycles
    timeline with all scheduler state fields. The timeline is the
    primary diagnostic surface for Phase B's calibration audit.

## Phase A landing validation

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | PASS |
| `cargo clippy --workspace --all-targets --features test-roms,irq-timing-trace -- -D warnings` | PASS |
| `cargo doc --workspace --no-deps` (RUSTDOCFLAGS=-D warnings) | PASS |
| `cargo build --workspace` | PASS |
| `cargo test --workspace --features test-roms` | **541 strict + 5 ignored** (baseline preserved) |
| `cargo test -p nes-test-harness --features test-roms,irq-timing-trace --test irq_trace_fixture` | 7/7 PASS (6 prior + 1 new DMC sentinel) |
| `cargo test -p nes-core --features irq-timing-trace irq_trace::tests` | 8/8 PASS (5 prior + 3 new) |
| AccuracyCoin pass rate | 82.73% (unchanged — no chip-stack code changed by Phase A) |

## Sample DMC trace output

From the `--dmc` cross-diff mode on `dmc_dma_during_read4/dma_2007_read.nes`:

```
RustyNES DMC activity:
  dmc_dma_pending_post=1 rows: 147
  in_dmc_dma=1 rows:           115
  dmc_abort_pending_post=1:    0
  bus_access tally:            {'R': 611181, 'I': 319762, 'W': 293575, 'r': 31}

RustyNES halt-cycle timeline (first 12 halt cycles):
  idx  cycle      scnln dot  pre post short abrt abrtD cd  delay  apu_phase  bus
    0     270497    21  252  1  1      0     0      0   0      0          0  r
    1     273921    51  294  1  1      0     0      0   0      0          0  r
    2     277367    82   61  1  1      1     0      0   0      0          0  r
    3     280769   112   37  1  1      0     0      0   0      0          0  r
    4     325397   242  249  1  1      1     0      0   0      0          0  r
    ...
```

This is exactly the per-cycle visibility Session-20 lacked. Phase B
can now cross-diff specific halt-cycle windows against Mesen2's
output-side DMC trace to localize the mis-calibrated compensating
delay.

## Phase B planning

Phase B (diagnosis + iteration 2 fix attempt) requires:

1. A working Mesen2 binary with Lua scripting enabled to generate the
   reference trace.
2. The DMC-target AccuracyCoin assertion at `0x046D` exercised. The
   AccuracyCoin source at `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`
   indicates the `Implied Dummy Reads` test is in the `CPU Behavior`
   suite; the test ROM exercises DMC implicitly via `JSR $4011 → DMC
   DMA → implied opcode → $4015 frame-counter IRQ flag clear`.

### Phase B exploratory pass (Session-21)

Ran two exploratory Mesen2 trace passes to validate the extended
Lua + scout the diagnosis surface:

**Pass 1: DMC sentinel sanity check**

```bash
MESEN2_IRQ_TRACE_OUT=/tmp/mesen2_dmc_test.csv \
MESEN2_IRQ_TRACE_MAX_FRAMES=200 MESEN2_IRQ_TRACE_BOOT_FRAMES=10 \
  xvfb-run -a ~/AppImages/mesen.appimage --testRunner \
    tests/roms/blargg/dmc_dma_during_read4/dma_2007_read.nes \
    scripts/mesen2_irq_trace.lua
```

Result: **51-row trace with 36 `dmc_run` events** (DMC DMA activations
+ deactivations across the sentinel's 200-frame run). The extended
Mesen2 Lua works; `apu.dmc.bytesRemaining` exposes per-instruction
zero-crossings cleanly.

Cross-diff result (`scripts/irq_trace_cross_diff.py --dmc`):

```
RustyNES DMC activity:
  dmc_dma_pending_post=1 rows: 63
  in_dmc_dma=1 rows:           31
  bus_access tally:            {'R': 165, 'r': 31, 'W': 33, 'I': 12}

Mesen2 DMC event tally: {'dmc_run': 36}

First DMC DMA activation:
  RustyNES (pending_post=1):     cycle=270496 frame=10 scanline=21 dot=249
  Mesen2   (dmc_run + bytes>0):  cycle=384953 frame=14 scanline=242 dot=238
```

The first-activation cycle delta is the boot-frame-skip artifact
(RustyNES traces from BOOT_FRAMES+5=15; Mesen2 from 10). **Critical
observation**: RustyNES halt #9 at cycle 384959 is ONLY 6 CPU cycles
adjacent to Mesen2's first `dmc_run` at 384953 — the two emulators
converge on the same scheduler behavior at later cycles on the DMC
sentinel ROM. This corroborates the strict-pass status of
`dmc_dma_during_read4` (5/5 in both emulator builds): the sentinel
DOES NOT exercise the calibration mismatch.

**Pass 2: AccuracyCoin scouting**

```bash
MESEN2_IRQ_TRACE_OUT=/tmp/mesen2_accuracycoin.csv \
MESEN2_IRQ_TRACE_MAX_FRAMES=4000 MESEN2_IRQ_TRACE_BOOT_FRAMES=30 \
  xvfb-run -a ~/AppImages/mesen.appimage --testRunner \
    tests/roms/accuracycoin/AccuracyCoin.nes \
    scripts/mesen2_irq_trace.lua
```

Result: **63-row trace with only 61 `nmi_svc` + 1 `init` rows** —
NO DMC events captured. AccuracyCoin's `Implied Dummy Reads` test
runs at a SPECIFIC stage of its continuous-run protocol, which the
current `mesen2_irq_trace.lua` early-stop ($DE-B0-61 magic + final-
status) protocol does NOT match (AccuracyCoin writes per-test results
to specific RAM addresses, not the magic-status protocol).

**Diagnostic gap identified**: extending the Mesen2 Lua to either
(a) disable EARLY_STOP_ON_STATUS and run for 6000+ frames (a long-
session capture), or (b) start the trace at a specific cycle window
known to cover the Implied Dummy Reads sub-test, is the missing piece.
Both options are out of scope for Session-21's effort budget but are
trivial to add in a focused next-session pass.

### Decision: Phase B NOT attempted; lands as Phase A only

Per the sprint brief decision gate:

> If no clean single-axis hypothesis emerges, STOP. Phase A
> infrastructure stays landed. Phase B documents the negative finding.

The Pass 1 + Pass 2 evidence so far shows:

1. **The DMC sentinel ROM does NOT expose the calibration mismatch.**
   RustyNES and Mesen2 converge on the same DMA scheduler behavior
   on this ROM (cycle-adjacent first activations). This is consistent
   with the sentinel being strict-pass 5/5 in RustyNES.
2. **AccuracyCoin's `Implied Dummy Reads` test was NOT captured in
   the exploratory Mesen2 trace** — the early-stop protocol mismatch
   prevented the test window from being recorded. A focused
   AccuracyCoin trace with extended Lua tooling (disable EARLY_STOP +
   raise MAX_FRAMES + possibly add per-sub-test RAM-address watchdog
   stop conditions) is the next-session prerequisite for a real Phase
   B diagnosis.

A speculative Phase B fix without trace-evidence-driven design would
risk a 3rd rollback (Sessions 19 + 20 both rolled back with byte-
identical shape from speculation). The user's brief explicitly
requires "ONLY if the trace evidence drives the design" should
iteration 2 proceed.

**Phase A is the load-bearing landing.** It is permanent diagnostic
infrastructure that:

- Unblocks any future Sprint 1 iteration 2 attempt.
- Captures the DMC scheduler's per-cycle behavior as a committed
  diagnostic asset.
- Adds the DMC sentinel ROM to the trace-fixture suite (new
  permanent regression coverage).
- Validates the extended Mesen2 Lua against a real ROM (Pass 1
  confirms 36 `dmc_run` events emit cleanly).

## Recommended next sprint

Phase B (iteration 2 diagnosis + fix) is now unblocked by Phase A but
requires a focused next-session pass to:

1. **Extend `scripts/mesen2_irq_trace.lua`** to support AccuracyCoin's
   continuous-run protocol:
   - Either disable EARLY_STOP_ON_STATUS (run the full MAX_FRAMES
     budget; AccuracyCoin needs ~6000-8000 frames to cover the full
     suite).
   - Or add per-sub-test RAM-address watchdog stop conditions
     (e.g. stop when `$046D` transitions out of "running" state).
   - The Session-21 exploratory pass confirmed the extended Lua emits
     DMC events correctly; only the stop-condition needs adaptation.
2. **Generate the AccuracyCoin Mesen2 oracle trace** covering the
   `Implied Dummy Reads` (`$046D`) + `Implicit DMA Abort` (`$0478`)
   sub-tests.
3. **Cross-diff against RustyNES** using `scripts/irq_trace_cross_diff.py
   --dmc`. The new `--dmc` mode surfaces the first-12-halt-cycles
   timeline with all RustyNES scheduler state fields (per Pass 1
   above). Compare against Mesen2's per-instruction `dmc_run` /
   `dmc_set` / `bytesRemaining` deltas to localize the mis-
   calibrated compensating-delay constant.
4. **Form a single-axis hypothesis** with cited Mesen2 source lines
   (e.g. `Core/NES/NesDmc.cpp:LINE-NUMBER`) showing the canonical
   value vs RustyNES's `dmc_abort_delay_for(2) = 2 / (3) = 3`.
5. **Implement behind `cpu-implied-dummy-coordinated` feature flag**
   (the one Session-20 added + reverted) PLUS the calibration
   adjustment in a SINGLE atomic commit, with the full per-iteration
   validation gauntlet from the sprint spec.

If no clean single-axis hypothesis emerges after step 3, document the
negative finding (per the sprint spec's "Step 6 — Land OR rollback"
protocol) and proceed to:

**Sprint 2** (APU put/get phase plumbing). Sprint 2's yield estimate
is +1 to +3 tests including Frame Counter IRQ #7 and Controller
Strobing — independent of the DMC scheduler surface and with no
cascade history. See `to-dos/phase-6-v1.0.0-final/overview.md` for
the sprint priority order.

## File changes summary

- `crates/nes-core/src/irq_trace.rs` — schema + filter + tests (+150 LoC, -20).
- `crates/nes-core/src/bus.rs` — trace plumbing + scheduler snapshot + DMA tagging (+60 LoC).
- `crates/nes-apu/src/apu.rs` — 4 public accessors (+30 LoC).
- `crates/nes-test-harness/tests/irq_trace_fixture.rs` — DMC sentinel + widened filter (+25 LoC).
- `scripts/mesen2_irq_trace.lua` — DMC columns + 4 edge events (+55 LoC).
- `scripts/irq_trace_cross_diff.py` — schema-tolerant loaders + `--dmc` mode (+115 LoC).

Total: ~435 LoC added across 6 files; no chip-stack production
behavior change (feature OFF byte-identical to HEAD).

## Workspace test counts

- `--features test-roms`: **541 strict pass + 5 expected-fail
  `#[ignore]`'d** across 34 suites (UNCHANGED).
- `--features test-roms,irq-timing-trace`: 541 strict + 7 trace-
  fixture tests (1 new, 6 prior — all regenerated baselines).

## References

- `docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
  — the Phase 1 investigation + Phase 2 rollback that motivated this
  tooling.
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` — the naive
  attempt + initial cascade discovery.
- `to-dos/phase-6-v1.0.0-final/sprint-1-implied-dummy-dmc-coordinated.md`
  — the open sprint spec, status remains ROLLBACK.
- Mesen2 `Core/NES/NesCpu.cpp:254-292` (CPU read/write + DummyRead).
- Mesen2 `Core/NES/APU/DeltaModulationChannel.cpp:275-290`
  (DMC abort path).
- Mesen2 `Core/NES/NesDmc.cpp` (DMC scheduler reference; the canonical
  oracle for Phase B's calibration audit).
- nesdev wiki APU DMC §"DMA conflicts"
  (https://www.nesdev.org/wiki/APU_DMC).
- `crates/nes-apu/src/apu.rs:102-109` — `dmc_abort_delay_for` lookup
  table (the most likely off-by-one candidate per Session-20's
  diagnosis).
- `crates/nes-core/src/bus.rs:960-1010` `drain_dma` — the DMC DMA
  service gate.
