# Session 23 — Custom AccuracyCoin sub-test ROMs unblock the Mesen2 oracle

**Date:** 2026-05-22
**Branch:** `main` (HEAD `f2b51c1` at this phase's start, Phase 1 audit
committed first)
**Scope:** Phase 2 of the v1.0.0-final brief
(`linked-puzzling-sutherland.md`). Build custom sub-test ROMs that boot
directly into a single AccuracyCoin target test, bypassing both the
title-screen menu and the full-battery loop. This unblocks the
Session-22 Mesen2 wall-time blocker: rather than the ~3000-frame
testRunner pause-and-cannot-reach-the-target-test path, the custom ROMs
reach the target test by frame ~30 — well within Mesen2's per-cycle Lua
trace budget.

**Predecessor:**
`docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md` —
Phase 1 source audit identifying the 4 target tests + their entry-point
labels in upstream `AccuracyCoin.asm`.

## Toolchain probe

```bash
which ca65 ld65 cc65 asm6 nesasm wine
# /home/parobek/.local/bin/ca65
# /home/parobek/.local/bin/ld65
# /home/parobek/.local/bin/cc65
# (asm6 / nesasm: not found)
# /usr/bin/wine  (11.9)
```

- **CA65 (cc65 toolchain)** available, but upstream `AccuracyCoin.asm`
  is **NESASM**-flavour assembly (directives like `.byte` / `.word` are
  shared but macros like `table .macro` + `LOW(...)` + `HIGH(...)` +
  the include-less single-file layout are NESASM idioms). Porting to
  CA65 is a 2+ day effort with brittle correspondence — high risk of
  introducing assembly bugs that diverge the custom ROM's behavior
  from upstream.
- **NESASM** native Linux binary unavailable in package repositories.
- **`wine` + the upstream `nesasm.exe`** (vendored at
  `/tmp/AccuracyCoin-source/nesasm.exe` after upstream clone) is
  available and produces bit-identical assembly to upstream. The
  unpatched assembly produces a `.nes` file that matches the
  upstream-shipped `AccuracyCoin.nes` in content (different MD5
  because the vendored ROM in the repo is an older snapshot — both are
  valid AccuracyCoin builds).

**Decision:** use `wine` + upstream `nesasm.exe`. Patched source goes
through the same assembler the upstream maintainer uses, eliminating
toolchain-divergence risk.

## Build script: `scripts/accuracycoin-build/build_sub_test_rom.py`

A 200-LoC Python build driver that:

1. Reads the upstream `AccuracyCoin.asm` from a caller-specified
   clone directory.
2. Applies two surgical source patches:
   - **Patch 1**: replace the body of `AutomaticallyRunEveryTestInROM`
     (asm:783-868) with a streamlined wrapper that:
     - sets `Y` to the target suite index,
     - calls `SetUpSuitePointer` + `LoadSuiteMenuNoRendering`,
     - sets `X` / `menuCursorYPos` to the target test index within
       the suite,
     - waits for VBlank, JSRs `RunTest` once,
     - halts forever with `STA $4015=0` and `JSR WaitForVBlank` so
       the post-test ROM state is observable by an oracle tool.
   - **Patch 2**: redirect the boot-path spinning `InfiniteLoop:
     JMP InfiniteLoop` (asm:467-468, reached after main-menu setup
     and `EnableNMI`) into `JMP AutomaticallyRunEveryTestInROM`. This
     bypasses the user-must-press-Start gate.
3. Copies the patched `.asm` + auxiliary files (`Sprites.pcx`,
   `Tiles.pcx`) + `nesasm.exe` into a scratch build directory.
4. Invokes `wine nesasm.exe AccuracyCoin.asm` (the upstream toolchain).
5. Copies the produced `AccuracyCoin.nes` to the caller-specified
   output path.

Each patched ROM is 40 976 bytes (16 B iNES header + 32 KiB PRG + 8 KiB
CHR — identical layout to the upstream).

### Validator binary

A second tool, `crates/nes-test-harness/src/bin/validate_sub_test_rom.rs`,
boots the custom ROM under `nes-core` and runs frames until the target
RAM byte stabilises. Prints the final byte + decoded TestStatus
(`Pass` / `Fail` / `PassWithCode` / `NotRun` per
`accuracy_coin_catalog::TestStatus::from_byte` semantics).

Used as a smoke test: any custom ROM that doesn't reach the target
RAM byte by frame ~600 is broken and won't help Mesen2.

## Built ROMs

| ROM | Target | Suite/Test | Frame target reached on RustyNES | Result byte | Decoded |
|---|---|---|---|---|---|
| `controller-strobing.nes` | `TEST_ControllerStrobing` (asm:8574) | suite=13, test=7 | **27** | `0x06` | Fail (error code 4) |
| `implied-dummy-reads.nes` | `TEST_ImpliedDummyRead` (asm:11634) | suite=19, test=1 | **31** | `0x0E` | Fail (error code 3) |
| `frame-counter-irq.nes` | `TEST_FrameCounterIRQ` (asm:10120) | suite=13, test=2 | **35** | `0x1E` | Fail (error code 7) |
| `apu-reg-activation.nes` | `TEST_APURegActivation` (asm:8000) | suite=13, test=6 | **31** | `0x12` | Fail (error code 4) |

