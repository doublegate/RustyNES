#!/usr/bin/env bash
set -e
cd "$(dirname "$0")/../.."  # the repository root
G=/usr/bin/git
echo "main=$($G rev-parse --short main) branch=$($G rev-parse --short HEAD)"
if $G merge-base --is-ancestor main HEAD; then echo "FF-clean"; else echo "NOT FF"; exit 1; fi
$G switch main
$G merge --ff-only feat/v2.2.x-accuracy-polish
$G push origin main
$G branch -d feat/v2.2.x-accuracy-polish
echo "merged + pushed. main=$($G rev-parse --short main); local==origin: $([ "$($G rev-parse main)" = "$($G rev-parse origin/main)" ] && echo yes || echo NO)"
echo "branches: $($G branch | tr '\n' ' ')"
