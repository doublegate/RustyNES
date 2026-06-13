#!/usr/bin/env python3
"""v2.0 P2 verify (census): walk the cold-boot streams in lockstep, and at every
point where R1's per-instruction cost differs from default's (or the PC stream
forks), record the PC, then RESYNC by finding the next common PC and continue.
Tallies ALL injection PCs across the boot window so the verdict is not based on
the first injection alone:

  all injections at $2002/$2007-poll PCs + interrupt vectors => verdict (a)
  any injection at a $4014 store / DMC-active read           => verdict (b)/(c)

  python3 scripts/instr_boot_census.py <r1_boot.csv> <def_boot.csv>
"""
import csv
import sys
from collections import Counter


def load(path):
    rows = list(csv.DictReader(open(path)))
    return [r["pc"] for r in rows], [int(r["cpu_cycle"]) for r in rows]


def main():
    if len(sys.argv) != 3:
        print("usage: instr_boot_census.py <r1_boot.csv> <def_boot.csv>")
        return 2
    rpc, rcy = load(sys.argv[1])
    dpc, dcy = load(sys.argv[2])
    i = j = 0
    nR, nD = len(rpc), len(dpc)
    cost_inj = Counter()   # PC -> count of per-instruction cost mismatches
    fork = Counter()       # PC (default side) -> count of stream forks (loop len)
    total_C = 0
    while i + 1 < nR and j + 1 < nD:
        if rpc[i] == dpc[j]:
            rcost = rcy[i + 1] - rcy[i]
            dcost = dcy[j + 1] - dcy[j]
            if rcost != dcost:
                cost_inj[rpc[i]] += 1
                total_C += rcost - dcost
            i += 1
            j += 1
        else:
            # Stream fork (a loop iterated a different number of times). Record
            # the forking PC and resync: advance whichever side until PCs match
            # again within a small window.
            fork[dpc[j]] += 1
            resynced = False
            for w in range(1, 40):
                if i + w < nR and rpc[i + w] == dpc[j]:
                    i += w
                    resynced = True
                    break
                if j + w < nD and dpc[j + w] == rpc[i]:
                    j += w
                    resynced = True
                    break
            if not resynced:
                i += 1
                j += 1
    print(f"net cumulative C drift across boot: {total_C:+d} "
          f"({'ODD' if total_C & 1 else 'even'})")
    print(f"\nPER-INSTRUCTION COST-MISMATCH PCs (cycle injected at a matched PC):")
    for pc, n in cost_inj.most_common(20):
        print(f"  {pc}: {n}")
    print(f"\nSTREAM-FORK PCs (loop exited a different # of iterations):")
    for pc, n in fork.most_common(20):
        print(f"  {pc}: {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
