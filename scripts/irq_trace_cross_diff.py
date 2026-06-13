#!/usr/bin/env python3
"""Cross-diff a RustyNES IRQ trace CSV against a Mesen2 IRQ trace CSV.

The two emulators emit different schemas:

* RustyNES (`crates/rustynes-test-harness/golden/irq_trace/<rom>.csv`) -- emits
  rows ONLY at IRQ-line / NMI / A12 / DMC-scheduler / bus-access
  state transitions.  Session-21 schema:
    cpu_cycle, ppu_frame, ppu_scanline, ppu_dot,
    irq_pending_mapper_at_low, irq_pending_apu_at_low,
    irq_pending_mapper_at_high, irq_pending_apu_at_high,
    nmi_line, a12_events,
    dmc_dma_pending_pre, dmc_dma_pending_post, dmc_dma_short_post,
    dmc_abort_pending_post, dmc_abort_delay_post, dmc_dma_cooldown_post,
    dmc_dma_delay_post, apu_phase_post, in_dmc_dma, dma_cycles_owed,
    bus_access, bus_addr, bus_data

  Pre-Session-21 (10-column) traces are still loadable; the DMC and
  bus-access columns default to absent / zero for those rows.

* Mesen2 (`crates/rustynes-test-harness/golden/irq_trace/mesen2/<rom>.csv`,
  produced by `scripts/mesen2_irq_trace.lua`) -- emits rows at
  per-instruction-boundary edge detection AND at irq_svc / nmi_svc
  callback fires.  Session-21 schema:
    cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, event_type, pc,
    apu_irq_flag, nmi_flag, dmc_irq_flag, dmc_irq_en, dmc_bytes_rem,
    dmc_sample_addr

  Pre-Session-21 (8-column) traces are still loadable; the DMC columns
  default to absent / zero.

This tool computes:

  * First mapper-IRQ-assertion cycle (RustyNES side: first row where
    irq_pending_mapper_at_high transitions 0->1).
  * First Mesen2 irq_svc cycle (the analogue: when the CPU vector-
    fetches the IRQ).
  * Cycle delta + scanline / dot context for each side's first event.
  * Total event counts on each side.
  * For mmc3_test_2/4: extracts the scanline / dot of the first
    mapper-IRQ event since that is the architectural-property check
    (Phase B4 invariant: must be scanline 0, NOT pre-render).

Usage:
    python3 scripts/irq_trace_cross_diff.py <rustynes.csv> <mesen2.csv>
    python3 scripts/irq_trace_cross_diff.py --svc <rustynes.svc.csv> <mesen2.csv>
    python3 scripts/irq_trace_cross_diff.py --dmc <rustynes.csv> <mesen2.csv>

The `--svc` form added in Phase 1.2 of Track C1 attempt 14 uses the new
RustyNES vector-fetch event sidecar (`.svc.csv`) for a direct
service-axis cross-diff with Mesen2's `irq_svc` / `nmi_svc` rows.  No
more state-transition vs vector-fetch schema asymmetry.

The `--dmc` form (Session-21, Sprint 1 iteration 2 prereq) focuses on
DMC scheduler events: it reports each side's DMC IRQ assertions, DMA
activity windows, and (RustyNES-side only since Mesen2's Lua doesn't
expose them directly) the abort-delay / cooldown / pending state at
each transition.  Use this when diagnosing the Implied Dummy Read +
DMC scheduler calibration mismatch that rolled back Sprint 1
iteration 1 (Session-19 + 20).

Prints a structured report; exits 0 always.
"""
from __future__ import annotations

import csv
import sys
from collections import Counter
from pathlib import Path


def load_rustynes(path: Path):
    """Parse a RustyNES IRQ trace CSV into a list of dicts.

    Schema-tolerant: pre-Session-21 (10-column) and Session-21+ (23-
    column) traces both load; the optional DMC + bus-access columns
    default to 0 / "" when absent so callers can mix-and-match traces.
    """
    rows = []
    with path.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            row["cpu_cycle"] = int(row["cpu_cycle"])
            row["ppu_frame"] = int(row["ppu_frame"])
            row["ppu_scanline"] = int(row["ppu_scanline"])
            row["ppu_dot"] = int(row["ppu_dot"])
            for k in (
                "irq_pending_mapper_at_low",
                "irq_pending_apu_at_low",
                "irq_pending_mapper_at_high",
                "irq_pending_apu_at_high",
                "nmi_line",
            ):
                row[k] = int(row[k])
            # Session-21 optional columns.
            for k in (
                "dmc_dma_pending_pre",
                "dmc_dma_pending_post",
                "dmc_dma_short_post",
                "dmc_abort_pending_post",
                "dmc_abort_delay_post",
                "dmc_dma_cooldown_post",
                "dmc_dma_delay_post",
                "apu_phase_post",
                "in_dmc_dma",
                "dma_cycles_owed",
            ):
                row[k] = int(row.get(k, 0) or 0)
            row["bus_access"] = row.get("bus_access", "")
            row["bus_addr"] = int(row.get("bus_addr", "0") or "0", 0)
            row["bus_data"] = int(row.get("bus_data", "0") or "0", 0)
            rows.append(row)
    return rows


