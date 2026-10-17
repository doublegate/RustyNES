//! v2.6.0: the N-peer (3-4 player) UDP roster handshake + a fully-connected
//! UDP **mesh transport**.
//!
//! v2.5.0 shipped the 2-player UDP path: a host
//! [`listens`](crate::NetplayConnection::host), a joiner dials it, they
//! exchange a [`Sync`](crate::NetMessage::Sync), and the host adopts the
//! joiner's address. That is point-to-point. For 3-4 players every peer must
//! reach every *other* peer (the rollback session broadcasts its own input to
//! all others and polls all of them — a **mesh**), and a joiner has no way to
//! learn the *other joiners'* addresses on its own.
//!
//! This module adds that missing piece:
//!
//! - [`UdpMeshTransport`] — the N-peer analogue of
//!   [`UdpTransport`](crate::UdpTransport): one bound socket, a table of every
//!   *other* peer's `(player, addr)`. [`send`](crate::Transport::send) fans the
//!   message out to all of them; [`poll`](crate::Transport::poll) drains the
//!   socket once and decodes every datagram. Malformed / foreign datagrams are
//!   dropped, never panicked on; per-poll work is capped.
//! - [`MeshHost`] — the host orchestrator. It listens, **adopts up to
//!   `num_players - 1` joiners** from their `Sync`s, assigns each the lowest
//!   free player index, and once the roster is full **distributes the full
//!   roster** ([`NetMessage::Roster`]) to every joiner. Then
//!   it yields a [`UdpMeshTransport`] for the host (player 0) wired to every
//!   joiner. v3.4.0: while it gathers it answers each adopted joiner's `Sync`
//!   with an accepted zero-share `Sync` ("adopted, still gathering"), and it
//!   drops a joiner that stops re-sending its `Sync` (it went away) instead of
//!   starting a session with it.
//! - [`MeshJoiner`] — the joiner orchestrator. It dials the host, `Sync`s, then
//!   **waits for the roster**; on receipt it builds a [`UdpMeshTransport`] wired
//!   to the host and every *other* joiner (skipping its own entry). Once the
//!   host has answered that it is gathering, the joiner waits for as long as
//!   the host keeps answering, up to its gather timeout.
//!
//! # Determinism boundary
//!
//! Exactly as for [`NetplayConnection`](crate::NetplayConnection): all
//! wall-clock / socket I/O lives here, never in the
//! [`RollbackSession`](crate::session::RollbackSession). The roster handshake is
//! plain host-side orchestration; once the mesh transport exists the session
//! drives it with no change, and the byte-identical replay is unperturbed.
//!
//! # Robustness
//!
//! Every inbound datagram is parsed with
//! [`NetMessage::from_bytes`](crate::NetMessage::from_bytes); anything malformed,
//! truncated, foreign-version, or duplicate is dropped. A duplicate `Sync` from
//! an already-adopted joiner does **not** re-adopt it or shift indices (idempotent
//! adoption keyed by source address). A `Sync` carrying a mismatched ROM hash is
//! rejected. None of these paths panic.

use std::collections::BTreeMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use crate::auth::{DatagramAuth, KeyShare, Refusal, fresh_key_share};
use crate::connection::same_peer;
use crate::message::{IdentityMismatch, KEY_SHARE_LEN, NetMessage, SessionIdentity, SyncVerdict};
use crate::transport::Transport;

/// The shortest interval between two handshake replies a sealed mesh
/// transport sends to one peer (see [`UdpMeshTransport::new_sealed`]).
const HANDSHAKE_REPLY_INTERVAL: Duration = Duration::from_millis(100);

/// Largest datagram read in one `recv_from`. The longest [`NetMessage`] is a
/// full 4-peer `Roster` (tag + count + 4×(player + v6-addr-19) ≈ 82 bytes) or
/// the 37-byte `Sync`; 1500 covers a standard MTU with headroom.
const RECV_BUF_LEN: usize = 1500;

/// Maximum datagrams drained in a single poll. UDP is hostile input; cap the
/// per-poll drain so a flood cannot spin the loop unbounded.
const MAX_DATAGRAMS_PER_POLL: usize = 1024;

/// A [`Transport`] over a non-blocking UDP socket talking to **several** remote
/// peers (the N-peer mesh).
///
/// `send` fans the message out to every peer in the table; `poll` drains the
/// socket once and decodes every datagram.
///
/// This is the multi-peer analogue of [`UdpTransport`](crate::UdpTransport):
/// where that targets one fixed remote, this broadcasts to all the *other*
/// players' gameplay addresses learned from the roster handshake
/// ([`MeshHost`] / [`MeshJoiner`]). The rollback session already tags each
/// outgoing `Input` with its own `player` index, so a recipient routes it to the
/// right controller port regardless of which socket it arrived on.
#[derive(Debug)]
pub struct UdpMeshTransport {
    socket: UdpSocket,
    /// Every *other* peer's gameplay address (this peer is excluded). `send`
    /// fans out to all of these.
    peers: Vec<SocketAddr>,
    /// Count of datagrams that failed to parse. Diagnostic only.
    dropped_invalid: u64,
    /// v3.4.0 (protocol 9, ADR 0046) — the datagram key and what goes with
    /// it, for a transport built by the sealed handshake.
    sealed: Option<Box<SealedMesh>>,
    /// Datagrams refused by the seal (unsealed, forged, replayed).
    refused: u64,
    /// Times a peer was followed to a new source address.
    roamed: u64,
    /// Sealed datagrams that opened but named a player other than their
    /// sender (see [`Self::misattributed_datagrams`]).
    misattributed: u64,
}

/// The sealing state of a [`UdpMeshTransport`].
#[derive(Debug)]
struct SealedMesh {
    auth: DatagramAuth,
    /// The sender index of each entry of `peers`, so a datagram that opens
    /// can move its sender's address (a NAT rebinding).
    senders: Vec<u8>,
    /// The player index of each entry of `peers`: what a message from that
    /// sender may name as its `player`. Equal to `senders` in a mesh, where
    /// the sender index IS the player index; on a joiner's 2-player fallback
    /// the host is player 0 whatever its pair sender index.
    players: Vec<u8>,
    /// What to answer an unsealed `Sync` from a peer's address with: the
    /// host's roster (a joiner that lost it is still handshaking). `None` on a
    /// joiner.
    handshake_reply: Option<Vec<u8>>,
    /// When each peer was last answered, rate-limiting the replies.
    last_reply: Vec<Option<Instant>>,
}

