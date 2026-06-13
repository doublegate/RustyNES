# Session 23 — AccuracyCoin upstream-source audit for v1.0.0-final grind

**Date:** 2026-05-22
**Branch:** `main` (HEAD `f3b15ea` at session start)
**Scope:** Phase 1 source audit per the multi-phase brief
(`linked-puzzling-sutherland.md`). For every AccuracyCoin sub-test failing
on RustyNES v1.0.0-rc2, locate the assertion block in upstream
`100thCoin/AccuracyCoin@main` (cloned to `/tmp/AccuracyCoin-source/` at
session start), categorise as SHALLOW / MEDIUM / DEEP, cite the RustyNES
code site that diverges, and inventory cascade risk.

**Predecessors:**
- `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md`
  (Mesen2 wall-time blocker documenting the case for the custom-ROM
  path Phase 2 enables).
- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md`
  (the Controller Strobing M2-low-latch hypothesis the brief is built
  around).
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` (cluster
  diagnosis + Cascade A geometric puzzle).

**Upstream source revision:** cloned 2026-05-22 from
`https://github.com/100thCoin/AccuracyCoin.git@main` (HEAD `main`).
Assembled with `nesasm.exe` (vendored in repo root); single-pass
6502 macro assembler. 18 758 LoC.

## Baseline diagnostic

```bash
env -u RUSTC_WRAPPER cargo test -p nes-test-harness --features test-roms \
    accuracycoin --release -- --nocapture
```

Result captured at `/tmp/baseline-accuracycoin.log`. Headline:

- **Total:** 144 / pass=108 / pass_with_code=7 / fail=24 / not_run=5
- **Pass rate (RAM-direct):** **82.73%** over 139 assigned tests
- 24 failing tests across 6 clusters (full names in the audit table).

## Per-test source audit (24 failing tests)

Tractability legend:

- **S — Shallow:** 1-line / 1-config fix or otherwise covered by the
  Phase 4 / Phase 3 sprints of this brief; cite exact divergent
  RustyNES site.
- **M — Medium:** needs Mesen2 oracle trace evidence; unblockable via
  the Phase 2 custom-ROM path.
- **D — Deep:** architectural surface; multi-session (DMC scheduler
  internal-bus model, sprite-eval FSM rewrite, C1 IRQ-sample axis,
  unstable SH* internal-bus model).

