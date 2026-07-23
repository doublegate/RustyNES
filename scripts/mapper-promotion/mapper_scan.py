#!/usr/bin/env python3
import os, sys, zipfile, glob, collections

LIB = os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)")
EXT = "/home/parobek/Code/OSS_Public-Projects/RustyNES/tests/roms/external"

BEST_EFFORT = [15,28,29,30,31,35,36,39,40,42,44,46,49,50,51,52,56,57,58,60,61,62,63,72,76,77,81,90,92,94,95,96,97,101,104,107,111,112,115,120,132,133,134,136,137,138,139,141,142,143,145,146,147,148,149,150,156,162,164,174,176,177,178,179,180,185,189,193,200,201,202,203,204,205,209,211,212,213,214,218,221,225,226,227,229,231,233,234,238,242,244,245,246,250,253,261,268,286,289,290,299,301,303,305,306,312,320,336,348,349,366,513]
BE_SET = set(BEST_EFFORT)

def parse_mapper(data):
    if len(data) < 16:
        return None, "too short"
    if data[0:4] != b"NES\x1a":
        return None, "bad magic"
    h6, h7, h8 = data[6], data[7], data[8]
    mapper = (h7 & 0xF0) | (h6 >> 4)
    is_nes2 = (h7 & 0x0C) == 0x08
    if is_nes2:
        mapper |= (h8 & 0x0F) << 8
    return mapper, None

mapper_roms = collections.defaultdict(list)
failed = []
total = 0
clean = 0

for zpath in sorted(glob.glob(os.path.join(LIB, "*.zip"))):
    total += 1
    zname = os.path.basename(zpath)
    try:
        with zipfile.ZipFile(zpath) as zf:
            nes_entry = None
            for n in zf.namelist():
                if n.lower().endswith(".nes"):
                    nes_entry = n
                    break
            if nes_entry is None:
                failed.append((zname, "no .nes entry"))
                continue
            with zf.open(nes_entry) as f:
                head = f.read(16)
        mapper, err = parse_mapper(head)
        if err:
            failed.append((zname, err))
            continue
        clean += 1
        mapper_roms[mapper].append(zname)
    except Exception as e:
        failed.append((zname, f"exc: {e}"))

# Report coverable BestEffort
print("=== TOTAL zips:", total, "clean:", clean, "failed:", len(failed), "===")
print()
print("=== BestEffort coverable (id | count | examples) ===")
coverable = []
for mid in BEST_EFFORT:
    roms = mapper_roms.get(mid, [])
    if roms:
        coverable.append(mid)
        ex = " || ".join(roms[:3])
        print(f"{mid}\t{len(roms)}\t{ex}")
print()
print("=== BestEffort NOT coverable (no ROM) ===")
notcov = [m for m in BEST_EFFORT if not mapper_roms.get(m)]
print(", ".join(str(m) for m in notcov))
print()
print("=== COUNTS: coverable =", len(coverable), " notcoverable =", len(notcov), "===")
print()
print("=== FAILED zips ===")
for z, why in failed:
    print(f"  {z}: {why}")
print()

# Also dump full mapper distribution for BestEffort ids present
print("=== All mapper ids present in library (id:count) ===")
for mid in sorted(mapper_roms):
    tag = " <BE>" if mid in BE_SET else ""
    print(f"  {mid}: {len(mapper_roms[mid])}{tag}")
