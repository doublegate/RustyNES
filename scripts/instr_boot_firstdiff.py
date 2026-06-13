#!/usr/bin/env python3
"""v2.0 P2 verify: find the FIRST point where R1 diverges from default in the
COLD-BOOT instruction stream (both dumps start at idx 0 / the reset vector and
are initially identical). Reports whether the first divergence is a CYCLE-COST
difference at a matching PC (interrupt/DMA stall accounted differently) or a
STREAM-LENGTH difference (a PPU-phase-dependent poll loop iterating a different
number of times). The PC classifies the source:

  $2002 / $2007 poll-loop PC  => PPU-phase timing (C1/PPU-entangled; verdict a)
  NMI/IRQ vector or BRK        => interrupt timing (verdict a)
  $4014 store / OAM stall      => OAM alignment (verdict b)
  DMC-active read              => DMC DMA (verdict c)

  python3 scripts/instr_boot_firstdiff.py <r1_boot.csv> <def_boot.csv>
"""
import csv
import sys


def load(path):
    rows = list(csv.DictReader(open(path)))
    return [r["pc"] for r in rows], [int(r["cpu_cycle"]) for r in rows]


def main():
    if len(sys.argv) != 3:
        print("usage: instr_boot_firstdiff.py <r1_boot.csv> <def_boot.csv>")
        return 2
    rpc, rcy = load(sys.argv[1])
    dpc, dcy = load(sys.argv[2])
    n = min(len(rpc), len(dpc))

    # Normalize each to start at cycle 0 (both should already, but be safe).
    r0, d0 = rcy[0], dcy[0]
    rcy = [c - r0 for c in rcy]
    dcy = [c - d0 for c in dcy]

    print(f"r len={len(rpc)} d len={len(dpc)}; first PCs r={rpc[0]} d={dpc[0]}")
    # Walk forward in lockstep; report the first index where PC OR cumulative
    # cycle differs.
    first_cyc_diff = None
    first_pc_diff = None
    for i in range(n):
        if rcy[i] != dcy[i] and first_cyc_diff is None:
            first_cyc_diff = i
        if rpc[i] != dpc[i]:
            first_pc_diff = i
            break

    if first_cyc_diff is not None:
        i = first_cyc_diff
        lo = max(0, i - 4)
        print(f"\nFIRST CUMULATIVE-CYCLE divergence at idx {i}: "
              f"C={rcy[i]-dcy[i]:+d} ({'ODD' if (rcy[i]-dcy[i]) & 1 else 'even'})")
        print("context (idx: R_pc R_cyc | D_pc D_cyc | per-instr R_cost D_cost):")
        for j in range(lo, min(i + 4, n - 1)):
            rcost = rcy[j + 1] - rcy[j]
            dcost = dcy[j + 1] - dcy[j]
            mark = "  <-- inject" if rcost != dcost else ""
            print(f"  {j}: {rpc[j]} {rcy[j]} | {dpc[j]} {dcy[j]} | "
                  f"{rcost} {dcost}{mark}")
    else:
        print("\nNo cumulative-cycle divergence in the lockstep prefix.")

    if first_pc_diff is not None:
        i = first_pc_diff
        lo = max(0, i - 3)
        print(f"\nFIRST PC-STREAM divergence at idx {i} "
              f"(streams take different paths here):")
        for j in range(lo, min(i + 3, n)):
            print(f"  {j}: R={rpc[j]}@{rcy[j]} | D={dpc[j]}@{dcy[j]}")
    else:
        print("\nPC streams identical across the lockstep prefix.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