impl UdpMeshTransport {
    /// Wrap an already-bound socket and the set of *other* peers' addresses.
    /// The socket is set non-blocking.
    ///
    /// # Errors
    ///
    /// Returns any error from setting the socket non-blocking.
    pub fn new(socket: UdpSocket, peers: Vec<SocketAddr>) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            peers,
            dropped_invalid: 0,
            sealed: None,
            refused: 0,
            roamed: 0,
            misattributed: 0,
        })
    }

    /// v3.4.0 (protocol 9) — a transport that seals every datagram it sends
    /// and opens every one it receives with `auth` (see `crate::auth`).
    /// `peers` lists every *other* peer's `(sender index, address)`.
    ///
    /// A datagram that does not open is refused before it is parsed, whatever
    /// its source; one that opens is accepted from any source and moves its
    /// sender's address there (a NAT rebinding). The one unsealed datagram
    /// answered is a `Sync` from a peer's address, with `handshake_reply` (the
    /// host's roster) at most every 100 ms per peer: that joiner lost the
    /// roster and is still handshaking.
    ///
    /// # Errors
    ///
    /// Returns any error from setting the socket non-blocking.
    ///
    /// In a mesh the sender index is the player index (ADR 0046), so a
    /// datagram from sender `s` that names a player (`Input`, `InputBatch`,
    /// `InputAck`) must name player `s`; one that names another seat is
    /// dropped and counted ([`Self::misattributed_datagrams`]).
    pub fn new_sealed(
        socket: UdpSocket,
        peers: &[(u8, SocketAddr)],
        auth: DatagramAuth,
        handshake_reply: Option<Vec<u8>>,
    ) -> io::Result<Self> {
        let seats: Vec<(u8, u8, SocketAddr)> = peers.iter().map(|&(s, a)| (s, s, a)).collect();
        Self::sealed_with_seats(socket, &seats, auth, handshake_reply)
    }

    /// [`Self::new_sealed`] with each peer's player index given apart from
    /// its sender index: `(sender, player, address)`. Only the joiner's
    /// 2-player fallback needs it, a pair key numbering its senders by share
    /// order rather than by seat.
    fn sealed_with_seats(
        socket: UdpSocket,
        peers: &[(u8, u8, SocketAddr)],
        auth: DatagramAuth,
        handshake_reply: Option<Vec<u8>>,
    ) -> io::Result<Self> {
        let senders: Vec<u8> = peers.iter().map(|&(s, _, _)| s).collect();
        let players: Vec<u8> = peers.iter().map(|&(_, p, _)| p).collect();
        let addrs: Vec<SocketAddr> = peers.iter().map(|&(_, _, a)| a).collect();
        let mut t = Self::new(socket, addrs)?;
        t.sealed = Some(Box::new(SealedMesh {
            auth,
            last_reply: vec![None; senders.len()],
            senders,
            players,
            handshake_reply,
        }));
        Ok(t)
    }

    /// `true` if this transport seals its datagrams.
    #[must_use]
    pub const fn is_sealed(&self) -> bool {
        self.sealed.is_some()
    }

    /// Datagrams refused by the seal. Diagnostic only.
    #[must_use]
    pub const fn refused_datagrams(&self) -> u64 {
        self.refused
    }

    /// Times a peer was followed to a new source address. Diagnostic only.
    #[must_use]
    pub const fn roamed(&self) -> u64 {
        self.roamed
    }

    /// v3.4.0 — sealed datagrams that opened under one peer's key but spoke
    /// for another seat (an `Input`, `InputBatch` or `InputAck` whose `player`
    /// is not the sender's), and were dropped. Diagnostic only.
    #[must_use]
    pub const fn misattributed_datagrams(&self) -> u64 {
        self.misattributed
    }

    /// One received datagram as a message, if it is one.
    fn accept_datagram(&mut self, data: &[u8], from: SocketAddr) -> Option<NetMessage> {
        let Some(sealed) = self.sealed.as_mut() else {
            let msg = NetMessage::from_bytes(data);
            if msg.is_none() {
                self.dropped_invalid = self.dropped_invalid.saturating_add(1);
            }
            return msg;
        };
        match sealed.auth.open(data) {
            Ok((sender, payload)) => {
                let msg = NetMessage::from_bytes(payload);
                if msg.is_none() {
                    self.dropped_invalid = self.dropped_invalid.saturating_add(1);
                    return None;
                }
                // The seal names the sender; a key exists only for a roster
                // member, and every other member is in `senders`.
                let Some(i) = sealed.senders.iter().position(|&s| s == sender) else {
                    self.refused = self.refused.saturating_add(1);
                    return None;
                };
                // v3.4.0 (Cursor security review on #613): a message that
                // names a player speaks for that seat, and the session takes
                // the first confirmed input of a cell as final and an ack as
                // a reason to stop retransmitting. So it must name its own
                // sender's seat: without this a mesh peer could seal another
                // player's input or ack under its own key. A 2-player pair
                // has only one remote seat, so there it changes nothing.
                if msg
                    .as_ref()
                    .and_then(seat_named)
                    .is_some_and(|p| p != sealed.players[i])
                {
                    self.misattributed = self.misattributed.saturating_add(1);
                    return None;
                }
                if !same_peer(self.peers[i], from) {
                    self.peers[i] = from;
                    self.roamed = self.roamed.saturating_add(1);
                }
                msg
            }
            Err(why) => {
                self.refused = self.refused.saturating_add(1);
                let now = Instant::now();
                if why == Refusal::Unsealed
                    && data.first() == Some(&NetMessage::TAG_SYNC)
                    && let Some(reply) = sealed.handshake_reply.as_ref()
                    && let Some(i) = self.peers.iter().position(|&a| same_peer(a, from))
                    && sealed.last_reply[i].is_none_or(|t| {
                        now.saturating_duration_since(t) >= HANDSHAKE_REPLY_INTERVAL
                    })
                {
                    sealed.last_reply[i] = Some(now);
                    let _ = self.socket.send_to(reply, self.peers[i]);
                }
                None
            }
        }
    }

    /// The local address the socket is bound to.
    ///
    /// # Errors
    ///
    /// Returns any error from the underlying `local_addr` call.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// The set of *other* peers this transport broadcasts to.
    // `Vec::as_slice` is not yet const-stable, so this cannot be a `const fn`
    // despite clippy's `missing_const_for_fn` suggestion.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn peers(&self) -> &[SocketAddr] {
        &self.peers
    }

    /// Total datagrams dropped for being malformed / truncated / foreign
    /// version. Diagnostic only.
    #[must_use]
    pub const fn dropped_invalid(&self) -> u64 {
        self.dropped_invalid
    }
}

impl Transport for UdpMeshTransport {
    fn send(&mut self, msg: &NetMessage) {
        let mut bytes = msg.to_bytes();
        // v3.4.0 — sealed once, the same datagram to every peer: each keeps
        // its own replay window for this sender.
        if let Some(sealed) = self.sealed.as_mut() {
            bytes = sealed.auth.seal(&bytes);
        }
        for peer in &self.peers {
            // A failed send is non-fatal (the rollback protocol tolerates loss
            // and resends); swallow it rather than panic.
            let _ = self.socket.send_to(&bytes, peer);
        }
    }

    fn poll(&mut self) -> Vec<NetMessage> {
        let mut out = Vec::new();
        let mut buf = [0u8; RECV_BUF_LEN];
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            match self.socket.recv_from(&mut buf) {
                Ok((len, from)) => {
                    if let Some(msg) = self.accept_datagram(&buf[..len], from) {
                        out.push(msg);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // Windows can surface a prior send's ICMP unreachable as a
                // ConnectionReset on the next recv — not fatal, keep draining.
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(_) => break,
            }
        }
        out
    }
}

/// Why a mesh handshake ended without completing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// Not all expected joiners completed their handshake before the timeout.
    Timeout,
    /// A joiner / the host announced a different ROM hash.
    RomMismatch,
    /// v2.9.8 — a joiner / the host runs the same ROM on a differently
    /// configured machine (emulation options, region or header).
    ConfigMismatch,
    /// v3.0.0 (ADR 0045) — a joiner / the host runs a version of `RustyNES` that
    /// emulates differently.
    EmulatorMismatch {
        /// This build's emulation epoch.
        ours: u32,
        /// The peer's epoch; `None` when it predates the epoch.
        theirs: Option<u32>,
    },
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("mesh handshake timed out before all joiners arrived"),
            Self::RomMismatch => f.write_str("a peer announced a different ROM"),
            Self::ConfigMismatch => f.write_str(
                "a peer runs this ROM with different emulation settings (console model, \
                 power-on RAM, die revisions, overclock, Four Score, Vs. settings, Game \
                 Genie codes, region or header); match them and reconnect",
            ),
            Self::EmulatorMismatch { ours, theirs } => {
                f.write_str(&crate::message::emulator_mismatch_text(*ours, *theirs))
            }
        }
    }
}

impl std::error::Error for MeshError {}

impl From<IdentityMismatch> for MeshError {
    fn from(m: IdentityMismatch) -> Self {
        match m {
            IdentityMismatch::Emulator { ours, theirs } => Self::EmulatorMismatch { ours, theirs },
            IdentityMismatch::Rom => Self::RomMismatch,
            IdentityMismatch::Config => Self::ConfigMismatch,
        }
    }
}

