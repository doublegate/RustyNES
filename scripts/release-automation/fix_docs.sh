#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES
# Convert broken intra-doc links [`X`] -> plain `X` for private/external items
# (private enum variants, the cross-module MovieUi, a private panel method).
for f in crates/rustynes-frontend/src/netplay_ui.rs crates/rustynes-frontend/src/debugger/netplay_panel.rs; do
  # shellcheck disable=SC2016 # the backticks are literal regex text, not expansions
  sed -i -E 's/\[`(NetplayState::[A-Za-z]+|MovieUi|NetplayPanelState::[a-z_]+)`\]/`\1`/g' "$f"
done
echo "remaining broken-link patterns:"
# shellcheck disable=SC2016 # the backticks are literal regex text, not expansions
grep -noE '\[`(NetplayState::[A-Za-z]+|MovieUi|NetplayPanelState::[a-z_]+)`\]' crates/rustynes-frontend/src/netplay_ui.rs crates/rustynes-frontend/src/debugger/netplay_panel.rs || echo "  (none)"
