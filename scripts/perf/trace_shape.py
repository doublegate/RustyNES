#!/usr/bin/env python3
"""Classify the TEMPORAL SHAPE of a RustyNES per-frame trace.

Why this exists
---------------
Every other perf instrument in this frontend reports percentile summaries over
a ring, sampled once per second. That destroys temporal order, and temporal
order is the whole question: a ``produced`` p95 of 26 ms is equally consistent
with

  * an ALTERNATING short/long cadence  -> reads as a shudder (content appears to
    step forward and back),
  * ISOLATED hitches                   -> reads as an occasional stutter,
  * a slow BEAT between two clocks     -> reads as a periodic wobble,

which feel completely different to a player and have three different causes.
No summary statistic can separate them. This script reads the raw event
sequence written by ``RUSTYNES_FRAME_TRACE=1`` and reports the discriminators
that can.

Usage
-----
    scripts/perf/trace_shape.py perf-logs/trace-<rom>-<utc>.csv [...]

Input columns: ``t_s,event,interval_ms,since_present[,flags]``.

``scanout`` rows carry the COMPOSITOR's own presentation timestamps, in the
clock its ``clock_id`` event names. They become comparable to the
``produce``/``present`` rows only through the ``# anchor_mono_ns`` header line;
without it the two halves are separate series and this script says so rather
than joining them wrongly.
"""

from __future__ import annotations

import argparse
import bisect
import csv
import math
import statistics
import sys
from collections import Counter
from pathlib import Path

# Startup transient to discard: window mapping, shader compilation and the GPU's
# own P8->P0 clock ramp (measured ~7 s on the reporting host) all produce
# present hiccups that say nothing about steady-state pacing.
#
# A HEURISTIC tuned to one machine, not a constant of nature: a host that clears
# its transients sooner has valid steady-state telemetry discarded, and a slower
# one keeps contaminated rows. Overridable with --warmup-s.
DEFAULT_WARMUP_S = 8.0


def lag1_autocorr(xs: list[float]) -> float:
    """Pearson correlation of the series with itself shifted by one.

    Strongly NEGATIVE (< -0.3) means each interval tends to be long when its
    predecessor was short: an alternating cadence, i.e. a shudder. Near zero
    means independent samples (isolated hitches). Strongly positive means runs
    of similar intervals (a drift or beat).
    """
    n = len(xs)
    if n < 3:
        return float("nan")
    mean = statistics.fmean(xs)
    num = sum((xs[i] - mean) * (xs[i + 1] - mean) for i in range(n - 1))
    den = sum((x - mean) ** 2 for x in xs)
    return num / den if den > 0 else float("nan")


def pair_sum_stats(xs: list[float]) -> tuple[float, float, float]:
    """(mean, stdev of consecutive pair sums, stdev of singles).

    The decisive test for alternation. If intervals alternate short/long around
    a stable period, then CONSECUTIVE PAIRS sum to ~2x the period with LOW
    variance while the singles vary widely. If instead the tail is isolated
    hitches, pair variance is comparable to single variance.
    """
    if len(xs) < 4:
        return (float("nan"),) * 3
    pairs = [xs[i] + xs[i + 1] for i in range(0, len(xs) - 1, 2)]
    return statistics.fmean(pairs), statistics.stdev(pairs), statistics.stdev(xs)


def runs(seq: list[int]) -> Counter:
    """Histogram of run lengths of equal consecutive values."""
    out: Counter = Counter()
    if not seq:
        return out
    cur, n = seq[0], 1
    for v in seq[1:]:
        if v == cur:
            n += 1
        else:
            out[n] += 1
            cur, n = v, 1
    out[n] += 1
    return out


def pct(xs: list[float], q: float) -> float:
    """Nearest-rank percentile, matching the Rust side's definition EXACTLY.

    `SampleRing::pick` in `perf.rs` computes ``ceil(q * n) - 1``, clamped into
    range. The first version here used ``round(q * n + 0.5) - 1``, which is not
    the same function: Python's ``round`` is round-half-to-even, so for an exact
    integer ``k = q * n`` it yields ``k`` when ``k`` is even and ``k + 1`` when
    ``k`` is odd, where ``ceil(k)`` is always ``k``. At ``q=0.95, n=100`` that is
    a genuine off-by-one — Rust picks index 94, the old code picked 95.

    A percentile helper that silently disagrees with the in-app statistic by one
    rank is worse than no helper, because the two get quoted side by side.
    """
    if not xs:
        return float("nan")
    s = sorted(xs)
    i = min(len(s) - 1, max(0, math.ceil(q * len(s)) - 1))
    return s[i]


def read_anchor(path: Path) -> float | None:
    """Seconds to add to a ``t_s`` to reach the compositor's clock domain."""
    with path.open() as fh:
        for ln in fh:
            if not ln.startswith("#"):
                return None
            if "anchor_mono_ns" in ln and "none" not in ln:
                try:
                    return int(ln.split("=", 1)[1].strip()) / 1e9
                except ValueError:
                    return None
    return None


