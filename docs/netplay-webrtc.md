# Netplay: NAT traversal (STUN) + WebRTC / browser transport

**Status:** shipped in v1.0.0 — **deploy bundle + wasm lobby landed**. On top of
the signaling/transport skeleton, the browser path is **deployable + usable**:
a `deploy/` Docker bundle (signaling server + Caddy TLS proxy + coturn STUN/TURN),
a configurable signaling URL + ICE/STUN list (`[netplay] signaling_url` /
`stun_servers`, plumbed into `BrowserNetplay::connect`), and a wired wasm **lobby
UI** that drives the `RollbackSession` over WebRTC per rAF frame. The remaining
gap is a live end-to-end browser session, which needs the deployed signaling
server **running** + two real browsers and cannot be verified headlessly. This
file is the spec for those pieces plus the STUN/hole-punch scaffold they
build on.

Also landed: the N-peer UDP roster handshake (3-4 player native UDP mesh,
loopback-verified); the reference signaling server (a deployable WebSocket relay
behind a non-default feature); and the wasm-frontend WebRTC wiring
(compile-verified).

**References:** RFC 5389 (STUN), RFC 8445 (ICE), the WebRTC data-channel API
(`RtcPeerConnection` / `RtcDataChannel`), and the existing transport-agnostic
session core in `crates/rustynes-netplay` (`docs/`-adjacent — see the crate rustdoc).

---

## 1. Where this fits

The rollback session (`RollbackSession<T: Transport>`) is **transport-agnostic**:
it only ever `send`s and `poll`s `NetMessage`s. The base `Transport` is native UDP
(`UdpTransport`); the two pieces a real internet deployment needs are also present:

| Piece | Crate location | State |
|---|---|---|
| STUN client (public-addr discovery) | `rustynes-netplay::stun` | Implemented + unit-tested; live round-trip `#[ignore]`d |
| UDP hole-punch state machine | `rustynes-netplay::stun::HolePunch` | Implemented + unit-tested |
| N-peer UDP roster handshake (3-4 players) | `rustynes-netplay::mesh_net` | Implemented + loopback-verified |
| WebRTC data-channel transport (browser) | `rustynes-netplay::webrtc::{WebRtcTransport, WebRtcMeshTransport}` (wasm-only) | Compile-verified; 2-player + N-peer mesh transports |
| Signaling server + offer/answer/ICE | `crates/rustynes-netplay/examples/signaling_server.rs` | Implemented (reference WS relay, `--features signaling-server`) |
| N-peer browser mesh signaling (2-4 players) | `rustynes-netplay::signaling` (slot-routed offer/answer/candidate) | Implemented + unit-tested for 2/3/4-peer rooms |
| Wasm-frontend netplay wiring + lobby UI | `rustynes-frontend` (`wasm_netplay.rs`, `wasm_lobby.rs`) | Wired + compile-verified for 2-4 player mesh; browser session pending a live deploy |
| Deploy bundle (signaling + TLS + STUN/TURN) | `deploy/` (Dockerfile + compose + Caddy + coturn) | Shipped (builds); live session pending hosting |
| Configurable signaling URL + ICE/STUN list | `[netplay] signaling_url` / `stun_servers` | Shipped |

Nothing here touches the emulator core, so the determinism contract and the
single-player path are unaffected. AccuracyCoin stays 100.00% and the commercial
oracles byte-identical.

---

## 2. NAT traversal (native UDP)

### 2.1 STUN discovery

A peer behind a home NAT does not know the public `IP:port` its router presents
to the internet. STUN (RFC 5389) solves this: the peer sends a **Binding
Request** from its game UDP socket to a public STUN server; the server replies
with a **Binding Success Response** whose **XOR-MAPPED-ADDRESS** attribute is the
source address the server observed — i.e. this peer's public mapping.

`rustynes-netplay::stun` implements exactly this:

- `build_binding_request(rng) -> (bytes, transaction_id)` — the 20-byte header
  (type `0x0001`, magic cookie `0x2112A442`, a fresh random 96-bit transaction
  id, zero attributes).
