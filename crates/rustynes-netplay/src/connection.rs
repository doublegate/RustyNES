//! Stage 2: a real UDP [`Transport`] plus the direct host/join connection
//! layer (handshake, ping/RTT, frame-advantage estimation).
//!
//! # What this adds over Stage 1
//!
//! Stage 1's [`RollbackSession`](crate::session::RollbackSession) is fully
//! transport-agnostic: it only ever [`send`](Transport::send)s and
//! [`poll`](Transport::poll)s [`NetMessage`]s. This module supplies:
//!
//! - [`UdpTransport`] — a [`Transport`] backed by a non-blocking
//!   [`UdpSocket`]. `send` serializes with [`NetMessage::to_bytes`] and
//!   `send_to`s the remote peer; `poll` drains every pending datagram,
//!   parsing each with [`NetMessage::from_bytes`] and **silently dropping any
//!   malformed, truncated, or foreign-version packet** — UDP is hostile input
//!   and the transport never panics on a bad datagram. Per-poll work is capped
//!   so a flood cannot spin the loop unbounded. The remote may be **unknown**
//!   at construction (host "listen" mode); until it is learned, `send` is a
//!   silent no-op and [`poll`](Transport::poll) reports each datagram's source
//!   so the connection can adopt it.
//! - [`NetplayConnection`] — a small host/join state machine that owns a
//!   `UdpTransport`, performs the [`NetMessage::Sync`] handshake (both sides
//!   exchange + confirm a matching magic and identical `rom_hash`), and tracks
//!   the [`ConnectionState`]. There is no matchmaking — the joiner dials the
//!   host's `IP:port` via [`connect`](NetplayConnection::connect); the host can
//!   instead [`host`](NetplayConnection::host) WITHOUT a known remote and
//!   **adopt** the joiner's address from the first valid `Sync`.
//! - Ping / RTT + frame-advantage measurement (a lightweight `Quality`
//!   ping/pong), so Stage 3 / the session can size the input delay and drive
//!   time-sync.
//!
//! # Determinism boundary
//!
//! [`std::time::Instant`] / wall-clock and OS-level socket randomness live
//! **only** here, on the host side. The [`RollbackSession`] and its inputs stay
//! seeded and deterministic — nothing in this module is fed into the rollback
//! re-simulation. The connection measures RTT and frame advantage and hands
//! them to the caller as plain numbers; how (or whether) the caller uses them
//! does not perturb the byte-identical replay.
//!
//! [`RollbackSession`]: crate::session::RollbackSession
//! [`NetMessage::to_bytes`]: crate::message::NetMessage::to_bytes
//! [`NetMessage::from_bytes`]: crate::message::NetMessage::from_bytes

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use crate::message::NetMessage;
use crate::relay::RelayUdpSocket;
use crate::transport::Transport;

/// Largest datagram we will ever read. The longest [`NetMessage`] encoding is
/// the 37-byte `Sync` (1 tag + 4 magic + 32 hash); 1500 covers a standard MTU
/// with generous headroom for any future variant or batched payload, and
/// bounds the per-datagram receive buffer.
const RECV_BUF_LEN: usize = 1500;

/// Maximum datagrams drained in a single [`UdpTransport::poll`]. UDP is
/// hostile input: a peer (or an attacker) could flood the socket, so we cap
/// the per-poll drain to keep one frame's work bounded. Anything still queued
/// is read on the next poll. 1024 is far above the handful of messages a
/// well-behaved peer sends per frame.
const MAX_DATAGRAMS_PER_POLL: usize = 1024;

/// The datagram backend a [`UdpTransport`] rides on. Either a plain
/// [`UdpSocket`] (the direct / hole-punched path) or a [`RelayUdpSocket`] (the
/// symmetric-NAT TURN-relay fallback). This is the **socket-source-agnostic**
/// abstraction that lets the SAME `UdpTransport` — and therefore the same
/// [`RollbackSession`](crate::session::RollbackSession) — run over either path
/// with no new transport type and no second session generic (v1.8.7).
///
/// `Direct` does plain `send_to(peer)` / `recv_from`; `Relayed` wraps each
/// outgoing datagram in a TURN Send Indication to the peer's *relayed* address
/// and unwraps inbound Data Indications, all internal to
/// [`RelayUdpSocket`]. Both are non-blocking, so dispatch is uniform.
#[derive(Debug)]
enum SocketKind {
    /// The direct / hole-punched path: a plain non-blocking UDP socket.
    Direct(UdpSocket),
    /// The symmetric-NAT fallback: gameplay framed through a TURN relay.
    Relayed(RelayUdpSocket),
}

/// A [`Transport`] over a non-blocking datagram socket talking to a single
/// remote peer that may be learned after construction.
///
/// The socket is either a direct [`UdpSocket`] or a TURN [`RelayUdpSocket`]
/// (chosen at construction); both present the same plain peer-addressed
/// datagram surface, so the transport — and the session above it — is identical
/// on either path.
///
/// `send` encodes via [`NetMessage::to_bytes`] and sends to the remote;
/// `poll` drains all pending datagrams (until `WouldBlock`), decoding each with
/// [`NetMessage::from_bytes`] and dropping anything that fails to parse. The
/// socket is non-blocking, so neither call ever blocks.
///
/// The remote may be **unknown** at construction: a host that "listens" binds
/// a port without knowing who will dial it. While `remote` is `None`, `send`
/// is a silent no-op (there is nowhere to send yet) and the connection adopts
/// the first valid peer via [`set_remote`](Self::set_remote). Once set, the
/// remote is fixed — later packets from a third party are still readable via
/// `poll` (the connection guards against hijack) but `send` only ever targets
/// the adopted peer.
///
/// This is host-side, non-deterministic I/O — it is deliberately **not** used
/// by the determinism harness, which keeps [`crate::MemoryTransport`].
#[derive(Debug)]
pub struct UdpTransport {
    socket: SocketKind,
    /// The peer we send to. `None` in host-listen mode until the first valid
    /// `Sync` is adopted via [`set_remote`](Self::set_remote). On the relay
    /// path this is the peer's **relayed** transport address.
    remote: Option<SocketAddr>,
    /// Count of datagrams that failed to parse (malformed / truncated /
    /// foreign-version) since construction. Exposed for diagnostics; never
    /// affects behaviour.
    dropped_invalid: u64,
}

