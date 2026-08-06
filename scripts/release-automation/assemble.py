#!/usr/bin/env python3
import sys

from bs4 import BeautifulSoup, NavigableString

frag_path, out_path = sys.argv[1], sys.argv[2]
html = open(frag_path, encoding="utf-8").read()
soup = BeautifulSoup(html, "html.parser")

# --- RED for curated genuine takeaways (exact <strong> text contains) ---
KEY = [
    "more serious LLM error",
    "did not follow it",
    "AI self-attestation of license compliance is not trustworthy",
    "single most important piece of evidence",
    "Correct.",
    "not written purely from hardware documentation",
]
for s in soup.find_all("strong"):
    t = s.get_text()
    if any(k in t for k in KEY):
        cls = s.get("class", [])
        s["class"] = cls + ["key"]

# --- top-level elements, in order ---
els = [c for c in soup.contents if getattr(c, "name", None)]
table_i = next(i for i, e in enumerate(els) if e.name == "table")
note_i = next(i for i, e in enumerate(els)
              if e.name == "p" and e.get_text().lstrip().startswith("NOTE"))

before = "".join(str(e) for e in els[:table_i])
table = str(els[table_i])
middle = "".join(str(e) for e in els[table_i + 1:note_i])

# --- italicize everything after "): " in the NOTE paragraph (PDF only) ---
note_p = els[note_i]
MARKER = "): "
kids = list(note_p.children)
head, tail_nodes, splitting = [], [], True
for k in kids:
    if splitting and isinstance(k, NavigableString) and MARKER in k:
        pre, post = str(k).split(MARKER, 1)
        head.append(NavigableString(pre + MARKER))
        if post:
            tail_nodes.append(NavigableString(post))
        splitting = False
    elif splitting:
        head.append(k)
    else:
        tail_nodes.append(k.extract() if hasattr(k, "extract") else k)
if tail_nodes:  # only rebuild if the marker was found
    note_p.clear()
    for h in head:
        note_p.append(h)
    em = soup.new_tag("em")
    for t in tail_nodes:
        em.append(t)
    # Bold + upright "Fiskbit" inside the italic note body (PDF styling only)
    for tnode in list(em.find_all(string=True)):
        if "Fiskbit" in tnode:
            parts = str(tnode).split("Fiskbit")
            repl = []
            for i, seg in enumerate(parts):
                if i > 0:
                    st = soup.new_tag("strong")
                    st["class"] = ["upright"]
                    st.string = "Fiskbit"
                    repl.append(st)
                if seg:
                    repl.append(NavigableString(seg))
            tnode.replace_with(*repl)
            break
    note_p.append(em)
note = str(note_p)

TITLE = """<div class="titlewrap">
  <p class="eyebrow">RUSTYNES &#183; INCIDENT RECORD &#183; GPL PROVENANCE</p>
  <h1 class="title">Provenance Failure Post-Mortem</h1>
  <p class="subtitle">How GPL Emulator Code Was Lifted Despite a Black-Box Instruction</p>
  <p class="docdate">Forensic Root-Cause Analysis &#183; 2026-08-04 &#183; RustyNES v2.2.9</p>
</div>"""

body = (TITLE
        + '<div class="cols">' + before + '</div>'
        + table
        + '<div class="cols">' + middle + '</div>'
        + '<div class="note-box">' + note + '</div>')

doc = ('<!doctype html><html lang="en"><head><meta charset="utf-8">'
       '<title>Provenance Failure Post-Mortem</title></head><body>'
       + body + '</body></html>')

open(out_path, "w", encoding="utf-8").write(doc)
print("assembled ->", out_path,
      "| before/middle/note split at table_i=%d note_i=%d" % (table_i, note_i),
      "| reddened %d strong tags" % len(soup.select("strong.key")))
