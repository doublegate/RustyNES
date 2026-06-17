#!/usr/bin/env bash
# Shim: superseded by `scripts/coverage/coverage.py montage`.
# The showcase-montage logic now lives in the unified coverage tool.
exec python3 "$(dirname "$0")/../coverage/coverage.py" montage "$@"