/// The host side of the N-peer UDP roster handshake.
///
/// The host binds a listening socket and [`pump`](Self::pump)s it. Each valid
/// `Sync` from a new source is **adopted** as the next joiner (player 1, 2, …),
/// idempotently (a duplicate `Sync` from an already-known joiner does not
/// re-adopt). Once `num_players - 1` joiners are adopted the host broadcasts the
/// full [`NetMessage::Roster`] — including the host's
/// own player-0 entry — to every joiner, and [`pump`](Self::pump) returns
/// `Ok(Some(transport))` with a [`UdpMeshTransport`] wired to every joiner. The
/// roster is re-sent a few times to ride out loss.
pub struct MeshHost {
    socket: Option<UdpSocket>,
    identity: SessionIdentity,
    num_players: u8,
    /// `addr -> (player index, key share, last heard)` (the host itself is
    /// player 0, never in this map). v3.4.0: the share is the one the
    /// joiner's first adopting `Sync` carried, and "last heard" is its most
    /// recent `Sync`, which drops a joiner that went away while the host
    /// gathered.
    joiners: BTreeMap<SocketAddr, (u8, KeyShare, Instant)>,
    /// v3.4.0 (protocol 9) — the host's own key share (player 0's).
    key_share: KeyShare,
    /// Host's own gameplay address (its bound local addr; for a roster a loopback
    /// or LAN test uses this directly, an internet deployment substitutes the
    /// STUN-discovered public addr — see `docs/netplay-webrtc.md`).
    host_addr: SocketAddr,
    started: Instant,
    timeout: Duration,
    /// How many extra roster re-broadcasts remain after the roster filled.
    roster_resends: u8,
    last_roster_sent: Option<Instant>,
    /// v3.4.0 — how long a gathering joiner may go unheard before the host
    /// drops it (see [`Self::JOINER_SILENCE`]).
    joiner_silence: Duration,
}

impl MeshHost {
    /// Default time to wait for all joiners.
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    /// Re-broadcast the roster this many times (loss tolerance).
    const ROSTER_RESENDS: u8 = 5;
    /// Interval between roster re-broadcasts.
    const ROSTER_RESEND_INTERVAL: Duration = Duration::from_millis(50);
    /// v3.4.0 — a gathering joiner re-sends its `Sync` every 50 ms until the
    /// roster arrives, so three seconds without one (60 resends) means it has
    /// gone: the host drops it rather than start a session with it.
    const JOINER_SILENCE: Duration = Duration::from_secs(3);

    /// Bind a listening host socket on `local` and prepare to gather
    /// `num_players - 1` joiners. `host_gameplay_addr` is the address joiners
    /// will be told to send to for the host (on loopback/LAN this is the bound
    /// local address; on the internet, the STUN-discovered public address).
    ///
    /// # Errors
    ///
    /// Returns any socket bind / configuration error.
    pub fn bind(
        local: SocketAddr,
        host_gameplay_addr: SocketAddr,
        num_players: u8,
        identity: SessionIdentity,
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: Some(socket),
            identity,
            num_players: num_players.clamp(2, 4),
            joiners: BTreeMap::new(),
            key_share: fresh_key_share(),
            host_addr: host_gameplay_addr,
            started: Instant::now(),
            timeout: Self::DEFAULT_TIMEOUT,
            roster_resends: Self::ROSTER_RESENDS,
            last_roster_sent: None,
            joiner_silence: Self::JOINER_SILENCE,
        })
    }

    /// Override the gather timeout (default 30s). Builder-style.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The local address the listening socket is bound to (resolves an ephemeral
    /// `:0` port). Joiners dial this.
    ///
    /// # Errors
    ///
    /// Returns any error from the underlying `local_addr` call, or if the socket
    /// has already been consumed into the transport.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "socket already taken"))?
            .local_addr()
    }

    /// How many joiners have been adopted so far.
    #[must_use]
    pub fn joiners_ready(&self) -> usize {
        self.joiners.len()
    }

    /// `true` once every expected joiner has been adopted.
    #[must_use]
    pub fn roster_full(&self) -> bool {
        self.joiners.len() + 1 >= self.num_players as usize
    }

    /// The number of players the host gathers (2..=4).
    #[must_use]
    pub const fn num_players(&self) -> u8 {
        self.num_players
    }

    /// Build the full roster (host first, then joiners in adoption order),
    /// each entry with its key share.
    fn build_roster(&self) -> Vec<(u8, SocketAddr, KeyShare)> {
        let mut peers = Vec::with_capacity(self.num_players as usize);
        peers.push((0u8, self.host_addr, self.key_share));
        // BTreeMap iterates by address; sort the emitted list by player index
        // instead so the roster is stable + index-ordered.
        let mut joiners: Vec<(u8, SocketAddr, KeyShare)> = self
            .joiners
            .iter()
            .map(|(&a, &(p, share, _))| (p, a, share))
            .collect();
        joiners.sort_by_key(|&(p, _, _)| p);
        peers.extend(joiners);
        peers
    }

    /// v3.4.0 — drop a joiner that stopped re-sending its `Sync` while the
    /// host gathered (closed, or gave up), so the session never starts with a
    /// peer that is gone; its seat goes to the next joiner. Only before the
    /// roster goes out: after that the joiners stop sending `Sync`s by design.
    fn drop_silent_joiners(&mut self, now: Instant) {
        if self.last_roster_sent.is_none() {
            let silence = self.joiner_silence;
            self.joiners
                .retain(|_, &mut (_, _, heard)| now.saturating_duration_since(heard) < silence);
        }
    }

    /// Drive the handshake one step: drain the socket, adopt new joiners, and —
    /// once the roster is full — broadcast it and return the host's
    /// [`UdpMeshTransport`].
    ///
    /// Returns:
    /// - `Ok(None)` while still gathering joiners (call again).
    /// - `Ok(Some(transport))` once the roster filled, was broadcast, and the
    ///   host's mesh transport is ready.
    /// - `Err(MeshError::RomMismatch)` if a joiner announced a different ROM.
    /// - `Err(MeshError::Timeout)` if the gather timeout elapsed first.
    ///
    /// # Errors
    ///
    /// See above; also surfaces a socket error only via the `MeshError` mapping
    /// (I/O failures are non-fatal and swallowed, matching the UDP transport).
    pub fn pump(&mut self) -> Result<Option<UdpMeshTransport>, MeshError> {
        let now = Instant::now();

        // Drain inbound datagrams (with source, so we can adopt joiners).
        let mut buf = [0u8; RECV_BUF_LEN];
        let socket = self.socket.as_ref().expect("host socket present in pump");
        let mut inbound: Vec<(NetMessage, SocketAddr)> = Vec::new();
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            match socket.recv_from(&mut buf) {
                Ok((len, from)) => {
                    if let Some(msg) = NetMessage::from_bytes(&buf[..len]) {
                        inbound.push((msg, from));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(_) => break,
            }
        }

        for (msg, from) in inbound {
            if let NetMessage::Sync {
                magic,
                identity,
                key_share,
            } = msg
            {
                match self.identity.check_sync(magic, &identity) {
                    SyncVerdict::Ignore => continue,
                    SyncVerdict::Accept => {}
                    SyncVerdict::Refuse(m) => {
                        // Answer with our own identity before refusing, so
                        // the joiner can name the disagreement too. Without
                        // it the joiner, which waits for a `Sync` or a
                        // `Roster`, heard nothing and reported a bare timeout
                        // (Copilot on #586): the silent failure ADR 0045
                        // exists to remove. Its `pump` already refuses a
                        // mismatched `Sync` with the reason. A peer older than
                        // protocol 6 ignores this datagram's magic, so it is
                        // no worse off than before.
                        let reply = NetMessage::Sync {
                            magic: NetMessage::SYNC_MAGIC,
                            identity: self.identity,
                            key_share: [0; KEY_SHARE_LEN],
                        }
                        .to_bytes();
                        let _ = socket.send_to(&reply, from);
                        return Err(MeshError::from(m));
                    }
                }
                // v3.4.0: only a `Sync` with a key share is a joiner's
                // handshake; one without (a stray in-session `Sync`) adopts
                // no one and is not proof of life.
                if key_share == [0; KEY_SHARE_LEN] {
                    continue;
                }
                if let Some(joiner) = self.joiners.get_mut(&from) {
                    // A known joiner re-sending its Sync: alive. Idempotent —
                    // its index and share stay as first adopted.
                    joiner.2 = now;
                } else if !self.roster_full() {
                    // Adopt this source as a new joiner, in the lowest free
                    // seat (a dropped joiner's seat is reused, so the full
                    // roster is always exactly players 0..n).
                    let seat = (1..self.num_players)
                        .find(|s| self.joiners.values().all(|&(p, _, _)| p != *s))
                        .unwrap_or(u8::MAX);
                    self.joiners.insert(from, (seat, key_share, now));
                } else {
                    continue;
                }
                // v3.4.0 (Codex and CodeRabbit on #613): tell a gathering
                // joiner it is adopted and the host is alive, with an
                // accepted zero-share `Sync` (the share is in the roster to
                // come). Until then the host sent nothing before the roster
                // was full, so the first joiner of a 3-4 player host heard
                // nothing for as long as the others took and timed out at
                // its own handshake timeout. Once the roster is full the
                // roster itself is the answer.
                if !self.roster_full() {
                    let gathering = NetMessage::Sync {
                        magic: NetMessage::SYNC_MAGIC,
                        identity: self.identity,
                        key_share: [0; KEY_SHARE_LEN],
                    }
                    .to_bytes();
                    let _ = socket.send_to(&gathering, from);
                }
            }
            // Non-Sync traffic during the handshake is ignored (early/stray).
        }

        self.drop_silent_joiners(now);

        // Once the roster is full, broadcast it (initial + a few resends) and
        // then hand back the host's mesh transport.
        if self.roster_full() {
            let due = self
                .last_roster_sent
                .is_none_or(|t| now.saturating_duration_since(t) >= Self::ROSTER_RESEND_INTERVAL);
            if due && self.roster_resends > 0 {
                let roster = NetMessage::Roster {
                    peers: self.build_roster(),
                };
                let bytes = roster.to_bytes();
                let socket = self.socket.as_ref().expect("host socket present");
                for &joiner in self.joiners.keys() {
                    let _ = socket.send_to(&bytes, joiner);
                }
                self.last_roster_sent = Some(now);
                self.roster_resends -= 1;
            }
            // v3.2.0: hand off only after the LAST resend. The resends exist to
            // ride out a lost roster, and they are sent from this pump; once
            // the host has its transport nothing pumps this handshake again.
            // Until v3.2.0 it handed off after the first broadcast, so the
            // other four were never sent and one lost datagram stranded a
            // joiner. Cost: the host starts about
            // (ROSTER_RESENDS - 1) x ROSTER_RESEND_INTERVAL = 200 ms later.
            if self.roster_resends > 0 {
                return Ok(None);
            }
            // v3.4.0 (protocol 9): every peer's share, in player order, keys
            // the mesh; the host is sender 0 and each joiner its player index.
            let roster = self.build_roster();
            let shares: Vec<(u8, KeyShare)> = roster.iter().map(|&(p, _, k)| (p, k)).collect();
            let Some(auth) = DatagramAuth::mesh(&self.identity, &shares, 0) else {
                return Err(MeshError::Timeout);
            };
            let peers: Vec<(u8, SocketAddr)> =
                self.joiners.iter().map(|(&a, &(p, _, _))| (p, a)).collect();
            let reply = NetMessage::Roster { peers: roster }.to_bytes();
            let socket = self.socket.take().expect("host socket present to hand off");
            // We never reach here without at least the resend above; map an I/O
            // error on the (already-configured non-blocking) socket to a Timeout
            // rather than panic.
            return UdpMeshTransport::new_sealed(socket, &peers, auth, Some(reply))
                .map(Some)
                .map_err(|_| MeshError::Timeout);
        }

        // Still gathering. Enforce the timeout.
        if now.saturating_duration_since(self.started) >= self.timeout {
            return Err(MeshError::Timeout);
        }
        Ok(None)
    }
}

