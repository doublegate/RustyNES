#!/usr/bin/env bash
# ab_check.sh — adjudicate ONE optimization against the >3% adoption bar.
#
# Companion to `bench_relative_check.sh`, and deliberately a different tool
# answering a different question:
#
#   bench_relative_check.sh  CI gate. "Did this commit regress by more than
#                            BENCH_MAX_REGRESSION_PCT (10%)?" Point estimates
#                            plus the 3x-robust-CV contention rule are the right
#                            statistics for that: the question is whether ONE
#                            delta could be noise.
#
#   ab_check.sh (this)       Adoption decision. "Is this change worth keeping at
#                            the >3% bar?" That is a question about the MEAN of
#                            ~100 samples, where the confidence interval governs
#                            and the standard error falls as CV/sqrt(n). Applying
#                            the 3xCV rule here demands a quiet host no desktop
#                            provides and refuses every verdict -- a mistake made
#                            once, in v2.3.2 G1, and recorded in
#                            docs/performance.md so it is not repeated.
#
# So this script defers to criterion's own `--baseline` change analysis, which
# reports a change interval and a p-value. That is what P2/P3/P4 and G1 used.
#
# ## What it compares
#
# The WORKING TREE against a reference (default HEAD), back to back on the same
# host, sharing one target dir. The reference is built in a throwaway git
# worktree -- never a `git checkout`, so uncommitted work is never touched even
# if the run dies. Optionally applies extra cargo features to the candidate side
# only, which is how a default-OFF feature flag is adjudicated (G1 used exactly
# that shape).
#
# ## Usage
#
#   scripts/perf/ab_check.sh                        # working tree vs HEAD
#   scripts/perf/ab_check.sh --base HEAD~1
#   scripts/perf/ab_check.sh --features ppu-idle-line-fast   # flag A/B, same tree
#   scripts/perf/ab_check.sh --bench nes_run_frame_nestest   # one workload
#   AB_MEASUREMENT_TIME=20 scripts/perf/ab_check.sh          # tighter intervals
#
# CPU pinning (`taskset`) is applied when available: measured on this project's
# host it took robust CV from 2.73% to 1.95%, which narrows every confidence
# interval for free.
#
# ## Reading the result
#
# criterion prints, per workload, `change: [lo mid hi] (p = ...)`, and the run
# ends with an A/B/A order-bias control plus the full adoption rule.
#
# The bar is EVIDENCE QUALITY, not effect size (maintainer decision, v2.3.2): a
# consistent, reproduced, statistically clean gain is adoptable even below 3%.
# What is NOT negotiable is the second independent run -- a single run has
# already produced a p=0.00 result on all four workloads that was pure artifact.
# A mixed-sign result across workloads is a rejection, not an average.
#
# Record the outcome in docs/performance.md either way -- including rejections
# with their numbers. That convention is why this campaign could skip so many
# already-settled dead ends.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."
repo_root="$(pwd)"

BASE_REF="HEAD"
FEATURES=""
BENCH_FILTER=""
MEASUREMENT_TIME="${AB_MEASUREMENT_TIME:-10}"
WARMUP="${AB_WARMUP_TIME:-2}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --base) BASE_REF="$2"; shift 2 ;;
        --features) FEATURES="$2"; shift 2 ;;
        --bench) BENCH_FILTER="$2"; shift 2 ;;
        -h|--help) sed -n '2,60p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if ! base_sha="$(git rev-parse --verify --quiet "${BASE_REF}^{commit}")"; then
    echo "SKIP: cannot resolve base ref '${BASE_REF}'." >&2
    exit 0
fi

# Pin to a fixed CPU set when possible. Frequency scaling and scheduler
# migration are the dominant noise sources on a desktop; pinning removes the
# second and stabilises the first.
PIN=()
if command -v taskset >/dev/null 2>&1; then
    ncpu="$(nproc 2>/dev/null || echo 1)"
    if [[ "${ncpu}" -ge 6 ]]; then
        PIN=(taskset -c 2-5)
    fi
fi

work="$(mktemp -d)"
cleanup() {
    git worktree remove --force "${work}/base" >/dev/null 2>&1 || true
    rm -rf "${work}"
}
trap cleanup EXIT

export CARGO_TARGET_DIR="${repo_root}/target"

bench_args=()
[[ -n "${BENCH_FILTER}" ]] && bench_args+=("${BENCH_FILTER}")
bench_args+=(--warm-up-time "${WARMUP}" --measurement-time "${MEASUREMENT_TIME}")

echo "==> Adoption A/B  (bar: >3% faster, whole interval, p < 0.05)"
echo "    reference : ${base_sha:0:12} (${BASE_REF})"
if [[ -n "${FEATURES}" ]]; then
    echo "    candidate : same tree + features '${FEATURES}'"
