#!/usr/bin/env python3
"""Assemble the guardrails HTML: inject a title block, redden a curated
set of hard takeaways. Single-column theme handles the rest of the flow."""
import sys

from bs4 import BeautifulSoup

# Argument check added on salvage (PR #349 review). Everything BELOW this
# point is the original /tmp source, unmodified: a usage message is additive and
# cannot change the success path, whereas rewriting the parsing logic could —
# and `bs4` is not installed here and no sample fragment survived, so a rewrite
# could not be run to prove it still behaved.
if len(sys.argv) != 3:
    raise SystemExit(f"usage: {sys.argv[0]} INPUT_FRAGMENT OUTPUT_HTML")
frag_path, out_path = sys.argv[1], sys.argv[2]
html = open(frag_path, encoding="utf-8").read()
soup = BeautifulSoup(html, "html.parser")

# --- RED reserved for a few genuine, full-span takeaways ---
KEY = [
    "capability plus availability plus an accuracy objective",
    "C used as if it were A",
    "source physically unavailable to the agent",
    "Never launder",
]
reddened = 0
for s in soup.find_all("strong"):
    t = s.get_text()
    if any(k in t for k in KEY):
        s["class"] = s.get("class", []) + ["key"]
        reddened += 1

body = "".join(str(c) for c in soup.contents)

TITLE = """<div class="titlewrap">
  <p class="eyebrow">COMMUNITY BEST-GUIDANCE &#183; AI-ASSISTED EMULATOR DEVELOPMENT</p>
  <h1 class="title">Provenance &amp; License Guardrails</h1>
  <p class="subtitle">A ready-to-ingest ruleset for Claude Code and other agentic / AI-assisted development tools</p>
  <p class="docdate">Preventing the copyleft-source-lifting trap &#183; 2026-08-04</p>
</div>"""

doc = ('<!doctype html><html lang="en"><head><meta charset="utf-8">'
       '<title>Provenance &amp; License Guardrails</title></head><body>'
       + TITLE + body + '</body></html>')

open(out_path, "w", encoding="utf-8").write(doc)
print("assembled ->", out_path, "| reddened %d strong tags" % reddened)
