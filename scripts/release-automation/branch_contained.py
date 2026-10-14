#!/usr/bin/env python3
"""Is everything a branch or worktree added already on `main`?

Run before deleting a branch or removing a worktree after a squash merge.

Why a line check and not git's own: a squash merge makes a NEW commit, so
`git branch --merged main` never lists the branch and `git branch -d` refuses
it, while `-D` deletes it without looking. Comparing trees fails too, because
`main` has moved on since the merge. What is actually wanted is "every line this
branch added is present in that file on `main`", which survives both.

It checks four things:

* the branch's committed lines: `git diff <main>...<branch>` (from the merge
  base, so lines `main` added meanwhile do not count against the branch);
* in a worktree, its uncommitted lines: `git diff HEAD`;
* binary files changed either way, which carry no lines: the branch's (or the
  worktree's) blob must equal `main`'s at the same path;
* in a worktree, its untracked files: each must exist on `main` at the same
  path WITH THE SAME CONTENTS.

A line counts as present when its stripped text EQUALS a stripped line of the
same path on `main` (a substring test would let `return value` pass against
`return value + 1`). Lines shorter than 8 characters (braces, `end`, blank-ish
lines) are skipped as uninformative. This is a containment heuristic, not a
proof: a line moved to a different file reads as missing (a false alarm, safe),
and a line that also occurs elsewhere in the file reads as present. Read every
MISSING line before deleting anything.

The exact-line, binary and untracked-contents rules were added after
`CodeRabbit` on #604 and #605 found the first version weaker than this
docstring claimed.

The v3.1.0 and v3.2.0 cleanups used an earlier form of this, which checked
only uncommitted lines. It was enough for those worktrees, but it would have
passed a branch with unmerged commits.

Usage:
    branch_contained.py <worktree-or-repo> [--branch NAME] [--main main]

Exit status: 0 when nothing is missing, 1 when something is, 2 on a usage or
git error.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
from collections import defaultdict

MIN_LEN = 8
SHOW_MISSING = 12


def git(repo: str, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True, check=check)


def blob(repo: str, rev_path: str) -> bytes | None:
    """The bytes of `rev:path`, or None when it does not exist there."""
    r = subprocess.run(["git", "-C", repo, "show", rev_path], capture_output=True, check=False)
    return r.stdout if r.returncode == 0 else None


def binary_paths(numstat: str) -> list[str]:
    """Paths `git diff --numstat` reports as binary (`-<TAB>-<TAB>path`)."""
    return [ln.split("\t", 2)[2] for ln in numstat.splitlines() if ln.startswith("-\t-\t")]


def added_lines(diff: str) -> dict[str, list[str]]:
    """Map each `+++ b/<path>` in a unified diff to its added, stripped lines."""
    out: dict[str, list[str]] = defaultdict(list)
    cur = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            cur = line[6:] if line.startswith("+++ b/") else None
        elif line.startswith("+") and cur is not None:
            text = line[1:].strip()
            if len(text) >= MIN_LEN:
                out[cur].append(text)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("path", help="a worktree, or the repository holding the branch")
    ap.add_argument("--branch", help="branch to check (default: the one checked out at PATH)")
    ap.add_argument("--main", default="main", help="the branch it was merged into (default: main)")
    a = ap.parse_args()

    try:
        branch = a.branch or git(a.path, "branch", "--show-current").stdout.strip()
        if not branch:
            print("detached HEAD: pass --branch", file=sys.stderr)
            return 2
        committed = git(a.path, "diff", f"{a.main}...{branch}").stdout
        committed_bin = binary_paths(
            git(a.path, "diff", "--no-renames", "--numstat", f"{a.main}...{branch}").stdout
        )
        checked_out = git(a.path, "branch", "--show-current").stdout.strip() == branch
        uncommitted = git(a.path, "diff", "HEAD").stdout if checked_out else ""
        uncommitted_bin = (
            binary_paths(git(a.path, "diff", "--no-renames", "--numstat", "HEAD").stdout)
            if checked_out
            else []
        )
        untracked = (
            git(a.path, "ls-files", "--others", "--exclude-standard").stdout.split("\n")
            if checked_out
            else []
        )
    except subprocess.CalledProcessError as e:
        print(f"git failed: {' '.join(e.cmd)}\n{e.stderr.strip()}", file=sys.stderr)
        return 2

    lines = added_lines(committed)
    for path, extra in added_lines(uncommitted).items():
        lines[path].extend(extra)

    total = missing = 0
    for path, texts in sorted(lines.items()):
        on_main = git(a.path, "show", f"{a.main}:{path}", check=False).stdout
        main_lines = {ln.strip() for ln in on_main.splitlines()}
        for text in texts:
            total += 1
            if text not in main_lines:
                missing += 1
                if missing <= SHOW_MISSING:
                    print(f"  MISSING {path}: {text[:110]}")
    if missing > SHOW_MISSING:
        print(f"  ... and {missing - SHOW_MISSING} more")

    # Binary files: the branch's blob (or, for an uncommitted change, the file
    # on disk) must equal main's at the same path.
    binaries = 0
    for path in committed_bin:
        binaries += 1
        if blob(a.path, f"{branch}:{path}") != blob(a.path, f"{a.main}:{path}"):
            missing += 1
            print(f"  MISSING (binary) {path}")
    for path in uncommitted_bin:
        binaries += 1
        disk = os.path.join(a.path, path)
        here = open(disk, "rb").read() if os.path.isfile(disk) else None
        if here != blob(a.path, f"{a.main}:{path}"):
            missing += 1
            print(f"  MISSING (binary, uncommitted) {path}")

    absent = 0
    for path in filter(None, untracked):
        with open(os.path.join(a.path, path), "rb") as f:
            here = f.read()
        there = blob(a.path, f"{a.main}:{path}")
        if there != here:
            absent += 1
            what = "NOT ON" if there is None else "DIFFERS FROM"
            print(f"  UNTRACKED, {what} {a.main}: {path}")

    print(
        f"{branch}: {total} added lines and {binaries} binary files checked, "
        f"{missing} not found on {a.main}; "
        f"{len([u for u in untracked if u])} untracked files, {absent} not on {a.main}"
    )
    return 0 if missing == 0 and absent == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
