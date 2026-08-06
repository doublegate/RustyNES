#!/usr/bin/env python3
"""perf_log_check.py — v1.5.0 "Lens" Workstream H7 perf-log regression gate.

Parses a RustyNES perf-log CSV (the one the Performance panel's "Logging"
checkbox / the `RUSTYNES_PERF_LOG` env hook writes under `perf-logs/`) and
asserts the frontend pacing/audio-sync health signals stay within bounds, so a
regression in the present/pace/audio layer surfaces as a tracked failure
instead of a one-off observation.

Tracked signals:
  * produced_p99_ms  — p99 produced-frame interval. THE STUTTER SIGNAL (v2.3.2).
  * presented_p99_ms — p99 presented-frame interval. Ditto, on the present side.
  * underruns        — cumulative audio underruns (goal: 0 in a steady run).
  * produced_max_ms  — worst produced-frame interval; a coarse backstop only.
  * catchup_bursts   — wall-clock pacer catch-up bursts (>=2 frames in a pace).
  * snap_forwards    — catch-up windows abandoned (deep stalls).

**Why p99 and not the max (v2.3.2 F2).** This gate originally tracked only
`produced_max_ms`, against a 150 ms threshold -- NINE TIMES the 16.639 ms NTSC
budget. `produced_max` is also a single sample, so it is simultaneously too loose
to catch real degradation and too noisy to tighten. Checked against the captures
in `perf-logs/`, the old gate passed
`perf-Super_Mario_Bros_nes-20260616-231215.csv` on every threshold it tracked
except a stray underrun, despite that run peaking at 128.9 ms with 62 catch-up
bursts and a 35.0 ms produced p99.

The p99 columns have been in the CSV all along; nothing read them. Healthy
captures sit at produced/presented p99 ~17.2-17.6 ms (the budget plus pacing
slack); degraded ones reach 24-35 ms produced and up to 56 ms presented. The
22 ms default sits cleanly between the two populations. Each row already carries
a windowed p99, so the run's figure is the MEDIAN of those rows -- not their max,
which would reintroduce exactly the single-sample fragility being replaced.

This remains an *absolute*-threshold gate (shared/headful hosts vary run to run)
and is overridable per host, but it now gates the metric that actually
corresponds to user-visible stutter rather than a peak that no one felt.

The CSV columns are looked up BY NAME from the header row, so this keeps
working as `perf_log.rs::columns()` adds fields (the H8 parity guarantee).

Usage:
    perf_log_check.py <perf-log.csv> [--max-underruns N] [--max-produced-ms MS]
                      [--max-produced-p99-ms MS] [--max-presented-p99-ms MS]
                      [--max-catchup-bursts N] [--max-snap-forwards N]
                      [--warmup-rows N]

Exit code 0 = within bounds, 1 = a threshold tripped, 2 = bad input.
"""

from __future__ import annotations

import argparse
import csv
import sys


def load_rows(path: str) -> tuple[list[str], list[dict[str, str]]]:
    """Return (header, data_rows) skipping the `#`-commented header block."""
    with open(path, newline="", encoding="utf-8") as fh:
        lines = [ln for ln in fh if not ln.startswith("#")]
    if not lines:
        print(f"perf_log_check: {path}: no data rows", file=sys.stderr)
        sys.exit(2)
    reader = csv.DictReader(lines)
    return reader.fieldnames or [], list(reader)


def _median(values: list[float]) -> float:
    """Median of the sampled values; 0.0 when the column is absent entirely."""
    vals = sorted(v for v in values if v > 0.0)
    if not vals:
        return 0.0
    mid = len(vals) // 2
    return vals[mid] if len(vals) % 2 else (vals[mid - 1] + vals[mid]) / 2.0


def col_float(row: dict[str, str], name: str) -> float:
    """Read a numeric column, treating anything unparseable as absent (0.0).

    `csv.DictReader` fills a SHORT row's missing keys with its `restval`, which
    defaults to `None` -- not `""`. A capture killed mid-write (the normal way a
    timed run ends) therefore yields a final row whose tail columns are `None`,
    and `float(None)` raises `TypeError`, which a bare `except ValueError` does
    not catch. That crashed this gate on a real capture
    (`perf-Super_Mario_Bros_nes-20260613-010043.csv`). Handle both.
    """
    raw = row.get(name)
    if raw is None or raw.strip() in ("", "-"):
        return 0.0
    try:
        return float(raw)
    except (TypeError, ValueError):
        return 0.0


