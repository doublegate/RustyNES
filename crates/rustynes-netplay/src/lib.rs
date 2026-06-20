//! GGPO-style rollback netcode for `RustyNES` v2.
//!
//! This crate is the **transport-agnostic session core** of the netplay
//! feature (v2.3.0). It implements rollback networking — predict the remote
//! player's input, advance immediately, and when the real input arrives
//! differing from the prediction, restore a save-state and re-simulate. The
//! emulator core's determinism contract (same ROM + seed + input ⇒
//! byte-identical state, see `rustynes-core`) is what makes the re-simulation
//! reproduce frames exactly, so two peers converge on identical state.
//!
//! # What is here (Stage 1)
//!
//! - [`Transport`] — the network-agnostic message channel the session talks
//!   through. Stage 2 plugs a UDP implementation in here without touching
//!   the session.
//! - [`MemoryTransport`] — a deterministic in-memory paired transport with
//!   configurable latency / jitter / drop (seeded PRNG only), for tests.
//! - [`NetMessage`] — the versioned wire protocol (`Input`, `InputAck`,
//!   `Sync`, `Checksum`, `Quality`), with a hand-rolled byte encoding ready
//!   for the UDP layer.
//! - [`RollbackSession`] — the GGPO-style core: input history, a save-state
//!   ring, prediction, rollback + re-simulation, confirmation tracking,
//!   periodic checksums (desync detection), and basic time-sync.
//!
//! Determinism is a hard requirement throughout: the session, the transport,
//! and the tests draw randomness only from a seeded [`rng::SplitMix64`] —
//! never `std::time` or the OS RNG — so a run is perfectly reproducible.
//!
//! # What is here (Stage 2)
//!
//! - [`UdpTransport`] — a real UDP [`Transport`] over a non-blocking socket
//!   (serializes via [`NetMessage::to_bytes`], drops malformed / foreign
//!   datagrams without panicking, caps per-poll work).
//! - [`NetplayConnection`] — a direct host/join connection layer: the
//!   [`NetMessage::Sync`] handshake (with [`ConnectionState`] +
//!   [`DisconnectReason`] for rom-mismatch / timeout), and ping/RTT +
//!   frame-advantage measurement for Stage 3's time-sync. All wall-clock use
//!   ([`std::time::Instant`]) is confined here, on the host side — the
//!   [`RollbackSession`] stays seeded + deterministic.
//!
//! # N players (v2.5.0)
//!
//! The session is generalized to **2..=4 players** ([`SessionConfig`]'s
//! `num_players`). Input history is per-player (indexed `[player][frame]`),
//! remote inputs are tagged with their `player` index on the wire
//! ([`NetMessage::Input`]'s `player` field), `last_confirmed_frame` requires
//! *all* players confirmed, and rollback re-applies every player's input each
//! re-sim frame (with the Four Score adapter enabled for >2 players). For more
//! than two players the topology is a **mesh** ([`MeshTransport`]): each peer
//! broadcasts its own input to all others and polls all of them.
//! `num_players == 2` is byte-for-byte the prior pairwise session.
//!
//! # NAT traversal + WebRTC scaffold (v2.5.0 Phase C)
//!
//! - [`stun`] — a minimal STUN client (RFC 5389): build a Binding Request and
//!   parse the XOR-MAPPED-ADDRESS of the Success Response to discover this
//!   peer's public ([`SocketAddr`](std::net::SocketAddr)), plus a [`HolePunch`]
//!   state machine (discovering → punching → connected) for UDP hole punching
//!   once both public addresses are exchanged. The encode/decode + state machine
//!   are unit-tested headlessly; the `StunClient` socket round-trip is native
//!   and real cross-NAT traversal is documented-pending (needs a STUN server +
//!   two real NATs). See `docs/netplay-webrtc.md`.
//! - `webrtc` (wasm-only) — a `WebRtcTransport` skeleton implementing
//!   [`Transport`] over an `RtcDataChannel` (unreliable+unordered, matching UDP
//!   semantics). It compiles on wasm and is structurally complete; full browser
//!   netplay is pending a signaling server + a browser + the wasm-frontend
//!   wiring (the frontend still gates netplay to native).
//! - **Wasm-compile gate:** the portable session core ([`RollbackSession`],
//!   [`Transport`], [`MemoryTransport`]/[`MeshTransport`], [`NetMessage`],
//!   [`stun`]'s encode/decode) compiles on `wasm32-unknown-unknown`; the
//!   `std::net` parts (`connection`, `StunClient`) are `cfg(not(wasm32))`-gated.
//!
//! # N-peer UDP roster handshake (v2.6.0)
//!
//! The multi-joiner **UDP handshake** is now implemented in [`mesh_net`]: a
//! [`MeshHost`] listens, adopts up to `num_players - 1` joiners from their
//! `Sync`s, assigns each the next player index, and distributes the full peer
//! [`Roster`](NetMessage::Roster) (every peer's `SocketAddr` + index) so the
//! joiners form the fully-connected mesh. The resulting per-peer
//! [`UdpMeshTransport`] fans each peer's input out to all others — the UDP
//! analogue of the in-memory [`MeshTransport`]. A loopback integration test
//! (`tests/mesh_udp.rs`) stands up a host + 2-3 joiners on `127.0.0.1`,
//! completes the handshake, runs an N-player session over real sockets, and
//! asserts every peer's confirmed digest matches a no-rollback reference.
//!
//! # Deferred
//!
//! - Matchmaking and richer time-sync remain out of scope.
//! - Symmetric-NAT traversal (TURN relay, RFC 8656) is out of scope; basic
//!   STUN + hole punching covers the common cone-NAT case.
//!
//! [`NetMessage::Sync`]: crate::message::NetMessage::Sync
//! [`NetMessage::Input`]: crate::message::NetMessage::Input

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// The portable session core: transport-agnostic, no `std::net`, compiles on
// `wasm32-unknown-unknown` (the v2.5.0 Phase C wasm-compile gate).
pub mod diagnostics;
pub mod message;
pub mod rng;
pub mod session;
// v1.7.0 "Forge" Workstream H8 — the read-only spectator session: a
// determinism-safe, receive-only extension of the rollback stack (it replays
// the players' confirmed input stream, predicts nothing, sends nothing). It is
// transport-agnostic + std-free in the same way the session core is, so it
// compiles on the wasm gate too.
pub mod spectator;
pub mod stun;
pub mod transport;