def scanout_report(
    path: Path, rows: list[dict], offset: float | None, warmup_s: float
) -> None:
    """Scanouts per produced frame — what the DISPLAY actually showed.

    At divisor N the intended answer is exactly N for every frame. Anything else
    is a frame held for the wrong length of time, which is what the eye reads as
    judder. Requires the anchor: without it the two series are in different
    clock domains and cannot be joined (see v2.3.3 F10, where exactly that
    mistake invalidated a published result).
    """
    sc = sorted(float(r["t_s"]) for r in rows if r["event"] == "scanout")
    if not sc:
        return
    print(f"\n  [scanout] {len(sc)} compositor-reported presentations")
    iv = [(b - a) * 1000.0 for a, b in zip(sc, sc[1:])]
    if iv:
        med = statistics.median(iv)
        q = Counter(max(1, round(x / med)) for x in iv)
        total_refreshes = sum(k * v for k, v in q.items())
        missed = sum(v * (k - 1) for k, v in q.items())
        print(f"    median interval {med:.4f} ms  ->  {1000.0 / med:.3f} scanouts/s")
        print(f"    MISSED refreshes (display repeated the previous image): "
              f"{missed} = {100.0 * missed / max(total_refreshes, 1):.2f}%")
    if offset is None:
        print("    (no anchor_mono_ns header — cannot join to produced frames)")
        return
    pr = sorted(float(r["t_s"]) + offset for r in rows if r["event"] == "produce")
    lo, hi = sc[0] + warmup_s, min(sc[-1], pr[-1] if pr else sc[-1])
    pr = [t for t in pr if lo <= t <= hi]
    sc = [t for t in sc if lo <= t <= hi]
    if len(pr) < 10:
        return
    hold: Counter = Counter()
    for a, b in zip(pr, pr[1:]):
        hold[bisect.bisect_left(sc, b) - bisect.bisect_left(sc, a)] += 1
    tot = sum(hold.values()) or 1
    print("    scanouts per produced frame (divisor N ideal = exactly N):")
    for k in sorted(hold):
        print(f"      {k}: {hold[k]:6}  {100.0 * hold[k] / tot:6.2f}%")


def report(path: Path, warmup_s: float) -> int:
    with path.open() as fh:
        rows = [r for r in csv.DictReader(ln for ln in fh if not ln.startswith("#"))]
    if not rows:
        print(f"{path}: empty trace")
        return 1

    produce = [r for r in rows if r["event"] == "produce" and float(r["t_s"]) >= warmup_s]
    present = [r for r in rows if r["event"] == "present" and float(r["t_s"]) >= warmup_s]
    if len(produce) < 10 or len(present) < 10:
        print(f"{path}: too few post-warmup events ({len(produce)} produce, "
              f"{len(present)} present) — capture longer than {warmup_s:.0f}s")
        return 1

    print(f"\n=== {path.name}")
    print(f"    {len(produce)} produce / {len(present)} present events "
          f"after {warmup_s:.0f}s warmup")

    for label, evs in (("produce", produce), ("present", present)):
        iv = [float(r["interval_ms"]) for r in evs if float(r["interval_ms"]) > 0]
        if len(iv) < 4:
            continue
        ac = lag1_autocorr(iv)
        pmean, pstd, sstd = pair_sum_stats(iv)
        print(f"\n  [{label}] mean {statistics.fmean(iv):.3f} ms   "
              f"p50 {pct(iv, 0.50):.3f}   p95 {pct(iv, 0.95):.3f}   "
              f"p99 {pct(iv, 0.99):.3f}   max {max(iv):.3f}")
        print(f"    lag-1 autocorrelation : {ac:+.3f}   "
              f"{'<-- ALTERNATING' if ac < -0.3 else ('runs/drift' if ac > 0.3 else 'independent')}")
        print(f"    pair-sum mean/stdev   : {pmean:.3f} / {pstd:.3f} ms   "
              f"(single stdev {sstd:.3f} ms)")
        if sstd > 0:
            ratio = pstd / sstd
            # Alternation around a STABLE period cancels in the pair sum, so
            # pair stdev collapses relative to single stdev. This is a test of
            # how CLEAN the alternation is, not of whether it exists — the
            # autocorrelation above answers that. The two can legitimately
            # disagree: a strong negative autocorrelation with a ratio near 1
            # means alternation whose amplitude itself varies, or alternation
            # superimposed on isolated excursions. Report both, conclude from
            # neither alone.
            verdict = ("clean alternation (pairs cancel)" if ratio < 0.5
                       else "amplitude varies / excursions present (pairs do not cancel)")
            print(f"    pair/single stdev     : {ratio:.3f}   <-- {verdict}")

    # Refreshes per produced frame: the divisor cadence actually delivered.
    sp = [int(r["since_present"]) for r in present]
    hist = Counter(sp)
    total = sum(hist.values())
    print("\n  [refreshes per produced frame] "
          "(at divisor N the healthy pattern is a clean alternation of "
          "N-1 zeros then a 1)")
    for k in sorted(hist):
        print(f"    since_present={k:<3} {hist[k]:6}  {100.0 * hist[k] / total:5.1f}%")
    rl = runs(sp)
    print(f"    run-length histogram : {dict(sorted(rl.items()))}")
    scanout_report(path, rows, read_anchor(path), warmup_s)
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(
        description="Classify the temporal shape of a RustyNES per-frame trace"
    )
    ap.add_argument("traces", nargs="+", help="perf-logs/trace-<rom>-<utc>.csv")
    ap.add_argument(
        "--warmup-s",
        type=float,
        default=DEFAULT_WARMUP_S,
        help=f"seconds of startup transient to discard (default {DEFAULT_WARMUP_S}; "
        "window mapping, shader compilation and the GPU clock ramp — a host that "
        "settles sooner should lower this rather than discard valid rows)",
    )
    args = ap.parse_args(argv[1:])
    if args.warmup_s < 0 or not math.isfinite(args.warmup_s):
        ap.error("--warmup-s must be a finite, non-negative number of seconds")
    rc = 0
    for a in args.traces:
        rc |= report(Path(a), args.warmup_s)
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv))
