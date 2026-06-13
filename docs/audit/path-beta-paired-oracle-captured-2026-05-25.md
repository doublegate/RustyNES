# Path β — paired oracle captured + structural DMC scheduler mismatch identified

**Date:** 2026-05-25 (third-iteration follow-on to the trace-tooling
foundation work and the previous paired-trace audit).
**Outcome:** Mesen2 now successfully runs the AccuracyCoin battery
under `--testRunner` (root cause: no controller plugged in). Three
paired oracle traces captured. Within-RustyNES diff reveals a
1-cycle structural DMC scheduler timing divergence under `cpu-
implied-dummy-reads`. Comparison against Mesen2's `NesCpu.cpp`
shows the underlying scheduler model is fundamentally different
(get/put cycle alternation vs. noop loop) — **the fix is a model
refactor, not a 4-delay recalibration.**

---

## Mesen2 START-press fix: controller was unplugged

Investigation by sub-agent + my own probe via
`~/.config/Mesen2/settings.json` revealed that **all NES controller
ports had `Type: "None"`** in the Mesen2 GUI settings. Mesen2's
`emu.setInput({start = true}, 0, 0)` requires a controller device
at the target port to write into — with `Type: "None"` there is no
device buffer and the call is a no-op.

**Fix:** programmatically set `Nes.Port1.Type = "NesController"` in
`~/.config/Mesen2/settings.json` before running. Verified by
re-running the trace — Mesen2 now captures 92 DMC fetches, 1,219
register writes, and reaches PPU frame 1669 (just past the
`Implicit DMA Abort` test at frame 1620).

The 5-step protocol memory should be updated to add a "controller
plug-in" prerequisite if any future Lua-driven Mesen2 testing
runs into the same issue. (Settings persist after the first manual
change, so this is mostly a fresh-environment / first-time problem.)

---

## Captured oracle artifacts

Three CSVs committed under `docs/audit/dmc-paired-traces-2026-05-25/`
(real data, not synthetic):

| File | Source | Rows | Frame range |
|---|---|---|---|
| `rusty_acc_base.csv` | RustyNES, default features | 22 693 | 1100 → 1620 (skip start through `Implicit DMA Abort` result-write) |
| `rusty_acc_idr.csv` | RustyNES, `--features cpu-implied-dummy-reads` | 22 629 | same window |
| `mesen2_acc_v2.csv` | Mesen2, accuracycoin protocol | 3 025 | 0 → 1669 (PPU-frame count, Mesen2-resets-on-reset) |

The Mesen2 schema is the asymmetric one — events fire only on
`$4010-$4015` callbacks + delta-detected `dmc_get` from
`bytes_remaining` decrements (Mesen2 doesn't expose per-cycle bus
state via Lua). Cross-emulator alignment uses the first `$4015 W
val=$10` (the canonical DMC-enable signal).

---

## Within-RustyNES cycle-precise diff (the actionable finding)

`scripts/dmc_dma_within_rusty_diff.py` (NEW this session) pairs the
two RustyNES traces by `cpu_cycle` (no offset needed — both traces
come from the same scheduler clock).

```
common cycles:       21 450
only-in-baseline:     1 243   <- mostly extra DMC sample-fetch
                                 cycles at $FFC0 at the END of each
                                 DMC service window
only-in-variant:      1 179   <- new cycle-2 dummy reads and the
                                 abort cycles that follow them
divergent same-cyc:     435   <- same cycle, different bus access
                                 (almost always Idle → DmaRead at
                                 $FF4E/$FF25 — cycle-2 PC dummy
                                 reads of implied opcodes)
```

### The first divergent cycle (frame 1576, scanline 241)

```
cyc 46931957  H  W $4015=$10            (DMC enable; same on both)
cyc 46931960  L  R $FF4D=$EA            (implied opcode init; same on both)
cyc 46931961  H  base: I $0000=$00      (idle halt cycle)
              H  var:  r $FF4E=$EA      (DMC fetch already at $FF4E)
cyc 46931962  L  base: r $FF4E=$EA      (DMC fetch starts)
              L  var:  r $FF4E=$EA      (DMC fetch continues)
cyc 46931963  H  base: r $FF4E=$EA      (DMC fetch continues)
              H  var:  r $FFC0=$00      (sample fetch — 1 cycle EARLY)
cyc 46931964  L  base: r $FFC0=$00      (sample fetch)
              L  var:  [no event]
```

**Under `cpu-implied-dummy-reads` ON, the DMC DMA service window
runs ONE CYCLE EARLIER than baseline.** What was a 4-cycle pattern
(idle-halt + 2 noops + 1 sample-fetch) becomes a 3-cycle pattern
(2 noops + 1 sample-fetch). The first noop "absorbs" the bus-busy
cycle-2 dummy read that the CPU now performs.

### Result-encoding divergence

* baseline: `$0478 = 0x09` (PASS)
* IDR-ON: `$0478 = 0x0A` (FAIL — error code 2)

