#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES
G=/usr/bin/git
echo "main=$($G rev-parse --short main) branch=$($G rev-parse --short HEAD)"
$G merge-base --is-ancestor main HEAD && echo "FF-clean" || { echo "NOT FF"; exit 1; }
echo "=== commits to land ==="; $G log --oneline main..HEAD
$G switch main
$G merge --ff-only feat/v2.3.0-netplay
$G tag -a v2.3.0 -m "v2.3.0 — Netplay (rollback netcode)

Two-player online via GGPO-style rollback over UDP, built on the deterministic
core. New rustynes-netplay crate (rollback session + UDP transport) + a native
frontend (host/join + HUD). No accuracy/behaviour change: AccuracyCoin 100%,
oracle 60/60 byte-identical. See CHANGELOG [2.3.0] + docs/release-notes/v2.3.0.md."
$G push origin main
$G push origin v2.3.0
echo "merged + pushed. main=$($G rev-parse --short main); local==origin: $([ "$($G rev-parse main)" = "$($G rev-parse origin/main)" ] && echo yes || echo NO)"
