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


def read_anchor(path: Path) -> tuple[float | None, int | None]:
    """``(offset_seconds, clock_id)`` recovered from the trace's comment rows.

    Scans the WHOLE file, not just the header. ``clock_id`` cannot be in the
    header: it is only known after the Wayland registry answers, which happens
    several frames after the trace file is opened, so it is appended as a
    comment row at the point it becomes available (see
    ``PerfLogger::note_clock_id``). Traces written before that fix carry
    ``clock_id = unknown`` and nothing else, which is why the join below is
    verified empirically rather than trusted.
    """
    off: float | None = None
    clk: int | None = None
    with path.open() as fh:
        for ln in fh:
            if not ln.startswith("#"):
                continue
            if "anchor_mono_ns" in ln and "none" not in ln:
                try:
                    off = int(ln.split("=", 1)[1].strip()) / 1e9
                except ValueError:
                    off = None
            elif "clock_id" in ln and "unknown" not in ln:
                try:
                    clk = int(ln.split("=", 1)[1].strip())
                except ValueError:
                    clk = None
    return off, clk


def join_is_sound(
    produce: list[float], scanout_abs: list[float], offset: float, clk: int | None
) -> tuple[bool, str]:
    """Verify the two halves really are in one clock domain before joining.

    The docstring of this module has always said an unjoinable trace must be
    reported as such rather than joined anyway. It was not enforced: the
    ``clock_id`` was absent from every trace ever written, and the analysis
    joined regardless. This is the enforcement, and it is deliberately
    EMPIRICAL rather than a ``clk == 1`` assertion — the span test works on the
    traces already on disk, which have no id at all, and it would also catch a
    compositor that reports ``CLOCK_MONOTONIC`` and then stamps something else.

    Shifted scanouts must cover substantially the same wall-clock interval as
    the produce rows. A different clock domain (``CLOCK_REALTIME``, a
    per-compositor epoch) misses by years, not milliseconds.
    """
    if not produce or not scanout_abs:
        return False, "no rows to join"
    lo_p, hi_p = produce[0], produce[-1]
    lo_s, hi_s = scanout_abs[0] - offset, scanout_abs[-1] - offset
    span = max(hi_p - lo_p, 1e-9)
    skew = max(abs(lo_s - lo_p), abs(hi_s - hi_p))
    if skew > 0.05 * span + 1.0:
        return False, (
            f"clock domains disagree: produce spans {lo_p:.3f}-{hi_p:.3f} s, "
            f"shifted scanouts span {lo_s:.3f}-{hi_s:.3f} s (skew {skew:.3f} s)"
        )
    # `clk or ...` would be wrong here: POSIX `CLOCK_REALTIME` is 0, which is
    # falsey, so a compositor stamping REALTIME — precisely the case this check
    # exists to catch — would be reported as "unrecorded" rather than named.
    if clk == 1:
        named = "confirmed CLOCK_MONOTONIC"
    elif clk is None:
        named = "id unrecorded"
    else:
        named = f"id={clk}" + (" = CLOCK_REALTIME, NOT monotonic" if clk == 0 else "")
    return True, f"join verified by span overlap (skew {skew * 1000:.0f} ms, {named})"


def display_cadence(rows: list[dict], warmup_s: float) -> None:
    """THE display-duration metric: did each present alternate as the divisor demands?

    ``since_present`` is the count of frames produced since the previous
    present, recorded on the present itself. At divisor D the healthy pattern is
    D-1 presents carrying nothing, then one carrying a frame — so the **gap
    between successive frame-carrying presents is exactly D, every time**. A gap
    of D+1 is a frame that stayed on screen a refresh too long; D-1, one too
    short. That is what the eye reads as judder.

    The divisor is INFERRED as the modal gap rather than assumed, because the
    trace does not record it. The first version of this function tested
    run-lengths of equal values against 1, which is the correct test only at
    divisor 2: at divisor 3 the healthy sequence is ``0,0,1,0,0,1``, whose runs
    are ``2,1,2,1``, so a perfectly-paced 180 Hz panel would have scored ~50%
    wrong. Raised in review on PR #362 — an assumption that happened to hold on
    the reporting host, which is the kind this campaign keeps having to correct.

    This is a purely DISPLAY-SIDE series: both the value and the instant come
    from the present. Nothing about when the producer happened to run can leak
    into it, which is exactly the property `scanouts_per_produce` lacks.
    """
    # `r["since_present"] is not None` guards the final row: a capture ends by
    # killing the process, so the last line is routinely a partial write and
    # `DictReader` fills the missing fields with `None`. Without this the whole
    # analysis dies on a `TypeError` at the very last row of a good trace.
    sp = [
        int(r["since_present"])
        for r in rows
        if r["event"] == "present"
        and r["since_present"] is not None
        and r["t_s"] is not None
        and float(r["t_s"]) >= warmup_s
    ]
    # Index of every present that carried at least one new frame; the gaps
    # between them are the per-frame display durations, in refreshes.
    carried = [i for i, v in enumerate(sp) if v > 0]
    gaps = [b - a for a, b in zip(carried, carried[1:])]
    # A rate needs a denominator worth dividing by. Without this a four-event
    # trace prints "1/4 = 25.00%" beside a real 3724-sample measurement, in the
    # same column, with nothing to say which is which.
    if len(gaps) < 100:
        print(f"\n  [display cadence] only {len(gaps)} displayed frames after "
              "warmup — too few to rate")
        return
    hist = Counter(gaps)
    divisor = hist.most_common(1)[0][0]
    bad = len(gaps) - hist[divisor]
    print(f"\n  [display cadence] divisor {divisor} (inferred as the modal gap) — "
          f"frames shown for the WRONG duration: "
          f"{bad}/{len(gaps)} = {100.0 * bad / len(gaps):.2f}%")
    print(f"    refreshes per displayed frame : {dict(sorted(hist.items()))}")


