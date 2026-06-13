#!/usr/bin/env bash
# extract-batch.sh — extract a ROM from Dropbox zip, verify its mapper, copy to
# tests/roms/external/mapper-NNN-NAME/<basename>.nes
#
# Usage: extract-batch.sh <expected-mapper> <subdir-name> <dropbox-relative-zip>
#
# Example:
#   extract-batch.sh 0 mapper-000-NROM "Balloon Fight.zip"
#   extract-batch.sh 4 mapper-004-MMC3 "Translations/Akumajou Special - Boku Dracula-kun.zip"
set -euo pipefail
expected="$1"
subdir="$2"
zipname="$3"
DROPBOX="/home/parobek/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)"
DEST_ROOT="/home/parobek/Code/RustyNES_v2/tests/roms/external"
STAGE="/tmp/rustynes-rom-staging/extract"
mkdir -p "$STAGE" "$DEST_ROOT/$subdir"

zippath="$DROPBOX/$zipname"
if [[ ! -f "$zippath" ]]; then
  echo "ZIP-MISSING: $zipname" >&2
  exit 2
fi

# Find the .nes filename in the zip
nesfile=$(unzip -Z1 "$zippath" | grep -i '\.nes$' | head -1)
if [[ -z "$nesfile" ]]; then
  echo "NO-NES-IN-ZIP: $zipname" >&2
  exit 3
fi

# Extract
rm -f "$STAGE/$nesfile"
unzip -qo "$zippath" "$nesfile" -d "$STAGE"

# Verify mapper
result=$(/tmp/rustynes-rom-staging/check-mapper.sh "$STAGE/$nesfile")
got_mapper=$(echo "$result" | cut -f1)
if [[ "$got_mapper" != "$expected" ]]; then
  echo "MAPPER-MISMATCH: expected=$expected got=$got_mapper file=$nesfile" >&2
  echo "  $result" >&2
  exit 4
fi

# Copy to dest
cp "$STAGE/$nesfile" "$DEST_ROOT/$subdir/$nesfile"
sha=$(sha256sum "$DEST_ROOT/$subdir/$nesfile" | cut -d' ' -f1)
echo "OK mapper=$got_mapper subdir=$subdir file=\"$nesfile\" sha256=$sha"
