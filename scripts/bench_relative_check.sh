#!/usr/bin/env bash
# bench_relative_check.sh — same-runner RELATIVE frame-time regression gate.
#
# Companion to `bench_regression_check.sh`, which asserts an ABSOLUTE ceiling
# (~10 ms vs the 16.67 ms NTSC deadline). That ceiling is deliberately loose so
# it never flakes, but the looseness is a real hole: on the ~4 ms/frame the core
# actually runs at, a change could get **2.5x slower** and still pass. This gate
# closes it.
#
# ## Why a percentage gate is safe here when it was not before
#
# `bench_regression_check.sh` says a tight percentage gate "would flake on
# shared runners", and for a CROSS-run comparison (this run's number vs a figure
# recorded on some other machine) that is correct — hosted runners differ by
# tens of percent.
#
# This gate never does that. It builds and benches BOTH commits back to back on
# the SAME runner, in the same job, sharing one target dir, and compares them to
# each other. Runner-to-runner variance is common-mode and cancels. That is the
# identical technique `pgo.yml` relies on for its >3% promotion bar, and the
# measured back-to-back noise floor on a quiet host is ±0.7% (see
# `docs/performance.md` §P2, where an identical configuration benched against
# its own baseline reported "no change" on all four workloads, p > 0.05).
#
# The default threshold is nevertheless a generous 10%, not 3%: a CI runner is
# noisier than a quiet desktop, and this gate's job is to catch the gross
# regression that the absolute ceiling sleeps through, not to adjudicate a 2%
# micro-optimization. Tightening it is a decision to make once there is CI
# noise data to justify it, not up front.
#
# ## Usage
#
#   scripts/bench_relative_check.sh [BASE_REF]
#
# BASE_REF defaults to $BENCH_BASE_REF, then to `HEAD~1`. If the base commit
# cannot be resolved (shallow clone, root commit, unknown ref) the gate SKIPS
# with a clear message and exit 0 — a gate that cannot establish a baseline must
# not manufacture a verdict.
#
# ## Host contention: the gate refuses to guess (v2.3.1)
#
# The cancellation argument above holds only while the two back-to-back runs see
# a comparable machine. On a contended host they do not: whichever run happens to
# land next to the noisy neighbour is inflated, the "common-mode" assumption
# breaks, and the delta stops measuring the code. v2.3.0 P1 is the worked example
# — profiled on a busy machine it read +2%; re-measured quiet, the same commit
# was -5.13%. The number was not merely imprecise, it had the wrong sign.
#
# So this gate now reads criterion's own contention evidence and declines to
# conclude when the host was too noisy to support a conclusion:
#
#   * **robust CV** — `1.4826 * MAD / median`. This is the trigger. Unlike
#     stddev it is not itself dragged around by the outliers being measured, so
#     it stays a usable yardstick on exactly the contended runs that matter.
#   * **outlier %** — computed the way criterion computes it: each sample's
#     per-iteration average against the Tukey fences criterion already wrote to
#     `tukey.json`. Reported as evidence only; deliberately NOT the trigger.
#
# Outlier % looks like the obvious signal and is a trap here. Criterion's fences
# are IQR-derived, so a benchmark whose bulk is unusually *tight* flags a large
# outlier fraction from tiny absolute excursions. Measured on this repo's own
# saved baselines: `nes_run_frame_flowing_palette_fast` reports **30% outliers
# at 0.19% robust CV** (a superbly quiet run), while `nes_run_frame_nestest_fast`
# reports **0% outliers at 2.79% CV**. The two signals not only disagree, they
# invert. Gating on outlier % would have refused a verdict on the quietest run
# in the set.
#
# The CV threshold is not a magic constant either: it is derived from the effect
# size the gate exists to detect. A gate cannot adjudicate an effect it cannot
# resolve, so the host counts as contended once `3 * CV` exceeds
# `BENCH_MAX_REGRESSION_PCT` — i.e. once the noise band is wide enough to
# swallow the very regression being tested for. At the default 10% limit that is
# a 3.33% CV.
#
# When contended the gate emits **NO VERDICT** (exit 0, loudly) rather than a
# pass or a fail — a clean delta on a noisy host is not evidence of no regression
# any more than a dirty one is evidence of a regression. The one exception is a
# delta that dwarfs even the inflated noise (more than 3x the measured CV):
# contention inflates a measurement, it does not invent a 40% one, so that still
# FAILs. The absolute ceiling in `bench_regression_check.sh` applies either way,
# so declining here never leaves the branch ungated.
#
# Env knobs:
#   BENCH_BASE_REF            base commit-ish (default HEAD~1)
#   BENCH_MAX_REGRESSION_PCT  fail above this % slower (default 10)
#   BENCH_MAX_NOISE_CV_PCT    above this robust CV %, emit NO VERDICT instead of
#                             pass/fail (default BENCH_MAX_REGRESSION_PCT / 3)
#   BENCH_MEASUREMENT_TIME    criterion measurement seconds (default 3)
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd)"