def scanouts_per_produce(
    rows: list[dict], offset: float | None, clk: int | None, warmup_s: float
) -> None:
    """Refreshes between consecutive PRODUCE instants — producer-side jitter.

    **Read the name literally.** This counts scanouts falling between two
    producer timestamps, so a produce that fires 3 ms early followed by one 3 ms
    late scores ``(1, 3)`` **even when the display showed both frames for
    exactly two refreshes each**. It is a measure of how regularly the emulator
    thread ran, NOT of what the display did.

    It was quoted as a display metric in the first version of v2.3.3 F13, and it
    disagrees with the real one by a factor of ~20 on the same captures (32.7%
    vs 1.6% pooled over sixteen). That is the same error as F10 — a display
    claim computed from a producer-side series — committed while building the
    instrument meant to prevent it. Kept, because producer-side regularity is
    worth seeing; renamed and labelled so it cannot be mistaken again.
    """
    sc_abs = sorted(float(r["t_s"]) for r in rows if r["event"] == "scanout")
    if not sc_abs:
        return
    print(f"\n  [scanout] {len(sc_abs)} compositor-reported presentations")
    iv = [(b - a) * 1000.0 for a, b in zip(sc_abs, sc_abs[1:])]
    if iv:
        med = statistics.median(iv)
        q = Counter(max(1, round(x / med)) for x in iv)
        total_refreshes = sum(k * v for k, v in q.items())
        missed = sum(v * (k - 1) for k, v in q.items())
        print(f"    median interval {med:.4f} ms  ->  {1000.0 / med:.3f} scanouts/s")
        print(f"    MISSED refreshes (display repeated the previous image): "
              f"{missed} = {100.0 * missed / max(total_refreshes, 1):.2f}%")
    flags = {r["flags"] for r in rows if r["event"] == "scanout" and r.get("flags")}
    if flags:
        print(f"    presentation flags observed: {sorted(flags)}"
              f"{'  (constant — no per-frame path change)' if len(flags) == 1 else ''}")
    seqs = {r["since_present"] for r in rows if r["event"] == "scanout"}
    if seqs == {"0"}:
        print("    seq: all zero — this compositor does not report a presentation "
              "counter, so missed refreshes above are INFERRED from intervals")
    if offset is None:
        print("    (no anchor_mono_ns header — cannot join to produced frames)")
        return
    # `join_is_sound` compares in the ORIGIN-relative domain — it de-shifts the
    # scanouts — so it must be handed the raw produce times, not ones already
    # shifted into the compositor's domain. Passing the shifted series made the
    # check report a 120130 s skew on a trace that joins perfectly.
    pr_rel = sorted(float(r["t_s"]) for r in rows if r["event"] == "produce")
    ok, why = join_is_sound(pr_rel, sc_abs, offset, clk)
    pr = [t + offset for t in pr_rel]
    print(f"    {why}")
    if not ok:
        return
    # `offset + warmup_s`, NOT `sc_abs[0] + warmup_s`: `warmup_s` is measured
    # from the trace ORIGIN, which is what every other filter in this script
    # uses. Anchoring it to the first scanout instead discards an extra
    # `first_scanout - origin` of valid rows and silently disagrees with the
    # produce/present windows by that amount.
    lo, hi = offset + warmup_s, min(sc_abs[-1], pr[-1] if pr else sc_abs[-1])
    pr = [t for t in pr if lo <= t <= hi]
    sc = [t for t in sc_abs if lo <= t <= hi]
    if len(pr) < 10:
        return
    hold: Counter = Counter()
    for a, b in zip(pr, pr[1:]):
        hold[bisect.bisect_left(sc, b) - bisect.bisect_left(sc, a)] += 1
    tot = sum(hold.values()) or 1
    print("    refreshes between consecutive PRODUCE instants "
          "(producer jitter, NOT display duration):")
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
    display_cadence(rows, warmup_s)
    offset, clk = read_anchor(path)
    scanouts_per_produce(rows, offset, clk, warmup_s)
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
