#!/usr/bin/env python3
"""Extract the curated commercial ROMs from the Dropbox zips into the gitignored
tests/roms/external/mapper-NNN-FAMILY/ tree (never committed)."""
import os, zipfile, glob, re

DB = os.path.expanduser("~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)")
DIRS = [DB, os.path.join(DB, "Homebrew & Unlicensed"), os.path.join(DB, "Japan")]
PROJ = "/home/parobek/Code/Commercial_Private-Projects/RustyNES_v2"
EXT = os.path.join(PROJ, "tests/roms/external")
FAMILY = {0:"NROM",1:"MMC1",2:"UxROM",3:"CNROM",4:"MMC3",5:"MMC5",7:"AxROM",
          9:"MMC2",10:"MMC4",11:"ColorDreams",13:"CPROM",66:"GxROM",
          69:"FME7-Sunsoft5B",71:"Camerica",206:"Namcot118",
          33:"TaitoTC0190",48:"TaitoTC0690",93:"Sunsoft3R",152:"Bandai74161"}
# (display stem, exact zip basename)
GAMES = [
 ("Tetris","Tetris.zip"),("Duck Hunt","Duck Hunt.zip"),("Dr. Mario","Dr. Mario.zip"),
 ("Zelda II","Zelda II - The Adventure of Link.zip"),("Dragon Warrior","Dragon Warrior.zip"),
 ("Bionic Commando","Bionic Commando.zip"),("Bubble Bobble","Bubble Bobble.zip"),
 ("Blaster Master","Blaster Master.zip"),("StarTropics","StarTropics.zip"),
 ("Crystalis","Crystalis.zip"),("Mega Man 4","Mega Man 4.zip"),("Mega Man 5","Mega Man 5.zip"),
 ("Mega Man 6","Mega Man 6.zip"),("Double Dragon","Double Dragon.zip"),("Gun.Smoke","Gun.Smoke.zip"),
 ("Rad Racer","Rad Racer.zip"),("Wizards & Warriors","Wizards & Warriors.zip"),
 ("Jackal","Jackal.zip"),("Life Force","Life Force.zip"),("1942","1942.zip"),
 ("1943","1943 - The Battle of Midway.zip"),("Maniac Mansion","Maniac Mansion.zip"),
 ("Gauntlet","Gauntlet.zip"),("Ice Hockey","Ice Hockey.zip"),("Kung Fu","Kung Fu.zip"),
 ("Pac-Man","Pac-Man (Namco).zip"),("Galaga","Galaga - Demons of Death.zip"),
 ("Tecmo Bowl","Tecmo Bowl.zip"),("Bad Dudes","Bad Dudes.zip"),("Rampage","Rampage.zip"),
 ("Rygar","Rygar.zip"),("Bucky O'Hare","Bucky O'Hare.zip"),("Felix the Cat","Felix the Cat.zip"),
 ("Battletoads & DD","Battletoads & Double Dragon - The Ultimate Team.zip"),
 ("Burai Fighter","Burai Fighter.zip"),("MiG 29","MiG 29 - Soviet Fighter.zip"),
 ("Micro Machines","Micro Machines.zip"),("Bee 52","Bee 52.zip"),("Fire Hawk","Firehawk.zip"),
 # --- v2.6.0 broader survey: long-tail mappers (33/48/93/152) ---
 ("Don Doko Don","Don Doko Don.zip"),
 ("Flintstones Dino Peak","Flintstones, The - The Surprise at Dinosaur Peak!.zip"),
 ("Flintstones Dino & Hoppy","Flintstones, The - The Rescue of Dino & Hoppy.zip"),
 ("Shanghai","Shanghai.zip"),("Arkanoid II","Arkanoid II.zip"),
 # --- v2.6.0 broader survey: tricky/notable supported-mapper titles ---
 ("Batman","Batman - The Video Game.zip"),("Batman ROTJ","Batman - Return of the Joker.zip"),
 ("Darkwing Duck","Darkwing Duck.zip"),("DuckTales 2","DuckTales 2.zip"),
 ("Gremlins 2","Gremlins 2 - The New Batch.zip"),("Kirby's Adventure","Kirby's Adventure.zip"),
 ("Ninja Gaiden II","Ninja Gaiden II - The Dark Sword of Chaos.zip"),
 ("Ninja Gaiden III","Ninja Gaiden III - The Ancient Ship of Doom.zip"),
 ("Metroid","Metroid.zip"),("Faxanadu","Faxanadu.zip"),
 ("Marble Madness","Marble Madness.zip"),("Cobra Triangle","Cobra Triangle.zip"),
 ("Time Lord","Time Lord.zip"),("Rad Racer II","Rad Racer II.zip"),
 ("Adventures of Lolo","Adventures of Lolo.zip"),("Mega Man 2","Mega Man 2.zip"),
 ("Mega Man 3","Mega Man 3.zip"),("Super Mario Bros. 2","Super Mario Bros. 2.zip"),
 ("Punch-Out","Punch-Out!!.zip"),("Castlevania III","Castlevania III - Dracula's Curse.zip"),
 ("Bubble Bobble Part 2","Bubble Bobble Part 2.zip"),("Image Fight","Image Fight.zip"),
 ("City Connection","City Connection.zip"),("Fantasy Zone","Fantasy Zone.zip"),
]

def find(zipname):
    for d in DIRS:
        p = os.path.join(d, zipname)
        if os.path.exists(p): return p
    return None

extracted = []
for disp, zipname in GAMES:
    z = find(zipname)
    if not z:
        print(f"  SKIP (missing): {disp}  [{zipname}]"); continue
    with zipfile.ZipFile(z) as zf:
        nes = next((n for n in zf.namelist() if n.lower().endswith(".nes")), None)
        data = zf.read(nes)
    if data[:4] != b"NES\x1a":
        print(f"  SKIP (bad header): {disp}"); continue
    m = (data[6] >> 4) | (data[7] & 0xF0)
    fam = FAMILY.get(m, f"m{m}")
    out_dir = os.path.join(EXT, f"mapper-{m:03d}-{fam}")
    os.makedirs(out_dir, exist_ok=True)
    # clean filename: drop region/version parens, keep the base game name
    base = re.sub(r"\.zip$", "", zipname)
    out = os.path.join(out_dir, base + ".nes")
    with open(out, "wb") as f: f.write(data)
    extracted.append((m, fam, base, len(data)//1024))
    print(f"  + mapper-{m:03d}-{fam}/{base}.nes  ({len(data)//1024}K)")

print(f"\nextracted {len(extracted)} ROMs into tests/roms/external/")
from collections import Counter
c = Counter(f"mapper-{m:03d}-{fam}" for m,fam,_,_ in extracted)
for k,v in sorted(c.items()): print(f"  {k}: {v}")
