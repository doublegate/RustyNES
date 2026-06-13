# Sprint 2.3 (Implied Dummy Reads + DMC Coordinated) — Recon

**Date:** 2026-05-25 (post-v1.1.0; after Sprint 2.1 + 2.5 closures + Sprint 2.4 rollback)
**Sprint scope (per v2.0.0 plan):** Close
`CPU Behavior 2 :: Implied Dummy Reads [error 3]` by adding the
canonical cycle-2 PC dummy read to all implied / accumulator /
transfer / flag instructions (21 opcodes), AND simultaneously
fix the DMC DMA scheduler's "DMA halts on any bus-active CPU
cycle" model so the change doesn't regress
`APU Registers and DMA tests :: Implicit DMA Abort [error 2]`.
**Outcome of this session:** Reconnaissance + plan only. The
coordinated fix is multi-day work; this audit documents the
precise change shape so the next session can land it cleanly.

---

## The cascade Session-19 documented

Per `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md`,
Attempt 1 of Session-19 added `implied_dummy_read(bus) = read1(bus, self.pc)`
to 21 opcodes:

* `ASL/LSR/ROL/ROR A` (`0x0A / 0x4A / 0x2A / 0x6A`)
* `CLC/SEC/CLI/SEI/CLV/CLD/SED`
  (`0x18 / 0x38 / 0x58 / 0x78 / 0xB8 / 0xD8 / 0xF8`)
* `TAX/TAY/TSX/TXA/TXS/TYA` (`0xAA / 0xA8 / 0xBA / 0x8A / 0x9A / 0x98`)
* `INX/DEX/INY/DEY` (`0xE8 / 0xCA / 0xC8 / 0x88`)
* `NOP` (`0xEA`) + 6 unofficial 1-byte NOPs (`0x1A / 0x3A / 0x5A / 0x7A / 0xDA / 0xFA`)

Result:
- **Did NOT flip the target** `Implied Dummy Reads [error 3]`
  (stayed at error 3).
- **BROKE** `APU Registers and DMA tests :: Implicit DMA Abort [error 2]`
  (was strict-pass).

Session-19's attempt 1 was **reverted** as a cascade regression.

## Why the simple fix cascades

