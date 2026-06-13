#!/usr/bin/env python3
"""Cross-diff a RustyNES DMC DMA trace against a Mesen2 DMC DMA trace.

Sprint 2.3 Step 3 oracle alignment (Path β trace tooling). Pairs the
focused per-cycle CSVs emitted by

    crates/rustynes-test-harness/src/bin/trace_dmc_dma.rs   (RustyNES)
    scripts/mesen2_dmc_dma_trace.lua                    (Mesen2)

The cross-diff identifies per-cycle divergence on the four
compensating delays Session-20 named as load-bearing for
``APU Registers and DMA tests :: Implicit DMA Abort``:

    * ``dmc_dma_short``        (load vs early-deliver-get path)
    * ``dmc_dma_cooldown``     (4 post-load / 5 post-early-deliver)
    * ``dmc_abort_delay_for``  (cycles-until-output -> abort halt delay)
    * ``dmc_dma_pending`` + ``in_dmc_dma`` (scheduler state pair)

The schemas the two emit are deliberately asymmetric -- RustyNES
exposes its private scheduler state at every cycle of interest;
Mesen2's Lua API only exposes the ``apu.dmc.*`` getState surface, so
the script infers ``dmc_get`` (DMC DMA fetch) events from
``bytesRemaining`` decrements observed at memory-callback boundaries.

ALIGNMENT
=========

The script aligns at the FIRST ``$4015`` write on each side (the
canonical DMC enable / disable signal that bootstraps the test).
After alignment it walks forward emitting per-event divergence:

  * each DMC-get event: cycle delta vs Mesen2 (positive = Mesen2 lands
    the fetch later, negative = earlier)
  * each ``$4015`` R/W: bus_data agreement
  * each DMC IRQ set/clear: cycle delta

Output:

  * SUMMARY block (first-fetch delta, total fetches, IRQ-set delta)
  * EVENT WALK (per-event paired rows up to ``--max-events``)
  * DIVERGENCE block (the largest cycle deltas + the first 5
    "RustyNES has no match within tolerance" events, which are the
    actionable axes for the multi-axis recalibration)

USAGE
=====

    python3 scripts/dmc_dma_trace_cross_diff.py \\
        /tmp/RustyNES/dmc_rusty.csv \\
        /tmp/RustyNES/dmc_mesen2.csv

    python3 scripts/dmc_dma_trace_cross_diff.py \\
        --max-events 50 --tolerance 4 \\
        rusty.csv mesen2.csv

Exits 0 (always; this is a diagnostic, not a gate).
"""
from __future__ import annotations

import argparse
import csv
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


def _i(v):
    return int(v) if v not in (None, "") else 0


def _h(v):
    # Hex columns are emitted as `$XXXX` strings (e.g. `$4015`).
    if v in (None, ""):
        return 0
    s = str(v).strip()
    if s.startswith("$"):
        return int(s[1:], 16)
    try:
        return int(s, 0)
    except ValueError:
        return 0


@dataclass
class RustyRow:
    cpu_cycle: int
    ppu_frame: int
    ppu_scanline: int
    ppu_dot: int
    m2_phase: str
    access: str        # 'R'/'W'/'r' (DmaRead)/'w' (DmaWrite)/'I' (idle)
    bus_addr: int
    bus_data: int
    dmc_pending_pre: int
    dmc_pending_post: int
    dmc_dma_short: int
    dmc_abort_pending: int
    dmc_abort_delay: int
    dmc_cooldown: int
    mapper_irq_low: int
    apu_irq_low: int


@dataclass
class MesenRow:
    cpu_cycle: int
    ppu_frame: int
    ppu_scanline: int
    ppu_dot: int
    m2_phase: str
    kind: str          # 'R'/'W'/'dmc_get'/'dmc_irq_set'/'dmc_irq_clr'/'dmc_en_set'/'dmc_en_clr'/'frame'
    addr: int
    value: int
    bytes_rem: int
    sample_addr: int
    irq_flag: int
    irq_en: int
    silence: int


