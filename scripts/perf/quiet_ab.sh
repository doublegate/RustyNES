#!/usr/bin/env bash
# quiet_ab.sh — run scripts/perf/ab_check.sh only on a quiet host, and keep
# heavy jobs from starting underneath a measurement.
#
# `ab_check.sh`'s adoption rule needs two independent runs on a QUIET host, and
# it says so, but it does not wait for one. On a desktop that is also building,
# fuzzing or simulating, a run started by hand lands on a busy machine more
# often than not, and a busy-host result is noise that reads as a measurement.
# The v2.9.9 and v3.0.1 campaigns ran their A/Bs through a small queue that
# waits for the load to fall and holds two lock files; this is that queue,
# promoted from session scratch at v3.2.0.
#
# Subcommands:
#
#   quiet_ab.sh run LABEL BASE [ab_check args...]
#       Wait until no BUSY lock is held and the one-minute load average is below
#       QUIET_LOAD (default 1.5), hold the MEASURING lock, and run
#       `scripts/perf/ab_check.sh --base BASE [args]` in this checkout. The log,
#       headed by the load at the start and the end, goes to LOG_DIR/LABEL.log.
#
#   quiet_ab.sh queue FILE
#       Pop lines `LABEL BASE [args...]` from FILE one at a time and `run` each,
#       until FILE is empty or its first line is STOP. Lines may be appended
#       while it runs; `#` lines and blank lines are skipped. Each finished line
#       is appended to FILE.done.
#
#   quiet_ab.sh heavy CMD [args...]
#       Wait until no measurement holds MEASURING, then hold BUSY while CMD runs,
#       so no queued A/B starts under it. Wrap ladders, sweeps and test suites
#       in this while a queue is running.
#
# State (the two locks and the default logs) lives in PERF_STATE_DIR, default
# ${TMPDIR:-/tmp}/rustynes-perf. Both commands must see the same directory.
#
# What it does not do: make a run trustworthy. A quiet start is necessary, not
# sufficient; read ab_check.sh's order-bias control and its adoption rule
# before the candidate column, and take the second independent run anyway.
set -uo pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null) || { echo "quiet_ab.sh: not inside a git checkout" >&2; exit 2; }
state="${PERF_STATE_DIR:-${TMPDIR:-/tmp}/rustynes-perf}"
log_dir="${LOG_DIR:-$state/logs}"
quiet="${QUIET_LOAD:-1.5}"
busy="$state/BUSY"
measuring="$state/MEASURING"
lock="$state/lock"
mkdir -p "$state" "$log_dir"

load1() { cut -d' ' -f1 /proc/loadavg; }

# One flock decides who may run: a measurement holds it EXCLUSIVE, every heavy
# job holds it SHARED, so heavy jobs run together but never under a
# measurement, and a measurement waits for all of them. BUSY and MEASURING are
# informational markers only. They used to BE the lock, checked and then
# touched (a window between the two), and two heavy jobs shared one BUSY file,
# so the first to finish removed it while the second still ran (CodeRabbit on
# #605).
run_one() {
  local label="$1" base="$2"; shift 2
  local la
  exec 9> "$lock"
  while :; do
    flock -x 9
    la=$(load1)
    if awk -v l="$la" -v q="$quiet" 'BEGIN { exit !(l < q) }'; then break; fi
    flock -u 9
    sleep 60
  done
  echo "$label" > "$measuring"
  local rc
  {
    echo "LABEL $label  BASE $base  ARGS $*  TREE $root @ $(git -C "$root" rev-parse --short HEAD)"
    echo "START $(date -Is)  LOAD1 $la  ($(cat /proc/loadavg))"
    ( cd "$root" && scripts/perf/ab_check.sh --base "$base" "$@" )
    rc=$?
    echo "END $(date -Is)  rc=$rc  ($(cat /proc/loadavg))"
  } > "$log_dir/$label.log" 2>&1
  rm -f "$measuring"
  exec 9>&-
  echo "$label done (start load $la, rc=$rc), log $log_dir/$label.log"
  grep -E "^LABEL|^START|^END|change:|^==>" "$log_dir/$label.log"
  # The function's status is ab_check.sh's, not the grep's (which always
  # matches the LABEL line and so reported every run as a success).
  return "$rc"
}

case "${1:-}" in
  run)
    shift
    [ $# -ge 2 ] || { echo "usage: quiet_ab.sh run LABEL BASE [ab_check args...]" >&2; exit 2; }
    trap 'rm -f "$measuring"' EXIT
    run_one "$@"
    exit $? ;;
  queue)
    q="${2:-}"
    [ -f "$q" ] || { echo "usage: quiet_ab.sh queue FILE (FILE must exist)" >&2; exit 2; }
    if [ -e "$q.inprogress" ]; then
      echo "quiet_ab.sh: $q.inprogress names a run that was interrupted: $(cat "$q.inprogress")" >&2
      echo "quiet_ab.sh: re-queue it if it is still wanted, then delete the marker" >&2
      exit 2
    fi
    trap 'rm -f "$measuring"' EXIT
    while :; do
      line=$(grep -v '^[[:space:]]*$' "$q" | grep -v '^#' | head -1)
      if [ -z "$line" ] || [[ "$line" == STOP* ]]; then echo "queue empty"; break; fi
      # Remove exactly that first occurrence, so lines appended meanwhile stay.
      python3 - "$q" "$line" <<'PY'
import sys
path, line = sys.argv[1], sys.argv[2]
lines = open(path).read().splitlines()
lines.remove(line)
open(path, "w").write("\n".join(lines) + ("\n" if lines else ""))
PY
      # An interrupted run is a void measurement: the marker says which one,
      # so it is re-queued deliberately rather than silently lost.
      echo "$line" > "$q.inprogress"
      # `line` holds LABEL BASE and flags, which must word-split.
      # shellcheck disable=SC2086
      run_one $line
      echo "$line rc=$?" >> "$q.done"
      rm -f "$q.inprogress"
    done ;;
  heavy)
    shift
    [ $# -ge 1 ] || { echo "usage: quiet_ab.sh heavy CMD [args...]" >&2; exit 2; }
    exec 9> "$lock"
    flock -s 9
    touch "$busy"
    "$@"
    rc=$?
    # Clear the marker only when no other heavy job still holds the lock.
    flock -u 9
    if flock -n -x 9; then rm -f "$busy"; flock -u 9; fi
    exit "$rc" ;;
  *)
    sed -n '/^set -uo/q;2,$p' "$0"
    exit 2 ;;
esac