The spec is correct per nesdev `6502_cpu.txt` + MOS 6502
datasheet: implied/accumulator/transfer/flag opcodes do a
canonical **cycle-2 PC dummy read** (the byte after the opcode,
which the CPU is decoding but doesn't use). PHA/PHP/PLA/PLP also
have this cycle-2 PC dummy read.

The bug: our existing DMC DMA scheduler in
`bus.rs::service_dmc_dma` was tuned to halt on cycles it
*assumed* were bus-active (typically cycle 2 of multi-cycle
instructions). Adding cycle-2 dummy reads to implied opcodes
changes which cycle is "the read cycle" for those instructions —
but the scheduler still assumes the OLD pattern. The DMC scheduler
now mis-detects halt boundaries, mis-timing the `Implicit DMA Abort`
sentinel's get/put cycle expectations.

Per the v2.0.0 plan Sprint 2.3:

> restructure DMC DMA halt-cycle logic to model "DMA halts on any
> bus-active CPU cycle" rather than implicit cycle-2-quiet
> assumption. Cross-reference Mesen2's `Core/NES/NesApu.cpp` DMC
> fetch path.

## Current state of the relevant code

### CPU side (`crates/nes-cpu/src/cpu.rs`)

Each of the 21 opcodes is a 3-5 line block that does ALU /
flag / register-transfer work and sets `*cycles = 2`. None
issue a `read1(bus, self.pc)` dummy read. Example
(`CLC`, opcode `0x18`):

```rust
0x18 => {
    self.p.remove(Status::CARRY);
    *cycles = 2;
}
```

The 4 stack opcodes (`PHA/PHP/PLA/PLP`, `0x48/0x08/0x68/0x28`) and
`BRK` (`0x00`) / `JSR` (`0x20`) / `RTI` (`0x40`) / `RTS` (`0x60`)
already have nuanced bus-access patterns and need separate audit.

### Bus side (`crates/nes-core/src/bus.rs`)

`service_dmc_dma(halted_addr)` and `service_dmc_dma_during_oam(...)`
both compute `noop_cycles = if dmc_dma_short { 2 } else { 3 }`
and run `noop_cycles` halt cycles before the actual read. The
`dmc_dma_short` flag distinguishes the alignment-dependent halt
duration.

The `Implicit DMA Abort` test brackets the **post-halt-cycle DMA
abort** behavior: if rendering / write / specific bus access
happens at the halt-cycle boundary, the DMA aborts rather than
proceeding. Our current implementation is tuned to the OLD bus-
access pattern; the cycle-2-dummy-read insertion shifts which
cycle the abort would fire on.

## The coordinated-fix recipe

### Step 1 — Add `Cpu::implied_dummy_read` helper

```rust
/// Canonical cycle-2 PC dummy read for implied / accumulator /
/// transfer / flag instructions. The 6502 fetches the byte after
/// the opcode (the would-be operand) but discards it.
fn implied_dummy_read<B: Bus>(&mut self, bus: &mut B) {
    let _ = self.read1(bus, self.pc);
}
```

### Step 2 — Wire into all 21 opcodes

Each opcode arm gains an `self.implied_dummy_read(bus);` call
BEFORE the ALU / flag / transfer work:

```rust
0x18 => {  // CLC
    self.implied_dummy_read(bus);
    self.p.remove(Status::CARRY);
    *cycles = 2;
}
```

The `read1(bus, self.pc)` issues a bus access (which the DMC
scheduler now sees as a bus-active cycle 2 — matching real silicon).

### Step 3 — Audit DMC scheduler for bus-active recognition

The DMC scheduler currently runs `noop_cycles` halt cycles
THEN issues the actual DMC read. If a cycle that the OLD
scheduler thought was bus-quiet (i.e., a cycle 2 of an
implied opcode) is now bus-active (because of the new dummy
read), the scheduler may:
- (a) Insert too-few halt cycles (under-count)
- (b) Insert too-many halt cycles (over-count)
- (c) Mis-detect the abort condition

Per the v2.0.0 plan: "restructure DMC DMA halt-cycle logic to
model 'DMA halts on any bus-active CPU cycle' rather than
implicit cycle-2-quiet assumption."

The fix shape: rather than precomputing `noop_cycles = 2 or 3`,
the scheduler should peek at the CPU's next bus-access status
each cycle and halt accordingly. This requires either:
- (a) The CPU exposes a per-cycle `bus_active: bool` flag that
  the bus inspects during `tick_one_cpu_cycle`
- (b) The scheduler tracks "since last bus-active cycle" and
  re-evaluates at each cycle boundary

(b) is the simpler refactor. (a) is the cleaner model.

### Step 4 — Per-iteration sentinel

`APU Registers and DMA tests :: Implicit DMA Abort [error 2]`
must stay strict-pass after every sub-step. Sessions 19+ have a
documented `Implicit DMA Abort` regression detection convention;
this audit's recommended workflow:

1. Land Step 1 + Step 2 under a cargo feature flag
   `cpu-implied-dummy-reads` (default off).
2. With the feature enabled, run the gauntlet:
   - If `Implied Dummy Reads` flips PASS AND `Implicit DMA Abort`
     stays PASS → flip default to on, ship.
   - If `Implicit DMA Abort` regresses → don't ship; proceed to
     Step 3 + 4 under the feature.
3. After Step 3 lands, re-run the gauntlet. Iterate until both
   tests pass simultaneously.
4. Flip the feature flag default to ON.
5. Remove the feature flag (production-active permanently).

This pattern matches the established "feature flag → unit-test
reproducer → land or roll back" gauntlet methodology proven
across Cascades A/B, Phase B4, and Sessions 13-18.

## Cross-references

- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md`
  (the original cascade-revert)
- Mesen2 `Core/NES/NesApu.cpp` DMC fetch path
- `crates/nes-cpu/src/cpu.rs` opcode dispatch (lines 1683-1750 for
  the implied/flag/transfer block)
- `crates/nes-core/src/bus.rs::service_dmc_dma` (line 1390)
- `crates/nes-core/src/bus.rs::service_dmc_dma_during_oam`
  (line 1440)
- `crates/nes-test-harness/src/bin/trace_apu_reg_activation.rs`
  (related trace tooling that may help cross-validate)

## Sprint 2.3 status

**Recon complete. Coordinated fix is multi-day work — recommended
for a focused next-session attempt.**

Expected outcome on success:
- `Implied Dummy Reads [error 3]` flips PASS (+1 AccuracyCoin test)
- `Implicit DMA Abort [error 2]` preserved PASS (sentinel)
- AccuracyCoin: 90.65% → 91.37% (+1 of 139)

Acceptance gates: standard 10-gate gauntlet + AccuracyCoin
per-suite verification + sacred trio bisect.

## v1.2.0 takeaway

Given v1.2.0's three completed/closed sprints (2.1 ✓, 2.4 ✗ rolled
back, 2.5 ✓), the realistic v1.2.0 deliverable is now:

| Sprint | Status | Tests gained |
|---|---|---:|
| 2.1 sprite-eval | CLOSED (audit) | +0 |
| 2.2 PPU misc | NOT YET ATTACKED (EXTREME cascade, re-baseline gate) | TBD |
| 2.3 Implied Dummy Reads + DMC | THIS RECON | +1 target |
| 2.4 APU edge cases | iter 1 ROLLED BACK; v2.0 work | +0 |
| 2.5 commercial ROMs | CLOSED (audit) | +0 |

If Sprint 2.3 lands cleanly: **AccuracyCoin 90.65% → 91.37%
(+1 test). v1.2.0 effective scope is Sprint 2.3 + (optionally)
Sprint 2.2 if user authorizes Cascade-A re-baseline.**
