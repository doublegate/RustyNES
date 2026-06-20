//! The **NAT-traversal orchestrator** — a non-blocking pump wiring the building
//! blocks into one end-to-end flow (v1.8.7).
//!
//! It composes the previously-isolated pieces ([signaling](crate::signaling),
//! [STUN + hole-punch](crate::stun), [TURN relay](crate::relay)): register/join a
//! room, discover this peer's public address, exchange it, punch through the
//! NAT, and fall back to a TURN relay on a symmetric NAT — handing off a ready
//! [`NetplayConnection`] whose transport the existing
//! [`RollbackSession`](crate::session::RollbackSession) drives unchanged.
//!
//! # The flow ([`pump`](NatConnect::pump) sequences these phases)
//!
//! ```text
//! Registering ─ join/host the signaling room ───────────────▶ Discovering
//! Discovering ─ STUN: learn our public reflexive addr ──────▶ Exchanging
//! Exchanging  ─ send/receive PublicAddr over signaling ─────▶ Punching
//! Punching    ─ send Sync packets at the peer's public addr ▶ Synced
//!               └─ (symmetric NAT: punch times out) ────────▶ Relaying ─▶ Synced
//! ```
//!
//! Each [`pump`](NatConnect::pump) call is non-blocking and steppable once per
//! tick. The whole thing reuses one UDP socket for STUN discovery, the punch
//! packets, AND the eventual gameplay transport, so the public mapping the peer
//! learns is the one gameplay flows over.
//!
//! Native-only and gated behind `netplay-client` (it drives the blocking
//! [`SignalingClient`]).

#![cfg(all(not(target_arch = "wasm32"), feature = "netplay-client"))]

use std::io;
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use crate::connection::{NetplayConnection, UdpTransport};
use crate::message::NetMessage;
use crate::relay::{RelayUdpSocket, TurnClient, TurnConfig};
use crate::rng::SplitMix64;
use crate::signaling::SignalMessage;
use crate::signaling_client::{SignalEvent, SignalingClient};
use crate::stun::{HolePunch, StunClient};

/// The phase of a [`NatConnect`] orchestration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NatPhase {
    /// Connecting to the signaling relay + joining/hosting the room.
    Registering,
    /// Discovering this peer's public reflexive address via STUN.
    Discovering,
    /// Exchanging public addresses with the peer over signaling.
    Exchanging,
    /// Both public addresses known; sending punch packets to open the NAT.
    Punching,
    /// Punch failed (symmetric NAT); allocating + routing through a TURN relay.
    Relaying,
    /// A direct (or relayed) path is open; the [`NetplayConnection`] is ready.
    Synced,
    /// Traversal failed; the `String` is a short reason. Terminal.
    Failed(String),
}

/// Configuration for a [`NatConnect`] orchestration.
#[derive(Clone, Debug)]
pub struct NatConfig {
    /// STUN servers to try for public-address discovery (e.g.
    /// [`crate::DEFAULT_STUN_SERVERS`]). Resolved at run time.
    pub stun_servers: Vec<String>,
    /// Optional TURN relay for the symmetric-NAT fallback. `None` disables the
    /// relay path (punch-or-fail).
    pub turn: Option<TurnConfig>,
    /// The signaling relay URL (e.g. `wss://host` or `ws://host:9000`).
    pub signaling_url: String,
}

/// How long to attempt UDP hole punching before falling back to TURN (or
/// failing if no TURN is configured).
const PUNCH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for the signaling room + the peer's public address.
const SIGNALING_TIMEOUT: Duration = Duration::from_secs(20);
/// STUN discovery budget per pump (non-blocking; retried across pumps).
const STUN_PER_PUMP: Duration = Duration::from_millis(40);
/// How often to (re)send a punch packet while punching.
const PUNCH_RESEND: Duration = Duration::from_millis(50);
/// The no-look-alike room-code alphabet (no 0/O, 1/I/L) — 6 chars.
const ROOM_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// Room-code length.
const ROOM_CODE_LEN: usize = 6;