/// The joiner side of the N-peer UDP roster handshake.
///
/// The joiner binds an ephemeral socket, sends its `Sync` to the host, and
/// [`pump`](Self::pump)s. It re-sends `Sync` periodically until the host's
/// [`Roster`](crate::NetMessage::Roster) arrives, then builds a
/// [`UdpMeshTransport`] wired to the host + every *other* joiner.
///
/// v3.4.0 (protocol 9): it finds its own roster entry by its own key share,
/// which every entry carries and which the host copied from this joiner's
/// `Sync`, so it works through a NAT that rewrote the joiner's source address
/// (the address the host lists for it is one the joiner cannot observe). The
/// match on its bound address and the `my_player` index given at
/// [`connect`](Self::connect) remain only as fallbacks no honest host
/// reaches. An accepted `Sync` with a key share from the dialed host is a
/// 2-player host's answer instead, and the joiner runs as player 1 of 2.
///
/// Timeouts: [`with_timeout`](Self::with_timeout) bounds the wait for the
/// host's first answer (a dead address fails fast) and, after it, any silence
/// of the host; [`with_gather_timeout`](Self::with_gather_timeout) bounds the
/// whole wait for a 3-4 player host that has answered it is still gathering.
pub struct MeshJoiner {
    socket: Option<UdpSocket>,
    host: SocketAddr,
    identity: SessionIdentity,
    /// This joiner's own player index (assigned by the host scheme; the frontend
    /// supplies it, or it is discovered from the roster — see module docs).
    my_player: u8,
    /// v3.4.0 — how many players the session has, learned from the roster
    /// (or 2 when the host turned out to be a 2-player host). `0` until then.
    num_players: u8,
    /// v3.4.0 (protocol 9) — this joiner's key share, sent in every `Sync`.
    key_share: KeyShare,
    started: Instant,
    timeout: Duration,
    last_sync_sent: Option<Instant>,
    /// v3.4.0 — the longest the joiner waits once its host has answered (a
    /// mesh host gathering its other joiners); see [`Self::with_gather_timeout`].
    gather_timeout: Duration,
    /// v3.4.0 — when the host last answered a `Sync` with an accepted
    /// zero-share `Sync` ("adopted, still gathering"). `None` until it does.
    last_host_answer: Option<Instant>,
}

impl MeshJoiner {
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    /// The default gather wait, the host's own default
    /// (`MeshHost::DEFAULT_TIMEOUT`).
    const DEFAULT_GATHER_TIMEOUT: Duration = MeshHost::DEFAULT_TIMEOUT;
    const SYNC_RESEND_INTERVAL: Duration = Duration::from_millis(50);

