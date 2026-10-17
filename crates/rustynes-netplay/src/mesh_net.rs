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
//!   `num_players - 1` joiners** from their `Sync`s, assigns each the next free
//!   player index, and once the roster is full **distributes the full roster**
//!   ([`NetMessage::Roster`]) to every joiner. Then
//!   it yields a [`UdpMeshTransport`] for the host (player 0) wired to every
//!   joiner.
//! - [`MeshJoiner`] — the joiner orchestrator. It dials the host, `Sync`s, then
//!   **waits for the roster**; on receipt it builds a [`UdpMeshTransport`] wired
//!   to the host and every *other* joiner (skipping its own entry).
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
}

/// The sealing state of a [`UdpMeshTransport`].
#[derive(Debug)]
struct SealedMesh {
    auth: DatagramAuth,
    /// The sender index of each entry of `peers`, so a datagram that opens
    /// can move its sender's address (a NAT rebinding).
    senders: Vec<u8>,
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
    pub fn new_sealed(
        socket: UdpSocket,
        peers: &[(u8, SocketAddr)],
        auth: DatagramAuth,
        handshake_reply: Option<Vec<u8>>,
    ) -> io::Result<Self> {
        let senders: Vec<u8> = peers.iter().map(|&(s, _)| s).collect();
        let addrs: Vec<SocketAddr> = peers.iter().map(|&(_, a)| a).collect();
        let mut t = Self::new(socket, addrs)?;
        t.sealed = Some(Box::new(SealedMesh {
            auth,
            last_reply: vec![None; senders.len()],
            senders,
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
                if let Some(i) = sealed.senders.iter().position(|&s| s == sender)
                    && !same_peer(self.peers[i], from)
                {
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
    /// `addr -> (player index, key share)`, in adoption order (the host
    /// itself is player 0, never in this map). v3.4.0: the share is the one
    /// the joiner's first adopting `Sync` carried.
    joiners: BTreeMap<SocketAddr, (u8, KeyShare)>,
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
}

impl MeshHost {
    /// Default time to wait for all joiners.
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    /// Re-broadcast the roster this many times (loss tolerance).
    const ROSTER_RESENDS: u8 = 5;
    /// Interval between roster re-broadcasts.
    const ROSTER_RESEND_INTERVAL: Duration = Duration::from_millis(50);

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
            .map(|(&a, &(p, share))| (p, a, share))
            .collect();
        joiners.sort_by_key(|&(p, _, _)| p);
        peers.extend(joiners);
        peers
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
                // Adopt this source as a new joiner IF there is still room and it
                // is not already known (idempotent — a re-sent Sync from a known
                // joiner must not shift indices).
                // v3.4.0: only a `Sync` with a key share is a joiner's
                // handshake; one without (a stray in-session `Sync`) adopts
                // no one.
                if key_share != [0; KEY_SHARE_LEN]
                    && !self.joiners.contains_key(&from)
                    && !self.roster_full()
                {
                    let next_index = u8::try_from(self.joiners.len() + 1).unwrap_or(u8::MAX);
                    self.joiners.insert(from, (next_index, key_share));
                }
            }
            // Non-Sync traffic during the handshake is ignored (early/stray).
        }

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
                self.joiners.iter().map(|(&a, &(p, _))| (p, a)).collect();
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
/// [`Roster`](crate::NetMessage::Roster) arrives; on receipt it learns its own
/// player index (the entry whose address matches the host's view of it — but
/// since a joiner cannot see its own NAT-rewritten source, the host assigns the
/// index and the joiner finds itself by elimination: it is the index NOT equal
/// to the host's player 0 and reachable; in practice the roster lists the host
/// at player 0 and the joiner takes the index the host told it implicitly by
/// position). It then builds a [`UdpMeshTransport`] wired to the host + every
/// *other* joiner.
///
/// On loopback / LAN (no NAT rewrite) the joiner identifies its own roster entry
/// by matching its bound source address exactly — the robust path — and that
/// match overrides the index it was given at [`connect`](Self::connect). Through
/// a NAT the host saw a rewritten source the joiner cannot observe, so it falls
/// back to the index supplied out of band at [`connect`](Self::connect) (the
/// frontend chooses "I am joiner #k").
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
}

impl MeshJoiner {
    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
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
        })
    }

    /// Override the wait timeout (default 30s). Builder-style.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
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
                    Some(NetMessage::Roster { peers }) => roster = Some(peers),
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
            let host_sender = auth.remote_senders().next().unwrap_or(0);
            let socket = self
                .socket
                .take()
                .expect("joiner socket present to hand off");
            return UdpMeshTransport::new_sealed(socket, &[(host_sender, self.host)], auth, None)
                .map(Some)
                .map_err(|_| MeshError::Timeout);
        }

        // Re-send Sync periodically until the roster arrives.
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

        if now.saturating_duration_since(self.started) >= self.timeout {
            return Err(MeshError::Timeout);
        }
        Ok(None)
    }
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
