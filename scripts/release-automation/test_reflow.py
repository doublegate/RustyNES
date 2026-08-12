#!/usr/bin/env python3
"""Round-trip tests for `reflow.py`.

Stdlib only, so this runs anywhere the tool does:

    python3 scripts/release-automation/test_reflow.py

Every case here exists because the behaviour was wrong once. The hard-break and
fence cases came out of the PR #349 review; the rest pin the preservation
guarantees the module docstring claims, so the doc and the code cannot drift.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from reflow import reflow  # noqa: E402


FAILURES = []


def check(name, given, want):
    got = reflow(given)
    if got != want:
        FAILURES.append(
            f"{name}\n  given: {given!r}\n  want:  {want!r}\n  got:   {got!r}"
        )


# --- the point of the tool -------------------------------------------------

check(
    "wrapped paragraph joins to one line",
    "alpha\nbeta\ngamma\n",
    "alpha beta gamma\n",
)

check(
    "a list item absorbs its own continuations",
    "- alpha\n  beta\n- gamma\n",
    "- alpha beta\n- gamma\n",
)

# --- hard breaks (regression: `strip()` destroyed these) --------------------

check(
    "a hard break ends the logical line and keeps its marker",
    "first  \nsecond\n",
    "first  \nsecond\n",
)

check(
    "text after a hard break still wraps into one line",
    "first  \nsecond\nthird\n",
    "first  \nsecond third\n",
)

check(
    "a trailing hard break inside a list item is kept",
    "- alpha  \n  beta\n",
    "- alpha  \n  beta\n",
)

check(
    "ONE trailing space is not a hard break and is dropped",
    "first \nsecond\n",
    "first second\n",
)

# --- blank-line runs (regression: collapsed to a single blank) -------------

check(
    "runs of blank lines are preserved",
    "alpha\n\n\n\nbeta\n",
    "alpha\n\n\n\nbeta\n",
)

# --- fences (regression: any fence closed any block) -----------------------

check(
    "a ``` line does not close a ````-opened block",
    "````\n```\nstill code\n````\nafter one\nafter two\n",
    "````\n```\nstill code\n````\nafter one after two\n",
)

check(
    "a tilde fence does not close a backtick fence",
    "```\n~~~\nstill code\n```\nafter one\nafter two\n",
    "```\n~~~\nstill code\n```\nafter one after two\n",
)

check(
    "code inside a fence is never rewrapped",
    "```\nlet a = 1;\nlet b = 2;\n```\n",
    "```\nlet a = 1;\nlet b = 2;\n```\n",
)

check(
    "a longer closing fence still closes",
    "```\ncode\n`````\nafter\n",
    "```\ncode\n`````\nafter\n",
)

# --- the other preservation guarantees -------------------------------------

check("headings stand alone", "# Title\nbody\n", "# Title\nbody\n")
check("tables are untouched", "| a | b |\n| - | - |\n", "| a | b |\n| - | - |\n")
check("horizontal rules stand alone", "alpha\n\n---\n\nbeta\n", "alpha\n\n---\n\nbeta\n")
check(
    "a blockquote joins into one wide line",
    "> alpha\n> beta\n",
    "> alpha beta\n",
)
check(
    "a standalone HTML line is untouched",
    '<img src="x.png">\nalpha\nbeta\n',
    '<img src="x.png">\nalpha beta\n',
)

if FAILURES:
    print(f"FAIL: {len(FAILURES)} case(s)\n")
    for f in FAILURES:
        print(f)
        print()
    sys.exit(1)
print("reflow: all cases pass")
