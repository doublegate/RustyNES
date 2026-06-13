# Session 24 — Phase 3: Controller Strobing oracle + M2-low-defer fix

**Date:** 2026-05-23
**Branch:** `main` (HEAD `f7c1b75` at session start)
**Scope:** Phase 3 of the v1.0.0-final brief
(`linked-puzzling-sutherland.md`): run the Mesen2 oracle on the custom
`controller-strobing.nes` sub-test ROM, cross-diff against RustyNES,
confirm or refute the Session-22 M2-low-latch hypothesis, and (if
confirmed) implement the surgical fix.

**Predecessors:**
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md`
  (Phase 1 source audit identifying Controller Strobing as MEDIUM-tractability).
- `docs/audit/session-23-custom-accuracycoin-sub-test-roms-2026-05-22.md`
  (Phase 2 custom-ROM build infrastructure).
- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md`
  (Session-22 hypothesis: M2-low boundary latch).

## Phase 3.1 — Expected pass byte

From `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` (line 102) +
`AccuracyCoin.asm` line 8574-8635: `Controller Strobing` lives at result
address `$045F`. The 4 sub-tests inside it write to ErrorCode (zp $10)
which `RunTest` initialises to `1` (line 17720). Each passing sub-test
INCs ErrorCode. Result byte encoding:

- All 4 sub-tests pass → return `LDA #$01`, result = `$01`.
- Sub-test N fails (where N is 1-based) → return `(ErrorCode<<2) | 2`.
  Mapping: Test 1 fail → `$06`, Test 2 fail → `$0A`, Test 3 fail → `$0E`,
  Test 4 fail → `$12`.

Full battery on RustyNES v1.0.0-rc2: `$12 = Fail at Test 4` (Tests 1-3
pass, Test 4 fails). The Test 4 surface is the M2-phase one (a 1-cycle
DEC `$4016` strobe pulse that must NOT latch when contained inside a
single CPU cycle).

## Phase 3.2 — Custom ROM bug surfaced + fixed (Session-23 retrofit)

The custom ROM as committed in Session-23 reproduced result `$06` (Test
1 fail) on BOTH RustyNES and Mesen2 — a degenerate diagnostic where the
custom ROM hits Test 1's controller-state expectation before reaching
the M2-phase-sensitive Test 4. Root cause: Test 1 expects 8 reads of
`$4016` to return `$FF` (bit 0 = 1 in each), implying the shift
register has been fully drained. The full-battery flow's NMI handler
calls `ReadController1` every frame, which leaves the shift fully
drained before each test. The custom ROM wrapper from Session-23 went
straight from `LoadSuiteMenuNoRendering` to `RunTest` with no drain.

**Fix**: extended the build script's wrapper template
(`scripts/accuracycoin-build/build_sub_test_rom.py`) to add an inline
controller drain (8 reads of `$4016` + 8 reads of `$4017` after a
strobe-high-then-low pulse) between `WaitForVBlank` and `JSR RunTest`.
Rebuilt `controller-strobing.nes` validated:

| Emulator | $045F result | Interpretation |
|---|---|---|
| Mesen2  | `$01`  | **PASS** (all 4 sub-tests pass) |
| RustyNES | `$12` | **Fail Test 4** (M2-phase surface — matches full battery) |

The other 3 custom ROMs (`implied-dummy-reads.nes`,
`frame-counter-irq.nes`, `apu-reg-activation.nes`) were also rebuilt
with the drain. Their result bytes are unchanged after rebuild (verified
via `validate_sub_test_rom`): Implied Dummy Reads still `$0E` (Fail
Test 3), Frame Counter IRQ still `$1E` (Fail Test 7), APU Reg Activation
still `$12` (Fail Test 4). The drain doesn't affect those tests because
they don't predicate on the initial shift state.

## Phase 3.3 — Mesen2 + RustyNES trace tooling

### New artifacts

| File | Description |
|---|---|
| `scripts/mesen2_controller_trace.lua` | Per-`$4016`/`$4017` read+write Lua oracle. Captures cycle, frame, scanline, dot, parity-derived M2 phase, access type, value. |
| `crates/nes-test-harness/src/bin/trace_controller_strobing.rs` | RustyNES counterpart binary. Uses the `irq-timing-trace` feature to record per-CPU-cycle bus access, then emits a CSV filtered to `$4016`/`$4017` rows + result-addr write + ErrorCode writes. Build/run requires `--features irq-timing-trace`; no-feature build emits a stub `main` that prints a usage hint. |
| `crates/nes-test-harness/golden/irq_trace/controller-strobing.csv` | RustyNES trace (committed). |
| `crates/nes-test-harness/golden/irq_trace/mesen2/controller-strobing.csv` | Mesen2 trace (committed). |

## Phase 3.4 — Oracle cross-diff: Test 4 divergence

The two traces, focusing on Test 4's `DEC $4016` 1-cycle strobe pulse:

