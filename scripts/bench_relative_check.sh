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
# Env knobs:
#   BENCH_BASE_REF            base commit-ish (default HEAD~1)
#   BENCH_MAX_REGRESSION_PCT  fail above this % slower (default 10)
#   BENCH_MEASUREMENT_TIME    criterion measurement seconds (default 3)
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd)"

BASE_REF="${1:-${BENCH_BASE_REF:-HEAD~1}}"
MAX_REGRESSION_PCT="${BENCH_MAX_REGRESSION_PCT:-10}"
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

rc=0
printf '\n%-32s %12s %12s %10s\n' "bench" "base (ms)" "head (ms)" "delta"
printf '%-32s %12s %12s %10s\n' "-----" "---------" "---------" "-----"
for id in "${BENCH_IDS[@]}"; do
    base_ns="$(mean_ns "${id}" relgate_base)"
    head_ns="$(mean_ns "${id}" relgate_head)"
    if [[ "${base_ns}" == "MISSING" || "${head_ns}" == "MISSING" ]]; then
        echo "FAIL: ${id}: criterion estimates missing (base=${base_ns} head=${head_ns})"
        rc=1
        continue
    fi
    read -r base_ms head_ms delta_pct <<<"$(python3 - "$base_ns" "$head_ns" <<'PY'
import sys
b, h = int(sys.argv[1]), int(sys.argv[2])
print(f"{b/1e6:.3f} {h/1e6:.3f} {(h - b) / b * 100:+.2f}")
PY
)"
    printf '%-32s %12s %12s %9s%%\n' "${id}" "${base_ms}" "${head_ms}" "${delta_pct}"
    over="$(python3 -c "print('1' if ${delta_pct} > ${MAX_REGRESSION_PCT} else '0')")"
    if [[ "${over}" == "1" ]]; then
        echo "FAIL: ${id} regressed ${delta_pct}% (limit ${MAX_REGRESSION_PCT}%)"
        rc=1
    fi
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
echo "==> Relative frame-time gate passed (no bench regressed beyond ${MAX_REGRESSION_PCT}%)."