/// The orchestrator.
///
/// Build with [`host`](Self::host) or [`join`](Self::join), drive with
/// [`pump`](Self::pump) once per tick until [`NatPhase::Synced`], then take the
/// [`NetplayConnection`] via
/// [`into_connection`](Self::into_connection).
pub struct NatConnect {
    socket: Option<UdpSocket>,
    rom_hash: [u8; 32],
    cfg: NatConfig,
    signaling: SignalingClient,
    /// Our assigned slot in the room (0 = host).
    slot: Option<u8>,
    /// The peer's slot we exchange addresses with (the first OTHER slot we learn).
    peer_slot: Option<u8>,
    punch: HolePunch,
    phase: NatPhase,
    started: Instant,
    /// When the Punching phase began (for the punch-vs-TURN timeout).
    punch_started_at: Option<Instant>,
    last_punch_sent: Option<Instant>,
    last_addr_sent: bool,
    rng: SplitMix64,
    /// Our allocated TURN relay socket (symmetric-NAT fallback), once the
    /// `Relaying` phase has allocated it. The gameplay transport is built from
    /// this on the relay path.
    relay_socket: Option<RelayUdpSocket>,
    /// The peer's **relayed** transport address (its TURN allocation), learned
    /// from the second `PublicAddr` the peer sends during the relay handshake.
    /// This is where we route gameplay on the relay path.
    peer_relayed: Option<SocketAddr>,
    /// Whether traversal fell back to the TURN relay. Set in `tick_relaying`;
    /// read by [`is_relayed`](Self::is_relayed) and [`into_connection`].
    relayed: bool,
}

impl NatConnect {
    /// Host a new room: connect to signaling, announce `num_players` + the ROM
    /// hash, and return the orchestrator plus the **room code** to share. The
    /// returned `String` is the 6-char code joiners pass to [`join`](Self::join).
    ///
    /// `seed` seeds the deterministic room-code + STUN transaction PRNG (so a
    /// test can fix the code); pass a fresh value per real session.
    ///
    /// # Errors
    ///
    /// Returns any socket bind error.
    pub fn host(
        num_players: u8,
        rom_hash: [u8; 32],
        cfg: NatConfig,
        seed: u64,
    ) -> io::Result<(Self, String)> {
        let mut rng = SplitMix64::new(seed);
        let room = room_code(&mut rng);
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_nonblocking(true)?;
        let signaling = SignalingClient::connect(&cfg.signaling_url);
        signaling.send(SignalMessage::Join {
            room: room.clone(),
            rom_hash: hex(&rom_hash),
            max_players: num_players,
        });
        Ok((Self::new_inner(socket, rom_hash, cfg, signaling, rng), room))
    }

    /// Join an existing room by its `room_code`: connect to signaling, announce
    /// the ROM hash, and return the orchestrator.
    ///
    /// # Errors
    ///
    /// Returns any socket bind error.
    pub fn join(
        room_code: &str,
        rom_hash: [u8; 32],
        cfg: NatConfig,
        seed: u64,
    ) -> io::Result<Self> {
        let rng = SplitMix64::new(seed);
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_nonblocking(true)?;
        let signaling = SignalingClient::connect(&cfg.signaling_url);
        signaling.send(SignalMessage::Join {
            room: room_code.to_string(),
            // The relay's max_players for a joiner is ignored; default 2.
            rom_hash: hex(&rom_hash),
            max_players: 2,
        });
        Ok(Self::new_inner(socket, rom_hash, cfg, signaling, rng))
    }

    fn new_inner(
        socket: UdpSocket,
        rom_hash: [u8; 32],
        cfg: NatConfig,
        signaling: SignalingClient,
        rng: SplitMix64,
    ) -> Self {
        Self {
            socket: Some(socket),
            rom_hash,
            cfg,
            signaling,
            slot: None,
            peer_slot: None,
            punch: HolePunch::new(),
            phase: NatPhase::Registering,
            started: Instant::now(),
            punch_started_at: None,
            last_punch_sent: None,
            last_addr_sent: false,
            rng,
            relay_socket: None,
            peer_relayed: None,
            relayed: false,
        }
    }

    /// The current phase (without advancing). See [`pump`](Self::pump).
    #[must_use]
    pub fn phase(&self) -> NatPhase {
        self.phase.clone()
    }

    /// Advance the orchestration one non-blocking step and return the resulting
    /// [`NatPhase`]. Drive this once per tick until [`NatPhase::Synced`] or
    /// [`NatPhase::Failed`].
    pub fn pump(&mut self) -> NatPhase {
        if matches!(self.phase, NatPhase::Synced | NatPhase::Failed(_)) {
            return self.phase.clone();
        }
        // Drain signaling events first (they drive Registering → Exchanging).
        self.drain_signaling();
        if matches!(self.phase, NatPhase::Failed(_)) {
            return self.phase.clone();
        }

        match self.phase {
            NatPhase::Registering => self.tick_registering(),
            NatPhase::Discovering => self.tick_discovering(),
            NatPhase::Exchanging => self.tick_exchanging(),
            NatPhase::Punching => self.tick_punching(),
            NatPhase::Relaying => self.tick_relaying(),
            NatPhase::Synced | NatPhase::Failed(_) => {}
        }
        self.phase.clone()
    }

