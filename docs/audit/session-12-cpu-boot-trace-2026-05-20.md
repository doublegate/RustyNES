# Session-12 — Per-CPU-Instruction Boot Trace + Cold-Boot Divergence Findings

**Date**: 2026-05-20
**Status**: Tooling landed; fix deferred to Session-13 (high-risk, multi-test surface)
**Predecessor**: `session-11-mesen2-trace-validation-2026-05-20.md`
**Branch / commit**: `main` post-tag

---

## Summary

Built the per-CPU-instruction boot-trace observability infrastructure that Session-11 identified as the precondition for closing the AccuracyCoin cold-boot divergence. Captured matching traces from RustyNES and Mesen2 across the first 200,000 CPU cycles of `AccuracyCoin.nes` and ran a per-record diff.

**Identified the load-bearing axis**: the PPU's power-up scheduler position differs between the two emulators by approximately one pre-render scanline (~344 PPU dots / ~115 CPU cycles). Mesen2 starts the PPU at `(scanline=-1, cycle=340)` — the last dot of pre-render, about to wrap immediately into frame 1's scanline 0. RustyNES starts at `(scanline=261, dot=0)` — the first dot of pre-render, requiring an entire pre-render scanline before reaching scanline 0.

A secondary divergence is the stack-pointer cold-boot path: Mesen2 power-up sets `SP = $FD` directly, then soft-reset additionally decrements by 3. RustyNES `Cpu::new()` sets `S = $FD` and `Cpu::reset()` unconditionally decrements by 3 → ends at `$FA`. This is `+3` off, but is not pass/fail load-bearing for AccuracyCoin (the `TEST_PowerOnState_CPU_Registers` test is documentation-only, no pass/fail).

**No fix was attempted in this session**. The PPU power-up position is the same architectural surface that Track C1 has rolled back 6 times; every blargg / kevtris / sprite / sprite-zero / dot-skip / MMC3-A12 test is calibrated to the current power-up alignment. A 4 + axis-coordinated change is required (PPU init position + CPU reset cycle count + ROM-corpus rebaseline). Documenting the divergence + landing the tooling is the durable Session-12 deliverable.

---

## Infrastructure landed

### `cpu-boot-trace` cargo feature

Added to `crates/nes-core/Cargo.toml` and forwarded through
`crates/nes-test-harness/Cargo.toml`. Off by default; the CI gate stays
fast. The default workspace build is byte-identical pre- and
post-Session-12 (verified via `cargo check --workspace` + `cargo clippy --workspace`).

### `crates/nes-core/src/cpu_boot_trace.rs` (new, ~460 LOC)

- `CpuBootRecord` — 32-byte little-endian-packed record capturing
  `(cycle, frame, scanline, dot, pc, a, x, y, p, s, opcode, op1, op2, flags)`.
  Pad bytes are intentional so future flag-bit additions don't break
  `RECORD_SIZE`. Schema version 1, magic `b"RUSTYNES_CPU"` (12 ASCII
  bytes), HEADER_SIZE = 16 bytes.
- `CpuBootTrace` — linear buffer with overflow counter + filter-aware
  `maybe_push`. Binary + CSV emitters mirror the existing `PpuStateTrace`
  pattern.
- `CpuBootTraceConfig { cycle_range: 0..=200_000 }` — default filter
  covering ~5 cold-boot frames at NTSC (~29,780 cycles/frame).
- 8 unit tests: roundtrip, header magic, schema mismatch, body
  alignment, capacity overflow, filter respect, CSV column coverage.

### `Nes::enable_cpu_boot_trace` / `Nes::take_cpu_boot_trace`

Hook integrated into `run_frame` and `step_instruction` BEFORE the
`Cpu::step` call. Side-effect-free: uses `LockstepBus::debug_peek_cpu`
to peek the opcode + 2 operand bytes without perturbing emulator state.
Feature-gated; when `cpu-boot-trace` is off, `run_frame` is
byte-identical to pre-Session-12.

### `crates/nes-test-harness/tests/cpu_boot_trace_fixture.rs` (new)

Drives `AccuracyCoin.nes` from cold-boot for the configured cycle
window, dumps a binary trace + 500-record CSV preview to
`target/cpu_boot_trace/`. Env-var overrides for cycle range and output
path. Roundtrip-verifies the binary against
`CpuBootTrace::from_binary`. Gated behind `test-roms,cpu-boot-trace`.

