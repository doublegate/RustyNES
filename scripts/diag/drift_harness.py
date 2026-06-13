#!/usr/bin/env python3
"""Automated default-vs-R1 per-cycle drift harness.

Aligns two trace_dma_4015 CSVs on the CycleClockBegin landmark ($DA1E then
W $4010=$4E), then walks both CPU-access (R/W) streams forward in lockstep.
The CPU instruction streams are identical code, so the (addr,data) sequences
must match; the cpu_cycle DELTA between matched accesses is constant until the
DMA-cycle accounting drifts. Reports the first access where the delta changes
= the exact cycle where R1's master-clock accounting diverges from default's.

Usage: drift_harness.py <default.csv> <r1.csv>
"""
import csv, sys

def load(fn):
    rows = []
    with open(fn) as f:
        r = csv.reader(f); next(r)
        for row in r:
            if len(row) < 13:
                continue
            rows.append((int(row[0]), row[4], row[5], row[6]))  # cyc,access,addr,data
    return rows

def find_anchor(rows):
    # W $4010=$4E preceded by a read of $DA1E (CycleClockBegin's STA $4010)
    for i in range(1, len(rows)):
        if rows[i][1] == 'W' and rows[i][2] == '$4010' and rows[i][3] == '$4E' \
           and rows[i-1][2] == '$DA1E':
            return i
    return None

def cpu_accesses(rows, start):
    # CPU-visible accesses only (R/W); skip DMA (r/w) + idle (I).
    out = []
    for i in range(start, len(rows)):
        acc = rows[i][1]
        if acc in ('R', 'W'):
            out.append(rows[i])
    return out

def main():
    de = load(sys.argv[1])
    r1 = load(sys.argv[2])
    da = find_anchor(de); ra = find_anchor(r1)
    print(f"default anchor idx={da} cyc={de[da][0] if da else None}")
    print(f"r1      anchor idx={ra} cyc={r1[ra][0] if ra else None}")
    if da is None or ra is None:
        print("ANCHOR MISSING in one stream — recapture needed")
        return
    dca = cpu_accesses(de, da)
    rca = cpu_accesses(r1, ra)
    d0 = dca[0][0]; r0 = rca[0][0]
    base_delta = r0 - d0
    print(f"base cyc delta (r1-default) at anchor = {base_delta}")
    print("walking CPU-access streams; reporting (addr,data) mismatches + cyc-delta changes:")
    n = min(len(dca), len(rca))
    prev_delta = base_delta
    diffs = 0
    for k in range(n):
        dc, da_acc, da_addr, da_data = dca[k]
        rc, ra_acc, ra_addr, ra_data = rca[k]
        delta = rc - dc
        # sequence divergence (code mismatch — should not happen until drift desyncs)
        if (da_addr, da_data, da_acc) != (ra_addr, ra_data, ra_acc):
            print(f"  [k={k}] SEQ MISMATCH default {da_acc} {da_addr}={da_data} (cyc {dc}) | "
                  f"r1 {ra_acc} {ra_addr}={ra_data} (cyc {rc}); delta={delta}")
            diffs += 1
            if diffs >= 12:
                print("  ... (capped)")
                break
            continue
        # cycle-delta change (the drift) on a matched access
        if delta != prev_delta:
            print(f"  [k={k}] DELTA CHANGE {prev_delta}->{delta} at "
                  f"{da_acc} {da_addr}={da_data} (default cyc {dc}, r1 cyc {rc}) "
                  f"= +{delta-prev_delta} cyc R1 drift here")
            prev_delta = delta
            diffs += 1
            if diffs >= 12:
                print("  ... (capped)")
                break
    print(f"done: {diffs} divergences in first {n} matched CPU accesses; final delta={prev_delta}")

main()