def main() -> int:
    ap = argparse.ArgumentParser(description="RustyNES perf-log regression gate")
    ap.add_argument("csv", help="path to a perf-logs/perf-*.csv capture")
    ap.add_argument("--max-underruns", type=int, default=0,
                    help="max cumulative audio underruns at the LAST row (default 0)")
    ap.add_argument("--max-produced-ms", type=float, default=150.0,
                    help="max produced-frame interval ms over the run (default 150; a "
                         "coarse backstop -- the p99 gates below are the real signal)")
    # v2.3.2 F2 — p99 gates. `produced_max` is ONE sample and 150 ms is nine
    # times the 16.639 ms NTSC budget, so the old gate passed visibly-degraded
    # runs: `perf-Super_Mario_Bros_nes-20260616-231215.csv` peaks at 128.9 ms
    # with 62 catch-up bursts and only fails on underruns. Healthy captures in
    # `perf-logs/` sit at produced/presented p99 ~17.2-17.6 ms (the budget plus
    # pacing slack); degraded ones reach 24-35 ms produced and up to 56 ms
    # presented. 22 ms sits cleanly between the two populations.
    ap.add_argument("--max-produced-p99-ms", type=float, default=22.0,
                    help="max median produced-frame p99 ms (default 22; healthy ~17.5)")
    ap.add_argument("--max-presented-p99-ms", type=float, default=22.0,
                    help="max median presented-frame p99 ms (default 22; healthy ~17.5)")
    ap.add_argument("--max-catchup-bursts", type=int, default=200,
                    help="max cumulative catch-up bursts at the LAST row (default 200)")
    ap.add_argument("--max-snap-forwards", type=int, default=40,
                    help="max cumulative snap-forwards at the LAST row (default 40)")
    ap.add_argument("--warmup-rows", type=int, default=3,
                    help="rows to skip at the start (startup gate / first-frame)")
    args = ap.parse_args()

    header, rows = load_rows(args.csv)
    for required in ("underruns", "produced_max_ms", "catchup_bursts", "snap_forwards"):
        if required not in header:
            print(f"perf_log_check: column `{required}` missing from {args.csv} "
                  f"(stale CSV? re-capture)", file=sys.stderr)
            return 2

    body = rows[args.warmup_rows:] if len(rows) > args.warmup_rows else rows
    if not body:
        print("perf_log_check: no rows after warmup", file=sys.stderr)
        return 2

    last = body[-1]
    # Cumulative counters are taken at the final row; produced_max is a
    # windowed peak, so take the max across the run.
    underruns = int(col_float(last, "underruns"))
    catchup = int(col_float(last, "catchup_bursts"))
    snaps = int(col_float(last, "snap_forwards"))
    produced_max = max(col_float(r, "produced_max_ms") for r in body)
    # p99 is taken as the MEDIAN across sample rows, not the max: each row already
    # reports a windowed p99, so the median of those is the run's typical tail and
    # is not itself hostage to one bad window (the failure mode `produced_max` has).
    produced_p99 = _median([col_float(r, "produced_p99_ms") for r in body])
    presented_p99 = _median([col_float(r, "presented_p99_ms") for r in body])

    failures: list[str] = []
    if underruns > args.max_underruns:
        failures.append(f"underruns {underruns} > {args.max_underruns}")
    if produced_max > args.max_produced_ms:
        failures.append(f"produced_max {produced_max:.1f} ms > {args.max_produced_ms} ms")
    if produced_p99 > args.max_produced_p99_ms:
        failures.append(
            f"produced_p99 {produced_p99:.1f} ms > {args.max_produced_p99_ms} ms")
    if presented_p99 > args.max_presented_p99_ms:
        failures.append(
            f"presented_p99 {presented_p99:.1f} ms > {args.max_presented_p99_ms} ms")
    if catchup > args.max_catchup_bursts:
        failures.append(f"catchup_bursts {catchup} > {args.max_catchup_bursts}")
    if snaps > args.max_snap_forwards:
        failures.append(f"snap_forwards {snaps} > {args.max_snap_forwards}")

    print(f"perf_log_check: {args.csv}")
    print(f"  rows={len(rows)} (analyzed {len(body)} after {args.warmup_rows} warmup)")
    print(f"  underruns={underruns}  produced_max={produced_max:.1f}ms  "
          f"catchup_bursts={catchup}  snap_forwards={snaps}")
    print(f"  produced_p99={produced_p99:.2f}ms  presented_p99={presented_p99:.2f}ms  "
          f"(NTSC budget 16.639 ms)")
    if failures:
        print("  FAIL: " + "; ".join(failures))
        return 1
    print("  OK: all tracked signals within bounds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
