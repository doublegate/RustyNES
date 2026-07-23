#!/usr/bin/env bash
set -e
cd /home/parobek/Code/Commercial_Private-Projects/RustyNES
G=/usr/bin/git
$G reset --soft HEAD~1
$G restore --staged .
# mapper 119 commit
$G add crates/rustynes-mappers/src/m119_tqrom.rs crates/rustynes-mappers/src/lib.rs crates/rustynes-mappers/src/m004_mmc3.rs crates/rustynes-test-harness/tests/v21_coverage_mappers.rs docs/mappers.md docs/compatibility.md
$G commit -q -F - <<'EOF'
feat(mappers): add iNES mapper 119 (TQROM) — Pin*Bot, High Speed

TQROM is an MMC3 variant with a mixed CHR address space: 64 KiB CHR-ROM +
8 KiB CHR-RAM, selected per 1 KiB bank by bit 6 of the MMC3 CHR bank number
(set = CHR-RAM, clear = CHR-ROM). New rustynes-mappers/src/m119_tqrom.rs embeds an Mmc3
and delegates PRG/IRQ/mirroring verbatim (the TxSrom/mapper-118 pattern),
overriding only the pattern-table read/write to route on bit 6 (Mmc3 gains a
small chr_bank_1k helper). Parse arm for 119; save-state v1. 8 unit tests +
a mapper-119 boot-smoke; Pin*Bot + High Speed boot/render (coverage_smoke).
39 mapper families now. AccuracyCoin 100%, oracle 60/60 byte-identical.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
echo "mapper: $($G rev-parse --short HEAD)"
# netplay commit
$G add crates/rustynes-netplay/src/connection.rs crates/rustynes-frontend/src/app.rs crates/rustynes-frontend/src/netplay_ui.rs crates/rustynes-frontend/src/debugger/netplay_panel.rs
$G commit -q -F - <<'EOF'
feat(netplay): host learns the joiner's address from the first Sync

Removes the v2.3.0 wart where the host pre-entered the joiner's IP:port.
NetplayConnection::host(local, rom_hash) binds with an unknown remote; pump()
adopts the source address of the first valid Sync (right magic + matching
rom_hash) as the peer, then completes the handshake. Only the first peer is
adopted (set_remote is a no-op once bound); foreign/non-Sync packets ignored
until adoption. UdpTransport holds remote: Option<SocketAddr> (send is a no-op
until known). The joiner still dials via connect(). Frontend Host needs only a
local-port field now. +2 rustynes-netplay tests -> 24. No core/chip change:
AccuracyCoin 100%, oracle 60/60; native + both wasm builds compile. NAT /
>2 players / WebRTC remain future work.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
echo "netplay: $($G rev-parse --short HEAD)"
echo "=== verify split ==="
$G show --stat --oneline HEAD~1 | head -1
$G show --stat --oneline HEAD | head -1
echo "remaining unstaged/untracked (compat-validation + release docs):"
$G status --short | grep -vE "\.snap" | head; echo "(+ 39 .snap files)"
