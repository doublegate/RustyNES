# Session 20 — Sprint 1 investigation: DMC abort gating mechanism + coordinated implied-dummy-read fix design

**Date:** 2026-05-22
**Branch:** `main` (HEAD `d516058` at session start)
**Scope:** Phase 1 of Sprint 1 (`to-dos/phase-6-v1.0.0-final/sprint-1-implied-dummy-dmc-coordinated.md`).
**Outcome:** Investigation complete. Option A selected (Mesen2-aligned).
**Predecessor:** `session-19-accuracycoin-pivot-2026-05-22.md` (the naive-attempt cascade revert).

## Baseline at session start

- HEAD `d516058` (post-rc2 release notes + Phase 6 backlog).
- Workspace: **541 strict pass + 5 expected-fail `#[ignore]`'d** with `--features test-roms` across 34 suites.
- AccuracyCoin: **82.73%** (108 pass + 7 pass_with_code of 139 assigned tests).
- Failing list (24): 1 Implied Dummy Reads + 5 SH* + 1 Open Bus #9 + 3 CPU Interrupts + 4 APU + 4 Sprite Eval + 6 PPU Misc.
- Implicit DMA Abort: **strict-pass** at baseline.

Reproduced via `cargo test -p nes-test-harness --features test-roms accuracycoin --release -- --nocapture`.

## Investigation methodology

Three-axis cross-reference:

1. **Nesdev canon**: `6502_cpu.txt` (canonical 6502 cycle structures) and the MOS 6502 datasheet
   (cycle-by-cycle behavior of implied / accumulator addressing modes).
2. **Mesen2 source** as the reference implementation (Option A oracle).
3. **AccuracyCoin source** at `/tmp/AccuracyCoin.asm` (extracted by Session-D2 catalog tooling)
   — both the `Implied Dummy Reads` test sequence (line 11782+) and the `Implicit DMA Abort`
   test sequence (line 13153+) read in full.

## Finding 1: Mesen2's gating mechanism

Mesen2 (Core/NES/NesCpu.cpp lines 254-268, 270-292):

```cpp
uint8_t NesCpu::MemoryRead(uint16_t addr, MemoryOperationType operationType) {
    ProcessPendingDma(addr, operationType);          // ALL reads check DMA gating
    StartCpuCycle(true);
    uint8_t value = _memoryManager->Read(addr, operationType);
    EndCpuCycle(true);
    return value;
}

void NesCpu::MemoryWrite(uint16_t addr, ...) {
    // NO ProcessPendingDma call — writes do NOT service DMC DMA
    _cpuWrite = true;
    StartCpuCycle(false);
    _memoryManager->Write(addr, value, operationType);
    EndCpuCycle(false);
    _cpuWrite = false;
}

uint16_t NesCpu::FetchOperand() {
    switch(_instAddrMode) {
        case NesAddrMode::Acc:
        case NesAddrMode::Imp: DummyRead(); return 0;     // *** implied = real bus read
        ...
    }
}

void DummyRead() {
    MemoryRead(_state.PC, MemoryOperationType::DummyRead);
}
```

**Mesen2 emits implied/accumulator cycle-2 as a `MemoryRead`** (with the
`DummyRead` operation type for tracing); this goes through the standard
read pipeline, including `ProcessPendingDma` which services any pending
DMC DMA halt.

The DMC abort path (Core/NES/NesCpu.cpp lines 534-548):

```cpp
void NesCpu::StopDmcTransfer() {
    if(_dmcDmaRunning) {
        if(_needHalt) {
            // If interrupted BEFORE the halt cycle starts, cancel DMA completely
            _dmcDmaRunning = false;
            _needDummyRead = false;
            _needHalt = false;
        } else {
            // Abort DMA if possible — only within the first cycle of DMA
            _abortDmcDma = true;
        }
    }
}
```

Triggered from `DeltaModulationChannel::ProcessClock` when `_disableDelay`
expires (the test sequence writes `$00 → $4015` and `$10 → $4015` to
toggle DMC; the disable+enable cadence is what produces the
"implicit abort" the test measures).

## Finding 2: RustyNES's current gating mechanism

RustyNES (`crates/nes-core/src/bus.rs` line 1374-1404):

```rust
impl Bus for LockstepBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.drain_dma(Some(addr));      // reads service DMC DMA
        ...
    }
    fn cpu_write(&mut self, addr: u16, value: u8) {
        self.drain_dma(None);            // writes do NOT service DMC DMA
        ...
    }
}
```

And `drain_dma` (line 960+):

