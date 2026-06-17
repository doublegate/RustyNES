#!/usr/bin/env bash
# Shim: superseded by the unified coverage pipeline.
#
# Raw-capture staging is no longer a separate step: the Rust harness
# (crates/rustynes-test-harness/tests/external_real_games.rs) emits screenshots
# directly into the per-mapper tree, and `coverage.py categorize` tier-splits
# them (Core/Curated -> screenshots/external/, BestEffort -> screenshots/besteffort/).
# Build the showcase montage with `coverage.py montage`.
echo "organize_screenshots.sh is superseded; use:" >&2
echo "  python3 scripts/coverage/coverage.py categorize   # tier-split the tree" >&2
echo "  python3 scripts/coverage/coverage.py montage       # build the montage" >&2
exec python3 "$(dirname "$0")/../coverage/coverage.py" categorize "$@"
