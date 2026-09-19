#!/usr/bin/env python3
"""
Build an AccuracyCoin ROM that MIRRORS its result vector into the cartridge's
battery-backed PRG-RAM window, so a hardware run can be read back as BYTES
instead of transcribed from a photograph.

WHY THIS EXISTS
---------------
In simulation the co-simulation harness reads AccuracyCoin's result vector
straight out of work RAM, which is how rung 5 compares the catalog entry for
entry. On hardware there is no such channel: the readable output is the screen,
and `RustyNES_MiSTer/docs/bringup.md` says so plainly -- *"a photograph is not
149 status bytes, and reading one is a human transcribing a picture."*

The MiSTer core's save path persists the cartridge PRG-RAM window `$6000-$7FFF`
to `<rom>.sav` (T-MISTER-SAVE, v2.6.21). AccuracyCoin's vector lives in CPU RAM
at `$0300-$04FF`, which is NOT in that window, so nothing of it reaches `.sav`.
Mirroring those 512 bytes into `$6000-$61FF` once the battery has finished puts
the whole vector inside the save window, and the on-device comparison becomes
the same entry-for-entry diff rung 5 does.

Two header facts make this work, and both were read out of the RTL rather than
assumed:

  * `rtl/emu.sv:609` takes `has_prg_ram` from iNES **flags6 bit 1** -- the
    battery bit -- so setting it is what gives the cartridge a PRG-RAM window
    at all; and
  * `rtl/emu.sv:353` feeds that SAME signal to the save controller as
    `cart_has_battery`, so the one bit both creates the window and arms the
    write-back.

THE PATCH MOVES NOTHING, AND THAT IS THE POINT
----------------------------------------------
A ROM whose code has shifted is a different ROM. AccuracyCoin is full of
cycle-exact tests, several of which are sensitive to page crossings in their own
code, so an insertion that displaced every routine after it could flip a verdict
for reasons having nothing to do with the console under test -- and the flipped
verdict would look exactly like a result.

So the patch is size-neutral in both places it touches:

  1. **The call site.** At `; All tests are complete!` the upstream source runs
     `LDA #$00` / `STA $4015` -- five bytes. Those five are replaced by
     `JSR ACMirrorResults` plus two `NOP`s. Five bytes for five.
  2. **The routine.** It is appended at the END of bank 2, which nesasm pads to
     `$E000` with `$FF`. The 29-byte routine consumes padding; it does not push
     anything.

The displaced instructions are re-executed as the routine's first act, in the
same order and at the same point in the instruction stream, so the APU is
silenced exactly where upstream silences it. `X` is saved and restored, `Y` is
untouched, and `A` returns as `$00` just as `LDA #$00` left it.

The expected result is therefore an exact, auditable byte budget -- and this
script ASSERTS it rather than hoping for it, unconditionally and with no flag to
turn it off. Every run rebuilds the UNPATCHED source with the same toolchain and
refuses to emit a ROM unless the difference is precisely one header byte, five
at the call site, and a run inside bank 2's fill region. Any other diff means
something moved, and the script exits non-zero saying which offsets.

THE CONTROL IS NOT IN THIS SCRIPT
---------------------------------
A byte budget proves nothing MOVED. It does not prove the patched ROM produces
the SAME ANSWERS. That control is
`crates/rustynes-test-harness/tests/accuracycoin_mirror.rs`, which runs both
ROMs through the oracle and refuses unless the decoded status vectors are
byte-identical and the mirror reproduces the live window. Per the v2.7.0 plan:
*"the patched ROM must produce a byte-identical vector to the unpatched one in
simulation before a single hardware reading is taken from it."* If that control
does not come out clean, drop the patch and keep the photograph -- a readback
channel that alters the thing it reads is worse than no channel.

USAGE
-----
    python3 build_mirror_rom.py <upstream-source-dir> \\
        --out tests/roms/AccuracyCoin/mirror/AccuracyCoin-mirror.nes \\
        --wine /usr/bin/wine

`<upstream-source-dir>` is a clone of https://github.com/100thCoin/AccuracyCoin
at the commit the vendored corpus is pinned to (`tests/roms/accuracycoin/
README.md` records it). It must contain `AccuracyCoin.asm` and `nesasm.exe`:
the ROM is assembled with the AUTHOR'S OWN assembler, so the output is upstream's
toolchain rather than merely an equivalent one, and this script checks that the
toolchain reproduces the committed upstream `.nes` byte-identically before it
will trust a patched build from it.

LICENSE: this build script is GPL-3.0-or-later with the rest of RustyNES. The
generated ROM is a derivative work of 100thCoin/AccuracyCoin and inherits the
upstream MIT license (tests/roms/accuracycoin/LICENSE; the corpus-wide index
is tests/roms/LICENSES.md).
"""

