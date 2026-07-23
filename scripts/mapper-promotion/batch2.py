#!/usr/bin/env python3
"""Batch 2: copy 30 GoodNES ROMs into external/, verify header mapper, emit tests + tier lists."""
import os, re, shutil, zipfile

BASE = os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/GoodNES [v3.23b]")
EXT = "/home/parobek/Code/OSS_Public-Projects/RustyNES/tests/roms/external"

# (id, subdir/filename, target_dir_name)
ITEMS = [
 (35, "UVW/Warioland II (Unl).zip", "mapper-035-JYCompany35"),
 (42, "#AB/Ai Senshi Nicol (FDS Conversion) [p1][!].zip", "mapper-042-BioMiracleFDS"),
 (44, "RST/Super Big 7-in-1 [p1][!].zip", "mapper-044-SuperBig7in1"),
 (46, "RST/RumbleStation 15-in-1 (Unl).zip", "mapper-046-RumbleStation"),
 (49, "RST/Super HIK 4-in-1 [p1][!].zip", "mapper-049-SuperHIK4in1"),
 (50, "RST/Super Mario Bros. (Alt Levels) [p1][!].zip", "mapper-050-SMB2j-FDS"),
 (51, "#AB/11-in-1 Ball Games [p1][!].zip", "mapper-051-BallGames11in1"),
 (52, "#AB/2-in-1 - 1996 Super HIK Gold Card (NT-803) [p1][!].zip", "mapper-052-MarioParty7in1"),
 (56, "RST/Super Mario Bros. 3 (J) (PRG1) [p2][!].zip", "mapper-056-KaiserKS202"),
 (57, "#AB/54-in-1 (Game Star - GK-54) [p1][!].zip", "mapper-057-BMC-GKA"),
 (90, "#AB/1997 Super HIK 4-in-1 (JY-052) [p1][!].zip", "mapper-090-JYCompany90"),
 (115, "#AB/AV Jiu Ji Ma Jiang 2 (Unl) [!].zip", "mapper-115-KashengSFC03"),
 (120, "RST/Tobidase Daisakusen (FDS Conversion).zip", "mapper-120-TobidaseFDS"),
 (134, "#AB/2-in-1 - Family Kid & Aladdin 4 (Ch) [!].zip", "mapper-134-BMC-T4A54A"),
 (136, "LMN/Mei Loi Siu Ji (Metal Fighter) (Sachen) [!].zip", "mapper-136-SachenTCU02"),
 (138, "RST/Silver Eagle (Sachen) [!].zip", "mapper-138-Sachen8259B"),
 (139, "FGH/Final Combat (Sachen-JAP) [!].zip", "mapper-139-Sachen8259C"),
 (141, "OPQ/Po Po Team (Sachen) [!].zip", "mapper-141-Sachen8259A"),
 (142, "OPQ/Pipe 5 (Sachen) [!].zip", "mapper-142-KaiserKS7032"),
 (164, "CDE/Digital Dragon (Ch) [!].zip", "mapper-164-WaixingFinalFantasy"),
 (176, "#AB/12-in-1 Console TV Game Cartridge (Unl) [!].zip", "mapper-176-WaixingFK23C"),
 (189, "LMN/Mario Fighter III (Unl) [!].zip", "mapper-189-TXC-MMC3"),
 (193, "UVW/War in The Gulf (B) (Unl) [!].zip", "mapper-193-NTDEC-TC112"),
 (204, "#AB/64-in-1 [p1][!].zip", "mapper-204-BMC-64in1"),
 (205, "#AB/4-in-1 (K-3131GS, GN-45) [p1][!].zip", "mapper-205-BMC-JC016"),
 (209, "LMN/Mike Tyson's Punch-Out!! (Unl) [!].zip", "mapper-209-JYCompany209"),
 (211, "#AB/2-in-1 - Donkey Kong Country 4 + Jungle Book 2 (Unl) [!].zip", "mapper-211-JYCompany211"),
 (221, "#AB/1000-in-1 (JPx72) [p1][!].zip", "mapper-221-NTDEC-N625092"),
 (245, "CDE/Di Guo Shi Dai (Age of Empires) (ES-1070) (Ch).zip", "mapper-245-WaixingMMC3"),
 (253, "CDE/Dragon Ball Z - Kyoushuu! Saiya Jin Qi Long Zhu (ES-1064) (Ch).zip", "mapper-253-WaixingVRC4-DBZ"),
]

