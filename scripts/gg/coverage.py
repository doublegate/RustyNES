#!/usr/bin/env python3
"""Estimate name-join coverage: for each distinct game in genie_database_full.tsv
(No-Intro-named, full-file keyed), can we find a nes20db entry (by normalized name)
to attach a header-excluded <rom crc32>?"""
import re, unicodedata

FULL = "/home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-frontend/src/genie_database_full.tsv"
XML  = "nes20db.xml"

def norm(s):
    # strip region-path prefix, .nes suffix, unicode-fold, drop punctuation/spaces, lowercase
    s = s.split("\\")[-1]
    s = re.sub(r"\.nes$", "", s, flags=re.I)
    s = re.sub(r"\s*\(rev\d+\)", "", s, flags=re.I)
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = s.lower()
    s = re.sub(r"[^a-z0-9]", "", s)
    return s

# --- nes20db: name -> list of rom_crc (header-excluded) ---
xml = open(XML, encoding="utf-8").read()
nes20 = {}
for block in re.findall(r"<game>(.*?)</game>", xml, re.S):
    nm = re.search(r"<!--\s*(.*?)\s*-->", block)
    rc = re.search(r'<rom\b[^>]*crc32="([0-9A-Fa-f]+)"', block)
    if nm and rc:
        nes20.setdefault(norm(nm.group(1)), []).append(int(rc.group(1), 16))

# --- full-file DB: distinct game names + their full-file CRCs ---
games = {}
for line in open(FULL, encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        continue
    parts = line.rstrip("\n").split("\t")
    if len(parts) < 4:
        continue
    crc, name = parts[0], parts[1]
    games.setdefault(name, set()).add(crc)

matched = [g for g in games if norm(g) in nes20]
unmatched = [g for g in games if norm(g) not in nes20]

print(f"distinct games in full-file DB: {len(games)}")
print(f"  name-joined to nes20db:       {len(matched)} ({100*len(matched)/len(games):.1f}%)")
print(f"  UNMATCHED:                    {len(unmatched)} ({100*len(unmatched)/len(games):.1f}%)")
print("\nsample UNMATCHED (first 40):")
for g in sorted(unmatched)[:40]:
    print("   ", g)

# rows-level coverage (each cheat row inherits its game's match status)
total_rows = matched_rows = 0
for line in open(FULL, encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        continue
    parts = line.rstrip("\n").split("\t")
    if len(parts) < 4:
        continue
    total_rows += 1
    if norm(parts[1]) in nes20:
        matched_rows += 1
print(f"\ncheat rows: {total_rows}, joinable: {matched_rows} ({100*matched_rows/total_rows:.1f}%)")

# Mega Man 2 sanity
print("\nMega Man 2 nes20db header-excluded rom crcs:",
      [f"{c:08X}" for c in nes20.get(norm('Mega Man II'), [])],
      [f"{c:08X}" for c in nes20.get(norm('Mega Man 2'), [])])
