# Session 22 — Sprint 2: APU put/get phase plumbing investigation

**Date:** 2026-05-22
**Branch:** `main` (HEAD `6169efe` at sprint start)
**Scope:** Sprint 2 (`to-dos/phase-6-v1.0.0-final/sprint-2-apu-put-get-phase.md`).
**Outcome:** INVESTIGATION-ONLY. No production fix attempted. Mesen2
cross-reference + AccuracyCoin source analysis landed; a precise
single-axis hypothesis for the Controller Strobing surface is now
documented. Production code change deferred — the same wall-time blocker
that blocked Sprint 1 Phase 1B (Mesen2 cannot reach the AccuracyCoin
sub-tests within a reasonable session budget) blocks Sprint 2's oracle
trace too. Per the user's brief "ONLY if the trace evidence drives the
design", proceeding without empirical confirmation risks a Session-19 /
20-style cascade revert against the load-bearing `apu_test/*` and
`dmc_dma_during_read4` surfaces.
**Predecessor:** `session-22-sprint1-iter2-phase-b-2026-05-22.md`.

## Baseline at sprint start

- HEAD `6169efe` (Sprint 1 Phase 1A: Mesen2 Lua AccuracyCoin protocol).
- Workspace: **541 strict pass + 5 expected-fail `#[ignore]`'d** with
  `--features test-roms` across 34 suites.
- AccuracyCoin: **82.73%** (108 pass + 7 pass_with_code of 139 assigned
  tests).
- Target tests (4): `APU Tests :: Frame Counter IRQ [error 7]`, `APU
  Tests :: Delta Modulation Channel [error 21]`, `APU Tests :: APU
  Register Activation [error 4]`, `APU Tests :: Controller Strobing
  [error 4]`.

## Existing put/get plumbing in RustyNES

The `M2Phase::Low/High` enum infrastructure (Session-13 Phase B1) is
already wired through `LockstepBus::current_m2_phase()` and the CPU's
`Bus::poll_irq_at_phase` accessor. The APU's `apu_phase` field (bool;
toggled per `tick_with_external`) plus its 4 derived accessors
(`apu_phase()`, `dmc_abort_delay()`, `dmc_dma_cooldown()`,
`dmc_dma_delay()`, exposed Session-21) provide the get/put visibility.

Surfaces that ALREADY use `apu_phase`:

| File | Line | Use |
|---|---:|---|
| `crates/nes-apu/src/apu.rs` | 516 | `$4017` write passes `apu_phase` as `aligned` to `FrameCounter::write` — selects 3 vs 4 cycle reset delay. |
| `crates/nes-apu/src/apu.rs` | 546 | `$4015` write (DMC enable) passes `apu_phase` to compute initial `dmc_dma_delay` (3 vs 4). |
| `crates/nes-apu/src/apu.rs` | 572 | `schedule_explicit_dmc_abort_if_needed` uses `apu_phase` to compute `first_apu_clock`. |
| `crates/nes-apu/src/apu.rs` | 616 | `read_status` passes `apu_phase` to `FrameCounter::read_status` — selects immediate vs deferred IRQ-flag clear. |
| `crates/nes-apu/src/frame_counter.rs` | 109-152 | `write` schedules `reset_in = 3 if apu_aligned else 4`; `read_status` either clears immediately (aligned) or defers (non-aligned). |

## Mesen2 cross-reference

### Frame Counter (Mesen2 `Core/NES/APU/ApuFrameCounter.h`)

```cpp
void WriteRam(uint16_t addr, uint8_t value) override
{
    _console->GetApu()->Run();   // catch up frame counter to current cycle
    _newValue = value;
    if(_console->GetCpu()->GetCycleCount() & 0x01) {
        _writeDelayCounter = 4;  // odd cycle → 4-cycle delay
    } else {
        _writeDelayCounter = 3;  // even cycle → 3-cycle delay
    }
    _inhibitIRQ = (value & 0x40) == 0x40;
    if(_inhibitIRQ) {
        _console->GetCpu()->ClearIrqSource(IRQSource::FrameCounter);
        _irqFlag = false;
        _irqFlagClearClock = 0;
    }
}
```

Mesen2's delay calculation uses **CPU cycle count parity** as the "put
vs get" axis: `cycle & 0x01`. RustyNES's `apu_phase` is set to `true`
on the *get* half of each APU cycle (per `tick_with_external` toggle
semantics: `apu_phase = !apu_phase; if apu_phase { pulse/noise/DMC clock
}`). The two emulators use different signal sources but converge on the
same put/get classification of CPU cycles when initial alignment matches.