BATCH1_CURATED = [15,28,30,31,36,38,40,41,58,60,61,62,63,72,76,77,79,86,92,94,95,96,97,101,107,111,112,113,132,133,137,140,143,145,146,147,148,149,150,156,162,177,178,180,185,200,201,202,203,212,213,214,218,225,226,227,229,231,232,233,234,240,241,242,244,246,250]
BATCH1_BE = [29,35,39,42,44,46,49,50,51,52,56,57,81,90,104,115,120,134,136,138,139,141,142,164,174,176,179,189,193,204,205,209,211,221,238,245,253,261,268,286,289,290,299,301,303,305,306,312,320,336,348,349,366,513]

def ines_mapper(b):
    if len(b)<16 or b[0:4]!=b"NES\x1a": return None
    lo=b[6]>>4; hi=b[7]&0xF0
    return (hi|lo|((b[8]&0x0F)<<8)) if (b[7]&0x0C)==0x08 else hi|lo
def zip_nes(p):
    z=zipfile.ZipFile(p)
    for n in z.namelist():
        if n.lower().endswith(".nes"): return z.read(n)
    return None
def snake(s):
    s=re.sub(r"\.(nes|zip)$","",s,flags=re.I); s=re.sub(r"\(.*?\)","",s); s=re.sub(r"\[.*?\]","",s)
    s=re.sub(r"[^A-Za-z0-9]+","_",s).strip("_").lower(); return re.sub(r"_+","_",s)

blocks=[]; promoted=[]; errors=[]
for mid, rel, dirname in ITEMS:
    src=os.path.join(BASE, rel)
    if not os.path.isfile(src): errors.append((mid,"MISSING SRC",src)); continue
    hm=ines_mapper(zip_nes(src))
    if hm!=mid: errors.append((mid,f"HEADER {hm} != {mid}",src)); continue
    tgt_dir=os.path.join(EXT, dirname); os.makedirs(tgt_dir, exist_ok=True)
    fn=os.path.basename(rel)
    shutil.copy2(src, os.path.join(tgt_dir, fn))
    name=f"extended_m{mid}_{snake(fn)}"
    blocks.append((mid,name,f"{dirname}/{fn}")); promoted.append(mid)

# emit test blocks
out=["\n// ============================================================",
     "// v2.1.0 \"Fathom\" F3 (batch 2) — GoodNES-sourced BestEffort -> Curated.",
     "// Sachen/Waixing/Kaiser/JY-Company/pirate-multicart boards, one clean",
     "// dump each, byte-identity boot snapshot (ADR 0011).",
     "// ============================================================\n"]
for mid,name,rel in blocks:
    rel_esc=rel.replace('"','\\"')
    out.append(f"#[test]\nfn {name}() {{\n    check(\n        \"{rel_esc}\",\n        DEFAULT_IDLE,\n        \"{name}\",\n    );\n}}\n")
open("/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/6112789e-19c9-4d7b-9638-b241cdbc833d/scratchpad/promo_tests_2.rs","w").write("\n".join(out))

new_curated=sorted(BATCH1_CURATED+promoted)
new_be=sorted(set(BATCH1_BE)-set(promoted))
print("copied+verified:", len(promoted), "errors:", len(errors))
for e in errors: print("  ERR", e)
print("\nFINAL CURATED", len(new_curated), ":", ", ".join(map(str,new_curated)))
print("\nFINAL BEST_EFFORT", len(new_be), ":", ", ".join(map(str,new_be)))