```rust
fn drain_dma(&mut self, read_addr: Option<u16>) {
    if self.apu.dmc_abort_pending() {
        if let Some(addr) = read_addr {
            self.service_dmc_abort(addr);
        } else {
            self.apu.complete_dmc_abort();     // *** writes silently consume aborts
        }
    }
    ...
    if self.dma_cycles_owed == 0 {
        if let Some(addr) = read_addr {
            if self.apu.dmc_dma_pending() && !self.in_dmc_dma {
                self.service_dmc_dma(addr);
            }
        }
        return;
    }
    ...
}
```

The CPU's implied opcode handlers (`crates/nes-cpu/src/cpu.rs` lines
888-916 transfers, 1322-1339 INC/DEC, 1344-1422 shift-A, 1558-1593
flag/NOP):

```rust
0xAA => {                       // TAX
    self.x = self.a;
    self.p.set_nz(self.x);
    *cycles = 2;                // declares 2 CPU cycles
}
```

The opcode fetch (`fetch_pc`) consumes 1 cycle (via `read1`); the
remaining 1 cycle is emitted by the burn loop at the bottom of
`Cpu::step` (line 340-342) as `idle_tick`:

```rust
while self.cycles_emitted < cycles {
    self.idle_tick(bus);
}
```

`idle_tick` (line 449-481) ticks the PPU/APU lockstep and samples
interrupts but does **NOT** call `bus.cpu_read()` — so DMC DMA's
read-only gate (`read_addr.is_some()`) is never hit. The implied
opcode's cycle 2 is **bus-quiet** in current RustyNES, whereas
Mesen2 emits a real `DummyRead()` at PC.

## Finding 3: Why Session-19's naive fix cascaded

The naive fix added `implied_dummy_read(bus) → read1(bus, self.pc)`
to 21 instruction sites. This converted the bus-quiet T2 to a real
bus read at PC. The expected behavior:

1. **Target**: `CPU Behavior 2 :: Implied Dummy Reads` should flip
   FAIL → PASS because the cycle-2 dummy read now reaches `$4015`
   (the test sets up PC to land on `$4015`) and clears the frame
   counter IRQ flag.
2. **Side-effect**: DMC DMA pending during the implied opcode
   cycle would now fire there (RustyNES gates DMC DMA on
   `read_addr.is_some()`).

The observed cascade:

1. Target test did NOT flip (still error 3).
2. `APU Registers and DMA tests :: Implicit DMA Abort` REGRESSED
   from strict-pass to `[error 2]`.

The cascade root cause (per Session-19 audit + my deeper read):

`Implicit DMA Abort`'s `Key1`/`Key2`/`Key3` answer tables encode the
**expected DMA duration values** for each X iteration of the test
loop (lines 13298-13348 of AccuracyCoin.asm). These values are
calibrated against a canonical CPU that emits implied dummy reads.
At baseline (no implied dummies), RustyNES was producing the
correct DMA-duration sequence by accident — the bus-quiet cycle-2
delayed DMA service by exactly the right amount to match the
canonical answer.

Adding the implied dummy reads converges to canonical bus-cycle
patterns, but RustyNES's DMC DMA scheduler has **multiple
compensating delays** (`dmc_dma_cooldown = 4` after a load, `5`
after an early-deliver get; `dmc_abort_delay_for(2) = 2`, `(3) =
3`) that were also tuned to the non-canonical baseline. The
combined system is now off-by-one in some bus-phase corner cases.

The target test ALSO did not flip because:

- The test sequence is `JSR $4011 → DMC DMA → PHA → LDY <$A4 →
  LDA <$A5 → [opcode] → fetch from $4015`.
- The DMC DMA halt currently inserts 3-4 cycles in our scheduler
  AFTER `JSR $4011` (between JSR cycle 6 and PHA cycle 1).
- For the test to flip, the opcode's cycle-2 dummy read MUST land
  on `$4015` (which is `$4015 = PC + 0` after the JSR jumps to
  `$4011` — i.e. the next opcode fetch will be at `$4015`).
- The naive fix correctly places the dummy at PC, but the test's
  earlier `JMP $400F` (line 11860) does some PC arithmetic that
  the dummy read at PC alone may not satisfy. Need deeper
  cycle-trace.

## Finding 4: The structural surface

Both Mesen2 and RustyNES gate DMC DMA on **reads only** (writes do
not service DMA). The structural difference is NOT the gating
signal — both use the same "is this cycle a CPU read" axis.

The difference is **when implied opcodes emit a read**:
- Mesen2: implied cycle 2 = real `MemoryRead` (`DummyRead`).
- RustyNES: implied cycle 2 = `idle_tick` (no bus read).