BASE_REF="${1:-${BENCH_BASE_REF:-HEAD~1}}"
MAX_REGRESSION_PCT="${BENCH_MAX_REGRESSION_PCT:-10}"
# Default derived from the regression limit rather than picked: the gate declines
# once the noise band (3x CV) is wide enough to swallow the effect it is testing
# for. Overridable, but the derivation is the point.
# Derived with awk, which treats the value as DATA. The obvious form
# interpolates ${MAX_REGRESSION_PCT} into inline Python source, so a non-numeric
# BENCH_MAX_REGRESSION_PCT would either break parsing or execute as code.
MAX_NOISE_CV_PCT="${BENCH_MAX_NOISE_CV_PCT:-$(awk -v r="${MAX_REGRESSION_PCT}" \
    'BEGIN { if (r + 0 <= 0) { print "3.33" } else { printf "%.2f", (r + 0) / 3 } }')}"
# An OVERRIDE is copied verbatim, so validate it here rather than letting a value
# like `3oops` reach the python comparison below and die mid-run with a traceback
# after both benches have already been paid for.
case "${MAX_NOISE_CV_PCT}" in
    ''|*[!0-9.]*|*.*.*)
        echo "bench_relative_check: BENCH_MAX_NOISE_CV_PCT must be a number," \
             "got '${MAX_NOISE_CV_PCT}'" >&2
        exit 2
        ;;
esac
case "${MAX_REGRESSION_PCT}" in
    ''|*[!0-9.]*|*.*.*)
        echo "bench_relative_check: BENCH_MAX_REGRESSION_PCT must be a number," \
             "got '${MAX_REGRESSION_PCT}'" >&2
        exit 2
        ;;
esac
MEASUREMENT_TIME="${BENCH_MEASUREMENT_TIME:-3}"
BENCH_IDS=(nes_run_frame_nestest nes_run_frame_flowing_palette)

# ---- Resolve the base commit, or skip -------------------------------------
if ! base_sha="$(git rev-parse --verify --quiet "${BASE_REF}^{commit}")"; then
    echo "SKIP: cannot resolve base ref '${BASE_REF}' (shallow clone or root commit?)."
    echo "      The relative gate needs both commits; the absolute ceiling in"
    echo "      bench_regression_check.sh still applies."
    exit 0
fi
head_sha="$(git rev-parse HEAD)"
if [[ "${base_sha}" == "${head_sha}" ]]; then
    echo "SKIP: base and HEAD are the same commit (${head_sha:0:12}) — nothing to compare."
    exit 0
fi

echo "==> Relative frame-time gate"
echo "    base: ${base_sha:0:12} (${BASE_REF})"
echo "    head: ${head_sha:0:12}"
echo "    fail if HEAD is more than ${MAX_REGRESSION_PCT}% slower"
echo "    no verdict above ${MAX_NOISE_CV_PCT}% robust CV (host too noisy to resolve it)"

# Recorded purely as evidence in the log: a reader diagnosing a NO VERDICT wants
# to know what the machine was doing. Nothing branches on this — load average is
# a lagging 1-minute figure and the runner may not be Linux, so the actual
# contention decision is made from criterion's own per-sample data below.
if [[ -r /proc/loadavg ]]; then
    echo "    host: load avg $(cut -d' ' -f1-3 /proc/loadavg) across $(nproc 2>/dev/null || echo '?') cpus"
fi

# ---- Bench the BASE commit in a throwaway worktree ------------------------
# A worktree, never `git checkout`: this script must not touch the working tree
# it was invoked from. Uncommitted work stays untouched even if the run dies.
work_dir="$(mktemp -d)"
cleanup() {
    git worktree remove --force "${work_dir}/base" >/dev/null 2>&1 || true
    rm -rf "${work_dir}"
}
trap cleanup EXIT

git worktree add --detach "${work_dir}/base" "${base_sha}" >/dev/null

