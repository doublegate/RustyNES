#!/usr/bin/env python3
"""Enumerate staged BestEffort mapper ROMs, verify header mapper, pick one per mapper."""
import os, re, zipfile, io, sys

EXT = "/home/parobek/Code/OSS_Public-Projects/RustyNES/tests/roms/external"
BEST_EFFORT = [15,28,29,30,31,35,36,39,40,42,44,46,49,50,51,52,56,57,58,60,61,62,63,72,76,77,81,90,92,94,95,96,97,101,104,107,111,112,115,120,132,133,134,136,137,138,139,141,142,143,145,146,147,148,149,150,156,162,164,174,176,177,178,179,180,185,189,193,200,201,202,203,204,205,209,211,212,213,214,218,221,225,226,227,229,231,233,234,238,242,244,245,246,250,253,261,268,286,289,290,299,301,303,305,306,312,320,336,348,349,366,513]

def ines_mapper(b):
    if len(b) < 16 or b[0:4] != b"NES\x1a":
        return None
    lo = b[6] >> 4; hi = b[7] & 0xF0
    if (b[7] & 0x0C) == 0x08:
        return hi | lo | ((b[8] & 0x0F) << 8)
    return hi | lo

def rom_bytes(path):
    if path.endswith(".zip"):
        try:
            z = zipfile.ZipFile(path)
            for n in z.namelist():
                if n.lower().endswith((".nes",)):
                    return z.read(n)
        except Exception:
            return None
        return None
    with open(path, "rb") as f:
        return f.read()

def snake(s):
    s = re.sub(r"\.(nes|zip)$", "", s, flags=re.I)
    s = re.sub(r"\(.*?\)", "", s)  # drop (USA) etc
    s = re.sub(r"[^A-Za-z0-9]+", "_", s).strip("_").lower()
    return re.sub(r"_+", "_", s)

# map id -> dir
dirs = {}
for d in sorted(os.listdir(EXT)):
    m = re.match(r"mapper-(\d+)-", d)
    if m:
        dirs[int(m.group(1))] = d

rows = []
mismatch = []
missing = []
for mid in BEST_EFFORT:
    d = dirs.get(mid)
    if not d:
        missing.append(mid); continue
    full = os.path.join(EXT, d)
    roms = sorted([f for f in os.listdir(full) if f.lower().endswith((".nes", ".zip"))])
    if not roms:
        missing.append(mid); continue
    # pick first ROM whose header mapper == mid; else first
    chosen = None; chosen_ok = False
    for r in roms:
        b = rom_bytes(os.path.join(full, r))
        hm = ines_mapper(b) if b else None
        if hm == mid:
            chosen = r; chosen_ok = True; break
    if not chosen:
        chosen = roms[0]
        b = rom_bytes(os.path.join(full, chosen))
        hm = ines_mapper(b) if b else None
        mismatch.append((mid, d, chosen, hm))
    rows.append((mid, d, chosen))

print("=== PROMOTABLE (staged, header verified) ===", len(rows))
for mid, d, r in rows:
    print(f"{mid}\t{d}/{r}\textended_m{mid}_{snake(r)}")
print("\n=== MISSING (no staged dir or empty) ===", len(missing))
print(sorted(missing))
print("\n=== HEADER MISMATCH (chosen ROM's mapper != dir id) ===", len(mismatch))
for row in mismatch:
    print(row)

promoted_ids = sorted([r[0] for r in rows])
remaining_be = sorted(set(BEST_EFFORT) - set(promoted_ids))
print("\n=== NEW CURATED additions (sorted) ===")
print(promoted_ids)
print("\n=== REMAINING BestEffort (sorted) ===")
print(remaining_be)
print(f"\ncounts: promote={len(promoted_ids)} remaining_be={len(remaining_be)} total={len(BEST_EFFORT)}")
