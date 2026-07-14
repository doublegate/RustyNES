#!/usr/bin/env python3
"""Parse nes20db.xml and resolve cheat-DB game-name aliases to header-excluded ROM CRCs."""
import re
import sys

XML = "nes20db.xml"

def load():
    data = open(XML, encoding="utf-8").read()
    entries = []  # (region, title, rom_crc32, raw_comment)
    for g in re.findall(r"<game>(.*?)</game>", data, re.S):
        m = re.search(r"<!--\s*(.*?)\s*-->", g)
        comment = m.group(1) if m else ""
        # region\title.nes
        path = comment
        if path.endswith(".nes"):
            path = path[:-4]
        if "\\" in path:
            region, title = path.split("\\", 1)
        else:
            region, title = "", path
        rom = re.search(r'<rom[^>]*crc32="([0-9A-Fa-f]+)"', g)
        crc = rom.group(1).upper() if rom else None
        entries.append((region, title, crc, comment))
    return entries

def norm(s):
    # normalize for fuzzy: lowercase, replace modifier-colon, & etc
    s = s.replace("꞉", ":")  # modifier colon
    s = s.lower()
    return s

def search(entries, *terms):
    terms = [t.lower() for t in terms]
    out = []
    for region, title, crc, comment in entries:
        n = norm(title)
        if all(t in n for t in terms):
            out.append((region, title, crc))
    return out

if __name__ == "__main__":
    entries = load()
    args = sys.argv[1:]
    for region, title, crc in search(entries, *args):
        print(f"{crc}  [{region}]  {title}")