- `parse_binding_response(buf, expected_tx) -> Option<SocketAddr>` — validates
  type/cookie/length/transaction-id and decodes XOR-MAPPED-ADDRESS (`0x0020`),
  falling back to the deprecated MAPPED-ADDRESS (`0x0001`). Malformed / short /
  wrong-cookie / wrong-id / non-success buffers return `None` (never panic).
  IPv4 and IPv6 are both handled (X-Port = port XOR high-16 of the cookie;
  X-Address = address XOR cookie, plus the transaction id for IPv6).
- `StunClient` (native) — `discover(server, timeout)` drives the round-trip on a
  bound `UdpSocket` and returns the public `SocketAddr`. For a socket **shared**
  with the live `UdpTransport`, use the non-blocking `send_request` +
  `parse_binding_response` (with `last_transaction_id()`) so STUN and game
  traffic share one drain.

**Recommended public servers** (resolved at run time, never hardcode an IP):
`stun.l.google.com:19302`, `stun1.l.google.com:19302`. A production deployment
should run its own (e.g. `coturn`) to avoid third-party rate limits.

**Manual verification** (the `#[ignore]`d probe):

```text
cargo test -p rustynes-netplay --test stun_probe -- --ignored --nocapture
```

### 2.2 UDP hole punching

Once each peer knows its own public address, the two public addresses are
**exchanged out of band** (see §3 — through a signaling server, or in a LAN/test
setup, manually). Then both peers send packets at each other's public address
*simultaneously*: the first outbound packet from each side opens its own NAT's
mapping, so the peer's matching packet then traverses it. A `Sync` packet is a
fine punch packet — it doubles as the existing handshake.

`HolePunch` models this without doing any I/O (so it is portable + unit-tested):

```text
Discovering ──(both public addrs known)──▶ Punching ──(peer's packet received)──▶ Connected
```

- `local_discovered(addr)` — record our STUN result.
- `peer_discovered(addr)` — record the peer's (from signaling); advances to
  `Punching` once both are known.
- `should_punch()` — true while `Punching`; the caller sends punch packets at
  `peer_public()`.
- `punch_received(from)` — a packet from the peer's known public address
  advances to `Connected` (a stray source is ignored — no hijack).

The caller then points the live `UdpTransport`'s remote at `peer_public()` (via
`UdpTransport::set_remote`) and runs the normal `NetplayConnection` handshake +
`RollbackSession`.

### 2.3 Pending (native NAT)

- **Real cross-NAT traversal** needs a reachable STUN server and two real NATs —
  not reproducible in CI/offline, hence the `#[ignore]`d probe.
- **Symmetric NATs** (which assign a different external port per destination)
  defeat basic hole punching; the fallback is a **TURN relay** (RFC 8656), which
  is out of scope here.
- **Plumbing** `HolePunch` into `NetplayConnection` end to end (discover →
  exchange → punch → handshake as one flow) is a small follow-up; the pieces are
  all present and tested in isolation.

### 2.4 N-peer UDP roster handshake

The 2-player UDP path is **point-to-point**: the host adopts a single joiner and
the two exchange input directly. For **3-4 players** every peer must reach every
*other* peer — a **fully-connected mesh** — and a joiner cannot learn the *other*
joiners' addresses by itself (it only ever talks to the host during its
handshake). A host-distributed **roster** closes that gap.

`rustynes-netplay::mesh_net` adds three pieces:

- **`UdpMeshTransport`** — the UDP analogue of the in-memory `MeshTransport`. One
  bound socket plus a table of every *other* peer's `(player, SocketAddr)`;
  `send` fans a `NetMessage` out to all of them, `poll` drains the socket and
  attributes each datagram to its sender. Foreign / malformed datagrams are
  dropped (never panic).
- **`MeshHost`** — listens, adopts up to `num_players - 1` joiners from their
  `Sync`s (assigning each the next free player index), then broadcasts the full
  **`NetMessage::Roster`** — every peer's `SocketAddr` + player index — to all
  joiners. The roster is **re-sent a few times** for UDP loss tolerance.