This shifts which CPU cycle the DMA halt lands on, which
**propagates into the abort-window measurement** the test
performs.

## Coordinated design — Option A (selected)

**Option A**: Make the implied dummy read a real bus read, AND
audit the DMC DMA scheduler for any compensating delays that were
tuned to the bus-quiet baseline.

Concretely:
1. Add `Cpu::implied_dummy_read<B>(bus)` helper:
   ```rust
   fn implied_dummy_read<B: Bus>(&mut self, bus: &mut B) {
       let _ = self.read1(bus, self.pc);    // real read; PC unchanged
   }
   ```
2. Wire into all 21 implied/accumulator sites BEFORE setting
   `*cycles = 2`. The `read1` call consumes the cycle, so we
   also remove the burn-loop emission (set `*cycles = 1` for
   the dispatcher, OR keep `*cycles = 2` and rely on the fact
   that `read1` increments `cycles_emitted`).
3. Audit the DMC DMA scheduler's compensating delays for any
   off-by-one. The most likely candidates:
   - `dmc_dma_cooldown = 4` / `= 5` post-deliver (apu.rs:299, 333)
   - `dmc_abort_delay_for(2) = 2` / `(3) = 3` (apu.rs:103-108)
   - `dmc_dma_delay` from `write_register($4015)` enable path
   - Bus's `drain_dma` `read_addr.is_some()` short-circuit
     ordering relative to the abort path.

The naive Session-19 fix did step (1) but skipped step (3). The
coordinated fix must do both.

## Option A vs Options B/C — rejection rationale

**Option B** (heuristic update): keep the implied opcode as
bus-quiet but classify the cycle-2 internally as "still a real
read" for the DMC DMA gating. **Rejected**: this makes RustyNES
diverge from Mesen2's gating signal (and from canonical 6502).
Other tests that rely on bus-quiet semantics for read-sensitive
registers (e.g. `$2002` race window, controller strobing) would
have unpredictable interactions.

**Option C** (no-op idle tick visible only to specific bus units):
add a per-cycle "is bus-quiet" boolean that DMC DMA reads but
`$4015` flag-clearing reads. **Rejected**: same diverge-from-canon
issue, plus introduces a hidden state field that complicates the
save-state surface.

**Option A** is the structurally correct path. The DMC scheduler
audit in step (3) is the work the Session-19 attempt deferred.

## Estimated scope

- Step (1) implementation: 20 minutes (same as Session-19 naive
  attempt — the call sites are mechanical).
- Step (3) audit + adjustment: 1-3 hours (read the existing DMC
  scheduler carefully, identify compensating delays, adjust them
  to absorb the new bus-cycle pattern).
- Validation gauntlet: 30-45 minutes (full workspace + commercial
  oracle + AccuracyCoin re-measure).

**Phase 2 of Sprint 1 is in-scope for a single session.**

## Acceptance criteria for Phase 2

- Implied Dummy Reads error 3 → PASS.
- Implicit DMA Abort: stays strict-PASS.
- `dmc_dma_during_read4` (5 strict): stays 5/5.
- `apu_test` (8 strict): stays 8/8.
- `apu_mixer` (4 strict): stays 4/4.
- AccuracyCoin pass rate: monotonically increases (≥ 82.73%).
- B4 invariant preserved.
- Sacred trio preserved.
- Commercial-ROM oracle: stays 60/60 with byte-identical FNV-1a
  audio + cumulative cycle counts.

If gauntlet is green, **Phase 2 lands.** If any cascade surfaces,
revert and land this investigation doc as audit-only commit.

## References

