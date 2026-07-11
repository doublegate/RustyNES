# Netplay: NAT traversal (STUN) + WebRTC / browser transport

**Status:** shipped in v1.0.0 — **deploy bundle + wasm lobby landed; the hosted
stack is deployment-ready, live verification pending the maintainer's hosted
run.** On top of the signaling/transport skeleton, the browser path is
**deployable + usable**: a turn-key `deploy/` Docker bundle (signaling server +
Caddy TLS proxy + coturn STUN/TURN, all per-deploy values via `.env`), a
configurable signaling URL + ICE/STUN list (`[netplay] signaling_url` /
`stun_servers`, plumbed into `BrowserNetplay::connect`), and a wired wasm **lobby
UI** that drives the `RollbackSession` over WebRTC per rAF frame. The remaining
gap is a live end-to-end browser session, which needs the deployed signaling
server **running** + real browsers and **cannot be verified headlessly** — it is
the maintainer's manual step (checklist in `deploy/README.md`), and is **not**
claimed verified here. This file is the spec for those pieces plus the
STUN/hole-punch scaffold they build on.

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
| Deploy bundle (signaling + TLS + STUN/TURN) | `deploy/` (Dockerfile + compose + Caddy + coturn + `.env.example`) | Turn-key + deployment-ready (builds); live session pending the maintainer's hosted run |
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
  defeat basic hole punching; the fallback is a **TURN relay** (RFC 8656). The
  TURN *client* + the relay-transport hand-off are now wired (§2.5, `relay.rs` +
  `UdpTransport::from_relay`, mock-TURN-verified by `relay_loopback`); the only
  remaining item is a **live coturn + two real symmetric-NAT devices** verify
  (see §5).
- **Plumbing** `HolePunch` into `NetplayConnection` end to end (discover →
  exchange → punch → handshake as one flow) **landed in v1.8.7** — the
  `NatConnect` orchestrator (§2.5). The cone-NAT path is end-to-end and
  loopback/mock-verified in CI; live cross-NAT play is still the maintainer's
  manual run (see §5).

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

### 2.5 Native UDP rendezvous (mobile) — the room-code / CGNAT path (v1.8.7)

§2.1–§2.2 give the *pieces* (STUN discovery + the hole-punch state machine);
§2.3 listed "plumb them into `NetplayConnection` end to end" as the small
follow-up. v1.8.7 lands that orchestrator so a mobile (or any native) peer can
host/join an **internet** match by sharing a short **room code** — the path that
matters for Android, where two phones are typically behind **carrier-grade NAT
(CGNAT)** and cannot exchange addresses by hand.

**`NatConnect` — the steppable pump.** `rustynes-netplay::nat_connect`
(native-only, gated behind the `netplay-client` feature) wires the isolated
building blocks into one steppable flow. Each `pump()` call advances one step and
returns the current `NatPhase`; STUN discovery and TURN allocation do
read-timeout-bounded *blocking* probes (sub-second), not strictly non-blocking
I/O. The caller drives it once per tick (the mobile bridge does this inside
`np_advance_frame`) until `Synced`,
then takes the ready `NetplayConnection` with `into_connection()`:

```text
Registering ─ connect to signaling + Join/host the room ──────────▶ Discovering
Discovering ─ STUN: learn our public reflexive addr (§2.1) ───────▶ Exchanging
Exchanging  ─ send/receive PublicAddr over signaling ─────────────▶ Punching
Punching    ─ send Sync packets at the peer's public addr (§2.2) ─▶ Synced
              └─ (symmetric NAT: punch times out, TURN configured) ▶ Relaying ─▶ Synced
```

Key properties:

- **One socket throughout.** The same bound `UdpSocket` carries the STUN probe,
  the punch packets, *and* the eventual gameplay transport — so the public
  mapping the peer learns is exactly the mapping gameplay flows over. During the
  bounded STUN probe — and again during the bounded TURN allocate + permission —
  the socket is briefly made blocking, then restored to non-blocking for the
  punch / gameplay path.
