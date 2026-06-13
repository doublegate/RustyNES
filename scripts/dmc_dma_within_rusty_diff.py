#!/usr/bin/env python3
"""Within-RustyNES diff of two `trace_dmc_dma` CSVs (e.g., a baseline vs a
candidate change).

Unlike the cross-emulator cross-diff (`dmc_dma_trace_cross_diff.py`)
that has to wrestle with Mesen2's different `cpu.cycleCount` semantics,
this comparison is cycle-precise because both sides come from
RustyNES and share the same scheduler clock. Used to identify the
exact CPU cycles where adding cycle-2 dummy reads (or any other
opcode-level change) cascades into DMC scheduler behavior.

The two traces are paired by CPU cycle. Aligned events:

  * SAME-CYCLE divergence: same `cpu_cycle` value, different `access`
    / `bus_addr` / `bus_data`. These are the cascade trigger cycles.
  * MISSING events: a cycle present on one side but not the other.

Usage:
    python3 scripts/dmc_dma_within_rusty_diff.py BASELINE.csv VARIANT.csv

Output:
    SUMMARY: total rows, divergent cycles, missing-on-each-side
    DIVERGENT-CYCLE EVENTS: per-cycle delta of bus state
"""
from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path


def load(path: Path):
    rows = []
    with path.open() as f:
        for r in csv.DictReader(f):
            rows.append({
                "cpu_cycle": int(r["cpu_cycle"]),
                "ppu_frame": int(r["ppu_frame"]),
                "ppu_scanline": int(r["ppu_scanline"]),
                "ppu_dot": int(r["ppu_dot"]),
                "m2": r["m2_phase"],
                "access": r["access"],
                "bus_addr": r["bus_addr"],
                "bus_data": r["bus_data"],
                "dmc_pending_pre": int(r["dmc_pending_pre"]),
                "dmc_pending_post": int(r["dmc_pending_post"]),
                "dmc_dma_short": int(r["dmc_dma_short"]),
                "dmc_abort_pending": int(r["dmc_abort_pending"]),
                "dmc_abort_delay": int(r["dmc_abort_delay"]),
                "dmc_cooldown": int(r["dmc_cooldown"]),
                "mapper_irq": int(r["mapper_irq_low"]),
                "apu_irq": int(r["apu_irq_low"]),
            })
    return rows


def fmt(r):
    return (
        f"{r['access']:1}@{r['bus_addr']}={r['bus_data']} "
        f"pre={r['dmc_pending_pre']} post={r['dmc_pending_post']} "
        f"short={r['dmc_dma_short']} abrt={r['dmc_abort_pending']}/"
        f"{r['dmc_abort_delay']} cd={r['dmc_cooldown']} "
        f"airq={r['apu_irq']}"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("baseline", type=Path)
    ap.add_argument("variant", type=Path)
    ap.add_argument("--max-divergent", type=int, default=40,
                    help="rows to print in DIVERGENT-CYCLE list (default 40)")
    ap.add_argument("--start-cycle", type=int, default=0,
                    help="skip cycles before this (default 0)")
    args = ap.parse_args()

    base = load(args.baseline)
    var = load(args.variant)
    print(f"baseline rows: {len(base)}  variant rows: {len(var)}")

    by_cyc_base = {r["cpu_cycle"]: r for r in base if r["cpu_cycle"] >= args.start_cycle}
    by_cyc_var = {r["cpu_cycle"]: r for r in var if r["cpu_cycle"] >= args.start_cycle}

    cycles_base = set(by_cyc_base)
    cycles_var = set(by_cyc_var)
    only_in_base = sorted(cycles_base - cycles_var)
    only_in_var = sorted(cycles_var - cycles_base)
    common = sorted(cycles_base & cycles_var)

    # Find divergent same-cycle rows.
    divergent = []
    for c in common:
        a, b = by_cyc_base[c], by_cyc_var[c]
        keys = ("access", "bus_addr", "bus_data", "dmc_pending_post",
                "dmc_dma_short", "dmc_abort_pending", "dmc_abort_delay",
                "dmc_cooldown", "apu_irq")
        diffs = [k for k in keys if a[k] != b[k]]
        if diffs:
            divergent.append((c, a, b, diffs))

    print()
    print("=== SUMMARY ===")
    print(f"  common cycles:       {len(common)}")
    print(f"  only-in-baseline:    {len(only_in_base)}")
    print(f"  only-in-variant:     {len(only_in_var)}")
    print(f"  divergent same-cyc:  {len(divergent)}")
    if not divergent and not only_in_base and not only_in_var:
        print("  -> traces are byte-identical")
        return 0
    print()
    print(f"=== DIVERGENT SAME-CYCLE EVENTS (first {args.max_divergent}) ===")
    for c, a, b, diffs in divergent[: args.max_divergent]:
        f, s, d, m = a["ppu_frame"], a["ppu_scanline"], a["ppu_dot"], a["m2"]
        print(f"  cyc={c:>10} f={f} sl={s:>3} dot={d:>3} {m}  diffs={','.join(diffs)}")
        print(f"     base: {fmt(a)}")
        print(f"      var: {fmt(b)}")

    if only_in_base:
        print()
        print(f"=== CYCLES ONLY IN BASELINE (first 20) ===")
        for c in only_in_base[:20]:
            a = by_cyc_base[c]
            print(f"  cyc={c}  f={a['ppu_frame']} sl={a['ppu_scanline']} dot={a['ppu_dot']} {a['m2']} {fmt(a)}")

    if only_in_var:
        print()
        print(f"=== CYCLES ONLY IN VARIANT (first 20) ===")
        for c in only_in_var[:20]:
            b = by_cyc_var[c]
            print(f"  cyc={c}  f={b['ppu_frame']} sl={b['ppu_scanline']} dot={b['ppu_dot']} {b['m2']} {fmt(b)}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
