#!/usr/bin/env bash
# Create a temporary `/tmp/rustynes-bisect-$$/` worktree at a target
# commit, optionally overlay a harness file as untracked (so the bisect
# can run a NEW harness against OLD commits that pre-date its
# existence), and `exec` a sub-shell rooted in the worktree.
#
# Use case: the FSM-recovery bisect needed to run
# `external_real_games.rs` against commits that pre-dated its landing
# at 3e53802. The overlay pattern is:
#
#   1. `git worktree add /tmp/rustynes-bisect-$$ <commit>`
#   2. `cp <new-harness>.rs <worktree>/crates/.../tests/<new-harness>.rs`
#   3. patch Cargo.toml to add the feature flag the new harness needs
#   4. symlink `tests/roms/external/` into the worktree if it exists
#   5. `cd <worktree> && cargo test ...`
#
# This script automates steps 1-4 and `exec`s a sub-shell at step 5.
# When the sub-shell exits, the worktree is automatically removed.
#
# Usage:
#
#   ./worktree_overlay.sh <commit> \
#       --harness crates/rustynes-test-harness/tests/audio_tests.rs \
#       --feature 'test-roms = []' \
#       --shell    # optional; default `bash`
#
# Or without overlay (just a clean worktree at a commit):
#
#   ./worktree_overlay.sh <commit>
#
# Then in the sub-shell:
#
#   cargo test -p rustynes-test-harness --features test-roms --test audio_tests
#
# Shell-redirection gotcha (see bisect_runner.sh): `> file 2>&1`, NOT
# `2>&1 > file`.

set -eu

if [ "$#" -lt 1 ]; then
    cat <<'USAGE' 1>&2
Usage: worktree_overlay.sh <commit> [--harness <path>]... [--feature <line>]...

Required:
  <commit>             commit-ish to check out in the temp worktree

Optional, repeatable:
  --harness <path>     path (relative to repo root) of a harness file to
                       overlay as untracked. Example:
                       --harness crates/rustynes-test-harness/tests/audio_tests.rs
                       (the file must exist in the CURRENT HEAD)
  --feature <line>     Cargo.toml line to insert into
                       `crates/rustynes-test-harness/Cargo.toml`'s
                       `[features]` table. Example:
                       --feature 'test-roms = []'
  --no-rom-symlink     skip the `tests/roms/external/` symlink

Default: bash sub-shell in the worktree. Exit the shell to clean up.
USAGE
    exit 1
fi

COMMIT="$1"
shift

HARNESSES=()
FEATURE_LINES=()
DO_SYMLINK=1

while [ "$#" -gt 0 ]; do
    case "$1" in
        --harness)
            HARNESSES+=("$2")
            shift 2
            ;;
        --feature)
            FEATURE_LINES+=("$2")
            shift 2
            ;;
        --no-rom-symlink)
            DO_SYMLINK=0
            shift
            ;;
        *)
            echo "unknown arg: $1" 1>&2
            exit 1
            ;;
    esac
done

REPO_ROOT="$(git rev-parse --show-toplevel)"
WT_DIR="/tmp/rustynes-bisect-$$"

trap 'set +e; cd "$REPO_ROOT" 2>/dev/null; git worktree remove --force "$WT_DIR" 2>/dev/null; rm -rf "$WT_DIR" 2>/dev/null' EXIT

echo "[worktree_overlay] git worktree add $WT_DIR $COMMIT" 1>&2
git worktree add --detach "$WT_DIR" "$COMMIT"

# Overlay any harness files from the CURRENT HEAD into the worktree.
# These land as UNTRACKED files; the worktree is read-only-ish from
# git's perspective but cargo doesn't care.
for h in "${HARNESSES[@]+"${HARNESSES[@]}"}"; do
    src="$REPO_ROOT/$h"
    dst="$WT_DIR/$h"
    if [ ! -f "$src" ]; then
        echo "[worktree_overlay] WARN: $src not found in HEAD; skipping" 1>&2
        continue
    fi
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
    echo "[worktree_overlay] overlay: $h" 1>&2
done

# Patch Cargo.toml features. We append to the [features] table of
# `crates/rustynes-test-harness/Cargo.toml` since that's where the new
# harnesses' feature flags live. The simple `awk` insert avoids
# pulling in toml-rs.
CARGO_TOML="$WT_DIR/crates/rustynes-test-harness/Cargo.toml"
for line in "${FEATURE_LINES[@]+"${FEATURE_LINES[@]}"}"; do
    if [ -f "$CARGO_TOML" ]; then
        # Insert AFTER the `[features]` heading. If the feature already
        # exists at this commit, skip (don't duplicate).
        key="${line%% =*}"
        if grep -q "^${key} " "$CARGO_TOML" 2>/dev/null; then
            echo "[worktree_overlay] feature '$key' already present; skipping" 1>&2
            continue
        fi
        awk -v insert="$line" '
            /^\[features\]/ { print; print insert; next }
            { print }
        ' "$CARGO_TOML" > "$CARGO_TOML.tmp"
        mv "$CARGO_TOML.tmp" "$CARGO_TOML"
        echo "[worktree_overlay] cargo feature: $line" 1>&2
    fi
done

# Symlink the gitignored real-ROM directory if it exists. The new
# `audio_tests` / `m22` / `mmc1_a12` harnesses don't need this (their
# ROMs are committed), but `external_real_games` does.
if [ "$DO_SYMLINK" = "1" ]; then
    ext="$REPO_ROOT/tests/roms/external"
    if [ -d "$ext" ]; then
        # The worktree has its own `tests/roms/external/` from the
        # commit (likely empty if gitignored). Replace with a symlink
        # to the parent repo's copy.
        wt_ext="$WT_DIR/tests/roms/external"
        if [ -d "$wt_ext" ] && [ ! -L "$wt_ext" ]; then
            rmdir "$wt_ext" 2>/dev/null || true
        fi
        ln -sf "$ext" "$wt_ext"
        echo "[worktree_overlay] symlink: $wt_ext -> $ext" 1>&2
    fi
fi

echo "[worktree_overlay] entering shell at $WT_DIR. Exit when done." 1>&2
cd "$WT_DIR"
exec "${SHELL:-bash}"