| Phase | Mesen2 (cycles 563985-563987) | RustyNES (cycles 861794-861796) |
|---|---|---|
| Read $4016 | 563985, `$41` | 861794 L, `$41` |
| Write $4016 (dummy DEC, value $41) | 563986 L | 861795 H |
| Write $4016 (final DEC, value $40) | 563987 H | 861796 L |
| Subsequent 8 reads of $4016 | all `$41` (LSB=1; NOT strobed — buttons NOT latched, expected behavior) | all `$40` (LSB=0; controller WAS strobed, latched empty button state, FAIL) |

The two emulators see the SAME `$41/$40` write sequence but produce
DIFFERENT latch outcomes. The structural difference is in the strobe
write commit semantics.

## Phase 3.5 — Mesen2 source cross-reference (commit-deferred writes)

Mesen2's `Core/NES/NesControlManager.cpp` lines 252-273 implement
**deferred `$4016`/`$4017` writes**:

```cpp
void NesControlManager::WriteRam(uint16_t addr, uint8_t value)
{
    //The OUT pins are only updated at the start of PUT cycles
    _writeAddr = addr;
    _writeValue = value;
    _writePending = (_console->GetMasterClock() & 0x01) ? 1 : 2;
}

void NesControlManager::ProcessWrites()
{
    if(_writePending && --_writePending == 0) {
        // ...
        for(shared_ptr<BaseControlDevice>& device : _controlDevices) {
            if(device->IsConnected()) {
                device->WriteRam(_writeAddr, _writeValue);
            }
        }
    }
}
```

`ProcessWrites()` runs once per CPU cycle inside `NesConsole::ProcessCpuClock`
(`Core/NES/NesConsole.cpp:72`).

The semantic: a CPU `$4016` write does NOT directly update the
controller's strobe latch. It stores the write in a one-slot buffer
(`_writeAddr`, `_writeValue`, `_writePending`). The buffer commits to
the device's actual `WriteRam` on the next M2-low (PUT, even-master-clock)
boundary: odd-cycle writes commit 1 cycle later, even-cycle writes
commit 2 cycles later. In both cases the commit cycle is EVEN
(the "PUT cycle"). When multiple writes happen before the commit, the
LATEST write wins — earlier values are silently overwritten in the
one-slot buffer.

For `DEC $4016` (read-modify-write):
- Read $4016 (no write — no `_writePending` update)
- Dummy write $41 → schedules commit
- Real write $40 → overwrites buffer; schedules commit

**Test 3 (synced via `STA $4014`)**: dummy write at cycle 534197 (H,
odd) → commit at 534198. Real write at cycle 534198 (L, even) →
commit at 534200. The dummy commit fires FIRST (cycle 534198) →
strobe=1 (rising edge). Real commit fires at 534200 → strobe=0
(falling edge). The latch detects 1→0 between two committed values →
LATCHES (buttons empty → all subsequent reads = 0).

**Test 4 (synced via `STA $4014` + `LDA <$00` 3-cycle delay)**: dummy
write at cycle 563986 (L, even) → schedules commit at cycle 563988
(`_writePending = 2`). Real write at cycle 563987 (H, odd) →
**overwrites** the buffer to $40, schedules commit at cycle 563988
(`_writePending = 1`). At cycle 563988 (even): one commit fires,
WriteRam($4016, $40). Previous committed value was 0, new is 0 — no
edge. **NO LATCH**. Subsequent reads see shift register unchanged from
the prior drain ($FF state) → 8 reads return `$41` (LSB=1).

**Hypothesis confirmed empirically.** The M2-phase axis isn't the
strobe LATCH logic itself; it's the COMMIT LATENCY (the deferred-write
mechanism) which collapses single-cycle strobe pulses when they happen
to start on M2-low and end on M2-high of consecutive cycles.

## Phase 3.6 — RustyNES current model

`crates/nes-core/src/bus.rs:1514-1518` `$4016` write dispatch:
```rust
0x4016 => {
    // The strobe line is shared between both controllers.
    self.controllers[0].write_strobe(value);
    self.controllers[1].write_strobe(value);
}
```

`crates/nes-core/src/controller.rs:88-97` `write_strobe`:
```rust
pub const fn write_strobe(&mut self, value: u8) {
    let new_strobe = value & 1 != 0;
    if new_strobe {
        self.shift = self.buttons.bits();
    }
    self.strobe = new_strobe;
}
```

RustyNES commits the strobe SYNCHRONOUSLY. Every `$4016` write
immediately updates `self.strobe`. There is no M2-phase commit
deferral. The 0→1→0 pulse in Test 4 fires the rising-edge latch
(`if new_strobe { self.shift = ... }`) on the dummy write, then the
real write resets strobe to 0 — the shift register has been latched
with $00 (buttons empty), so 8 subsequent reads return $40 (LSB=0).

## Phase 3.7 — Production fix design

**Surface**: add a deferred-write buffer to `LockstepBus` for `$4016`
writes. Commit at the next M2-low (even) cycle. Multiple writes within
the commit window collapse to the latest value.