def load_mesen2(path: Path):
    """Parse a Mesen2 IRQ trace CSV into a list of dicts.

    Schema-tolerant: pre-Session-21 (8-column) and Session-21+ (12-
    column) traces both load; the optional DMC columns default to 0
    when absent.
    """
    rows = []
    with path.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            row["cpu_cycle"] = int(row["cpu_cycle"])
            row["ppu_frame"] = int(row["ppu_frame"])
            row["ppu_scanline"] = int(row["ppu_scanline"])
            row["ppu_dot"] = int(row["ppu_dot"])
            row["pc"] = int(row["pc"])
            row["apu_irq_flag"] = int(row["apu_irq_flag"])
            row["nmi_flag"] = int(row["nmi_flag"])
            # Session-21 optional columns.
            for k in ("dmc_irq_flag", "dmc_irq_en", "dmc_bytes_rem", "dmc_sample_addr"):
                row[k] = int(row.get(k, 0) or 0)
            rows.append(row)
    return rows


def find_first_mapper_assertion_rustynes(rows):
    prev_mapper = 0
    for r in rows:
        if r["irq_pending_mapper_at_high"] == 1 and prev_mapper == 0:
            return r
        prev_mapper = r["irq_pending_mapper_at_high"]
    return None


def find_first_apu_assertion_rustynes(rows):
    prev_apu = 0
    for r in rows:
        if r["irq_pending_apu_at_high"] == 1 and prev_apu == 0:
            return r
        prev_apu = r["irq_pending_apu_at_high"]
    return None


def find_first_nmi_rustynes(rows):
    for r in rows:
        if r["nmi_line"] == 1:
            return r
    return None


def find_first_irq_svc_mesen2(rows):
    for r in rows:
        if r["event_type"] == "irq_svc":
            return r
    return None


def find_first_apu_set_mesen2(rows):
    for r in rows:
        if r["event_type"] == "apu_set":
            return r
    return None


def find_first_nmi_svc_mesen2(rows):
    for r in rows:
        if r["event_type"] == "nmi_svc":
            return r
    return None


def fmt(row, prefix=""):
    if row is None:
        return f"{prefix}<none>"
    return (
        f"{prefix}cycle={row['cpu_cycle']} "
        f"frame={row['ppu_frame']} scanline={row['ppu_scanline']} dot={row['ppu_dot']}"
    )


def load_rustynes_svc(path: Path):
    """Parse a RustyNES service-event sidecar CSV (`.svc.csv`).

    Phase 1.2 of Track C1 attempt 14: rows are CPU vector fetches
    (one row per IRQ or NMI vector fetch).  Schema:
      cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, event_type, vector.
    """
    rows = []
    with path.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            row["cpu_cycle"] = int(row["cpu_cycle"])
            row["ppu_frame"] = int(row["ppu_frame"])
            row["ppu_scanline"] = int(row["ppu_scanline"])
            row["ppu_dot"] = int(row["ppu_dot"])
            rows.append(row)
    return rows


