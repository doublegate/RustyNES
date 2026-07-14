#!/usr/bin/env python3
"""Improved name-join: handle ', The/A/An' article suffix, &/_ , roman<->arabic."""
import os
import re
import unicodedata
from pathlib import Path

# Repo-relative inputs (not a developer-local absolute path) so this runs from a
# fresh checkout regardless of CWD; override the full-file DB via GENIE_FULL_TSV.
_HERE = Path(__file__).resolve().parent          # scripts/gg/
_REPO = _HERE.parents[1]                          # repo root
FULL = os.environ.get(
    "GENIE_FULL_TSV",
    str(_REPO / "crates" / "rustynes-frontend" / "src" / "genie_database_full.tsv"),
)
XML = str(_HERE / "nes20db.xml")

ROMAN = {'i':'1','ii':'2','iii':'3','iv':'4','v':'5','vi':'6','vii':'7','viii':'8','ix':'9','x':'10'}

def norm(s):
    s = s.split("\\")[-1]
    s = re.sub(r"\.nes$", "", s, flags=re.I)
    s = re.sub(r"\s*\(rev\d+\)", "", s, flags=re.I)
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = s.lower()
    # move trailing ", the/a/an" to front
    m = re.search(r",\s*(the|a|an)\b(.*)$", s)
    if m:
        s = m.group(1) + " " + re.sub(r",\s*(the|a|an)\b.*$", "", s) + m.group(2)
    s = s.replace("&", "and").replace("_", "and")
    # tokenize, roman->arabic per token
    toks = re.findall(r"[a-z0-9]+", s)
    toks = [ROMAN.get(t, t) for t in toks]
    return "".join(toks)

xml = open(XML, encoding="utf-8").read()
nes20 = {}
for block in re.findall(r"<game>(.*?)</game>", xml, re.S):
    nm = re.search(r"<!--\s*(.*?)\s*-->", block)
    rc = re.search(r'<rom\b[^>]*crc32="([0-9A-Fa-f]+)"', block)
    if nm and rc:
        nes20.setdefault(norm(nm.group(1)), []).append(int(rc.group(1), 16))

games = {}
for line in open(FULL, encoding="utf-8"):
    if line.startswith("#") or not line.strip(): continue
    p = line.rstrip("\n").split("\t")
    if len(p) < 4: continue
    games.setdefault(p[1], set()).add(p[0])

matched = [g for g in games if norm(g) in nes20]
unmatched = sorted(g for g in games if norm(g) not in nes20)
print(f"distinct games: {len(games)}  matched: {len(matched)} ({100*len(matched)/len(games):.1f}%)  unmatched: {len(unmatched)}")

tot=mat=0
for line in open(FULL, encoding="utf-8"):
    if line.startswith("#") or not line.strip(): continue
    p = line.rstrip("\n").split("\t")
    if len(p) < 4: continue
    tot+=1
    if norm(p[1]) in nes20: mat+=1
print(f"cheat rows: {tot}  joinable: {mat} ({100*mat/tot:.1f}%)")
print("\nremaining unmatched (all):")
for g in unmatched: print("   ", g)