The 1-bit difference between PASS and FAIL maps to a single
sub-test inside `Implicit DMA Abort` that detects the cycle-skew.
The cascade propagates from the first divergent cycle through 435
subsequent same-cycle events.

---

## Mesen2 DMC scheduler is structurally different

`Mesen2/Core/NES/NesCpu.cpp:395-450` reveals Mesen2 implements DMC
DMA via a **get/put cycle alternation** keyed on
`(_state.CycleCount & 0x01)`, with three independent state flags:

* `_dmcDmaRunning`: DMC has requested DMA and hasn't delivered yet
* `_needHalt`: still need the initial halt cycle (~ load mode)
* `_needDummyRead`: still need the dummy-read alignment cycle

The main loop walks until `_dmcDmaRunning` clears:

```cpp
while(_dmcDmaRunning || _spriteDmaTransfer) {
    bool getCycle = (_state.CycleCount & 0x01) == 0;
    if(getCycle) {
        if(_dmcDmaRunning && !_needHalt && !_needDummyRead) {
            // DELIVER THE BYTE
            readValue = ProcessDmaRead(GetDmcReadAddress(), ...);
            EndCpuCycle(true);
            _dmcDmaRunning = false;
        } else if(_spriteDmaTransfer) {
            // OAM DMA read cycle
        } else {
            // Dummy read (halt or alignment cycle)
            _memoryManager->Read(readAddress, MemoryOperationType::DmaRead);
        }
    } else {
        // PUT cycle: sprite DMA write OR alignment dummy read
    }
}
```

Compare to RustyNES's `crates/nes-core/src/bus.rs:1333-1366`:

```rust
let noop_cycles = if self.apu.dmc_dma_short() { 2 } else { 3 };
for _ in 0..noop_cycles {
    self.replay_dma_noop_read(halted_addr);
    self.tick_one_cpu_cycle();
}
let byte = self.dmc_dma_read(addr, halted_addr);
// ...
```

**Mesen2's model is phase-aware** (get/put alternation by cycle
parity). **RustyNES's model is phase-agnostic** (fixed 2 or 3 noop
loop). The 4 "compensating delays" (`dmc_dma_short`,
`dmc_dma_cooldown`, `dmc_abort_delay_for`, `dmc_dma_pending`) in
RustyNES were tuned to make the phase-agnostic model approximate
the phase-aware one — under the BASELINE bus-cycle pattern.

Under `cpu-implied-dummy-reads` ON, the CPU's bus-cycle pattern
changes (cycle-2 of implied opcodes is now bus-active instead of
idle). The phase-aware Mesen2 model handles this naturally — its
get/put alternation absorbs the new busy cycles. The phase-
agnostic RustyNES model does NOT — its 4 delays are still tuned
for the OLD pattern.

**No combination of delay-recalibration can match Mesen2** because
the scheduler model is wrong. Recalibrating the 4 delays would
fix one specific test instance but cascade into others (Session-20
and Sprint 2.3 Step 3 iter 1+2 empirically confirmed this).

The structural fix is a refactor of `service_dmc_dma` to use the
get/put-cycle alternation model. Roughly:

```rust
// New shape (sketch — not implemented):
while self.in_dmc_dma {
    let get_cycle = (self.cpu_cycle_count & 1) == 0;
    if get_cycle {
        if !self.dmc_need_halt && !self.dmc_need_dummy_read {
            self.dmc_dma_read(addr, halted_addr);  // deliver byte
            self.in_dmc_dma = false;
        } else {
            self.replay_dma_noop_read(halted_addr);  // dummy
            if self.dmc_need_halt {
                self.dmc_need_halt = false;
            } else {
                self.dmc_need_dummy_read = false;
            }
        }
    } else {
        // PUT cycle: alignment dummy read
        self.replay_dma_noop_read(halted_addr);
    }
    self.tick_one_cpu_cycle();
}
```

Plus:
* `dmc_dma_pending` → split into the three flags above
* `dmc_dma_short`, `dmc_dma_cooldown`, `dmc_abort_delay_for`,
  `dmc_dma_delay` all RETIRED (they're emergent from the get/put
  alternation, not parameters)
* The OAM-DMA / DMC-DMA conflict path (`service_dmc_dma_during_oam`)
  needs the same get/put refactor

**Estimated scope**: 2-3 days of careful surgery + full validation
gauntlet (sacred trio + commercial oracle re-baseline + AccuracyCoin
trajectory + blargg suites). This is the legitimate Sprint 2.3
Step 3 closure path, NOT a "compensating-delay tweak" that prior
sessions attempted.

---

## Tooling that landed this session

### New artifacts

* `scripts/dmc_dma_within_rusty_diff.py` — within-emulator cycle-
  precise diff for any two `trace_dmc_dma` CSVs. Pairs by
  `cpu_cycle`, reports common/divergent/only-in-each-side counts,
  prints divergent same-cycle events with bus_access + bus_addr +
  bus_data + DMC scheduler state. **This is the actionable tool**
  — it identifies WHICH cycles the cascade triggers on.
