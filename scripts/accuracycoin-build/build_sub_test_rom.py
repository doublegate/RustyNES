#!/usr/bin/env python3
"""
Build a custom AccuracyCoin sub-test ROM that runs ONE target test
directly from boot, bypassing the menu and the full-battery loop.

This addresses the Session-22 Mesen2 wall-time blocker: Mesen2's
testRunner under xvfb cannot reach the AccuracyCoin sub-tests we care
about (~3000+ frames at ~7 effective FPS, with the testRunner pausing
at the spinning-menu loop around frame 1589). A custom ROM that lands
in the target test by frame ~30 lets Mesen2 produce per-cycle oracle
traces in <1 NES second of execution.

USAGE:
    python build_sub_test_rom.py <upstream-source-dir> \\
        --suite <suite-index> --test <test-index> \\
        --out <output.nes>

The "suite index" is the 0-based offset into TableTable
(`AccuracyCoin.asm` line 497-518). The "test index" is the 0-based
offset of the target within the suite's `table "name", ...` lines.

Suite map (from upstream `AccuracyCoin.asm` lines 497-517):
   0: Suite_CPUBehavior          1: Suite_CPUInstructions
   2: Suite_UnofficialOps_SLO    3: Suite_UnofficialOps_RLA
   4: Suite_UnofficialOps_SRE    5: Suite_UnofficialOps_RRA
   6: Suite_UnofficialOps__AX    7: Suite_UnofficialOps_DCP
   8: Suite_UnofficialOps_ISC    9: Suite_UnofficialOps_SH_
  10: Suite_UnofficialOps_Immediates  11: Suite_CPUInterrupts
  12: Suite_DMATests            13: Suite_APUTiming
  14: Suite_PowerOnState        15: Suite_PPUBehavior
  16: Suite_PPUTiming           17: Suite_SpriteZeroHits
  18: Suite_PPUMisc             19: Suite_CPUBehavior2

Targets (per docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md):
- Controller Strobing:   suite=13, test=7 (TEST_ControllerStrobing $045F)
- Implied Dummy Reads:   suite=19, test=1 (TEST_ImpliedDummyRead   $046D)
- Frame Counter IRQ:     suite=13, test=2 (TEST_FrameCounterIRQ    $0467)
- APU Register Activation: suite=13, test=6 (TEST_APURegActivation $045C)

Implementation: replaces the body of `AutomaticallyRunEveryTestInROM`
with a streamlined version that initialises Y to the suite index,
calls `SetUpSuitePointer` + `LoadSuiteMenuNoRendering`, sets X /
menuCursorYPos to the test index, calls `RunTest` once, and falls
into an infinite loop with `STA $4015 = 0`.

LICENSE: build script is MIT (same as the rest of RustyNES). The
generated ROMs are derivative works of 100thCoin/AccuracyCoin and
inherit the upstream MIT license (see tests/roms/AccuracyCoin/LICENSES.md).
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

# Replacement body for `AutomaticallyRunEveryTestInROM`. NESASM-flavour
# 6502 assembly with `LDY #imm` injection for SUITE / `LDX #imm` for
# TEST.
REPLACEMENT_TEMPLATE = """\
AutomaticallyRunEveryTestInROM:
	; CUSTOM SUB-TEST WRAPPER — built by scripts/accuracycoin-build/build_sub_test_rom.py
	; Runs a single test then halts, instead of iterating the full
	; battery (which Mesen2's testRunner cannot reach within budget).
	LDA #1
	STA <RunningAllTests
	JSR DisableNMI
	JSR DisableRendering
	JSR ClearNametable
	LDA #0
	STA <dontSetPointer
	JSR ResetScroll
	LDY #{suite_idx}                 ; suite index in TableTable
	STY <menuTabXPos
	JSR SetUpSuitePointer
	JSR LoadSuiteMenuNoRendering
	LDX #{test_idx}                  ; test index within the suite
	STX <menuCursorYPos
	JSR WaitForVBlank
	; --- Pre-drain controller 1 (Session-24 fix): in the full battery
	; the menu's NMI handler calls ReadController1 every frame, which
	; leaves the shift register fully-shifted (== $FF) before each
	; test begins. Without this drain, TEST_ControllerStrobing's Test
	; 1 fails because the FIRST 8 reads see the un-drained shift
	; (which holds $00 on a fresh boot) instead of the all-1s "empty"
	; state the test expects. Drain in-line so the custom ROM
	; reproduces the same controller state the full battery sees.
	LDA #$01
	STA $4016
	LDA #$00
	STA $4016
	LDX #$08
CustomSubTest_DrainLoop:
	LDA $4016
	DEX
	BNE CustomSubTest_DrainLoop
	; Drain controller 2 as well (some tests poll $4017).
	LDX #$08
CustomSubTest_DrainLoop2:
	LDA $4017
	DEX
	BNE CustomSubTest_DrainLoop2
	; --- Pre-seed cross-test prerequisite flags (Session-24 fix): some
	; tests check `result_*_PreTest` zero-page bytes that get set by
	; earlier tests in the full battery.  In the custom-ROM path those
	; earlier tests never run, so the prerequisite check fails before
	; the target test even gets to its first interesting sub-test.
	; Pre-seed the known prerequisite addresses to `$01` (pass).
	; (Adding more here is cheap — better to over-seed than to silently
	; degrade the diagnostic.)
	LDA #$01
	STA <$12 ; result_DMADMASync_PreTest — used by Implied Dummy Reads test 3
	JSR RunTest
	; --- Test complete. The result byte was written by RunTest to
	; `[TestResultPointer]`. Halt with $4015 disabled so DMC stops
	; firing, and burn frames forever so the Mesen2 oracle has a
	; stable post-test window.
	LDA #0
	STA $4015
CustomSubTest_Halt:
	JSR WaitForVBlank
	JMP CustomSubTest_Halt
"""


def patch_source(src: str, suite_idx: int, test_idx: int) -> str:
    """Replace the body of `AutomaticallyRunEveryTestInROM`."""
    # Match from the label line through the end of the routine. The
    # routine ends before `;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;`
    # block marker that starts the next section, but it's safer to
    # find the next labelled subroutine after it.
    #
    # Find anchor:  `AutomaticallyRunEveryTestInROM:` line.
    # Find end: search forward for the next top-level label or RTS
    #           followed by a comment-line section break.
    start_re = re.compile(
        r"^AutomaticallyRunEveryTestInROM:.*?$", re.MULTILINE
    )
    m = start_re.search(src)
    if not m:
        sys.exit("Could not find AutomaticallyRunEveryTestInROM label")
    start = m.start()

    # End: find the next ";;;;;;;" block marker that's followed by a
    # different label. Look for the closing RTS of this routine, then
    # advance to the next labelled subroutine to be safe. The upstream
    # source has the routine end at the RTS before the
    # `Sleep_15_Frames:` label (or whatever follows).
    #
    # Simpler heuristic: from `AutomaticallyRunEveryTestInROM:` find
    # the next ";;;;;;;" line followed by a label. Take everything up
    # to and including the ";;;;;;;" delimiter line.
    end_re = re.compile(r"^;;;;;;;\s*$", re.MULTILINE)
    end_match = end_re.search(src, start)
    if not end_match:
        sys.exit("Could not find end-of-routine marker")
    end = end_match.end()

    new_body = REPLACEMENT_TEMPLATE.format(
        suite_idx=suite_idx, test_idx=test_idx
    )
    src = src[:start] + new_body + "\n;;;;;;;\n" + src[end:]

    # === Patch 2: redirect the boot-path spinning loop ===
    # Original:
    #   InfiniteLoop:
    #       JMP InfiniteLoop	; This is the spinning loop ...
    # New:
    #   InfiniteLoop:
    #       JMP AutomaticallyRunEveryTestInROM
    # The upstream boot path reaches InfiniteLoop after main-menu setup
    # + EnableNMI. The NMI handler is what normally dispatches the
    # automated-battery on Start-press. For the custom sub-test ROM we
    # bypass the user-input wait by jumping directly into the (now
    # streamlined) wrapper.
    loop_re = re.compile(
        r"^InfiniteLoop:\s*\n\s*JMP\s+InfiniteLoop\b[^\n]*",
        re.MULTILINE,
    )
    loop_m = loop_re.search(src)
    if not loop_m:
        sys.exit("Could not find boot InfiniteLoop spin")
    src = (
        src[:loop_m.start()]
        + "InfiniteLoop:\n"
        + "\tJMP AutomaticallyRunEveryTestInROM\t"
        + "; CUSTOM PATCH: auto-enter target sub-test"
        + src[loop_m.end():]
    )
    return src


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("src_dir", type=Path,
                   help="Cloned upstream AccuracyCoin source directory")
    p.add_argument("--suite", type=int, required=True,
                   help="Suite index in TableTable (see script docstring)")
    p.add_argument("--test", type=int, required=True,
                   help="Test index within the suite (0-based)")
    p.add_argument("--out", type=Path, required=True,
                   help="Output .nes file path")
    p.add_argument("--name", type=str, required=True,
                   help="Human label for the test (for diagnostic only)")
    p.add_argument(
        "--build-dir", type=Path, default=Path("/tmp/accuracycoin-build"),
        help="Scratch directory for patched source + intermediate ROM"
    )
    args = p.parse_args()

    src_asm = args.src_dir / "AccuracyCoin.asm"
    if not src_asm.is_file():
        sys.exit(f"missing {src_asm}")
    nesasm_exe = args.src_dir / "nesasm.exe"
    if not nesasm_exe.is_file():
        sys.exit(f"missing {nesasm_exe}")

    src_text = src_asm.read_text(encoding="utf-8", errors="replace")
    patched = patch_source(src_text, args.suite, args.test)

    args.build_dir.mkdir(parents=True, exist_ok=True)
    # Copy the entire source dir's auxiliary files (palette PCX etc.)
    # into the build dir so nesasm can find them.
    import shutil
    for f in args.src_dir.iterdir():
        if f.is_file() and f.suffix.lower() in {".pcx", ".exe", ".asm"}:
            dest = args.build_dir / f.name
            if f.suffix.lower() == ".asm":
                dest.write_text(patched, encoding="utf-8")
            else:
                shutil.copy2(f, dest)

    # Assemble via wine + nesasm.exe (the upstream toolchain).
    print(f"[build] suite={args.suite} test={args.test} name={args.name}",
          file=sys.stderr)
    cmd = ["wine", str(args.build_dir / "nesasm.exe"), "AccuracyCoin.asm"]
    r = subprocess.run(cmd, cwd=args.build_dir, capture_output=True,
                       text=True, timeout=120)
    if r.returncode != 0:
        print(r.stdout, file=sys.stderr)
        print(r.stderr, file=sys.stderr)
        sys.exit(f"nesasm failed: rc={r.returncode}")

    built = args.build_dir / "AccuracyCoin.nes"
    if not built.is_file():
        sys.exit("nesasm produced no .nes file")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(built.read_bytes())
    print(f"[build] wrote {args.out} ({args.out.stat().st_size} bytes)",
          file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
