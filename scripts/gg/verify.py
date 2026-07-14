#!/usr/bin/env python3
"""Verify nes20db supplies RustyNES's header-excluded rom_crc32, two ways:
  (1) direct:   nes20db <rom crc32> == the header-EXCLUDED CRC the shipped
                headerless catalog keys on (crates/.../genie_database_headerless.tsv)
  (2) identity: crc32_combine(prgCRC, chrCRC, chrLen) == that CRC (CHR-RAM => prgCRC)

The `targets` below are the header-EXCLUDED CRCs. They are deliberately NOT the
values from the 6-game curated `genie_database.tsv`: those were mislabeled
full-file (header-INCLUDED) CRCs (e.g. SMB's 0x3337EC46), which is exactly the
defect this tooling uncovered and would produce false FAILs against nes20db's
content-only <rom crc32>. The correct header-excluded keys are taken from the
shipped headerless catalog (SMB 0x9A2DB086, Contra 0x43FB36BD, ...).
"""
import re
from pathlib import Path

from crc_combine import crc32_combine

# Resolve nes20db.xml next to this script so it is found regardless of CWD.
XML = (Path(__file__).resolve().parent / "nes20db.xml").read_text(encoding="utf-8")

# Split into <game>...</game> blocks
games = re.findall(r"<game>(.*?)</game>", XML, re.S)

def attr(block, tag, name):
    m = re.search(rf"<{tag}\b[^>]*\b{name}=\"([0-9A-Fa-f]+)\"", block)
    return m.group(1) if m else None

def parse(block):
    name = re.search(r"<!--\s*(.*?)\s*-->", block)
    prg_crc = attr(block, "prgrom", "crc32")
    prg_sz  = attr(block, "prgrom", "size")
    chr_crc = attr(block, "chrrom", "crc32")
    chr_sz  = attr(block, "chrrom", "size")
    rom_crc = attr(block, "rom", "crc32")
    rom_sz  = attr(block, "rom", "size")
    return {
        "name": name.group(1) if name else "?",
        "prg_crc": int(prg_crc, 16) if prg_crc else None,
        "prg_sz": int(prg_sz) if prg_sz else None,
        "chr_crc": int(chr_crc, 16) if chr_crc else None,
        "chr_sz": int(chr_sz) if chr_sz else 0,   # 0 => CHR-RAM
        "rom_crc": int(rom_crc, 16) if rom_crc else None,
        "rom_sz": int(rom_sz) if rom_sz else None,
    }

entries = [parse(g) for g in games]
by_romcrc = {}
for e in entries:
    by_romcrc.setdefault(e["rom_crc"], []).append(e)

# The 6 classics' header-EXCLUDED CRCs, as keyed by the shipped headerless
# catalog (crates/rustynes-frontend/src/genie_database_headerless.tsv). These are
# nes20db <rom crc32> content-only values -- NOT the mislabeled full-file CRCs the
# old curated genie_database.tsv carried.
targets = {
    "Super Mario Bros.":   0x9A2DB086,
    "The Legend of Zelda": 0x34540318,
    "Mega Man 2":          0x0FCFC04D,
    "Metroid":             0x6CF3116A,
    "Castlevania":         0x0AC1AA8F,
    "Contra":              0x43FB36BD,
}

print(f"Parsed {len(entries)} nes20db game entries.\n")


def is_standard_cart(e):
    """True for a plain cart dump whose ROM bytes are exactly PRG++CHR — i.e. the
    combine identity is expected to hold. Excludes PlayChoice / Vs. dumps that
    carry extra ROM sections, for which <rom crc32> covers more than PRG++CHR."""
    if e["prg_crc"] is None or e["rom_crc"] is None or e["prg_sz"] is None:
        return False
    return e["rom_sz"] == e["prg_sz"] + (e["chr_sz"] or 0)


def combine_of(e):
    if e["chr_sz"] and e["chr_crc"] is not None:
        return crc32_combine(e["prg_crc"], e["chr_crc"], e["chr_sz"])
    return e["prg_crc"]                       # CHR-RAM: rom == prg


# (1) The invariant the whole re-key rests on: for EVERY standard cart dump,
#     crc32_combine(prg, chr) == the content-only <rom crc32>. Gate OVERALL here
#     (thousands of dumps), not on any single hand-picked CRC.
checked = passed = 0
mismatches = []
for e in entries:
    if not is_standard_cart(e):
        continue
    checked += 1
    if combine_of(e) == e["rom_crc"]:
        passed += 1
    else:
        mismatches.append(e)
print(f"combine identity over standard cart dumps: {passed}/{checked} "
      f"crc32_combine(prg,chr) == <rom crc32>")
for e in mismatches[:5]:
    print(f"  MISMATCH  {e['name']}: prg={e['prg_crc']:08X} rom={e['rom_crc']:08X}")

# (2) Informational spot-check: each classic's header-excluded key is present in
#     nes20db. IDENTITY is 'n/a' when the matched dump is multi-section (e.g. the
#     PlayChoice Metroid), where rom != prg++chr by construction.
print(f"\n{'GAME':<22} {'HDR-EXCL':>9} {'nes20db<rom>':>12} {'DIRECT':>7} {'IDENTITY':>9}   dump")
for name, want in targets.items():
    hits = by_romcrc.get(want, [])
    if not hits:
        print(f"{name:<22} {want:08X}   NOT FOUND in nes20db <rom crc32>")
        continue
    e = hits[0]
    if not is_standard_cart(e):
        ident = "n/a"
    else:
        ident = "PASS" if combine_of(e) == want else "FAIL"
    print(f"{name:<22} {want:08X}     {e['rom_crc']:08X}   "
          f"{'PASS':>7} {ident:>9}   [{e['name']}]  prg={e['prg_sz']} chr={e['chr_sz']}")

allpass = (not mismatches) and all(by_romcrc.get(w) for w in targets.values())
print("\nOVERALL:",
      "PASS -- combine identity holds for every standard dump, and all 6 "
      "header-excluded keys are present in nes20db" if allpass else "FAIL")
