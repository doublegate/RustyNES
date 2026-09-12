"""Apply prepared replies to review threads, and resolve only the ones addressed.

Completes `scripts/pr-review/`: the two `list_*` filters READ the ceremony's
working list, and this writes the outcome back. Before it existed the reply and
resolve half was hand-rolled `gh api graphql` per thread, once per release, which
is how a thread gets resolved without a reply or a reply lands on the wrong id.

Reads the same GraphQL `reviewThreads` payload on **stdin** as its siblings (see
this directory's README for the query) and a PLAN on `--plan`:

    {"PRRT_kwDO...": {"reply": "why this is fixed", "resolve": true},
     "PRRT_kwDO...": {"reply": "declined, because ...", "resolve": false}}

Dry run by default; `--execute` performs the mutations.

# The properties this encodes, each from a scar this project already has

* **Dry-run first.** A prepared plan is reviewable before anything is posted, and
  a reply cannot be unsent.
* **A resolve REQUIRES a reply.** The ceremony's rule is that a thread is
  resolved only once it has been addressed, with a note saying what happened --
  never to tidy feedback away. `resolve` without `reply` is refused rather than
  honoured, so the coupling is structural instead of remembered.
* **A failed reply never resolves.** Resolving a thread whose reply did not post
  hides the finding behind a green checkmark.
* **A GraphQL error exits 0.** `gh api graphql` reports mutation failures in an
  `errors` array with a zero exit status, so every response is inspected. This is
  the same shape as the lookup that reported "absent" on failure -- an error that
  cannot be told from success is worse than a crash.
* **A plan entry that matches nothing is REFUSED**, not skipped. A thread id that
  is stale, mistyped, or already resolved means the plan was built against a
  different state, and silently applying the rest is how a finding goes
  unanswered while the run reports success.
"""

import argparse
import json
import subprocess
import sys

_REPLY = (
    "mutation($t:ID!,$b:String!){addPullRequestReviewThreadReply"
    "(input:{pullRequestReviewThreadId:$t,body:$b}){clientMutationId}}"
)
_RESOLVE = "mutation($t:ID!){resolveReviewThread(input:{threadId:$t}){thread{isResolved}}}"


def threads_from(doc: dict) -> dict:
    """Map thread id -> thread node, surfacing GraphQL errors rather than indexing past them."""
    if doc.get("errors"):
        msgs = "; ".join(str(e.get("message", e)) for e in doc["errors"])
        raise SystemExit(f"GraphQL error(s): {msgs}")
    pr = ((doc.get("data") or {}).get("repository") or {}).get("pullRequest")
    if pr is None:
        raise SystemExit(
            "GraphQL response has no repository/pullRequest data "
            "(check the owner/repo/pr arguments and token scope)"
        )
    nodes = (pr.get("reviewThreads") or {}).get("nodes") or []
    return {n["id"]: n for n in nodes}


def validate(plan: dict, threads: dict) -> list:
    """Check the plan against reality; return the ordered actions, or raise.

    Extracted so the selftest can reach it without a network -- a validator that
    only exists inside the network path cannot be tested, which this project has
    paid for three times in one release.
    """
    problems, actions = [], []
    for tid, spec in plan.items():
        if not isinstance(spec, dict):
            problems.append(f"{tid}: plan entry is not an object")
            continue
        reply = spec.get("reply")
        resolve = bool(spec.get("resolve"))
        if tid not in threads:
            problems.append(
                f"{tid}: no such thread in the payload -- the plan was built "
                f"against a different state"
            )
            continue
        if threads[tid].get("isResolved"):
            problems.append(f"{tid}: already resolved; refusing to act on a stale plan")
            continue
        if resolve and not (reply or "").strip():
            problems.append(
                f"{tid}: resolve=true with no reply -- a thread is resolved only "
                f"once it has been addressed, with a note saying what happened"
            )
            continue
        if not (reply or "").strip():
            problems.append(f"{tid}: empty reply and resolve=false -- nothing to do")
            continue
        actions.append((tid, reply, resolve))
    if problems:
        raise SystemExit("plan refused:\n  " + "\n  ".join(problems))
    return actions


def _graphql(query: str, **params) -> dict:
    args = ["gh", "api", "graphql", "-f", f"query={query}"]
    for key, value in params.items():
        args += ["-f", f"{key}={value}"]
    done = subprocess.run(args, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        return {"errors": [{"message": (done.stderr or done.stdout).strip()[:400]}]}
    try:
        return json.loads(done.stdout or "{}")
    except json.JSONDecodeError as exc:
        return {"errors": [{"message": f"unparseable response ({exc})"}]}


def apply(actions: list, call) -> tuple:
    """Post each reply and resolve only what succeeded. Returns (replied, resolved, failed).

    `call(query, **params) -> dict` is injected so the selftest can drive the
    ordering rules without a network. Keeping this inside `main()` made the most
    important rule here -- never resolve a thread whose reply failed -- reachable
    only by a live GitHub call, and a mutation removing it came back NOT CAUGHT.
    """
    replied = resolved = failed = 0
    for tid, reply, resolve in actions:
        res = call(_REPLY, t=tid, b=reply)
        if res.get("errors"):
            failed += 1
            print(f"FAILED reply {tid}: {res['errors']}", file=sys.stderr)
            # Deliberately no resolve: a resolved thread whose reply never posted
            # hides the finding behind a green checkmark.
            continue
        replied += 1
        if not resolve:
            print(f"replied {tid} (left OPEN)")
            continue
        res = call(_RESOLVE, t=tid)
        if res.get("errors"):
            failed += 1
            print(f"replied {tid} but RESOLVE FAILED: {res['errors']}", file=sys.stderr)
            continue
        resolved += 1
        print(f"replied + resolved {tid}")
    return replied, resolved, failed


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--plan", required=True, help="JSON file: thread id -> {reply, resolve}")
    ap.add_argument("--execute", action="store_true", help="perform the mutations")
    args = ap.parse_args()

    threads = threads_from(json.load(sys.stdin))
    with open(args.plan, encoding="utf-8") as fh:
        plan = json.load(fh)
    actions = validate(plan, threads)

    if not args.execute:
        for tid, reply, resolve in actions:
            path = threads[tid].get("path")
            print(f"[dry-run] {tid} {path}: reply {len(reply)}B, "
                  f"{'RESOLVE' if resolve else 'leave open'}")
        print(f"\n{len(actions)} action(s). Re-run with --execute to apply.")
        return

    replied, resolved, failed = apply(actions, _graphql)
    print(f"\nreplied={replied} resolved={resolved} failed={failed}")
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
