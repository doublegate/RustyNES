#!/usr/bin/env python3
"""Generate external_extended.rs test blocks + new tier id-lists for the 58 promotions."""
import os, re, zipfile

EXT = "/home/parobek/Code/OSS_Public-Projects/RustyNES/tests/roms/external"
PROMOTE = [15,28,30,31,36,40,58,60,61,62,63,72,76,77,92,94,95,96,97,101,107,111,112,132,133,137,143,145,146,147,148,149,150,156,162,177,178,180,185,200,201,202,203,212,213,214,218,225,226,227,229,231,233,234,242,244,246,250]
EXISTING_CURATED = [38, 41, 79, 86, 113, 140, 232, 240, 241]
ALL_BEST_EFFORT = [15,28,29,30,31,35,36,39,40,42,44,46,49,50,51,52,56,57,58,60,61,62,63,72,76,77,81,90,92,94,95,96,97,101,104,107,111,112,115,120,132,133,134,136,137,138,139,141,142,143,145,146,147,148,149,150,156,162,164,174,176,177,178,179,180,185,189,193,200,201,202,203,204,205,209,211,212,213,214,218,221,225,226,227,229,231,233,234,238,242,244,245,246,250,253,261,268,286,289,290,299,301,303,305,306,312,320,336,348,349,366,513]

def ines_mapper(b):
    if len(b) < 16 or b[0:4] != b"NES\x1a": return None
    lo=b[6]>>4; hi=b[7]&0xF0
    return (hi|lo|((b[8]&0x0F)<<8)) if (b[7]&0x0C)==0x08 else hi|lo
def rom_bytes(p):
    if p.endswith(".zip"):
        try:
            z=zipfile.ZipFile(p)
            for n in z.namelist():
                if n.lower().endswith(".nes"): return z.read(n)
        except: return None
        return None
    return open(p,"rb").read()
def snake(s):
    s=re.sub(r"\.(nes|zip)$","",s,flags=re.I); s=re.sub(r"\(.*?\)","",s)
    s=re.sub(r"[^A-Za-z0-9]+","_",s).strip("_").lower(); return re.sub(r"_+","_",s)

dirs={}
for d in sorted(os.listdir(EXT)):
    m=re.match(r"mapper-(\d+)-",d)
    if m: dirs[int(m.group(1))]=d

blocks=[]
for mid in PROMOTE:
    d=dirs[mid]; full=os.path.join(EXT,d)
    roms=sorted([f for f in os.listdir(full) if f.lower().endswith((".nes",".zip"))])
    chosen=None
    for r in roms:
        b=rom_bytes(os.path.join(full,r))
        if b and ines_mapper(b)==mid: chosen=r; break
    if not chosen: chosen=roms[0]
    name=f"extended_m{mid}_{snake(chosen)}"
    rel=f"{d}/{chosen}"
    blocks.append((mid,name,rel))

# emit rust test file section
out=[]
out.append("\n// ============================================================")
out.append("// v2.1.0 \"Fathom\" F3 — BestEffort -> Curated promotions.")
out.append("// One representative staged commercial ROM per mapper, locked as a")
out.append("// byte-identity boot-output snapshot (600-frame idle). Promoting these")
out.append("// mappers to Curated (see rustynes-mappers::tier) is gated on this")
out.append("// oracle evidence per ADR 0011.")
out.append("// ============================================================\n")
for mid,name,rel in blocks:
    rel_esc=rel.replace('"','\\"')
    out.append(f"#[test]\nfn {name}() {{\n    check(\n        \"{rel_esc}\",\n        DEFAULT_IDLE,\n        \"{name}\",\n    );\n}}\n")
open("/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/6112789e-19c9-4d7b-9638-b241cdbc833d/scratchpad/promo_tests.rs","w").write("\n".join(out))

new_curated=sorted(EXISTING_CURATED+PROMOTE)
new_be=sorted(set(ALL_BEST_EFFORT)-set(PROMOTE))
def fmt(ids):
    return ", ".join(str(i) for i in ids)
print("NEW_CURATED count:", len(new_curated))
print(fmt(new_curated))
print("\nNEW_BEST_EFFORT count:", len(new_be))
print(fmt(new_be))
print("\ntest blocks:", len(blocks), "-> promo_tests.rs")