- **Deterministic, look-alike-free room codes.** `host()` returns a 6-char code
  from a SplitMix64-seeded alphabet that omits `0/O/1/I/L` (so a verbally-shared
  code is unambiguous). The seed also drives the STUN transaction ids; the mobile
  bridge seeds it non-deterministically per session so two concurrent hosts never
  collide on a code. This is host-side orchestration only — it never touches the
  emulator's determinism contract (the ROM + input + core seed are untouched).
- **It hands off to the existing session unchanged.** Once `Synced`,
  `into_connection()` builds a `UdpTransport` fixed at the peer's punched public
  address and runs the normal `NetplayConnection` handshake; a standard
  `RollbackSession<UdpTransport>` then drives the match exactly as the LAN /
  direct-IP path does. The 2-player room-code path completes the first joiner
  (mirroring the direct-IP `np_host`).

**The `PublicAddr` signaling extension — one relay, two rendezvous shapes.**
The address exchange in the `Exchanging` phase rides the **same signaling relay**
that brokers the browser SDP/ICE handshake (§3.2). A new
`SignalMessage::PublicAddr { from, to, addr }` variant carries a single
`IP:port` string (a `SocketAddr` rendered to text) from one slot to another.
Unlike `Offer` / `Answer` / `Candidate` (which carry browser WebRTC SDP/ICE),
`PublicAddr` carries the raw reflexive address the native client feeds to
`HolePunch::peer_discovered`. The relay routes it **by slot exactly like the SDP
messages** — `signaling::Relay` already had the room bookkeeping + slot routing,
so the same deployed server serves *both* the browser path and the mobile
native-UDP path with no new service. The mobile **signaling client**
(`signaling_client.rs`) is a small blocking-worker `tungstenite` WebSocket client
behind the `netplay-client` feature (no tokio; it mirrors the
`rustynes-cheevos/http.rs` blocking-worker shape), so it slots into the
single-threaded mobile tick without an async runtime.

**TURN relay fallback (`relay.rs`) — with its transport hand-off now wired.** A
**symmetric NAT** assigns a different external port per destination, which
defeats basic hole punching. When the punch times out *and* a TURN relay is
configured, `NatConnect` enters `Relaying`: `relay.rs` is an RFC 8656 TURN client
(long-term-credential `Allocate` → `XOR-RELAYED-ADDRESS`, `Send`/`Data`
indications) plus a `RelayUdpSocket` shim, and the orchestrator allocates a relay,
installs a permission for the peer, and exchanges the **relayed** address over the
same `PublicAddr` channel. Once both relayed addresses are exchanged,
`into_connection()` builds a relay-backed `UdpTransport` via
`UdpTransport::from_relay`, so live gameplay flows over the relay (§2.5 below).

Because `Allocate` and `CreatePermission` ride **unreliable UDP**, the client
**retransmits** each request every `RTO` (250 ms) until the caller's overall
timeout (5 s for `Allocate`, 2 s for `CreatePermission`), guided by RFC 5389 §7.2.1 (a fixed 250 ms RTO, not the RFC default 500 ms + exponential backoff) —
a single dropped request-or-response datagram must not hard-fail the transaction.
STUN/TURN requests are idempotent (the server re-answers a retransmit; a late
duplicate response is discarded by the transaction-id filter), so the recovery is
transparent. The unauthenticated 401-challenge round and the authenticated retry
are each their own retransmitted transaction. (The STUN discovery probe is
single-shot per pump but retried across pumps by `tick_discovering`, so it heals
a dropped datagram the same way.) The read-timeout expiry that ends each blocking
receive surfaces as `WouldBlock` on Unix and `TimedOut` on Windows; both are
treated identically (loop and retransmit, never fail).