# Both benches share ONE target dir, which is what makes the comparison
# same-runner AND lets criterion see the saved baseline from the other
# checkout (it stores under $CARGO_TARGET_DIR/criterion). It also shares
# compiled dependencies, so the base build is far cheaper than a cold one.
export CARGO_TARGET_DIR="${repo_root}/target"

echo "==> Benching BASE (${base_sha:0:12})"
(
    cd "${work_dir}/base"
    cargo bench -p rustynes-core --bench full_frame -- \
        --warm-up-time 1 --measurement-time "${MEASUREMENT_TIME}" \
        --save-baseline relgate_base
)

echo "==> Benching HEAD (${head_sha:0:12})"
cargo bench -p rustynes-core --bench full_frame -- \
    --warm-up-time 1 --measurement-time "${MEASUREMENT_TIME}" \
    --save-baseline relgate_head

# ---- Compare -------------------------------------------------------------
# Read the two saved baselines directly rather than criterion's `change/`
# estimates: `change/` is only written when a `--baseline` comparison ran, and
# reading the means ourselves keeps the arithmetic visible in the log.
mean_ns() {
    local id="$1" which="$2"
    local est="${CARGO_TARGET_DIR}/criterion/${id}/${which}/estimates.json"
    [[ -f "${est}" ]] || { echo "MISSING"; return; }
    python3 -c "import json,sys; print(int(json.load(open(sys.argv[1]))['mean']['point_estimate']))" "${est}"
}

# Contention evidence for one saved baseline, straight from criterion's own
# artifacts: `sample.json` (per-sample iteration counts + elapsed times) and
# `tukey.json` (the four Tukey fences criterion already computed). Emits
# "<outlier%> <robust-CV%>", or "MISSING" when either artifact is absent.
noise_of() {
    local id="$1" which="$2"
    local dir="${CARGO_TARGET_DIR}/criterion/${id}/${which}"
    [[ -f "${dir}/sample.json" && -f "${dir}/tukey.json" ]] || { echo "MISSING"; return; }
    python3 - "${dir}" <<'PY'
import json, statistics, sys

d = sys.argv[1]
sample = json.load(open(f"{d}/sample.json"))
# Criterion classifies on the per-sample AVERAGE (elapsed / iterations), which is
# also the scale its Tukey fences are expressed in. Guard against a zero iters
# entry rather than trusting the file.
avgs = [t / i for t, i in zip(sample["times"], sample["iters"]) if i]
if not avgs:
    print("MISSING")
    raise SystemExit

# tukey.json is [lo_severe, lo_mild, hi_mild, hi_severe]; anything outside the
# mild fences is an outlier, matching criterion's "Found N outliers" tally.
_, lo_mild, hi_mild, _ = json.load(open(f"{d}/tukey.json"))
outliers = sum(1 for a in avgs if a < lo_mild or a > hi_mild)

# Robust spread: MAD scaled to a normal-consistent sigma. Unlike stddev this is
# not itself inflated by the very outliers being measured, so it stays a usable
# yardstick on exactly the contended runs this gate cares about.
med = statistics.median(avgs)
mad = statistics.median([abs(a - med) for a in avgs])
cv = (1.4826 * mad / med * 100) if med else 0.0
print(f"{outliers / len(avgs) * 100:.1f} {cv:.2f}")
PY
}

rc=0
contended=0
printf '\n%-32s %11s %11s %9s %9s %8s\n' \
    "bench" "base (ms)" "head (ms)" "delta" "outliers" "noise"
printf '%-32s %11s %11s %9s %9s %8s\n' \
    "-----" "---------" "---------" "-----" "--------" "-----"
for id in "${BENCH_IDS[@]}"; do
    base_ns="$(mean_ns "${id}" relgate_base)"
    head_ns="$(mean_ns "${id}" relgate_head)"
    if [[ "${base_ns}" == "MISSING" || "${head_ns}" == "MISSING" ]]; then
        echo "FAIL: ${id}: criterion estimates missing (base=${base_ns} head=${head_ns})"
        rc=1
        continue
    fi

    base_noise="$(noise_of "${id}" relgate_base)"
    head_noise="$(noise_of "${id}" relgate_head)"
    if [[ "${base_noise}" == "MISSING" || "${head_noise}" == "MISSING" ]]; then
        # Fall back to the pre-v2.3.1 behaviour rather than skipping: without
        # sample data we cannot show the host was quiet, but we also cannot show
        # it was noisy, and the delta itself is still a real measurement.
        out_pct="n/a"
        noise_pct="n/a"
        bench_contended=0
    else
        read -r base_out base_cv <<<"${base_noise}"
        read -r head_out head_cv <<<"${head_noise}"
        read -r out_pct noise_pct bench_contended <<<"$(python3 - \
            "$base_out" "$head_out" "$base_cv" "$head_cv" "$MAX_NOISE_CV_PCT" <<'PY'