- nesdev `6502_cpu.txt` (canonical 6502 cycle structures).
- MOS 6502 datasheet (cycle-by-cycle behavior of implied / accumulator).
- nesdev wiki APU DMC §"DMA conflicts"
  (https://www.nesdev.org/wiki/APU_DMC).
- Mesen2 `Core/NES/NesCpu.cpp` (lines 191, 194-195, 254-268, 270-292,
  325-448, 520-548).
- Mesen2 `Core/NES/APU/DeltaModulationChannel.cpp` (lines 275-290).
- RustyNES `crates/nes-cpu/src/cpu.rs` (lines 280-345 `step`,
  389-393 `fetch_pc`, 449-481 `idle_tick`, 485-489 `read1`,
  888-916 transfers, 1322-1339 INC/DEC implied, 1344-1422 shift-A,
  1558-1593 flag/NOP).
- RustyNES `crates/nes-core/src/bus.rs` (lines 960-1010 `drain_dma`,
  1044-1079 `service_dmc_dma` + `service_dmc_abort`, 1374-1405
  Bus impl).
- RustyNES `crates/nes-apu/src/apu.rs` (lines 102-109
  `dmc_abort_delay_for`, 229-340 DMC pending/abort API, 359-431
  `tick_with_external` with DMA delay state machine).
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md`.
- `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` (rows for `Implied
  Dummy Reads = 0x046D` and `Implicit DMA Abort = 0x0478`).
- AccuracyCoin source `/tmp/AccuracyCoin.asm` lines 11782-11941
  (`Implied Dummy Reads` test) and 13153-13348 (`Implicit DMA
  Abort` test).

## Sprint 1 progression

- Phase 1 (this doc): COMPLETE.
- Phase 2 (Option A iteration 1): INVESTIGATED + ROLLED BACK
  (this session, 2026-05-22). See "Phase 2 — investigated and
  rolled back" below.

## Phase 2 — investigated and rolled back (Option A iteration 1)

### Implementation summary

Per the Phase 1 design, landed under feature flag
`cpu-implied-dummy-coordinated` (default OFF; propagated through
`nes-cpu` → `nes-core` → `nes-test-harness`):

- `crates/nes-cpu/src/cpu.rs` added `Cpu::implied_dummy_read<B>(bus)`
  helper that emits `read1(bus, self.pc)` when the feature is on,
  else no-op. PC is already post-incremented from the cycle-1
  opcode fetch (see `fetch_pc`: `read1(bus, self.pc); self.pc =
  self.pc.wrapping_add(1)`), so `self.pc` at helper entry points
  to the byte AFTER the opcode — matching Mesen2's `DummyRead()`
  → `MemoryRead(_state.PC, MemoryOperationType::DummyRead)` at
  the post-`GetOPCode` PC (Mesen2 NesCpu.h lines 72-82).
- Wired into all 21 sites: 6 transfers (TAX, TAY, TSX, TXA, TXS,
  TYA), 4 INC/DEC implied (INX, DEX, INY, DEY), 4 shift-A
  (ASL A, LSR A, ROL A, ROR A), 1 NOP (`$EA`), 7 flag ops
  (CLC, SEC, CLI, SEI, CLV, CLD, SED), 6 unofficial 1-byte NOPs
  (`$1A`/`$3A`/`$5A`/`$7A`/`$DA`/`$FA`) — all set `*cycles = 2`
  unchanged; the helper's `read1` increments `cycles_emitted` so
  the burn loop emits zero residual idle cycles when feature ON.

### Validation gauntlet result

- `cargo fmt --all --check`: PASS (no diff).
- `cargo clippy --workspace --all-targets --features test-roms
  -- -D warnings`: PASS (4 `#[allow]` annotations needed on
  `implied_dummy_read`: `unused_variables`,
  `needless_pass_by_ref_mut`, `missing_const_for_fn`, `unused_self`
  for the feature-OFF no-op shape; `inline_always` for the
  feature-ON hot-path attribute).
- `cargo clippy --workspace --all-targets --features
  test-roms,cpu-implied-dummy-coordinated -- -D warnings`: PASS.
- `cargo build --workspace`: PASS.
- `cargo test --workspace --features test-roms` (feature OFF):
  **541 strict + 5 ignored — baseline preserved.**
- `cargo test --workspace --features
  test-roms,cpu-implied-dummy-coordinated`: **541 strict + 5
  ignored — no workspace regressions.** (Includes
  `dmc_dma_during_read4` 5/5, `apu_test` 8/8, `apu_mixer` 4/4,
  `ppu_vbl_nmi` 10/10, `sprite_hit_tests` 11/11,
  `mmc3_test_2/*` strict.)
- `cargo test -p nes-test-harness --features
  test-roms,cpu-implied-dummy-coordinated accuracycoin --release
  -- --nocapture`: **REGRESSED 82.73% → 82.01%** (108 pass + 7
  pass_with_code → 108 pass + 6 pass_with_code). Identical to
  Session-19's cascade:
  - Target `CPU Behavior 2 :: Implied Dummy Reads` did NOT flip
    (stayed `[error 3]`).
  - `APU Registers and DMA tests :: Implicit DMA Abort` REGRESSED
    from strict-pass to `[error 2]`.

### Why the iteration didn't yield (deeper diagnosis)

Two distinct paths contribute to the no-flip + cascade:

