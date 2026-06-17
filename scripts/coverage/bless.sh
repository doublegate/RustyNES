#!/usr/bin/env bash
#
# The ONE safe way to (re-)bless the data-driven coverage baselines.
#
# WHY THIS EXISTS — postmortem (2026-06-17): the screenshot coverage bless is a
# ~30-minute full sweep of every staged ROM. It was run several ways at once —
# directly, `nohup`-detached, and by a sub-agent that ALSO armed `until`-loop
# "waiters" to relaunch it. Those detached jobs:
#   1. raced each other on the single Cargo target lock (only one `cargo test`
#      compiles/links at a time), so runs blocked / stalled / produced partial
#      `.snap.new` + PNG state; and
#   2. survived `TaskStop` (nohup) and, when the sub-agent exited, were reparented
#      to the top-level Claude process and kept relaunching — a self-sustaining
#      runaway that could not be cleanly stopped.
#
# This script removes both hazards:
#   * **flock** — a second invocation REFUSES (it never races a running bless).
#   * **foreground + tracked** — run it via the harness's `run_in_background`
#     (so it is `TaskStop`-able) — NEVER `nohup` it and NEVER wrap it in a
#     self-relaunching `until` loop. One bless at a time, full stop.
#
# Usage:
#   scripts/coverage/bless.sh [log-path]          # default log: /tmp/RustyNES/bless.log
# Then, after reviewing the dumped PNGs:
#   cargo insta accept
#   python3 scripts/coverage/coverage.py categorize
#
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO"
LOG="${1:-/tmp/RustyNES/bless.log}"
LOCK="$REPO/target/.coverage-bless.lock"
DUMP="/tmp/rustynes-baseline-screenshots/external"
SNAP_DIR="crates/rustynes-test-harness/tests/snapshots"
mkdir -p "$(dirname "$LOG")" "$REPO/target"

# --- single-instance guard: refuse to start a second concurrent bless --------
exec 9>"$LOCK"
if ! flock -n 9; then
  echo "ERROR: a coverage bless already holds $LOCK." >&2
  echo "       Concurrent blesses race the Cargo target lock and corrupt the" >&2
  echo "       .snap.new / PNG state. Wait for it (tail $LOG) or stop that task;" >&2
  echo "       do NOT launch a second one." >&2
  exit 2
fi

echo "[bless] cleaning stale artifacts (.snap.new + dumped PNGs) ..."
find "$SNAP_DIR" -name '*.snap.new' -delete 2>/dev/null || true
rm -f "$DUMP"/*.png 2>/dev/null || true

echo "[bless] full external_coverage sweep (single-threaded). log: $LOG"
# INSTA_UPDATE=auto writes a .snap.new per new/changed baseline; insta still
# reports the run as "failed" (non-zero) by design — that is NOT an error here,
# so the cargo exit is intentionally ignored.
INSTA_UPDATE=auto RUSTYNES_DUMP_FRAMES=1 \
  cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
  --test external_coverage -- --test-threads=1 --nocapture > "$LOG" 2>&1 || true

n_snap=$(find "$SNAP_DIR" -name '*.snap.new' 2>/dev/null | wc -l | tr -d ' ')
n_png=$(find "$DUMP" -name '*.png' 2>/dev/null | wc -l | tr -d ' ')
echo "BLESS_DONE snap_new=${n_snap} png=${n_png}"
echo "[bless] next: review PNGs in $DUMP, then 'cargo insta accept' + 'python3 scripts/coverage/coverage.py categorize'"
# flock auto-released when fd 9 closes on exit.
