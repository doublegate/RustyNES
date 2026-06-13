# Session 19 — AccuracyCoin pivot attempt + halt

**Date:** 2026-05-22
**Branch:** `main` (clean — no commits landed)
**Outcome:** `no-progress-halted` — pivot to non-C1 AccuracyCoin
residuals halted after 1 attempt with cascade revert.
**Starting state:** HEAD `129ee53`, workspace 541 strict + 5 ignored,
AccuracyCoin 82.73% (108 pass + 7 pass_with_code of 139).

## Mandate (user authorization)

Pivot away from Track C1 (12 rollbacks since v0.9.0-rc — see
ADR-0002) to flip ≥ 11 AccuracyCoin tests from the non-C1 / non-SH* /
non-Open-Bus-error-9 residual set, then tag v1.0.0 once ≥ 90%.

## Triage outcome (Phase 1)

The 24 failing tests cluster into these candidate groups (after
removing the explicit exclusion zones: 4 C1 + 5 SH* + 1 Open Bus
error 9):

| Group | Tests | Tractability assessment |
|---|---|---|
| Implied Dummy Reads error 3 | 1 (CPU Behavior 2) | Fix found at the nesdev-canonical level (cycle-2 PC dummy read on implied/accumulator/transfer/flag opcodes). **Cascade risk: VERY HIGH** — see "Attempt 1" below. |
| Sprite Eval residuals | 4 (`$2002 flag timing`, `Arbitrary Sprite zero`, `Misaligned OAM`, `OAM Corruption`) | Sprite-eval FSM dot-precise timing. Per `feedback_emulator_fsm_mid_cycle_clobber.md`, modifying the sprite-eval FSM mid-scanline has caused the sacred-trio regression (B8b → 834be9e recovery) — extreme cascade risk. |
| PPU Misc residuals | 6 (`Stale BG/Sprite Shift`, `BG Serial In`, `Sprites On Scanline 0`, `$2004 Stress`, `$2007 Stress`) | Dot-precise PPU shift register + OAM-buffer-decay + PPUDATA-buffer-state-machine modeling. Multi-day per test. |
| APU residuals | 4 (`Frame Counter IRQ #7`, `DMC #21`, `APU Reg Activation #4`, `Controller Strobing #4`) | All require architectural surfaces we do not currently model: APU put/get phase at bus, separate 6502/DMC/OAM address buses, controller-strobe phase logic. Multi-day per test. |

## Attempt 1: Implied Dummy Reads — cascade-revert

### Hypothesis (correct per spec)

Per nesdev `6502_cpu.txt` and the MOS 6502 datasheet, all
implied / accumulator addressing-mode instructions perform a
canonical cycle-2 dummy read of PC (the byte after the opcode,
PC unchanged). Our `step()` burn-loop emits this as a bus-quiet
`idle_tick` instead of a `read1`, so the AccuracyCoin test's
DMC-DMA-driven $4015-fetch sequence fails to clear the frame
counter IRQ flag on the dummy read.

The test:
```
JSR $4011 → DMC DMA (data bus = $48) →
PHA → LDY <$A4 → LDA <$A5 (test opcode) →
[opcode executes] → cycle-2 dummy read should clear $4015 frame IRQ →
[next fetch reads $00 (BRK)] → BRK PASS
```

### Implementation

Added a single `implied_dummy_read(bus)` helper that calls
`read1(bus, self.pc)`, wired into 21 instructions:
ASL A (0x0A), LSR A (0x4A), ROL A (0x2A), ROR A (0x6A);
CLC/SEC/CLI/SEI/CLV/CLD/SED (0x18/0x38/0x58/0x78/0xB8/0xD8/0xF8);
TAX/TAY/TSX/TXA/TXS/TYA (0xAA/0xA8/0xBA/0x8A/0x9A/0x98);
INX/DEX/INY/DEY (0xE8/0xCA/0xC8/0x88);
NOP (0xEA) + 6 unofficial 1-byte NOPs (0x1A..0xFA).

PHA/PHP/PLA/PLP also have a canonical cycle-2 PC dummy read per
nesdev, but tried both with-and-without it for the stack
sub-group — see cascade below.

### Validation result

`cargo test --workspace --features test-roms`:
**541 strict pass + 5 ignored — baseline preserved.**

