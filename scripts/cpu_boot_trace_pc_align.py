#!/usr/bin/env python3
"""cpu_boot_trace_pc_align.py — PC-aligned cross-diff for binary CPU
boot traces emitted by `scripts/mesen2_cpu_boot_trace.lua` and the
RustyNES `cpu-boot-trace`-gated fixture
(`crates/rustynes-test-harness/tests/cpu_boot_trace_fixture.rs`).

Companion to the `cpu_boot_trace_diff` Rust binary
(`crates/rustynes-test-harness/src/bin/cpu_boot_trace_diff.rs`).  Where
`cpu_boot_trace_diff` aligns by absolute CPU cycle, this script aligns
by PC-subsequence so a small per-emulator phase offset (RustyNES vs
Mesen2 sitting at the same PC but ±1-2 CPU cycles apart) does not
masquerade as instruction-stream divergence.

Session-17 used this script to discover that RustyNES and Mesen2
execute identical PC sequences on `cpu_interrupts_v2/1-cli_latency`
and `mmc3_test_2/4-scanline_timing` in the 250 k-350 k cycle window
post-Session-13 boot alignment, while diverging only on PPU `$2002`
reads inside blargg's `sync_vbl` synchronization routine on the four
FAILING `cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` (post-IRQ-fire)
ROMs.

# Modes

* `--first-divergence` (default): stop after the first PC mismatch.
* `--all-divergences`: walk to end, count + summarize.
* `--cycle-align`: alternate baseline tool — match records at IDENTICAL
  cycle counts (rather than by PC subsequence).  Reproduces the
  `cpu_boot_trace_diff --align-by-cycle` Rust binary at script speed.

# Usage

    python3 scripts/cpu_boot_trace_pc_align.py \
        <ref_path>.bin <actual_path>.bin \
        [--first-divergence|--all-divergences|--cycle-align] \
        [--context N]

The default schema is the v1 layout (12-byte ASCII magic
`RUSTYNES_CPU`, 16-byte header, 32 bytes per record).  See
`crates/rustynes-core/src/cpu_boot_trace.rs` for the canonical decoder.
"""

import struct
import sys
from typing import List, Tuple

BINARY_MAGIC = b"RUSTYNES_CPU"
HEADER_SIZE = 16
RECORD_SIZE = 32


def load(path: str):
    """Parse a binary CPU boot trace into a list of records."""
    with open(path, "rb") as f:
        data = f.read()
    if data[: len(BINARY_MAGIC)] != BINARY_MAGIC:
        raise ValueError(
            f"bad magic in {path}: {data[: len(BINARY_MAGIC)]!r} "
            f"(expected {BINARY_MAGIC!r})"
        )
    recs: List[Tuple] = []
    for i in range(HEADER_SIZE, len(data), RECORD_SIZE):
        rec = data[i : i + RECORD_SIZE]
        if len(rec) < RECORD_SIZE:
            break
        cycle = struct.unpack("<Q", rec[0:8])[0]
        frame = struct.unpack("<I", rec[8:12])[0]
        scanline = struct.unpack("<h", rec[12:14])[0]
        dot = struct.unpack("<H", rec[14:16])[0]
        pc = struct.unpack("<H", rec[16:18])[0]
        a, x, y, p, s, oc, o1, o2, fl = rec[18:27]
        recs.append(
            {
                "cycle": cycle,
                "frame": frame,
                "scan": scanline,
                "dot": dot,
                "pc": pc,
                "a": a,
                "x": x,
                "y": y,
                "p": p,
                "s": s,
                "oc": oc,
                "o1": o1,
                "o2": o2,
                "fl": fl,
            }
        )
    return recs


def find_first_common_pc(ref, act, search_window=10, lookahead=4):
    """Return (ref_start_idx, act_start_idx) of the first PC that
    appears in both within `search_window` records, with consistent
    PC for `lookahead` consecutive records on both sides."""
    for i in range(min(search_window, len(ref))):
        for j in range(min(search_window, len(act))):
            if ref[i]["pc"] != act[j]["pc"]:
                continue
            ok = True
            for k in range(1, lookahead):
                if i + k >= len(ref) or j + k >= len(act):
                    ok = False
                    break
                if ref[i + k]["pc"] != act[j + k]["pc"]:
                    ok = False
                    break
            if ok:
                return i, j
    return 0, 0