    /// Bind an ephemeral joiner socket, dial `host`, and send the opening
    /// `Sync`. `my_player` is this joiner's player index (1..=3).
    ///
    /// # Errors
    ///
    /// Returns any socket bind / configuration error.
    pub fn connect(
        local: SocketAddr,
        host: SocketAddr,
        my_player: u8,
        identity: SessionIdentity,
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        socket.set_nonblocking(true)?;
        let key_share = fresh_key_share();
        let sync = NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity,
            key_share,
        }
        .to_bytes();
        let _ = socket.send_to(&sync, host);
        let now = Instant::now();
        Ok(Self {
            socket: Some(socket),
            host,
            identity,
            my_player,
            num_players: 0,
            key_share,
            started: now,
            timeout: Self::DEFAULT_TIMEOUT,
            last_sync_sent: Some(now),
            gather_timeout: Self::DEFAULT_GATHER_TIMEOUT,
            last_host_answer: None,
        })
    }

    /// Override the wait timeout (default 30s). Builder-style.
    ///
    /// v3.4.0: this is how long the joiner waits for its host to answer at
    /// all, and, once a mesh host has answered, how long it may then fall
    /// silent. The whole wait for a gathering host is bounded by
    /// [`Self::with_gather_timeout`] instead.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// v3.4.0 — the longest the joiner waits, from [`Self::connect`], once a
    /// mesh host has answered that it is gathering its other joiners (default
    /// 30 s, the host's default). Set it to the host's gather timeout.
    /// Builder-style.
    #[must_use]
    pub const fn with_gather_timeout(mut self, timeout: Duration) -> Self {
        self.gather_timeout = timeout;
        self
    }

    /// v3.4.0 — `true` once the host has answered: a mesh host that adopted
    /// this joiner and is gathering the rest. A timeout after this means the
    /// session never filled (or the host went away), not that no host
    /// answered.
    #[must_use]
    pub const fn host_answered(&self) -> bool {
        self.last_host_answer.is_some()
    }

    /// The local bound address (resolves an ephemeral `:0`).
    ///
    /// # Errors
    ///
    /// Returns any error from the underlying `local_addr` call, or if the socket
    /// has already been consumed into the transport.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "socket already taken"))?
            .local_addr()
    }

    /// This joiner's assigned player index.
    #[must_use]
    pub const fn my_player(&self) -> u8 {
        self.my_player
    }

    /// v3.4.0 — the number of players in the session, once
    /// [`pump`](Self::pump) has returned the transport (`0` before).
    #[must_use]
    pub const fn num_players(&self) -> u8 {
        self.num_players
    }

    /// Drive the joiner one step: re-send `Sync`, watch for the host's `Roster`,
    /// and on receipt build the [`UdpMeshTransport`] (wired to the host + every
    /// other joiner, skipping this joiner's own entry).
    ///
    /// Returns `Ok(None)` while waiting, `Ok(Some(transport))` once the roster
    /// arrived, `Err(MeshError::RomMismatch)` on a bad-ROM `Sync`, or
    /// `Err(MeshError::Timeout)` if the wait elapsed.
    ///
    /// # Errors
    ///
    /// See above.
    pub fn pump(&mut self) -> Result<Option<UdpMeshTransport>, MeshError> {
        let now = Instant::now();

        let mut buf = [0u8; RECV_BUF_LEN];
        let socket = self.socket.as_ref().expect("joiner socket present in pump");
        let mut roster: Option<Vec<(u8, SocketAddr, KeyShare)>> = None;
        let mut pair_host_share: Option<KeyShare> = None;
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            match socket.recv_from(&mut buf) {
                Ok((len, from)) => match NetMessage::from_bytes(&buf[..len]) {
                    // The roster arrives BEFORE any key exists, so it is
                    // untrusted input from anyone who can reach this port:
                    // only the host we dialed may send it, and only a roster
                    // whose players are exactly 0..n (2 <= n <= 4) is kept. A
                    // gap (`{0, 3}`) would otherwise make a 2-player session
                    // whose local player is 3, which the session indexes out
                    // of range in a release build.
                    Some(NetMessage::Roster { peers })
                        if same_peer(from, self.host) && roster_is_well_formed(&peers) =>
                    {
                        roster = Some(peers);
                    }
                    Some(NetMessage::Sync {
                        magic,
                        identity,
                        key_share,
                    }) => match self.identity.check_sync(magic, &identity) {
                        SyncVerdict::Refuse(m) => return Err(MeshError::from(m)),
                        // v3.4.0 — an accepted `Sync` with a key share, from
                        // the host we dialed, is a 2-player host's answer
                        // (`NetplayConnection::host` echoes the joiner's
                        // `Sync`; a mesh host only ever sends a roster). The
                        // joiner then runs as player 1 of 2 over the pair key.
                        SyncVerdict::Accept
                            if key_share != [0; KEY_SHARE_LEN] && same_peer(from, self.host) =>
                        {
                            pair_host_share.get_or_insert(key_share);
                        }
                        // v3.4.0 — an accepted zero-share `Sync` from the
                        // host is a mesh host's "adopted, still gathering"
                        // (`MeshHost::pump`): the host is alive, so keep
                        // waiting for the roster.
                        SyncVerdict::Accept if same_peer(from, self.host) => {
                            self.last_host_answer = Some(now);
                        }
                        _ => {}
                    },
                    _ => {}
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(_) => break,
            }
        }

        if let Some(mut peers) = roster {
            // v3.4.0 — the host is reached where this joiner reached it. The
            // host's own roster entry is the address it believes it has,
            // which for a host bound to `0.0.0.0` is no address at all; the
            // one the joiner dialed is the one known to work.
            for entry in &mut peers {
                if entry.0 == 0 {
                    entry.1 = self.host;
                }
            }
            // Identify our own entry in the roster. v3.4.0: by our own key
            // share, which works through a NAT; failing that (it cannot, from
            // an honest host), on loopback / LAN by our bound source address,
            // which the host saw unrewritten; failing both, the `my_player`
            // index assigned out of band at connect (see module docs).
            let my_local = self.socket.as_ref().and_then(|s| s.local_addr().ok());
            if let Some(&(p, _, _)) = peers.iter().find(|&&(_, _, k)| k == self.key_share) {
                self.my_player = p;
            } else if let Some(local) = my_local
                && let Some(&(p, _, _)) = peers.iter().find(|&&(_, a, _)| a == local)
            {
                self.my_player = p;
            }
            let shares: Vec<(u8, KeyShare)> = peers.iter().map(|&(p, _, k)| (p, k)).collect();
            let Some(auth) = DatagramAuth::mesh(&self.identity, &shares, self.my_player) else {
                // A roster that does not form a mesh with us in it (a
                // duplicate index, a missing share): wait for a good one.
                return Ok(None);
            };
            self.num_players = u8::try_from(peers.len()).unwrap_or(u8::MAX);
            // The mesh is every peer EXCEPT this joiner's own index.
            let others: Vec<(u8, SocketAddr)> = peers
                .iter()
                .filter(|&&(p, _, _)| p != self.my_player)
                .map(|&(p, a, _)| (p, a))
                .collect();
            let socket = self
                .socket
                .take()
                .expect("joiner socket present to hand off");
            return UdpMeshTransport::new_sealed(socket, &others, auth, None)
                .map(Some)
                .map_err(|_| MeshError::Timeout);
        }

        if let Some(theirs) = pair_host_share
            && let Some(auth) = DatagramAuth::pair(&self.identity, self.key_share, theirs)
        {
            self.my_player = 1;
            self.num_players = 2;
            let seats = [pair_fallback_seat(&auth, self.host)];
            let socket = self
                .socket
                .take()
                .expect("joiner socket present to hand off");
            return UdpMeshTransport::sealed_with_seats(socket, &seats, auth, None)
                .map(Some)
                .map_err(|_| MeshError::Timeout);
        }

        self.resend_sync(now);
        if self.expired(now) {
            return Err(MeshError::Timeout);
        }
        Ok(None)
    }

    /// Re-send `Sync` periodically until the roster arrives. It is also the
    /// joiner's proof of life to a gathering host (`MeshHost::JOINER_SILENCE`).
    fn resend_sync(&mut self, now: Instant) {
        let due = self
            .last_sync_sent
            .is_none_or(|t| now.saturating_duration_since(t) >= Self::SYNC_RESEND_INTERVAL);
        if due && let Some(socket) = self.socket.as_ref() {
            let sync = NetMessage::Sync {
                magic: NetMessage::SYNC_MAGIC,
                identity: self.identity,
                key_share: self.key_share,
            }
            .to_bytes();
            let _ = socket.send_to(&sync, self.host);
            self.last_sync_sent = Some(now);
        }
    }

    /// v3.4.0 — whether the wait is over. Until the host answers, `timeout`
    /// from connect (a dead address fails fast). Once a mesh host has said it
    /// is gathering, the joiner waits while the host keeps answering: it fails
    /// if the host falls silent for `timeout`, and in any case at
    /// `gather_timeout` from connect.
    fn expired(&self, now: Instant) -> bool {
        self.last_host_answer.map_or_else(
            || now.saturating_duration_since(self.started) >= self.timeout,
            |heard| {
                now.saturating_duration_since(heard) >= self.timeout
                    || now.saturating_duration_since(self.started) >= self.gather_timeout
            },
        )
    }
}