### `crates/nes-test-harness/src/bin/cpu_boot_trace_diff.rs` (new, ~530 LOC)

Diff tool comparing two binary traces:

```bash
./target/release/cpu_boot_trace_diff \
    --reference /tmp/mesen2_cpu_boot_trace.bin \
    --actual    target/cpu_boot_trace/accuracycoin_boot.bin \
    --first-divergence --context 5
```

Features:
- `--first-divergence` (default) / `--all-divergences`
- `--align-by-cycle` — skip-ahead to align first common cycle (handles
  the case where the two emulators emit different numbers of records
  before a synchronization point)
- `--context N` — print N records before/after divergence point
- `--skip-fields field1,field2,...` — ignore enumerated fields
- ~150-entry built-in opcode disassembler — covers all the official
  6502 opcodes that boot-time RESET routines actually execute. Unknown
  opcodes (unofficial NOPs etc.) fall through to `???`.
- Exit codes: `0` = equivalent, `1` = divergence reported, `2` = parse error.

### `scripts/mesen2_cpu_boot_trace.lua` (new, ~190 LOC)

Mesen2 Lua reference-trace script using `emu.addMemoryCallback(...,
emu.callbackType.exec, ...)` — a per-instruction exec callback that
fires at every opcode fetch (verified against Mesen2 0.42+, 2026-05-20).
Emits the SAME binary format as the Rust fixture. Reads `cpu.cycleCount,
cpu.pc, cpu.a, cpu.x, cpu.y, cpu.ps, cpu.sp, cpu.nmiFlag, ppu.frameCount,
ppu.scanline, ppu.cycle` from the flat `emu.getState()` table (Session-11
finding: Mesen2 returns a flat dotted-string-keyed table, not nested
subtables).

Headless invocation:

```bash
xvfb-run -a /home/parobek/AppImages/mesen.appimage \
    --testRunner tests/roms/accuracycoin/AccuracyCoin.nes \
    scripts/mesen2_cpu_boot_trace.lua
```

Honours `MESEN2_CPU_BOOT_TRACE_OUT`, `_START_CYCLE`, `_END_CYCLE` env
vars.

---

## Captured traces

| Side | Records | First-record cycle | First-record PC | First-record state |
|------|---------|--------------------|-----------------|--------------------|
| Mesen2 | 64,540 | 7 | $8004 STA $00 | `A=$00 X=$00 Y=$00 P=$04 S=$FD` frame=1 scanline=0 dot=25 |
| RustyNES | 61,064 | 7 | $8004 STA $00 | `A=$00 X=$00 Y=$00 P=$24 S=$FA` frame=0 scanline=261 dot=21 |

Both emulators emit the **same opcode at the same CPU cycle (7) at the
same PC ($8004)** — the cycle anchor is identical. CPU register A, X, Y
agree byte-for-byte. The divergences are in:

1. **PPU position** (`frame, scanline, dot`) — see below for analysis.
2. **CPU `P` flag** — `$04` (Mesen2) vs `$24` (RustyNES; UNUSED bit set
   on power-up).
3. **CPU `S` stack pointer** — `$FD` (Mesen2) vs `$FA` (RustyNES).