def load_rusty(path: Path) -> list[RustyRow]:
    out = []
    with path.open() as f:
        for r in csv.DictReader(f):
            out.append(RustyRow(
                cpu_cycle=_i(r["cpu_cycle"]),
                ppu_frame=_i(r["ppu_frame"]),
                ppu_scanline=_i(r["ppu_scanline"]),
                ppu_dot=_i(r["ppu_dot"]),
                m2_phase=str(r["m2_phase"]),
                access=str(r["access"]),
                bus_addr=_h(r["bus_addr"]),
                bus_data=_h(r["bus_data"]),
                dmc_pending_pre=_i(r["dmc_pending_pre"]),
                dmc_pending_post=_i(r["dmc_pending_post"]),
                dmc_dma_short=_i(r["dmc_dma_short"]),
                dmc_abort_pending=_i(r["dmc_abort_pending"]),
                dmc_abort_delay=_i(r["dmc_abort_delay"]),
                dmc_cooldown=_i(r["dmc_cooldown"]),
                mapper_irq_low=_i(r["mapper_irq_low"]),
                apu_irq_low=_i(r["apu_irq_low"]),
            ))
    return out


def load_mesen(path: Path) -> list[MesenRow]:
    out = []
    with path.open() as f:
        for r in csv.DictReader(f):
            out.append(MesenRow(
                cpu_cycle=_i(r["cpu_cycle"]),
                ppu_frame=_i(r["ppu_frame"]),
                ppu_scanline=_i(r["ppu_scanline"]),
                ppu_dot=_i(r["ppu_dot"]),
                m2_phase=str(r["m2_phase"]),
                kind=str(r["kind"]),
                addr=_h(r["addr"]),
                value=_h(r["value"]),
                bytes_rem=_i(r["dmc_bytes_rem"]),
                sample_addr=_h(r["dmc_sample_addr"]),
                irq_flag=_i(r["dmc_irq_flag"]),
                irq_en=_i(r["dmc_irq_en"]),
                silence=_i(r["dmc_silence"]),
            ))
    return out


def first_4015_write(rusty: list[RustyRow], value: Optional[int] = None) -> Optional[RustyRow]:
    """First `$4015` write on the RustyNES side; if ``value`` is given,
    the first write whose ``bus_data == value`` (lets the caller align
    on the canonical DMC-enable `$10` write rather than an earlier
    `$00` disable)."""
    for r in rusty:
        if r.access == "W" and r.bus_addr == 0x4015 and (value is None or r.bus_data == value):
            return r
    return None


def first_4015_write_mesen(mesen: list[MesenRow], value: Optional[int] = None) -> Optional[MesenRow]:
    for r in mesen:
        if r.kind == "W" and r.addr == 0x4015 and (value is None or r.value == value):
            return r
    return None


def rusty_dmc_fetches(rusty: list[RustyRow]) -> list[RustyRow]:
    # One row per logical DMC-DMA fetch -- rising edge of
    # `dmc_pending_post`. Distinguishes DMC-DMA from OAM-DMA (which
    # also emits `DmaRead` rows at $8000..=$80FF when DMA source page
    # is $80+, but with `dmc_pending_post == 0` since the DMC
    # scheduler isn't active during OAM-DMA bursts).
    out = []
    prev = 0
    for r in rusty:
        if r.dmc_pending_post == 1 and prev == 0:
            out.append(r)
        prev = r.dmc_pending_post
    return out


def mesen_dmc_fetches(mesen: list[MesenRow]) -> list[MesenRow]:
    return [r for r in mesen if r.kind == "dmc_get"]


def rusty_apu_irq_rises(rusty: list[RustyRow]) -> list[RustyRow]:
    out = []
    prev = 0
    for r in rusty:
        if r.apu_irq_low == 1 and prev == 0:
            out.append(r)
        prev = r.apu_irq_low
    return out


def mesen_dmc_irq_sets(mesen: list[MesenRow]) -> list[MesenRow]:
    return [r for r in mesen if r.kind == "dmc_irq_set"]


def rusty_4015_accesses(rusty: list[RustyRow]) -> list[RustyRow]:
    return [r for r in rusty if r.bus_addr == 0x4015 and r.access in ("R", "W")]


def mesen_4015_accesses(mesen: list[MesenRow]) -> list[MesenRow]:
    return [r for r in mesen if r.addr == 0x4015 and r.kind in ("R", "W")]


