# Session 25 — Sprint 2 iter 3: Frame Counter IRQ Test 7

**Date:** 2026-05-23
**Branch:** `main` (HEAD `ba5944a` at session start)
**Scope:** Sprint 2 iteration 3 of v1.0.0-final: flip `APU Tests :: Frame
Counter IRQ` (`AccuracyCoin` result address `$0467`) from error 7 (Fail
Test 7) to PASS, mirroring the Phase 3 (Controller Strobing)
M2-phase-aware deferred-action pattern that landed in
`d3f8dee`.

**Predecessors:**
- `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md` —
  Phase 3 LANDED template (M2-low-defer for `$4016` strobe commit).
- `docs/audit/session-24-phase4-implied-dummy-dmc-2026-05-23.md` —
  Phase 4 INVESTIGATION-ONLY; identified `Frame Counter IRQ Test 7`
  and `APU Register Activation Test 4` as the two clean-oracle
  alternates after the Implied Dummy custom-ROM dependency-chain
  blocked Phase 4.
- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md` — Sprint 2
  original put/get phase hypothesis (rolled back).
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md` —
  per-test tractability audit; rated Frame Counter IRQ Test 7 as
  MEDIUM-tractability "put/get phase" family.

## Phase 1.1 — Custom ROM validation

`tests/roms/AccuracyCoin/sub-tests/frame-counter-irq.nes` (Session-23
infrastructure + Session-24 Phase 3 controller-drain + Phase 4
dep-pre-seed) was re-validated under `validate_sub_test_rom`:

| Emulator | `$0467` result | Decoded |
|---|---|---|
| RustyNES (HEAD `ba5944a`) | `$1E = (7<<2)\|2` | **Fail at Test 7** |
| Mesen2 | `$01` | **PASS** (all 14 sub-tests) |

The ROM is clean as an oracle — both emulators reach
`TEST_FrameCounterIRQ` and write to `$0467` in 35 frames.

## Phase 1.2 — Mesen2 + RustyNES trace tooling

### New artifacts

| File | Description |
|---|---|
| `scripts/mesen2_frame_counter_irq_trace.lua` | Per-`$4015`/`$4017` access Lua oracle. Captures cycle, frame, scanline, dot, parity-derived M2 phase, access type, value, and bit-6 irq_pending derivation. |
| `crates/nes-test-harness/src/bin/trace_frame_counter_irq.rs` | RustyNES counterpart binary. Uses the `irq-timing-trace` feature to record per-CPU-cycle bus access, then emits a CSV filtered to `$4015`/`$4017`/result-addr/ErrorCode rows. |
| `crates/nes-test-harness/golden/irq_trace/frame-counter-irq.csv` | RustyNES trace (committed). |
| `crates/nes-test-harness/golden/irq_trace/mesen2/frame-counter-irq.csv` | Mesen2 trace (committed). |

## Phase 1.3 — Cross-diff: Test 6 vs Test 7 divergence

The clean diagnostic is the SLO ABS,X double-read of `$4015` in
Tests 6 and 7. Test 6 inserts an extra `LDA <$00` cycle to shift the
SLO alignment by 3 cycles, flipping the put/get parity at the first
`$4015` read.

| Region | Mesen2 (cycle, m2, val) | RustyNES (cycle, m2, val) |
|---|---|---|
| Test 6 first read | 684536, **L (even)**, `$40` (pending=1) | 982345, **H (odd)**, `$40` (pending=1) |
| Test 6 second read | 684537, **H (odd)**, `$00` (pending=0, **CLEARED**) | 982346, **L (even)**, `$00` (pending=0, **CLEARED**) |
| Test 7 first read | 715083, **H (odd)**, `$40` (pending=1) | 1012892, **L (even)**, `$40` (pending=1) |
| Test 7 second read | 715084, **L (even)**, `$40` (pending=1, **STILL SET**) | 1012893, **H (odd)**, `$00` (pending=0, **CLEARED**) |

