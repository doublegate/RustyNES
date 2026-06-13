#!/usr/bin/env bash
# Build the showcase montage from the staged screenshots.
set -euo pipefail
ROOT=/home/parobek/Code/Commercial_Private-Projects/RustyNES_v2
EXT="$ROOT/screenshots/external"
OUT="$ROOT/screenshots/montage.png"
TMP=/tmp/montage-work
rm -rf "$TMP"; mkdir -p "$TMP"

# Roster: "relative/path/in/external|Label"
roster=(
"mapper-000-NROM/Super Mario Bros.png"
"mapper-000-NROM/Donkey Kong.png"
"mapper-000-NROM/Excitebike.png"
"mapper-001-MMC1/Legend of Zelda, The.png"
"mapper-001-MMC1/Mega Man 2.png"
"mapper-001-MMC1/Castlevania II - Simon's Quest.png"
"mapper-001-MMC1/Ninja Gaiden.png"
"mapper-001-MMC1/Faxanadu.png"
"mapper-001-MMC1/Bionic Commando.png"
"mapper-002-UxROM/Mega Man.png"
"mapper-002-UxROM/Contra.png"
"mapper-002-UxROM/Castlevania.png"
"mapper-002-UxROM/Disney's DuckTales.png"
"mapper-002-UxROM/Life Force.png"
"mapper-003-CNROM/Gradius.png"
"mapper-004-MMC3/Super Mario Bros. 3.png"
"mapper-004-MMC3/Mega Man 3.png"
"mapper-004-MMC3/Kirby's Adventure.png"
"mapper-004-MMC3/Crystalis.png"
"mapper-004-MMC3/Ninja Gaiden II - The Dark Sword of Chaos.png"
"mapper-069-FME7-Sunsoft5B/Batman - Return of the Joker.png"
"mapper-009-MMC2/Mike Tyson's Punch-Out!!.png"
"fds/Zelda no Densetsu - The Hyrule Fantasy (Japan) (Rev 1).png"
"fds/Metroid (Japan) (Rev 3).png"
"mapper-005-MMC5/Castlevania III - Dracula's Curse.png"
"vs-system/VS Castlevania.png"
"vs-system/VS Excitebike.png"
"mapper-085-VRC7/Lagrange Point (Japan) (En) (1.01).png"
)

tiles=()
for entry in "${roster[@]}"; do
  src="$EXT/$entry"
  if [ ! -e "$src" ]; then echo "MISSING: $entry" >&2; continue; fi
  tiles+=("$src")
done

echo "montage tiles: ${#tiles[@]}"
# 7 columns x 4 rows = 28 cells; tile size 256x240, small border, dark bg.
montage "${tiles[@]}" \
  -tile 7x4 -geometry 256x240+3+3 -background '#101014' \
  "$OUT"
identify -format 'montage: %wx%h  %B bytes\n' "$OUT"
