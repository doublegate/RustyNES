"""Print the unresolved review threads from a GitHub GraphQL `reviewThreads` payload.

Reads the GraphQL response on stdin; see this directory's README for the query.

Every field printed here (path, author login, comment body) is attacker-controlled
text: anyone who can comment on a public PR chooses it. Writing it to a terminal raw
would let an ESC/C1 sequence repaint the screen, hide subsequent output, or fake a
"resolved" line during the closeout ceremony -- so control characters are escaped
before printing rather than passed through.
"""

import json
import sys

# C0 controls (minus the tab/newline we handle ourselves), DEL, and the C1 block.
# ESC (0x1b) is the one that actually matters -- it introduces every CSI/OSC
# sequence -- but the whole range is cheap to neutralize and leaves no gaps.
_UNSAFE = (
    set(range(0x00, 0x20)) - {0x09, 0x0A, 0x0D}
) | {0x7F} | set(range(0x80, 0xA0))


def safe(value: object) -> str:
    """Render `value` as a single line with control characters made visible."""
    text = "" if value is None else str(value)
    out = []
    for ch in text:
        cp = ord(ch)
        if cp in _UNSAFE:
            out.append(f"\\x{cp:02x}")
        elif ch in "\n\r\t":
            out.append(" ")
        else:
            out.append(ch)
    return "".join(out)


def main() -> None:
    doc = json.load(sys.stdin)

    # A GraphQL response can carry `errors` with a null (or partial) `data`, and
    # `gh api graphql` exits 0 in that case. Blindly indexing into `data` then
    # dies with a bare KeyError/TypeError that hides the real cause (a bad token,
    # a renamed field, a rate limit). Surface the API error instead.
    if doc.get("errors"):
        msgs = "; ".join(safe(e.get("message", e)) for e in doc["errors"])
        raise SystemExit(f"GraphQL error(s): {msgs}")

    pr = (((doc.get("data") or {}).get("repository") or {}).get("pullRequest"))
    if pr is None:
        raise SystemExit(
            "GraphQL response has no repository/pullRequest data "
            "(check the owner/repo/pr arguments and token scope)"
        )
    # FAIL CLOSED. This is a closeout gate: "0 unresolved thread(s)" is the line
    # that lets a merge go ahead, so it must mean "the payload listed threads and
    # none was open" -- never "the payload had no thread list". Until v3.0.1 a
    # missing or null `reviewThreads.nodes` (a query without the field, a partial
    # response) collapsed to `[]` and printed exactly that all-clear.
    threads = (pr.get("reviewThreads") or {}).get("nodes")
    if not isinstance(threads, list):
        raise SystemExit(
            "GraphQL response has no reviewThreads.nodes list "
            "(check that the query selects reviewThreads { nodes { ... } })"
        )
    # A TRUNCATED list must fail closed too (v3.0.1, Copilot on #590): the query
    # asks for `first:100`, and a PR with more threads used to print an
    # all-clear for page one while later pages held open threads. The payload
    # must say whether more pages exist, and this gate refuses if they do.
    page = (pr.get("reviewThreads") or {}).get("pageInfo")
    if not isinstance(page, dict) or not isinstance(page.get("hasNextPage"), bool):
        raise SystemExit(
            "GraphQL response has no reviewThreads.pageInfo.hasNextPage "
            "(select it, so a truncated thread list cannot read as complete)"
        )
    if page["hasNextPage"]:
        raise SystemExit(
            "more review threads than one page: refusing a partial count "
            "(raise first:, or page with after: endCursor and check each page)"
        )
    # The other end too (CodeRabbit on #592): a page fetched with `after:` can
    # be the LAST page, with `hasNextPage: false`, while earlier pages hold
    # open threads. Only a payload that starts at the first page is complete.
    if not isinstance(page.get("hasPreviousPage"), bool):
        raise SystemExit(
            "GraphQL response has no reviewThreads.pageInfo.hasPreviousPage "
            "(select it, so a later page cannot read as the whole list)"
        )
    if page["hasPreviousPage"]:
        raise SystemExit("this thread list is not the first page: refusing a partial count")
    shown = 0
    for i, thread in enumerate(threads):
        # A partial node used to die on a bare KeyError/TypeError traceback;
        # name the node and the field instead.
        if not isinstance(thread, dict) or not isinstance(thread.get("isResolved"), bool):
            raise SystemExit(f"review thread #{i} has no boolean isResolved")
        if thread["isResolved"]:
            continue
        if not thread.get("id"):
            raise SystemExit(f"review thread #{i} has no id")
        comments = (thread.get("comments") or {}).get("nodes")
        if not isinstance(comments, list):
            raise SystemExit(f"review thread #{i} ({safe(thread.get('id'))}) has no comments.nodes list")
        # A real review thread always has a comment, so an empty list means a
        # partial query (`comments(first:0)`) -- skipping it would let an open
        # thread reach the all-clear (v3.0.1, Copilot on #592).
        if not comments:
            raise SystemExit(f"review thread #{i} ({safe(thread.get('id'))}) has no comments")
        c = comments[0]
        if not isinstance(c, dict) or c.get("databaseId") is None:
            raise SystemExit(
                f"review thread #{i} ({safe(thread.get('id'))}): first comment has no databaseId"
            )
        author = (c.get("author") or {}).get("login")
        print(
            f"TID={safe(thread['id'])} dbId={safe(c['databaseId'])} "
            f"{safe(thread.get('path'))}:{safe(thread.get('line'))} by={safe(author)}"
        )
        print("  ", safe(c.get("body"))[:650])
        print()
        shown += 1
    print(f"{shown} unresolved thread(s)")


if __name__ == "__main__":
    main()