impl UdpTransport {
    /// Wrap an already-bound socket and fix its remote peer. The socket is set
    /// non-blocking. Most callers should use [`NetplayConnection::connect`]
    /// instead, which also performs the handshake.
    ///
    /// # Errors
    ///
    /// Returns any error from setting the socket non-blocking.
    pub fn from_socket(socket: UdpSocket, remote: SocketAddr) -> io::Result<Self> {
        Self::from_socket_opt(socket, Some(remote))
    }

    /// Wrap an already-bound socket with an optional remote. `None` means
    /// host-listen mode: `send` is a no-op until [`set_remote`](Self::set_remote)
    /// adopts the first peer. The socket is set non-blocking.
    ///
    /// # Errors
    ///
    /// Returns any error from setting the socket non-blocking.
    pub fn from_socket_opt(socket: UdpSocket, remote: Option<SocketAddr>) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: SocketKind::Direct(socket),
            remote,
            dropped_invalid: 0,
        })
    }

    /// Wrap an allocated TURN [`RelayUdpSocket`] and fix the peer's **relayed**
    /// transport address (the address the peer's own TURN allocation listens
    /// on, exchanged over signaling). Every datagram then rides the relay, but
    /// the transport surface is identical to the direct path, so the same
    /// [`RollbackSession`](crate::session::RollbackSession) drives it — this is
    /// the symmetric-NAT fallback hand-off (v1.8.7). The underlying socket is
    /// set non-blocking.
    ///
    /// # Errors
    ///
    /// Returns any error from setting the underlying socket non-blocking.
    pub fn from_relay(relay: RelayUdpSocket, peer_relayed: SocketAddr) -> io::Result<Self> {
        relay.socket().set_nonblocking(true)?;
        Ok(Self {
            socket: SocketKind::Relayed(relay),
            remote: Some(peer_relayed),
            dropped_invalid: 0,
        })
    }

    /// Bind a fresh non-blocking socket to `local` and fix its `remote` peer.
    ///
    /// # Errors
    ///
    /// Returns any bind or socket-configuration error.
    pub fn bind(local: SocketAddr, remote: SocketAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        Self::from_socket(socket, remote)
    }

    /// Bind a fresh non-blocking socket to `local` with NO remote yet (host
    /// "listen" mode). The remote is adopted later from the first valid `Sync`
    /// via [`set_remote`](Self::set_remote); until then `send` is a no-op.
    ///
    /// # Errors
    ///
    /// Returns any bind or socket-configuration error.
    pub fn bind_listening(local: SocketAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        Self::from_socket_opt(socket, None)
    }

    /// The local address the socket is bound to (resolves an ephemeral
    /// `:0` port to the concrete one the OS picked). On the relay path this is
    /// the local UDP port the relay traffic flows over (NOT the relayed
    /// transport address peers send to).
    ///
    /// # Errors
    ///
    /// Returns any error from the underlying `local_addr` call.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.socket {
            SocketKind::Direct(s) => s.local_addr(),
            SocketKind::Relayed(r) => r.socket().local_addr(),
        }
    }

    /// `true` if this transport rides a TURN relay (the symmetric-NAT
    /// fallback) rather than a direct / hole-punched socket. Surfaced so the
    /// frontend can report "relayed" in its connection status.
    #[must_use]
    pub const fn is_relayed(&self) -> bool {
        matches!(self.socket, SocketKind::Relayed(_))
    }

    /// The remote peer address, once known. `None` in host-listen mode before
    /// the first peer is adopted.
    #[must_use]
    pub const fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote
    }

    /// Adopt `remote` as the peer IF none is set yet (host-listen mode learning
    /// its joiner from the first valid `Sync`). Returns `true` if it was
    /// adopted, `false` if a remote was already bound (later packets from a
    /// different source must NOT hijack an established session).
    pub const fn set_remote(&mut self, remote: SocketAddr) -> bool {
        if self.remote.is_none() {
            self.remote = Some(remote);
            true
        } else {
            false
        }
    }

    /// Total datagrams dropped for being malformed / truncated / foreign
    /// version. Diagnostic only.
    #[must_use]
    pub const fn dropped_invalid(&self) -> u64 {
        self.dropped_invalid
    }

    /// Drain all pending datagrams, returning each decoded [`NetMessage`]
    /// paired with the source [`SocketAddr`] it arrived from. Malformed /
    /// truncated / foreign-version datagrams are dropped (and counted), never
    /// surfaced or panicked on. Per-poll work is capped. This is the
    /// source-aware primitive [`poll`](Transport::poll) is built on, and the
    /// one the connection uses to ADOPT a peer in host-listen mode.
    fn poll_with_source(&mut self) -> Vec<(NetMessage, SocketAddr)> {
        let mut out = Vec::new();
        let mut buf = [0u8; RECV_BUF_LEN];
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            // Read one datagram, dispatching over the socket source. Both arms
            // map to the same three outcomes: a decoded `(len, from)`, "stray —
            // keep draining", or "empty / fatal — stop".
            let recv = match &self.socket {
                SocketKind::Direct(s) => match s.recv_from(&mut buf) {
                    Ok((len, from)) => RecvStep::Got(len, from),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => RecvStep::Empty,
                    // Windows surfaces an ICMP port-unreachable for a prior
                    // send_to as a ConnectionReset on the *next* recv. It is
                    // not fatal and not tied to a specific inbound datagram —
                    // keep draining; the peer may simply not be listening yet.
                    Err(e) if e.kind() == io::ErrorKind::ConnectionReset => RecvStep::Stray,
                    Err(_) => RecvStep::Empty,
                },
                SocketKind::Relayed(r) => match r.recv_step(&mut buf) {
                    Ok(Some((len, from))) => RecvStep::Got(len, from),
                    // A stray relay datagram (not from the server / not a Data
                    // Indication) was consumed — keep draining.
                    Ok(None) => RecvStep::Stray,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => RecvStep::Empty,
                    Err(_) => RecvStep::Empty,
                },
            };
            match recv {
                RecvStep::Got(len, from) => {
                    // We accept datagrams from any source (a NAT may rewrite
                    // the peer's address, and the handshake already bound us to
                    // one logical peer); a foreign or malformed packet simply
                    // fails to decode and is dropped below. We do NOT trust the
                    // address to gate parsing.
                    match NetMessage::from_bytes(&buf[..len]) {
                        Some(msg) => out.push((msg, from)),
                        None => self.dropped_invalid = self.dropped_invalid.saturating_add(1),
                    }
                }
                RecvStep::Stray => {}
                RecvStep::Empty => break,
            }
        }
        out
    }
}