**Mesen2's IRQ-flag clear** (`GetIrqFlag()`):

```cpp
bool GetIrqFlag()
{
    if(_irqFlag) {
        uint64_t clock = _console->GetMasterClock();
        if(_irqFlagClearClock == 0) {
            _irqFlagClearClock = clock + ((clock & 0x01) ? 2 : 1);
        } else if(clock >= _irqFlagClearClock) {
            _irqFlagClearClock = 0;
            _irqFlag = false;
        }
    }
    return _irqFlag;
}
```

The IRQ flag clears at the **next APU cycle boundary** (master clock
crossing), accounting for parity. The clear is observable on the FIRST
read; subsequent reads on the next cycle see the cleared flag. This is
the behavior RustyNES's `pending_irq_clear` flag models, but via
`apu_aligned` rather than master-clock parity.

### Controller Strobing (Mesen2 `Core/Shared/BaseControlDevice.cpp`)

```cpp
void StrobeProcessWrite(uint8_t value)
{
    bool prevStrobe = _strobe;
    _strobe = (value & 0x01) == 0x01;
    if(prevStrobe && !_strobe) {
        RefreshStateBuffer();   // FALLING-EDGE latch
    }
}

void StrobeProcessRead()
{
    if(_strobe) {
        RefreshStateBuffer();   // continuous reload while strobe high
    }
}
```

Mesen2's strobe latch fires on the **falling edge** of bit 0
(high→low). RustyNES's `Controller::write_strobe`:

```rust
pub const fn write_strobe(&mut self, value: u8) {
    let new_strobe = value & 1 != 0;
    if new_strobe {
        self.shift = self.buttons.bits();   // RISING-EDGE-and-continuous latch
    }
    self.strobe = new_strobe;
}
```

RustyNES latches on the **rising edge** (and continuously while strobe
is high via the same code path). The two impls converge for the common
two-write sequence `STA $4016 ($01)` then `STA $4016 ($00)`:

- Mesen2: rising edge does nothing; falling edge latches.
- RustyNES: rising edge latches; falling edge does nothing.

For the common case the end state is the same. **But for the RMW
sequence** `DEC $4016` (`read=$41 → write=$41 → write=$40`), the
emulators diverge:

- Mesen2: prevStrobe was false (bit 0 of $40 in RAM); first write $41
  sets _strobe=true (rising), no latch. Second write $40 sets _strobe=false
  (falling), latches. Result: latches.
- RustyNES: first write $41 sets new_strobe=true, latches buttons.
  Second write $40 sets new_strobe=false, does not re-latch. Result:
  latches.

Both emulators latch in this scenario. **But the AccuracyCoin Test 4
expects NO latch in the get-put-aligned variant of the RMW.** Neither
RustyNES NOR Mesen2's code in isolation models this — the put/get
gating must come from a separate axis on the bus side.

## AccuracyCoin Controller Strobing test analysis

From `AccuracyCoin.asm` lines 8574-8635 (test #102, result address
`$045F`):

The test exercises FOUR sub-tests:

| # | Test | Expected pass condition |
|---:|---|---|
| 1 | `LDA #2; STA $4016` (bit 0 = 0) → controllers NOT strobed | Shift register holds previous value |
| 2 | `LDA #3; STA $4016` (bit 0 = 1, then $4016=$00) → controllers ARE strobed | Shift register holds live buttons |
| 3 | `DEC $4016` (RMW) aligned to put-get-put boundary → controllers ARE strobed | Latch fires on put-aligned strobe rise |
| 4 | `DEC $4016` (RMW) after `LDA <$00` (3 extra cycles) → controllers NOT strobed | Latch does NOT fire on get-aligned strobe rise |

The **error 4** failure at our baseline means Tests 1+2+3 pass but Test
4 fails. Per the test description (lines 8617-8624):

> ; This test will run DEC $4016
> ; cycle 5: write ($41) to $4016, then DEC to ($40)
> ; cycle 6: write ($40) to $4016
> ;
> ; This results in a 1-cycle strobe of the controller ports!
> ; - if that 1-cycle strobe happens on a get cycle, the controllers
> ;   actually aren't strobed at all!
> ; - But if the strobe occurs on a put cycle, the controllers DO get
> ;   strobed.