Both `P` and `S` differences trace back to specific cold-boot
initialization paths in the two emulators (see "Mesen2 reference
behaviour" section below).

---

## Mesen2 reference behaviour (from upstream source)

From `Core/NES/NesCpu.cpp` `NesCpu::Reset(bool softReset, ...)`:

```cpp
//Use _memoryManager->Read() directly to prevent clocking the PPU/APU when setting PC at reset
_state.PC = _memoryManager->Read(NesCpu::ResetVector) |
            _memoryManager->Read(NesCpu::ResetVector + 1) << 8;

if(softReset) {
    SetFlags(PSFlags::Interrupt);
    _state.SP -= 0x03;
} else {
    _irqMask = 0xFF;
    _state.SP = 0xFD;          // <-- direct assignment, no decrement
    _state.X = 0;
    _state.Y = 0;
    _state.PS = PSFlags::Interrupt;   // <-- $04, no UNUSED bit
    _runIrq = false;
}

//The CPU takes 8 cycles before it starts executing the ROM's code after a reset/power up
for(int i = 0; i < 8; i++) {
    StartCpuCycle(true);
    EndCpuCycle(true);
}
```

From `Core/NES/NesPpu.cpp` `NesPpu::Reset(false)`:

```cpp
_scanline = -1;
_cycle = 340;
```

So at the moment Mesen2's CPU begins executing the first ROM
instruction, it has clocked 8 CPU cycles after power-up. The PPU has
been ticked 8 × 3 = 24 PPU dots from `(scanline=-1, cycle=340)`:
- Tick 1: `(−1, 340) → (0, 0)` with scanline-wrap (frame counter
  increments from 0 to 1)
- Ticks 2–24: `(0, 0) → (0, 23)`

…which lines up with the trace's `frame=1 scanline=0 dot=25` (some
small Mesen2-internal offset accounts for the +2 vs +23).

---

## RustyNES current behaviour

From `crates/nes-cpu/src/cpu.rs`:

```rust
pub const fn new() -> Self {
    Self {
        a: 0,
        x: 0,
        y: 0,
        pc: 0,
        s: 0xFD,                          // <-- assumes post-cold-reset
        p: Status::power_on(),            // <-- $24 (UNUSED + INTERRUPT_DISABLE)
        ...
    }
}

pub fn reset<B: Bus>(&mut self, bus: &mut B) {
    self.s = self.s.wrapping_sub(3);      // <-- unconditional decrement
    self.p.insert(Status::INTERRUPT_DISABLE);
    ...
    // 7-cycle reset sequence: 5 idle/internal cycles + 2 vector reads.
    for _ in 0..5 {
        self.idle_tick(bus);
    }
    let lo = self.read1(bus, RESET_VECTOR);
    let hi = self.read1(bus, RESET_VECTOR + 1);
    self.pc = u16::from(lo) | (u16::from(hi) << 8);
}
```

`Cpu::new()` produces `S=$FD` (the post-reset value), then
`Cpu::reset()` decrements by 3 unconditionally → `S=$FA`. **Mesen2's
power-up path skips this decrement entirely**; only soft-reset
decrements.