/// The outcome of one datagram read inside [`UdpTransport::poll_with_source`],
/// uniform across the direct and relay socket sources.
enum RecvStep {
    /// A datagram of `len` bytes arrived from `SocketAddr`.
    Got(usize, SocketAddr),
    /// A datagram was consumed but should not surface (a stray / benign error);
    /// keep draining the socket.
    Stray,
    /// The socket is empty or hit a fatal error; stop draining.
    Empty,
}

impl Transport for UdpTransport {
    fn send(&mut self, msg: &NetMessage) {
        // In host-listen mode the remote is not known yet — there is nowhere
        // to send, so this is a silent no-op until the peer is adopted.
        let Some(remote) = self.remote else {
            return;
        };
        let bytes = msg.to_bytes();
        // A failed send (e.g. transient ICMP port-unreachable surfacing as
        // ConnectionReset on Windows, or a full socket buffer) is non-fatal:
        // the rollback protocol tolerates loss, and the next resend covers it.
        // We deliberately swallow the error rather than propagate or panic.
        match &mut self.socket {
            SocketKind::Direct(s) => {
                let _ = s.send_to(&bytes, remote);
            }
            SocketKind::Relayed(r) => {
                let _ = r.send_to(&bytes, remote);
            }
        }
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        self.poll_with_source()
            .into_iter()
            .map(|(msg, _from)| msg)
            .collect()
    }
}

/// The handshake / liveness state of a [`NetplayConnection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// The `Sync` handshake has not yet completed (no matching peer `Sync`
    /// with an identical `rom_hash` confirmed).
    Connecting,
    /// The peer confirmed a matching magic + identical `rom_hash`. Gameplay
    /// traffic can flow; the rollback session can run.
    Synced,
    /// The connection was torn down — either an explicit disconnect, a
    /// handshake timeout, or a rejected ROM hash. Terminal.
    Disconnected,
}

/// Why a [`NetplayConnection`] is in [`ConnectionState::Disconnected`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectReason {
    /// The peer never completed the handshake before the timeout elapsed.
    HandshakeTimeout,
    /// The peer announced a different ROM hash in its `Sync`.
    RomMismatch,
    /// The peer was synced but then went silent past the disconnect timeout
    /// (no datagram of any kind received for [`NetplayConnection`]'s
    /// `peer_disconnect_timeout`). See [`PeerLink`] for the graded liveness
    /// signal that precedes this terminal state.
    PeerTimeout,
}

/// The liveness of an already-[`Synced`](ConnectionState::Synced) peer, graded
/// by how long it has been since the last datagram of any kind arrived.
///
/// This is the **run-time** counterpart to the one-shot
/// [`DisconnectReason::HandshakeTimeout`]: once gameplay is underway, a peer
/// can stall (packet loss, a paused laptop, a flaky LTE handoff) without ever
/// formally disconnecting. GGPO-style netcode surfaces that as a graded signal
/// so the frontend can show a "connection interrupted" overlay *before*
/// tearing the match down, then only give up after a much longer grace period.
///
/// # Why not Mesen's 150 ms
///
/// Mesen's netplay declares a peer stalled after ~150 ms of silence, which is
/// famously trigger-happy: a single dropped `Quality` ping (they are sent only
/// once per second here) or a routine Wi-Fi/LTE retransmit spike routinely
/// exceeds 150 ms of inter-arrival gap on a real internet path whose RTT is
/// already 60-120 ms, producing spurious "desynced/interrupted" flapping on
/// otherwise-healthy connections. We instead grade liveness against the packet
/// cadence: the [`Interrupted`](Self::Interrupted) warning fires only after
/// `peer_interrupt_timeout` (default **2 s** — two full ping intervals plus
/// slack, so a single lost ping never trips it), and the terminal
/// [`TimedOut`](Self::TimedOut) only after `peer_disconnect_timeout`
/// (default **5 s**), matching the multi-second grace windows GGPO/Parsec use.
/// Both are configurable via
/// [`with_peer_timeouts`](NetplayConnection::with_peer_timeouts) for LAN play
/// (where much tighter bounds are appropriate) or high-latency relayed play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerLink {
    /// A datagram arrived within `peer_interrupt_timeout`; the link is healthy.
    Live,
    /// No datagram for at least `peer_interrupt_timeout` but less than
    /// `peer_disconnect_timeout` — the frontend should warn, but the session is
    /// still recoverable (a late packet returns the link to
    /// [`Live`](Self::Live)).
    Interrupted,
    /// No datagram for at least `peer_disconnect_timeout`; the connection is
    /// (or is about to be) torn down with [`DisconnectReason::PeerTimeout`].
    TimedOut,
}

