#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES_v2
# Convert broken intra-doc links [`X`] -> plain `X` for private/external items
# (private enum variants, the cross-module MovieUi, a private panel method).
for f in crates/nes-frontend/src/netplay_ui.rs crates/nes-frontend/src/debugger/netplay_panel.rs; do
  sed -i -E 's/\[`(NetplayState::[A-Za-z]+|MovieUi|NetplayPanelState::[a-z_]+)`\]/`\1`/g' "$f"
done
echo "remaining broken-link patterns:"
grep -noE '\[`(NetplayState::[A-Za-z]+|MovieUi|NetplayPanelState::[a-z_]+)`\]' crates/nes-frontend/src/netplay_ui.rs crates/nes-frontend/src/debugger/netplay_panel.rs || echo "  (none)"
