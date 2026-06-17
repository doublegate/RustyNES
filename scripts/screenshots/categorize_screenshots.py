#!/usr/bin/env python3
"""Shim: superseded by ``scripts/coverage/coverage.py categorize``.

The screenshot tier-split logic now lives in the unified coverage tool. This
forwards to it so the historical entry point keeps working.
"""
import os
import subprocess
import sys

COVERAGE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "coverage", "coverage.py")
sys.exit(subprocess.call([sys.executable, COVERAGE, "categorize", *sys.argv[1:]]))
