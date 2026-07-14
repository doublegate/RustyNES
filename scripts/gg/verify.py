#!/usr/bin/env python3
"""Verify nes20db provides RustyNES's header-excluded rom_crc32, two ways:
  (1) direct: nes20db <rom crc32> == curated header-excluded CRC
  (2) identity: crc32_combine(prgCRC, chrCRC, chrLen) == that CRC (CHR-RAM => prgCRC)
"""
import re
from crc_combine import crc32_combine

XML = open("nes20db.xml", encoding="utf-8").read()

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

# The 6 curated header-excluded CRCs from genie_database.tsv
targets = {
    "Super Mario Bros.": 0x3337EC46,
    "The Legend of Zelda": 0xD7AE93DF,
    "Mega Man 2":         0x0FCFC04D,
    "Metroid":            0x6562B348,
    "Castlevania":        0xB453BBE5,
    "Contra":             0x7D9E68AD,
}

print(f"Parsed {len(entries)} nes20db game entries.\n")
print(f"{'GAME':<22} {'CURATED':>9} {'nes20db<rom>':>12} {'combine(prg,chr)':>17} {'DIRECT':>7} {'IDENTITY':>9}")
allpass = True
for name, want in targets.items():
    hits = by_romcrc.get(want, [])
    if not hits:
        print(f"{name:<22} {want:08X}   NO <rom crc32> MATCH IN nes20db")
        allpass = False
        continue
    e = hits[0]
    # identity check
    if e["chr_sz"] and e["chr_crc"] is not None:
        combined = crc32_combine(e["prg_crc"], e["chr_crc"], e["chr_sz"])
    else:
        combined = e["prg_crc"]          # CHR-RAM: rom == prg
    direct_ok = (e["rom_crc"] == want)
    ident_ok = (combined == want)
    allpass &= direct_ok and ident_ok
    print(f"{name:<22} {want:08X}     {e['rom_crc']:08X}          {combined:08X}   "
          f"{'PASS' if direct_ok else 'FAIL':>7} {'PASS' if ident_ok else 'FAIL':>9}   "
          f"[{e['name']}]  prg={e['prg_sz']} chr={e['chr_sz']}")

print("\nOVERALL:", "PASS -- nes20db<rom crc32> == rom_crc32, and combine identity holds"
      if allpass else "FAIL")
