#!/usr/bin/env python3
"""v2.0 R1c / burst-DMA-rewrite cycle-for-cycle validation harness.

Aligns two `trace_instr_cycles` dumps (per-instruction `(PC, cumulative
cpu_cycle)`) on UNIQUE PC anchors and reports the cumulative-cycle offset C =
R1cyc - DEFcyc and its PARITY. The 4-test DMA tail closes iff R1's cumulative
CPU-vs-DMC cycle phase matches default's (C parity EVEN at the CheckDMATiming
region <=> `CheckDMATiming Y = 4`). The interleaved model gives a CONTINUOUS
ODD-leaning divergence; a correct burst-DMA model should drive C's variation
toward 0 / a constant EVEN offset.

NOTE (R1c-1 caveat): do NOT anchor on loop PCs (e.g. the 15-cycle `F0D5` wait
loop) — their per-iteration odd cost makes the parity arbitrary. This script
uses ONLY PCs that occur exactly once in BOTH dumps.

Build a dump with the `cpu-instr-cycle-trace` feature (the R1 master clock is
the only scheduler now):
  cargo run -p nes-test-harness --release \
    --features cpu-instr-cycle-trace,test-roms --bin trace_instr_cycles -- \
    tests/roms/accuracycoin/AccuracyCoin.nes 0477 2000 /tmp/RustyNES_v2/ic.csv

  python3 scripts/instr_cycle_diff.py /tmp/RustyNES_v2/ic.csv <reference.csv>
"""
import csv
import sys
from collections import Counter


def load(path):
    rows = list(csv.DictReader(open(path)))
    return {r["pc"]: int(r["cpu_cycle"]) for r in rows}, Counter(r["pc"] for r in rows)


def main():
    if len(sys.argv) != 3:
        print("usage: instr_cycle_diff.py <r1.csv> <def.csv>")
        return 2
    rmap, rc = load(sys.argv[1])
    dmap, dc = load(sys.argv[2])
    uniq = [pc for pc in dmap if dc[pc] == 1 and rc.get(pc) == 1]
    if not uniq:
        print("no unique-in-both PC anchors")
        return 1
    cs = [(pc, rmap[pc] - dmap[pc]) for pc in uniq]
    cs.sort(key=lambda t: dmap[t[0]])
    par = Counter("ODD" if c & 1 else "even" for _, c in cs)
    vals = [c for _, c in cs]
    print(f"unique anchors: {len(cs)}")
    print(f"C-parity: {dict(par)}")
    print(f"C range: {min(vals)}..{max(vals)}  (distinct values: {len(set(vals))})")
    print("VERDICT:", end=" ")
    if len(set(vals)) == 1 and not (vals[0] & 1):
        print("CONVERGED — constant EVEN offset (burst matches default's cycle phase). Expect Y=4.")
    elif par["ODD"] == 0:
        print("all-even (no odd divergence) — close; check Y directly.")
    else:
        print("DIVERGENT — C varies / odd-leaning (the interleaved model's continuous divergence). Y!=4.")
    print("last 12 anchors (exec order):")
    for pc, c in cs[-12:]:
        print(f"  pc={pc} C={c} ({'ODD' if c & 1 else 'even'})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