def pc_align_walk(ref, act, mode="first-divergence", context=8):
    """Walk ref/act in parallel from the first common-PC index.  In
    `first-divergence` mode, stops at first PC mismatch and prints
    context.  In `all-divergences` mode, counts mismatches + reports
    the first one."""
    ri, ai = find_first_common_pc(ref, act)
    print(
        f"Synchronizing at ref[{ri}] PC=${ref[ri]['pc']:04X} cyc={ref[ri]['cycle']}"
        f"  <->  act[{ai}] PC=${act[ai]['pc']:04X} cyc={act[ai]['cycle']}"
    )
    n = min(len(ref) - ri, len(act) - ai)
    divs = 0
    first_div = None
    for k in range(n):
        rec_r = ref[ri + k]
        rec_a = act[ai + k]
        if rec_r["pc"] != rec_a["pc"]:
            if first_div is None:
                first_div = k
            divs += 1
            if mode == "first-divergence":
                break
            if divs <= 3:
                print(
                    f"  [{k}] ref cyc={rec_r['cycle']} PC=${rec_r['pc']:04X}"
                    f"    act cyc={rec_a['cycle']} PC=${rec_a['pc']:04X}"
                )
    if first_div is None:
        print(f"NO PC DIVERGENCE in {n} parallel-walked instructions")
        return
    k = first_div
    rec_r = ref[ri + k]
    rec_a = act[ai + k]
    print(f"\nFIRST PC DIVERGENCE at parallel-walk idx={k}")
    print(
        f"  ref cyc={rec_r['cycle']} PC=${rec_r['pc']:04X} oc=${rec_r['oc']:02X} "
        f"A=${rec_r['a']:02X} X=${rec_r['x']:02X} Y=${rec_r['y']:02X} "
        f"P=${rec_r['p']:02X} S=${rec_r['s']:02X}  scan={rec_r['scan']} dot={rec_r['dot']}"
    )
    print(
        f"  act cyc={rec_a['cycle']} PC=${rec_a['pc']:04X} oc=${rec_a['oc']:02X} "
        f"A=${rec_a['a']:02X} X=${rec_a['x']:02X} Y=${rec_a['y']:02X} "
        f"P=${rec_a['p']:02X} S=${rec_a['s']:02X}  scan={rec_a['scan']} dot={rec_a['dot']}"
    )
    print(f"  Delta_cycles = act_cyc - ref_cyc = {rec_a['cycle'] - rec_r['cycle']}")
    if mode == "all-divergences":
        print(f"Total divergences in {n} walks: {divs}")
    print()
    print(f"Context: {context} records before, {context} after divergence:")
    lo = max(0, k - context)
    hi = min(n, k + context + 1)
    for i in range(lo, hi):
        rec_r = ref[ri + i]
        rec_a = act[ai + i]
        marker = "  " if rec_r["pc"] == rec_a["pc"] else " *"
        print(
            f"  [{i:>5}] {marker} ref cyc={rec_r['cycle']:<7} PC=${rec_r['pc']:04X} "
            f"oc=${rec_r['oc']:02X} P=${rec_r['p']:02X}    "
            f"act cyc={rec_a['cycle']:<7} PC=${rec_a['pc']:04X} "
            f"oc=${rec_a['oc']:02X} P=${rec_a['p']:02X}"
        )


def cycle_align_walk(ref, act):
    """Compare at identical cycle counts and report PC equality."""
    ref_by_cyc = {r["cycle"]: r for r in ref}
    act_by_cyc = {a["cycle"]: a for a in act}
    common = sorted(set(ref_by_cyc) & set(act_by_cyc))
    print(f"ref={len(ref)} act={len(act)}; records at IDENTICAL cycles: {len(common)}")
    if not common:
        print("(no common-cycle records; emulators have disjoint instruction-boundary cycles)")
        return
    matches = sum(1 for c in common if ref_by_cyc[c]["pc"] == act_by_cyc[c]["pc"])
    print(f"PC-equal at common cycles: {matches}/{len(common)} ({100*matches/len(common):.1f}%)")
    if matches < len(common):
        for c in common:
            r, a = ref_by_cyc[c], act_by_cyc[c]
            if r["pc"] != a["pc"]:
                print(
                    f"  FIRST cycle-aligned PC mismatch @ cyc={c}: "
                    f"ref PC=${r['pc']:04X} oc=${r['oc']:02X} "
                    f"vs act PC=${a['pc']:04X} oc=${a['oc']:02X}"
                )
                break


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(2)
    ref_path = sys.argv[1]
    act_path = sys.argv[2]
    mode = "first-divergence"
    context = 8
    args = sys.argv[3:]
    while args:
        arg = args.pop(0)
        if arg == "--first-divergence":
            mode = "first-divergence"
        elif arg == "--all-divergences":
            mode = "all-divergences"
        elif arg == "--cycle-align":
            mode = "cycle-align"
        elif arg == "--context":
            context = int(args.pop(0))
        else:
            print(f"unknown arg: {arg}", file=sys.stderr)
            sys.exit(2)

    ref = load(ref_path)
    act = load(act_path)
    if mode == "cycle-align":
        cycle_align_walk(ref, act)
    else:
        pc_align_walk(ref, act, mode=mode, context=context)


if __name__ == "__main__":
    main()
