#!/usr/bin/env python3
"""
Build an expanded NES Game Genie code database for RustyNES by ingesting the
openly-licensed libretro-database:

  * cheat codes  -> cht/Nintendo - Nintendo Entertainment System/*.cht
                    (only the "(Game Genie)" files with valid GG codes)
  * CRC32 keys   -> metadat/no-intro/Nintendo - Nintendo Entertainment System.dat
                    (No-Intro DAT; full-file CRC of the headered .nes dump)

Output rows: CRC32<TAB>Game<TAB>Effect<TAB>Code<TAB>Category
One row per (crc32, code) pair.  See genie_ingest_report.txt for stats.
"""

import os
import re
import sys

BASE = os.path.dirname(os.path.abspath(__file__))
CLONE = os.path.join(BASE, "libretro-database")
CHT_DIR = os.path.join(CLONE, "cht", "Nintendo - Nintendo Entertainment System")
DAT = os.path.join(CLONE, "metadat", "no-intro",
                   "Nintendo - Nintendo Entertainment System.dat")

OUT_TSV = os.path.join(BASE, "genie_ingested.tsv")
OUT_REPORT = os.path.join(BASE, "genie_ingest_report.txt")

GG_LETTERS = set("APZLGITYEOXUKSVN")


def is_valid_gg(code: str) -> bool:
    code = code.strip().upper()
    if len(code) not in (6, 8):
        return False
    return all(c in GG_LETTERS for c in code)


# ---- Category inference ---------------------------------------------------
def categorize(desc: str) -> str:
    d = desc.lower()
    # order matters: more specific first
    if any(k in d for k in ("invincib", "invuln", "immun", "no damage",
                            "can't be hurt", "cannot be hurt", "ignore you",
                            "protection", "untouchable", "can't die",
                            "cannot die", "never die", "don't get hurt")):
        return "Invincibility"
    if "lives" in d or "life" in d or re.search(r"\blive\b", d):
        return "Lives"
    if any(k in d for k in ("weapon", "gun", "fire ", "fireball", "sword",
                            "bomb", "missile", "ammo", "bullet", "shot",
                            "shoot")):
        return "Weapons"
    if any(k in d for k in ("jump", "walk", "speed", "moon", "fly", "float",
                            "run ", "climb", "swim", "high jump",
                            "super jump")):
        return "Movement"
    if any(k in d for k in ("hard", "easy", "difficult", "timer", "time ",
                            "slower", "faster")):
        return "Difficulty"
    if any(k in d for k in ("start with", "start on", "begin", "start at",
                            "start game", "starting")):
        return "Start"
    if any(k in d for k in ("star", "energy", "health", "hp", "hearts",
                            "heart", "power", "mega", "invincible star")):
        return "Items"
    if any(k in d for k in ("item", "coin", "money", "cash", "gold", "key",
                            "keys", "gem", "point")):
        return "Items"
    return "Misc"


# ---- Parse a .cht file ----------------------------------------------------
def parse_cht(path):
    """Return list of (desc, code) valid GG pairs."""
    descs = {}
    codes = {}
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = re.match(r"\s*cheat(\d+)_desc\s*=\s*\"(.*)\"", line)
            if m:
                descs[int(m.group(1))] = m.group(2).strip()
                continue
            m = re.match(r"\s*cheat(\d+)_code\s*=\s*\"(.*)\"", line)
            if m:
                codes[int(m.group(1))] = m.group(2).strip()
    out = []
    for idx in sorted(codes):
        code = codes[idx].strip().upper()
        desc = descs.get(idx, "").strip()
        # A single cheat entry may hold multiple GG codes joined by '+'.
        parts = re.split(r"[+]", code)
        for p in parts:
            p = p.strip()
            if is_valid_gg(p):
                out.append((desc if desc else "Cheat", p.upper()))
    return out


# ---- Clean a game title ---------------------------------------------------
PAREN_RE = re.compile(r"\s*\([^)]*\)")


def clean_title(name: str) -> str:
    # strip all parenthetical groups, collapse whitespace
    t = PAREN_RE.sub("", name).strip()
    return re.sub(r"\s+", " ", t)


def norm_key(name: str) -> str:
    """
    Match key: clean title (region/rev/etc. parentheticals removed) reduced to
    lowercase alphanumerics only. This absorbs cosmetic differences between the
    cht filename and the DAT name -- region ordering ("(USA, Japan)" vs
    "(Japan, USA)"), the filesystem '&'->'_' sanitisation, punctuation, and
    spacing -- so a cht joins ALL of a game's USA/World dumps (every revision).
    """
    return re.sub(r"[^0-9a-z]", "", clean_title(name).lower())


# ---- Parse No-Intro DAT ---------------------------------------------------
def parse_dat(path):
    """
    Return list of (name, crc) for every .nes rom (full-file, headered CRC).
    """
    entries = []
    cur_name = None
    game_re = re.compile(r'^\s*name\s+"(.*)"\s*$')
    rom_re = re.compile(r'^\s*rom\s*\(\s*name\s+"([^"]*)"\s+size\s+\d+\s+crc\s+([0-9A-Fa-f]{8})')
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        in_game = False
        for line in fh:
            s = line.rstrip("\n")
            st = s.strip()
            if st.startswith("game ("):
                in_game = True
                cur_name = None
                continue
            if in_game:
                gm = game_re.match(s)
                if gm and cur_name is None:
                    cur_name = gm.group(1)
                    continue
                rm = rom_re.match(s)
                if rm:
                    romname = rm.group(1)
                    crc = rm.group(2).upper()
                    if romname.lower().endswith(".nes"):
                        entries.append((cur_name if cur_name else romname, crc))
                    continue
                if st == ")":
                    in_game = False
    return entries