def run_service_diff(rusty_path: Path, mesen_path: Path) -> int:
    """Phase 1.2 direct cross-diff: RustyNES `.svc.csv` vs Mesen2 service rows."""
    rusty = load_rustynes_svc(rusty_path)
    mesen = [
        r for r in load_mesen2(mesen_path) if r["event_type"] in ("irq_svc", "nmi_svc")
    ]
    print(f"== Service-event cross-diff ==")
    print(f"  RustyNES (.svc.csv): {rusty_path} ({len(rusty)} rows)")
    print(f"  Mesen2 (svc rows):   {mesen_path} ({len(mesen)} rows)")
    print()

    types_r = Counter(r["event_type"] for r in rusty)
    types_m = Counter(r["event_type"] for r in mesen)
    print(f"  RustyNES event-type tally: {dict(types_r)}")
    print(f"  Mesen2   event-type tally: {dict(types_m)}")
    print()

    n = min(len(rusty), len(mesen))
    if n == 0:
        print("  no matched events")
        return 0
    cyc_deltas: list[int] = []
    dot_deltas: list[int] = []
    print(f"  First {min(n, 12)} index-aligned events:")
    print(
        "    idx   r_cycle    m_cycle     dCyc  r_dot  m_dot   dDot  r_evt    m_evt   r_scanl m_scanl"
    )
    for i in range(min(n, 12)):
        d = mesen[i]["cpu_cycle"] - rusty[i]["cpu_cycle"]
        dd = mesen[i]["ppu_dot"] - rusty[i]["ppu_dot"]
        cyc_deltas.append(d)
        dot_deltas.append(dd)
        print(
            f"    {i:3d}  {rusty[i]['cpu_cycle']:9d}  {mesen[i]['cpu_cycle']:9d}  {d:+8d}  "
            f"{rusty[i]['ppu_dot']:5d}  {mesen[i]['ppu_dot']:5d}  {dd:+5d}  "
            f"{rusty[i]['event_type']:7s}  {mesen[i]['event_type']:7s}  "
            f"{rusty[i]['ppu_scanline']:7d} {mesen[i]['ppu_scanline']:7d}"
        )
    # Histogram across the full intersection.
    for i in range(min(n, 12), n):
        cyc_deltas.append(mesen[i]["cpu_cycle"] - rusty[i]["cpu_cycle"])
        dot_deltas.append(mesen[i]["ppu_dot"] - rusty[i]["ppu_dot"])
    print()
    print(
        f"  Δcyc summary: min={min(cyc_deltas)} max={max(cyc_deltas)} "
        f"mean={sum(cyc_deltas) / len(cyc_deltas):.2f} n={len(cyc_deltas)}"
    )
    print(
        f"  Δdot summary: min={min(dot_deltas)} max={max(dot_deltas)} "
        f"mean={sum(dot_deltas) / len(dot_deltas):.2f}"
    )
    return 0


def run_dmc_diff(rusty_path: Path, mesen_path: Path) -> int:
    """Session-21 DMC scheduler cross-diff.

    Focuses on:
      * DMC IRQ assertion cycles (both sides).
      * RustyNES-only: DMC DMA pending / abort-delay / cooldown / short
        / delay timeline.
      * Bus-access type at each DMC-active cycle.
      * Mesen2 DMC `dmc_set` / `dmc_clr` / `dmc_run` / `dmc_irqen`
        events (the per-instruction edge-detected DMC activity).
    """
    rusty = load_rustynes(rusty_path)
    mesen = load_mesen2(mesen_path)
    print("== DMC scheduler cross-diff ==")
    print(f"  RustyNES: {rusty_path} ({len(rusty)} rows)")
    print(f"  Mesen2:   {mesen_path} ({len(mesen)} rows)")
    print()

    # RustyNES DMC activity stats.
    dmc_pending_rows = [r for r in rusty if r["dmc_dma_pending_post"] == 1]
    dmc_halt_rows = [r for r in rusty if r["in_dmc_dma"] == 1]
    dmc_abort_rows = [r for r in rusty if r["dmc_abort_pending_post"] == 1]
    bus_access_counts = Counter(r["bus_access"] for r in rusty if r["bus_access"])
    print("  RustyNES DMC activity:")
    print(f"    dmc_dma_pending_post=1 rows: {len(dmc_pending_rows)}")
    print(f"    in_dmc_dma=1 rows:           {len(dmc_halt_rows)}")
    print(f"    dmc_abort_pending_post=1:    {len(dmc_abort_rows)}")
    print(f"    bus_access tally:            {dict(bus_access_counts)}")
    print()

    # Mesen2 DMC event tally.
    mesen_dmc_events = [
        r
        for r in mesen
        if r["event_type"] in ("dmc_set", "dmc_clr", "dmc_run", "dmc_irqen")
    ]
    types_m = Counter(r["event_type"] for r in mesen_dmc_events)
    print(f"  Mesen2 DMC event tally: {dict(types_m)}")
    print()

    # First DMC IRQ assertion on each side.
    rusty_first_dmc_irq = None
    for r in rusty:
        # DMC IRQ is one component of `irq_pending_apu_*`.  We can't
        # cleanly disambiguate APU frame IRQ vs DMC IRQ from RustyNES's
        # current trace; use the first APU-IRQ assertion as the proxy
        # when the trace covers a DMC-active ROM.
        if r["irq_pending_apu_at_high"] == 1:
            rusty_first_dmc_irq = r
            break
    mesen_first_dmc_irq = None
    for r in mesen:
        if r["event_type"] == "dmc_set":
            mesen_first_dmc_irq = r
            break
    print("  First DMC IRQ assertion:")
    print(fmt(rusty_first_dmc_irq, "    RustyNES (apu_at_high proxy): "))
    print(fmt(mesen_first_dmc_irq, "    Mesen2   (dmc_set):           "))
    if rusty_first_dmc_irq and mesen_first_dmc_irq:
        d = mesen_first_dmc_irq["cpu_cycle"] - rusty_first_dmc_irq["cpu_cycle"]
        print(f"    delta (mesen - rusty): {d:+d} CPU cycles")
    print()

    # First DMC DMA activation window.
    rusty_first_pending = dmc_pending_rows[0] if dmc_pending_rows else None
    mesen_first_run = next(
        (r for r in mesen if r["event_type"] == "dmc_run"), None
    )
    print("  First DMC DMA activation:")
    print(fmt(rusty_first_pending, "    RustyNES (pending_post=1):     "))
    print(fmt(mesen_first_run, "    Mesen2   (dmc_run + bytes>0): "))
    print()

    # Per-event delay timeline (RustyNES side only — Mesen2 does not
    # expose the scheduler's compensating-delay countdowns).
    if dmc_halt_rows:
        print("  RustyNES halt-cycle timeline (first 12 halt cycles):")
        print(
            "    idx  cycle      scnln dot  pre post short abrt abrtD cd  delay  apu_phase  bus"
        )
        for i, r in enumerate(dmc_halt_rows[:12]):
            print(
                f"    {i:3d}  {r['cpu_cycle']:9d}  {r['ppu_scanline']:4d}  "
                f"{r['ppu_dot']:3d}  {r['dmc_dma_pending_pre']}  "
                f"{r['dmc_dma_pending_post']}  {r['dmc_dma_short_post']:5d}  "
                f"{r['dmc_abort_pending_post']:4d}  {r['dmc_abort_delay_post']:5d}  "
                f"{r['dmc_dma_cooldown_post']:2d}  {r['dmc_dma_delay_post']:5d}  "
                f"{r['apu_phase_post']:9d}  {r['bus_access']}"
            )
    print()
    return 0