1. **The target test doesn't flip purely from adding the
   dummy.** The AccuracyCoin `Implied Dummy Reads` test sequence
   (line 11826-11878) injects the under-test opcode through a
   sophisticated open-bus mechanism: `JSR $4011 → DMC DMA (data
   bus = $48) → PHA → LDY <$A4 → LDA <$A5 → [opcode] → fetch
   from $4015`. The `[opcode]` step doesn't fetch from ROM — it
   fetches from open bus (which the DMC DMA seeded). For the
   under-test opcode's cycle-2 dummy read to clear `$4015`'s
   frame counter IRQ flag, PC at that moment must be `$4015`.
   The setup arranges PC=$4014 at opcode-fetch time, PC→$4015
   after fetch, so cycle-2 dummy at PC=$4015 is correct in
   theory.

   But the trace shows the test STILL doesn't flip. Hypothesis:
   either (a) RustyNES's open-bus seeding via DMC DMA is not
   delivering `$48` to the bus at the precise cycle the test
   expects, OR (b) the implied dummy read at `$4015` isn't
   actually reaching `$4015` in our scheduler's bus path (the
   DMC DMA halt may insert itself before the dummy can resolve,
   moving the effective address). Diagnosing (a) vs (b)
   requires a per-CPU-cycle bus-access trace comparison with
   Mesen2 — out of scope for this sprint's effort budget.

2. **The cascade into Implicit DMA Abort is the previously-
   diagnosed off-by-one.** The fix converges RustyNES toward
   canonical bus-cycle behavior, but RustyNES's DMC DMA
   scheduler has compensating delays
   (`dmc_dma_cooldown = 4`/`5`, `dmc_abort_delay_for(2) = 2`/
   `(3) = 3`, plus the bus-side `drain_dma` ordering) that were
   tuned to the bus-quiet baseline. Without re-tuning, the
   post-fix bus-cycle pattern produces off-by-one DMA durations
   in `Implicit DMA Abort`'s `CalculateDMADuration` measurement
   loop, which compares against `Key1`/`Key2`/`Key3` answer
   tables.

   Re-tuning is a multi-session DMC scheduler audit (compare each
   compensating-delay constant against Mesen2's `NesApu.cpp` +
   `DeltaModulationChannel.cpp` per-CPU-cycle behavior; we have
   per-CPU-cycle IRQ trace infrastructure (Phase A) but need to
   extend it to capture DMC-DMA-state per cycle).

### Decision: rollback Phase 2 code change

Per the Sprint 1 spec decision tree:

> Flag ON: AccuracyCoin regresses by ANY amount: revert. Try a
> different design.

The cascade is real and reproducible (Session-19 saw the same
shape). The Phase-1-recommended "Option A iteration 1" was the
naive coordinated form; closing the cascade requires Option A
iteration 2: a DMC scheduler cycle-by-cycle audit against
Mesen2. That work is multi-session and out of Sprint 1's effort
budget.

The Phase 2 code (helper + 21 call sites + feature scaffolding)
has been reverted via `git checkout --
crates/nes-cpu/src/cpu.rs crates/nes-cpu/Cargo.toml
crates/nes-core/Cargo.toml crates/nes-test-harness/Cargo.toml`.
Verified post-revert: workspace 541 strict + 5 ignored
restored, AccuracyCoin 82.73% restored.

### Next steps (for future Sprint 1 re-attempt or Sprint 2)

Two candidate paths:

1. **Sprint 1 iteration 2** (extend to multi-session): add
   per-CPU-cycle DMC DMA state to the existing irq-timing-trace
   fixture; generate Mesen2 reference DMC traces of the
   `Implicit DMA Abort` test; diff per-cycle to identify which
   compensating-delay constant to retune. Estimated 3-5 days.

2. **Skip to Sprint 2** (APU put/get phase plumbing). Sprint 2's
   yield estimate is +1 to +3 tests including Frame Counter IRQ
   #7 and possibly Controller Strobing #4 — independent of the
   Implied Dummy Reads / DMC scheduler surface.

Recommendation: defer Sprint 1 iteration 2 to a future session
when the DMC trace tooling is built; proceed to Sprint 2 in the
next session.

### Bottom-line metrics (Phase 2 + rollback)

- Workspace strict: 541 → 541 (no change — rollback restored).
- AccuracyCoin: 82.73% → 82.73% (no change — rollback restored).
- Commercial-ROM oracle: untouched (no commits with chip-stack
  diff).
- B4 invariant: untouched.
- Sacred trio: untouched.
- Implicit DMA Abort: strict-pass restored.
- Code commits with chip-stack changes: 0 (Phase 2 reverted).
- Audit-doc commits: 1 (this doc, Phase 1) + 1 (this addendum,
  Phase 2 rollback).
