#!/usr/bin/env bash
# Organize generated screenshots into screenshots/external/ mirror layout.
set -euo pipefail
ROOT=/home/parobek/Code/Commercial_Private-Projects/RustyNES
DEST="$ROOT/screenshots/external"
P600=/tmp/ss-gen-600
P2000=/tmp/ss-gen-2000
VS=/tmp/ss-gen-vs
FDS=/tmp/ss-fds
STAGE=/tmp/ss-final
rm -rf "$STAGE"; mkdir -p "$STAGE"

colour_count() { identify -format '%k' "$1" 2>/dev/null || echo 0; }

# --- iNES: union of labels in 600 + 2000, pick higher colour count ---
# Skip vs-system__ labels here (handled by dedicated VS pass) but keep pc10__.
declare -A seen
for f in "$P600"/*.png "$P2000"/*.png; do
  [ -e "$f" ] || continue
  base=$(basename "$f")
  case "$base" in
    "vs-system__"*) continue ;;  # use dedicated VS pass
  esac
  seen["$base"]=1
done

for base in "${!seen[@]}"; do
  a="$P600/$base"; b="$P2000/$base"
  pick=""
  if [ -e "$a" ] && [ -e "$b" ]; then
    ca=$(colour_count "$a"); cb=$(colour_count "$b")
    if [ "$cb" -gt "$ca" ]; then pick="$b"; else pick="$a"; fi
  elif [ -e "$a" ]; then pick="$a"; else pick="$b"; fi
  # label "mapper-NNN-FAM__Game" or "pc10__Game"  -> dir/Game.png
  rel="${base%.png}"
  subdir="${rel%%__*}"
  game="${rel#*__}"
  mkdir -p "$STAGE/$subdir"
  cp "$pick" "$STAGE/$subdir/$game.png"
done

# --- vs-system: dedicated 2500 pass, flat game names ---
mkdir -p "$STAGE/vs-system"
for f in "$VS"/*.png; do
  [ -e "$f" ] || continue
  cp "$f" "$STAGE/vs-system/$(basename "$f")"
done

# --- FDS: strip the " [disksys-fcd]" BIOS suffix from the label ---
mkdir -p "$STAGE/fds"
for f in "$FDS"/*.png; do
  [ -e "$f" ] || continue
  b=$(basename "$f" .png)
  b="${b% \[disksys-fcd\]}"
  cp "$f" "$STAGE/fds/$b.png"
done

echo "=== staged layout ==="
for d in "$STAGE"/*/; do printf "%-30s %d\n" "$(basename "$d")" "$(ls "$d"*.png 2>/dev/null | wc -l)"; done
echo "total: $(find "$STAGE" -name '*.png' | wc -l)"