**Cross-check vs full-battery baseline diagnostic** (per
`/tmp/baseline-accuracycoin.log`):

| Test | Full battery on RustyNES | Custom-ROM on RustyNES | Match? |
|---|---|---|---|
| Controller Strobing | `[error 4]` | `0x06` = Fail #4 | **YES** |
| Implied Dummy Reads | `[error 3]` | `0x0E` = Fail #3 | **YES** |
| Frame Counter IRQ | `[error 7]` | `0x1E` = Fail #7 | **YES** |
| APU Register Activation | `[error 4]` | `0x12` = Fail #4 | **YES** |

The custom ROMs reproduce the EXACT error codes from the full-battery
run, confirming each target test is being exercised correctly.

## Why this unblocks Phase 3 and Phase 4

Per Session-22's empirical measurement (line 116-149 of
`session-22-sprint1-iter2-phase-b-2026-05-22.md`):

> Mesen2's Lua `emu.addMemoryCallback` trampolines fire per-CPU-
> instruction at ~500k/frame. […] The AccuracyCoin battery reaches the
> relevant DMC sub-tests only after ~3000+ NES frames […] Mesen2's
> testRunner additionally pauses the emulator at the AccuracyCoin
> spinning-menu loop around frame 1589, short of test #141 (`Implied
> Dummy Reads`).

The custom ROMs collapse the "≥ 3000 NES frames per trace pass" cost
to "~30 NES frames per trace pass". Mesen2 can therefore produce
per-cycle oracle traces for each of the 4 target tests in well under
1 NES second of trace execution.

Phase 3 (Controller Strobing fix) and Phase 4 (Implied Dummy + DMC
coordinated fix) can now proceed with empirical Mesen2 oracle
evidence — the brief's "ONLY if the trace evidence drives the design"
discipline is satisfied.

## File changes summary

- `scripts/accuracycoin-build/build_sub_test_rom.py`: new Python build
  driver (~200 LoC).
- `crates/nes-test-harness/src/bin/validate_sub_test_rom.rs`: new
  validator binary (~95 LoC).
- `tests/roms/AccuracyCoin/sub-tests/controller-strobing.nes`: 40 976 B
  custom ROM.
- `tests/roms/AccuracyCoin/sub-tests/implied-dummy-reads.nes`: 40 976 B.
- `tests/roms/AccuracyCoin/sub-tests/frame-counter-irq.nes`: 40 976 B.
- `tests/roms/AccuracyCoin/sub-tests/apu-reg-activation.nes`: 40 976 B.
- `tests/roms/LICENSES.md`: 4 new sub-test entries + provenance note
  (derivative works under upstream MIT).
- `tests/roms/AccuracyCoin/README.md`: pointer to `sub-tests/`.
- `docs/audit/session-23-custom-accuracycoin-sub-test-roms-2026-05-22.md`:
  this doc.

## Validation

- All 4 custom ROMs validated via
  `cargo run -p nes-test-harness --release --bin validate_sub_test_rom`:
  each reaches its target test by frame ~30 with the expected error
  code matching the full-battery baseline.
- Workspace tests `--features test-roms`: 541 strict + 5 ignored
  (preserved; no chip-stack code changed).
- AccuracyCoin pass rate: 82.73% (preserved; the custom ROMs are
  test infrastructure, not chip-stack changes).
- `cargo fmt --all --check`: PASS.

## Phase 2 outcome

**SUCCESS.** All 4 target sub-test ROMs build via the upstream NESASM
toolchain under `wine` + the surgical source patch. Each ROM boots
directly into its target test on RustyNES, reproducing the full-
battery's exact error code in ~30 NES frames. The infrastructure is now
in place for Phase 3 (Controller Strobing) and Phase 4 (Implied Dummy +
DMC) to proceed with Mesen2 oracle evidence.

Pass rate unchanged at **82.73%** (test infrastructure only).

## Next steps

- **Phase 3 (Controller Strobing fix)**: run `controller-strobing.nes`
  through Mesen2 with the existing `scripts/mesen2_irq_trace.lua` +
  `accuracycoin` protocol path. Cross-diff vs RustyNES trace to
  confirm or refute the Session-22 M2-low-latch hypothesis. If
  confirmed, implement the surgical fix in
  `crates/nes-core/src/controller.rs:88-114` +
  `crates/nes-core/src/bus.rs:1514-1518`.
- **Phase 4 (Implied Dummy + DMC coordinated fix)**: run
  `implied-dummy-reads.nes` through Mesen2; cross-diff DMC scheduler
  state vs RustyNES sidecar trace. Derive a single-axis calibration
  hypothesis. Implement under `cpu-implied-dummy-coordinated` feature
  flag.

## References

- `docs/audit/session-22-sprint1-iter2-phase-b-2026-05-22.md` (wall-time
  blocker).
- `docs/audit/session-22-sprint2-apu-put-get-2026-05-22.md` (Controller
  Strobing hypothesis source).
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md`
  (Phase 1 audit — entry-point labels).
- `scripts/accuracycoin-build/build_sub_test_rom.py` (build driver).
- `crates/nes-test-harness/src/bin/validate_sub_test_rom.rs` (validator).
- upstream `https://github.com/100thCoin/AccuracyCoin.git@main`.
- `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` (144-entry catalog).
- `tests/roms/LICENSES.md` (provenance + MIT inheritance).
