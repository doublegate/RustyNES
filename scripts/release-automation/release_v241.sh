#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES
G=/usr/bin/git
echo "main=$($G rev-parse --short main) branch=$($G rev-parse --short HEAD)"
$G merge-base --is-ancestor main HEAD && echo "FF-clean" || { echo "NOT FF"; exit 1; }
echo "=== commits to land ==="; $G log --oneline main..HEAD | cat
$G switch main
$G merge --ff-only fix/v2.4.1-vrc2a
$G tag -a v2.4.1 -m "v2.4.1 — VRC2a (mapper 22) register-select fix

Patches the v2.4.0 VRC2 fix: it swapped the A0/A1 register-select pins for VRC2c
(mapper 25) but left VRC2a (mapper 22) straight. Per nesdev, VRC2a wires the
chip's A0 pin to CPU A1 and A1 to CPU A0 (the same swap as VRC2c), so mapper-22
CHR-bank writes landed in the wrong slot/nibble and TwinBee 3's background tiles
stayed scrambled. Fix = 22 => (bit(1), bit(0)); visually verified. Isolated to
mapper 22 (m23/m25 byte-identical); AccuracyCoin 100% (139/139) unchanged.
See CHANGELOG [2.4.1] + docs/release-notes/v2.4.1.md."
$G push origin main
$G push origin v2.4.1
echo "merged + pushed. main=$($G rev-parse --short main); local==origin: $([ "$($G rev-parse main)" = "$($G rev-parse origin/main)" ] && echo yes || echo NO)"