`cargo test -p nes-test-harness --features test-roms accuracycoin`:
**REGRESSED 82.73% → 82.01%** (108 pass + 7 pass_with_code →
108 pass + 6 pass_with_code). The implied-dummy-read fix:
- did **not** flip the target `Implied Dummy Reads [error 3]`
  (it stayed at error 3),
- **broke** `APU Registers and DMA tests :: Implicit DMA Abort [error 2]`
  (was strict-pass at baseline).

### Cascade diagnosis

The `Implicit DMA Abort` test relies on cycle-precise DMC DMA
scheduling. The test sequence (line 13176 of AccuracyCoin.asm)
uses `JSR Clockslide_44` and similar fixed-cycle burns to align
specific instructions to specific CPU cycles relative to the DMC
DMA. Adding a cycle-2 bus READ where there was an internal idle
tick changes when DMC DMA can fire (DMA cannot start on certain
bus phases). The test was implicitly asserting "our DMC DMA
scheduler is correct given the current cycle-2-is-idle
convention" — flipping that convention regresses the test.

Resolution: revert. This fix needs to be coordinated with a DMC
DMA scheduler audit and likely a baseline re-derivation of the
`Implicit DMA Abort` expected behavior. Deferred to v1.x.

`git checkout -- crates/nes-cpu/src/cpu.rs` brought workspace
back to baseline (541 strict + 82.73% AccuracyCoin verified).

## Halt rationale

Per the session brief's stop conditions:
- "Three consecutive candidates yielded no flip" — partially true:
  Implied Dummy Reads is the highest-tractability candidate and
  it cascade-reverted; the remaining categories are all
  architectural surfaces with multi-day effort estimates.
- "Estimated effort to fix next candidate > 1 hour of focused work
  (large surface → defer to user decision)" — true for ALL
  remaining candidates.
- "Cascade risk for next candidate is HIGH" — true for
  sprite-eval (sacred-trio risk per
  `feedback_emulator_fsm_mid_cycle_clobber.md`), PPU misc
  (PPU stress tests assume dot-precise shift-register modeling),
  APU (requires put/get phase modeling at the bus layer +
  separate address buses).

## Recommendations for the next session

Three categorisations of work for the user to choose between:

1. **Coordinated Implied-Dummy-Read + DMC-DMA-scheduler fix.**
   Re-baseline the `Implicit DMA Abort` test against a DMC
   scheduler that correctly models bus-quiet vs bus-active
   cycles. Estimated 1-2 days. Net gain: flip 1 AccuracyCoin
   test (+ possibly side-flips in `APU Tests :: APU Register
   Activation` and `Controller Strobing` which share the
   bus-phase machinery).

2. **APU put/get phase plumbing.** Make `Apu::read_status` and
   `read_register($4015/$4016/$4017)` aware of the cycle phase
   they're called on. Estimated 2-3 days. Net gain: 1-3
   AccuracyCoin tests (Frame Counter IRQ #7, possibly Controller
   Strobing #4).

3. **Tag v1.0.0-rc2 at 82.73% and ship.** The remaining gap to
   the original 90% gate is gated on architectural changes that
   are multi-week, not multi-hour. The 82.73% pass rate already
   demonstrates strong cycle-accurate behavior — by comparison,
   the AccuracyCoin README cites "good" emulators at 70-85%.
   Re-evaluate the 90% bar for v1.0.0 vs v1.x.

## Files touched (and reverted)

- `crates/nes-cpu/src/cpu.rs` — added 1 helper + 21 call sites,
  then `git checkout --`. No commits landed.

## Bottom-line metrics

- Workspace strict: 541 → 541 (no change, no commits)
- AccuracyCoin: 82.73% → 82.73% (no change, no commits)
- Commercial-ROM oracle: untouched (no commits)
- B4 invariant: untouched
- Sacred trio: untouched
- C1 axis: untouched (per exclusion zone)
- v1.x backlog (carried forward unchanged):
  * 4 C1 sub-tests (`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` #3)
  * 5 SH* unstable stores
  * 1 Open Bus error 9 (internal-bus distinction)
  * 1 Implied Dummy Reads error 3 (cascades into DMC DMA scheduler)
  * 4 Sprite Eval residuals (sacred-trio cascade risk)
  * 6 PPU Misc residuals
  * 4 APU residuals