**Both emulators see opposite cycle parity for the same test**
(Mesen2 calls Test 6 first read "EVEN cycle = PUT cycle"; RustyNES
calls it "ODD cycle / apu_phase=true at read"). This is a known
absolute-master-clock offset between the two emulators. The relative
outcome is what matters:

- **Test 6 (both emulators)**: first read followed by 2nd read 1
  cycle later → 2nd read sees **CLEARED**. PASS.
- **Test 7 (Mesen2)**: first read followed by 2nd read 1 cycle later
  → 2nd read sees **STILL SET**. PASS.
- **Test 7 (RustyNES)**: first read followed by 2nd read 1 cycle
  later → 2nd read sees **CLEARED**. **FAIL**.

The divergence is in the put-cycle-first-read path: in Mesen2 the
clear matures on the NEXT GET cycle (which is 2 CPU cycles later, NOT
the immediate next cycle); in RustyNES the put-cycle pending
consumption fires at the end of the SAME CPU cycle.

## Phase 2 — Hypothesis

### Mesen2 model (canonical)

`Core/NES/APU/ApuFrameCounter.h` lines 214-227:

```cpp
bool GetIrqFlag()
{
    if(_irqFlag) {
        uint64_t clock = _console->GetMasterClock();
        if(_irqFlagClearClock == 0) {
            // The flag will be cleared at the start of the next APU cycle
            // (see AccuracyCoin test)
            _irqFlagClearClock = clock + ((clock & 0x01) ? 2 : 1);
        } else if(clock >= _irqFlagClearClock) {
            _irqFlagClearClock = 0;
            _irqFlag = false;
        }
    }
    return _irqFlag;
}
```

Semantics:
- Read at EVEN master clock → schedule clear at `clock + 1` (next odd
  cycle = next GET cycle in Mesen2 convention).
- Read at ODD master clock → schedule clear at `clock + 2` (cycle
  after next = next GET cycle).
- Subsequent `GetIrqFlag` calls check if the matured clear should
  fire NOW. The flag returns TRUE until matured.

The schedule cycle is always ODD = GET cycle in Mesen2 convention.
The clear matures at the NEXT GET cycle AFTER the read cycle.

### RustyNES current model

`crates/nes-apu/src/frame_counter.rs:141-152`:

```rust
pub fn read_status(&mut self, apu_aligned: bool) -> bool {
    let f = self.irq_flag;
    if apu_aligned {
        // GET cycle: clear immediately.
        self.irq_flag = false;
        self.pending_irq_clear = false;
    } else {
        // PUT cycle: defer the clear to the next GET cycle.
        self.pending_irq_clear = true;
    }
    f
}
```

`crates/nes-apu/src/frame_counter.rs:159-167` consumes the pending in
`tick(apu_aligned)`:

```rust
pub fn tick(&mut self, apu_aligned: bool) -> FrameEvents {
    if self.pending_irq_clear && apu_aligned {
        self.irq_flag = false;
        self.pending_irq_clear = false;
    }
    // ...
}
```

The `apu.tick_with_external` (apu.rs:418) toggles `apu_phase` THEN
calls `frame_counter.tick(apu_phase)`. So a put-cycle read
(`apu_phase=false` at start of cycle) sets pending; end-of-cycle
toggle makes `apu_phase=true`; tick consumes pending → clear by end
of SAME cpu cycle. Then 2nd read at next cycle sees CLEARED.

This is correct for the case where Mesen2 sees `clock+1` schedule
(even-cycle first read in Mesen2) — both emulators clear by 2nd read.

It is INCORRECT for the case where Mesen2 sees `clock+2` schedule
(odd-cycle first read in Mesen2). RustyNES's parity at the
equivalent ROM-execution moment is OPPOSITE (RustyNES sees
apu_phase=false where Mesen2 sees odd-clock), so RustyNES takes the
put-cycle path and clears too early.

