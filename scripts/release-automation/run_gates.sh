#!/usr/bin/env bash
# run_gates.sh — run every Rust gate CI runs, locally, one cargo job at a time,
# and print one PASS/FAIL line per gate.
#
# Why this exists: CI's gate set is spread across `.github/workflows/ci.yml`,
# and the invocations that catch real defects are the ones a habitual local
# `cargo clippy --workspace` never reaches: each feature set of the frontend
# (`--all-targets` lints DEFAULT features only, so a cfg-gated module is unlinted
# until a command names its feature), both wasm32 builds (unreachable from a
# native target), the trace features of `rustynes-core`, the `no_std` thumbv7em
# build, and the cosim crate, which `--workspace` does not include. The list
# below was transcribed from ci.yml for the v3.2.0 cut. When ci.yml gains a
# gate, add it here in the same change.
#
# It never stops early: a red gate is reported and the next one still runs, so
# one invocation gives the whole picture. Each gate's full output goes to its
# own log; the summary names the log of every failure.
#
# Usage:
#   scripts/release-automation/run_gates.sh                 # everything
#   scripts/release-automation/run_gates.sh --only clippy   # gates whose name matches a regex
#   scripts/release-automation/run_gates.sh --no-roms       # skip the ROM suites (the slow part)
#   GATES_LOG_DIR=/path scripts/release-automation/run_gates.sh
#
# The `test-roms-commercial` gate runs only when local dumps are staged under
# `tests/roms/external/` (gitignored; see AGENTS.md "Never commit commercial
# Nintendo ROMs"). Without them it is reported as SKIP, never as PASS.
#
# Exit status: 0 when every gate that ran passed, 1 otherwise.
set -u

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "run_gates.sh: not inside a git checkout" >&2; exit 2; }
cd "$root" || exit 2

only=""
roms=1
while [ $# -gt 0 ]; do
  case "$1" in
    --only) only="${2:-}"; shift 2 ;;
    --no-roms) roms=0; shift ;;
    -h|--help) sed -n '2,/^# Exit status/p' "$0"; exit 0 ;;
    *) echo "run_gates.sh: unknown argument '$1'" >&2; exit 2 ;;
  esac
done

log_dir="${GATES_LOG_DIR:-${TMPDIR:-/tmp}/rustynes-gates}"
mkdir -p "$log_dir"
echo "HEAD $(git rev-parse --short HEAD) $(git branch --show-current), logs in $log_dir"

failed=0
ran=0
gate() {
  local name="$1"; shift
  if [ -n "$only" ] && ! [[ "$name" =~ $only ]]; then return; fi
  local t0=$SECONDS
  "$@" > "$log_dir/$name.log" 2>&1
  local rc=$?
  ran=$((ran + 1))
  if [ $rc -eq 0 ]; then
    echo "GATE $name: PASS ($((SECONDS - t0))s)"
  else
    failed=$((failed + 1))
    echo "GATE $name: FAIL rc=$rc ($((SECONDS - t0))s) -- $log_dir/$name.log"
  fi
}
skip() {
  if [ -n "$only" ] && ! [[ "$1" =~ $only ]]; then return; fi
  echo "GATE $1: SKIP ($2)"
}

cosim=crates/rustynes-cosim/Cargo.toml

gate fmt cargo fmt --all --check
gate fmt-cosim cargo fmt --manifest-path "$cosim" --check
gate clippy-workspace cargo clippy --workspace --all-targets -- -D warnings
gate clippy-cosim cargo clippy --manifest-path "$cosim" --all-targets --all-features -- -D warnings
for f in cpu-boot-trace irq-timing-trace ppu-state-trace ppu-fetch-trace; do
  gate "clippy-core-$f" cargo clippy -p rustynes-core --all-targets --features "$f" -- -D warnings
done
gate clippy-harness-traces cargo clippy -p rustynes-test-harness --all-targets --features cpu-boot-trace,irq-timing-trace,ppu-state-trace -- -D warnings
gate clippy-harness-phi2 cargo clippy -p rustynes-test-harness --all-targets --features test-roms,phi2-write-sweep -- -D warnings
gate clippy-ppu-phi2-pst cargo clippy -p rustynes-ppu --all-targets --features ppu-state-trace,phi2-write-sweep -- -D warnings
gate clippy-frontend-scripting cargo clippy -p rustynes-frontend --all-targets --features scripting -- -D warnings
gate clippy-frontend-scripting-hdpack cargo clippy -p rustynes-frontend --all-targets --features scripting,hd-pack -- -D warnings
gate clippy-frontend-ra cargo clippy -p rustynes-frontend --all-targets --features retroachievements -- -D warnings
gate clippy-frontend-full cargo clippy -p rustynes-frontend --all-targets --features full -- -D warnings
gate clippy-mappers-noaudio cargo clippy -p rustynes-mappers --no-default-features --all-targets -- -D warnings
gate clippy-harness-commercial cargo clippy -p rustynes-test-harness --all-targets --features commercial-roms,debug-hooks -- -D warnings
gate clippy-harness-roms cargo clippy -p rustynes-test-harness --all-targets --features test-roms,commercial-roms -- -D warnings
gate clippy-wasm cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --lib --bins -- -D warnings
gate clippy-wasm-canvas cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --no-default-features --features wasm-canvas --lib --bins -- -D warnings
gate clippy-wasm-browser-cheevos cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --features browser-cheevos --lib --bins -- -D warnings
gate rustdoc env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
gate rustdoc-cosim env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --manifest-path "$cosim"
gate no-std cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features
# The iOS FFI is `#[cfg(target_os = "ios")]`, so no cargo invocation above
# compiles it; this script type-checks it on the host against a stub renderer.
# Added after v3.2.0's first CI run, where it was the one gate this list
# lacked, and the one that failed (ffi.rs had moved to a zero-copy render).
gate ios-host-typecheck scripts/ios-host-typecheck.sh
gate deny cargo deny check
gate test-workspace cargo test --workspace
gate test-cosim cargo test --manifest-path "$cosim"
gate test-mappers-noaudio cargo test -p rustynes-mappers --no-default-features
gate test-frontend-full cargo test -p rustynes-frontend --lib --features full
gate test-core-features cargo test -p rustynes-core --features debug-hooks,hd-pack
gate test-ppu-features cargo test -p rustynes-ppu --features debug-hooks,hd-pack
gate test-apu-features cargo test -p rustynes-apu --features debug-hooks
gate test-script-features cargo test -p rustynes-script --features script-ipc,script-sqlite
if [ $roms -eq 1 ]; then
  gate test-roms cargo test --workspace --features test-roms
  if [ -d tests/roms/external ] && [ -n "$(ls -A tests/roms/external 2>/dev/null)" ]; then
    gate test-roms-commercial cargo test --workspace --features test-roms,commercial-roms
  else
    skip test-roms-commercial "no dumps staged under tests/roms/external/"
  fi
else
  skip test-roms "--no-roms"
  skip test-roms-commercial "--no-roms"
fi

if [ $ran -eq 0 ]; then
  echo "run_gates.sh: no gate matched '--only $only'" >&2
  exit 2
fi
echo "$ran gates ran, $failed failed"
[ $failed -eq 0 ]
