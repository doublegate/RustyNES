"""Selftest for `reply_and_resolve.py` -- exercises the real validator, no network.

Follows `agy-review-selftest.sh`'s precedent: a script that posts to GitHub gets a
test that runs its ACTUAL decision function rather than a copy. A selftest that
re-implements the rule it checks agrees with itself forever -- this project has
that scar, from a test whose local `strip()` made deleting the production one come
back NOT CAUGHT.

Run: `python3 scripts/pr-review/reply_and_resolve_selftest.py`
"""

import importlib.util
import pathlib
import sys

_HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("rar", _HERE / "reply_and_resolve.py")
rar = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(rar)

FAILURES = []


def check(name: str, ok: bool, detail: str = "") -> None:
    print(f"  {'ok  ' if ok else 'FAIL'} {name}{('  -- ' + detail) if detail and not ok else ''}")
    if not ok:
        FAILURES.append(name)


def refuses(plan, threads, because: str) -> bool:
    """True only if validate() refused FOR THE STATED REASON.

    Asserting merely that it raised is too weak, and the mutation pass proved it:
    deleting the resolve-requires-reply guard still "passed", because the next
    guard caught the same input with a different message. A test that cannot tell
    which mechanism fired lets a neighbouring check silently cover for a deleted
    one -- and the surviving message then misdescribes the failure.
    """
    try:
        rar.validate(plan, threads)
    except SystemExit as exc:
        return because in str(exc)
    return False


def payload(*threads) -> dict:
    return {"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": list(threads)}}}}}


def thread(tid: str, resolved: bool = False, path: str = "a.rs") -> dict:
    return {"id": tid, "isResolved": resolved, "path": path}