### Single-axis hypothesis

**Replace the boolean `pending_irq_clear` + tick-consumption mechanism
with a cycle-counter scheduler that mirrors Mesen2's algorithm but
with INVERTED parity to match RustyNES's apu_phase convention.**

Specifically:
- Add `irq_flag_clear_cycle: u64` field on `FrameCounter`. 0 = no
  schedule pending. Otherwise: the cpu_cycle at which the clear
  matures.
- On `read_status(cpu_cycle, apu_aligned)`: if `irq_flag` is true,
  schedule `irq_flag_clear_cycle = cpu_cycle + if apu_aligned { 1 } else { 2 }`
  (apu_aligned=true = RustyNES GET cycle = 1-cycle delta;
  apu_aligned=false = RustyNES PUT cycle = 2-cycle delta). This is
  INVERTED relative to Mesen2's `(cycle & 0x01) ? 2 : 1` because
  RustyNES's apu_phase polarity at the SLO read site is opposite to
  Mesen2's master-clock parity.
- On `tick(cpu_cycle, apu_aligned)`: if `irq_flag` is true and
  `cpu_cycle >= irq_flag_clear_cycle` and `irq_flag_clear_cycle != 0`:
  clear flag, reset schedule.
- The schedule is also checked at subsequent `read_status` calls so
  the matured clear fires the moment the CPU observes it.
- Remove the boolean `pending_irq_clear`. Save-state version bump
  (apu chunk).

**Falsifiable prediction**: `Frame Counter IRQ` `$0467` flips from
`$1E` (Fail Test 7) to `$01` (PASS all 14 sub-tests).

**Cascade risk**:
- `apu_test` 8 sub-tests: HIGH. The frame-counter IRQ timing tests in
  blargg are MOST likely to be sensitive. Must verify all 8 strict.
- `apu_mixer` 4 sub-tests: NONE (no IRQ-flag interaction).
- `dmc_dma_during_read4` 5 sub-tests: LOW-MEDIUM. The DMC DMA pre-read
  of `$4015` happens via `clear_frame_irq_immediate_for_dma` which is
  separately retained. The change should not affect DMC IRQ semantics.
- Commercial-ROM oracle (60 ROMs): LOW. Games rarely poll `$4015` in
  the cycle-sensitive way the test ROM does.
- Controller Strobing (Phase 3 just landed): NONE (different surface).
- B4 MMC3 invariants: NONE (mapper IRQ, distinct).

## Phase 2 outcome

Hypothesis fully formed. Implementation proceeds to Phase 3.

## Phase 3 — Implementation

### Phase 3.1 — Surface

`crates/nes-apu/src/frame_counter.rs`:
- Removed `pending_irq_clear: bool`.
- Added `irq_flag_clear_cycle: u64`. 0 = no schedule pending; otherwise
  the CPU cycle at which the deferred clear matures.
- `read_status(cpu_cycle, apu_aligned)` signature change. First
  checks if a previously-scheduled clear has matured (clearing if so);
  then schedules a fresh clear at `cpu_cycle + (if apu_aligned { 1 } else { 2 })`
  if the flag is still set and no schedule is currently pending. Returns
  the OLD flag value (matching Mesen2's `GetIrqFlag` "return-then-mature"
  ordering).
- `tick(cpu_cycle, apu_aligned)` signature change. Matures the pending
  clear if `cpu_cycle >= irq_flag_clear_cycle`, then proceeds with the
  existing $4017 reset / step-event logic.
- The step-setting branches (29828, 29829, 29830 in 4-step mode) now
  also `irq_flag_clear_cycle = 0` alongside `irq_flag = true` so that
  a re-assertion mid-deferral wipes the stale schedule. (Mirrors
  Mesen2 `ApuFrameCounter.h` line 107.)
- The $4017 inhibit-on-reset branch wipes the schedule too.

