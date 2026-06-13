#!/usr/bin/env bash
# bench_regression_check.sh — headless frame-time regression gate (v1.6.0).
#
# Runs the `rustynes-core` `full_frame` criterion benches and asserts each stays
# under an ABSOLUTE wall-clock ceiling. This is a deliberately non-flaky gate:
# shared CI runners vary by tens of percent run-to-run, so a tight
# percentage-regression gate would flake. The ceiling instead protects the
# property that actually matters — headless frame production stays comfortably
# under the 16.67 ms NTSC real-time deadline — and trips only on a gross (≈3x+)
# regression. For the tighter ~5% comparison, use criterion baselines locally:
#
#     cargo bench -p rustynes-core --bench full_frame -- --save-baseline main
#     # ... make changes ...
#     cargo bench -p rustynes-core --bench full_frame -- --baseline main
#
# Ceilings are in nanoseconds and overridable via env (CI can loosen them for a
# slow runner without editing this file). Defaults give ~3x margin over the
# ~2-3 ms/frame measured on a 2026-era dev machine, well under 16.67 ms.
set -euo pipefail

cd "$(dirname "$0")/.."

MEASUREMENT_TIME="${BENCH_MEASUREMENT_TIME:-3}"
# ceiling (ns) per bench id; 10 ms = 60% of the 16.67 ms NTSC frame deadline.
NESTEST_CEILING_NS="${NESTEST_CEILING_NS:-10000000}"
FLOWING_CEILING_NS="${FLOWING_CEILING_NS:-10000000}"

echo "==> Running full_frame benches (measurement-time=${MEASUREMENT_TIME}s)"
cargo bench -p rustynes-core --bench full_frame -- \
    --warm-up-time 1 --measurement-time "${MEASUREMENT_TIME}"

check() {
    local id="$1" ceiling="$2"
    local est="target/criterion/${id}/new/estimates.json"
    if [[ ! -f "${est}" ]]; then
        echo "FAIL: ${id}: estimates file not found (${est})"
        return 1
    fi
    local mean
    mean="$(python3 -c "import json,sys; print(int(json.load(open(sys.argv[1]))['mean']['point_estimate']))" "${est}")"
    local mean_ms
    mean_ms="$(python3 -c "print(f'{${mean}/1e6:.3f}')")"
    local ceiling_ms
    ceiling_ms="$(python3 -c "print(f'{${ceiling}/1e6:.3f}')")"
    if (( mean > ceiling )); then
        echo "FAIL: ${id}: ${mean_ms} ms/frame exceeds ceiling ${ceiling_ms} ms"
        return 1
    fi
    echo "PASS: ${id}: ${mean_ms} ms/frame (ceiling ${ceiling_ms} ms)"
    return 0
}

rc=0
check "nes_run_frame_nestest" "${NESTEST_CEILING_NS}" || rc=1
check "nes_run_frame_flowing_palette" "${FLOWING_CEILING_NS}" || rc=1

if (( rc != 0 )); then
    echo "==> Frame-time regression gate FAILED — see docs/performance.md."
    exit 1
fi
echo "==> Frame-time regression gate passed."