/// The player a message speaks for, if it names one: the seat whose input it
/// carries or which acknowledges. `Sync`, `Checksum`, `Quality` and `Roster`
/// name no seat.
const fn seat_named(msg: &NetMessage) -> Option<u8> {
    match msg {
        NetMessage::Input { player, .. }
        | NetMessage::InputBatch { player, .. }
        | NetMessage::InputAck { player, .. } => Some(*player),
        NetMessage::Sync { .. }
        | NetMessage::Checksum { .. }
        | NetMessage::Quality { .. }
        | NetMessage::Roster { .. } => None,
    }
}

/// A roster a joiner may act on: 2 to 4 entries whose player indices are
/// exactly `0..len`, each once, in any order. The host is player 0 and the
/// session is built for `len` players, so a gap or a repeat would hand the
/// session a player index it does not have.
fn roster_is_well_formed(peers: &[(u8, SocketAddr, KeyShare)]) -> bool {
    if !(2..=NetMessage::MAX_ROSTER).contains(&peers.len()) {
        return false;
    }
    let mut seen = [false; NetMessage::MAX_ROSTER];
    for &(player, _, _) in peers {
        match seen.get_mut(usize::from(player)) {
            Some(slot) if usize::from(player) < peers.len() && !*slot => *slot = true,
            _ => return false,
        }
    }
    true
}

