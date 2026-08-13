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
ROM="${ROM:-tests/roms/assorted/flowing_palette.nes}"

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

# v2.3.3 — snapshot the config we are ABOUT to run with, so the emitted
# header can be checked against it afterwards. See the assertion below for why
# this exists; read it here rather than earlier so a config edit made just
# before this call is what we compare against.
CFG="${XDG_CONFIG_HOME:-$HOME/.config}/rustynes/config.toml"
WANT_RUN_AHEAD=""
WANT_REWIND=""
if [[ -f "$CFG" ]]; then
    # `tomllib` is stdlib only from Python 3.11. Import it INSIDE the `try`:
    # at module level the ImportError escapes before the handler exists, python
    # exits non-zero, and `set -euo pipefail` then aborts the whole capture on a
    # host that is merely older — rather than degrading to "cannot read the
    # config", which the empty-value checks below already handle correctly. The
    # `|| true` covers python3 being absent entirely for the same reason.
    WANT_RUN_AHEAD="$(python3 - "$CFG" <<'PYCFG' || true
import sys, pathlib
try:
    import tomllib
    d = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
    print(d.get("input", {}).get("run_ahead", ""))
except Exception:
    print("")
PYCFG
)"
    WANT_REWIND="$(python3 - "$CFG" <<'PYCFG' || true
import sys, pathlib
try:
    import tomllib
    d = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
    v = d.get("rewind", {}).get("enabled", None)
    print("" if v is None else str(bool(v)).lower())
except Exception:
    print("")
PYCFG
)"
    if [[ -z "$WANT_RUN_AHEAD" && -z "$WANT_REWIND" ]]; then
        echo "perf_capture: warning: could not read $CFG (Python 3.11+ required for" >&2
        echo "perf_capture: tomllib). The capture will run, but its configuration" >&2
        echo "perf_capture: cannot be asserted." >&2
    fi
fi

echo "perf_capture: running ${DURATION}s capture on $ROM (perf logging on)…"
# Run the frontend with logging forced on; kill it after DURATION. The
# background+kill pattern (never `timeout` wrapping a GUI) is the project norm.
RUSTYNES_PERF_LOG=1 ./target/release/rustynes "$ROM" &
APP_PID=$!
# Clean up the background frontend even if the script is interrupted (Ctrl+C)
# or exits early before the explicit kill below.
trap 'kill "$APP_PID" 2>/dev/null || true' EXIT
sleep "$DURATION"
kill "$APP_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true
trap - EXIT

NEWEST="$(ls -1t perf-logs/perf-*.csv 2>/dev/null | head -1 || true)"
AFTER="$(ls -1 perf-logs/ 2>/dev/null | wc -l)"
if [[ -z "$NEWEST" || "$AFTER" -le "$BEFORE" ]]; then
    echo "perf_capture: no new perf-log CSV produced (did the window open?)" >&2
    exit 2
fi
echo "perf_capture: captured $NEWEST"

# v2.3.3 — ASSERT the capture describes the run we intended.
#
# This exists because it silently did not, repeatedly, and every conclusion
# drawn from those runs was wrong. The emulator rewrites its config file on
# exit, so a config edit made between one capture and the next races that
# write: several captures in the v2.3.3 investigation ran at `run_ahead = 1`
# while the caller believed they were comparing 0 against 2, and one
# "rewind on vs off" comparison ran with rewind ON for both halves. Nothing in
# the output looked wrong — the numbers were real, they just described a
# different configuration than the one being reasoned about.
#
# A capture that does not match its own config is worse than no capture, so
# this fails the run rather than warning. To capture a config deliberately
# different from the file on disk, set SKIP_CONFIG_ASSERT=1.
if [[ -z "${SKIP_CONFIG_ASSERT:-}" ]]; then
    # The trailing `|| true` is LOAD-BEARING, not defensive noise. `grep` exits
    # 1 when the header is absent; with `set -euo pipefail` that status
    # propagates out of the command substitution and kills the script right
    # here, with status 1 and no message — so the carefully-worded failure
    # below, and the documented exit status 3, were both unreachable in exactly
    # the case they exist for. Let the extraction yield an empty string and let
    # the empty-header check own the failure.
    GOT_RUN_AHEAD="$(grep -E '^# run_ahead = ' "$NEWEST" | head -1 | sed 's/.*= //' || true)"
    GOT_REWIND="$(grep -E '^# rewind_enabled = ' "$NEWEST" | head -1 | sed 's/.*= //' || true)"
    MISMATCH=""
    # FAIL CLOSED on a header we cannot read. An absent header means the
    # capture cannot be checked at all, which is exactly the state that let
    # mis-configured runs through in the first place — treating "no header" as
    # "no mismatch" would rebuild the hole this assertion exists to close.
    # (Same failure shape as the release-tag check that could not distinguish
    # "tag absent" from "lookup failed".)
    if [[ -z "$GOT_RUN_AHEAD" || -z "$GOT_REWIND" ]]; then
        echo "perf_capture: capture has no readable metadata header, so it CANNOT be" >&2
        echo "perf_capture: checked against the configuration it was meant to run." >&2
        echo "perf_capture: Refusing to report it as a valid measurement." >&2
        echo "perf_capture: (A too-short run can produce this: the header is written" >&2
        echo "perf_capture: once logging starts.)" >&2
        exit 3
    fi
    if [[ -n "$WANT_RUN_AHEAD" && -n "$GOT_RUN_AHEAD" && "$WANT_RUN_AHEAD" != "$GOT_RUN_AHEAD" ]]; then
        MISMATCH="run_ahead: config says $WANT_RUN_AHEAD, capture says $GOT_RUN_AHEAD"
    fi
    if [[ -n "$WANT_REWIND" && -n "$GOT_REWIND" && "$WANT_REWIND" != "$GOT_REWIND" ]]; then
        MISMATCH="${MISMATCH:+$MISMATCH; }rewind_enabled: config says $WANT_REWIND, capture says $GOT_REWIND"
    fi
    if [[ -n "$MISMATCH" ]]; then
        echo "perf_capture: CONFIG MISMATCH — $MISMATCH" >&2
        echo "perf_capture: the capture does not describe the configuration on disk." >&2
        echo "perf_capture: the emulator rewrites config.toml on exit, so an edit made" >&2
        echo "perf_capture: just before this run can be clobbered by the PREVIOUS run's" >&2
        echo "perf_capture: exit write. Re-apply the edit and re-run, or set" >&2
        echo "perf_capture: SKIP_CONFIG_ASSERT=1 if the difference is deliberate." >&2
        exit 3
    fi
    echo "perf_capture: config assertion OK (run_ahead=${GOT_RUN_AHEAD:-?} rewind=${GOT_REWIND:-?})"
fi

exec python3 "$ROOT/scripts/perf/perf_log_check.py" "$NEWEST" \
    ${MAX_UNDERRUNS:+--max-underruns "$MAX_UNDERRUNS"} \
    ${MAX_PRODUCED_MS:+--max-produced-ms "$MAX_PRODUCED_MS"} \
    ${MAX_CATCHUP_BURSTS:+--max-catchup-bursts "$MAX_CATCHUP_BURSTS"} \
    ${MAX_SNAP_FORWARDS:+--max-snap-forwards "$MAX_SNAP_FORWARDS"}