- **`MeshJoiner`** — dials the host, `Sync`s, waits for the roster, then builds
  its own `UdpMeshTransport` wired to the host **and every other joiner**,
  skipping its own entry. It identifies its own entry by matching its bound
  source address (works on loopback / LAN); behind a NAT it can't self-observe,
  it falls back to the index the host assigned it out of band.

**Protocol version.** `PROTOCOL_VERSION` is bumped **2 → 3** for the new
`NetMessage::Roster` variant. An older (v2) peer's `from_bytes` rejects the
unknown message tag cleanly (returns `None`), so a v2 peer **drops** a v3
`Roster` rather than mis-parsing it. The roster is bounded to **4 entries**
(`NetMessage::MAX_ROSTER`); an oversized or otherwise malformed roster decodes to
`None` — no unbounded allocation on hostile input.

**Robustness.** Malformed / foreign / duplicate datagrams are dropped and never
panic. A duplicate `Sync` from an already-adopted joiner is **idempotent** (it
does not shift player indices). A `Sync` carrying a mismatched ROM hash is
rejected.

**Verification.** The loopback integration test `tests/mesh_udp.rs` stands up a
host + 2-3 joiners on `127.0.0.1` ephemeral ports, completes the multi-joiner
handshake, exchanges the roster, runs ~120 frames of N-player input over the
**real UDP mesh**, and asserts every peer's confirmed gameplay digest equals
each other *and* a single no-rollback reference run (Four Score on for >2
players). This is the same proof shape as the in-memory
`n_player_rollback_matches_reference` determinism test, but over real sockets.

---

## 3. WebRTC / browser transport

A browser cannot open a raw UDP socket, so the wasm netplay path uses WebRTC.

### 3.1 `WebRtcTransport` (implemented skeleton)

`rustynes-netplay::webrtc::WebRtcTransport` (wasm-only) implements the `Transport`
trait over an `RtcDataChannel`:

- Constructed from an **already-open** data channel configured **unreliable +
  unordered** (`RtcDataChannelInit` with `maxRetransmits = 0`, `ordered =
  false`) — the same lossy/out-of-order semantics rollback already tolerates,
  matching UDP.
- `send` → `data_channel.send_with_u8_array(&msg.to_bytes())`.
- `poll` drains an `Rc<RefCell<VecDeque<NetMessage>>>` that the channel's
  `onmessage` callback fills (binary type set to `arraybuffer`; each payload
  decoded with `NetMessage::from_bytes`, malformed dropped).

So a `RollbackSession` drives a browser peer with **no change** to the session
core — identical to how it drives a native UDP peer.

### 3.2 Signaling server (implemented)

A WebRTC peer connection forms only after the two browsers exchange connection
metadata through a third party. The **signaling server** is a small relay (a
WebSocket service) that brokers, per match, the standard WebRTC handshake:

1. **SDP offer** — the offerer creates an `RtcPeerConnection`, creates the data
   channel, calls `createOffer()` → `setLocalDescription(offer)`, and sends the
   offer SDP to the answerer via the server.
2. **SDP answer** — the answerer `setRemoteDescription(offer)`, `createAnswer()`
   → `setLocalDescription(answer)`, and sends the answer SDP back.
3. **ICE candidates** — as each side's ICE agent gathers candidates
   (`onicecandidate`), it forwards them through the server; the peer feeds each
   to `addIceCandidate()`. ICE (RFC 8445) is WebRTC's own STUN/TURN-based
   traversal — so for the browser path, ICE subsumes the §2 native
   STUN/hole-punch logic (configure the `RtcConfiguration` with `iceServers`
   pointing at a STUN/TURN server).
4. Once ICE connects, the data channel fires `onopen`; the app wraps it in
   `WebRtcTransport::new(channel)` and hands it to a `RollbackSession`.

The server only brokers the handshake; it carries **no gameplay traffic** (that
flows peer-to-peer over the data channel).

**Reference server.** `crates/rustynes-netplay/examples/signaling_server.rs`, behind
the **non-default** `signaling-server` cargo feature (so it never bloats the
core / wasm / workspace build). The routing logic is the pure, async-free
`rustynes_netplay::signaling::Relay` — room bookkeeping + the routing decision, no
I/O — which is **unit-tested headlessly in the default build**. The example bin
is just the async **tokio + tokio-tungstenite** WebSocket plumbing around it.

