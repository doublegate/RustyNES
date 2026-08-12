# Release-notes / doc rendering helpers

Three one-off tools rescued from `/tmp` before a reboot (2026-08-06). They were
written during the v2.2.5–v2.3.0 documentation work, existed **nowhere else on
disk**, and would have been lost. Recorded here so they are findable rather than
rediscovered.

| script | needs | what it does |
| --- | --- | --- |
| `reflow.py` | stdlib only | Unwraps hard-wrapped markdown into single full-width lines. Tests: `test_reflow.py` (16 cases). |
| `assemble.py` | `bs4` | Assembles a rendered HTML fragment into a full document. Tests: `test_assemble.py` (input contract). |
| `guardrails_assemble.py` | `bs4` | Same, for the provenance-guardrails doc: injects a title block and reddens a curated set of hard takeaways. |

## `reflow.py` — the one you will want again

This is the tool that fixed the GitHub release-notes formatting complaint: notes
published from v2.2.5 onward had been hard-wrapped at ~80 columns, which GitHub
renders as artificially narrow text instead of using the full width available.

It unwraps paragraphs and list items to one line each while **preserving**
blank lines, ATX headings, horizontal rules, fenced code blocks, tables,
blockquotes, and raw HTML lines — the things that break if naively joined.

```bash
python3 scripts/release-automation/reflow.py < in.md > out.md
```

Worth running over any hand-wrapped `.github/release-notes/vX.Y.Z.md` before
publishing.

## The `bs4` pair

`assemble.py` and `guardrails_assemble.py` turn a rendered HTML fragment into a
standalone document. They need BeautifulSoup, which is not a project dependency
but **is** present on this machine (`bs4` 4.15.0):

```bash
python3 scripts/release-automation/assemble.py frag.html out.html
# or, on a machine without it:
python3 -m venv /tmp/venv && /tmp/venv/bin/pip install beautifulsoup4
```

Both are specific to the one-time provenance/guardrails PDF build
(`ref-docs/`) and are kept for reproducing those artifacts, not for routine use.

### `assemble.py` does not accept an arbitrary fragment

It slices the document around two required top-level elements, in this order:

1. a `<table>`, and
2. a `<p>` whose text starts with `NOTE`.

Missing either — or a NOTE that precedes the table — is now a usage error with an
actionable message rather than a `StopIteration` traceback. `test_assemble.py`
covers each case.

### On the `ruff` nits

Both still carry `SIM115` (context managers) and `UP031` (percent-format) from
their `/tmp` originals, and the transform bodies are otherwise unmodified. Only
input validation was added on salvage, because that is additive and cannot change
the success path. The transform itself is still not covered by a test: **no
original rendered fragment survived**, so there is nothing to assert the output
against beyond "it did not crash". Clean up the lint nits the first time you run
one of these for real, with a genuine input to diff against.

An earlier version of this file claimed `bs4` was not installed here. That was
wrong, and it was half the stated reason for leaving these scripts untouched —
corrected in the PR #349 review rather than left standing.

`reflow.py` — stdlib-only and fully testable — has real coverage in
`test_reflow.py`.
