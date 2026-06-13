#!/usr/bin/env python3
"""v2.0 P2 verify-then-build: LOCALIZE where the R1-vs-default per-instruction
cycle-cost divergence is INJECTED, and CLASSIFY its source.

`instr_cycle_diff.py` reports the cumulative offset C and its parity but not
WHERE C changes. This script index-aligns two `trace_instr_cycles` dumps that
are still instruction-stream-aligned (the EARLY window, small C), walks them
PC-by-PC, and reports every instruction whose R1 cycle cost differs from
default's. The PCs of those instructions classify the divergence source:

  (a) interrupt / NMI / PPU-sync code  => C1-entangled, burst MISDIRECTED
  (b) the $4014 OAM-DMA-trigger store  => OAM 513/514 alignment fix
  (c) DMC-active reads                 => the burst premise is correct

Alignment: anchor on the LAST PC that is unique-in-both, then walk BACKWARD in
lockstep as long as the PC sequences agree. Within that agreeing span, per-
instruction cost = cyc[i+1]-cyc[i]; report indices where R1cost != defcost.

  python3 scripts/instr_cost_localize.py <r1.csv> <def.csv>
"""
import csv
import sys
from collections import Counter


def load(path):
    rows = list(csv.DictReader(open(path)))
    pcs = [r["pc"] for r in rows]
    cyc = [int(r["cpu_cycle"]) for r in rows]
    return pcs, cyc


def main():
    if len(sys.argv) != 3:
        print("usage: instr_cost_localize.py <r1.csv> <def.csv>")
        return 2
    rpc, rcy = load(sys.argv[1])
    dpc, dcy = load(sys.argv[2])

    # Find an anchor PC unique in both, near the end, to lock alignment.
    rcount, dcount = Counter(rpc), Counter(dpc)
    anchor = None
    for k in range(len(dpc) - 1, -1, -1):
        pc = dpc[k]
        if dcount[pc] == 1 and rcount.get(pc) == 1:
            anchor = pc
            break
    if anchor is None:
        print("no unique anchor")
        return 1
    ri = rpc.index(anchor)
    di = dpc.index(anchor)
    C_anchor = rcy[ri] - dcy[di]
    print(f"anchor pc={anchor} r_idx={ri} d_idx={di} C={C_anchor} "
          f"({'ODD' if C_anchor & 1 else 'even'})")

    # Walk BACKWARD in lockstep while the PC streams agree.
    agree = 0
    while ri - agree - 1 >= 0 and di - agree - 1 >= 0 \
            and rpc[ri - agree - 1] == dpc[di - agree - 1]:
        agree += 1
    print(f"backward-agreeing span: {agree} instructions "
          f"(r[{ri-agree}..{ri}] d[{di-agree}..{di}])")

    # Per-instruction cost diff across the agreeing span.
    inj = []
    for off in range(agree):
        a_r = ri - off
        a_d = di - off
        if a_r + 1 >= len(rcy) or a_d + 1 >= len(dcy):
            continue
        rcost = rcy[a_r + 1] - rcy[a_r]
        dcost = dcy[a_d + 1] - dcy[a_d]
        if rcost != dcost:
            inj.append((rpc[a_r], rcost, dcost, rcost - dcost))

    if not inj:
        print("NO per-instruction cost divergence in the agreeing span.")
        print("=> the ~C offset was injected BEFORE this window (deep boot).")
        return 0

    by_pc = Counter()
    odd_by_pc = Counter()
    for pc, rc, dc, dd in inj:
        by_pc[pc] += 1
        if dd & 1:
            odd_by_pc[pc] += 1
    print(f"\nINJECTION SITES (instructions where R1cost != defcost): "
          f"{len(inj)} occurrences across {len(by_pc)} PCs")
    print("PC      count  odd-Δ  sample(rcost,dcost,Δ)")
    samp = {}
    for pc, rc, dc, dd in inj:
        samp.setdefault(pc, (rc, dc, dd))
    for pc, n in by_pc.most_common():
        rc, dc, dd = samp[pc]
        print(f"{pc:>5}  {n:>5}  {odd_by_pc[pc]:>5}  ({rc},{dc},{dd:+d})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
