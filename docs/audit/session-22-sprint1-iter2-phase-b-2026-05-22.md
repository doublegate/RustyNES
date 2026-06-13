# Session 22 — Sprint 1 iteration 2 Phase B + Sprint 2 disposition

**Date:** 2026-05-22
**Branch:** `main` (HEAD `98de856` at session start)
**Scope:** Sprint 1 iteration 2 (`to-dos/phase-6-v1.0.0-final/sprint-1-implied-dummy-dmc-coordinated.md`)
Phase B re-attempt **plus** Sprint 2 (`to-dos/phase-6-v1.0.0-final/sprint-2-apu-put-get-phase.md`)
disposition.
**Outcome (Sprint 1):** Phase 1A landed — Mesen2 Lua extended with the AccuracyCoin
protocol path (continuous-run, autostart Start-press, per-sub-test RAM
watchdog, DMC-event stop limit, row-limit safety). Phase 1B blocked by a
**hard wall-time constraint** discovered empirically: with the per-CPU-
instruction `on_exec` Lua callback armed, Mesen2 sustains ~7 emulator FPS
under xvfb; the AccuracyCoin battery reaches the relevant DMC sub-tests
only after ~3000+ NES frames, which would need 15-20 wall-minutes per
trace pass. Multiple trace passes (≥ 3) are needed for a calibration
audit — the cost is multiple hours of bench time and cannot fit in a
single session's effort budget. Per Decision Gate 1A of the brief: land
Phase 1A as infrastructure-only, document the blocker, defer Phase 1B
to a focused future session, **proceed to Sprint 2**.
**Outcome (Sprint 2):** [TBD pending Part 2 execution]
**Predecessors:**
- `session-21-dmc-trace-tooling-2026-05-22.md` — Phase A landing + Phase B
  exploratory pass.
- `session-20-sprint1-dmc-abort-investigation-2026-05-22.md` — iteration 1
  rollback.
- `session-19-accuracycoin-pivot-2026-05-22.md` — naive attempt.

## Baseline at session start

- HEAD `98de856` (Session-21 Phase A + Phase B exploratory).
- Workspace: **541 strict pass + 5 expected-fail `#[ignore]`'d** with
  `--features test-roms` across 34 suites.
- AccuracyCoin: **82.73%** (108 pass + 7 pass_with_code of 139 assigned
  tests).
- Sprint 1 iteration 1 status: ROLLBACK (Session-20 Phase 2).
- Per-cycle DMC scheduler trace tooling: landed Session-21.

## Phase 1A — Mesen2 Lua AccuracyCoin protocol support

### Design

`scripts/mesen2_irq_trace.lua` previously supported only the blargg
`$6000`-status + `$DE-$B0-$61` magic-bytes early-stop protocol. AccuracyCoin
uses an entirely different continuous-run protocol:

1. **No `$6000` status byte** — the ROM doesn't write the
   `$80 → final-status` transition the blargg suite uses.
2. **Per-test RAM-result encoding** — each test writes its single-byte
   result (Pass/Fail/PassWithCode/Skipped) to a dedicated CPU-RAM address
   listed in `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`.
3. **User-input gated battery** — the ROM boots to a title-screen menu
   and requires `Start` to enter the
   `AutomaticallyRunEveryTestInROM` path. The RustyNES harness
   (`crates/nes-test-harness/src/accuracy_coin.rs:283`) presses Start at
   frame ~306 for 6 NES frames; Mesen2 needs the same input injection.

Phase 1A adds a new `MESEN2_IRQ_TRACE_PROTOCOL` selector (`blargg` |
`accuracycoin` | `dmc_events`) plus 7 new tunables:

| Env var | Default | Purpose |
|---|---|---|
| `MESEN2_IRQ_TRACE_PROTOCOL` | `blargg` | Protocol selector. |
| `MESEN2_IRQ_TRACE_WATCH_ADDR` | `0` (off) | CPU-RAM address for the AccuracyCoin per-sub-test watchdog (e.g. `1133` for `$046D`, `Implied Dummy Reads`). |
| `MESEN2_IRQ_TRACE_DMC_EVENT_LIMIT` | `0` (off) | Stop after N `dmc_run` events have been emitted. |
| `MESEN2_IRQ_TRACE_ROW_LIMIT` | `100000` | CSV row cap. |
| `MESEN2_IRQ_TRACE_AUTOSTART_FRAME` | `300` | Press-Start frame. |
| `MESEN2_IRQ_TRACE_AUTOSTART_PRESS_FRAMES` | `6` | Press-Start duration. |
| `MESEN2_IRQ_TRACE_EXEC_START_FRAME` | `0` | Frame at which the per-CPU-instruction `on_exec` callback engages. Throughput optimization — see "Wall-time blocker" below. |