/// The seat table for a joiner that met a 2-player host (the pair fallback):
/// one entry, the host, whose authenticated **sender index** under the pair
/// key is whichever its key share sorts to, but whose **seat** (the `player`
/// its `Input` / `InputBatch` / `InputAck` must name) is always 0. Pulled out
/// of [`MeshJoiner::pump`] so the seat rule is tested for both share orders
/// rather than by whichever order a random share happens to produce: until
/// v3.4.0's review, the only test that noticed a sender index used as the seat
/// failed in about half its runs.
fn pair_fallback_seat(auth: &DatagramAuth, host: SocketAddr) -> (u8, u8, SocketAddr) {
    let host_sender = auth.remote_senders().next().unwrap_or(0);
    (host_sender, 0, host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn loopback() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0))
    }

    #[test]
    fn mesh_transport_fans_out_and_polls() {
        // Three bare sockets; peer 0 broadcasts to peers 1 and 2.
        let s0 = UdpSocket::bind(loopback()).unwrap();
        let s1 = UdpSocket::bind(loopback()).unwrap();
        let s2 = UdpSocket::bind(loopback()).unwrap();
        let a1 = s1.local_addr().unwrap();
        let a2 = s2.local_addr().unwrap();
        let mut t0 = UdpMeshTransport::new(s0, vec![a1, a2]).unwrap();
        let mut t1 = UdpMeshTransport::new(s1, vec![]).unwrap();
        let mut t2 = UdpMeshTransport::new(s2, vec![]).unwrap();

        let msg = NetMessage::Input {
            player: 0,
            frame: 11,
            input: 0x42,
        };
        t0.send(&msg);
        // Give loopback a brief, bounded chance to deliver.
        let g1 = drain(&mut t1, 1);
        let g2 = drain(&mut t2, 1);
        assert_eq!(g1, vec![msg.clone()]);
        assert_eq!(g2, vec![msg]);
    }

    #[test]
    fn mesh_transport_drops_malformed() {
        let s0 = UdpSocket::bind(loopback()).unwrap();
        let s1 = UdpSocket::bind(loopback()).unwrap();
        let a1 = s1.local_addr().unwrap();
        let _t0 = UdpMeshTransport::new(s0, vec![a1]).unwrap();
        let mut t1 = UdpMeshTransport::new(s1, vec![]).unwrap();

        // Blast junk at t1 directly from a stray socket.
        let stray = UdpSocket::bind(loopback()).unwrap();
        stray.send_to(&[], a1).unwrap();
        stray.send_to(&[250, 1, 2], a1).unwrap();
        stray
            .send_to(
                &NetMessage::InputAck {
                    player: 1,
                    frame: 9,
                }
                .to_bytes(),
                a1,
            )
            .unwrap();

        let got = drain(&mut t1, 1);
        assert_eq!(
            got,
            vec![NetMessage::InputAck {
                player: 1,
                frame: 9
            }]
        );
        assert!(t1.dropped_invalid() >= 2);
    }

    /// Drive a host + (`num_players` - 1) joiners through the roster handshake on
    /// loopback, returning the host transport + each joiner transport in player
    /// order.
    fn run_roster_handshake(
        num_players: u8,
        rom_hash: SessionIdentity,
    ) -> (UdpMeshTransport, Vec<UdpMeshTransport>) {
        // Probe a free loopback port so the host's listening addr == its
        // gameplay addr (correct for loopback; an internet deployment swaps in
        // the STUN-discovered public addr).
        let probe = UdpSocket::bind(loopback()).unwrap();
        let port = probe.local_addr().unwrap();
        drop(probe);
        let mut host = MeshHost::bind(port, port, num_players, rom_hash).unwrap();
        let host_listen = host.local_addr().unwrap();

        let mut joiners: Vec<MeshJoiner> = (1..num_players)
            .map(|p| MeshJoiner::connect(loopback(), host_listen, p, rom_hash).unwrap())
            .collect();

        let mut host_out: Option<UdpMeshTransport> = None;
        let mut joiner_out: Vec<Option<UdpMeshTransport>> =
            (0..joiners.len()).map(|_| None).collect();

        for _ in 0..2000 {
            if host_out.is_none() {
                host_out = host.pump().expect("host pump");
            }
            for (i, j) in joiners.iter_mut().enumerate() {
                if joiner_out[i].is_none() {
                    joiner_out[i] = j.pump().expect("joiner pump");
                }
            }
            if host_out.is_some() && joiner_out.iter().all(Option::is_some) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        let host_t = host_out.expect("host produced a transport");
        let joiner_ts: Vec<UdpMeshTransport> = joiner_out
            .into_iter()
            .map(|o| o.expect("joiner transport"))
            .collect();
        (host_t, joiner_ts)
    }

    #[test]
    fn three_player_roster_handshake_wires_full_mesh() {
        let hash = SessionIdentity::new([0x55u8; 32], [0; 32]);
        let (host, joiners) = run_roster_handshake(3, hash);
        // Host reaches 2 joiners; each joiner reaches 2 others (host + 1 joiner).
        assert_eq!(host.peers().len(), 2, "host wired to both joiners");
        for j in &joiners {
            assert_eq!(j.peers().len(), 2, "joiner wired to host + other joiner");
        }
    }

    #[test]
    fn host_rejects_rom_mismatch() {
        let probe = UdpSocket::bind(loopback()).unwrap();
        let port = probe.local_addr().unwrap();
        drop(probe);
        let mut host =
            MeshHost::bind(port, port, 3, SessionIdentity::new([0x11u8; 32], [0; 32])).unwrap();
        // A joiner with the WRONG rom hash dials in.
        let mut bad = MeshJoiner::connect(
            loopback(),
            port,
            1,
            SessionIdentity::new([0x22u8; 32], [0; 32]),
        )
        .unwrap();
        let mut rejected = false;
        for _ in 0..500 {
            let _ = bad.pump();
            if matches!(host.pump(), Err(MeshError::RomMismatch)) {
                rejected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(rejected, "host rejected the mismatched-ROM joiner");
    }

    /// v2.9.8 — same ROM, different configuration: the host refuses the
    /// joiner with `ConfigMismatch`.
    #[test]
    fn host_rejects_config_mismatch() {
        let probe = UdpSocket::bind(loopback()).unwrap();
        let port = probe.local_addr().unwrap();
        drop(probe);
        let rom = [0x11u8; 32];
        let mut host = MeshHost::bind(port, port, 3, SessionIdentity::new(rom, [1; 32])).unwrap();
        let mut bad =
            MeshJoiner::connect(loopback(), port, 1, SessionIdentity::new(rom, [2; 32])).unwrap();
        let mut rejected = false;
        for _ in 0..500 {
            let _ = bad.pump();
            if matches!(host.pump(), Err(MeshError::ConfigMismatch)) {
                rejected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(rejected, "host rejected the differently configured joiner");
    }

    /// v3.0.0 (ADR 0045) — a joiner from another emulation epoch is refused by
    /// the host, AND the joiner itself learns why: the host answers with its
    /// own identity before refusing, so the joiner reports `EmulatorMismatch`
    /// instead of timing out with no reason (Copilot on #586).
    #[test]
    fn epoch_mismatch_is_named_at_both_ends() {
        let probe = UdpSocket::bind(loopback()).unwrap();
        let port = probe.local_addr().unwrap();
        drop(probe);
        let ours = SessionIdentity::new([0x11u8; 32], [1; 32]);
        let mut theirs = ours;
        theirs.epoch = ours.epoch + 1;
        let mut host = MeshHost::bind(port, port, 3, ours).unwrap();
        let mut joiner = MeshJoiner::connect(loopback(), port, 1, theirs).unwrap();
        let (mut host_err, mut joiner_err) = (None, None);
        for _ in 0..500 {
            if joiner_err.is_none()
                && let Err(e) = joiner.pump()
            {
                joiner_err = Some(e);
            }
            if host_err.is_none()
                && let Err(e) = host.pump()
            {
                host_err = Some(e);
            }
            if host_err.is_some() && joiner_err.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            matches!(host_err, Some(MeshError::EmulatorMismatch { .. })),
            "host names the epoch: {host_err:?}"
        );
        assert!(
            matches!(joiner_err, Some(MeshError::EmulatorMismatch { .. })),
            "joiner names the epoch too, not a timeout: {joiner_err:?}"
        );
    }

    #[test]
    fn host_adoption_is_idempotent_for_duplicate_sync() {
        // A joiner that re-sends Sync many times must be adopted ONCE (its index
        // stays put). We verify the host fills exactly num_players-1 slots.
        let probe = UdpSocket::bind(loopback()).unwrap();
        let port = probe.local_addr().unwrap();
        drop(probe);
        let id = SessionIdentity::new([0x77u8; 32], [0; 32]);
        let mut host = MeshHost::bind(port, port, 2, id).unwrap();
        let listen = host.local_addr().unwrap();
        let mut j = MeshJoiner::connect(loopback(), listen, 1, id).unwrap();
        let mut done = false;
        let mut j_done = false;
        for _ in 0..500 {
            // The joiner hands off once it has the roster; stop pumping it
            // then, as a real caller would (the host keeps resending).
            if !j_done && let Ok(Some(_)) = j.pump() {
                j_done = true;
            }
            if host.pump().expect("host pump").is_some() {
                done = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(done, "2-player roster filled with one joiner");
        assert_eq!(host.joiners_ready(), 1, "exactly one joiner adopted");
    }

    /// v3.4.0 (security review of the mesh merge) — the roster is the one
    /// message a joiner acts on BEFORE any key exists, so it is untrusted
    /// input from anyone who can reach the joiner's port. A scripted fake host
    /// learns the joiner's key share and address from its `Sync`, then offers
    /// rosters: one with a gap in the player indices (`{0, 3}`, which would
    /// leave the joiner as player 3 of a 2-player session), and a correct one
    /// sent from a THIRD address. Both must be ignored; the correct roster from
    /// the dialed host is the control that shows the joiner can hand off.
    #[test]
    fn joiner_ignores_a_malformed_or_foreign_roster() {
        let id = SessionIdentity::new([0x5Au8; 32], [3; 32]);
        let fake_host = UdpSocket::bind(loopback()).unwrap();
        fake_host.set_nonblocking(true).unwrap();
        let host_addr = fake_host.local_addr().unwrap();
        let stranger = UdpSocket::bind(loopback()).unwrap();
        let mut joiner = MeshJoiner::connect(loopback(), host_addr, 1, id).unwrap();

        // The joiner's first Sync names its key share; its source is its address.
        let mut buf = [0u8; RECV_BUF_LEN];
        let mut learned = None;
        for _ in 0..500 {
            let _ = joiner.pump();
            if let Ok((len, from)) = fake_host.recv_from(&mut buf)
                && let Some(NetMessage::Sync { key_share, .. }) =
                    NetMessage::from_bytes(&buf[..len])
            {
                learned = Some((from, key_share));
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let (joiner_addr, joiner_share) = learned.expect("the joiner sent its Sync");
        let host_share = [0x42u8; KEY_SHARE_LEN];
        let roster = |players: [u8; 2]| {
            NetMessage::Roster {
                peers: vec![
                    (players[0], host_addr, host_share),
                    (players[1], joiner_addr, joiner_share),
                ],
            }
            .to_bytes()
        };
        let handed_off_after = |joiner: &mut MeshJoiner, from: &UdpSocket, bytes: &[u8]| {
            for _ in 0..50 {
                from.send_to(bytes, joiner_addr).unwrap();
                std::thread::sleep(Duration::from_millis(1));
                if let Ok(Some(_)) = joiner.pump() {
                    return true;
                }
            }
            false
        };

        assert!(
            !handed_off_after(&mut joiner, &fake_host, &roster([0, 3])),
            "a roster with a gap in the player indices is refused"
        );
        // A gap on ANOTHER peer's index: this joiner is a valid player 1, so
        // only the roster shape check can refuse `{0, 1, 3}` (a 3-player
        // session with a peer 3 it does not have).
        let third = (3u8, stranger.local_addr().unwrap(), [0x24u8; KEY_SHARE_LEN]);
        let gapped_elsewhere = NetMessage::Roster {
            peers: vec![
                (0, host_addr, host_share),
                (1, joiner_addr, joiner_share),
                third,
            ],
        }
        .to_bytes();
        assert!(
            !handed_off_after(&mut joiner, &fake_host, &gapped_elsewhere),
            "a roster with a gap at another peer's index is refused"
        );
        assert!(
            !handed_off_after(&mut joiner, &stranger, &roster([0, 1])),
            "a roster from anyone but the dialed host is refused"
        );
        assert!(
            handed_off_after(&mut joiner, &fake_host, &roster([0, 1])),
            "control: the dialed host's well-formed roster is accepted"
        );
        assert_eq!(joiner.num_players(), 2);
    }

    /// Pump `host` and `joiners` together until every one has handed off or
    /// `budget` elapses; panics on any error. Returns the host's transport
    /// and each joiner's.
    fn pump_all(
        host: &mut MeshHost,
        joiners: &mut [MeshJoiner],
        budget: Duration,
    ) -> (Option<UdpMeshTransport>, Vec<Option<UdpMeshTransport>>) {
        let mut host_out = None;
        let mut joiner_out: Vec<Option<UdpMeshTransport>> =
            (0..joiners.len()).map(|_| None).collect();
        let start = Instant::now();
        while start.elapsed() < budget {
            if host_out.is_none() {
                host_out = host.pump().expect("host pump");
            }
            for (j, out) in joiners.iter_mut().zip(joiner_out.iter_mut()) {
                if out.is_none() {
                    *out = j.pump().expect("joiner pump");
                }
            }
            if host_out.is_some() && joiner_out.iter().all(Option::is_some) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        (host_out, joiner_out)
    }

    /// v3.4.0 (review of #613) — the first joiner of a 3-4
    /// player host waits for the others. The host sent nothing until its
    /// roster was full, so a joiner armed with the frontend's 10 s handshake
    /// timeout gave up while the host (gathering for 120 s) still counted it,
    /// and the host later started a session with a peer that was gone. The
    /// host now answers each gathering joiner's `Sync`, and a joiner that has
    /// heard its host keeps waiting while the host keeps answering. Here the
    /// joiner's handshake timeout is 300 ms and the second joiner arrives
    /// after 800 ms: both complete.
    #[test]
    fn a_joiner_outlives_its_handshake_timeout_while_the_host_gathers() {
        let id = SessionIdentity::new([0x66u8; 32], [6; 32]);
        let mut host = MeshHost::bind(loopback(), loopback(), 3, id).unwrap();
        let listen = host.local_addr().unwrap();
        let short = Duration::from_millis(300);
        let mut first = vec![
            MeshJoiner::connect(loopback(), listen, 1, id)
                .unwrap()
                .with_timeout(short),
        ];
        let (h, j) = pump_all(&mut host, &mut first, Duration::from_millis(800));
        assert!(h.is_none() && j[0].is_none(), "still gathering");
        assert!(first[0].host_answered(), "the gathering host answered");
        let mut all = vec![
            first.pop().unwrap(),
            MeshJoiner::connect(loopback(), listen, 2, id)
                .unwrap()
                .with_timeout(short),
        ];
        let (h, j) = pump_all(&mut host, &mut all, Duration::from_secs(5));
        assert!(h.is_some(), "the host completed");
        assert!(j.iter().all(Option::is_some), "both joiners completed");
    }

    /// The short timeout still applies to an address where no host answers.
    #[test]
    fn a_joiner_of_a_dead_address_still_times_out_quickly() {
        let id = SessionIdentity::new([0x67u8; 32], [7; 32]);
        let nobody = UdpSocket::bind(loopback()).unwrap();
        let dead = nobody.local_addr().unwrap();
        let mut joiner = MeshJoiner::connect(loopback(), dead, 1, id)
            .unwrap()
            .with_timeout(Duration::from_millis(200))
            .with_gather_timeout(Duration::from_secs(60));
        let start = Instant::now();
        let verdict = loop {
            match joiner.pump() {
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                other => break other,
            }
        };
        assert!(matches!(verdict, Err(MeshError::Timeout)), "{verdict:?}");
        assert!(!joiner.host_answered());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }

    /// v3.4.0 — a joiner that leaves while the host gathers (its process
    /// closed, or it gave up) stops re-sending its `Sync`, and the host drops
    /// it after `joiner_silence` instead of starting a session with it. Its
    /// seat is reused: in a 4-player gather, seat 1 leaves while seat 2 stays,
    /// and the two late joiners must take seats 1 and 3 (the next index by
    /// count would be 2 again, a roster that forms no mesh).
    #[test]
    fn the_host_drops_a_joiner_that_went_silent_before_the_roster_filled() {
        let id = SessionIdentity::new([0x68u8; 32], [8; 32]);
        let mut host = MeshHost::bind(loopback(), loopback(), 4, id).unwrap();
        host.joiner_silence = Duration::from_millis(300);
        let listen = host.local_addr().unwrap();
        let mut gone = vec![MeshJoiner::connect(loopback(), listen, 1, id).unwrap()];
        let gone_addr = gone[0].local_addr().unwrap();
        let _ = pump_all(&mut host, &mut gone, Duration::from_millis(100));
        let mut stays = vec![MeshJoiner::connect(loopback(), listen, 2, id).unwrap()];
        let _ = pump_all(&mut host, &mut stays, Duration::from_millis(100));
        assert_eq!(host.joiners_ready(), 2, "both adopted");
        drop(gone);
        let _ = pump_all(&mut host, &mut stays, Duration::from_millis(600));
        assert_eq!(host.joiners_ready(), 1, "the silent one was dropped");
        let mut all: Vec<MeshJoiner> = stays
            .into_iter()
            .chain((0..2).map(|_| MeshJoiner::connect(loopback(), listen, 1, id).unwrap()))
            .collect();
        let (h, j) = pump_all(&mut host, &mut all, Duration::from_secs(5));
        let h = h.expect("the host completed with the three live joiners");
        assert!(j.iter().all(Option::is_some));
        assert!(!h.peers().contains(&gone_addr), "{:?}", h.peers());
        assert_eq!(
            all[0].my_player(),
            2,
            "the joiner that stayed kept its seat"
        );
        let mut seats: Vec<u8> = all.iter().map(MeshJoiner::my_player).collect();
        seats.sort_unstable();
        assert_eq!(seats, [1, 2, 3]);
    }

    /// v3.4.0 — the seat check on a joiner's 2-player fallback, where the
    /// pair key numbers senders by share order: with the host's share sorting
    /// second the host is sender 1 but player 0. Its player-0 input must open
    /// (a seat check keyed on the sender index would drop every input the
    /// host sends), and an input it sends naming player 1 (the joiner's own
    /// seat) must not.
    #[test]
    fn the_pair_fallback_checks_the_hosts_seat_not_its_sender_index() {
        let id = SessionIdentity::new([0x31u8; 32], [0x13; 32]);
        let (joiner_share, host_share) = ([1u8; KEY_SHARE_LEN], [2u8; KEY_SHARE_LEN]);
        let joiner_auth = DatagramAuth::pair(&id, joiner_share, host_share).unwrap();
        let mut host_auth = DatagramAuth::pair(&id, host_share, joiner_share).unwrap();
        assert_eq!(host_auth.local_sender(), 1, "the host's share sorts second");
        let host = UdpSocket::bind(loopback()).unwrap();
        let joiner_socket = UdpSocket::bind(loopback()).unwrap();
        let joiner_addr = joiner_socket.local_addr().unwrap();
        let mut joiner = UdpMeshTransport::sealed_with_seats(
            joiner_socket,
            &[(1, 0, host.local_addr().unwrap())],
            joiner_auth,
            None,
        )
        .unwrap();
        let own = NetMessage::Input {
            player: 0,
            frame: 4,
            input: 0x08,
        };
        let theirs = NetMessage::Input {
            player: 1,
            frame: 4,
            input: 0x80,
        };
        for msg in [&theirs, &own] {
            host.send_to(&host_auth.seal(&msg.to_bytes()), joiner_addr)
                .unwrap();
        }
        assert_eq!(drain(&mut joiner, 1), vec![own]);
        assert_eq!(joiner.misattributed_datagrams(), 1);
    }

    /// The pair fallback's host is seat 0 whichever way the two key shares
    /// sort, so whichever pair sender index it authenticates as. Both orders,
    /// fixed shares: the mutation "seat = sender index" fails the second.
    #[test]
    fn the_pair_fallback_host_is_seat_zero_for_both_share_orders() {
        let id = SessionIdentity::new([0x31u8; 32], [0x13; 32]);
        let host = loopback();
        for (joiner_share, host_share, host_sender) in [
            ([2u8; KEY_SHARE_LEN], [1u8; KEY_SHARE_LEN], 0u8),
            ([1u8; KEY_SHARE_LEN], [2u8; KEY_SHARE_LEN], 1u8),
        ] {
            let auth = DatagramAuth::pair(&id, joiner_share, host_share).unwrap();
            assert_eq!(
                pair_fallback_seat(&auth, host),
                (host_sender, 0, host),
                "host share sorting to sender {host_sender} must still be seat 0"
            );
        }
    }

    fn drain(t: &mut UdpMeshTransport, want: usize) -> Vec<NetMessage> {
        let mut got = Vec::new();
        for _ in 0..200 {
            got.extend(t.poll());
            if got.len() >= want {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        got
    }
}
