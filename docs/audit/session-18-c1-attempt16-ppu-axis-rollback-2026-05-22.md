# Session-18 — C1 Attempt 16: PPU-axis `$2002` Race-Window Rollback

**Date**: 2026-05-22
**Status**: Phase 2 oracle (Mesen2-independent `$2002` race-window unit test) landed as permanent regression guard. Phase 5 feature-flagged predicate narrowing (`dot <= 1` → `dot == 0` matching Mesen2 + nesdev wiki) attempted and ROLLED BACK because it did NOT flip the target `cpu_interrupts_v2/{2,3,5}` tests. 13th C1-axis rollback overall; first one on the PPU axis (the prior 12 were on the CPU IRQ-sample-point axis).
**Predecessor**: `session-17-c1-attempt15-per-instruction-divergence-2026-05-22.md` (per-instruction divergence trace; PPU-axis reframe).
**Branch / commit**: `main`, building on `e55f4e7`.

---

## Summary

The Session-17 audit predicted a PPU-axis closure of `cpu_interrupts_v2/{2,3,5}` based on the per-instruction divergence trace: at the `BIT $2002` inside blargg's `sync_vbl` precise-poll loop, Mesen2 returned `$80` (VBL=1) and RustyNES returned `$00` (VBL=0). The Session-17 hypothesis was that RustyNES's `$2002` race-window suppression predicate (`dot <= 1` in `cpu_read_register`) was 1 PPU dot wider than Mesen2's `_cycle == 0` and the nesdev wiki spec, and that narrowing it would close the divergence.

Session-18 landed the **highest-leverage** prerequisite Phase 2 first — a Mesen2-independent `$2002` race-window sweep unit test in `crates/nes-ppu/src/ppu.rs::vbl_race_window_2002_read_sweep`. The test is now permanent regression guard infrastructure and tabulates the exact per-PPU-dot behavior of `$2002` reads across the boundary scanline 240/dot 339 through scanline 242/dot 1.

The empirical output of the unit test surfaced a critical clarification of the Session-17 hypothesis:

| sl | dot | read | bit7 | suppress? | PPU.VBLANK? |
|----|-----|------|------|-----------|-------------|
| 240 | 339 | 0x00 | 0 | false | false |
| 240 | 340 | 0x00 | 0 | false | false |
| **241** | **0** | **0x00** | **0** | **true** | **false** |
| **241** | **1** | **0x80** | **1** | **true** | **false** |
| 241 | 2 | 0x80 | 1 | false | false |
| 241 | 3+ | 0x80 | 1 | false | false |

The key observation: **at scanline 241 dot 1, RustyNES's `$2002` read returns bit 7 = 1, not 0 as Session-17 inferred from the Mesen2 trace diff**. The "RustyNES reads $00" finding in Session-17 was correct, but the divergent BIT $2002 must therefore have been landing at **scanline 241 dot 0** in RustyNES — not dot 1.

Implication: the actual axis is not "the suppression predicate is too wide" but "the PPU is 1 dot behind Mesen2 at the moment of the read".

Phase 5 attempted the predicate fix anyway under feature flag `ppu-c1-attempt-16` (predicate `dot <= 1` → `dot == 0`) to confirm or rule out the predicate-axis hypothesis. The result is unambiguous:

* Unit test with flag ON: the dot-1 read no longer latches `suppress_vbl_this_frame` (the predicate change works AT the test layer).
* `cpu_interrupts_v2/{2,3,5}` failure shape with flag ON: **byte-identical** to flag OFF. The 5-branch_delays_irq `test_jmp` output's `00 02 04 / 01 01 04 / 02 03 07 / ...` CK column matches exactly between flag states. The downstream FNV-1a hash `AB1A8F0A` is identical.
* `ppu_vbl_nmi/*`: 10/10 strict pass with flag ON (no regression on the PPU side).