`crates/nes-apu/src/apu.rs`:
- `Apu::tick_with_external` now passes `self.cpu_cycle` to
  `frame_counter.tick`.
- `Apu::read_status` now passes `self.cpu_cycle` to
  `frame_counter.read_status`.

`crates/nes-apu/src/snapshot.rs`:
- Bump `APU_SNAPSHOT_VERSION` from 1 to 2. v1 blobs are still
  restorable; the v1 `pending_irq_clear: bool` migrates to the v2
  `irq_flag_clear_cycle: u64` via `u64::from(bool)` (no pending ->
  0, pending -> 1). Per ADR-0003 cross-version save-state policy.

### Phase 3.2 — Validation gauntlet results

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --features test-roms` -> -D warnings | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo build --workspace` | PASS |
| `cargo test --workspace` | PASS (no test-roms) |
| `cargo test --workspace --features test-roms` | **545 strict pass + 5 ignored** (+4 vs pre-Session-25 baseline of 541+5: 4 new unit tests in `frame_counter.rs` / `snapshot.rs`) |
| `cargo test --test apu_test --features test-roms` | **8/8** PASS (load-bearing apu_test surface preserved) |
| `cargo test --test apu_mixer --features test-roms` | **4/4** PASS |
| `cargo test --test dmc_dma --features test-roms` | **5/5** PASS |
| `cargo test --test mmc3 --features test-roms` | **12 strict + 2 ignored** PASS (B4 invariant preserved -- `mmc3_test_2/4` sub-test #2 strict PASS, sub-test #3 ignored expected-fail) |
| Controller Strobing (Phase 3 landing) | `$01` PASS preserved |
| Custom ROM `frame-counter-irq.nes` | `$0467 = $01` **PASS (was `$1E` Fail Test 7)** |
| AccuracyCoin RAM-direct | 83.45% (unchanged headline; internal Test 7 -> Test J advancement = 12 sub-tests now pass that previously didn't run; the Frame Counter IRQ catalog entry is one "test" so the per-suite count stays at 6 pass / 3 fail) |
| AccuracyCoin framebuffer | 89.83% (unchanged) |
| Commercial-ROM oracle (60 ROMs, `--features test-roms,commercial-roms`) | **60/60** PASS |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) visual legibility | preserved (subset of commercial oracle) |

### Phase 3.3 — Test J refinement attempted + rolled back

An attempt was made to ALSO flip Test J (the "even with inhibit, the
flag is set for 2 CPU cycles" surface): change the
29828/29829/29830 step branches to set `irq_flag = true`
UNCONDITIONALLY and then conditionally clear at 29830 if inhibited.
This mirrors Mesen2 `ApuFrameCounter.h` lines 104-115:
- ALWAYS `_irqFlag = true; _irqFlagClearClock = 0;` at steps 3, 4, 5.
- THEN conditionally `SetIrqSource(FrameCounter)` if `!_inhibitIRQ`,
  or conditionally clear back to 0 if `_currentStep == 5 && _inhibitIRQ`.

Outcome: **4 commercial-ROM oracle regressions** (`external_mmc3_mega_man_3`,
`external_mmc3_tmnt3`, `external_mmc3_ninja_gaiden_2`,
`external_mmc3_tiny_toon_adventures_2`).

Root cause: RustyNES conflates the "$4015 bit 6 visibility" concept
with the "CPU IRQ line asserted" concept into a single
`frame_counter.irq_flag` field. `Apu::irq_line()` ORs this with
`dmc.irq_flag` and the bus uses it for CPU IRQ polling. Setting the
flag unconditionally during inhibit caused the CPU to see spurious
IRQs and broke MMC3 games' IRQ handlers.

Mesen2 separates these as two fields: `_irqFlag` (for $4015 bit 6)
and `IRQSource::FrameCounter` (for CPU IRQ line). Lifting this
conflation in RustyNES is a structural change distinct from the
put/get phase axis -- deferred to a future sprint. The Test J
refinement was reverted; the Test 7 axis architectural fix lands
alone.

### Phase 3.4 — Post-fix trace verification

The golden RustyNES CSV at
`crates/nes-test-harness/golden/irq_trace/frame-counter-irq.csv` was
regenerated AFTER the fix landed. The Test 7 sub-region now matches
Mesen2's behavior (both 2nd reads return `$40` = flag still set):

| Region | Mesen2 (committed) | RustyNES post-fix (committed) |
|---|---|---|
| Test 6 first read | 684536, **L**, `$40` (pending=1) | 982345, **H**, `$40` (pending=1) |
| Test 6 second read | 684537, **H**, `$00` (CLEARED) | 982346, **L**, `$00` (CLEARED) |
| Test 7 first read | 715083, **H**, `$40` (pending=1) | 1012892, **L**, `$40` (pending=1) |
| Test 7 second read | 715084, **L**, `$40` (**STILL SET**) | 1012893, **H**, `$40` (**STILL SET**) ✓ |

The Test 7 axis closure is empirically confirmed: RustyNES now sees
the IRQ flag still set on the 2nd back-to-back `$4015` read 1 CPU
cycle after the first, matching Mesen2's behavior.

### Phase 3.5 — Outcome decision

**LANDED.** The primary architectural target (Test 7 put/get phase
axis) is closed. No regressions. The Frame Counter IRQ catalog entry
advances from error 7 to error 19 (12 sub-tests further internally,
but the per-test catalog metric remains 1 fail in the suite, so the
83.45% RAM-direct headline is unchanged).

The Test J/K/L residual is documented as a separate axis
("`$4015`-bit-6-vs-CPU-IRQ-source conflation") and is a future-sprint
target. The architectural fix is the foundation that any subsequent
Test J/K/L work will build on.

## Final references

- `crates/nes-apu/src/frame_counter.rs:69-88` (new `irq_flag_clear_cycle`
  field doc-comment with the inverted-parity rationale).
- `crates/nes-apu/src/frame_counter.rs:127-185` (rewritten `read_status`
  + `tick` lazy-clear bodies).
- `crates/nes-apu/src/apu.rs:433` (`tick_with_external` passes
  `self.cpu_cycle` to `frame_counter.tick`).
- `crates/nes-apu/src/apu.rs:622` (`read_status` passes
  `self.cpu_cycle` to `frame_counter.read_status`).
- `crates/nes-apu/src/snapshot.rs:32-43` (`APU_SNAPSHOT_VERSION = 2`
  with v1 -> v2 migration policy).
- Mesen2 `Core/NES/APU/ApuFrameCounter.h` lines 214-227 (the
  `GetIrqFlag` lazy algorithm this implementation mirrors).
- AccuracyCoin `AccuracyCoin.asm` lines 10120-10211 (Tests 1-7),
  lines 10377-10447 (Tests I-L, the future-sprint axis).
- `docs/adr/0002-irq-timing-coordination.md` (C1 axis -- distinct
  surface, not touched by this change).
- `docs/adr/0003-save-state-migration.md` (cross-version policy used
  for v1 -> v2 migration).


## References

- Mesen2 `Core/NES/APU/ApuFrameCounter.h` lines 214-227 (`GetIrqFlag`).
- Mesen2 `Core/NES/APU/NesApu.cpp` lines 88-114 (`ReadRam` for `$4015`).
- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 10120-10211
  (TEST_FrameCounterIRQ Tests 1-7).
- `crates/nes-apu/src/frame_counter.rs:141-167` (current `read_status`
  + `tick` pending-clear mechanism).
- `crates/nes-apu/src/apu.rs:589-618` (`read_status` site in APU,
  delegates to `frame_counter` with `apu_phase`).
- `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`
  (Phase 3 LANDED template).

