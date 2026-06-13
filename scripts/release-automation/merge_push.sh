#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES_v2
G=/usr/bin/git
echo "main=$($G rev-parse --short main) branch=$($G rev-parse --short HEAD)"
$G merge-base --is-ancestor main HEAD && echo "FF-clean" || { echo "NOT FF"; exit 1; }
$G switch main
$G merge --ff-only feat/v2.2.x-accuracy-polish
$G push origin main
$G branch -d feat/v2.2.x-accuracy-polish
echo "merged + pushed. main=$($G rev-parse --short main); local==origin: $([ "$($G rev-parse main)" = "$($G rev-parse origin/main)" ] && echo yes || echo NO)"
echo "branches: $($G branch | tr '\n' ' ')"
