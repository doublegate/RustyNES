#!/usr/bin/env python3
"""Generate genie_database_headerless.tsv — the libretro Game Genie codes
re-keyed from full-file No-Intro CRCs to the header-EXCLUDED `rom_crc32`
(what RustyNES's game_db::rom_crc32 computes), via nes20db's <rom crc32>.

This is what gives the Cheats panel header-INSENSITIVE matching for all ~520
games (a re-headered dump still matches, because the header-excluded CRC only
depends on PRG + CHR content). See `crates/rustynes-frontend/src/genie_db.rs`.

Usage (from anywhere; paths are derived from this file's location):

    # 1. Fetch the NES 2.0 database (a BUILD-TIME input — do NOT commit it):
    curl -L -o scripts/gg/nes20db.xml \\
      https://raw.githubusercontent.com/Kreeblah/NES20Tool/master/nes20db/nes20db.xml
    # 2. Regenerate:
    python3 scripts/gg/gen_headerless_genie_db.py

Inputs (read-only): the committed genie_database_full.tsv (cheat content) +
nes20db.xml (game name -> header-excluded <rom crc32>; NewRisingSun's NES 2.0
DB) + alias_crcs.py (manual name aliases for the long-tail title variants).
Output: the committed crates/rustynes-frontend/src/genie_database_headerless.tsv.

nes20db is a BUILD-TIME input only and is NEVER committed; only the factual
CRC32 + Game Genie code data is emitted, matching the project's data policy.
"""
import os
import re
import gzip
import sys
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
FULL = os.path.join(REPO, "crates/rustynes-frontend/src/genie_database_full.tsv")
XML = os.path.join(HERE, "nes20db.xml")
OUT = os.path.join(REPO, "crates/rustynes-frontend/src/genie_database_headerless.tsv")

if not os.path.exists(XML):
    sys.exit(
        f"nes20db.xml not found at {XML}\n"
        "Fetch it first (see the module docstring) — it is a build-time input "
        "and is intentionally not committed."
    )

# Manual name aliases (cheat-DB game name -> [header-excluded CRC hex...]) for
# the ~45 long-tail titles that fail the automatic normalized-name join.
sys.path.insert(0, HERE)
try:
    from alias_crcs import ALIAS_CRCS
except ImportError:
    # Only the file being genuinely absent is tolerated (degraded run without the
    # long-tail aliases). A syntax/other error in a present alias_crcs.py must NOT
    # be swallowed into a silently-incomplete TSV — let it propagate.
    print("warning: alias_crcs.py not found — running without the alias table")
    ALIAS_CRCS = {}

ROMAN = {"i": "1", "ii": "2", "iii": "3", "iv": "4", "v": "5",
         "vi": "6", "vii": "7", "viii": "8", "ix": "9", "x": "10"}


def norm(s):
    s = re.split(r"[\\/]", s)[-1]
    s = re.sub(r"\.nes$", "", s, flags=re.I)
    s = re.sub(r"\s*\(rev\d+\)", "", s, flags=re.I)
    s = unicodedata.normalize("NFKD", s)
    s = "".join(c for c in s if not unicodedata.combining(c))
    s = s.lower()
    m = re.search(r",\s*(the|a|an)\b(.*)$", s)
    if m:
        s = m.group(1) + " " + re.sub(r",\s*(the|a|an)\b.*$", "", s) + m.group(2)
    s = s.replace("&", "and").replace("_", "and")
    toks = re.findall(r"[a-z0-9]+", s)
    toks = [ROMAN.get(t, t) for t in toks]
    return "".join(toks)


# 1. nes20db: normalized name -> set of header-excluded <rom crc32>. Each <game>
# block has one <rom> today, but findall is used so multiple would all be picked
# up (the `<rom\b` anchor excludes <prgrom>/<chrrom>).
with open(XML, encoding="utf-8") as f:
    xml = f.read()
nes20 = {}
for block in re.findall(r"<game>(.*?)</game>", xml, re.S):
    nm = re.search(r"<!--\s*(.*?)\s*-->", block)
    rcs = re.findall(r'<rom\b[^>]*crc32="([0-9A-Fa-f]+)"', block)
    if nm and rcs:
        for rc in rcs:
            nes20.setdefault(norm(nm.group(1)), set()).add(int(rc, 16))

