#!/usr/bin/env bash
# perf_capture.sh — v1.5.0 "Lens" Workstream H7 scripted perf capture + gate.
#
# Drives the windowed `rustynes` frontend with perf logging auto-enabled
# (RUSTYNES_PERF_LOG=1) for a bounded run, then feeds the emitted CSV to
# perf_log_check.py so the frontend pacing/audio-sync health signals
# (produced_max / underruns / catchup_bursts / snap_forwards) become a
# tracked, repeatable regression check.
#
# REQUIRES A DISPLAY + an audio device: pacing/present/audio behavior only
# exists with the real winit present loop + cpal stream — there is no headless
# path (the same reason the v1.2.0 F1/F3 items are maintainer-manual). On a
# headless host (no DISPLAY/WAYLAND_DISPLAY) this skips cleanly with exit 0 so
# CI never hard-fails on a runner without a compositor; the maintainer runs it
# locally on real hardware (the high-value capture).
#
# By default it uses a committed CC0 ROM (flowing_palette.nes — rendering-heavy,
# exercises the produce/present/audio path) so it needs no commercial dump.
# Point it at a real SMB dump with ROM=/path/to/smb.nes for the canonical
# capture the workstream was tuned against.
#
# Usage:
#   scripts/perf/perf_capture.sh [SECONDS]
#   ROM=~/roms/smb.nes scripts/perf/perf_capture.sh 60
#
# Env knobs: ROM, SECONDS (default 30), and any MAX_* forwarded to the checker.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

DURATION="${1:-${SECONDS_OVERRIDE:-30}}"
ROM="${ROM:-tests/roms/sprint-2/flowing_palette.nes}"

if [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
    echo "perf_capture: no DISPLAY/WAYLAND_DISPLAY — skipping (needs a compositor)."
    echo "perf_capture: pacing/audio-sync can't be measured headless; run locally."
    exit 0
fi

if [[ ! -f "$ROM" ]]; then
    echo "perf_capture: ROM not found: $ROM" >&2
    exit 2
fi

echo "perf_capture: building release frontend…"
cargo build --release -p rustynes-frontend >/dev/null

# Fresh perf-logs dir so we can pick the new file deterministically.
mkdir -p perf-logs
BEFORE="$(ls -1 perf-logs/ 2>/dev/null | wc -l)"

echo "perf_capture: running ${DURATION}s capture on $ROM (perf logging on)…"
# Run the frontend with logging forced on; kill it after DURATION. The
# background+kill pattern (never `timeout` wrapping a GUI) is the project norm.
RUSTYNES_PERF_LOG=1 ./target/release/rustynes "$ROM" &
APP_PID=$!
sleep "$DURATION"
kill "$APP_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true

NEWEST="$(ls -1t perf-logs/perf-*.csv 2>/dev/null | head -1 || true)"
AFTER="$(ls -1 perf-logs/ 2>/dev/null | wc -l)"
if [[ -z "$NEWEST" || "$AFTER" -le "$BEFORE" ]]; then
    echo "perf_capture: no new perf-log CSV produced (did the window open?)" >&2
    exit 2
fi
echo "perf_capture: captured $NEWEST"

exec python3 "$ROOT/scripts/perf/perf_log_check.py" "$NEWEST" \
    ${MAX_UNDERRUNS:+--max-underruns "$MAX_UNDERRUNS"} \
    ${MAX_PRODUCED_MS:+--max-produced-ms "$MAX_PRODUCED_MS"} \
    ${MAX_CATCHUP_BURSTS:+--max-catchup-bursts "$MAX_CATCHUP_BURSTS"} \
    ${MAX_SNAP_FORWARDS:+--max-snap-forwards "$MAX_SNAP_FORWARDS"}
