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
    threads = (pr.get("reviewThreads") or {}).get("nodes") or []
    shown = 0
    for thread in threads:
        if thread["isResolved"]:
            continue
        comments = thread["comments"]["nodes"]
        if not comments:
            continue
        c = comments[0]
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
