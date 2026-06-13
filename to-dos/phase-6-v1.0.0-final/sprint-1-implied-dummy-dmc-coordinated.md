# Sprint 1 — Implied-Dummy-Read + DMC DMA coordinated fix

**Phase:** 6 — v1.0.0 final
**Status:** ROLLBACK (iterations 1 + 2 Phase B deferred). Sprint
remains OPEN. As of Session-22 (2026-05-22) the Phase 1A AccuracyCoin
protocol support landed in `scripts/mesen2_irq_trace.lua` — the Mesen2
oracle infrastructure now matches AccuracyCoin's continuous-run,
per-sub-test RAM-result protocol with autostart Start-press + watchdog
+ exec-callback throughput knob. Phase 1B (the actual Mesen2 oracle
trace covering the DMC sub-tests + the calibration audit) is **deferred
on the wall-time blocker** — Mesen2's Lua exec callback cannot sustain
the throughput needed to reach the DMC sub-tests within reasonable
session budget (~7 effective FPS under xvfb; ~15-20 minutes per pass; ≥
3 passes needed for an audit; Mesen2's testRunner additionally pauses
the emulator at the AccuracyCoin spinning-menu loop around frame 1589,
short of test #141 `Implied Dummy Reads`). See
`docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md` for the full
empirical analysis + the two viable unblock paths (native Mesen2 debug
hooks OR custom AccuracyCoin sub-test ROM). Phase A trace tooling +
Session-22 Phase 1A Lua infrastructure both remain landed as permanent
diagnostic assets.

Predecessor chain:
- `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md` (this
  session's Phase B deferral; Phase 1A landing).
- `docs/audit/session-21-dmc-trace-tooling-2026-05-22.md` (Phase A trace
  tooling).
- `docs/audit/session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
  (iteration 1 cascade rollback rationale).
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` (naive
  attempt + initial cascade).

**Cascade risk:** **HIGH** (Session-19 + Session-20 both regressed
`Implicit DMA Abort`).

## Target tests

Primary: `CPU Behavior 2 :: Implied Dummy Reads [error 3]`

Possible side-flips (per Session-19 hypothesis):
- `APU Tests :: Implied Dummy Reads` (variant of above)
- `APU Tests :: APU Register Activation` (shares bus-phase machinery)
- `APU Tests :: Controller Strobing` (shares bus-phase machinery)
- `APU Tests :: Frame Counter IRQ #7` (frame-counter clears on the
  cycle-2 dummy read on `$4015` access path)

Estimated yield: **+1 to +3 AccuracyCoin tests**.

## Hypothesis (correct per nesdev spec)

Per nesdev `6502_cpu.txt` and the MOS 6502 datasheet, every implied /
accumulator addressing-mode instruction performs a canonical cycle-2
dummy read of PC (the byte after the opcode, PC unchanged). The
RustyNES `step()` burn-loop currently emits this as a bus-quiet
`idle_tick` instead of a `read1` — making the cycle-2 dummy invisible
to the bus, the open-bus latch, the DMC DMA scheduler, and `$4015`
flag-clearing logic.

Affected opcodes (21 sites):
- Shift-A: ASL A (`$0A`), LSR A (`$4A`), ROL A (`$2A`), ROR A (`$6A`)
- Flag: CLC (`$18`), SEC (`$38`), CLI (`$58`), SEI (`$78`), CLV (`$B8`),
  CLD (`$D8`), SED (`$F8`)
- Transfer: TAX (`$AA`), TAY (`$A8`), TSX (`$BA`), TXA (`$8A`), TXS
  (`$9A`), TYA (`$98`)
- INC/DEC implied: INX (`$E8`), DEX (`$CA`), INY (`$C8`), DEY (`$88`)
- NOP: official (`$EA`) + 6 unofficial 1-byte NOPs (`$1A`, `$3A`, `$5A`,
  `$7A`, `$DA`, `$FA`)

Test flow that exercises the bug (`AccuracyCoin.asm` line 13176-ish
region):
```
JSR $4011          ; trigger DMC DMA (data bus = $48)
PHA                ; push A
LDY <$A4           ; load Y from zp
LDA <$A5           ; load A from zp (under-test opcode varies)
[opcode]           ; the IMPLIED/ACCUMULATOR opcode under test
                   ; canonical: cycle-2 dummy read of PC clears the
                   ; $4015 frame counter IRQ flag
[next fetch reads $00] = BRK
[BRK service vector points to PASS]
```

## Why Session-19's naive fix cascade-reverted

Adding a single `implied_dummy_read(bus) → bus.cpu_read(self.pc)` helper
wired into 21 instruction sites flipped 0 target tests and broke
`APU Registers and DMA tests :: Implicit DMA Abort [error 2]`.

Cascade diagnosis (per Session-19 audit):

The `Implicit DMA Abort` test uses `JSR Clockslide_44`-style
fixed-cycle burns to align specific instructions to specific CPU cycles
relative to the DMC DMA. The test sequence implicitly assumes the
current "cycle-2 of 2-cycle implied opcodes is bus-quiet" convention —
the DMC DMA scheduler in `crates/rustynes-core/src/bus.rs` gates DMA-start
on "current CPU cycle is bus-quiet". Adding a cycle-2 bus READ where
there was an internal idle tick changes when the DMC DMA can fire, which
breaks the test's expected timing.

The fix needs the DMC DMA scheduler to either:
- (a) Consider the cycle-2 dummy as a real bus access (DMA respects it
  as a halt boundary), or
- (b) Treat the dummy as "internal" — it reads from PC for open-bus /
  `$4015` clear purposes but does NOT trigger DMA gating, or
- (c) Narrow the DMA abort window by one cycle to compensate.

Each option is a coordinated change touching CPU + Bus + APU. The
correct path is determined by what Mesen2's `NesCpu.cpp` +
`NesApu.cpp` do; that is the design step for this sprint.

## Sprint plan

### Step 1 — Mesen2 cross-reference (research)

Read Mesen2's source (under `~/Code/Mesen2/Core/NES/` or wherever the
local Mesen2 build lives — see `scripts/mesen2_irq_trace.lua` for the
expected path).

Target files:
- `Core/NES/NesCpu.cpp` — find the implied / accumulator instruction
  cycle structure; confirm whether the cycle-2 dummy IS or IS NOT
  emitted, and on which bus.
- `Core/NES/NesApu.cpp` (or `NesApu.h`) — find the DMC DMA scheduler
  and the abort-window logic.
- The combined behavior at the `JSR $4011 → PHA → implied opcode`
  sequence is the oracle.

Capture findings in `docs/audit/sprint-1-mesen2-cross-reference.md`.

### Step 2 — Mesen2 trace oracle (empirical)

Use the existing Mesen2 IRQ-trace infrastructure
(`scripts/mesen2_irq_trace.lua` + the START_CYCLE post-boot diff per
Session-16) to record a Mesen2 trace of the `Implied Dummy Reads` test
+ the `Implicit DMA Abort` test. The cross-diff against RustyNES gives
the precise cycle bounds for the fix.

### Step 3 — Coordinated design

Pick option (a) / (b) / (c) per the Mesen2 finding. Land the change in
3 surfaces:
- `crates/rustynes-cpu/src/cpu.rs` — add `implied_dummy_read` helper +
  wire into 21 sites (per Session-19 implementation pointer).
- `crates/rustynes-core/src/bus.rs` — adjust DMC DMA scheduler abort-window
  logic per the chosen option.
- `crates/rustynes-apu/src/dmc.rs` (or wherever the DMC DMA-start gate
  lives) — if the gating axis is APU-side rather than bus-side.

All under feature flag `cpu-implied-dummy-coordinated` (default off).

### Step 4 — Unit tests

Before landing the production change, add unit tests:

- `crates/rustynes-cpu/tests/opcodes.rs` — exercise CLI (`$58`) under DMC
  DMA pressure; assert cycle-2 dummy fires AND DMC DMA timing matches
  Mesen2.
- `crates/rustynes-core/src/bus.rs` `#[cfg(test)]` — DMC DMA abort-window
  unit test with the cycle-2-dummy convention. Mirror Mesen2's
  behavior precisely.

### Step 5 — Validation gauntlet

Per `to-dos/phase-6-v1.0.0-final/overview.md` "Validation gauntlet"
section. All 10 gates must stay green.

Special-attention gates for this sprint:
- `dmc_dma_during_read4` (5 strict): the canonical DMC DMA regression
  sentinel. Must remain 5/5 strict.
- `apu_test/*` (8 strict): the APU broad regression sentinel.
- `apu_mixer/*` (4 strict): unchanged by this sprint by design.
- The target `Implied Dummy Reads [error 3]` flips.
- `Implicit DMA Abort` stays strict-pass (does NOT regress like
  Session-19).

### Step 6 — Land OR rollback

Land if gauntlet green AND `Implied Dummy Reads [error 3]` flips AND
`Implicit DMA Abort` stays strict. Otherwise:

- Save the chip-stack code change to a worktree branch
  (`sprint-1-implied-dummy-attempt-N`) for post-mortem.
- `git checkout -- crates/rustynes-cpu/src/cpu.rs crates/rustynes-core/src/bus.rs
   crates/rustynes-apu/src/dmc.rs`.
- Land the diagnostic / Mesen2 cross-ref / unit-test infrastructure as
  a separate "Investigated and rolled back" commit with a new
  `docs/audit/sprint-1-attempt-N-rollback.md` audit doc.

## Estimated effort + yield

- **Effort:** 1-2 days of focused work (Step 1 + 2 are research; Steps
  3-6 are implementation + validation).
- **Yield:** +1 (target test only) to +3 (target + APU Reg Activation +
  Controller Strobing) AccuracyCoin tests.

## Cascade-risk callouts

1. The `Implicit DMA Abort` regression from Session-19 is the documented
   cascade. The coordinated fix must close that path explicitly.
2. The `apu_test/*` tests are forgiving in some sub-cycle granularity
   but ARE sensitive to DMC DMA scheduling. Run the full 8-test suite
   on every iteration of Step 3 (not just `dmc_dma_during_read4`).
3. The cycle-2 dummy read updates the open-bus latch. Open Bus error 9
   may flip as a side-benefit; do NOT modify the open-bus logic
   independently to chase that test (cross-cascade risk).
4. PHA / PHP / PLA / PLP also have a canonical cycle-2 PC dummy read per
   nesdev. Session-19's session brief noted the stack-opcode group is
   ambiguous about whether the dummy is observable; defer the stack
   opcodes to Sprint 2 (APU put/get phase) where the put/get
   convention disambiguates.

## References

- nesdev `6502_cpu.txt` (canonical 6502 cycle structures)
- MOS 6502 datasheet (cycle-by-cycle behavior of implied / accumulator
  addressing modes)
- Mesen2 `Core/NES/NesCpu.cpp` (implementation reference)
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` (the
  cascade-revert audit; Step 1 of this sprint extends it)
- `crates/rustynes-core/src/bus.rs:1013-1040` `service_dmc_dma` (the
  surface that cascaded in Session-19)
- AccuracyCoin source `AccuracyCoin.asm` (`100thCoin/AccuracyCoin` on
  GitHub, MIT) — the test sequence for `Implied Dummy Reads` is around
  line 13176; `Implicit DMA Abort` around line 13176-ish in the
  `APU Registers and DMA tests` block.

## Exit criterion

- AccuracyCoin pass rate increases (target +1 to +3 tests).
- No regressions in any of the 10 validation gauntlet gates.
- Diagnostic + audit doc landed regardless of attempt success.
- Next sprint pointer updated in `to-dos/phase-6-v1.0.0-final/overview.md`.

If pass rate reaches ≥ 90% after this sprint, jump to v1.0.0 final tag.
Otherwise proceed to Sprint 2.