> **Status — the relay-transport hand-off is wired and mock-TURN-verified;
> only a live coturn run is pending.** `#40` added `UdpTransport::from_relay`,
> so when both peers are symmetric-NAT-bound `NatConnect::into_connection` now
> builds a **relay-backed** `UdpTransport` over `RelayUdpSocket` and live
> gameplay flows over the TURN relay (the bridge reports `relayed = true` and the
> Android UI shows the "via relay" badge). The end-to-end relay path (allocate,
> permission, relayed-address exchange, and the gameplay transport) is exercised
> by the `relay_loopback` test against a mock TURN endpoint. The **only** remaining
> item is a **live verify with a real coturn server and two real symmetric-NAT
> devices**, which CI/offline cannot reproduce (see §5). Cone NAT (the common
> home/CGNAT-cone case) does not need the relay and works end-to-end regardless.

**Bridge + Android surface.** The mobile bridge (`rustynes-mobile`) exposes
`np_host_room(num_players, NpNetConfig) -> room code` and
`np_join_room(room_code, NpNetConfig)`. `NpNetConfig { stun_servers, turn_url,
turn_user, turn_secret, signaling_url }` is projected onto `NatConfig`: an empty
`stun_servers` falls back to the crate's `DEFAULT_STUN_SERVERS`
(`stun.l.google.com:19302` + `stun1`), and a TURN relay is configured only when
the URL resolves *and* both credentials are present (otherwise the session is
punch-or-fail / cone-NAT-only). The session begins in a new `Negotiating` phase
(surfaced by `np_status` with a short sub-step string — `"Discovering"`,
`"Exchanging"`, `"Punching"`, `"Relaying"`) and converges on the existing
`Connecting` → `InGame`. The Android UI is a create-and-share-a-room-code /
join-by-code flow with the endpoints overridable in Settings; it ships defaulting
to a **placeholder** relay URL (`wss://relay.rustynes.example/ws`) until the
maintainer hosts the `deploy/` stack (§3.4) and substitutes a real one.

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

The `deploy/` directory is a **turn-key** bundle for the server side — a
maintainer can `docker compose up` on a host with a domain and get a working
signaling + STUN/TURN stack with no source edits (all per-deploy values come
from a `.env`). The **same** stack serves both the browser (SDP/ICE) and the
mobile (`PublicAddr`, §2.5) rendezvous — one relay, both clients. The mobile
client maps `.env` onto its `NpNetConfig` (`signaling_url` = `wss://<DOMAIN>`,
the TURN URL/creds from `TURN_*`, STUN default), as `deploy/README.md` lays out:

| File | Role |
|---|---|
| `deploy/Dockerfile` | Builds + runs the `rustynes-netplay` `signaling_server` example (`--features signaling-server`). |
| `deploy/docker-compose.yml` | Wires `signaling` + `caddy` (TLS → `wss://`) + `coturn` (STUN/TURN); coturn credential/realm injected from env. |
| `deploy/Caddyfile` | TLS termination + WebSocket-upgrade reverse proxy. |
| `deploy/turnserver.conf` | Minimal coturn STUN + TURN config (credential/realm come from env, not checked in). |
| `deploy/.env.example` | Template for `DOMAIN` + `TURN_*`; copy to `.env` (gitignored). |
| `deploy/README.md` | Full run/deploy steps + the manual verification checklist. |
| workspace-root `.dockerignore` | Keeps `target/`, ROMs, docs out of the image build context. |

**Local two-tab test:**

```text
cd deploy && DOMAIN=localhost docker compose up --build
```

Caddy serves `wss://localhost/` with an internal self-signed CA; coturn provides
STUN/TURN on `:3478`. Point the wasm build's `signaling_url` at `wss://localhost`
and open it in two tabs.

**Public deploy:** `cp deploy/.env.example deploy/.env`, set `DOMAIN` to a real
hostname (Caddy auto-provisions a Let's Encrypt cert — drop `tls internal` from
the `Caddyfile`), set a strong `TURN_SECRET` + `TURN_REALM`, and point
`[netplay] signaling_url` / `stun_servers` at your host. Full notes + the
**manual verification checklist** (2-tab → 2-machine → 4-player matrix + the
ops/DNS/TLS/TURN-bandwidth steps) live in `deploy/README.md`.