/// A direct host/join netplay connection: owns a [`UdpTransport`], performs the
/// [`NetMessage::Sync`] handshake, and measures ping / frame advantage.
///
/// # Lifecycle
///
/// 1. The joiner calls [`NetplayConnection::connect`] (binds the socket, fixes
///    the host peer, sends the opening `Sync`); the host can instead call
///    [`NetplayConnection::host`] (binds a listening socket with NO remote and
///    adopts the joiner's address from its first valid `Sync`). Either way the
///    connection starts [`ConnectionState::Connecting`].
/// 2. The caller [`pump`](NetplayConnection::pump)s periodically (e.g. once
///    per frame). `pump` re-sends `Sync` until the peer's matching `Sync`
///    arrives, and emits periodic `Quality` pings for RTT / frame-advantage.
/// 3. Once both sides confirm, the state becomes [`ConnectionState::Synced`].
/// 4. The caller then takes the [`UdpTransport`] (via
///    [`into_transport`](NetplayConnection::into_transport)) and hands it to a
///    [`RollbackSession`](crate::session::RollbackSession), OR keeps the
///    connection and drives the session through
///    [`transport_mut`](NetplayConnection::transport_mut).
///
/// Because the same `Sync` messages the connection exchanges are exactly what
/// `RollbackSession::new` also sends/validates, the handshake is robust to
/// either layer driving it — but doing it here first gives the frontend a clear
/// "connected / rejected / timed out" signal before gameplay starts.
#[derive(Debug)]
pub struct NetplayConnection {
    transport: UdpTransport,
    rom_hash: [u8; 32],
    state: ConnectionState,
    disconnect_reason: Option<DisconnectReason>,

    /// We have seen the peer's matching `Sync`.
    peer_synced: bool,

    // --- timing (host-side, non-deterministic) ---
    started: Instant,
    handshake_timeout: Duration,
    last_sync_sent: Instant,
    sync_resend_interval: Duration,
    last_ping_sent: Instant,
    ping_interval: Duration,
    /// Outstanding ping send time, if a ping is in flight awaiting its pong.
    ping_in_flight: Option<Instant>,
    /// Exponentially-smoothed round-trip estimate, milliseconds.
    smoothed_ping_ms: Option<f64>,
    /// When we last received *any* datagram from the peer. Drives the graded
    /// [`PeerLink`] liveness signal and the [`DisconnectReason::PeerTimeout`]
    /// terminal state once [`Synced`](ConnectionState::Synced).
    last_recv: Instant,
    /// Silence after which a synced peer is reported [`PeerLink::Interrupted`].
    peer_interrupt_timeout: Duration,
    /// Silence after which a synced peer is torn down with
    /// [`DisconnectReason::PeerTimeout`].
    peer_disconnect_timeout: Duration,
    /// The peer's most recently reported frame advantage (its local frame
    /// minus its last-confirmed remote frame).
    remote_frame_advantage: i32,
}

impl NetplayConnection {
    /// How often to re-send the opening `Sync` while connecting.
    const DEFAULT_SYNC_RESEND: Duration = Duration::from_millis(100);
    /// How often to send a `Quality` ping once synced.
    const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(1);
    /// Default handshake timeout.
    const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
    /// Default peer-interrupt threshold: silence after which a synced peer is
    /// reported [`PeerLink::Interrupted`]. Two full [`DEFAULT_PING_INTERVAL`]s
    /// (2 s) so a single lost ping never trips it — deliberately far above
    /// Mesen's trigger-happy ~150 ms (see [`PeerLink`]).
    const DEFAULT_PEER_INTERRUPT: Duration = Duration::from_secs(2);
    /// Default peer-disconnect threshold: silence after which a synced peer is
    /// torn down with [`DisconnectReason::PeerTimeout`]. Matches the multi-second
    /// grace window GGPO/Parsec use.
    const DEFAULT_PEER_DISCONNECT: Duration = Duration::from_secs(5);
    /// Smoothing factor for the RTT estimate (new sample weight).
    const PING_SMOOTHING: f64 = 0.2;

    /// Bind `local`, fix `remote`, send the opening `Sync`, and return a
    /// connection in [`ConnectionState::Connecting`]. Drive it to
    /// [`ConnectionState::Synced`] by calling [`pump`](Self::pump)
    /// periodically.
    ///
    /// `rom_hash` is the SHA-256 from [`Nes::rom_sha256`](rustynes_core::Nes::rom_sha256);
    /// the peer must announce an identical hash or the connection is rejected.
    ///
    /// # Errors
    ///
    /// Returns any socket bind / configuration error.
    pub fn connect(local: SocketAddr, remote: SocketAddr, rom_hash: [u8; 32]) -> io::Result<Self> {
        let transport = UdpTransport::bind(local, remote)?;
        Ok(Self::with_transport(transport, rom_hash))
    }

