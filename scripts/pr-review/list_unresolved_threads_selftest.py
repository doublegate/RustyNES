"""Selftest for `list_unresolved_threads.py` -- runs the real script, no network.

Same precedent as `reply_and_resolve_selftest.py`: the test drives the ACTUAL
script (here as a subprocess, feeding a payload on stdin, exactly as the
ceremony pipes `gh api graphql` into it) rather than a copy of its rules.

The cases that matter are the fail-closed ones. "0 unresolved thread(s)" is the
line that lets a merge go ahead, so a payload WITHOUT a thread list must be an
error, not an all-clear -- until v3.0.1 a missing or null `reviewThreads.nodes`
printed exactly that line and exited 0.

Run: `python3 scripts/pr-review/list_unresolved_threads_selftest.py`
"""

import json
import pathlib
import subprocess
import sys

_SCRIPT = pathlib.Path(__file__).resolve().parent / "list_unresolved_threads.py"

FAILURES = []


def check(name: str, ok: bool, detail: str = "") -> None:
    print(f"  {'ok  ' if ok else 'FAIL'} {name}{('  -- ' + detail) if detail and not ok else ''}")
    if not ok:
        FAILURES.append(name)


def run(doc) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-I", str(_SCRIPT)],
        input=json.dumps(doc),
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )


def pr(review_threads) -> dict:
    return {"data": {"repository": {"pullRequest": review_threads}}}


def refused(doc, because: str) -> bool:
    """True only if the script exited non-zero FOR THE STATED REASON, with no traceback."""
    r = run(doc)
    return r.returncode != 0 and because in r.stderr and "Traceback" not in r.stderr


def comment(db_id=7, body="b") -> dict:
    return {"databaseId": db_id, "body": body, "author": {"login": "bot"}}


def thread(tid="T1", resolved=False, comments=None) -> dict:
    return {
        "id": tid,
        "isResolved": resolved,
        "path": "a.rs",
        "line": 1,
        "comments": {"nodes": [comment()] if comments is None else comments},
    }


def main() -> None:
    print("list_unresolved_threads selftest")

    r = run(pr({"reviewThreads": {"nodes": [thread("T1"), thread("T2", resolved=True)]}}))
    check(
        "an open and a resolved thread list exactly one",
        r.returncode == 0 and "TID=T1 dbId=7" in r.stdout and "T2" not in r.stdout
        and r.stdout.rstrip().endswith("1 unresolved thread(s)"),
        r.stdout + r.stderr,
    )
    r = run(pr({"reviewThreads": {"nodes": []}}))
    check(
        "an EMPTY list is a genuine all-clear",
        r.returncode == 0 and r.stdout.strip() == "0 unresolved thread(s)",
        r.stdout + r.stderr,
    )

    # Fail closed: no list is not an empty list.
    check(
        "missing reviewThreads is refused, not 0 threads",
        refused(pr({}), "no reviewThreads.nodes list"),
    )
    check(
        "null reviewThreads.nodes is refused, not 0 threads",
        refused(pr({"reviewThreads": {"nodes": None}}), "no reviewThreads.nodes list"),
    )

    # Partial nodes: a named error, never a bare KeyError traceback.
    check(
        "a node without isResolved is refused by name",
        refused(pr({"reviewThreads": {"nodes": [{"id": "T1"}]}}), "has no boolean isResolved"),
    )
    bad = thread("T1")
    del bad["comments"]
    check(
        "an open node without comments.nodes is refused by name",
        refused(pr({"reviewThreads": {"nodes": [bad]}}), "has no comments.nodes list"),
    )
    check(
        "a first comment without databaseId is refused by name",
        refused(
            pr({"reviewThreads": {"nodes": [thread("T1", comments=[{"body": "x"}])]}}),
            "first comment has no databaseId",
        ),
    )

    if FAILURES:
        print(f"FAILED: {len(FAILURES)} check(s): {', '.join(FAILURES)}")
        sys.exit(1)
    print("all checks passed")


if __name__ == "__main__":
    main()
