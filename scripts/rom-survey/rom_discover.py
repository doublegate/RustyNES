#!/usr/bin/env python3
"""Discover candidate ROMs: prefer the EXACT base-name zip (handle '(USA)'),
search top-level + Homebrew & Unlicensed + Japan; report mapper + supported."""
import os, zipfile, glob

DB = os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)")
DIRS = [DB, os.path.join(DB, "Homebrew & Unlicensed"), os.path.join(DB, "Japan")]
SUPPORTED = {0,1,2,3,4,5,7,9,10,11,13,16,18,19,21,22,23,24,25,26,34,64,65,66,67,68,69,70,71,73,75,78,85,88,118,159,206,210}

CANDIDATES = [
 "Tetris", "Duck Hunt", "Dr. Mario", "Zelda II - The Adventure of Link",
 "Dragon Warrior", "Bionic Commando", "Bubble Bobble", "Blaster Master",
 "StarTropics", "Crystalis", "Mega Man 4", "Mega Man 5", "Mega Man 6",
 "Double Dragon", "Gun.Smoke", "Rad Racer", "Wizards & Warriors", "Jackal",
 "Life Force", "1942", "1943 - The Battle of Midway", "Maniac Mansion",
 "Gauntlet", "Ice Hockey", "Kung Fu", "Pac-Man", "Galaga", "Tecmo Bowl",
 "Bad Dudes", "Rampage", "Rygar", "Bucky O'Hare", "Felix the Cat",
 "Battletoads & Double Dragon", "Burai Fighter",
 # unlicensed / tricky (Homebrew & Unlicensed):
 "MiG 29 - Soviet Fighter", "Micro Machines", "Bee 52", "Fire Hawk",
 "Silver Surfer", "Snake, Rattle n' Roll",
 # --- v2.6.0 broader survey: long-tail mappers ---
 "Image Fight", "Major League",                      # 32 Irem G-101
 "Don Doko Don", "Power Blazer", "Insector X",       # 33 Taito TC0190
 "Flintstones - The Surprise at Dinosaur Peak!",     # 48 Taito TC0690
 "Bubble Bobble Part 2", "Jetsons - Cogswell's Caper, The",  # 48
 "City Connection", "Ninja Kid", "Argus",            # 87 Jaleco JF / others
 "Mito Koumon",                                       # 89 Sunsoft-2
 "Fantasy Zone", "Shanghai",                          # 93 Sunsoft-3R / others
 "Arkanoid II", "Saint Seiya - Ougon Densetsu",      # 152 Bandai
 "Atlantis no Nazo",                                  # 184 Sunsoft-1
 # --- v2.6.0 broader survey: tricky/notable supported-mapper games ---
 "Batman - The Video Game", "Batman - Return of the Joker",
 "Darkwing Duck", "DuckTales 2", "Gremlins 2 - The New Batch",
 "Kirby's Adventure", "Ninja Gaiden II - The Dark Sword of Chaos",
 "Ninja Gaiden III - The Ancient Ship of Doom", "Metroid", "Faxanadu",
 "Marble Madness", "Cobra Triangle", "Time Lord", "Rad Racer II",
 "Adventures of Lolo", "Mega Man 2", "Mega Man 3", "Super Mario Bros. 2",
 "Punch-Out!!", "Castlevania III - Dracula's Curse", "Gimmick!",
]

def mapper_of(b):
    return None if b[:4] != b"NES\x1a" else (b[6] >> 4) | (b[7] & 0xF0)

def find_exact(stem):
    # exact base name, then "(USA)" variants, across DIRS; else loose glob
    for d in DIRS:
        for cand in (f"{stem}.zip", f"{stem} (USA).zip"):
            p = os.path.join(d, cand)
            if os.path.exists(p): return p
    for d in DIRS:
        hits = sorted(glob.glob(os.path.join(d, f"{stem}*.zip")), key=len)
        if hits: return hits[0]  # shortest = closest to base name
    return None

for stem in CANDIDATES:
    z = find_exact(stem)
    if not z:
        # show near-matches to help fix the name
        near = []
        for d in DIRS:
            near += [os.path.basename(p) for p in glob.glob(os.path.join(d, f"{stem.split()[0]}*.zip"))]
        print(f"{stem:34s} NOT FOUND   near: {', '.join(near[:4])}")
        continue
    with zipfile.ZipFile(z) as zf:
        nes = next((n for n in zf.namelist() if n.lower().endswith(".nes")), None)
        data = zf.read(nes) if nes else b""
    m = mapper_of(data)
    sup = "OK" if m in SUPPORTED else "UNSUP"
    print(f"{stem:34s} mapper {str(m):3s} [{sup:5s}] {len(data)//1024:4d}K  {os.path.basename(z)}")