import sys
bo, ho, bc, hc, limit = (float(x) for x in sys.argv[1:6])
# Worst case of the two runs on each axis: if EITHER commit was measured on a
# noisy machine, the comparison between them is compromised.
out, cv = max(bo, ho), max(bc, hc)
print(f"{out:.1f} {cv:.2f} {1 if cv > limit else 0}")
PY
)"
        (( bench_contended )) && contended=1
    fi

    read -r base_ms head_ms delta_pct <<<"$(python3 - "$base_ns" "$head_ns" <<'PY'
import sys
b, h = int(sys.argv[1]), int(sys.argv[2])
print(f"{b/1e6:.3f} {h/1e6:.3f} {(h - b) / b * 100:+.2f}")
PY
)"
    printf '%-32s %11s %11s %8s%% %8s%% %7s%%\n' \
        "${id}" "${base_ms}" "${head_ms}" "${delta_pct}" "${out_pct}" "${noise_pct}"

    over="$(python3 -c "print('1' if ${delta_pct} > ${MAX_REGRESSION_PCT} else '0')")"
    [[ "${over}" == "1" ]] || continue

    # Over the limit on a QUIET host is a regression. Over the limit on a noisy
    # one is only a regression if it is too large for that noise to explain —
    # 3x the measured robust CV. Contention inflates a measurement; it does not
    # invent a 40% one.
    if (( bench_contended )); then
        dwarfs="$(python3 -c "print('1' if ${delta_pct} > 3 * ${noise_pct} else '0')")"
        if [[ "${dwarfs}" != "1" ]]; then
            echo "NOTE: ${id} is ${delta_pct}% slower, but the host was contended"
            echo "      (${out_pct}% outliers, ${noise_pct}% robust CV) and the delta is"
            echo "      within 3x that noise — not attributable to the code change."
            continue
        fi
        echo "FAIL: ${id} regressed ${delta_pct}% — beyond 3x the ${noise_pct}% measured"
        echo "      noise, so host contention cannot account for it."
        rc=1
        continue
    fi
    echo "FAIL: ${id} regressed ${delta_pct}% (limit ${MAX_REGRESSION_PCT}%)"
    rc=1
done
echo

if (( rc != 0 )); then
    cat <<EOF
==> Relative frame-time gate FAILED.

A regression beyond ${MAX_REGRESSION_PCT}% on a same-runner back-to-back A/B is
not runner noise. Reproduce locally:

    cargo bench -p rustynes-core --bench full_frame -- --save-baseline base
    git checkout <your-branch>
    cargo bench -p rustynes-core --bench full_frame -- --baseline base

If the regression is intentional and justified, record the measurement in
docs/performance.md (that file documents changes that did NOT clear the bar as
well as ones that did) and raise BENCH_MAX_REGRESSION_PCT for this run.
EOF
    exit 1
fi

if (( contended )); then
    cat <<EOF
==> Relative frame-time gate: NO VERDICT (host too noisy to resolve the effect).

Measured sample spread exceeded ${MAX_NOISE_CV_PCT}% robust CV, so the noise band
(3x CV) is wide enough to swallow the ${MAX_REGRESSION_PCT}% regression this gate
tests for. The two back-to-back runs did not see a comparable machine, and the
common-mode cancellation the gate depends on does not hold. This is reported as
neither a pass nor a fail on purpose: a clean delta measured on a noisy host is
not evidence that nothing regressed. (v2.3.0 P1 read +2% contended and -5.13%
quiet — the same commit, opposite signs.)

No regression large enough to outrun the measured noise was found, so this does
not block. The absolute ceiling in bench_regression_check.sh still applies.

To get a real verdict, re-run on a quiet machine — or locally:

    scripts/bench_relative_check.sh ${BASE_REF}
EOF
    exit 0
fi
echo "==> Relative frame-time gate passed (no bench regressed beyond ${MAX_REGRESSION_PCT}%,"
echo "    on a host quiet enough for the comparison to mean something)."
