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
#       `run` the lines `LABEL BASE [args...]` of FILE in order, until the next
#       one is missing or starts with STOP; `#` lines and blank lines are
#       skipped. FILE is APPEND-ONLY: it is never rewritten, so lines may be
#       appended while it runs (`echo ... >> FILE`) and none is ever lost.
#       Progress is a count of entries taken, in FILE.pos; each finished entry
#       is appended to FILE.done with its exit status, and the queue exits 1 if
#       any entry failed. One process per queue (FILE.lock); a second refuses.
#       To re-run a queue from the top, delete FILE.pos.
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
    # Every sidecar (.lock, .pos, .inprogress, .done) is named from the
    # CANONICAL path: through a symlink alias the queue would otherwise get a
    # second lock and a second cursor, and two processors would run it
    # (CodeRabbit on #604).
    q=$(realpath -- "$q") || { echo "quiet_ab.sh: cannot resolve $2" >&2; exit 2; }
    # ONE processor per queue, for the whole run. The benchmark lock orders
    # measurements against heavy jobs; it never protected choosing an entry
    # or recording a result, so two queue processes could take the same one
    # (CodeRabbit on #604).
    exec 8>> "$q.lock"
    flock -n 8 || { echo "quiet_ab.sh: $q is already being processed (holder of $q.lock)" >&2; exit 2; }
    if [ -e "$q.inprogress" ]; then
      # The marker's first line is the count BEFORE the interrupted entry was
      # taken. Comparing it with FILE.pos says which of the two states the
      # interruption left, so the recovery step printed is the right one: the
      # same marker text used to mean both, and re-queuing in the "still
      # pending" state ran the entry twice (CodeRabbit on #604).
      mpos=$(sed -n 1p "$q.inprogress"); entry=$(sed -n 2p "$q.inprogress")
      cur=$(cat "$q.pos" 2>/dev/null || echo 0)
      case "$mpos" in ''|*[!0-9]*) mpos="?"; entry=$(cat "$q.inprogress") ;; esac
      echo "quiet_ab.sh: $q.inprogress names a run that was interrupted: $entry" >&2
      if [ "$mpos" = "?" ]; then
        echo "quiet_ab.sh: the marker has no count (written before v3.2.0's format); compare $q.pos ($cur) with the queue by hand" >&2
      elif [ "$cur" = "$mpos" ]; then
        echo "quiet_ab.sh: it is STILL PENDING ($q.pos is $cur, not advanced): delete the marker and rerun; do not re-queue it" >&2
      elif [ "$cur" = "$((mpos + 1))" ]; then
        echo "quiet_ab.sh: it was TAKEN ($q.pos moved $mpos -> $cur) and did not finish: re-queue it if it is still wanted, then delete the marker" >&2
      else
        echo "quiet_ab.sh: $q.pos is $cur and the marker recorded $mpos; inspect both by hand before deleting the marker" >&2
      fi
      exit 2
    fi
    trap 'rm -f "$measuring"' EXIT
    failed=0
    while :; do
      # FILE is never rewritten. The first version removed each entry by
      # rewriting the file, which lost a line appended between the read and
      # the write, and its fixed `FILE.tmp` name could be a planted symlink
      # (CodeRabbit on #604). Progress is a count instead, and an append can
      # only ever add entries.
      pos=$(cat "$q.pos" 2>/dev/null || echo 0)
      case "$pos" in ''|*[!0-9]*) echo "quiet_ab.sh: $q.pos is not a count: '$pos'" >&2; exit 2 ;; esac
      line=$(grep -v '^[[:space:]]*$' "$q" | grep -v '^[[:space:]]*#' | sed -n "$((pos + 1))p")
      if [ -z "$line" ] || [[ "$line" == STOP* ]]; then echo "queue empty"; break; fi
      # The marker is written, and checked, BEFORE the count moves: interrupted
      # in between, the entry is still pending AND named by the marker, and the
      # refusal above stops a silent re-run (CodeRabbit on #605).
      # Created with noclobber (O_EXCL), so a symlink planted at the marker's
      # name between entries is refused rather than followed (CodeRabbit on
      # #604); the startup check sees only a link whose target exists.
      if ! (set -C; printf '%s\n%s\n' "$pos" "$line" > "$q.inprogress"); then
        echo "quiet_ab.sh: cannot create $q.inprogress (it exists, or is a link)" >&2
        exit 2
      fi
      # A unique file made by mktemp (O_EXCL, so never an existing name or a
      # symlink), renamed over the count.
      tmp=""
      if ! { tmp=$(mktemp "$q.pos.XXXXXX") && echo "$((pos + 1))" > "$tmp" && mv -f "$tmp" "$q.pos"; }; then
        echo "quiet_ab.sh: could not advance $q.pos; '$line' stays pending" >&2
        rm -f "$tmp" "$q.inprogress"
        exit 2
      fi
      # `line` holds LABEL BASE and flags, which must word-split.
      # shellcheck disable=SC2086
      run_one $line
      rc=$?
      [ "$rc" -eq 0 ] || failed=1
      # The marker goes only once the result is recorded: if the append
      # fails, the marker is the one remaining record of the run.
      if echo "$line rc=$rc" >> "$q.done"; then
        rm -f "$q.inprogress"
      else
        echo "quiet_ab.sh: could not record '$line rc=$rc' in $q.done; $q.inprogress kept" >&2
        exit 2
      fi
    done
    # A queue that drained after a failed benchmark used to exit 0
    # (Codex, then CodeRabbit on #604); FILE.done says which one failed.
    [ "$failed" -eq 0 ] || { echo "quiet_ab.sh: at least one queued run failed; see $q.done" >&2; exit 1; } ;;
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
