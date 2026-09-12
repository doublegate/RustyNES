# `scripts/pr-review/` — PR review-thread helpers

Helpers for the GitHub GraphQL `reviewThreads` payload, used during the
bot-comment closeout ceremony (this repo runs three automated reviewers:
`gemini-code-assist`, `copilot-pull-request-reviewer`, and CodeRabbit, and every
thread is replied to and resolved before a merge).

Both read the GraphQL response on **stdin**, so they compose with `gh api`:

```bash
gh api graphql -f query='
  query($owner:String!,$repo:String!,$pr:Int!){
    repository(owner:$owner,name:$repo){
      pullRequest(number:$pr){
        reviewThreads(first:100){ nodes{
          id isResolved isOutdated path line
          comments(first:1){ nodes{ databaseId body author{login} } }
        }}
      }
    }
  }' -F owner=doublegate -F repo=RustyNES -F pr=325 \
  | python3 scripts/pr-review/list_unresolved_threads.py
```

| Script | Output |
|---|---|
| `list_unresolved_threads.py` | Only unresolved threads: thread id, comment `databaseId`, `path:line`, author, and a truncated body. The working list for the ceremony. |
| `list_all_threads.py` | Every thread with its `isResolved` / `isOutdated` flags — the audit view, for confirming nothing was missed. |
| `reply_and_resolve.py` | Applies prepared replies and resolves **only** the threads that were addressed. Dry run by default. |
| `reply_and_resolve_selftest.py` | No-network selftest of the two above it. Run it after editing either. |

## Writing the outcome back

The two filters read the working list; `reply_and_resolve.py` writes the result,
so the reply/resolve half of the ceremony stops being hand-rolled `gh api` calls
once per release — which is how a thread gets resolved without a reply, or a
reply lands on the wrong id.

It takes the same payload on stdin plus a plan keyed by **thread** id:

```bash
cat > /tmp/plan.json <<'JSON'
{"PRRT_kwDOxxx": {"reply": "Fixed in abc1234 — the guard now ...", "resolve": true},
 "PRRT_kwDOyyy": {"reply": "Declined: this is vendored upstream ...", "resolve": false}}
JSON

gh api graphql -f query='...' -F owner=doublegate -F repo=RustyNES -F pr=503 \
  | python3 scripts/pr-review/reply_and_resolve.py --plan /tmp/plan.json          # dry run
  # ... review the plan, then:
  | python3 scripts/pr-review/reply_and_resolve.py --plan /tmp/plan.json --execute
```

**What it refuses, and why.** Each rule is a scar this repo already has, and each
is pinned by a mutation in the selftest:

- `resolve: true` with **no reply** — a thread is resolved once it has been
  *addressed*, with a note saying what happened, never to tidy feedback away.
- A **failed reply never resolves.** A resolved thread whose reply did not post
  hides the finding behind a green checkmark.
- A plan entry naming a thread that is **absent or already resolved** is refused
  rather than skipped: it means the plan was built against a different state, and
  applying the rest would leave a finding unanswered while the run reports success.
- Every response is checked for a GraphQL `errors` array, because
  `gh api graphql` reports mutation failures **with exit status 0**.

Declining a finding is a first-class outcome — set `resolve: false` and say why
in the reply, which leaves the thread visible instead of tidied away.

The `id` field is the **thread** node id needed by the `resolveReviewThread`
mutation; the `databaseId` is the **comment** id needed to reply via the REST
`pulls/comments/{id}/replies` endpoint. They are different identifiers and are
not interchangeable, which is why both are printed.

Note when replying via GraphQL: `gh api graphql -F body=-` posts a literal `-`.
Use `-F body=@file` or the REST endpoint with `-f body=...` instead.