    /// Whether traversal fell back to the TURN relay (symmetric NAT) rather than
    /// completing a direct hole punch. Valid once [`pump`](Self::pump) has
    /// reached [`NatPhase::Synced`]; the resulting [`NetplayConnection`] /
    /// [`UdpTransport`] reports the same via their own `is_relayed`.
    #[must_use]
    pub const fn is_relayed(&self) -> bool {
        self.relayed
    }

    /// Consume the orchestrator, yielding the ready [`NetplayConnection`]. Only
    /// valid once [`pump`](Self::pump) has reached [`NatPhase::Synced`]; panics
    /// otherwise (call sites gate on the phase).
    ///
    /// For the **direct** path the connection's transport targets the peer's
    /// punched public address over a plain [`UdpSocket`]; for the **relay** path
    /// it targets the peer's *relayed* transport address through the
    /// [`RelayUdpSocket`]. Either way the result is a [`NetplayConnection`] whose
    /// [`UdpTransport`] presents the same plain peer-addressed datagram surface,
    /// so the same [`RollbackSession`](crate::session::RollbackSession) drives
    /// it with no second generic.
    #[must_use]
    pub fn into_connection(mut self) -> NetplayConnection {
        assert!(
            matches!(self.phase, NatPhase::Synced),
            "into_connection called before Synced"
        );
        let transport = if self.relayed {
            // Relay path: hand the RelayUdpSocket + the peer's relayed transport
            // address to a relay-backed UdpTransport.
            let relay = self
                .relay_socket
                .take()
                .expect("relayed orchestration retains its relay socket");
            let peer_relayed = self
                .peer_relayed
                .expect("relayed orchestration knows the peer's relayed address");
            UdpTransport::from_relay(relay, peer_relayed)
                .expect("relay socket reconfigured for the relayed transport")
        } else {
            // Direct path: the socket already carries the open NAT mapping; build
            // a UdpTransport fixed at the peer's punched public address.
            let socket = self
                .socket
                .take()
                .expect("synced orchestration retains its socket");
            let peer = self
                .punch
                .peer_public()
                .expect("synced orchestration knows the peer address");
            UdpTransport::from_socket(socket, peer)
                .expect("socket reconfigured for the punched transport")
        };
        NetplayConnection::with_transport(transport, self.rom_hash)
    }

    // ── phase steps ─────────────────────────────────────────────────────────

    fn drain_signaling(&mut self) {
        for ev in self.signaling.poll() {
            match ev {
                SignalEvent::Message(SignalMessage::Joined { slot, .. }) => {
                    self.slot = Some(slot);
                    if matches!(self.phase, NatPhase::Registering) {
                        self.phase = NatPhase::Discovering;
                    }
                }
                SignalEvent::Message(SignalMessage::PeerJoined { slot }) => {
                    // The first OTHER peer is the one we punch to.
                    if self.peer_slot.is_none() {
                        self.peer_slot = Some(slot);
                    }
                }
                SignalEvent::Message(SignalMessage::PublicAddr { from, addr, .. }) => {
                    if self.peer_slot.is_none() {
                        self.peer_slot = Some(from);
                    }
                    if let Ok(parsed) = addr.parse::<SocketAddr>() {
                        // Each peer sends at most TWO PublicAddrs: its public
                        // reflexive address (always), then — only on the relay
                        // fallback — its relayed transport address. The FIRST one
                        // feeds the hole punch; any SUBSEQUENT one is the relayed
                        // address (captured separately, never clobbering the
                        // punch peer). Keying off arrival order rather than our
                        // own phase is robust to the two peers entering Relaying
                        // at different times.
                        if self.punch.peer_public().is_none() {
                            self.punch.peer_discovered(parsed);
                        } else {
                            self.peer_relayed = Some(parsed);
                        }
                    }
                }
                SignalEvent::Message(SignalMessage::Error { reason }) => {
                    self.phase = NatPhase::Failed(format!("signaling: {reason}"));
                }
                SignalEvent::Closed(reason) => {
                    if !matches!(self.phase, NatPhase::Synced) {
                        self.phase = NatPhase::Failed(format!("signaling closed: {reason}"));
                    }
                }
                // `Connected` (the Join was already queued; we wait for `Joined`)
                // and browser-path SDP messages are both no-ops here.
                SignalEvent::Connected | SignalEvent::Message(_) => {}
            }
        }
    }