**Run:**

```text
cargo run -p rustynes-netplay --features signaling-server --example signaling_server
```

It listens on `127.0.0.1:9000` by default; override with one CLI arg (e.g.
`0.0.0.0:9000`).

**Deploy:** put it behind a **TLS-terminating reverse proxy** (nginx / Caddy) so
browsers reach it as `wss://...` — an `https` page cannot open a plain `ws://`.
It is stateless apart from its in-memory rooms, so run a single instance or a
room-affinity load balancer. Pair it with a **STUN/TURN** server (`coturn`) for
the actual NAT traversal; the signaling server only brokers the handshake and
carries no gameplay traffic.

**Wire format** (JSON over WebSocket text frames). This is generalized from
2 peers to an **N-peer mesh** (2..=4): `join` carries the room's `max_players`,
and `offer` / `answer` / `candidate` carry `{ from, to }` slots so the relay
routes each to a specific peer:

```text
client → join      { "room": "<code>", "rom_hash": "<hex>", "max_players": 4 }
server → joined     { "slot": N, "max_players": 4 }   (your slot + room size)
server → peer-joined{ "slot": M }                     (a higher-slot peer joined → offer to it)
peer  → offer       { "from": A, "to": B, "sdp": "..." }   (routed to slot B)
peer  → answer      { "from": B, "to": A, "sdp": "..." }   (routed to slot A)
peer  → candidate   { "from": A, "to": B, "candidate": "...", "sdp_mid": "...", "sdp_m_line_index": N }
server → peer-left  { "slot": M }                     (on a peer's disconnect)
server → error      { "reason": "<room-full | rom-mismatch>" }
```

The server assigns each joiner the **next free slot** (`0..max_players`), and the
rule is **the lower slot of any pair offers to the higher slot** — so when a
newcomer joins, every *existing* peer is sent `peer-joined { slot: newcomer }`
and offers to it. The relay routes `offer` / `answer` / `candidate` to the named
`to` slot (a legacy 2-peer client that omits `from`/`to`/`max_players` falls back
to 2 players + "the other peer" routing). It verifies every peer in a room
announced the **same `rom_hash`** (`rom-mismatch` otherwise; `room-full` past
`max_players`). The pure relay logic is unit-tested for 2-, 3-, and 4-peer rooms
in `rustynes_netplay::signaling`.

### 3.3 Wasm-frontend wiring + lobby (wired)

`rustynes-frontend` has a **wasm-only netplay path** (`wasm_netplay.rs`) that:

1. Opens a **WebSocket signaling client** (via `web-sys` `WebSocket`) to the
   configured signaling URL (§3.2). The URL is **configurable** —
   `BrowserNetplay::connect(signaling_url, room, ice_servers)` takes the
   `[netplay] signaling_url` + `stun_servers` from config (no longer a hardcoded
   STUN entry); an empty ICE list falls back to
   `rustynes_netplay::DEFAULT_STUN_SERVERS`.
2. Runs the **N-peer `RtcPeerConnection` offer/answer/ICE mesh handshake** over
   that socket — one peer connection per *other* player — yielding an open
   `RtcDataChannel` to each, configured `iceServers` from the list above.
3. Once **all `max_players - 1`** channels are open, bundles them into a
   `rustynes_netplay::WebRtcMeshTransport` (the browser analogue of the native
   `UdpMeshTransport`: `send` broadcasts to every peer, `poll` drains one merged
   inbox; each `NetMessage` carries its own `player` field so the session
   demultiplexes) and drives the existing `RollbackSession` from the **rAF frame
   loop** — `App::produce_one_frame` routes through
   `produce_one_frame_browser_netplay` while a browser session is active,
   mirroring the native `produce_one_frame_netplay` (single-player path
   byte-for-byte unchanged when inactive).