**No COOP/COEP needed.** The browser path uses a WebRTC `RtcDataChannel` plus an
`AudioWorklet` — neither needs `SharedArrayBuffer`, so the hosting page needs no
cross-origin-isolation (`Cross-Origin-Opener-Policy` / `-Embedder-Policy`)
headers, and the existing GitHub Pages deploy serves it unchanged.

**NAT / relay notes.** ICE (the browser's own STUN/TURN agent) subsumes the §2
native hole-punch logic for the WebRTC path. STUN alone traverses most home
(cone) NATs; **symmetric NATs** need the **TURN relay** (coturn) as a fallback —
which, unlike STUN, carries the media (here the data-channel) so it costs
bandwidth. Run your own coturn (in the bundle) rather than leaning on public STUN
for anything beyond a quick test.

---

## 4. Spectator mode — read-only (v1.7.0 "Forge" Workstream H8)

A **spectator** joins a running match purely to watch. It is a
determinism-safe, *receive-only* extension of the rollback stack:
`rustynes_netplay::SpectatorSession` (`crates/rustynes-netplay/src/spectator.rs`),
surfaced natively by the Netplay panel's **Spectate** control
(`netplay_ui::NetplayUi::start_spectate`).

**How it stays determinism-safe (the hard contract):**

- A spectator **predicts nothing and rolls back never.** It advances the local
  emulator a frame *only* once every player's real input for that frame has
  arrived (the frame is fully confirmed). Because the players, at a confirmed
  frame, ran it from real inputs only, the spectator — replaying those same
  confirmed inputs from the same deterministic cold-boot — reproduces the frame
  **byte-for-byte** (the same `set_buttons` / Four Score / `run_frame` routing
  as the players, in `apply_and_run`). This is a strict subset of the player
  rollback algorithm.
- It uses the transport **poll-only**: it `send`s no `Input`, `InputAck`,
  `Checksum`, or `Quality`, so it cannot perturb the match it watches. It does
  one optional self-announce `Sync` so a spectator-aware host can learn where to
  relay the stream; after that it is silent.
- It draws no randomness and reads no wall clock. Its effect on the existing
  2-4 player rollback path is **zero** — a spectator is invisible to the players
  (the players' `RollbackSession` already drops any unexpected packet).

A spectator necessarily runs `input_delay + network-latency` frames *behind*
the live match (it can only show a frame it has *received* every input for).
`SpectatorSession::pending_frames` reports how far behind it is, so the frontend
can fast-forward to catch up; the Netplay panel + status bar show `spectate fN
+pending`.

**Maintainer-manual carryover (like the live 2-4p matrix):** the host-side
spectator *broadcast/relay* (a host fanning the confirmed input stream to N
spectators) + the `deploy/` relay config are deployment-ready but not
self-certifiable headlessly. The frontend driver + the determinism-safety
property are exercised by unit tests (see the verified table below). Until the
host-side spectator broadcast lands, the panel's Spectate control dials a host
and waits for a stream; the byte-identical replay path it runs on receipt is the
tested-and-proven part.

---

## 5. What is verified vs. pending

**Verified:**

| Item | How |
|---|---|
| STUN request encode + response decode (XOR-MAPPED + MAPPED, v4/v6) | Unit tests |
| Malformed/short/wrong-cookie/wrong-id rejection | Unit tests |
| Hole-punch state machine transitions | Unit tests |
| Native UDP rendezvous orchestration (`NatConnect`: register → discover → exchange → punch → hand-off) end to end (v1.8.7, §2.5) | Loopback integration test (`tests/nat_loopback.rs`) — drives two orchestrators over **real loopback UDP sockets** through an **in-process WS signaling relay** (the production `signaling::Relay`) + a **mock STUN responder**, both reach `Synced`, hand off `NetplayConnection`s, and run an N-frame `RollbackSession` whose confirmed digests agree (the §2.4 proof shape) |
| Deterministic, look-alike-free 6-char room codes + `PublicAddr` slot routing | Unit tests (`nat_connect` room-code determinism/alphabet; `signaling::Relay` routes `PublicAddr` by slot like SDP) |
| `rustynes-netplay` compiles on `wasm32-unknown-unknown` | Build + clippy |
| TURN relay-transport hand-off for symmetric-NAT pairs (v1.8.7 #40, §2.5) | Integration test (`relay_loopback`) — `UdpTransport::from_relay` routes gameplay over `RelayUdpSocket` against a **mock TURN endpoint** (allocate + permission + relayed-address exchange + N-frame `RollbackSession`); `NpStatus.relayed` reflects the hand-off |
| N-peer UDP roster handshake (3-4 players) | Loopback integration test (`tests/mesh_udp.rs`, real sockets) |
| Signaling room/relay protocol (`signaling::Relay`) | Unit tests (default build) |
| Signaling server builds with its feature | `--features signaling-server` build |
| `Roster` wire encode/decode + oversized/malformed rejection | Unit tests |
| Wasm WebRTC frontend wiring + lobby compiles (both `wasm-winit` + `wasm-canvas`) | Build |
| Configurable signaling URL + ICE/STUN list plumbed into `BrowserNetplay::connect` | Build + `wasm_lobby` unit tests |
| Deploy bundle (signaling + Caddy TLS + coturn) builds + is turn-key | `deploy/` Docker images; `.env`-driven, no source edits to deploy |
| Live STUN round-trip | `#[ignore]`d manual probe — **confirmed working live** against `stun.l.google.com`; kept ignored because CI may be sandboxed |
| Desync-diagnostics capture (CRC-match history, first-desync frame, consecutive-mismatch counter) | Unit tests (`rustynes-netplay::diagnostics`) — synthetic CRC sequences; observational only (does not affect the rollback algorithm) |
| Spectator replay is byte-identical to a reference run over the same confirmed inputs (H8) | Unit test (`rustynes-netplay::spectator` — framebuffer compare); `SpectatorSession` sends nothing and waits for fully-confirmed frames |
| Spectator frontend driver (enter `Spectating`, poll-only tick, clean leave) | Unit test (`netplay_ui::spectator_enters_phase_and_leaves_cleanly`) |

**Debugging aid (v1.3.0 Workstream G1).** When a session desyncs, the native
Netplay panel's read-only **Diagnostics** section surfaces a GeraNES-style
`DesyncMonitor`: the room / input topology (which peer drives which controller
port), the in-sync / desynced-at-frame-N status, lifetime checksum-compare +
mismatch counts, the consecutive-mismatch counter, the most recent local-vs-
remote CRC (classified as a timing/cycle divergence when the framebuffer hashes
match, else a state divergence), and a rolling CRC-match history. It reads the
confirmed-frame digests the session already exchanges (`NetMessage::Checksum`)
and never feeds back into the rollback — pure telemetry, determinism intact.

**Pending (deployment-ready, NOT verified — the maintainer's manual run):**

| Item | Needs |
|---|---|
| Real cross-NAT UDP traversal | A STUN server + two real NATs |
| Live mobile room-code play across two cellular devices (§2.5) | The hosted `deploy/` relay (the same signaling + coturn stack) + two real devices behind carrier NAT; live STUN through CGNAT + the punch are exercised against the *real* network, which CI cannot reproduce |
| Live TURN relay play for symmetric-NAT pairs (§2.5) | A **live coturn** server + **two real symmetric-NAT devices**. The relay-transport hand-off itself is wired and mock-TURN-verified (`UdpTransport::from_relay` + `relay_loopback`, see Verified); only the live cross-network run — which CI/offline cannot reproduce — remains. Cone NAT works end-to-end without the relay. |
| Full browser WebRTC netplay (2-4 players) | The deploy bundle **running** on a host/domain + N real browsers — cannot verify headlessly. Walk the checklist in `deploy/README.md` (2-tab → 2-machine → 4-player matrix + ops/DNS/TLS/TURN-bandwidth steps). |
| Host-side spectator broadcast/relay + live spectate (H8) | A spectator-aware host fanning the confirmed input stream to N spectators + the `deploy/` relay config running — the frontend driver + byte-identical replay are unit-tested, the live relay is the maintainer's manual run. |