// The native UDP transport + host/join connection layer use `std::net`, which
// is unavailable on wasm. Gated to native; the wasm build relies on the
// portable session + a `WebRtcTransport` (see `webrtc`) instead.
#[cfg(not(target_arch = "wasm32"))]
pub mod connection;

// The N-peer UDP roster handshake + mesh transport (v2.6.0). Native-only
// (`std::net`); the in-memory `MeshTransport` covers the wasm + harness path.
#[cfg(not(target_arch = "wasm32"))]
pub mod mesh_net;

// v1.8.7 — the TURN relay client (RFC 8656) for the symmetric-NAT fallback +
// the `RelayUdpSocket` shim so the existing `UdpTransport` runs over a relay
// unchanged. Native-only (`std::net`); the STUN framing is reused from `stun`.
#[cfg(not(target_arch = "wasm32"))]
pub mod relay;

// v1.8.7 — the blocking signaling CLIENT (worker thread + mpsc, no tokio) and
// the NAT-traversal orchestrator that ties signaling + STUN/punch + TURN into
// one steppable pump. Native-only and behind the `netplay-client` feature (it
// pulls a sync WebSocket client); the modules carry their own cfg gate.
#[cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
pub mod nat_connect;
#[cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
pub mod signaling_client;

// The WebRTC signaling room/relay protocol (v2.6.0) — the pure, async-free core
// of the reference signaling server (`examples/signaling_server.rs`, behind the
// `signaling-server` feature). I/O-free + portable, so it compiles everywhere:
// natively it backs the server + is unit-tested; on wasm the frontend uses its
// `SignalMessage` parse/encode for the browser signaling client.
pub mod signaling;

// The WebRTC transport skeleton is wasm-only (it speaks `web_sys`).
#[cfg(target_arch = "wasm32")]
pub mod webrtc;

#[cfg(not(target_arch = "wasm32"))]
pub use connection::{ConnectionState, DisconnectReason, NetplayConnection, UdpTransport};
pub use diagnostics::{CrcCompare, DesyncDiagnostics};
#[cfg(not(target_arch = "wasm32"))]
pub use mesh_net::{MeshError, MeshHost, MeshJoiner, UdpMeshTransport};
pub use message::{NetMessage, PROTOCOL_VERSION, fnv1a64};
#[cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
pub use nat_connect::{NatConfig, NatConnect, NatPhase};
#[cfg(not(target_arch = "wasm32"))]
pub use relay::{RelayUdpSocket, TurnClient, TurnConfig};
pub use rng::SplitMix64;
pub use session::{AdvanceOutcome, MAX_PLAYERS, NetplayError, RollbackSession, SessionConfig};
pub use signaling::{Action, ClientId, Relay, SignalMessage};
#[cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]
pub use signaling_client::{SignalEvent, SignalingClient};
pub use spectator::{SpectatorConfig, SpectatorOutcome, SpectatorSession};
#[cfg(not(target_arch = "wasm32"))]
pub use stun::StunClient;
pub use stun::{
    HolePunch, MAGIC_COOKIE, PunchState, TransactionId, build_binding_request,
    parse_binding_response,
};
pub use transport::{LinkConditions, MemoryTransport, MeshTransport, Transport};

#[cfg(target_arch = "wasm32")]
pub use webrtc::{WebRtcMeshTransport, WebRtcTransport};

/// Default public STUN servers for the browser (WebRTC `iceServers`) and the
/// native STUN client (v2.7.0).
///
/// These are Google's free, well-known public STUN servers — enough for the
/// common cone-NAT case. A production deployment SHOULD run its own
/// (e.g. `coturn`, see `deploy/`) to avoid third-party rate limits and to add a
/// TURN relay for symmetric NATs; both the wasm config (`[netplay] stun_servers`)
/// and the native code can override this list. Resolved at run time by the ICE
/// agent / STUN client — never hardcode a bare IP.
pub const DEFAULT_STUN_SERVERS: [&str; 2] = [
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];

/// Returns the crate version string.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
