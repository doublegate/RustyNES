#!/usr/bin/env bash
# v2.8.0 Phase 4 — profile-guided-optimization recipe for the shipping
# `rustynes-v2` binary, adapted from Mesen2's `buildPGO.sh`/`PGOHelper`
# (instrument -> train on a ROM sweep at maximum speed -> merge -> rebuild).
#
# Prerequisites (one-time):
#   cargo install cargo-pgo
#   rustup component add llvm-tools-preview
#
# Training corpus: the committed CC0/MIT test ROMs always work; drop your
# own game dumps (never committed) into tests/roms/external/PGOGames/ for a
# more representative profile (varied mappers + heavy scrollers recommended:
# NROM / MMC1 / MMC3 / MMC5 / VRC6 titles).
#
# Usage:  scripts/pgo/run.sh [frames-per-rom]
#   The optimized binary lands at target/<triple>/release/rustynes-v2.
#   Compare against the plain release build with
#   `cargo bench -p nes-core --bench full_frame` or a frame-time soak.
set -euo pipefail
cd "$(dirname "$0")/../.."

FRAMES="${1:-3600}" # ~60 s of NTSC gameplay per ROM at full speed

command -v cargo-pgo >/dev/null || {
    echo "error: cargo-pgo not installed (cargo install cargo-pgo)" >&2
    exit 1
}

echo "== 1/3 instrumented build (trainer + shared core crates) =="
cargo pgo build -- -p nes-test-harness --bin pgo_trainer

echo "== 2/3 training run (${FRAMES} frames per ROM, scripted input) =="
TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
"target/${TRIPLE}/release/pgo_trainer" "${FRAMES}"

echo "== 3/3 optimized build of the shipping frontend =="
cargo pgo optimize build -- -p nes-frontend

echo "done: target/${TRIPLE}/release/rustynes-v2 (PGO-optimized)"
echo "Optional extra: 'cargo pgo bolt build -- -p nes-frontend' chains BOLT"
echo "post-link optimization on Linux (adopt only if it adds >2%)."
