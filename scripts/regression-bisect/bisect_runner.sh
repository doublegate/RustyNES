#!/usr/bin/env bash
# `git bisect run` wrapper for the RustyNES v2 regression harnesses.
#
# Exits:
#   0   GOOD  — build OK, test PASS at this commit (bisect: --good)
#   1   BAD   — build OK, test FAIL                (bisect: --bad)
#   125 SKIP  — build BROKE (or env unreliable)    (bisect: --skip)
#
# Origin: the FSM-recovery bisect of `/tmp/rustynes-bisect/.bisect_run.sh`
# (commit 834be9e fix, May 17 2026). That copy lived in `/tmp/` and was
# wiped on every CachyOS reboot; this version is committed so the
# next regression doesn't pay the rediscovery cost.
#
# Usage (defaults — bisect the FSM-recovery oracle):
#
#   git bisect start
#   git bisect bad   <bad-commit>
#   git bisect good  <good-commit>
#   git bisect run /home/parobek/Code/RustyNES_v2/scripts/regression-bisect/bisect_runner.sh
#
# Usage (parametric — bisect a different harness):
#
#   HARNESS_TEST=audio_tests \
#   HARNESS_FEATURE=test-roms \
#   git bisect run scripts/regression-bisect/bisect_runner.sh
#
# Environment variables (all optional; defaults shown):
#
#   HARNESS_TEST       (external_real_games) Test binary name. Maps to
#                       `cargo test --test <HARNESS_TEST>`. Set to
#                       `audio_tests`, `m22`, `mmc1_a12`,
#                       `visual_regression`, etc. for the corresponding
#                       harness file under `crates/nes-test-harness/tests/`.
#   HARNESS_FEATURE    (commercial-roms) Cargo feature to enable. Pass
#                       `test-roms` for the committed test-ROM corpora
#                       (audio_tests / m22 / mmc1_a12 / blargg_cpu
#                       / visual_regression / mmc5 / holy_mapperel).
#   HARNESS_PACKAGE    (nes-test-harness) `-p` target. Unlikely to need
#                       changing unless the harness moves crates.
#   HARNESS_FILTER     ("") Optional test-name substring passed after
#                       the `--` separator (e.g. `super_mario_bros`).
#   HARNESS_RELEASE    ("0") Set to "1" to build with `--release`.
#                       Useful when the regression is performance-only
#                       or the dev profile is too slow.
#   BUILD_ONLY         ("0") Set to "1" to skip the test run and exit
#                       0 if `cargo build --tests` succeeds. Useful for
#                       sanity-checking the build graph during a
#                       regression that compiles but doesn't link.
#
# Shell-redirection gotcha (the parent FSM-recovery bisect bit on this):
#   `2>&1 > file` is WRONG  — redirects stderr to the old stdout, then
#                              stdout to file. stderr ends up at terminal.
#   `> file 2>&1` is RIGHT  — stdout → file, stderr → same fd as stdout.
# The harness uses the right form; users redirecting bisect logs should
# follow suit.

set -u

cd "$(git rev-parse --show-toplevel)" || exit 125

HARNESS_TEST="${HARNESS_TEST:-external_real_games}"
HARNESS_FEATURE="${HARNESS_FEATURE:-commercial-roms}"
HARNESS_PACKAGE="${HARNESS_PACKAGE:-nes-test-harness}"
HARNESS_FILTER="${HARNESS_FILTER:-}"
HARNESS_RELEASE="${HARNESS_RELEASE:-0}"
BUILD_ONLY="${BUILD_ONLY:-0}"

RELEASE_FLAG=()
if [ "$HARNESS_RELEASE" = "1" ]; then
    RELEASE_FLAG=(--release)
fi

CARGO_FLAGS=(
    -p "$HARNESS_PACKAGE"
    --features "$HARNESS_FEATURE"
    --test "$HARNESS_TEST"
)

# Phase 1: build. A build break should be SKIP, not BAD — the regression
# we're tracking is a runtime test failure, not a compile error at this
# commit. (Build errors on intermediate commits are common during
# bisect — they don't tell us anything about whether the test would
# have passed if the code compiled.)
cargo build --tests "${RELEASE_FLAG[@]}" "${CARGO_FLAGS[@]}" 1>&2
build_rc=$?
if [ "$build_rc" -ne 0 ]; then
    echo "[bisect_runner] cargo build failed (rc=$build_rc) — SKIP" 1>&2
    exit 125
fi

if [ "$BUILD_ONLY" = "1" ]; then
    echo "[bisect_runner] BUILD_ONLY=1 — build succeeded, exit GOOD" 1>&2
    exit 0
fi

# Phase 2: run the harness. Any non-zero exit means the test failed at
# this commit, which is BAD for the bisect (the regression is present
# here).
TEST_ARGS=()
if [ -n "$HARNESS_FILTER" ]; then
    TEST_ARGS+=("$HARNESS_FILTER")
fi
TEST_ARGS+=(--test-threads=1)

cargo test "${RELEASE_FLAG[@]}" "${CARGO_FLAGS[@]}" -- "${TEST_ARGS[@]}" 1>&2
test_rc=$?
if [ "$test_rc" -eq 0 ]; then
    echo "[bisect_runner] cargo test PASS — GOOD" 1>&2
    exit 0
else
    echo "[bisect_runner] cargo test FAIL (rc=$test_rc) — BAD" 1>&2
    exit 1
fi