    /// Bind `local` and start "listening" as the host WITHOUT a known remote:
    /// the joiner's address is adopted from the first valid [`NetMessage::Sync`]
    /// (right magic + matching `rom_hash`) seen in [`pump`](Self::pump). This is
    /// the joiner-dials-host flow — the host no longer needs to pre-enter the
    /// joiner's `IP:port`; it only shares its own listening `IP:port`.
    ///
    /// Drive to [`ConnectionState::Synced`] by calling [`pump`](Self::pump)
    /// periodically. Until a peer is adopted, outgoing `Sync`/ping sends are
    /// silent no-ops (there is no remote to send to); the handshake timeout
    /// still applies, so a host nobody ever dials eventually
    /// [`Disconnected`](ConnectionState::Disconnected)s.
    ///
    /// # Errors
    ///
    /// Returns any socket bind / configuration error.
    pub fn host(local: SocketAddr, rom_hash: [u8; 32]) -> io::Result<Self> {
        let transport = UdpTransport::bind_listening(local)?;
        Ok(Self::with_transport(transport, rom_hash))
    }

    /// Build a connection around an existing [`UdpTransport`] (e.g. one bound
    /// with custom socket options). Sends the opening `Sync` immediately.
    #[must_use]
    pub fn with_transport(mut transport: UdpTransport, rom_hash: [u8; 32]) -> Self {
        // Announce ourselves right away so a peer already listening syncs fast.
        transport.send(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            rom_hash,
        });
        let now = Instant::now();
        Self {
            transport,
            rom_hash,
            state: ConnectionState::Connecting,
            disconnect_reason: None,
            peer_synced: false,
            started: now,
            handshake_timeout: Self::DEFAULT_HANDSHAKE_TIMEOUT,
            last_sync_sent: now,
            sync_resend_interval: Self::DEFAULT_SYNC_RESEND,
            last_ping_sent: now,
            ping_interval: Self::DEFAULT_PING_INTERVAL,
            ping_in_flight: None,
            smoothed_ping_ms: None,
            last_recv: now,
            peer_interrupt_timeout: Self::DEFAULT_PEER_INTERRUPT,
            peer_disconnect_timeout: Self::DEFAULT_PEER_DISCONNECT,
            remote_frame_advantage: 0,
        }
    }

    /// Override the handshake timeout (default 10s). Builder-style.
    #[must_use]
    pub const fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// Override the synced-peer liveness thresholds (defaults: interrupt 2 s,
    /// disconnect 5 s). Builder-style. Tighten these for LAN play or loosen
    /// them for high-latency relayed play; keep `interrupt < disconnect`.
    ///
    /// See [`PeerLink`] for why these are seconds, not Mesen's ~150 ms.
    #[must_use]
    pub const fn with_peer_timeouts(mut self, interrupt: Duration, disconnect: Duration) -> Self {
        self.peer_interrupt_timeout = interrupt;
        self.peer_disconnect_timeout = disconnect;
        self
    }

    /// The graded liveness of an already-[`Synced`](ConnectionState::Synced)
    /// peer (or [`PeerLink::TimedOut`] once disconnected for that reason).
    ///
    /// Before the handshake completes this always reports [`PeerLink::Live`]
    /// (the handshake has its own [`DisconnectReason::HandshakeTimeout`]).
    #[must_use]
    pub fn peer_link(&self) -> PeerLink {
        if matches!(self.disconnect_reason, Some(DisconnectReason::PeerTimeout)) {
            return PeerLink::TimedOut;
        }
        if !matches!(self.state, ConnectionState::Synced) {
            return PeerLink::Live;
        }
        let silent = self.last_recv.elapsed();
        if silent >= self.peer_disconnect_timeout {
            PeerLink::TimedOut
        } else if silent >= self.peer_interrupt_timeout {
            PeerLink::Interrupted
        } else {
            PeerLink::Live
        }
    }

    /// Override the ping interval (default 1s). Builder-style.
    #[must_use]
    pub const fn with_ping_interval(mut self, interval: Duration) -> Self {
        self.ping_interval = interval;
        self
    }

    /// The current handshake / liveness state.
    #[must_use]
    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    /// Why the connection disconnected, if it has.
    #[must_use]
    pub const fn disconnect_reason(&self) -> Option<DisconnectReason> {
        self.disconnect_reason
    }

    /// `true` once the handshake has fully completed.
    #[must_use]
    pub const fn is_synced(&self) -> bool {
        matches!(self.state, ConnectionState::Synced)
    }

    /// The smoothed round-trip ping in whole milliseconds, once at least one
    /// ping/pong has completed. `None` before the first RTT sample.
    #[must_use]
    pub fn ping_ms(&self) -> Option<u32> {
        self.smoothed_ping_ms.map(|p| {
            // Clamped into `[0, u32::MAX]` first, so the cast neither truncates
            // nor loses a sign.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let ms = p.round().clamp(0.0, f64::from(u32::MAX)) as u32;
            ms
        })
    }

    /// The peer's most recently reported frame advantage (how many frames ahead
    /// of its last-confirmed remote frame it is running). Stage 3 / the session
    /// can use the local-vs-remote difference to drive time-sync.
    #[must_use]
    pub const fn remote_frame_advantage(&self) -> i32 {
        self.remote_frame_advantage
    }

    /// Borrow the underlying transport (e.g. to drive a session, or inspect
    /// dropped-datagram stats).
    #[must_use]
    pub const fn transport(&self) -> &UdpTransport {
        &self.transport
    }

    /// `true` if the underlying transport rides a TURN relay (the symmetric-NAT
    /// fallback) rather than a direct / hole-punched socket. Surfaced so the
    /// frontend's connection status can report whether gameplay is relayed.
    #[must_use]
    pub const fn is_relayed(&self) -> bool {
        self.transport.is_relayed()
    }

    /// Mutably borrow the underlying transport — the surface a
    /// [`RollbackSession`](crate::session::RollbackSession) drives. NOTE: a
    /// session calls `poll` itself, which consumes inbound datagrams; if you
    /// run the session through this, do NOT also call [`pump`](Self::pump)
    /// (which polls too), or you will race the two pollers. Pick one driver.
    pub const fn transport_mut(&mut self) -> &mut UdpTransport {
        &mut self.transport
    }

    /// Consume the connection, yielding the bound + handshaken transport to
    /// hand to a [`RollbackSession`](crate::session::RollbackSession).
    #[must_use]
    pub fn into_transport(self) -> UdpTransport {
        self.transport
    }

    /// Drive the connection: poll the socket, advance the handshake, emit
    /// periodic `Sync` resends and `Quality` pings, and update RTT /
    /// frame-advantage estimates. Call periodically (e.g. once per frame) while
    /// the state is [`ConnectionState::Connecting`], and optionally afterwards
    /// to keep the ping fresh.
    ///
    /// `local_frame_advantage` is this peer's own frame advantage (e.g. from
    /// [`RollbackSession`](crate::session::RollbackSession)); it is sent in the
    /// outgoing `Quality` ping so the peer can time-sync against us. Pass `0`
    /// before a session exists.
    ///
    /// Returns the current [`ConnectionState`]. Idempotent once
    /// [`ConnectionState::Synced`] or [`ConnectionState::Disconnected`].
    pub fn pump(&mut self, local_frame_advantage: i32) -> ConnectionState {
        if matches!(self.state, ConnectionState::Disconnected) {
            return self.state;
        }

        let now = Instant::now();

        // 1. Drain inbound datagrams (with their source addresses, so a
        //    host-listen connection can ADOPT the peer) and react to the
        //    handshake / ping traffic.
        for (msg, from) in self.transport.poll_with_source() {
            // Any datagram from the peer is proof of life — refresh the liveness
            // clock that drives `PeerLink` / `DisconnectReason::PeerTimeout`.
            self.last_recv = now;
            // A host that has not yet adopted a remote ignores everything
            // EXCEPT a valid Sync (right magic + matching rom_hash), whose
            // source it adopts. Until then there is no peer to talk to.
            let remote_known = self.transport.remote_addr().is_some();
            match msg {
                NetMessage::Sync { magic, rom_hash } => {
                    if magic != NetMessage::SYNC_MAGIC {
                        continue; // foreign / corrupt — ignore, never panic.
                    }
                    if rom_hash != self.rom_hash {
                        // A mismatched ROM is rejected the same way whether or
                        // not we have adopted this peer yet.
                        self.state = ConnectionState::Disconnected;
                        self.disconnect_reason = Some(DisconnectReason::RomMismatch);
                        return self.state;
                    }
                    // Right magic + our ROM. If we are a listening host with no
                    // remote yet, adopt THIS source as the peer (only the first
                    // such packet — `set_remote` is a no-op once bound, so a
                    // later third party cannot hijack the session).
                    if !remote_known {
                        let _ = self.transport.set_remote(from);
                    }
                    // Peer is running our ROM. Echo a Sync so it learns we are
                    // here too (the handshake is symmetric: each side needs the
                    // other's Sync), then mark the peer as synced. The echo now
                    // has a concrete remote to reach (just adopted, if it was a
                    // listening host).
                    if !self.peer_synced {
                        self.transport.send(&NetMessage::Sync {
                            magic: NetMessage::SYNC_MAGIC,
                            rom_hash: self.rom_hash,
                        });
                    }
                    self.peer_synced = true;
                }
                NetMessage::Quality {
                    ping_ms,
                    frame_advantage,
                } => {
                    if !remote_known {
                        // A listening host with no adopted peer ignores all
                        // non-Sync traffic — only a valid Sync may bind a peer.
                        continue;
                    }
                    // A Quality message doubles as the ping pong: the peer
                    // echoes our most recent measured ping back, and reports
                    // its own frame advantage. Record the advantage; resolve
                    // any in-flight ping into an RTT sample.
                    self.remote_frame_advantage = frame_advantage;
                    let _ = ping_ms;
                    if let Some(sent) = self.ping_in_flight.take() {
                        let rtt_ms = now.saturating_duration_since(sent).as_secs_f64() * 1000.0;
                        // Exponential moving average: prev*(1-a) + sample*a,
                        // written via mul_add for accuracy.
                        let smoothed = self.smoothed_ping_ms.map_or(rtt_ms, |prev| {
                            prev.mul_add(1.0 - Self::PING_SMOOTHING, rtt_ms * Self::PING_SMOOTHING)
                        });
                        self.smoothed_ping_ms = Some(smoothed);
                    }
                }
                // Input / InputAck / Checksum belong to the session, not the
                // connection layer. A `Roster` belongs to the N-peer mesh
                // handshake (`mesh_net`), not this 2-player connection. If pump()
                // is the sole poller during the handshake these can only be
                // early/stray and are safely ignored; once a session takes over
                // polling it sees the session-bound ones.
                NetMessage::Input { .. }
                | NetMessage::InputAck { .. }
                | NetMessage::Checksum { .. }
                | NetMessage::Roster { .. } => {}
            }
        }

        // 2. Promote to Synced once the peer has acknowledged our ROM.
        if self.peer_synced && matches!(self.state, ConnectionState::Connecting) {
            self.state = ConnectionState::Synced;
        }

        // 2b. Once synced, enforce the run-time peer-liveness disconnect: a peer
        //     that goes silent past `peer_disconnect_timeout` is terminal. The
        //     softer `Interrupted` grade is surfaced via `peer_link()` without
        //     tearing the session down (a late packet recovers it). See
        //     `PeerLink` for why this is seconds, not Mesen's ~150 ms.
        if matches!(self.state, ConnectionState::Synced)
            && now.saturating_duration_since(self.last_recv) >= self.peer_disconnect_timeout
        {
            self.state = ConnectionState::Disconnected;
            self.disconnect_reason = Some(DisconnectReason::PeerTimeout);
            return self.state;
        }

        // 3. While still connecting, re-send Sync periodically and enforce the
        //    handshake timeout.
        if matches!(self.state, ConnectionState::Connecting) {
            if now.saturating_duration_since(self.started) >= self.handshake_timeout {
                self.state = ConnectionState::Disconnected;
                self.disconnect_reason = Some(DisconnectReason::HandshakeTimeout);
                return self.state;
            }
            if now.saturating_duration_since(self.last_sync_sent) >= self.sync_resend_interval {
                self.transport.send(&NetMessage::Sync {
                    magic: NetMessage::SYNC_MAGIC,
                    rom_hash: self.rom_hash,
                });
                self.last_sync_sent = now;
            }
        }

        // 4. Emit a periodic Quality ping (carries our frame advantage + the
        //    smoothed ping, and starts an RTT timer if none is in flight).
        if now.saturating_duration_since(self.last_ping_sent) >= self.ping_interval {
            self.transport.send(&NetMessage::Quality {
                ping_ms: self.ping_ms().unwrap_or(0),
                frame_advantage: local_frame_advantage,
            });
            self.last_ping_sent = now;
            if self.ping_in_flight.is_none() {
                self.ping_in_flight = Some(now);
            }
        }

        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// Bind a loopback `UdpTransport` to an ephemeral port; fill in the remote
    /// once the peer's port is known via [`UdpTransport::local_addr`].
    fn bind_loopback(remote: SocketAddr) -> UdpTransport {
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        UdpTransport::bind(local, remote).expect("bind loopback udp")
    }

    /// A pair of connected loopback transports on ephemeral ports. Bind two
    /// bare sockets first so each transport is constructed already pointing at
    /// the other's concrete ephemeral port.
    fn transport_pair() -> (UdpTransport, UdpTransport) {
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let sa = UdpSocket::bind(local).unwrap();
        let sb = UdpSocket::bind(local).unwrap();
        let addr_a = sa.local_addr().unwrap();
        let addr_b = sb.local_addr().unwrap();
        let a = UdpTransport::from_socket(sa, addr_b).unwrap();
        let b = UdpTransport::from_socket(sb, addr_a).unwrap();
        (a, b)
    }

    #[test]
    fn netmessage_roundtrips_over_loopback_socket() {
        let (mut a, mut b) = transport_pair();
        let msgs = [
            NetMessage::Input {
                player: 1,
                frame: 0x0102_0304,
                input: 0x5A,
            },
            NetMessage::InputAck { frame: 77 },
            NetMessage::Checksum {
                frame: 9,
                hash: 0xDEAD_BEEF_0BAD_F00D,
                fb_hash: 0x00FF_00FF_00FF_00FF,
            },
            NetMessage::Quality {
                ping_ms: 12,
                frame_advantage: -3,
            },
        ];
        for m in &msgs {
            a.send(m);
        }
        // Localhost UDP is reliable in-order for a handful of small datagrams;
        // give the loopback a brief, bounded chance to deliver.
        let got = drain(&mut b, msgs.len());
        assert_eq!(got, msgs);
    }

    #[test]
    fn malformed_and_foreign_datagrams_are_dropped_not_panicked() {
        let (a, mut b) = transport_pair();
        // Send raw junk straight at b: an empty packet, an unknown tag, a
        // truncated Sync, and a valid one. Only the valid one should surface;
        // none may panic.
        let SocketKind::Direct(raw) = a.socket else {
            panic!("transport_pair builds a Direct transport");
        };
        let dst = b.local_addr().unwrap();
        raw.send_to(&[], dst).unwrap();
        raw.send_to(&[250, 1, 2, 3], dst).unwrap();
        // Tag 2 == Sync but truncated to 3 bytes (needs 37): must fail decode.
        raw.send_to(&[2u8, 1, 2], dst).unwrap();
        let valid = NetMessage::InputAck { frame: 5 }.to_bytes();
        raw.send_to(&valid, dst).unwrap();

        let got = drain(&mut b, 1);
        assert_eq!(got, vec![NetMessage::InputAck { frame: 5 }]);
        assert!(b.dropped_invalid() >= 3, "malformed datagrams were dropped");
    }

    #[test]
    fn handshake_succeeds_over_loopback() {
        let hash = [0x11u8; 32];
        let (ta, tb) = transport_pair();
        let mut a = NetplayConnection::with_transport(ta, hash);
        let mut b = NetplayConnection::with_transport(tb, hash);

        // Pump both until synced or a bounded number of rounds elapse.
        let mut rounds = 0;
        while !(a.is_synced() && b.is_synced()) && rounds < 200 {
            a.pump(0);
            b.pump(0);
            rounds += 1;
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(a.is_synced(), "a synced");
        assert!(b.is_synced(), "b synced");
        assert_eq!(a.state(), ConnectionState::Synced);
    }

    #[test]
    fn host_listen_adopts_joiner_from_first_sync() {
        // The host binds WITHOUT a known remote; the joiner dials the host's
        // concrete port. The host must adopt the joiner's address from its
        // first Sync and both must reach Synced.
        let hash = [0x33u8; 32];
        let host_sock = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let host_addr = host_sock.local_addr().unwrap();
        let host_transport = UdpTransport::from_socket_opt(host_sock, None).unwrap();
        assert!(
            host_transport.remote_addr().is_none(),
            "host starts with no remote"
        );
        let mut host = NetplayConnection::with_transport(host_transport, hash);

        // Joiner dials the host's address from an ephemeral local port.
        let join_transport = bind_loopback(host_addr);
        let mut join = NetplayConnection::with_transport(join_transport, hash);

        let mut rounds = 0;
        while !(host.is_synced() && join.is_synced()) && rounds < 200 {
            host.pump(0);
            join.pump(0);
            rounds += 1;
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(host.is_synced(), "host synced after adopting joiner");
        assert!(join.is_synced(), "joiner synced");
        assert_eq!(
            host.transport().remote_addr(),
            Some(join.transport().local_addr().unwrap()),
            "host adopted the joiner's source address"
        );
    }

    #[test]
    fn host_listen_ignores_foreign_then_adopts_real_peer() {
        // A listening host receives junk + a foreign-magic Sync from a stray
        // socket FIRST; neither must bind it as the peer. Only the valid Sync
        // from the real joiner is adopted.
        let hash = [0x44u8; 32];
        let host_sock = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let host_addr = host_sock.local_addr().unwrap();
        let host_transport = UdpTransport::from_socket_opt(host_sock, None).unwrap();
        let mut host = NetplayConnection::with_transport(host_transport, hash);

        // A stray third party blasts noise + a bad-magic Sync at the host.
        let stray = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        stray.send_to(&[0xFF, 0x00, 0x11], host_addr).unwrap();
        stray
            .send_to(&NetMessage::InputAck { frame: 1 }.to_bytes(), host_addr)
            .unwrap();
        // Pump so the host drains the stray traffic; it must adopt no one.
        for _ in 0..10 {
            host.pump(0);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            host.transport().remote_addr().is_none(),
            "host must not adopt a non-Sync / junk source"
        );

        // Now the real joiner dials in and completes the handshake.
        let mut join = NetplayConnection::with_transport(bind_loopback(host_addr), hash);
        let mut rounds = 0;
        while !(host.is_synced() && join.is_synced()) && rounds < 200 {
            host.pump(0);
            join.pump(0);
            rounds += 1;
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(host.is_synced() && join.is_synced());
        assert_eq!(
            host.transport().remote_addr(),
            Some(join.transport().local_addr().unwrap()),
            "host adopted the REAL joiner, not the stray"
        );
    }

    #[test]
    fn handshake_rejects_rom_mismatch() {
        let (ta, tb) = transport_pair();
        let mut a = NetplayConnection::with_transport(ta, [0x11u8; 32]);
        let mut b = NetplayConnection::with_transport(tb, [0x22u8; 32]);

        let mut rounds = 0;
        while !matches!(
            (a.state(), b.state()),
            (ConnectionState::Disconnected, _) | (_, ConnectionState::Disconnected)
        ) && rounds < 200
        {
            a.pump(0);
            b.pump(0);
            rounds += 1;
            std::thread::sleep(Duration::from_millis(2));
        }
        // At least one side must reject the mismatched ROM.
        let a_rejected = a.disconnect_reason() == Some(DisconnectReason::RomMismatch);
        let b_rejected = b.disconnect_reason() == Some(DisconnectReason::RomMismatch);
        assert!(a_rejected || b_rejected, "a rom mismatch must be rejected");
        assert!(!a.is_synced() && !b.is_synced());
    }

    #[test]
    fn handshake_times_out_with_no_peer() {
        // Point at a dead port; no peer will ever answer.
        let dst = SocketAddr::from((Ipv4Addr::LOCALHOST, 9));
        let t = bind_loopback(dst);
        let mut c = NetplayConnection::with_transport(t, [0u8; 32])
            .with_handshake_timeout(Duration::from_millis(50));
        let mut rounds = 0;
        while !matches!(c.state(), ConnectionState::Disconnected) && rounds < 200 {
            c.pump(0);
            rounds += 1;
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.state(), ConnectionState::Disconnected);
        assert_eq!(
            c.disconnect_reason(),
            Some(DisconnectReason::HandshakeTimeout)
        );
    }

    /// Drain `b` until at least `want` messages have arrived or a bounded
    /// number of attempts elapse (localhost delivery is near-instant but not
    /// strictly synchronous; the bounded retry avoids both a flake and a hang).
    fn drain(b: &mut UdpTransport, want: usize) -> Vec<NetMessage> {
        let mut got = Vec::new();
        for _ in 0..100 {
            got.extend(b.poll());
            if got.len() >= want {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        got
    }
}
