#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES_v2
G=/usr/bin/git
echo "main=$($G rev-parse --short main) branch=$($G rev-parse --short HEAD)"
$G merge-base --is-ancestor main HEAD && echo "FF-clean" || { echo "NOT FF"; exit 1; }
echo "=== commits to land (main..HEAD) ==="; $G log --oneline main..HEAD | cat
$G switch main
$G merge --ff-only feat/v2.4.0-polish
$G tag -a v2.4.0 -m "v2.4.0 — Compatibility & rendering-accuracy

A 99-title commercial-ROM survey surfaced + FIXED two rendering bugs the
byte-identical oracle had locked into its baselines: VRC7 (Lagrange Point)
blank-screen (unbacked WRAM) and VRC2/VRC4 garbled tiles (vrc_a_bits decode +
m22 CHR >>1). Also: mapper 119 TQROM (39 families), netplay host-learns-address,
the 99-title survey + an audited 107-frame screenshot corpus, and CI maintenance
(clippy clean on 1.86 + current stable; test-roms job --release; checkout@v6).
AccuracyCoin 100% (139/139); ~95 unaffected oracle games byte-identical.
See CHANGELOG [2.4.0] + docs/release-notes/v2.4.0.md."
$G push origin main
$G push origin v2.4.0
echo "merged + pushed. main=$($G rev-parse --short main); local==origin: $([ "$($G rev-parse main)" = "$($G rev-parse origin/main)" ] && echo yes || echo NO)"