* `crates/nes-test-harness/src/bin/scan_dma_abort.rs` — full-
  battery DMA-test frame scanner (presses START + tracks which
  frame each result address gets set). Found that `Implicit DMA
  Abort` lands at absolute frame 1620.
* `crates/nes-test-harness/src/bin/dump_battery_ram.rs` — post-
  battery RAM-dump helper for the DMA-test address range.

### Extended artifacts

* `crates/nes-test-harness/src/bin/trace_dmc_dma.rs`:
  - `--battery`: presses START at frame 306 for 6 frames
  - `--start-frame N`: skip frames before enabling trace
  - `--buffer-cycles N`: configurable buffer size
  - Tight result-stop (immediate break on first result-write)
* `scripts/mesen2_dmc_dma_trace.lua`:
  - `MESEN2_DMC_TRACE_AUTOSTART_FRAME` / `_AUTOSTART_PRESS_FRAMES`
    env vars: AccuracyCoin START-press driver
  - `MESEN2_DMC_TRACE_START_FRAME`: skip events before this PPU
    frame
  - Tightened `dmc_get` detection: only emits when
    `bytes_rem == prev - 1 && prev > 0`
* `scripts/dmc_dma_trace_cross_diff.py`:
  - `--align-value`: align on first `$4015 W` with specific value
  - Rusty DMC-fetch filter switched to rising-edge of
    `dmc_pending_post`
* `.github/workflows/release.yml`: removed `body:` and
  `generate_release_notes:` (root cause of v1.0.0 + v1.1.0 release-
  notes overwrite)
* Mesen2 settings: `Nes.Port1.Type` set to `"NesController"`

---

## Workspace state at end of this session

* Tests: 537 strict pass + 5 ignored across 34 suites with
  `--features test-roms`. **PRESERVED.**
* AccuracyCoin: **90.65% (126/139) PRESERVED.**
* 60-ROM commercial-ROM oracle: **60/60 PRESERVED.**
* Sacred trio: **PRESERVED.**
* B4 invariant: **PRESERVED.**
* v1.1.0 release notes: 14 398 chars on GitHub.
* CI release-workflow body-clobber bug: **structural fix
  committed** (working-tree change).

**No production scheduler code modified this session** — the
get/put refactor is documented as a multi-day v1.x or v2.0
milestone, not attempted under single-session scope.

---

## Concrete next-session work items (in priority order)

1. **Decide between v1.x and v2.0 for the DMC scheduler refactor**:
   - v1.x: standalone refactor; ships as `v1.2.0` with `Implicit
     DMA Abort` (via IDR-ON) flipped PASS, AccuracyCoin trajectory
     +1 to +3 (depends on how many other tests this surface
     touches). Risk: re-baseline of the 60-ROM commercial oracle
     because audio FNV-1a will shift (DMC fetch timing is
     audio-observable).
   - v2.0: bundle with the master-clock refactor (Sprint A). The
     get/put-cycle model is exactly what master-clock-precise
     scheduling produces naturally — once you have fractional
     PPU/CPU cycle alignment from the 12-master-clocks-per-cycle
     refactor, the get/put alternation is just `(cycle_count &
     1)`. Doing both at once avoids two oracle re-baselines.

2. **If v1.x path chosen**: implement the get/put refactor in a
   feature branch under a new `dmc-get-put-scheduler` cargo
   feature flag (default OFF). Parallel-implementation
   equivalence harness (compare baseline scheduler vs get/put
   scheduler across 1,000 randomized DMC trigger sequences) before
   flipping default. ADR 0007 for the model change.

3. **CI release-workflow fix needs to be committed** (working-tree
   change at `.github/workflows/release.yml`). Without committing,
   the v1.2.0 release will hit the same clobber bug.

---

## Cross-references

* Foundation: `docs/audit/path-beta-dmc-trace-tooling-2026-05-25.md`
* Previous paired-trace attempt:
  `docs/audit/path-beta-paired-trace-2026-05-25.md`
* Sprint 2.3 Step 3 iter 1+2 (single-axis insufficient):
  `docs/audit/sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`
* Sprint 2.3 recon:
  `docs/audit/sprint-2.3-implied-dummy-dmc-recon-2026-05-25.md`
* Session-20 (origin of the 4-compensating-delay diagnosis):
  `docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
* RustyNES DMC scheduler implementation:
  `crates/nes-core/src/bus.rs::service_dmc_dma` (line 1333)
* Mesen2 reference (get/put cycle alternation model):
  `Core/NES/NesCpu.cpp:395-450`, `Core/NES/APU/DeltaModulationChannel.cpp:59`
* Captured trace artifacts (real data, ~2.5 MB):
  `docs/audit/dmc-paired-traces-2026-05-25/`
* v2.0.0 release plan:
  `/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md`
