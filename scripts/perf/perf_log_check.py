#!/usr/bin/env python3
"""perf_log_check.py — v1.5.0 "Lens" Workstream H7 perf-log regression gate.

Parses a RustyNES perf-log CSV (the one the Performance panel's "Logging"
checkbox / the `RUSTYNES_PERF_LOG` env hook writes under `perf-logs/`) and
asserts the frontend pacing/audio-sync health signals stay within bounds, so a
regression in the present/pace/audio layer surfaces as a tracked failure
instead of a one-off observation.

Tracked signals:
  * produced_p99_ms  — p99 produced-frame interval. THE STUTTER SIGNAL (v2.3.3).
  * presented_p99_ms — p99 presented-frame interval. Ditto, on the present side.
  * underruns        — cumulative audio underruns (goal: 0 in a steady run).
  * produced_max_ms  — worst produced-frame interval; a coarse backstop only.
  * catchup_bursts   — wall-clock pacer catch-up bursts (>=2 frames in a pace).
  * snap_forwards    — catch-up windows abandoned (deep stalls).

**Why p99 and not the max (v2.3.3 F2).** This gate originally tracked only
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


def load_meta(path: str) -> dict[str, str]:
    """Return the `# key = value` metadata block from the top of a capture."""
    meta: dict[str, str] = {}
    with open(path, encoding="utf-8") as fh:
        for ln in fh:
            if not ln.startswith("#"):
                break
            if "=" in ln:
                k, v = ln[1:].split("=", 1)
                meta[k.strip()] = v.strip()
    return meta


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
                    help="max cumulative audio underruns over the analyzed rows (default 0)")
    ap.add_argument("--max-produced-ms", type=float, default=150.0,
                    help="max produced-frame interval ms over the run (default 150; a "
                         "coarse backstop -- the p99 gates below are the real signal)")
    # v2.3.3 F2 — p99 is REPORTED, not gated by default. The first version of
    # this gate defaulted both p99 knobs to 22 ms, justified by "healthy captures
    # sit at 17.2-17.6 ms, degraded ones reach 24-35". Measuring on real hardware
    # disproved it:
    #
    #   capture                       p99   bursts underruns snap_fwd
    #   flowing_palette 20260812 x2  34.4        0         0        0   <- HEALTHY
    #   SMB 20260616-231215          35.0       62        12       12   <- degraded
    #
    # Two clean 90 s captures reproduce p99 = 34.4 ms with ZERO bursts, ZERO
    # underruns and ZERO snap-forwards — the same p99 as the worst archived run.
    # `produced_mean_ms` is 16.64 ms (the NTSC budget) in all 8 captures on file,
    # so the emulator always produces on time; the p99 is the wall-clock pacer
    # beating against vsync, which v2.3.0's own notes predict. On a 120 Hz
    # Wayland host winit cannot read the refresh rate at all (`monitor unknown`),
    # so the beat is a property of the DISPLAY, not of frontend health.
    #
    # An absolute-millisecond p99 gate therefore measures the host's display
    # configuration and reports it as a regression. Keep the number visible —
    # it is genuinely useful when comparing two runs on ONE machine — and gate
    # on the signals that actually separate the populations.
    ap.add_argument("--max-produced-p99-ms", type=float, default=None,
                    help="optional gate on median produced-frame p99 ms "
                         "(default: reported only — see the note in this file)")
    ap.add_argument("--max-presented-p99-ms", type=float, default=None,
                    help="optional gate on median presented-frame p99 ms "
                         "(default: reported only)")
    # Derived from the 8 captures in `perf-logs/`, not chosen: healthy runs sit
    # at 0 catch-up bursts (five captures), the borderline one at 12, and the
    # degraded ones at 32 and 62. 16 separates {0, 12} from {32, 62}. The old
    # default of 200 let a run with 62 bursts, 12 underruns and a 128.9 ms peak
    # pass every threshold it tracked — the hole this gate exists to close.
    ap.add_argument("--max-catchup-bursts", type=int, default=16,
                    help="max cumulative catch-up bursts over the analyzed rows (default 16; "
                         "healthy 0, degraded 32-62)")
    # Same derivation: healthy 0, borderline 2, degraded 12.
    ap.add_argument("--max-snap-forwards", type=int, default=8,
                    help="max cumulative snap-forwards over the analyzed rows (default 8; "
                         "healthy 0, degraded 12)")
    # THE signal the p99 gate was reaching for and missed. `cost_*` is the
    # emulator's own work per displayed frame, with the pacer's sleep excluded —
    # so unlike `produced_*` it is independent of the display's refresh rate and
    # of the wall-clock/vsync beat. If the p95 of that work exceeds one frame
    # period, 60 fps is not sustainable and frames WILL drop, whatever the
    # monitor is doing.
    #
    # v2.3.3 — the two gates that used to live here were derived from
    # CONTAMINATED numbers. `cost_*` included time blocked on the emulator
    # mutex until v2.3.3 (the produce paths timed from before `emu.lock()`),
    # which made run-ahead look like it pushed emulation work to a 24 ms p95
    # against a 16.639 ms budget. Corrected, the same configuration measures
    # ~6 ms. See docs/performance.md v2.3.3 F1.
    #
    # Both gates are now REPORTED, not enforced, for the same reason the p99
    # gate above is: they measure the host, not the emulator.
    #
    #   - `cost_p95` legitimately scales with `run_ahead`, which multiplies
    #     work by design: the shipped `run_ahead = 2` measures 13.9-21.9 ms
    #     and is perfectly healthy, with ~0 drops and an exact console rate.
    #     An absolute work ceiling would fail the shipped default forever.
    #   - `produced_dropped` is a function of the DISPLAY: the same build on
    #     the same host drops 1-9 frames per 45 s under display-sync and
    #     35-131 under the wall-clock fallback. Gating it reports the user's
    #     compositor as a regression.
    #
    # What IS the emulator's own responsibility, and is independent of both
    # the display and `run_ahead`, is whether it keeps console time. That is
    # `produced_mean` against the header's `target_ms`, and it is gated below.
    # It held at exactly 16.64 ms in every capture taken across this
    # investigation, including the ones where everything else looked broken.
    ap.add_argument("--max-cost-p95-ms", type=float, default=None,
                    help="max median emulation-work p95 ms (default: reported, not "
                         "gated — scales with run_ahead by design)")
    ap.add_argument("--max-produced-dropped", type=int, default=None,
                    help="max cumulative dropped produced frames (default: reported, "
                         "not gated — a property of the display, not the emulator)")
    ap.add_argument("--max-rate-skew-pct", type=float, default=0.5,
                    help="max |produced_mean - target_ms| as %% of target (default "
                         "0.5%%, the audio DRC band: the emulator must keep console "
                         "time regardless of host display configuration)")
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
    # Cumulative counters: take the MAXIMUM readable value, not the final row.
    #
    # Every timed capture ends by killing the frontend mid-write, so the last
    # row is routinely short and `col_float` fills its missing fields with 0.0.
    # Reading these from `last` therefore reset them to zero exactly when a run
    # was cut off — and a capture could PASS while an earlier row had already
    # exceeded the gate. Since they are monotonic within one capture, the max is
    # both correct and immune to truncation. (Same defect class as the
    # `float(None)` crash fixed earlier in this file: the short final row is a
    # normal event here, not an anomaly.) `produced_max` is a windowed peak, so
    # it takes the max for a different reason.
    underruns = max(int(col_float(r, "underruns")) for r in body)
    catchup = max(int(col_float(r, "catchup_bursts")) for r in body)
    snaps = max(int(col_float(r, "snap_forwards")) for r in body)
    produced_max = max(col_float(r, "produced_max_ms") for r in body)
    # p99 is taken as the MEDIAN across sample rows, not the max: each row already
    # reports a windowed p99, so the median of those is the run's typical tail and
    # is not itself hostage to one bad window (the failure mode `produced_max` has).
    produced_p99 = _median([col_float(r, "produced_p99_ms") for r in body])
    presented_p99 = _median([col_float(r, "presented_p99_ms") for r in body])
    # Emulation work per displayed frame, pacer sleep excluded. `cost_*` and
    # `produced_dropped` arrived after some archived captures were taken, so an
    # absent column yields NaN / 0 and simply does not gate — an old CSV is not
    # retroactively a failure.
    cost_p95 = (
        _median([col_float(r, "cost_p95_ms") for r in body])
        if "cost_p95_ms" in header
        else float("nan")
    )
    # Cumulative and monotonic like the three above, so read it the same way —
    # from the maximum across the analysed rows, not from a final row that a
    # mid-write kill routinely leaves short. Missing entirely in archived
    # captures taken before the column existed, hence the header guard.
    dropped = (
        max(int(col_float(r, "produced_dropped")) for r in body)
        if "produced_dropped" in header
        else 0
    )

    failures: list[str] = []
    if underruns > args.max_underruns:
        failures.append(f"underruns {underruns} > {args.max_underruns}")
    if produced_max > args.max_produced_ms:
        failures.append(f"produced_max {produced_max:.1f} ms > {args.max_produced_ms} ms")
    # `None` (the default) reports the p99 without gating it — see the note by
    # the argument definitions for why an absolute p99 threshold measures the
    # host's display rather than frontend health.
    if args.max_produced_p99_ms is not None and produced_p99 > args.max_produced_p99_ms:
        failures.append(
            f"produced_p99 {produced_p99:.1f} ms > {args.max_produced_p99_ms} ms")
    if args.max_presented_p99_ms is not None and presented_p99 > args.max_presented_p99_ms:
        failures.append(
            f"presented_p99 {presented_p99:.1f} ms > {args.max_presented_p99_ms} ms")
    if catchup > args.max_catchup_bursts:
        failures.append(f"catchup_bursts {catchup} > {args.max_catchup_bursts}")
    if snaps > args.max_snap_forwards:
        failures.append(f"snap_forwards {snaps} > {args.max_snap_forwards}")
    if (
        args.max_cost_p95_ms is not None
        and cost_p95 == cost_p95  # NaN-safe
        and cost_p95 > args.max_cost_p95_ms
    ):
        failures.append(
            f"cost_p95 {cost_p95:.1f} ms > {args.max_cost_p95_ms} ms "
            f"(emulation work does not fit in a frame)")
    if args.max_produced_dropped is not None and dropped > args.max_produced_dropped:
        failures.append(f"produced_dropped {dropped} > {args.max_produced_dropped}")

    # v2.3.3 — the rate gate: does the emulator keep console time?
    meta = load_meta(args.csv)
    try:
        target_ms = float(meta.get("target_ms", ""))
    except ValueError:
        target_ms = 0.0
    produced_mean = _median([col_float(r, "produced_mean_ms") for r in body])
    if target_ms > 0.0 and produced_mean > 0.0:
        skew_pct = abs(produced_mean - target_ms) / target_ms * 100.0
        print(f"  produced_mean {produced_mean:.3f} ms vs target {target_ms:.3f} ms "
              f"({skew_pct:+.3f}%)")
        if skew_pct > args.max_rate_skew_pct:
            failures.append(
                f"console rate skew {skew_pct:.3f}% > {args.max_rate_skew_pct}% "
                f"(produced_mean {produced_mean:.3f} ms vs target {target_ms:.3f} ms)")

    print(f"perf_log_check: {args.csv}")
    print(f"  rows={len(rows)} (analyzed {len(body)} after {args.warmup_rows} warmup)")
    print(f"  underruns={underruns}  produced_max={produced_max:.1f}ms  "
          f"catchup_bursts={catchup}  snap_forwards={snaps}")
    print(f"  cost_p95={cost_p95:.2f}ms (emulation work)  produced_dropped={dropped}")
    print(f"  produced_p99={produced_p99:.2f}ms  presented_p99={presented_p99:.2f}ms  "
          f"(NTSC budget 16.639 ms)")
    if failures:
        print("  FAIL: " + "; ".join(failures))
        return 1
    print("  OK: all tracked signals within bounds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