def pair_events_by_index(
    a: list, b: list, a_cycle, b_cycle, offset: int, tolerance: int
) -> list[tuple[Optional[object], Optional[object], int]]:
    """Pair events by index, reporting cycle delta after applying ``offset``
    (the cycle-skew the first-$4015-write alignment introduces).

    ``offset = mesen_base - rusty_base``: subtract it from Mesen2 cycles
    to bring both axes into the same frame of reference. A delta within
    ±``tolerance`` cycles is treated as "matched"; larger deltas surface
    as actionable divergence."""
    n = min(len(a), len(b))
    out = []
    for i in range(n):
        ra, rb = a[i], b[i]
        delta = (b_cycle(rb) - offset) - a_cycle(ra)
        out.append((ra, rb, delta))
    # Unmatched tail on either side (one emulator has more events).
    for i in range(n, len(a)):
        out.append((a[i], None, 0))
    for i in range(n, len(b)):
        out.append((None, b[i], 0))
    return out


def fmt_rusty(r: Optional[RustyRow]) -> str:
    if r is None:
        return "[—]"
    return (
        f"cyc={r.cpu_cycle} f={r.ppu_frame} sl={r.ppu_scanline} "
        f"dot={r.ppu_dot} {r.m2_phase} {r.access} ${r.bus_addr:04X}=${r.bus_data:02X} "
        f"pre={r.dmc_pending_pre} post={r.dmc_pending_post} "
        f"short={r.dmc_dma_short} abort={r.dmc_abort_pending}/"
        f"{r.dmc_abort_delay} cd={r.dmc_cooldown} airq={r.apu_irq_low}"
    )