The architectural model:

- The standard NES controller latches **only on the M2-low (put) half**
  of a CPU cycle where `$4016 bit 0` rises 0→1.
- For `STA $4016` (write), the bit-0 value is held for the entire cycle
  including both M2-low and M2-high halves; the next cycle's M2-low
  sees the new bit-0 value and the LATCH circuit's RS-flop transitions
  if bit 0 went 0→1 between the previous cycle's M2-low and this
  cycle's M2-low.
- For `DEC $4016`, the bit-0 sequence is:
  - Cycle 4 (read): bit 0 = the value previously in $4016 (per the test,
    $00; so $40's bit 0 = 0). Strobe value visible at M2-low: 0.
  - Cycle 5 (RMW dummy write): writes $41 (bit 0 = 1). Strobe value at
    M2-low: 0 (the new value $41 is driven on M2-high). Strobe value at
    M2-high: 1.
  - Cycle 6 (RMW modified write): writes $40 (bit 0 = 0). Strobe value
    at M2-low: 1 (carryover from cycle 5's M2-high). At M2-high: 0.

The latch fires on the M2-low boundary where strobe transitions 0→1.
For the unaligned case (Test 4), the LDA #2 + LDA $00 (3 cycles)
shifts the RMW pattern by one CPU cycle, which means cycle 5's M2-high
write of $41 lands on a *get* cycle's M2-high. The latch's RS-flop
only sees a rising edge on M2-low of cycle 6 IF the M2-low of cycle 5
was already at strobe=1; but in the unaligned variant cycle 5's M2-low
was at strobe=0 (the OLD $00 value), and cycle 6's M2-low is also at
strobe=0 (the OLD $40 value driven late). The 0→1→0 pulse fully
fits within M2-high of cycle 5 — never visible on an M2-low boundary
— so the latch never fires.

## Required production change (deferred)

A correct fix would:

1. Replace `controller.write_strobe(value)` in `bus.rs:1516-1517` with
   a phase-aware call. The bus already knows the current cycle's
   `apu_phase` (via `self.apu.apu_phase()`).
2. Re-derive the strobe-latch firing rule: latch fires on the M2-low of
   the cycle where `strobe` is observed to go 0→1.
3. Track the previous cycle's strobe value to detect the transition.

The change is small but the **risk surface is wide**: any change to
controller strobe semantics ripples into all controller-input tests
(`cpu_dummy_writes_oam` cycle accounting, `apu_test` sub-tests that
write to `$4016` for synchronisation, every commercial-ROM oracle ROM
that strobes controllers).

## DMC test #100 (error 21) cross-reference

`APU Tests :: Delta Modulation Channel [error 21]` is test #100. The
high error code suggests deep into the test — likely a subtle DMC
timing edge case. Without a Mesen2 oracle trace covering this
specific sub-test, I cannot precisely identify the failing axis.
DMC scheduler put/get plumbing is already implemented (Session-21
visibility); a fix would require the same oracle work that blocks
Sprint 1 Phase 1B.

## APU Register Activation [error 4] cross-reference

Test #101 (result `$045C`). The "Register Activation" test family
exercises the cycle on which each `$40xx` register write actually
takes effect. Per nesdev wiki, some registers (e.g. `$4017`) take
effect 3-4 cycles after the write; others (e.g. `$4000-$4013`) take
effect immediately. The error 4 failure suggests one of 4 sub-tests
fails. The put/get axis is a candidate but not confirmed.

## Frame Counter IRQ [error 7]

Test #97 (result `$0467`). Sub-test 7 specifically tests `$4015` read
clearing the frame counter IRQ flag on put-vs-get cycle alignment —
**precisely** the surface that `frame_counter.rs:read_status` models
via `apu_aligned` + `pending_irq_clear`. The test fails despite the
existing plumbing. Two possibilities:

1. The `apu_aligned` passed to `read_status` is the WRONG semantic
   value (e.g. inverted, or one-cycle-off). The current code uses
   `self.apu_phase`; per the test, the relevant phase is the M2-low
   of the read cycle — which may not be `apu_phase` at the moment of
   the `$4015` read.
2. The `pending_irq_clear` semantics are off: the cleared-on-next-
   get-cycle behavior may need to be cleared-on-the-CURRENT-get-cycle
   when the read itself was on a get cycle (currently does this), or
   cleared-on-the-NEXT-cycle ALWAYS (alternative model).

Without a Mesen2 trace showing the exact CPU cycle on which the IRQ
flag is observed to clear after the read, both hypotheses are
speculation.

## Decision: Sprint 2 deferred to investigation-only

Per the user's brief:

> If at any point a fix candidate cascades into commercial-ROM oracle
> without prior user-authorized re-baselining, STOP. Don't re-baseline
> 60 ROMs to claim a 1-test AccuracyCoin gain.

The 4 target tests share infrastructure with the load-bearing
`apu_test/*` (8 strict), `apu_mixer/*` (4 strict), `dmc_dma_during_read4`
(5 strict) surfaces. Any speculative fix carries cascade risk into one
or more of these — and the cascade pattern from Sessions 19 + 20 (the
DMC scheduler axis) is the warning signal that put/get plumbing
changes are NOT safe without empirical evidence.

The Sprint 2 spec itself says (line 116-118):

> Effort: 2-3 days (more research-heavy than Sprint 1; the put/get
> convention is poorly documented and Mesen2 is the only reliable
> oracle).

The Mesen2 oracle remains blocked on the wall-time issue documented in
`session-22-sprint1-iter2-phase-b-2026-05-22.md`. Sprint 2 therefore
ships:

- **This audit doc** as the precise put/get analysis (Mesen2 cross-ref +
  AccuracyCoin sub-test architectural model).
- **Sprint 2 file status: INVESTIGATION-ONLY** — the file's "Status"
  header gains an iteration 1 entry; iteration 2 unblocks on the same
  paths Sprint 1 Phase 1B unblocks on (native Mesen2 debug hooks OR a
  custom AccuracyCoin sub-test ROM).

## Sprint 2 final state

| Surface | State |
|---|---|
| Workspace tests `--features test-roms` | **541 strict + 5 ignored** (baseline preserved; no chip-stack code changed) |
| AccuracyCoin pass rate | **82.73%** (unchanged) |
| Commercial-ROM oracle | Not re-run (no chip-stack code change) |
| B4 invariants | Preserved |
| Sacred trio | Preserved |
| `cargo fmt --all --check` | PASS |
| Sprint 2 outcome | **INVESTIGATION-ONLY** — Mesen2 + AccuracyCoin cross-reference landed; production change deferred. |

## Combined sprint sequence outcome

| Sprint | Phase | Outcome | Pass rate change |
|---|---|---|---:|
| 1 (iter 2 Phase 1A) | Mesen2 Lua AccuracyCoin protocol | LANDED | 0 |
| 1 (iter 2 Phase 1B) | Mesen2 oracle generation | DEFERRED (wall-time blocker) | 0 |
| 2 | APU put/get phase plumbing | INVESTIGATION-ONLY (oracle blocker shared) | 0 |

AccuracyCoin pass rate: **82.73% → 82.73%** (unchanged across the
session — neither Sprint 1 nor Sprint 2 ran a chip-stack code change).

## File changes summary

- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md`: this doc.
- `to-dos/phase-6-v1.0.0-final/sprint-2-apu-put-get-phase.md`: status
  bumped to INVESTIGATION-ONLY iteration 1.
- `to-dos/phase-6-v1.0.0-final/sprint-gate-conditions.md`: Sprint 2
  entry added.
- `CHANGELOG.md`: `[Unreleased]` Sprint 2 investigation entry.

## References

- Mesen2 `Core/NES/APU/ApuFrameCounter.h` (`WriteRam`, `Run`,
  `GetIrqFlag`).
- Mesen2 `Core/Shared/BaseControlDevice.cpp` (`StrobeProcessWrite`,
  `StrobeProcessRead`).
- AccuracyCoin source `AccuracyCoin.asm` lines 8574-8635 (Controller
  Strobing test #102).
- RustyNES `crates/nes-apu/src/apu.rs:516, 546, 572, 616` (existing
  put/get-aware register paths).
- RustyNES `crates/nes-apu/src/frame_counter.rs:109-152` (existing
  put/get-aware $4017 write + $4015 read).
- RustyNES `crates/nes-core/src/controller.rs:88-114` (current
  rising-edge-and-continuous strobe latch).
- `to-dos/phase-6-v1.0.0-final/sprint-2-apu-put-get-phase.md`.
- `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md` (shared
  oracle blocker).