The **lobby UI** (`wasm_lobby.rs`) is a bounded egui overlay (in the `~`
debugger surface, wasm-only): a signaling-URL field (seeded from config), a room
/ lobby code, Host vs Join, a 2-4 player selector, Connect/Leave, and a status
line (connecting / in-game / error). It is the browser counterpart of the native
`debugger/netplay_panel.rs` (which stays a "native-only" note on wasm). Edits
emit a `LobbyRequest` the `App` drains each frame.

Both `wasm-winit` and `wasm-canvas` builds **compile** with the lobby wired.

**Honest scope.** The path is **wired + compile-verified** for 2-, 3-, and
4-player mesh sessions, and the pure N-peer signaling *protocol* is
unit-tested. A full browser session still needs the signaling server **running**
(see §3.4) plus **N real browsers / tabs**, which **cannot be verified
headlessly**. The lobby is a *functional* lobby, not a polished multi-screen UI.
The build gate is `cargo build -p rustynes-netplay --target wasm32-unknown-unknown`
plus the frontend's two wasm flavours compiling with the netplay + lobby
present.

### 3.4 Deploying the signaling + STUN/TURN stack

The `deploy/` directory is a turn-key bundle for the server side:

| File | Role |
|---|---|
| `deploy/Dockerfile` | Builds + runs the `signaling_server` example (`--features signaling-server`). |
| `deploy/docker-compose.yml` | Wires `signaling` + `caddy` (TLS → `wss://`) + `coturn` (STUN/TURN). |
| `deploy/Caddyfile` | TLS termination + WebSocket-upgrade reverse proxy. |
| `deploy/turnserver.conf` | Minimal coturn STUN + TURN config. |
| `deploy/README.md` | Full run/deploy steps. |

**Local two-tab test:**

```text
cd deploy && DOMAIN=localhost docker compose up --build
```

Caddy serves `wss://localhost/` with an internal self-signed CA; coturn provides
STUN/TURN on `:3478`. Point the wasm build's `signaling_url` at `wss://localhost`
and open it in two tabs.

**Public deploy:** set `DOMAIN` to a real hostname (Caddy auto-provisions a
Let's Encrypt cert), set a strong coturn credential + `external-ip`, and point
`[netplay] signaling_url` / `stun_servers` at your host. Full notes in
`deploy/README.md`.

**NAT / relay notes.** ICE (the browser's own STUN/TURN agent) subsumes the §2
native hole-punch logic for the WebRTC path. STUN alone traverses most home
(cone) NATs; **symmetric NATs** need the **TURN relay** (coturn) as a fallback —
which, unlike STUN, carries the media (here the data-channel) so it costs
bandwidth. Run your own coturn (in the bundle) rather than leaning on public STUN
for anything beyond a quick test.

---

## 4. What is verified vs. pending

**Verified:**

| Item | How |
|---|---|
| STUN request encode + response decode (XOR-MAPPED + MAPPED, v4/v6) | Unit tests |
| Malformed/short/wrong-cookie/wrong-id rejection | Unit tests |
| Hole-punch state machine transitions | Unit tests |
| `rustynes-netplay` compiles on `wasm32-unknown-unknown` | Build + clippy |
| N-peer UDP roster handshake (3-4 players) | Loopback integration test (`tests/mesh_udp.rs`, real sockets) |
| Signaling room/relay protocol (`signaling::Relay`) | Unit tests (default build) |
| Signaling server builds with its feature | `--features signaling-server` build |
| `Roster` wire encode/decode + oversized/malformed rejection | Unit tests |
| Wasm WebRTC frontend wiring + lobby compiles (both `wasm-winit` + `wasm-canvas`) | Build |
| Configurable signaling URL + ICE/STUN list plumbed into `BrowserNetplay::connect` | Build + `wasm_lobby` unit tests |
| Deploy bundle (signaling + Caddy TLS + coturn) builds | `deploy/` Docker images |
| Live STUN round-trip | `#[ignore]`d manual probe — **confirmed working live** against `stun.l.google.com`; kept ignored because CI may be sandboxed |

**Pending:**

| Item | Needs |
|---|---|
| Real cross-NAT UDP traversal | A STUN server + two real NATs |
| Full browser WebRTC netplay (2-4 players) | The deploy bundle **running** + N browsers — cannot verify headlessly |
