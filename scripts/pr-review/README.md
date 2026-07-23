# `scripts/pr-review/` — PR review-thread helpers

Two small filters for the GitHub GraphQL `reviewThreads` payload, used during the
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

The `id` field is the **thread** node id needed by the `resolveReviewThread`
mutation; the `databaseId` is the **comment** id needed to reply via the REST
`pulls/comments/{id}/replies` endpoint. They are different identifiers and are
not interchangeable, which is why both are printed.

Note when replying via GraphQL: `gh api graphql -F body=-` posts a literal `-`.
Use `-F body=@file` or the REST endpoint with `-f body=...` instead.
