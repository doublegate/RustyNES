#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES
G=/usr/bin/git
# 1. De-nest the vendored nes-test-roms clone so the parent repo can track files
#    inside it. Only removes the nested git metadata; the ROM files remain.
if [ -e tests/roms/nes-test-roms/.git ]; then
  rm -rf tests/roms/nes-test-roms/.git
  echo "removed nested tests/roms/nes-test-roms/.git"
fi
# 2. Force-add exactly the 73 used .nes files past the gitignore.
xargs -a /tmp/add_roms.txt "$G" add -f
echo "staged nes-test-roms .nes files: $($G diff --cached --name-only -- tests/roms/nes-test-roms/ | grep -c '\.nes$')"
echo "== per-suite =="
$G diff --cached --name-only -- tests/roms/nes-test-roms/ | sed -E 's#tests/roms/nes-test-roms/([^/]+)/.*#\1#' | sort | uniq -c