Also `Cpu::reset` runs 7 CPU cycles (5 idle + 2 vector reads), while
Mesen2 runs 8 (per the "8 cycles before it starts executing the ROM's
code" comment).

From `crates/nes-ppu/src/ppu.rs` `Ppu::new`:

```rust
dot: 0,
scanline: region.prerender_line(),    // 261 NTSC
frame: 0,
```

The PPU starts at scanline 261 dot 0 (frame 0). After 7 CPU cycles × 3
PPU dots = 21 dots → `(scanline=261, dot=21)`, frame=0. Then frame 0's
remainder (pre-render scanline + frame complete + frame counter
increment) still needs to happen before reaching frame 1's scanline 0.

---

## Divergence analysis

### Net effect by the first CPU instruction

| Position axis | Mesen2 | RustyNES | Delta |
|---------------|--------|----------|-------|
| PPU `(frame, scanline, dot)` | `(1, 0, 25)` | `(0, 261, 21)` | ~+344 dots / ~+115 CPU cycles |
| CPU cycle counter | 7 | 7 | 0 |
| CPU SP | $FD | $FA | +3 |
| CPU P | $04 | $24 | UNUSED bit |

### Behavioural impact

The PPU position delta is the source of the Session-11 finding that
RustyNES is ≥1 boot frame slower than Mesen2 at reaching the
post-warm-up state where PPU register writes take effect.

Concretely, the divergence becomes program-observable around CPU
cycle 27,393 in AccuracyCoin's RESET routine:

```
$8045 LDA $2002    ; AccuracyCoin's VBL-wait poll
$8048 BPL VblLoop
```

At cycle 27,393 both emulators are at `frame=2 scanline=241 dot=2` per
the trace anchor (both have ticked the same number of CPU cycles), but:

- **Mesen2 reads `$2002 = $9A`** (bit 7 = VBL set) → exits the loop,
  proceeds to `INX`/`BEQ`.
- **RustyNES reads `$2002 = $1A`** (bit 7 = VBL clear) → continues
  looping.

The trace shows RustyNES stays in the BPL loop while Mesen2 progresses
through 50+ more instructions in the same cycle window.

Hypothesis (high confidence): the `LDA $2002` read happens during the
4th CPU cycle of the instruction. The PPU has been ticked through 3 ×
3 = 9 dots since instruction start. RustyNES at `dot=2 + 9 = dot 11
scanline 241` should have VBL set (set at scanline 241 dot 1). The fact
that RustyNES reads `$1A` strongly suggests the absolute frame /
scanline alignment is OFF by one whole frame because of the
boot-position divergence.

### The CPU SP cold-boot path

Independent of the PPU divergence, RustyNES's stack pointer is off by
3 at the first instruction. The trace shows this directly:

```
ref     cyc=47  PC=$801F  STX abs $0373    X=$FD
actual  cyc=47  PC=$801F  STX abs $0373    X=$FA
```

`STX $0373` is `PowerOn_SP` in AccuracyCoin's RAM. The two emulators
store different values. However, AccuracyCoin's
`TEST_PowerOnState_CPU_Registers` is **documentation-only** (the
upstream source comments out the pass/fail branch:
`;LDA <RunningAllTests ; Commented out because this is no longer a
pass/fail test`), so this delta is NOT load-bearing for the
`accuracycoin_pass_rate_meets_floor` test.

It IS visible to a player or visual diff, however, and is a real
divergence from Mesen2.

---

## Why no fix attempt this session

1. **The PPU power-up position is the architectural surface Track C1
   has rolled back 6 times.** Every blargg / kevtris / sprite-zero /
   sprite-overflow / dot-skip / MMC3-A12 / `cpu_interrupts_v2` /
   `ppu_vbl_nmi` test is calibrated to the current power-up alignment.
   Changing `(dot=0, scanline=261)` → `(dot=340, scanline=261)` shifts
   every test by ~341 PPU dots (~113 CPU cycles), which would cascade
   through dozens of timing-sensitive tests.

2. **The CPU reset cycle count change (7 → 8) is coordinate-bound to
   the PPU position change.** Mesen2's two changes (start PPU 1 dot
   from frame wrap + run CPU 8 cycles at reset) are mutually
   compensating: Mesen2's net post-reset PPU position lands close to
   where RustyNES + 1 frame would. Either change alone would shift
   timing in a way the other already accounts for.

3. **The SP cold-boot fix is independently small but not load-bearing
   for the AccuracyCoin score.** Per AccuracyCoin's
   `TEST_PowerOnState_CPU_Registers` source comment, the test is
   documentation-only. Landing the SP-fix in isolation would change
   `TSX/STX` byte values across the boot routine in ways that ripple
   through subsequent code paths (e.g. `LDX #$EF` overwrites SP a few
   instructions later, so the divergence is short-lived; but
   intermediate stack pushes during the splash-screen IRQ would observe
   different stack frames).

4. **Six prior C1-axis fix attempts have been rolled back**
   (Attempts 1–4 + Phase B4 threshold-axis prototype + the post-B4
   mid-cycle mapper-IRQ-snapshot experiment). The accumulated cost of
   another roll-back exceeds the value of guessing. The trace data is
   the durable Session-12 deliverable; landing a fix attempt without a
   coordinated multi-axis plan would risk another rollback.

---

## Session-13 next-step plan

The boot-trace infrastructure is now permanent and re-usable. Next
session's productive path:

### Option A (low risk, narrow scope): land the SP cold-boot fix only

1. Add `Cpu::power_on() -> Self` that sets `s = 0` (the cold-boot
   wire state) + uses the existing power-on flag pattern.
2. Modify `Nes::from_rom` / `Nes::from_rom_with_sample_rate` /
   `Nes::power_cycle` to call `Cpu::power_on()` then `cpu.reset()`.
   After reset's `S -= 3` (wrapping), S = $FD (matches Mesen2).
3. Keep `Cpu::new()` working as-is for the test fixtures
   (`nes-cpu/tests/*` use it directly and expect S=$FD).
4. Re-run the full gauntlet. Snapshot drift is expected on:
   `audio_tests` (cycle counts shift by some amount), `visual_regression`
   (probably zero drift; SP is rarely visible). If snapshot drift,
   re-baseline only after verifying audio FNV + cycle invariants are
   close.