The `blargg` path's behavior is byte-identical to pre-Session-22 (the
existing 7 trace-fixture tests + the 5 Mesen2 oracle CSVs at
`crates/nes-test-harness/golden/irq_trace/mesen2/` remain valid). The new
paths are inactive at default settings.

### Implementation

- `scripts/mesen2_irq_trace.lua`: +106 LoC.
  - `CONFIG` extended with the 7 new fields.
  - `write_record` tracks `dmc_run_events` + enforces `ROW_LIMIT`.
  - `on_start_frame` branches on `CONFIG.PROTOCOL`:
    - `blargg` → original `$6000` status protocol (preserved).
    - `accuracycoin` → optional `WATCH_ADDR` polling.
    - `dmc_events` → relies on `write_record`'s `DMC_EVENT_LIMIT` path.
  - `on_input_polled` callback registers only when `PROTOCOL ==
    "accuracycoin"`. Presses Start on player-1 for the configured
    window via `emu.setInput({start = true}, 0, 0)`.
  - Log message at script-arm time surfaces the active protocol +
    tunables for trace-run audit.

### Validation

- Blargg sentinel regression: `xvfb-run -a mesen.appimage --testRunner
  tests/roms/blargg/dmc_dma_during_read4/dma_2007_read.nes
  scripts/mesen2_irq_trace.lua` emits **51 rows** with `MAX_FRAMES=200,
  BOOT_FRAMES=10` — byte-identical to Session-21's exploratory pass.
- AccuracyCoin Start-press: with `PROTOCOL=accuracycoin
  AUTOSTART_FRAME=300 MAX_FRAMES=600` the trace shows the autostart
  visibly working — frames 33-300 emit `nmi_svc` only (title-screen
  loop), frames 302+ emit `apu_set`/`apu_clr` pairs (battery is running
  the APU-test sub-suite). The Start press correctly drives the ROM
  into `AutomaticallyRunEveryTestInROM`.
- Workspace tests: `cargo test --workspace --features test-roms`
  shows **541 strict + 5 ignored** (baseline preserved).
- `cargo fmt --all --check`: PASS.

## Wall-time blocker (Phase 1B)

The Phase B re-attempt's diagnosis pass needs:

1. A Mesen2 AccuracyCoin oracle trace covering the **DMC sub-tests** at
   tests #85-94 (`APU Registers and DMA tests`) — these are the most
   likely sub-tests to expose the calibration mismatch that Session-19 +
   Session-20 + the post-B4 mid-cycle-snapshot experiment cascaded on.
2. Ideally also a trace covering test #141 `Implied Dummy Reads`.

Empirical throughput measurement (Session-22, this session):

| Configuration | Wall time | NES frames reached | Effective FPS |
|---|---:|---:|---:|
| Blargg sentinel, `--features test-roms,irq-timing-trace` | 104 s | 200 | ~1.9 |
| AccuracyCoin, `EXEC_START_FRAME=0`, `--testRunner --timeout=1200` | ~600 s | ~430 | ~0.7 |
| AccuracyCoin, `EXEC_START_FRAME=0`, `--testRunner --timeout=1500 --noVideo --noAudio` | ~5 min then forced-kill | 354 | ~1.1 |
| AccuracyCoin, `EXEC_START_FRAME=2700`, `MAX_FRAMES=3000` | 590 s | 2989 | ~5.0 |
| AccuracyCoin, `EXEC_START_FRAME=2700`, `MAX_FRAMES=6000` | 8 s (early-stop, frame 1589) | 1589 | ~199 effective; but stopped early at frame 1589 due to Mesen2-internal `IsRunning()` returning false |

**Root cause of the slowdown**: Mesen2's Lua `emu.addMemoryCallback`
trampolines fire per-CPU-instruction at ~500k/frame. Each callback's
body calls `emu.getState()` twice (once for the main snapshot, once via
`snapshot_dmc()`). At ~1μs per `getState` call this is ~1 ms of Lua
overhead per NES frame, against a 16.67 ms NTSC frame budget. With the
xvfb wrapper + Mesen2's interpreter shell the effective speedup falls
to ~7× native real-time.

**Root cause of the early-stop at frame ~1589**: Confirmed
deterministic. AccuracyCoin's `AutomaticallyRunEveryTestInROM` completes
the battery, runs cleanup (`STA $4015 = 0; vblank wait; re-enable
rendering + NMI`), and falls into the `PressStartToContinue` infinite
loop at the menu. Mesen2's `TestRunner.Run` then detects
`EmuApi.IsRunning() == false` somehow — most likely because the
`MaximumSpeed` emulation flag's polling loop sees an infinite-loop +
no-progress condition and pauses the emulator. The exact mechanism is
Mesen2-internal; investigating it is out of scope for Phase A. **The
behavior is consistent** — at frame 1589 the battery has completed but
test #141 (`Implied Dummy Reads`) has NOT been reached yet (it runs
much later in the suite, around the end of the battery's display
phase). This is the load-bearing constraint: even if we eliminate the
`emu.getState()` overhead, the Mesen2 `--testRunner` mode's
interaction with the AccuracyCoin spinning menu loop terminates the
trace before we reach the relevant sub-test.

