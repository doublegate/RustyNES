#!/usr/bin/env python3
import os, sys, zipfile, re

DIRS = [
    os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/GoodNES [v3.23b]/"),
    os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/Homebrew & Unlicensed/"),
    os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/NES-on-a-chip/"),
]

TARGETS = {29,35,39,42,44,46,49,50,51,52,56,57,81,90,104,115,120,134,136,138,139,141,142,164,174,176,179,189,193,204,205,209,211,221,238,245,253,261,268,286,289,290,299,301,303,305,306,312,320,336,348,349,366,513}

def mapper_from_header(h):
    if len(h) < 16 or h[0:4] != b"NES\x1a":
        return None
    mapper = (h[7] & 0xF0) | (h[6] >> 4)
    if (h[7] & 0x0C) == 0x08:
        mapper |= (h[8] & 0x0F) << 8
    return mapper

def get_header(path):
    try:
        if path.lower().endswith(".zip"):
            with zipfile.ZipFile(path) as z:
                names = [n for n in z.namelist() if n.lower().endswith(".nes")]
                if not names:
                    return None
                with z.open(names[0]) as f:
                    return f.read(16)
        else:
            with open(path, "rb") as f:
                return f.read(16)
    except Exception:
        return None

# scoring for best candidate
def score(name):
    n = name
    if "[!]" in n: return 5
    # bad dumps
    if re.search(r"\[b\d*\]", n): return 0
    # hacks
    if re.search(r"\[h\d*\]", n): return 2
    # alternate / pirate
    if re.search(r"\[a\d*\]", n) or re.search(r"\[p\d*\]", n): return 3
    return 4  # no-suffix ok

matches = {t: [] for t in TARGETS}
total_files = 0
errors = 0

for d in DIRS:
    for root, _, files in os.walk(d):
        for fn in files:
            low = fn.lower()
            if not (low.endswith(".nes") or low.endswith(".zip")):
                continue
            total_files += 1
            p = os.path.join(root, fn)
            h = get_header(p)
            if h is None:
                errors += 1
                continue
            m = mapper_from_header(h)
            if m in TARGETS:
                matches[m].append((score(fn), fn, p))

print(f"# scanned {total_files} files, {errors} unreadable/no-header", file=sys.stderr)

covered = []
zero = []
for t in sorted(TARGETS):
    lst = matches[t]
    if not lst:
        zero.append(t)
        continue
    lst.sort(key=lambda x: (-x[0], x[1]))
    best = lst[0]
    flag = " [ONLY-BAD-DUMP]" if best[0] == 0 else ""
    covered.append((t, len(lst), best[2], flag))

print("ID\tCOUNT\tBESTSCORE_FLAG\tPATH")
for t, cnt, path, flag in covered:
    print(f"{t}\t{cnt}\t{flag}\t{path}")

print("\nZERO_MATCHES:", ",".join(str(z) for z in zero))
print(f"COVERABLE: {len(covered)} / {len(TARGETS)}")