| # | Test | Upstream label (.asm:line) | Result addr | Error code | Score | RustyNES site | Cascade-risk inventory |
|---|---|---|---|---|---|---|---|
| 1 | `CPU Behavior :: Open Bus [error 9]` | `TEST_OpenBus` (asm:3209) | `$0408` | 9 | **D** | `crates/nes-core/src/bus.rs` open-bus latch; `Mapper::cpu_read_unmapped`. Internal-vs-external bus distinction; nesdev `$4015` external-disconnected behavior already covered (Phase D3 fix #7). | DMC DMA + open-bus tests; out of v1.0.0 scope per brief. |
| 2 | `Unofficial: $93 SHA indirect,Y [error 7]` | `TEST_SH*` family | various | 7 | **D** | Unstable SH* opcodes in `crates/nes-cpu/src/cpu.rs`. | Out of v1.0.0 scope per brief. |
| 3 | `Unofficial: $9F SHA absolute,Y [error 7]` | ditto | | 7 | **D** | ditto | ditto |
| 4 | `Unofficial: $9B SHS absolute,Y [error 7]` | ditto | | 7 | **D** | ditto | ditto |
| 5 | `Unofficial: $9C SHY absolute,X [error 7]` | ditto | | 7 | **D** | ditto | ditto |
| 6 | `Unofficial: $9E SHX absolute,Y [error 7]` | ditto | | 7 | **D** | ditto | ditto |
| 7 | `CPU Interrupts :: Interrupt flag latency [error 11]` | `TEST_InterruptFlagLatency` block (`cpu_interrupts_v2/5` equivalent) | | 11 | **D** | C1 axis (`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` #3). 13+ rollback attempts. | EXPLICITLY excluded by brief. |
| 8 | `CPU Interrupts :: NMI Overlap BRK [error 2]` | `TEST_NMI_Overlap_BRK` | | 2 | **D** | C1 axis. | EXPLICITLY excluded by brief. |
| 9 | `CPU Interrupts :: NMI Overlap IRQ [error 1]` | `TEST_NMI_Overlap_IRQ` | | 1 | **D** | C1 axis. | EXPLICITLY excluded by brief. |
| 10 | `APU Tests :: Frame Counter IRQ [error 7]` | `TEST_FrameCounterIRQ` (asm:10120, sub-test 7 = asm:10177) | `$0467` | 7 | **M** | `crates/nes-apu/src/frame_counter.rs:109-152` + `apu.rs:616`. Phase-aware `$4015` bit-6 clear timing on M2-put cycle. | `apu_test 8/8`, `apu_mixer 4/4`, `dmc_dma_during_read4 5/5`. C1 axis shared surface. **HIGH**. |
| 11 | `APU Tests :: Delta Modulation Channel [error 21]` | `TEST_DeltaModulationChannel` (asm:10842, sub-test 21 = deep into list) | `$046A` | 21 | **D** | `crates/nes-apu/src/dmc.rs`. Deep DMC scheduler audit; Session-19/20 cascade. | `apu_test`, `dmc_dma_during_read4` (5/5 strict). |
| 12 | `APU Tests :: APU Register Activation [error 4]` | `TEST_APURegActivation` (asm:8000) | `$045C` | 4 | **M** | `crates/nes-apu/src/apu.rs` register-write put/get phase. | `apu_test`, `apu_mixer`. |
| 13 | `APU Tests :: Controller Strobing [error 4]` | `TEST_ControllerStrobing` (asm:8574, sub-test 4 = asm:8619) | `$045F` | 4 | **M** | `crates/nes-core/src/controller.rs:88-114` (latch on rising-edge-or-continuous) + `crates/nes-core/src/bus.rs:1514-1518` ($4016 write dispatch). | `cpu_dummy_writes_oam`, `apu_test`, all commercial-ROM oracle ROMs that strobe controllers. **MEDIUM**. |
| 14 | `Sprite Evaluation :: $2002 flag timing [error 1]` | `TEST_2002FlagTiming` (asm:2261) | `$048D` | 1 | **D** | Sprite-eval FSM ($2002 read clears bit-7 at specific dot; race window). Session-18 PPU axis. | `ppu_vbl_nmi 10/10`, `sprite_hit_tests 11/11`, `sprite_overflow_tests 5/5`. **HIGH**. |
| 15 | `Sprite Evaluation :: Arbitrary Sprite zero [error 2]` | `TEST_ArbitrarySpriteZero` (asm:7026) | `$0458` | 2 | **D** | Sprite-eval FSM (sprite-0-hit triggering geometry). Cascade A residual. | Sacred trio (sprite-zero geometry). **HIGH**. |
| 15 | `Sprite Evaluation :: Misaligned OAM behavior [error 1]` | `TEST_MisalignedOAM_Behavior` (asm:7308) | `$045A` | 1 | **D** | OAMADDR walk + non-$4-aligned `$2004` write semantics; partially covered Session-7 `c230489`. | OAM tests. **HIGH**. |
| 16 | `Sprite Evaluation :: OAM Corruption [error 2]` | `TEST_OAM_Corruption` (asm:13953) | `$047B` | 2 | **D** | OAM corruption pattern during sprite-eval. | OAM tests. **HIGH**. |
| 17 | `PPU Misc. :: Stale BG Shift Registers [error 3]` | `TEST_StaleBGShiftRegisters` (asm:15255) | `$0483` | 3 | **D** | BG-shift-register reload timing post-rendering-disable. | Sacred trio (BG-pipeline cycle 9). **HIGH**. |
| 18 | `PPU Misc. :: Stale Sprite Shift Regs [error 3]` | `TEST_StaleSpriteShiftRegs` (asm:3013) | | 3 | **D** | Sprite shift-register reload timing. | OAM tests + sacred trio. **HIGH**. |
| 19 | `PPU Misc. :: BG Serial In [error 2]` | `TEST_BGSerialIn` (asm:15780) | | 2 | **D** | $2001 write delay (2-5 PPU cycles depending on clock alignment); race against BG-shift load at dot%8==7. | Sacred trio. **HIGH**. |
| 20 | `PPU Misc. :: Sprites On Scanline 0 [error 2]` | `TEST_Scanline0Sprites` (asm:15420) | | 2 | **D** | Sprite-on-scanline-0 evaluation edge case. | `sprite_hit_tests`. **HIGH**. |
| 21 | `PPU Misc. :: $2004 Stress Test [error 2]` | `TEST_2004_Stress` (asm:1926) | | 2 | **D** | $2004 write-during-rendering OAMADDR walking. Partial fix Session-7. | OAM. **HIGH**. |
| 22 | `PPU Misc. :: $2007 Stress Test [error 2]` | `TEST_2007_Stress` (asm:2434) | | 2 | **D** | $2007 v-increment race during rendering. | Cascade A surface. **HIGH**. |
| 23 | `CPU Behavior 2 :: Implied Dummy Reads [error 3]` | `TEST_ImpliedDummyRead` (asm:11634, sub-test 3 = asm:11686) | `$046D` | 3 | **M** | `crates/nes-cpu/src/cpu.rs:888-1593` (21 implied-mode opcode handlers). Cycle-2 PC dummy read needs to be a bus-visible `read1` instead of `idle_tick` to clear the `$4015` frame-counter IRQ flag. Coordinated with DMC DMA scheduler per Sprint 1 spec. Session-19+20 cascade-reverted. | `dmc_dma_during_read4 5/5`, `apu_test 8/8`. **HIGH (cascade-documented twice)**. |

## Cluster summary

| Cluster | Count | Distribution | v1.0-final eligible? |
|---|---|---|---|
| C1 IRQ axis | 3 | All **D** | NO — brief explicitly excludes |
| Unstable SH* | 5 | All **D** | NO — brief explicitly excludes |
| Sprite-eval | 4 | All **D** | Sprint 3 (deferred per brief) |
| PPU misc | 6 | All **D** | Sprint 4 (deferred per brief) |
| APU residuals | 4 | 2 **M** (Controller Strobing + APU Reg Activation), 1 **D** (DMC), 1 **M** (Frame Counter IRQ #7) | YES — Phase 3 + Phase 4 cover Controller Strobing + Implied Dummy / DMC |
| Implied Dummy | 1 | **M** (with Mesen2 oracle) | YES — Phase 4 covers |
| Open Bus | 1 | **D** | NO — brief explicitly excludes (v1.x) |
| **TOTAL** | **24** | **0 S / 4 M / 20 D** | |

## Phase 1.5 free-win decision

**Zero SHALLOW candidates found.** The fix surfaces the brief flagged as
free-win sites (`controller.rs:88-114` strobe latch; 21 implied-mode
opcode handlers) are all categorised **M** rather than **S** because:

1. **Controller Strobing**: requires plumbing the M2 phase from
   `LockstepBus::current_m2_phase()` through to `Controller::write_strobe`
   (rising-edge-or-continuous → M2-low-boundary-where-strobe-transitions-0→1).
   This is a multi-file change with documented cascade risk into all
   controller-input tests (`cpu_dummy_writes_oam`, `apu_test/*` sync
   writes, every commercial-ROM oracle ROM that strobes controllers).
   Without an empirical Mesen2 oracle trace from Phase 2 confirming the
   exact cycle on which the latch must fire, this carries Session-19-style
   cascade-revert risk. **Per the brief's discipline**: this is a Phase 3
   item, not a Phase 1.5 free win.

2. **Implied Dummy Reads + DMC**: documented twice cascade-reverted
   (Session-19 + Session-20). The brief explicitly defers this to Phase 4
   under the `cpu-implied-dummy-coordinated` feature flag with mandatory
   Mesen2 oracle prerequisite (Phase 4.1) and coordinated DMC scheduler
   adjustment (Phase 4.3). Not a Phase 1.5 candidate.

3. **Frame Counter IRQ #7 + APU Reg Activation**: documented in
   Session-22 Sprint 2 audit as MEDIUM (multiple hypotheses without
   oracle differentiation). Not a Phase 1.5 candidate.

**Decision:** **SKIP Phase 1.5.** Proceed to Phase 2 (custom AccuracyCoin
sub-test ROMs) which is the brief's documented unblock path for both
Phase 3 (Controller Strobing) and Phase 4 (Implied Dummy + DMC).

## Phase 2 prerequisites confirmed

The audit located the exact entry-point labels needed for Phase 2:

| Target test | Upstream label | ASM line | Result addr |
|---|---|---|---|
| Controller Strobing (Phase 3) | `TEST_ControllerStrobing` | 8574 | `$045F` |
| Implied Dummy Reads (Phase 4) | `TEST_ImpliedDummyRead` | 11634 | `$046D` |
| Frame Counter IRQ (optional Phase 3/4) | `TEST_FrameCounterIRQ` | 10120 | `$0467` |
| APU Register Activation (optional) | `TEST_APURegActivation` | 8000 | `$045C` |

The `Suite_*` table-of-contents block (asm:541-770) gives the result-byte
addresses + the TEST function pointer for each sub-test.

The AccuracyCoin source assembles with the vendored `nesasm.exe` (Magic
Kit's NES Assembler). Native Linux build paths to verify in Phase 2:
`asm6`, `nesasm`, `ca65/ld65`, or as final fallback hex-patching
`AccuracyCoin.nes` to jump from the menu-loop to the target test
entry-point.

## File changes summary

- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md`: this doc
  (new).

## References

- `https://github.com/100thCoin/AccuracyCoin.git@main` — upstream source
  cloned at session start.
- `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` — 144-entry result-address
  catalog already extracted in Phase D2.
- `crates/nes-test-harness/src/accuracy_coin_catalog.rs` — Rust-side
  catalog with `OnceLock`-lazy parse.
- `crates/nes-test-harness/tests/accuracycoin.rs` — per-failing-test
  diagnostic harness (printed per CI run).
- `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md` — Mesen2
  wall-time blocker + custom-ROM unblock path.
- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md` — Controller
  Strobing M2-low-latch hypothesis (Phase 3 source).
- `docs/audit/session-19-accuracycoin-pivot-2026-05-22.md` — original
  cascade analysis informing this backlog.
- `crates/nes-core/src/controller.rs:88-114` — Phase 3 fix site.
- `crates/nes-core/src/bus.rs:1514-1518` — Phase 3 dispatch site.
- `crates/nes-cpu/src/cpu.rs:888-1593` — Phase 4 implied-mode dispatch
  surface (21 opcode handlers).
- `crates/nes-apu/src/dmc.rs` — Phase 4 DMC scheduler.
- `crates/nes-apu/src/frame_counter.rs:109-152` — APU put/get put/get
  surface (Frame Counter IRQ #7).