else
    echo "    candidate : working tree"
fi
[[ ${#PIN[@]} -gt 0 ]] && echo "    pinned    : ${PIN[*]}"
echo "    timing    : ${WARMUP}s warm-up, ${MEASUREMENT_TIME}s measurement"
echo

# ---- Reference side -------------------------------------------------------
# A feature-flag A/B compares the SAME tree with and without the flag, so the
# reference is the working tree too; only a code A/B needs the worktree.
if [[ -n "${FEATURES}" ]]; then
    echo "==> Benching reference (flag off)"
    "${PIN[@]}" cargo bench -p rustynes-core --bench full_frame -- \
        "${bench_args[@]}" --save-baseline ab_ref >/dev/null
else
    echo "==> Benching reference (${base_sha:0:12}) in a throwaway worktree"
    git worktree add --detach "${work}/base" "${base_sha}" >/dev/null
    (
        cd "${work}/base"
        "${PIN[@]}" cargo bench -p rustynes-core --bench full_frame -- \
            "${bench_args[@]}" --save-baseline ab_ref
    ) >/dev/null
fi

# ---- Candidate side, compared against it ----------------------------------
echo "==> Benching candidate, compared against the reference"
echo
feat_args=()
[[ -n "${FEATURES}" ]] && feat_args+=(--features "${FEATURES}")
"${PIN[@]}" cargo bench -p rustynes-core "${feat_args[@]}" --bench full_frame -- \
    "${bench_args[@]}" --baseline ab_ref 2>&1 \
    | grep -E "^nes_run_frame|time:|change:|Performance has|No change" \
    | sed 's/^/  /'

# ---- ORDER-BIAS CONTROL (A/B/A) -------------------------------------------
# The reference is always benched FIRST, so anything that makes the machine
# monotonically faster over the life of the run — page cache warming, CPU
# governor ramping, a background job finishing, thermal/boost settling — is
# indistinguishable from "the candidate is faster". This is not hypothetical:
# v2.3.2 G2's first run reported a clean −1.84%..−2.75% (p=0.00 on all four
# workloads) for a `#[repr(C)]` layout change that, re-measured, showed no
# effect at all. The candidate had not improved; the machine had.
#
# So bench the REFERENCE a second time, last, against its own first run. Any
# change reported below is pure position-in-the-run bias and is the noise floor
# the candidate's numbers must be read against. Ideally it is "No change" on
# every workload; if it is not, the candidate result above is worth exactly as
# much as this drift is small.
echo
echo "==> Order-bias control: re-benching the REFERENCE against itself, last"
echo
if [[ -n "${FEATURES}" ]]; then
    "${PIN[@]}" cargo bench -p rustynes-core --bench full_frame -- \
        "${bench_args[@]}" --baseline ab_ref 2>&1 \
        | grep -E "^nes_run_frame|change:|Performance has|No change" \
        | sed 's/^/  /'
else
    (
        cd "${work}/base"
        "${PIN[@]}" cargo bench -p rustynes-core --bench full_frame -- \
            "${bench_args[@]}" --baseline ab_ref
    ) 2>&1 \
        | grep -E "^nes_run_frame|change:|Performance has|No change" \
        | sed 's/^/  /'
fi

cat <<'EOF'

READ THE ORDER-BIAS CONTROL FIRST. It re-benches the reference against itself,
so whatever it reports is drift from position-in-the-run alone. If it is not
"No change" on every workload, the candidate numbers above carry at least that
much systematic error and a small result is not interpretable.

ADOPTION RULE (maintainer decision, v2.3.2): a consistent, well-established gain
is adoptable even below 3%. The old flat ">3%" bar existed to stop noise-chasing,
not because 2% is worthless -- so the burden moved from EFFECT SIZE to EVIDENCE
QUALITY. Adopt when ALL of:

  * reproduced by a SECOND INDEPENDENT RUN (not a re-read of the same run);
  * p < 0.05 on the workloads that moved;
  * the order-bias control reports no drift;
  * the sign is consistent across workloads -- mixed signs is a rejection, never
    something to average;
  * the shipped `_fast` variants move (fast_dotloop is default-on since v2.2.3,
    so a change that only moves the non-fast variants moves nothing a user runs).

The second run is not optional ceremony. v2.3.2 G2 produced a textbook -1.84%..
-2.75% at p=0.00 on ALL FOUR workloads, from an order-bias artifact; it measured
as exactly zero on re-run. Under a size-only bar that would have been rejected
for being under 3%. Under an evidence-based bar it is rejected for the right
reason -- it was never real.

Record the outcome in docs/performance.md either way, rejections included.
EOF
