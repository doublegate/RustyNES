#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# T-SPEEDRUN-MEMIF, second half of the plan gate: "a reference splitter reads
# the documented interface on two builds without changes".
#
# 1. The harness writes the fixture (a synthetic ROM, a splits file, a movie,
#    and the cycle counts the run must produce) -- the same run the first half
#    of the gate (crates/rustynes-test-harness/tests/speedrun_timer.rs) replays.
# 2. `rustynes` is built twice, differently: a debug build with only the
#    `memif` feature, and a release build with `full` (every native feature,
#    so a different binary layout, different struct sizes, different code).
# 3. Each build runs `rustynes speedrun --memif` on the fixture, and the SAME
#    reference reader (scripts/memif/reference_splitter.py, written from
#    docs/memory-export-interface.md alone, unchanged between runs) follows
#    the published block until the run finishes.
# 4. Both readings must equal each other and the fixture's expected cycles,
#    and each must equal what its build reported itself.
#
# Exit 0 on PASS. Needs python3 (3.8+) and a POSIX shell host.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/rustynes-memif-gate.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
cd "$ROOT"

echo "== fixture -> $WORK"
RUSTYNES_SPEEDRUN_FIXTURE_DIR="$WORK" \
  cargo test -q -p rustynes-test-harness --test speedrun_timer -- --ignored --exact write_fixture >/dev/null
cat "$WORK/expected.txt"
expected_elapsed="$(sed -n 's/^elapsed=//p' "$WORK/expected.txt")"
expected_splits="$(sed -n 's/^splits=//p' "$WORK/expected.txt")"

echo "== build A: debug, --features memif"
cargo build -q -p rustynes-frontend --bin rustynes --features memif
cp target/debug/rustynes "$WORK/rustynes-a"
echo "== build B: release, --features full"
cargo build -q --release -p rustynes-frontend --bin rustynes --features full
cp target/release/rustynes "$WORK/rustynes-b"
if cmp -s "$WORK/rustynes-a" "$WORK/rustynes-b"; then
  echo "FAIL: the two builds are the same binary" >&2
  exit 1
fi

run_one() { # <label> <binary>
  local label="$1" bin="$2" name="rnesgate$$${1}"
  "$bin" speedrun --rom "$WORK/synthetic.nes" --splits "$WORK/buttons.rnsplits" \
    --movie "$WORK/buttons.rnm" --json "$WORK/$label-self.json" \
    --memif --memif-name "$name" --memif-hold-ms 4000 >"$WORK/$label-stdout.txt" &
  local pid=$!
  python3 "$ROOT/scripts/memif/reference_splitter.py" --name "$name" --timeout 30 \
    >"$WORK/$label-reader.json"
  wait "$pid" || { echo "FAIL: build $label exited $?" >&2; exit 1; }
}
run_one a "$WORK/rustynes-a"
run_one b "$WORK/rustynes-b"

python3 - "$WORK" "$expected_elapsed" "$expected_splits" <<'PY'
import json, sys
work, elapsed, splits = sys.argv[1], int(sys.argv[2]), [int(x) for x in sys.argv[3].split(",")]
keys = ["timer_state", "elapsed_cycles", "split_elapsed_cycles", "splits_done",
        "splits_defined", "frame", "master_clock", "region", "rom_sha256", "epoch",
        "ram_0000_000f", "master_clock_hz"]
reads = {}
for label in "ab":
    r = json.load(open(f"{work}/{label}-reader.json"))
    me = json.load(open(f"{work}/{label}-self.json"))
    reads[label] = {k: r[k] for k in keys}
    print(f"build {label}: reader saw state={r['timer_state']} elapsed={r['elapsed_cycles']} "
          f"splits={r['split_elapsed_cycles']} frame={r['frame']} publishes={r['publishes']}")
    assert r["timer_state"] == "finished", r
    assert r["elapsed_cycles"] == elapsed, (label, r["elapsed_cycles"], elapsed)
    assert r["split_elapsed_cycles"] == splits, (label, r["split_elapsed_cycles"], splits)
    assert me["elapsed_cycles"] == r["elapsed_cycles"], (label, "self-report differs")
    assert [s["elapsed_cycles"] for s in me["splits"]] == r["split_elapsed_cycles"]
assert reads["a"] == reads["b"], (reads["a"], reads["b"])
print(f"PASS: one reader, two builds, {len(keys)} fields identical; "
      f"elapsed {elapsed} master clocks, splits {splits}")
PY
