#!/usr/bin/env python3
"""Structure-validation tests for `assemble.py`.

Needs `bs4` (which IS present here — the salvage README claimed otherwise and
was wrong). Run:

    python3 scripts/release-automation/test_assemble.py

These cover only the input contract the script enforces, not the full PDF
transform: no original rendered fragment survived the salvage, so the transform
itself is still exercised only by using it for real.
"""

import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "assemble.py"
GOOD = "<h1>T</h1><table><tr><td>x</td></tr></table><p>NOTE (a): b</p>"
FAILURES = []


def run(fragment, args=None):
    with tempfile.TemporaryDirectory() as d:
        frag = Path(d) / "frag.html"
        frag.write_text(fragment, encoding="utf-8")
        out = Path(d) / "out.html"
        argv = [sys.executable, str(SCRIPT)] + (args if args is not None else [str(frag), str(out)])
        return subprocess.run(argv, capture_output=True, text=True)


def expect_fail(name, fragment, needle, args=None):
    r = run(fragment, args)
    if r.returncode == 0:
        FAILURES.append(f"{name}: expected failure, got exit 0")
    elif needle not in (r.stderr + r.stdout):
        FAILURES.append(f"{name}: message missing {needle!r}\n  got: {(r.stderr + r.stdout).strip()[:200]}")


expect_fail("no arguments", GOOD, "usage:", args=[])
expect_fail("one argument", GOOD, "usage:", args=["only-one"])
expect_fail("missing table", "<h1>T</h1><p>NOTE (a): b</p>", "no top-level <table>")
expect_fail("missing NOTE", "<h1>T</h1><table><tr><td>x</td></tr></table>", "no top-level <p> starting with 'NOTE'")
expect_fail(
    "NOTE before table",
    "<p>NOTE (a): b</p><table><tr><td>x</td></tr></table>",
    "must come after the table",
)

r = run(GOOD)
if r.returncode != 0:
    FAILURES.append(f"well-formed fragment should succeed, got exit {r.returncode}\n  {(r.stderr + r.stdout).strip()[:300]}")

if FAILURES:
    print(f"FAIL: {len(FAILURES)} case(s)\n")
    for f in FAILURES:
        print(f + "\n")
    sys.exit(1)
print("assemble: all cases pass")
