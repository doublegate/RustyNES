#!/usr/bin/env python3
"""perf_log_check.py — v1.5.0 "Lens" Workstream H7 perf-log regression gate.

Parses a RustyNES perf-log CSV (the one the Performance panel's "Logging"
checkbox / the `RUSTYNES_PERF_LOG` env hook writes under `perf-logs/`) and
asserts the frontend pacing/audio-sync health signals stay within bounds, so a
regression in the present/pace/audio layer surfaces as a tracked failure
instead of a one-off observation.

Tracked signals (the ones the 2026-06-16 SMB capture flagged):
  * underruns        — cumulative audio underruns (goal: 0 in a steady run).
  * produced_max_ms  — worst produced-frame interval (transient OS-stall tail).
  * catchup_bursts   — wall-clock pacer catch-up bursts (>=2 frames in a pace).
  * snap_forwards    — catch-up windows abandoned (deep stalls).

Like `scripts/bench_regression_check.sh`, this is a deliberately
non-flaky, *absolute*-threshold gate (shared/headful hosts vary run-to-run):
it trips only on a gross regression, and the steady-run underruns target (0)
is the one tight assertion. Thresholds are overridable per host.

The CSV columns are looked up BY NAME from the header row, so this keeps
working as `perf_log.rs::columns()` adds fields (the H8 parity guarantee).

Usage:
    perf_log_check.py <perf-log.csv> [--max-underruns N] [--max-produced-ms MS]
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


def col_float(row: dict[str, str], name: str) -> float:
    raw = row.get(name, "")
    if raw in ("", "-"):
        return 0.0
    try:
        return float(raw)
    except ValueError:
        return 0.0


def main() -> int:
    ap = argparse.ArgumentParser(description="RustyNES perf-log regression gate")
    ap.add_argument("csv", help="path to a perf-logs/perf-*.csv capture")
    ap.add_argument("--max-underruns", type=int, default=0,
                    help="max cumulative audio underruns at the LAST row (default 0)")
    ap.add_argument("--max-produced-ms", type=float, default=150.0,
                    help="max produced-frame interval ms over the run (default 150)")
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

    failures: list[str] = []
    if underruns > args.max_underruns:
        failures.append(f"underruns {underruns} > {args.max_underruns}")
    if produced_max > args.max_produced_ms:
        failures.append(f"produced_max {produced_max:.1f} ms > {args.max_produced_ms} ms")
    if catchup > args.max_catchup_bursts:
        failures.append(f"catchup_bursts {catchup} > {args.max_catchup_bursts}")
    if snaps > args.max_snap_forwards:
        failures.append(f"snap_forwards {snaps} > {args.max_snap_forwards}")

    print(f"perf_log_check: {args.csv}")
    print(f"  rows={len(rows)} (analyzed {len(body)} after {args.warmup_rows} warmup)")
    print(f"  underruns={underruns}  produced_max={produced_max:.1f}ms  "
          f"catchup_bursts={catchup}  snap_forwards={snaps}")
    if failures:
        print("  FAIL: " + "; ".join(failures))
        return 1
    print("  OK: all tracked signals within bounds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