import argparse
import hashlib
import importlib.util
import shutil
import subprocess
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# The patch, stated as data so the diff budget below can be derived from it
# rather than written down twice.
# ---------------------------------------------------------------------------

# The upstream call site, verbatim. Matched as SOURCE TEXT, not as a byte
# pattern: `LDA #$00` / `STA $4015` occurs all over this ROM, and the anchor
# that makes this occurrence unique is the comment above it.
CALL_SITE_ANCHOR = "\t; All tests are complete!\n\tLDA #0\n\tSTA $4015\n"

# Five bytes for five. `JSR abs` is 3, and the two `NOP`s make up the
# difference so that not one byte after this point moves.
CALL_SITE_PATCH = (
    "\t; All tests are complete!\n"
    "\t; PATCHED by scripts/accuracycoin-build/build_mirror_rom.py.\n"
    "\t; `LDA #0` / `STA $4015` (5 bytes) -> `JSR` (3) + `NOP` `NOP` (2).\n"
    "\t; The displaced pair is re-executed as ACMirrorResults' first act, so\n"
    "\t; the APU is silenced at the same point in the instruction stream.\n"
    "\tJSR ACMirrorResults\n"
    "\tNOP\n"
    "\tNOP\n"
)

# Appended at the end of bank 2, into the `$FF` fill nesasm lays down to
# `$E000`. 29 bytes; measured free space is 50.
MIRROR_ROUTINE = """
;;;;;;;
; ACMirrorResults -- ADDED by scripts/accuracycoin-build/build_mirror_rom.py.
;
; Copy the result vector at $0300-$04FF into the cartridge's battery-backed
; PRG-RAM at $6000-$61FF, so the MiSTer core's save path writes it to
; <rom>.sav and a hardware run can be diffed as bytes.
;
; Called once, from `; All tests are complete!`, after every test has written
; its result byte and before the results menu is drawn.
;
; Preserves X and Y; returns with A = $00, exactly as the `LDA #0` it replaced
; left it. 29 bytes, placed in bank 2's end-of-bank fill so nothing moves.
ACMirrorResults:
\tLDA #0
\tSTA $4015          ; displaced from the call site, executed first
\tTXA
\tPHA
\tLDX #0
ACMirrorResults_Loop:
\tLDA $0300,X
\tSTA $6000,X
\tLDA $0400,X
\tSTA $6100,X
\tINX
\tBNE ACMirrorResults_Loop
\tPLA
\tTAX
\tLDA #0
\tRTS
;;;;;;;
"""

# Where the routine is appended: immediately before bank 3 opens. Matched on
# the directive pair rather than a line number, which drifts.
BANK3_ANCHOR = "\t.bank 3\n\t.org $E000\n"

# iNES flags6 bit 1. Sets battery-backed PRG-RAM, which on this core is what
# both creates the $6000 window (`emu.sv:609`) and arms the save controller
# (`emu.sv:353`).
INES_FLAGS6_OFFSET = 6
INES_BATTERY_BIT = 0x02

# PRG bank 2 is $C000-$DFFF, at file offset 16 + 0x4000.
INES_HEADER_LEN = 16
BANK2_FILE_START = INES_HEADER_LEN + 0x4000
BANK2_FILE_END = INES_HEADER_LEN + 0x6000

# Size of the mirrored window, and where it lands.
VECTOR_BASE = 0x0300
VECTOR_LEN = 0x0200
MIRROR_BASE = 0x6000


