#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES_v2
G=/usr/bin/git
# Stage all non-ignored changes (test files, .gitignore, snapshots, the clippy
# fix) — the 73 force-added ROMs are already in the index and stay there.
$G add -A
echo "== staged summary =="
$G status --short | sed -E 's#^(..) (tests/roms/nes-test-roms)/.*#\1 \2/<rom>#' | awk '{print $1}' | sort | uniq -c
echo "== staged ROM .nes count =="; $G diff --cached --name-only -- tests/roms/nes-test-roms/ | grep -c '\.nes$'
echo "== staged non-rom files =="; $G diff --cached --name-only | grep -v 'nes-test-roms/' | head -30