PAREN_GROUP_RE = re.compile(r"\(([^)]*)\)")


def usa_world_ok(name: str) -> bool:
    """
    Include a dump iff a parenthetical region group carries the USA or World
    token (region order-independent: "(USA, Japan)" and "(Japan, USA)" both
    qualify), and exclude Beta/Proto/Demo/Sample/Pirate dumps. Region tokens
    are matched only inside parentheticals, so a title word like "World" never
    mis-qualifies a Japan-only game.
    """
    groups = [g.lower() for g in PAREN_GROUP_RE.findall(name)]
    has_region = False
    for g in groups:
        toks = re.split(r"[^a-z]+", g)
        if "usa" in toks or "world" in toks:
            has_region = True
        if any(b in toks for b in ("beta", "proto", "demo", "sample",
                                   "pirate")):
            return False
    return has_region


def main():
    if not os.path.isdir(CHT_DIR):
        sys.exit("cht dir missing: " + CHT_DIR)
    if not os.path.isfile(DAT):
        sys.exit("dat missing: " + DAT)

    dat_entries = parse_dat(DAT)  # list of (fullname, crc)

    # Index USA/World .nes dumps by normalised clean-title key.
    dat_index = {}  # normkey -> list of (fullname, crc)
    for (nm, crc) in dat_entries:
        if not usa_world_ok(nm):
            continue
        dat_index.setdefault(norm_key(nm), []).append((nm, crc))

    # Collect GG cht files
    cht_files = [f for f in os.listdir(CHT_DIR)
                 if f.endswith("(Game Genie).cht")]
    cht_files.sort()

    rows = set()  # (crc, game, effect, code, category)
    games_with_codes = set()
    games_matched = set()
    dumps_matched = set()
    unmatched_games = []

    for fname in cht_files:
        # base name before " (Game Genie).cht"
        base = fname[:-len(" (Game Genie).cht")]  # e.g. "Contra (USA)"
        pairs = parse_cht(os.path.join(CHT_DIR, fname))
        if not pairs:
            continue
        games_with_codes.add(base)

        # Join by normalised clean title -> all USA/World dumps (every revision).
        matched_crcs = dat_index.get(norm_key(base), [])
        if not matched_crcs:
            unmatched_games.append(base)
            continue

        title = clean_title(base)
        games_matched.add(base)
        for (nm, crc) in matched_crcs:
            dumps_matched.add(crc)
            for (desc, code) in pairs:
                cat = categorize(desc)
                rows.add((crc, title, desc, code, cat))

    # Sort by Game, Category, Code, then CRC for stability
    ordered = sorted(rows, key=lambda r: (r[1].lower(), r[4], r[3], r[0]))

    with open(OUT_TSV, "w", encoding="utf-8") as fh:
        for (crc, game, effect, code, cat) in ordered:
            fh.write(f"{crc}\t{game}\t{effect}\t{code}\t{cat}\n")

    # Report
    lines = []
    lines.append("RustyNES Game Genie DB ingest report")
    lines.append("=" * 44)
    lines.append("")
    lines.append("Clone URL:  https://github.com/libretro/libretro-database.git (shallow)")
    lines.append("CRC source: metadat/no-intro/Nintendo - Nintendo Entertainment System.dat")
    lines.append("            (No-Intro DAT; full-file CRC32 of the headered .nes dump)")
    lines.append("Cheat src:  cht/Nintendo - Nintendo Entertainment System/*(Game Genie).cht")
    lines.append("")
    lines.append(f"GG cht files (with >=1 valid code): {len(games_with_codes)}")
    lines.append(f"Distinct games matched to a CRC:    {len(games_matched)}")
    lines.append(f"Distinct USA/World dumps matched:   {len(dumps_matched)}")
    lines.append(f"Total rows emitted:                 {len(ordered)}")
    lines.append(f"Games with NO CRC match:            {len(unmatched_games)}")
    lines.append("")
    # Split unmatched: those the cht itself tags USA/World are the actionable
    # misses; the rest are Japan/Europe-only cht files with no USA/World dump
    # in the DAT (correctly excluded per the USA+World restriction).
    claim = [g for g in unmatched_games if usa_world_ok(g)]
    nonclaim = [g for g in unmatched_games if not usa_world_ok(g)]
    lines.append(f"  of which JP/EU-only (no USA/World dump exists; expected): {len(nonclaim)}")
    lines.append(f"  of which tagged USA/World but still unmatched:           {len(claim)}")
    lines.append("")
    lines.append("USA/World-tagged unmatched (up to 20) -- mostly Proto/Beta-")
    lines.append("only US dumps (excluded) or subtitle spelling mismatches:")
    for g in claim[:20]:
        lines.append(f"  - {g}")
    lines.append("")
    lines.append("10 sample rows:")
    for r in ordered[:10]:
        lines.append("  " + "\t".join(r))
    with open(OUT_REPORT, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")

    print("\n".join(lines))


if __name__ == "__main__":
    main()