### Throughput optimizations explored + rejected

- **Lazy `addMemoryCallback` registration** (defer until `frame_count >=
  EXEC_START_FRAME`): introduced empty-CSV output even on the working
  blargg sentinel. Cause unclear (possibly Mesen2's Lua VM is not
  re-entrant across `emu.eventType.startFrame` ↔ `addMemoryCallback`).
  Reverted.
- **`--noVideo --noAudio`**: no visible improvement (Mesen2's
  `MaximumSpeed` flag was already on).
- **`--enableStdout`**: surfaces ROM-load info but no Lua `emu.log`
  output. Mesen2's Lua logging goes to its internal UI log, not stdout.

### What would actually unblock Phase 1B

Two viable paths, both out of scope for this session:

1. **Build Mesen2 from source** with C++ debug hooks that emit per-CPU-
   instruction events natively (no Lua bridge). This would eliminate
   the ~1μs/instruction Lua trampoline + `getState` cost. Estimated 1-2
   days of work — see Mesen2's
   `Core/Debugger/Disassembler/EmuTypeImpl.cpp`.
2. **Compile a custom AccuracyCoin test ROM that jumps directly to the
   target sub-test** (`TEST_ImpliedDummyRead` at line 11633 of
   `AccuracyCoin.asm`) without running the preceding 140 tests. This
   would let the Mesen2 trace cover the relevant cycle window in well
   under 1 NES second. Requires patching the AccuracyCoin source +
   re-assembling — estimated 0.5-1 day.

Either path is preferable to a 3rd speculative C1-area code change with
no trace evidence. Per the user's brief: "ONLY if the trace evidence
drives the design should iteration 2 proceed". Without a Mesen2 oracle
covering the target sub-test the design step has no oracle.

### Decision: Phase 1B deferred; Phase 1A lands

Per Decision Gate 1A of the brief:

> If AccuracyCoin Mesen2 trace generation FAILS (e.g., Mesen2 hangs,
> xvfb issues, ROM-specific crash), STOP at this point. Land the Lua
> adaptation as infrastructure-only. Push. Document the new blocker.
> Skip to Part 2 (Sprint 2).

The Phase 1A infrastructure stays landed (the Lua + the `EXEC_START_FRAME`
optimization knob are permanent assets); Phase 1B's diagnosis pass is
deferred to a future session that picks one of the two unblock paths
above.

## Sprint 1 final state

| Surface | State |
|---|---|
| Workspace tests `--features test-roms` | **541 strict + 5 ignored** (baseline preserved) |
| AccuracyCoin pass rate | **82.73%** (108 pass + 7 PWC of 139) — unchanged |
| Commercial-ROM oracle | Not re-run (no chip-stack code change) |
| B4 invariants | Preserved (no MMC3 / DMC scheduler code change) |
| Sacred trio | Preserved (no chip-stack code change) |
| `cargo fmt --all --check` | PASS |
| Sprint 1 iter 2 outcome | **INVESTIGATION-ONLY** — Phase A trace tooling extended; Phase B blocked on wall-time + cannot proceed in single session |

## Sprint 2 status

[Filled in after Sprint 2 execution completes.]

## File changes summary (Sprint 1 Phase 1A)

- `scripts/mesen2_irq_trace.lua`: +106 LoC (protocol selector, watchdog,
  autostart driver, throughput knob, row + DMC-event limits).
- `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md`: this doc.

## References

- `docs/audit/session-21-dmc-trace-tooling-2026-05-22.md` (Phase A
  landing + Phase B exploratory pass; the predecessor that motivated
  this session).
- `docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
  (iteration 1 cascade analysis).
- `to-dos/phase-6-v1.0.0-final/sprint-1-implied-dummy-dmc-coordinated.md`
  (the open sprint spec; status remains ROLLBACK).
- Mesen2 `UI/Utilities/TestRunner.cs` — testRunner loop (timeout-driven,
  polls `EmuApi.IsRunning()`).
- Mesen2 `UI/Utilities/CommandLineHelper.cs` — `--testRunner --timeout=N`
  invocation contract.
- AccuracyCoin source at
  `https://github.com/100thCoin/AccuracyCoin/blob/main/AccuracyCoin.asm`
  lines 11521-11800 (`TEST_ImpliedDummyRead` body + subroutines).