def main(argv):
    if len(argv) >= 4 and argv[1] == "--svc":
        rusty_path = Path(argv[2])
        mesen_path = Path(argv[3])
        if not rusty_path.exists():
            print(f"missing: {rusty_path}")
            return 2
        if not mesen_path.exists():
            print(f"missing: {mesen_path}")
            return 2
        return run_service_diff(rusty_path, mesen_path)
    if len(argv) >= 4 and argv[1] == "--dmc":
        rusty_path = Path(argv[2])
        mesen_path = Path(argv[3])
        if not rusty_path.exists():
            print(f"missing: {rusty_path}")
            return 2
        if not mesen_path.exists():
            print(f"missing: {mesen_path}")
            return 2
        return run_dmc_diff(rusty_path, mesen_path)
    if len(argv) != 3:
        print(__doc__)
        return 2
    rusty_path = Path(argv[1])
    mesen_path = Path(argv[2])
    if not rusty_path.exists():
        print(f"missing: {rusty_path}")
        return 2
    if not mesen_path.exists():
        print(f"missing: {mesen_path}")
        return 2

    rusty = load_rustynes(rusty_path)
    mesen = load_mesen2(mesen_path)

    print(f"== Cross-diff ==")
    print(f"  RustyNES: {rusty_path} ({len(rusty)} rows)")
    print(f"  Mesen2:   {mesen_path} ({len(mesen)} rows)")
    print()

    # Mesen2 event-type tally.
    types = Counter(r["event_type"] for r in mesen)
    print(f"  Mesen2 event-type tally:")
    for t, n in sorted(types.items()):
        print(f"    {t}: {n}")
    print()

    # First mapper-IRQ assertion (RustyNES) vs first irq_svc (Mesen2).
    rusty_first_mapper = find_first_mapper_assertion_rustynes(rusty)
    mesen_first_irq = find_first_irq_svc_mesen2(mesen)
    print(f"  First mapper-IRQ event:")
    print(fmt(rusty_first_mapper, "    RustyNES (assertion): "))
    print(fmt(mesen_first_irq, "    Mesen2   (service):   "))
    if rusty_first_mapper and mesen_first_irq:
        d = mesen_first_irq["cpu_cycle"] - rusty_first_mapper["cpu_cycle"]
        print(f"    delta (mesen - rusty): {d:+d} CPU cycles")
        print(
            f"    rusty scanline={rusty_first_mapper['ppu_scanline']} "
            f"vs mesen scanline={mesen_first_irq['ppu_scanline']}"
        )
    print()

    # First APU-IRQ assertion (RustyNES) vs first apu_set (Mesen2).
    rusty_first_apu = find_first_apu_assertion_rustynes(rusty)
    mesen_first_apu = find_first_apu_set_mesen2(mesen)
    print(f"  First APU-IRQ assertion:")
    print(fmt(rusty_first_apu, "    RustyNES (line@high): "))
    print(fmt(mesen_first_apu, "    Mesen2   (apu_set):  "))
    if rusty_first_apu and mesen_first_apu:
        d = mesen_first_apu["cpu_cycle"] - rusty_first_apu["cpu_cycle"]
        print(f"    delta (mesen - rusty): {d:+d} CPU cycles")
    print()

    # First NMI event.
    rusty_first_nmi = find_first_nmi_rustynes(rusty)
    mesen_first_nmi = find_first_nmi_svc_mesen2(mesen)
    print(f"  First NMI event:")
    print(fmt(rusty_first_nmi, "    RustyNES (line=high): "))
    print(fmt(mesen_first_nmi, "    Mesen2   (nmi_svc):  "))
    if rusty_first_nmi and mesen_first_nmi:
        d = mesen_first_nmi["cpu_cycle"] - rusty_first_nmi["cpu_cycle"]
        print(f"    delta (mesen - rusty): {d:+d} CPU cycles")
    print()

    # All Mesen2 IRQ-svc cycles aligned to RustyNES mapper-assertion cycles.
    mesen_svc_cycles = [r["cpu_cycle"] for r in mesen if r["event_type"] == "irq_svc"]
    rusty_assertion_cycles = []
    prev = 0
    for r in rusty:
        if r["irq_pending_mapper_at_high"] == 1 and prev == 0:
            rusty_assertion_cycles.append(r["cpu_cycle"])
        prev = r["irq_pending_mapper_at_high"]
    print(f"  Mapper IRQ-assertion cycles (RustyNES, low->high): {len(rusty_assertion_cycles)}")
    print(f"  IRQ-service cycles (Mesen2): {len(mesen_svc_cycles)}")
    if rusty_assertion_cycles[:5]:
        print(f"    First 5 RustyNES: {rusty_assertion_cycles[:5]}")
    if mesen_svc_cycles[:5]:
        print(f"    First 5 Mesen2:   {mesen_svc_cycles[:5]}")
    print()

    # All APU set/clear cycles aligned.
    rusty_apu_assertion_cycles = []
    prev = 0
    for r in rusty:
        if r["irq_pending_apu_at_high"] == 1 and prev == 0:
            rusty_apu_assertion_cycles.append(r["cpu_cycle"])
        prev = r["irq_pending_apu_at_high"]
    mesen_apu_set_cycles = [r["cpu_cycle"] for r in mesen if r["event_type"] == "apu_set"]
    print(f"  APU-IRQ assertion cycles (RustyNES, low->high): {len(rusty_apu_assertion_cycles)}")
    print(f"  APU-set events (Mesen2): {len(mesen_apu_set_cycles)}")
    if rusty_apu_assertion_cycles[:5]:
        print(f"    First 5 RustyNES: {rusty_apu_assertion_cycles[:5]}")
    if mesen_apu_set_cycles[:5]:
        print(f"    First 5 Mesen2:   {mesen_apu_set_cycles[:5]}")
    print()

    # Per-event delta histogram (only for matched lengths or shorter).
    if rusty_apu_assertion_cycles and mesen_apu_set_cycles:
        n = min(len(rusty_apu_assertion_cycles), len(mesen_apu_set_cycles))
        deltas = [
            mesen_apu_set_cycles[i] - rusty_apu_assertion_cycles[i] for i in range(n)
        ]
        avg = sum(deltas) / n if n else 0
        print(f"  Mesen-vs-Rusty APU-set delta histogram (first {n} matched events):")
        print(f"    min={min(deltas)} max={max(deltas)} avg={avg:.1f}")
        # Top-5 unique delta values.
        bucket = Counter(deltas)
        for delta, count in bucket.most_common(8):
            print(f"    delta={delta:+d}: {count} events")
        print()

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