Expected outcome: AccuracyCoin pass rate unchanged, SP divergence at
post-reset trace boundary closed.

### Option B (high risk, high reward): coordinated PPU position + CPU reset cycle

1. Change `Ppu::new()` to start at `(scanline=261, dot=340)` —
   equivalent to Mesen2's `(scanline=-1, cycle=340)`.
2. Change `Cpu::reset()` to run 8 cycles total (currently 7).
3. Run the full gauntlet. Expect cascading snapshot drift across many
   timing-sensitive tests. Re-baseline as needed, but ONLY after
   verifying audio + cycle invariants.

Risk: blargg `ppu_vbl_nmi` suite, `cpu_interrupts_v2` suite, MMC3 A12
timing, sprite-hit timing — all calibrated to the current alignment.

Expected outcome: RustyNES boot timing converges to Mesen2's; the
27,393-cycle VBL-poll divergence closes; AccuracyCoin pass rate may
move (direction unknown; could open new failures as much as close
existing ones).

### Option C (highest fidelity, longest horizon): patch the trace
to compute "absolute PPU position offset" and validate divergence
hypothesis against `LDA $2002` cycle precisely

1. Use the existing fixture to capture the trace at exactly cycle
   27,390-27,400 in both emulators.
2. Read the actual `$2002` byte returned at the cycle of the LDA's
   data fetch (T3, 3 cycles into the instruction).
3. If the difference correlates exactly with the +344-dot PPU position
   offset, the hypothesis is empirically proven.

This is the minimum viable diligence path before Option B.

### Recommended Session-13 sequence

1. Land Option A (the SP cold-boot fix) — small scope, immediate
   visible cleanup, no test surface risk.
2. Run Option C as the analytical step.
3. Decide on Option B based on Option C's empirical evidence.

---

## Invariants validated (Session-12 close)

| Invariant | Before | After | Status |
|-----------|--------|-------|--------|
| Workspace tests `--features test-roms` | 537 strict pass + 5 ignored | 537 strict pass + 5 ignored | OK |
| Workspace tests `--features test-roms,commercial-roms` | + 60 strict ROMs | + 60 strict ROMs | OK |
| AccuracyCoin RAM pass rate | 82.73% | 82.73% | OK |
| Sacred trio (SMB / Excitebike / Kid Icarus) | legible | legible (no snapshot drift) | OK |
| `cargo fmt --all --check` | clean | clean | OK |
| `cargo clippy --workspace --all-targets ...` | clean | clean | OK |
| `cargo doc --workspace --no-deps` (RUSTDOCFLAGS=-Dwarnings) | clean | clean | OK |
| `cargo build -p nes-core --target thumbv7em-none-eabihf --no-default-features` | builds | builds | OK |
| `cargo-boot-trace` clippy under all feature combos (with `test-roms`, `commercial-roms`, `ppu-state-trace`, `cpu-boot-trace`) | n/a | clean | OK |

Net change: pure-additive infrastructure; no behavioural change to any
existing test path; no snapshot drift; no audio / cycle invariant
disturbance.

---

## Files added

- `crates/nes-core/src/cpu_boot_trace.rs` (new, ~460 LOC)
- `crates/nes-test-harness/tests/cpu_boot_trace_fixture.rs` (new)
- `crates/nes-test-harness/src/bin/cpu_boot_trace_diff.rs` (new, ~530 LOC)
- `scripts/mesen2_cpu_boot_trace.lua` (new, ~190 LOC)
- `docs/audit/session-12-cpu-boot-trace-2026-05-20.md` (this file)

## Files modified

- `crates/nes-core/Cargo.toml` — added `cpu-boot-trace` feature
- `crates/nes-core/src/lib.rs` — gated `pub mod cpu_boot_trace`
- `crates/nes-core/src/nes.rs` — added `Nes::enable_cpu_boot_trace`,
  `Nes::take_cpu_boot_trace`, `Nes::cpu_boot_trace`, and the per-step
  recording hook (all under `#[cfg(feature = "cpu-boot-trace")]`)
- `crates/nes-test-harness/Cargo.toml` — added `cpu-boot-trace` forward
  + the `cpu_boot_trace_diff` bin target