def main() -> None:
    print("reply_and_resolve selftest")

    open_threads = rar.threads_from(payload(thread("T1"), thread("T2")))
    check("payload parses to a thread map", set(open_threads) == {"T1", "T2"})

    # The happy paths, both shapes the ceremony actually uses.
    acts = rar.validate({"T1": {"reply": "fixed in abc123", "resolve": True}}, open_threads)
    check("reply + resolve yields one resolving action", acts == [("T1", "fixed in abc123", True)])
    acts = rar.validate({"T2": {"reply": "declined because ...", "resolve": False}}, open_threads)
    check("reply alone leaves the thread open", acts == [("T2", "declined because ...", False)])

    # Each guard, stated as the rule it enforces.
    check(
        "resolve WITHOUT a reply is refused",
        refuses({"T1": {"resolve": True}}, open_threads, "resolve=true with no reply"),
        "a thread is resolved only once addressed, with a note saying what happened",
    )
    check(
        "resolve with a whitespace-only reply is refused",
        refuses({"T1": {"reply": "   \n", "resolve": True}}, open_threads, "resolve=true with no reply"),
        "an empty reply is not a note",
    )
    check(
        "an unknown thread id is refused, not skipped",
        refuses({"NOPE": {"reply": "x", "resolve": True}}, open_threads, "no such thread"),
        "a stale/mistyped id means the plan was built against a different state",
    )
    check(
        "an ALREADY-RESOLVED thread is refused",
        refuses({"T3": {"reply": "x", "resolve": True}},
                rar.threads_from(payload(thread("T3", resolved=True))), "already resolved"),
        "acting on a stale plan",
    )
    check(
        "an entry that would do nothing is refused",
        refuses({"T1": {"reply": "", "resolve": False}}, open_threads, "nothing to do"),
    )
    check(
        "a non-object plan entry is refused",
        refuses({"T1": "just a string"}, open_threads, "not an object"),
    )

    # GraphQL reports failure with exit status 0, so the reader must look.
    check(
        "a GraphQL `errors` payload is surfaced",
        refuses_threads({"errors": [{"message": "Bad credentials"}]}, "Bad credentials"),
        "gh api graphql exits 0 on an error body",
    )
    check(
        "a null pullRequest is surfaced",
        refuses_threads({"data": {"repository": {"pullRequest": None}}}, "no repository/pullRequest"),
    )

    # The execution loop's ordering rules, driven through an injected caller.
    class Fake:
        """Returns payloads shaped like the real API.

        The first version returned `{"data": {}}` for success -- which is exactly
        the shape review found slipping through, so the test was encoding the weak
        contract it was supposed to police. A fake must be at least as strict as
        the thing it stands in for.
        """
        def __init__(self, fail_on=None, hollow=False):
            self.calls, self.fail_on, self.hollow = [], fail_on, hollow
        def __call__(self, query, **params):
            kind = "resolve" if "resolveReviewThread" in query else "reply"
            self.calls.append((kind, params.get("t")))
            if kind == self.fail_on:
                return {"errors": [{"message": "nope"}]}
            if self.hollow:
                return {}          # no errors, and no evidence of anything
            return ({"data": {"resolveReviewThread": {"thread": {"isResolved": True}}}}
                    if kind == "resolve"
                    else {"data": {"addPullRequestReviewThreadReply": {"comment": {"id": "C1"}}}})

    ok = Fake()
    check("a successful reply+resolve issues both calls, reply first",
          rar.apply([("T1", "body", True)], ok) == (1, 1, 0)
          and ok.calls == [("reply", "T1"), ("resolve", "T1")])

    open_only = Fake()
    check("resolve=False issues NO resolve call",
          rar.apply([("T1", "body", False)], open_only) == (1, 0, 0)
          and open_only.calls == [("reply", "T1")])

    bad_reply = Fake(fail_on="reply")
    replied, resolved, failed = rar.apply([("T1", "body", True)], bad_reply)
    check("a FAILED reply never resolves",
          (replied, resolved, failed) == (0, 0, 1)
          and [k for k, _ in bad_reply.calls] == ["reply"],
          "resolving a thread whose reply never posted hides the finding")

    bad_resolve = Fake(fail_on="resolve")
    check("a failed resolve is counted as a failure, not a success",
          rar.apply([("T1", "body", True)], bad_resolve) == (1, 0, 1))

    # Regression: a response with no `errors` and no evidence is NOT a success.
    hollow = Fake(hollow=True)
    check("an EMPTY reply payload is not a success, and issues no resolve",
          rar.apply([("T1", "body", True)], hollow) == (0, 0, 1)
          and [k for k, _ in hollow.calls] == ["reply"],
          "{} carries no errors and proves nothing -- it must not resolve a thread")

    check("reply_ok demands the created comment id",
          rar.reply_ok({"data": {"addPullRequestReviewThreadReply": {"comment": {"id": "C1"}}}})
          and not rar.reply_ok({"data": {}}) and not rar.reply_ok({})
          and not rar.reply_ok({"errors": [{"message": "x"}]}))
    check("resolve_ok demands isResolved true",
          rar.resolve_ok({"data": {"resolveReviewThread": {"thread": {"isResolved": True}}}})
          and not rar.resolve_ok({"data": {"resolveReviewThread": {"thread": {"isResolved": False}}}})
          and not rar.resolve_ok({}))

    # Plan schema.
    check("a top-level array is refused",
          refuses([], open_threads, "top level must be an object"))
    check("a non-string reply is refused",
          refuses({"T1": {"reply": 1, "resolve": False}}, open_threads, "must be a string"))
    check("resolve given as the STRING \"false\" is refused, not coerced true",
          refuses({"T1": {"reply": "x", "resolve": "false"}}, open_threads,
                  "must be true/false"),
          "bool(\"false\") is True, which would resolve a thread meant to stay open")

    # Untrusted-input paths: every one of these CRASHED before, and a traceback
    # where a refusal belongs loses the diagnostic exactly when it is needed.
    check("a non-object stdin payload is refused, not crashed",
          refuses_threads([], "must be a JSON object"))
    check("a STRING GraphQL error entry is reported, not crashed",
          refuses_threads({"errors": ["boom"]}, "boom"),
          "reporting the API's error must not itself crash")
    check("a non-list `errors` value is still reported",
          refuses_threads({"errors": {"message": "solo"}}, "solo"))

    import io
    def loads(text):
        try:
            rar._load(io.StringIO(text), "the thing")
        except SystemExit as exc:
            return "not valid JSON" in str(exc)
        return False
    check("malformed JSON is refused with a message", loads("{not json"))

    print(f"\n{'FAILED: ' + ', '.join(FAILURES) if FAILURES else 'all checks passed'}")
    sys.exit(1 if FAILURES else 0)


def refuses_threads(doc, because: str) -> bool:
    """True only if threads_from() refused FOR THE STATED REASON -- see `refuses`."""
    try:
        rar.threads_from(doc)
    except SystemExit as exc:
        return because in str(exc)
    return False


if __name__ == "__main__":
    main()
