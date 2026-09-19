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

DO NOT USE A HAND-WRITTEN SUITE MAP. Use `derive_indices.py`, which reads
`TableTable` and each suite's own `table` lines out of the assembly and
VALIDATES itself against two independently recorded answers before reporting
any others.

The map that used to sit here was WRONG from index 14 onward, and it is kept
below struck through because it was acted on. It listed twenty suites and
stopped at 19, with `PowerOnState` at 14 and `CPUBehavior2` at 19. Upstream's
`TableTable` at 46199ae4 has TWENTY-TWO suites, `CPUBehavior2` at **14** and
`PPUMisc` at **19** -- the suites were reordered upstream at some point after
the map was written. A rebuild driven by the stale map enters the WRONG SUITE
and the ROM writes a plausible byte for a test nobody asked for, which is the
worst failure available here because it looks like a result.

    STALE, DO NOT USE:
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

Current, derived from 46199ae4 (22 suites, 149 catalog entries):
   0: CPUBehavior              1: CPUInstructions
   2: UnofficialOps_SLO        3: UnofficialOps_RLA
   4: UnofficialOps_SRE        5: UnofficialOps_RRA
   6: UnofficialOps__AX        7: UnofficialOps_DCP
   8: UnofficialOps_ISC        9: UnofficialOps_SH_
  10: UnofficialOps_Immediates 11: CPUInterrupts
  12: DMATests                13: APUTiming
  14: CPUBehavior2            15: PowerOnState
  16: PPUBehavior             17: PPUTiming
  18: SpriteZeroHits          19: PPUMisc
  20: AdvancedBGEval          21: AdvancedSpriteEval

Targets, RE-DERIVED at v2.6.21. Three of the four recorded here were correct
and one was not:
- Controller Strobing:     suite=13, test=7 ($045F)  -- was right
- Frame Counter IRQ:       suite=13, test=2 ($0467)  -- was right
- APU Register Activation: suite=13, test=6 ($045C)  -- was right
- Implied Dummy Reads:     suite=**14**, test=1 ($046D) -- the recorded
  `suite=19` is WRONG; 19/1 is `Address $2004 behavior` ($045B). They survived
  in suite 13 because the reordering began at 14.

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
import os
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


def _probe_wine(cand: str) -> "str | None":
    """`None` if `cand` is a usable wine, else a short reason why not.

    Requires the version line to START with "wine" rather than merely contain
    it: a wrapper announcing "firejail wrapper for wine" would pass a substring
    test, and the firejail shim that motivated all of this prints
    "firejail version 0.9.80".
    """
    path = Path(cand)
    if not path.is_absolute():
        return "not an absolute path"
    if not path.exists():
        return "does not exist"
    try:
        probe = subprocess.run([cand, "--version"], capture_output=True,
                               text=True, timeout=30)
    except (OSError, subprocess.SubprocessError) as exc:
        return f"could not be run ({exc})"
    for stream in (probe.stdout, probe.stderr):
        if re.match(r"^\s*wine[- ]", stream, re.IGNORECASE):
            return None
    got = (probe.stdout + probe.stderr).strip().splitlines()
    return f"did not identify as wine (said {got[0]!r})" if got else "printed no version"


def _find_wine(override: "Path | None" = None) -> str:
    """An absolute path to a real wine, or exit with why.

    NO IMPLICIT `PATH` SEARCH. `shutil.which("wine")` is exactly the hazard this
    function exists to avoid -- on this machine `/usr/local/bin/wine` is a
    symlink to `/usr/bin/firejail`, and a `PATH` entry can be relative or a
    wrapper that we would then EXECUTE while probing (CWE-426, untrusted search
    path). So the candidate set is a fixed allowlist of absolute paths plus one
    explicitly-trusted override, which is the project's stated preference for
    allowlists over denylists at a boundary.

    Reviewers disagreed here and the disagreement is recorded because both were
    reasonable: one asked for the `PATH` result to be tried FIRST so a developer
    with a custom wine keeps their override, the other asked for `PATH` to be
    dropped entirely as an untrusted search path. Trying `PATH` first is
    precisely backwards for this script, whose whole reason to exist is a `PATH`
    entry shadowing the real binary -- but the need behind it is real, so it is
    served by `--wine` / `RUSTYNES_WINE`, an override the operator sets
    deliberately rather than one inherited from the environment.
    """
    # An EXPLICIT override is checked alone and fails hard. Falling through to a
    # default when the operator named a binary would silently build with
    # something other than what they asked for -- found by a negative control
    # here, where `--wine /usr/bin/firejail` quietly used `/usr/bin/wine`.
    explicit = str(override) if override is not None else os.environ.get("RUSTYNES_WINE")
    if explicit:
        why = _probe_wine(explicit)
        if why is None:
            return explicit
        sys.exit(f"--wine/RUSTYNES_WINE={explicit!r} is not usable: {why}")

    candidates = ["/usr/bin/wine", "/usr/bin/wine64", "/opt/wine-stable/bin/wine"]

    # Deduplicate, preserving order, so the failure message cannot list a path
    # twice when the override happens to equal a default.
    seen: set[str] = set()
    ordered = [c for c in candidates if not (c in seen or seen.add(c))]

    for cand in ordered:
        if _probe_wine(cand) is None:
            return cand
    sys.exit(
        "no working wine found (tried: " + ", ".join(ordered) + "). "
        "PATH is deliberately NOT searched -- a `wine` there may be a sandbox "
        "wrapper shadowing the real binary. Pass --wine /abs/path or set "
        "RUSTYNES_WINE to an absolute path for an installation outside those."
    )


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
    p.add_argument(
        "--wine", type=Path, default=None,
        help="Absolute path to a real wine binary. PATH is deliberately not "
             "searched; see _find_wine(). RUSTYNES_WINE does the same."
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

    # Assemble via wine + nesasm.exe (the upstream toolchain, so the output is
    # the author's own assembler rather than merely an equivalent one).
    #
    # Resolve wine to an ABSOLUTE path rather than trusting `PATH`. On this
    # machine `/usr/local/bin/wine` is a symlink to `/usr/bin/firejail`, which
    # shadows the real binary: a bare `wine --version` prints
    # `firejail version 0.9.80` and the assemble silently runs the wrong
    # program. Prefer the first entry that actually reports a wine version.
    print(f"[build] suite={args.suite} test={args.test} name={args.name}",
          file=sys.stderr)
    wine = _find_wine(args.wine)
    print(f"[build] wine={wine}", file=sys.stderr)
    cmd = [wine, str(args.build_dir / "nesasm.exe"), "AccuracyCoin.asm"]
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
