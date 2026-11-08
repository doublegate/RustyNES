#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# cpu_target_ab.sh -- time the three CPU-target builds of the `full` binary
# against each other: `cargo full-baseline` (target-cpu=x86-64, what releases
# ship), `cargo full-v3` (x86-64-v3) and `cargo full-native` (this host).
#
# Why a separate tool. `ab_check.sh` compares two REVISIONS (or feature sets)
# at ONE CPU configuration: it runs `cargo bench`, which uses neither the
# `release-*` profiles nor the aliases' `--config` rustflags, so pointing it at
# the aliases benches the same configuration on both sides. A CPU-target
# comparison has to time the built binaries themselves, which is what this
# does. The evidence rule is `ab_check.sh`'s, unchanged:
#
#   - a SECOND independent run is required (a single run has produced a
#     p = 0.00 result on four workloads that was pure artifact);
#   - an A/B/A order control: every round runs the binaries in a balanced
#     order (B V N N V B), so a drift that favours whichever runs first or
#     last cancels instead of masquerading as a difference, and the baseline's
#     own first-vs-last drift is printed as the noise floor;
#   - a mixed-sign result across workloads is a REJECTION, not an average.
#
# Workload. `rustynes speedrun --rom <ROM> --splits <file> --frames N` is the
# headless frame loop the frontend already ships: `Nes::run_frame` plus an
# audio drain, no window, no video, no audio device. The splits file's start
# condition can never hold (`$0000 = 0 AND $0000 = 1`), so the timer never
# starts, every frame runs, and the binary exits 5 ("unfinished"); any other
# exit status is treated as a failed measurement, not timed. Each invocation
# is timed whole (process start to exit), so ROM load and process start are
# inside the number: keep FRAMES large enough that they are noise (the default
# 3,600 frames is a minute of NTSC emulation per invocation).
#
# The ROMs are the committed public-domain ones the criterion benches use
# (nestest, flowing_palette) plus AccuracyCoin, which exercises far more of the
# CPU and PPU than either.
#
# Usage:
#   scripts/perf/cpu_target_ab.sh                 # build all three, 2 runs
#   scripts/perf/cpu_target_ab.sh --no-build      # use the binaries in target/
#   CPU_AB_FRAMES=7200 CPU_AB_ROUNDS=7 scripts/perf/cpu_target_ab.sh
#
# Before trusting a number, read docs/performance.md's paragraph on the
# aliases' precedence traps (canonical text: `.cargo/config.toml`): an
# exported RUSTFLAGS / CARGO_ENCODED_RUSTFLAGS, or a `target-cpu` in a user
# `[target.'cfg(...)']` table, silently turns all three builds into the same
# build. The script refuses to run when either variable is set, and with
# `--build` it checks each binary's final rustc line for the CPU it asked for.
#
# Record the outcome in docs/performance.md either way, rejections included.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."
repo_root="$(pwd)"

FRAMES="${CPU_AB_FRAMES:-3600}"
ROUNDS="${CPU_AB_ROUNDS:-5}"
RUNS="${CPU_AB_RUNS:-2}"
BUILD=1
for arg in "$@"; do
    case "$arg" in
        --no-build) BUILD=0 ;;
        -h | --help)
            sed -n '2,52p' "$0"
            exit 0
            ;;
        *)
            echo "cpu_target_ab: unknown argument $arg" >&2
            exit 2
            ;;
    esac
done

if [[ -n "${RUSTFLAGS+x}" || -n "${CARGO_ENCODED_RUSTFLAGS+x}" ]]; then
    echo "cpu_target_ab: RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS is set (even empty," >&2
    echo "it replaces the aliases' target-cpu, making all three builds the same)." >&2
    echo "Unset both and re-run." >&2
    exit 2
fi
if [[ "$(uname -m)" != "x86_64" ]]; then
    echo "cpu_target_ab: full-baseline and full-v3 target x86-64 only." >&2
    exit 2
fi

declare -A BIN=(
    [baseline]="$repo_root/target/release-baseline/rustynes"
    [v3]="$repo_root/target/release-v3/rustynes"
    [native]="$repo_root/target/release-native/rustynes"
)
declare -A ALIAS=([baseline]=full-baseline [v3]=full-v3 [native]=full-native)
declare -A CPU=([baseline]=x86-64 [v3]=x86-64-v3 [native]=native)