def fmt_mesen(r: Optional[MesenRow]) -> str:
    if r is None:
        return "[—]"
    return (
        f"cyc={r.cpu_cycle} f={r.ppu_frame} sl={r.ppu_scanline} "
        f"dot={r.ppu_dot} {r.m2_phase} {r.kind} ${r.addr:04X}=${r.value:02X} "
        f"br={r.bytes_rem} sa=${r.sample_addr:04X} "
        f"irq={r.irq_flag} en={r.irq_en} si={r.silence}"
    )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("rusty_csv", type=Path, help="RustyNES trace_dmc_dma CSV")
    ap.add_argument("mesen_csv", type=Path, help="Mesen2 dmc_dma_trace CSV")
    ap.add_argument("--max-events", type=int, default=30,
                    help="rows per category in the EVENT WALK (default 30)")
    ap.add_argument("--tolerance", type=int, default=2,
                    help="cycle delta tolerance for 'matched' classification (default 2)")
    ap.add_argument("--align-value", type=lambda s: int(s, 0), default=None,
                    help="align on the first `$4015 W` whose bus_data == this value (default: any)")
    args = ap.parse_args()

    if not args.rusty_csv.exists():
        print(f"missing: {args.rusty_csv}", file=sys.stderr)
        return 2
    if not args.mesen_csv.exists():
        print(f"missing: {args.mesen_csv}", file=sys.stderr)
        return 2

    rusty = load_rusty(args.rusty_csv)
    mesen = load_mesen(args.mesen_csv)

    print(f"rusty rows: {len(rusty)}    mesen rows: {len(mesen)}")
    print(f"rusty path: {args.rusty_csv}")
    print(f"mesen path: {args.mesen_csv}")
    print()

    r4015 = first_4015_write(rusty, args.align_value)
    m4015 = first_4015_write_mesen(mesen, args.align_value)
    if r4015 is None or m4015 is None:
        print("[!] no $4015 write found on at least one side -- cannot align")
        print(f"    rusty: {fmt_rusty(r4015)}")
        print(f"    mesen: {fmt_mesen(m4015)}")
        return 0

    offset = m4015.cpu_cycle - r4015.cpu_cycle
    print(f"=== ALIGNMENT ===")
    print(f"  rusty first $4015 W: cyc={r4015.cpu_cycle} (sl={r4015.ppu_scanline} dot={r4015.ppu_dot} val=${r4015.bus_data:02X})")
    print(f"  mesen first $4015 W: cyc={m4015.cpu_cycle} (sl={m4015.ppu_scanline} dot={m4015.ppu_dot} val=${m4015.value:02X})")
    print(f"  offset (mesen - rusty): {offset} cycles")
    print()

    # ---------- SUMMARY ----------
    r_fetches = rusty_dmc_fetches(rusty)
    m_fetches = mesen_dmc_fetches(mesen)
    r_irq = rusty_apu_irq_rises(rusty)
    m_irq = mesen_dmc_irq_sets(mesen)
    r_4015 = rusty_4015_accesses(rusty)
    m_4015 = mesen_4015_accesses(mesen)

    print(f"=== SUMMARY ===")
    print(f"  DMC fetches:        rusty={len(r_fetches):4d}    mesen={len(m_fetches):4d}    diff={len(r_fetches)-len(m_fetches):+d}")
    print(f"  $4015 accesses:     rusty={len(r_4015):4d}    mesen={len(m_4015):4d}    diff={len(r_4015)-len(m_4015):+d}")
    print(f"  APU IRQ rises:      rusty={len(r_irq):4d}    mesen={len(m_irq):4d}    diff={len(r_irq)-len(m_irq):+d}")
    if r_fetches and m_fetches:
        d0 = (m_fetches[0].cpu_cycle - offset) - r_fetches[0].cpu_cycle
        print(f"  First DMC fetch delta (mesen-aligned vs rusty): {d0:+d} cycles")
    if r_irq and m_irq:
        d0 = (m_irq[0].cpu_cycle - offset) - r_irq[0].cpu_cycle
        print(f"  First APU/DMC IRQ rise delta:                   {d0:+d} cycles")
    print()

    # ---------- EVENT WALK: DMC fetches ----------
    print(f"=== DMC FETCHES (first {args.max_events}) ===")
    walked = pair_events_by_index(
        r_fetches, m_fetches,
        lambda r: r.cpu_cycle, lambda m: m.cpu_cycle, offset, args.tolerance,
    )
    diverged = 0
    largest_abs_delta = 0
    for i, (ra, rb, delta) in enumerate(walked[:args.max_events]):
        marker = "ok " if abs(delta) <= args.tolerance and ra and rb else "DIV"
        if abs(delta) > args.tolerance:
            diverged += 1
        if abs(delta) > abs(largest_abs_delta):
            largest_abs_delta = delta
        print(f"  [{i:3d}] {marker} Δ={delta:+4d}  r:{fmt_rusty(ra)}")
        print(f"           m:{fmt_mesen(rb)}")
    print()

    # ---------- EVENT WALK: $4015 R/W ----------
    print(f"=== $4015 R/W (first {args.max_events}) ===")
    walked2 = pair_events_by_index(
        r_4015, m_4015,
        lambda r: r.cpu_cycle, lambda m: m.cpu_cycle, offset, args.tolerance,
    )
    val_mismatches = 0
    for i, (ra, rb, delta) in enumerate(walked2[:args.max_events]):
        marker = "ok " if abs(delta) <= args.tolerance and ra and rb else "DIV"
        rv = ra.bus_data if ra else -1
        mv = rb.value if rb else -1
        kind = "R" if (ra and ra.access == "R") else "W"
        if rv != mv and ra and rb:
            marker = "VAL"
            val_mismatches += 1
        print(f"  [{i:3d}] {marker} Δ={delta:+4d} {kind} r=${rv:02X} m=${mv:02X}  r:{fmt_rusty(ra)}")
    print()

    # ---------- DIVERGENCE ----------
    print(f"=== DIVERGENCE ===")
    print(f"  DMC fetches diverged (|Δ|>{args.tolerance}): {diverged} of {min(len(r_fetches), len(m_fetches))}")
    print(f"  Largest signed Δ in walk:                      {largest_abs_delta:+d} cycles")
    print(f"  $4015 R/W value mismatches:                    {val_mismatches}")
    print()
    print("# Interpretation guide:")
    print("# - Positive Δ on DMC fetches => Mesen2 lands the fetch LATER")
    print("#   than RustyNES; consider decreasing dmc_dma_cooldown or")
    print("#   shrinking dmc_abort_delay (Session-20 axis 2 + 3).")
    print("# - Negative Δ on DMC fetches => Mesen2 lands the fetch EARLIER")
    print("#   than RustyNES; consider increasing dmc_dma_cooldown or")
    print("#   dmc_abort_delay.")
    print("# - $4015 value mismatch (VAL) means the DMC IRQ flag visibility")
    print("#   differs at that cycle -- axis 4 (dmc_dma_pending visibility")
    print("#   on the read latch). See Session-20 Finding 3 + Sprint 2.3")
    print("#   Step 3 audit (docs/audit/sprint-2.3-*.md).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