# 2. cheat content: game display name -> ordered distinct (effect, code, category)
games = {}
order = []
with open(FULL, encoding="utf-8") as f:
    for line in f:
        if line.startswith("#") or not line.strip():
            continue
        p = line.rstrip("\n").split("\t")
        if len(p) < 4:
            continue
        _crc, game, effect, code = p[0], p[1], p[2], p[3]
        category = p[4] if len(p) > 4 and p[4] else "Misc"
        if game not in games:
            games[game] = []
            order.append(game)
        key = (effect, code, category)
        if key not in games[game]:
            games[game].append(key)

# 3. resolve each game's header-excluded CRC set (name-join + alias fallback)
rows = []
covered_games = 0
covered_via_alias = 0
uncovered = []
for game in order:
    crcs = set(nes20.get(norm(game), set()))
    if game in ALIAS_CRCS:
        before = len(crcs)
        crcs |= {int(c, 16) for c in ALIAS_CRCS[game]}
        if before == 0 and crcs:
            covered_via_alias += 1
    if not crcs:
        uncovered.append(game)
        continue
    covered_games += 1
    for crc in sorted(crcs):
        for (effect, code, category) in games[game]:
            rows.append((crc, game, effect, code, category))

# 4. dedup (same header-excluded CRC can appear for name-variant collisions)
seen = set()
uniq = []
for r in rows:
    k = (r[0], r[3])  # (crc, code) — a code is unique per game/crc
    if k not in seen:
        seen.add(k)
        uniq.append(r)
uniq.sort(key=lambda r: (r[0], r[1], r[4], r[2]))

# 5. write with provenance header
HEADER = """\
# RustyNES Game Genie code database - header-EXCLUDED re-key (v2.1.3).
#
# The same libretro-database Game Genie codes as genie_database_full.tsv, but
# RE-KEYED from the full-file No-Intro CRC to the HEADER-EXCLUDED rom_crc32 (the
# CRC of PRG-ROM + CHR-ROM with the 16-byte iNES header and any trainer stripped)
# - exactly what `crate::game_db::rom_crc32` computes for a loaded ROM. This makes
# the nomination match a game even when the user's .nes has a NON-STANDARD iNES
# header (common with re-headered dumps), which the header-sensitive full-file key
# cannot. The header-excluded CRCs come from the NES 2.0 database (nes20db.xml,
# NewRisingSun) <rom crc32> field, joined to the cheat catalog by normalized game
# name (+ a manual alias table for long-tail title variants). Every known
# region/revision CRC per game is emitted, so any dump flavor matches; runtime
# dedups by code. nes20db is a BUILD-TIME input only and is NEVER committed - only
# the factual CRC32 + Game Genie code data is, matching the project's data policy.
# Commercial ROMs are NEVER committed.
#
# Format (tab-separated): CRC32<TAB>Game<TAB>Effect<TAB>Code<TAB>Category.
# CRC32 here is the HEADER-EXCLUDED rom_crc32 (distinct from the full-file key of
# genie_database_full.tsv). Codes re-validate through GenieCode::new at load.
#
"""
with open(OUT, "w", encoding="utf-8") as f:
    f.write(HEADER)
    for (crc, game, effect, code, category) in uniq:
        f.write(f"{crc:08X}\t{game}\t{effect}\t{code}\t{category}\n")

with open(OUT, "rb") as f:
    raw = f.read()
gz = len(gzip.compress(raw))
print(f"games total:        {len(order)}")
print(f"games covered:      {covered_games}  (via alias: {covered_via_alias})")
print(f"games UNCOVERED:    {len(uncovered)}")
print(f"output rows:        {len(uniq)}")
print(f"output raw:         {len(raw)} bytes ({len(raw)/1024:.0f} KiB)")
print(f"output gzip:        {gz} bytes ({gz/1024:.0f} KiB)")
if uncovered:
    print("\nUNCOVERED games:")
    for g in uncovered:
        print("   ", g)