    fn tick_registering(&mut self) {
        if self.started.elapsed() >= SIGNALING_TIMEOUT {
            self.phase = NatPhase::Failed("signaling: no room assignment".into());
        }
    }

    fn tick_discovering(&mut self) {
        // Lazily resolve + bind a StunClient over the SAME socket so the public
        // mapping is the gameplay mapping. We take the socket out, run a bounded
        // discovery, and put it back (StunClient owns the socket while probing).
        if self.punch.local_public().is_some() {
            self.phase = NatPhase::Exchanging;
            return;
        }
        let Some(server) = self.resolve_first_stun() else {
            self.phase = NatPhase::Failed("no resolvable STUN server".into());
            return;
        };
        // Move the socket into a StunClient for the bounded probe, then reclaim.
        let Some(socket) = self.socket.take() else {
            return;
        };
        // `discover` blocks on a read timeout, which a non-blocking socket would
        // defeat (recv returns WouldBlock instantly); make it blocking for the
        // bounded probe, then restore non-blocking for the punch/gameplay path.
        let _ = socket.set_nonblocking(false);
        let mut client = StunClient::new(socket, self.rng.next_u64());
        let discovered = client.discover(server, STUN_PER_PUMP);
        // Reclaim the socket regardless of the probe outcome.
        let socket = client.into_socket();
        let _ = socket.set_nonblocking(true);
        self.socket = Some(socket);
        match discovered {
            Ok(public) => {
                self.punch.local_discovered(public);
                self.phase = NatPhase::Exchanging;
            }
            Err(_) => {
                // No response this pump; retry next pump unless we have blown the
                // overall budget.
                if self.started.elapsed() >= SIGNALING_TIMEOUT {
                    self.phase = NatPhase::Failed("STUN discovery timed out".into());
                }
            }
        }
    }

    fn tick_exchanging(&mut self) {
        // Send our public address to the peer (once we know the peer slot), then
        // wait for theirs (delivered via drain_signaling → peer_discovered).
        if let (Some(local), Some(to), false) = (
            self.punch.local_public(),
            self.peer_slot,
            self.last_addr_sent,
        ) {
            let from = self.slot.unwrap_or(0);
            self.signaling.send(SignalMessage::PublicAddr {
                from,
                to,
                addr: local.to_string(),
            });
            self.last_addr_sent = true;
        }
        // `peer_discovered` (from drain_signaling) flips HolePunch to Punching.
        if self.punch.should_punch() {
            self.phase = NatPhase::Punching;
        } else if self.started.elapsed() >= SIGNALING_TIMEOUT {
            self.phase = NatPhase::Failed("no peer public address exchanged".into());
        }
    }