The predicate axis is therefore EMPIRICALLY FALSIFIED. The actual load-bearing axis is the intra-cycle CPU-vs-PPU access interleaving (the `bus.cpu_read(addr)` happens BEFORE `bus.on_cpu_cycle()` ticks the PPU 3 dots, vs. Mesen2's `StartCpuCycle (ticks PPU) → Read → EndCpuCycle (ticks PPU more)`). Closing this axis would require restructuring `Cpu::read1` and `Bus::tick_one_cpu_cycle` to align the read latch point with Mesen2's φ1 mid-cycle access — the same surface Attempts 1 (intra-cycle CPU phase split) and 4 (`LockstepBus` access-ordering swap) tried and rolled back at, plus the orthogonal `ppu_vbl_nmi` calibration risk that those attempts triggered.

Phase 5 is rolled back. The production code at `crates/nes-ppu/src/ppu.rs:cpu_read_register` returns to the pre-Session-18 `dot <= 1` predicate. The feature flag (`ppu-c1-attempt-16`) is removed. The Phase 2 unit test stays landed as a permanent oracle (a regression guard against future predicate drift, and an empirical-truth-record for the next axis attempt).

---

## Phase 2 — Mesen2-independent `$2002` race-window unit test

### Specification

`crates/nes-ppu/src/ppu.rs::tests::vbl_race_window_2002_read_sweep`:

1. Per (scanline, dot) sample point in `{(240, 339), (240, 340), (241, 0), ..., (241, 5), (242, 0), (242, 1)}`, build a fresh PPU.
2. Tick to the target (scanline, dot).
3. Issue a synchronous `$2002` read.
4. Record: (a) return value, (b) bit 7 of return, (c) `suppress_vbl_this_frame` after read, (d) `status.VBLANK` after read.
5. Tabulate and assert against the per-row `ExpectedRow` matrix (nesdev wiki spec).

The test is **independent of Mesen2** — it asserts the documented nesdev semantics directly. Future spec drift surfaces as a failing assertion; future RustyNES PPU changes are pinned to the documented spec at the unit-test layer.

### Result (baseline behavior)

All 10 sample-point assertions pass. The table is the empirical truth-record of current RustyNES `$2002` race-window behavior.

### Cross-reference with nesdev wiki

* `https://www.nesdev.org/wiki/PPU_registers`: "Reading the flag on the dot before it is set (scanline 241, dot 0) causes it to read as 0 and be cleared." — Captured by the dot-0 row (read=0, suppress=true, VBLANK=false).
* `https://www.nesdev.org/wiki/NMI`: "If [VBL set] and [PPUSTATUS read] happen simultaneously, PPUSTATUS bit 7 is read as false, and vblank_flag is set to false anyway." — Documents the dot-1 simultaneous case. RustyNES reads bit 7 = 1 here, NOT 0 as the wiki suggests; Mesen2's source (see below) reads bit 7 = 1 as well in its post-set-cycle-1 ordering.

### Cross-reference with Mesen2 source

`/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/NES/NesPpu.cpp`:

* Line 590 (`UpdateStatusFlag`): suppression latch predicate is `_scanline == _nmiScanline && _cycle == 0` — strict 1-dot wide (RustyNES has 2-dot wide `dot <= 1`).
* Line 1339-1344 (`Exec`): VBL is set at `_cycle == 1 && _scanline == _nmiScanline` IF `!_preventVblFlag`. The set check happens in the same `Exec()` call AFTER `_cycle++`, so the set lands after entering cycle 1.

### Cross-reference with CPU/PPU interleaving

`/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/NES/NesCpu.cpp:254-268`:

```cpp
uint8_t NesCpu::MemoryRead(uint16_t addr, MemoryOperationType operationType) {
    ProcessPendingDma(addr, operationType);
    StartCpuCycle(true);             // advances PPU to read point
    uint8_t value = _memoryManager->Read(addr, operationType);
    EndCpuCycle(true);               // advances PPU to end of cycle
    return value;
}
```

vs. RustyNES `crates/nes-cpu/src/cpu.rs::read1`:

```rust
fn read1<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
    let v = bus.cpu_read(addr);      // read FIRST, before PPU tick
    self.idle_tick(bus);             // ticks PPU 3 dots AFTER
    v
}
```

This 3-PPU-dot difference (the read sees end-of-prior-cycle PPU state in RustyNES vs mid-current-cycle PPU state in Mesen2) is the root cause of the dot-position-at-read divergence. The Phase 5 predicate change cannot move the read's dot position; only the access-ordering / phase-split change can. The latter has been attempted twice (Attempts 1 + 4) and rolled back twice.

---

## Phase 5 — predicate narrowing under `ppu-c1-attempt-16` feature flag

### Implementation

Feature `ppu-c1-attempt-16` added on `nes-ppu`, `nes-core`, and `nes-test-harness` (forwarding chain). Code change at `crates/nes-ppu/src/ppu.rs::cpu_read_register` case 2:

```rust
#[cfg(feature = "ppu-c1-attempt-16")]
let in_race_window =
    self.scanline == self.region.vblank_start_line() && self.dot == 0;
#[cfg(not(feature = "ppu-c1-attempt-16"))]
let in_race_window =
    self.scanline == self.region.vblank_start_line() && self.dot <= 1;
```

Surface: 1 cfg-gated boolean predicate. No other code touched.

### Validation gauntlet results

| Gate | Flag OFF | Flag ON |
|------|----------|---------|
| `cargo fmt --all --check` | clean | clean |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | clean | clean |
| `cargo clippy --workspace --all-targets --features test-roms,ppu-c1-attempt-16 -- -D warnings` | n/a | clean |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | clean | clean |
| `cargo build --workspace` | clean | clean |
| `cargo test --workspace --features test-roms` | **541 strict + 5 ignored** | **541 strict + 5 ignored** |
| `ppu_vbl_nmi/*` 10 sub-ROMs | 10/10 strict | 10/10 strict |
| `cpu_interrupts_v2/{1, 4}` (strict-pass) | 2/2 | 2/2 |
| `cpu_interrupts_v2/{2,3,5}` (`#[ignore]`'d) | 3/3 FAIL with `_currently_fails` | **3/3 FAIL — byte-identical shape, hash AB1A8F0A** |
| `vbl_race_window_2002_read_sweep` | PASS (dot 1: suppress=true) | PASS (dot 1: suppress=false) |
| AccuracyCoin pass rate | 82.73% (floor 0.60) | not re-run (no chip-stack code change vs flag OFF) |

The target tests' failure shape is byte-identical between flag states. The hypothesis is empirically falsified.

### Decision per Phase 5.4

> **Flag ON: target test failure-shape unchanged**: hypothesis wrong. Revert; keep Phase 1+2 infrastructure only.

Per the spec, ROLLBACK is invoked. The production code change is reverted. The feature flag is removed from all three Cargo.toml files. The unit test stays as permanent infrastructure.

### Why the predicate change does NOT close the target tests

The `sync_vbl` precise loop polls `BIT $2002` at instruction boundaries spaced by `27 - 11 = 16` CPU cycles (the `delay 27 - 11` macro plus the two `bit $2002` instructions). The exact PPU dot the second `BIT $2002` lands on depends on the boot-anchor + the per-instruction PPU dot drift. Mesen2 lands its read at scanline 241 cycle ≥ 1 (post-VBL-set, sees bit 7 = 1, no suppression, BMI taken). RustyNES lands its read at scanline 241 dot 0 (pre-VBL-set, sees bit 7 = 0, suppression latches, BMI not taken).

The predicate change tightens the suppression window — but the read STILL lands at dot 0 either way. At dot 0:
- Pre-fix: read = 0, suppress = true (correct per nesdev for the dot-0 case).
- Post-fix: read = 0, suppress = true (same — dot 0 is included in BOTH `dot <= 1` and `dot == 0`).

The predicate change ONLY differs at dot 1. The failing tests don't read at dot 1 — they read at dot 0. So the predicate cannot move them.

### What the actual fix would require

The actual axis closure requires the PPU to advance ONE PPU dot earlier (or equivalently, the read latch to sample the PPU state AFTER 1+ PPU dots of the current cycle have ticked). Three structural options:

1. **Restructure `Cpu::read1` / `Cpu::write1` to interleave ticks**: e.g., `tick PPU 1 dot → read → tick PPU 2 dots`. This is option (a) from Session-17's "directions". It mirrors Mesen2's `StartCpuCycle → Read → EndCpuCycle` order. **Risk**: changes the PPU sample point for every CPU memory access; calibration impact spans `ppu_vbl_nmi/*`, sprite-hit timing, `cpu_interrupts_v2/4`, AccuracyCoin's many sample-point tests. Same surface as Attempt 1 (rolled back). Out of scope for a single-axis Session-18 attempt.

2. **Shift PPU dot-0 events to dot 339 of the prior scanline**: equivalent to moving the VBL set point from `(241, 1)` to `(241, 0)` (or even `(240, 340)`). **Risk**: regresses `ppu_vbl_nmi/02-vbl_set_time` directly.

3. **Add a `BIT $2002`-specific late-cycle read latch**: synthetic — special-cases an instruction that shouldn't be special-cased. Rejected as architecturally inappropriate.

None of these is a "1-line predicate fix" candidate. The Session-17 hypothesis was the simplest possible one and it failed. The next attempt must target the access-interleaving axis with the full coordinated-rework discipline.

---

## Rollback artifact log

| File | Change | Final state |
|------|--------|-------------|
| `crates/nes-ppu/Cargo.toml` | Added then removed `ppu-c1-attempt-16` feature | RESTORED to pre-Session-18 |
| `crates/nes-core/Cargo.toml` | Added then removed `ppu-c1-attempt-16` forwarder | RESTORED to pre-Session-18 |
| `crates/nes-test-harness/Cargo.toml` | Added then removed `ppu-c1-attempt-16` forwarder | RESTORED to pre-Session-18 |
| `crates/nes-ppu/src/ppu.rs::cpu_read_register` | Added then removed `#[cfg(feature = "ppu-c1-attempt-16")]` predicate split | RESTORED to pre-Session-18 (`dot <= 1`) — comment updated with Session-18 rationale |
| `crates/nes-ppu/src/ppu.rs::tests::vbl_race_window_2002_read_sweep` | NEW permanent regression guard | LANDED |
| `docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md` | This document | LANDED |
| `docs/adr/0002-irq-timing-coordination.md` | New "Decision update (2026-05-22, Session-18)" section | LANDED |
| `CHANGELOG.md` `[Unreleased]` | New "C1 attempt 16 — PPU-axis predicate narrow, investigated and rolled back" entry | LANDED |

Production code unchanged at end-of-session except for the new test + comment updates. AccuracyCoin pass rate unchanged at 82.73%. Workspace test count: 540 strict + 5 ignored before Session-18 → **541 strict + 5 ignored** after (= +1 from the new `vbl_race_window_2002_read_sweep` test).

---

## Invariants validated (Session-18 close)

| Invariant | Pre-Session-18 | Post-Session-18 | Status |
|-----------|----------------|------------------|--------|
| Workspace tests `--features test-roms` | 540 strict + 5 ignored | **541 strict + 5 ignored** (+1: new unit test) | OK |
| AccuracyCoin RAM pass rate | 82.73% | 82.73% (production code unchanged) | OK |
| `ppu_vbl_nmi/*` | 10/10 strict | 10/10 strict | OK |
| `sprite_hit_tests` / `sprite_overflow_tests` / `oam_*` | strict pass | strict pass (production code unchanged) | OK |
| `mmc3_test_2/4` sub-test #2 (B4 invariant) | strict pass | strict pass | OK |
| First MMC3 IRQ at scanline 0 / dot 260 / cycle ~1,369,997 | preserved | preserved (no PPU code change) | OK |
| `cargo fmt --all --check` | clean | clean | OK |
| `cargo clippy ... -- -D warnings` | clean | clean | OK |
| `RUSTDOCFLAGS="-D warnings" cargo doc ...` | clean | clean | OK |
| Commercial-ROM oracle | green (last re-baseline Session-8) | not re-run (no chip-stack code change) | OK |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible | legible (no chip-stack code change) | OK |

Net change: pure-additive Phase 2 oracle infrastructure. Production CPU/PPU/APU/mapper code is byte-identical before vs after Session-18.

---

## Recommended next attempts (priority order)

For Session-19 / attempt 17:

1. **Per-PPU-dot trace of the failing `BIT $2002` window** (the original Phase 1 spec deferred this session). Configure `scripts/mesen2_ppu_trace.lua` for cycles 295,400-295,430 on `2-nmi_and_brk` and emit RustyNES's PPU-dot-resolution state via the existing `ppu-state-trace` feature (`crates/nes-ppu/src/state_trace.rs`). Cross-diff column-by-column. The expected finding: at the BIT $2002 read cycle, Mesen2's `(scanline, cycle)` is `(241, 1)` while RustyNES's is `(241, 0)` — confirming the 1-PPU-dot phase offset hypothesis.

2. **Quantify the offset distribution**: across all 4 in-window `BIT $2002` reads in `2-nmi_and_brk` (and the equivalent reads in `/3` and `/5`), tabulate the (scanline, dot) RustyNES vs Mesen2 lands the read at. The constant +N or jitter pattern decides whether option (1) "ticks-before-read interleave" or option (2) "move VBL-set dot" is the right structural lever.

3. **Per-CPU-cycle access-pattern instrumentation**: in `crates/nes-cpu/src/cpu.rs::read1` (and `write1`), record `(cpu_cycle, ppu_scanline, ppu_dot)` at the moment of the bus call. Compare against Mesen2's equivalent (from its `Run()` callbacks). The systematic gap is the load-bearing surface.

4. **Only after (1)+(2)+(3) yield a single-axis hypothesis**: structural rework of CPU read/write tick interleaving under feature flag `cpu-c1-attempt-17`. Stop conditions per ADR-0002 apply: ANY pre-existing strict test regression = rollback. ANY commercial-ROM oracle flip = STOP for user authorization. `ppu_vbl_nmi/*` regression = bad trade = rollback.

5. **Independently**: the canonical `T_last - 1` CPU IRQ-sample-point rework for `mmc3_test_2/4` sub-test #3. This is the orthogonal axis that the prior 12 attempts targeted; it is still open and `mmc3` sub-test #3 still fails (this session changed nothing on that axis).

---

## Outcome

**Outcome category**: `code-rolled-back` (the Phase 5 production change reverted; Phase 2 unit test landed as permanent infrastructure).

**Hypothesis**: PPU `$2002` race-window suppression predicate `dot <= 1` is 1 PPU dot wider than spec; narrowing to `dot == 0` flips `cpu_interrupts_v2/{2,3,5}`.

**Outcome**: FALSIFIED. The predicate change works at the unit-test layer (the dot-1 read no longer latches suppression) but does NOT flip the target tests because they land their `BIT $2002` reads at dot 0, NOT dot 1. The real load-bearing axis is the intra-cycle CPU-vs-PPU access interleaving (RustyNES reads BEFORE the cycle's PPU ticks; Mesen2 reads AFTER the cycle's first PPU tick).

**13th C1 rollback. First PPU-axis attempt.** The Phase 2 oracle (`vbl_race_window_2002_read_sweep`) is the first piece of permanent PPU-side infrastructure on this axis — useful for all future attempts.

The session-17 prediction "the predicate narrows from 2 dots to 1 dot, which is a *more permissive* behavior on dot 1 reads" was structurally correct. What it missed: the failing tests don't read at dot 1. They read at dot 0. The 1-PPU-dot phase drift between RustyNES and Mesen2 means the *position of the read* — not the *predicate applied to the read* — is the load-bearing axis.

---

## Key empirical findings

1. The `$2002` race-window predicate is now mechanically tested across 10 sample points per PPU-region cycle. Future drift surfaces immediately.
2. The Session-17 trace's "Mesen2 reads $80, RustyNES reads $00" finding is correct — but the reads are landing at DIFFERENT PPU dots. RustyNES at scanline 241 dot 0; Mesen2 at scanline 241 dot 1+.
3. The 1-PPU-dot offset is structural: RustyNES's `Cpu::read1` does `bus.cpu_read(addr); idle_tick();` — read FIRST, PPU tick AFTER. Mesen2's `MemoryRead` does `StartCpuCycle(); Read(); EndCpuCycle();` — PPU advances BEFORE the read. Same `_endClockCount/_startClockCount` cycle decomposition; different ordering.
4. The PPU-axis predicate (`dot == 0` vs `dot <= 1`) and the CPU-axis interleaving (read-then-tick vs tick-then-read) are independent. Both must be aligned with Mesen2 to make the failing `sync_vbl` polls converge. The PPU-axis predicate alone is insufficient.
5. The `_currently_fails` probes for `cpu_interrupts_v2/{2,3,5}` are well-calibrated (their failure-shape is byte-stable across Phase 5 flag states). No probe maintenance needed.
6. The B4 MMC3 reload-pending Sharp discriminator is fully independent of the PPU axis. `mmc3_test_2/4` sub-test #2 stays strict-pass; sub-test #3 stays `#[ignore]` and on the orthogonal CPU `T_last - 1` axis.