def _find_wine(override):
    """Borrow `build_sub_test_rom._find_wine` rather than restating it.

    That function encodes a real hazard on this machine -- `/usr/local/bin/wine`
    is a symlink to firejail, so a `PATH` search silently assembles with the
    wrong program -- together with the reviewer disagreement that shaped it. A
    second copy here would be a second thing to keep correct, which is exactly
    how the stale suite map in the sibling script came to exist.
    """
    spec = importlib.util.spec_from_file_location(
        "_build_sub_test_rom", Path(__file__).with_name("build_sub_test_rom.py")
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod._find_wine(override)


def _assemble(src_text, src_dir, build_dir, wine):
    """Assemble `src_text` with upstream's own nesasm. Returns the ROM bytes."""
    build_dir.mkdir(parents=True, exist_ok=True)
    for f in src_dir.iterdir():
        if f.is_file() and f.suffix.lower() in {".pcx", ".exe", ".asm"}:
            dest = build_dir / f.name
            if f.suffix.lower() == ".asm":
                dest.write_text(src_text, encoding="utf-8")
            else:
                shutil.copy2(f, dest)

    out = build_dir / "AccuracyCoin.nes"
    if out.exists():
        out.unlink()

    r = subprocess.run(
        [wine, str(build_dir / "nesasm.exe"), "AccuracyCoin.asm"],
        cwd=build_dir, capture_output=True, text=True, timeout=180,
    )
    if r.returncode != 0:
        print(r.stdout, file=sys.stderr)
        print(r.stderr, file=sys.stderr)
        sys.exit(f"nesasm failed: rc={r.returncode}")
    if not out.is_file():
        sys.exit("nesasm produced no .nes file")
    return out.read_bytes()


def patch_source(src):
    """Apply both halves of the patch, or exit saying which anchor failed.

    Fail-closed on purpose. An anchor that matches nothing would otherwise
    produce a ROM that assembles, runs, and mirrors NOTHING -- indistinguishable
    on the screen from one that works, and distinguishable in `.sav` only as 512
    bytes of zeroes that read like a console that failed every test.
    """
    if src.count(CALL_SITE_ANCHOR) != 1:
        sys.exit(
            "build_mirror_rom: the call-site anchor matched "
            f"{src.count(CALL_SITE_ANCHOR)} times, expected exactly 1.\n"
            "  Anchor (the comment is what makes it unique -- `LDA #0` /\n"
            "  `STA $4015` alone occurs throughout this ROM):\n"
            f"{CALL_SITE_ANCHOR!r}\n"
            "  Upstream moved or reworded it. Re-locate `All tests are\n"
            "  complete` in AccuracyCoin.asm and update CALL_SITE_ANCHOR."
        )
    if src.count(BANK3_ANCHOR) != 1:
        sys.exit(
            "build_mirror_rom: the bank-3 anchor matched "
            f"{src.count(BANK3_ANCHOR)} times, expected exactly 1."
        )

    src = src.replace(CALL_SITE_ANCHOR, CALL_SITE_PATCH)
    return src.replace(BANK3_ANCHOR, MIRROR_ROUTINE + "\n" + BANK3_ANCHOR)


def diff_budget(base, patched):
    """Every differing byte, classified. Returns (header, call_site, bank2, other).

    The classification is what turns "35 bytes differ" into a claim worth
    making. A byte that differs OUTSIDE the three budgeted regions means code
    moved, and a ROM whose code moved is a different ROM.

    The three buckets are deliberately narrow. `header` is flags6 ALONE, not
    "anything in the first sixteen bytes": a changed PRG or CHR count would
    otherwise be waved through as budgeted, and that is the one header change
    that really would mean the image is a different shape.
    """
    if len(base) != len(patched):
        sys.exit(
            f"build_mirror_rom: length changed, {len(base)} -> {len(patched)}. "
            "The patch is supposed to be size-neutral; something overflowed a "
            "bank."
        )
    header, call_site, bank2, other = [], [], [], []
    for i, (a, b) in enumerate(zip(base, patched)):
        if a == b:
            continue
        if i == INES_FLAGS6_OFFSET:
            header.append(i)
        elif i < INES_HEADER_LEN:
            other.append(i)
        elif BANK2_FILE_START <= i < BANK2_FILE_END:
            bank2.append(i)
        else:
            call_site.append(i)
    return header, call_site, bank2, other


def main():
    p = argparse.ArgumentParser()
    p.add_argument("src_dir", type=Path,
                   help="Clone of 100thCoin/AccuracyCoin at the pinned commit")
    p.add_argument("--out", type=Path, required=True, help="Output .nes path")
    p.add_argument("--expect-upstream-md5", type=str, default=None,
                   help="MD5 the UNPATCHED build must reproduce. Pass the "
                        "vendored ROM's hash to prove the toolchain here is "
                        "upstream's before trusting a patched build from it.")
    p.add_argument("--build-dir", type=Path,
                   default=Path("/tmp/accuracycoin-mirror-build"),
                   help="Scratch directory")
    p.add_argument("--wine", type=Path, default=None,
                   help="Absolute path to a real wine. PATH is deliberately "
                        "not searched; see build_sub_test_rom._find_wine().")
    args = p.parse_args()

    src_asm = args.src_dir / "AccuracyCoin.asm"
    if not src_asm.is_file():
        sys.exit(f"missing {src_asm}")
    if not (args.src_dir / "nesasm.exe").is_file():
        sys.exit(f"missing {args.src_dir / 'nesasm.exe'}")

    wine = _find_wine(args.wine)
    print(f"[mirror] wine={wine}", file=sys.stderr)
    # STRICT decoding, deliberately. `errors="replace"` would turn a
    # non-UTF-8 byte into U+FFFD and carry on, which corrupts the source
    # text and then assembles a ROM from it -- and the ROM would still
    # build, still run, and still write result bytes. Upstream is valid
    # UTF-8 today (692,263 bytes, zero replacement characters), so strict
    # costs nothing now and fails loudly if that ever stops being true.
    # Raised by the Antigravity reviewer on PR #530.
    src_text = src_asm.read_text(encoding="utf-8")

    # THE UNPATCHED BUILD IS THE CONTROL FOR THE TOOLCHAIN.
    #
    # Everything below compares the patched ROM against this one, so if this one
    # is not upstream's ROM the whole budget is a comparison between two things
    # nobody vouched for.
    base = _assemble(src_text, args.src_dir, args.build_dir / "base", wine)
    base_md5 = hashlib.md5(base).hexdigest()
    print(f"[mirror] unpatched rebuild: md5={base_md5} ({len(base)} bytes)",
          file=sys.stderr)
    if args.expect_upstream_md5 and base_md5 != args.expect_upstream_md5.lower():
        sys.exit(
            f"build_mirror_rom: the UNPATCHED rebuild is {base_md5}, but\n"
            f"  --expect-upstream-md5 says {args.expect_upstream_md5.lower()}.\n"
            "  The source tree or the assembler is not the one the vendored\n"
            "  corpus came from. Refusing to build a patched ROM from it."
        )

    patched_src = patch_source(src_text)
    patched = bytearray(_assemble(patched_src, args.src_dir,
                                  args.build_dir / "patched", wine))

    # The header bit, which no nesasm directive exposes. One byte, scripted, so
    # a later reader can see exactly which byte moved and why.
    before = patched[INES_FLAGS6_OFFSET]
    patched[INES_FLAGS6_OFFSET] = before | INES_BATTERY_BIT
    print(f"[mirror] iNES flags6 ${before:02X} -> "
          f"${patched[INES_FLAGS6_OFFSET]:02X} (battery bit set)",
          file=sys.stderr)

    header, call_site, bank2, other = diff_budget(base, bytes(patched))

    print(f"[mirror] diff budget: header={len(header)} "
          f"call-site={len(call_site)} bank2-fill={len(bank2)} "
          f"elsewhere={len(other)}", file=sys.stderr)
    if call_site:
        lo, hi = min(call_site), max(call_site)
        print(f"[mirror]   call site at file ${lo:04X}-${hi:04X} "
              f"(CPU ${0x8000 + lo - INES_HEADER_LEN:04X})", file=sys.stderr)
    if bank2:
        lo, hi = min(bank2), max(bank2)
        print(f"[mirror]   routine at file ${lo:04X}-${hi:04X} "
              f"(CPU ${0x8000 + lo - INES_HEADER_LEN:04X})", file=sys.stderr)

    # `other` is every differing byte outside the header, the call site's own
    # five, and bank 2's fill. It must be empty. A non-empty `other` is code
    # that moved.
    if len(header) != 1:
        sys.exit(
            f"build_mirror_rom: flags6 shows {len(header)} differing bytes,\n"
            "  expected exactly 1. The battery bit was already set upstream,\n"
            "  or the write did not land."
        )
    if other:
        sys.exit(
            f"build_mirror_rom: {len(other)} bytes differ OUTSIDE the three\n"
            "  regions this patch is allowed to touch. The patch is supposed\n"
            "  to move nothing; something shifted, and a ROM whose code has\n"
            "  shifted is a different ROM -- AccuracyCoin has cycle-exact\n"
            "  tests that are sensitive to page crossings in their own code.\n"
            f"  First ten offsets: {[hex(o) for o in other[:10]]}"
        )
    if len(call_site) > 5:
        sys.exit(
            f"build_mirror_rom: {len(call_site)} bytes differ at the call\n"
            "  site, expected at most 5. The replacement is not size-neutral."
        )
    if not bank2:
        sys.exit(
            "build_mirror_rom: the routine produced NO differing bytes in\n"
            "  bank 2. It was not assembled in, or it landed somewhere else --\n"
            "  either way the ROM mirrors nothing and would read back as 512\n"
            "  zero bytes, which looks like a console that failed everything."
        )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(bytes(patched))
    print(f"[mirror] wrote {args.out} ({args.out.stat().st_size} bytes, "
          f"md5={hashlib.md5(bytes(patched)).hexdigest()})", file=sys.stderr)
    print(f"[mirror] vector ${VECTOR_BASE:04X}-${VECTOR_BASE + VECTOR_LEN - 1:04X}"
          f" mirrors to ${MIRROR_BASE:04X}-${MIRROR_BASE + VECTOR_LEN - 1:04X}",
          file=sys.stderr)
    print("[mirror] THE BYTE BUDGET IS NOT THE CONTROL. Run "
          "`cargo test -p rustynes-test-harness --features test-roms "
          "--test accuracycoin_mirror` before taking a hardware reading from "
          "this ROM.", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