if ((BUILD)); then
    for k in baseline v3 native; do
        echo "== building cargo ${ALIAS[$k]}"
        # -v so the final rustc line can be checked for the CPU it was asked
        # for: an exit status says nothing about which target-cpu won.
        log="$(mktemp)"
        cargo "${ALIAS[$k]}" -v 2>"$log" >/dev/null || {
            tail -20 "$log" >&2
            exit 1
        }
        last="$(grep -E 'Running `.*rustc .*--crate-name rustynes ' "$log" | tail -1 |
            grep -oE 'target-cpu=[A-Za-z0-9_-]+' | tail -1 || true)"
        rm -f "$log"
        # A fresh build prints the rustynes rustc line; an up-to-date one does
        # not, and then there is nothing to check (say so rather than pass).
        if [[ -z "$last" ]]; then
            echo "   (up to date: no rustc line to check; touch the frontend to re-verify)"
        elif [[ "$last" != "target-cpu=${CPU[$k]}" ]]; then
            echo "cpu_target_ab: ${ALIAS[$k]} compiled with $last, not target-cpu=${CPU[$k]}" >&2
            exit 1
        else
            echo "   final rustc line: $last"
        fi
    done
fi
for k in baseline v3 native; do
    [[ -x "${BIN[$k]}" ]] || {
        echo "cpu_target_ab: ${BIN[$k]} missing; run without --no-build" >&2
        exit 1
    }
done

ROMS=(
    tests/roms/nestest/nestest.nes
    tests/roms/assorted/flowing_palette.nes
    tests/roms/accuracycoin/AccuracyCoin.nes
)
for r in "${ROMS[@]}"; do [[ -f "$r" ]] || {
    echo "cpu_target_ab: missing ROM $r" >&2
    exit 1
}; done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
splits="$work/never.splits"
printf 'game = cpu_target_ab\ncategory = none\nstart = 0xH0000=0_0xH0000=1\nsplit = End | 0xH0000=0\n' >"$splits"

pin=()
command -v taskset >/dev/null && pin=(taskset -c 2)

# time_one <key> <rom> -> elapsed nanoseconds on stdout
time_one() {
    local t0 t1 rc
    t0="$(date +%s%N)"
    set +e
    "${pin[@]}" "${BIN[$1]}" speedrun --rom "$2" --splits "$splits" --frames "$FRAMES" >/dev/null 2>&1
    rc=$?
    set -e
    t1="$(date +%s%N)"
    if ((rc != 5)); then
        echo "cpu_target_ab: $1 on $2 exited $rc, expected 5 (unfinished); not timed" >&2
        exit 1
    fi
    echo $((t1 - t0))
}

echo "frames/invocation=$FRAMES rounds=$ROUNDS runs=$RUNS pin=${pin[*]:-none}"
echo "host: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ //'), load $(cut -d' ' -f1-3 /proc/loadavg)"

for run in $(seq 1 "$RUNS"); do
    echo
    echo "== run $run of $RUNS"
    printf '%-40s %12s %12s %12s %9s %9s %9s\n' workload baseline_ms v3_ms native_ms v3_% native_% drift_%
    for rom in "${ROMS[@]}"; do
        declare -A sum=([baseline]=0 [v3]=0 [native]=0)
        first_b=0
        last_b=0
        for _ in $(seq 1 "$ROUNDS"); do
            # Balanced order: B V N N V B. Each binary runs twice per round,
            # once in each half, so a monotonic host drift adds the same to
            # all three.
            for k in baseline v3 native native v3 baseline; do
                ns="$(time_one "$k" "$rom")"
                sum[$k]=$((sum[$k] + ns))
                if [[ "$k" == baseline ]]; then
                    ((first_b == 0)) && first_b=$ns
                    last_b=$ns
                fi
            done
        done
        n=$((ROUNDS * 2))
        b=$((sum[baseline] / n))
        v=$((sum[v3] / n))
        nn=$((sum[native] / n))
        # Negative percent = faster than the baseline.
        awk -v w="$(basename "$rom")" -v b="$b" -v v="$v" -v n="$nn" -v f="$first_b" -v l="$last_b" 'BEGIN {
            printf "%-40s %12.1f %12.1f %12.1f %+9.2f %+9.2f %+9.2f\n",
                w, b/1e6, v/1e6, n/1e6, (v-b)*100/b, (n-b)*100/b, (l-f)*100/f }'
        unset sum
    done
done

cat <<'EOF'

Reading it (ab_check.sh's rule): a column is a gain only if it has the same
sign on EVERY workload in BOTH runs, and its size clearly exceeds drift_%
(the baseline's own first-to-last change, the order-bias floor). Mixed signs
across workloads reject. Record the table in docs/performance.md either way.
EOF