    fn tick_punching(&mut self) {
        let Some(socket) = self.socket.as_ref() else {
            return;
        };
        let Some(peer) = self.punch.peer_public() else {
            return;
        };
        let now = Instant::now();
        let punch_start = *self.punch_started_at.get_or_insert(now);

        // (Re)send a punch packet (a Sync doubles as the punch) at the interval.
        let due = self
            .last_punch_sent
            .is_none_or(|t| now.saturating_duration_since(t) >= PUNCH_RESEND);
        if due {
            let pkt = NetMessage::Sync {
                magic: NetMessage::SYNC_MAGIC,
                rom_hash: self.rom_hash,
            }
            .to_bytes();
            let _ = socket.send_to(&pkt, peer);
            self.last_punch_sent = Some(now);
        }

        // Drain inbound datagrams; a packet from the peer's public address
        // confirms the mapping is open.
        let mut buf = [0u8; 1500];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((_len, from)) => {
                    if self.punch.punch_received(from) {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        if self.punch.is_connected() {
            self.phase = NatPhase::Synced;
            return;
        }

        // Punch timeout → TURN fallback (or fail if no TURN configured).
        if now.saturating_duration_since(punch_start) >= PUNCH_TIMEOUT {
            if self.cfg.turn.is_some() {
                self.phase = NatPhase::Relaying;
            } else {
                self.phase = NatPhase::Failed("hole punch failed (no TURN relay)".into());
            }
        }
    }

    fn tick_relaying(&mut self) {
        // Two sub-steps, each idempotent across pumps:
        //   1. Once, allocate a TURN relay (blocking, bounded) over the gameplay
        //      socket, install a permission for the peer, send our relayed
        //      address over signaling, and keep the RelayUdpSocket.
        //   2. Wait for the peer's relayed address (its second PublicAddr,
        //      captured into `peer_relayed`), then go Synced — at which point
        //      `into_connection` builds a relay-backed UdpTransport.
        let Some(turn_cfg) = self.cfg.turn.clone() else {
            self.phase = NatPhase::Failed("relay requested without TURN config".into());
            return;
        };

        // Sub-step 1: allocate exactly once (the RelayUdpSocket, once present,
        // owns the gameplay socket; allocation has already happened).
        if self.relay_socket.is_none() {
            let Some(socket) = self.socket.take() else {
                return;
            };
            let blocking = socket.set_nonblocking(false);
            let alloc = TurnClient::allocate(
                &socket,
                &turn_cfg,
                Duration::from_secs(5),
                self.rng.next_u64(),
            );
            let _ = socket.set_nonblocking(true);
            let _ = blocking;
            match alloc {
                Ok(mut turn) => {
                    if let Some(peer) = self.punch.peer_public() {
                        let _ = turn.create_permission(&socket, peer, Duration::from_secs(2));
                    }
                    let relayed = turn.relayed_addr();
                    self.relay_socket = Some(RelayUdpSocket::new(socket, turn));
                    self.relayed = true;
                    // Send our relayed address so the peer relays back to it.
                    if let (Some(relayed), Some(to)) = (relayed, self.peer_slot) {
                        let from = self.slot.unwrap_or(0);
                        self.signaling.send(SignalMessage::PublicAddr {
                            from,
                            to,
                            addr: relayed.to_string(),
                        });
                    }
                }
                Err(e) => {
                    self.socket = Some(socket);
                    self.phase = NatPhase::Failed(format!("TURN allocate failed: {e}"));
                    return;
                }
            }
        }

        // Sub-step 2: once both allocations are exchanged, the relay path is
        // ready end-to-end — go Synced. `into_connection` then builds the
        // relay-backed transport from `relay_socket` + `peer_relayed`.
        if self.peer_relayed.is_some() {
            self.phase = NatPhase::Synced;
        } else if self.started.elapsed() >= SIGNALING_TIMEOUT {
            self.phase = NatPhase::Failed("no peer relayed address exchanged".into());
        }
    }

    fn resolve_first_stun(&self) -> Option<SocketAddr> {
        for s in &self.cfg.stun_servers {
            // Strip an optional `stun:` scheme prefix.
            let host = s.strip_prefix("stun:").unwrap_or(s);
            if let Ok(mut addrs) = host.to_socket_addrs()
                && let Some(a) = addrs.next()
            {
                return Some(a);
            }
        }
        None
    }
}

/// Lowercase hex-encode a 32-byte hash (the ROM-hash wire form).
fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// A deterministic 6-char room code from the no-look-alike alphabet.
fn room_code(rng: &mut SplitMix64) -> String {
    let alphabet_len = u32::try_from(ROOM_ALPHABET.len()).unwrap_or(1);
    let mut s = String::with_capacity(ROOM_CODE_LEN);
    for _ in 0..ROOM_CODE_LEN {
        let idx = rng.next_below(alphabet_len) as usize;
        s.push(ROOM_ALPHABET[idx] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_code_is_deterministic_and_in_alphabet() {
        let mut a = SplitMix64::new(0x1234);
        let mut b = SplitMix64::new(0x1234);
        let ca = room_code(&mut a);
        let cb = room_code(&mut b);
        assert_eq!(ca, cb, "same seed → same code");
        assert_eq!(ca.len(), ROOM_CODE_LEN);
        assert!(
            ca.bytes().all(|c| ROOM_ALPHABET.contains(&c)),
            "code uses the no-look-alike alphabet, got {ca}"
        );
        // No look-alike characters.
        assert!(!ca.contains('0') && !ca.contains('O') && !ca.contains('1') && !ca.contains('I'));
    }

    #[test]
    fn hex_encodes_a_known_hash() {
        let mut h = [0u8; 32];
        h[0] = 0xDE;
        h[1] = 0xAD;
        h[31] = 0xFF;
        let s = hex(&h);
        assert_eq!(s.len(), 64);
        assert!(s.starts_with("dead"));
        assert!(s.ends_with("ff"));
    }
}
