# Release-notes / doc rendering helpers

Three one-off tools rescued from `/tmp` before a reboot (2026-08-06). They were
written during the v2.2.5–v2.3.0 documentation work, existed **nowhere else on
disk**, and would have been lost. Recorded here so they are findable rather than
rediscovered.

| script | needs | what it does |
| --- | --- | --- |
| `reflow.py` | stdlib only | Unwraps hard-wrapped markdown into single full-width lines. |
| `assemble.py` | `bs4` | Assembles a rendered HTML fragment into a full document. |
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

`assemble.py` and `guardrails_assemble.py` take a rendered HTML fragment and
produce a standalone document. They need BeautifulSoup, which is **not** a
project dependency — install it in a throwaway venv rather than adding it to the
repo:

```bash
python3 -m venv /tmp/venv && /tmp/venv/bin/pip install beautifulsoup4
/tmp/venv/bin/python scripts/release-automation/assemble.py frag.html out.html
```

Both are specific to the one-time provenance/guardrails PDF build
(`ref-docs/`) and are kept for reproducing those artifacts, not for routine use.

**They are preserved verbatim and are not ruff-clean** (`SIM115` context
managers, `UP031` percent-format). That is deliberate: `bs4` is not installed
here and no sample fragment survives, so a lint rewrite could not be executed to
prove it still behaved. Rewriting code you cannot run is a worse trade than a
style nit. Clean them up the first time you actually need them, with a real
input to test against. `reflow.py` — which *is* testable, being stdlib-only — was
fixed and verified.