The fix is structural enough that it doesn't fit cleanly behind a
feature flag (the deferred write needs a per-cycle `ProcessWrites` tick
woven into the bus's lockstep loop). Implementation plan:

1. Add `controller_write_pending: u8` + `controller_write_value: u8`
   fields to `LockstepBus`.
2. In `cpu_write` for `$4016`, set `controller_write_value = value` and
   set `controller_write_pending = if (cycle & 1) == 1 { 1 } else { 2 }`.
   (Cycle parity convention: Mesen2's "even cycle" matches our M2-low
   convention, which is also our `(cycle & 1) == 0`.)
3. In `tick_one_cpu_cycle`, after `cycle = cycle.wrapping_add(1)` and
   before the M2-high snapshot, decrement `controller_write_pending`;
   when it reaches 0, call `self.controllers[*].write_strobe(value)`.
   This commits AT THE START of the next CPU cycle, before any
   sub-dot logic runs — matching Mesen2's "start of PUT cycle" timing.
4. The current immediate-strobe path is removed; the entire commit
   happens through the deferred buffer.

The fix does NOT need to touch `controller.rs` itself — the
`write_strobe` semantics (rising-edge latch) stay correct ONCE the
commit timing is right.

### Why no feature flag

The fix changes `LockstepBus` cycle-loop structure, not just a code
path. A feature flag would require dual-implementing the cycle loop,
which is invasive and adds permanent maintenance debt. Better to land
the fix as the new canonical behavior, gated only by the regression
test suite (workspace `--features test-roms` strict count must stay
541+5, and the commercial-roms oracle must stay byte-identical).

### Cascade-risk analysis

The deferred-write change affects `$4016` and `$4017` write commit
timing. Possible regression surfaces:

| Surface | Strict pass count | Risk |
|---|---|---|
| `dmc_dma_during_read4` (5 strict) | 5/5 | LOW — `$4016` writes are pre-DMC-DMA boundary work in those tests |
| `apu_test` (8 strict) | 8/8 | LOW-MEDIUM — `$4017` writes (frame counter) tested but at different timing; need to verify |
| `apu_mixer` (4 strict) | 4/4 | NONE — no $4016/$4017 interaction |
| `cpu_dummy_writes_oam` (1 strict) | 1/1 | MEDIUM — exercises `$2003` not `$4016`, but adjacent |
| `mmc3_test_2/4` sub-test #2 (B4 invariant) | strict pass | NONE — MMC3 surface |
| Commercial-ROM oracle (60 ROMs) | 54 strict + 6 ignored | LOW — game ROMs strobe controllers but typically with multi-cycle `STA` writes that defer-commit identically to immediate-commit |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | visually legible | LOW — same as commercial-ROM oracle |

Verification gauntlet (mandatory): all of the above must remain green
after the fix.

## Phase 3 outcome (provisional, pre-implementation)

**Oracle confirmed M2-low-defer hypothesis.** Implementation can
proceed without a feature flag (structural change, all-or-nothing).
Phase 3.7's fix design is precise and falsifiable: it should flip
`Controller Strobing` from Fail Test 4 to Pass while preserving all
other workspace tests.

## File changes (this audit doc + oracle infrastructure)

- `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`: this doc.
- `scripts/accuracycoin-build/build_sub_test_rom.py`: added inline
  controller drain to wrapper template (8 reads of $4016 + 8 reads of
  $4017 between WaitForVBlank and JSR RunTest).
- `tests/roms/AccuracyCoin/sub-tests/controller-strobing.nes`: rebuilt
  with drain. SHA changes; behaviour for Tests 1-3 now matches full
  battery (Test 4 is now the failing surface, matching reality).
- `tests/roms/AccuracyCoin/sub-tests/implied-dummy-reads.nes`: rebuilt
  with drain (functionally unchanged for the target test).
- `tests/roms/AccuracyCoin/sub-tests/frame-counter-irq.nes`: rebuilt.
- `tests/roms/AccuracyCoin/sub-tests/apu-reg-activation.nes`: rebuilt.
- `scripts/mesen2_controller_trace.lua`: new focused Lua trace.
- `crates/nes-test-harness/src/bin/trace_controller_strobing.rs`: new
  RustyNES trace binary (feature-gated).
- `crates/nes-test-harness/golden/irq_trace/controller-strobing.csv`:
  new RustyNES golden trace.
- `crates/nes-test-harness/golden/irq_trace/mesen2/controller-strobing.csv`:
  new Mesen2 golden trace.

## References

- `Core/NES/NesControlManager.cpp` (Mesen2 source, lines 252-273:
  deferred-write commit logic) +
  `Core/NES/NesConsole.cpp` (line 72: ProcessWrites tick).
- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 8574-8635
  (TEST_ControllerStrobing — 4 sub-tests, M2-phase Test 4 commentary
  on lines 8597-8615).
- nesdev wiki `Standard controller`
  (`https://www.nesdev.org/wiki/Standard_controller`) — strobe latch
  semantics.
- `crates/nes-core/src/bus.rs:1506-1530` (current $4016 write dispatch).
- `crates/nes-core/src/controller.rs:88-114` (current write_strobe).
- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md` (the
  earlier hypothesis the empirical evidence now refines from
  "M2-low-latch" to "M2-low-commit-defer").
