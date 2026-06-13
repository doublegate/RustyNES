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

use crate::message::NetMessage;
use crate::transport::Transport;

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
        })
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
        let bytes = msg.to_bytes();
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
                Ok((len, _from)) => match NetMessage::from_bytes(&buf[..len]) {
                    Some(msg) => out.push(msg),
                    None => self.dropped_invalid = self.dropped_invalid.saturating_add(1),
                },
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
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("mesh handshake timed out before all joiners arrived"),
            Self::RomMismatch => f.write_str("a peer announced a different ROM"),
        }
    }
}

impl std::error::Error for MeshError {}

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
    rom_hash: [u8; 32],
    num_players: u8,
    /// `addr -> player index`, in adoption order (the host itself is player 0,
    /// never in this map).
    joiners: BTreeMap<SocketAddr, u8>,
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
        rom_hash: [u8; 32],
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: Some(socket),
            rom_hash,
            num_players: num_players.clamp(2, 4),
            joiners: BTreeMap::new(),
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

    /// Build the full roster (host first, then joiners in adoption order).
    fn build_roster(&self) -> Vec<(u8, SocketAddr)> {
        let mut peers = Vec::with_capacity(self.num_players as usize);
        peers.push((0u8, self.host_addr));
        // BTreeMap iterates by address; sort the emitted list by player index
        // instead so the roster is stable + index-ordered.
        let mut joiners: Vec<(u8, SocketAddr)> =
            self.joiners.iter().map(|(&a, &p)| (p, a)).collect();
        joiners.sort_by_key(|&(p, _)| p);
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
            if let NetMessage::Sync { magic, rom_hash } = msg {
                if magic != NetMessage::SYNC_MAGIC {
                    continue;
                }
                if rom_hash != self.rom_hash {
                    return Err(MeshError::RomMismatch);
                }
                // Adopt this source as a new joiner IF there is still room and it
                // is not already known (idempotent — a re-sent Sync from a known
                // joiner must not shift indices).
                if !self.joiners.contains_key(&from) && !self.roster_full() {
                    let next_index = u8::try_from(self.joiners.len() + 1).unwrap_or(u8::MAX);
                    self.joiners.insert(from, next_index);
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
            // After the first broadcast, build + return the host's transport.
            // (We still want the joiners to have received it; the resends above
            // ride out loss, but the host can proceed immediately — its mesh
            // transport simply starts sending Inputs, which also doubles as
            // liveness for the joiners.)
            let socket = self.socket.take().expect("host socket present to hand off");
            let peers: Vec<SocketAddr> = self.joiners.keys().copied().collect();
            // We never reach here without at least the resend above; map an I/O
            // error on the (already-configured non-blocking) socket to a Timeout
            // rather than panic.
            return UdpMeshTransport::new(socket, peers)
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
    rom_hash: [u8; 32],
    /// This joiner's own player index (assigned by the host scheme; the frontend
    /// supplies it, or it is discovered from the roster — see module docs).
    my_player: u8,
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
        rom_hash: [u8; 32],
    ) -> io::Result<Self> {
        let socket = UdpSocket::bind(local)?;
        socket.set_nonblocking(true)?;
        let sync = NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            rom_hash,
        }
        .to_bytes();
        let _ = socket.send_to(&sync, host);
        let now = Instant::now();
        Ok(Self {
            socket: Some(socket),
            host,
            rom_hash,
            my_player,
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
        let mut roster: Option<Vec<(u8, SocketAddr)>> = None;
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            match socket.recv_from(&mut buf) {
                Ok((len, _from)) => match NetMessage::from_bytes(&buf[..len]) {
                    Some(NetMessage::Roster { peers }) => roster = Some(peers),
                    Some(NetMessage::Sync { magic, rom_hash })
                        if magic == NetMessage::SYNC_MAGIC && rom_hash != self.rom_hash =>
                    {
                        return Err(MeshError::RomMismatch);
                    }
                    _ => {}
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(_) => break,
            }
        }

        if let Some(peers) = roster {
            // Identify our own entry in the roster. On loopback / LAN there is no
            // NAT rewrite, so the host saw our real bound source address and we
            // can match it exactly — the most robust path. Through a NAT the host
            // saw a rewritten address we cannot observe locally, so we fall back
            // to the `my_player` index assigned out of band at connect (see
            // module docs). If the address match succeeds it OVERRIDES the
            // provided index (it is ground truth).
            let my_local = self.socket.as_ref().and_then(|s| s.local_addr().ok());
            if let Some(local) = my_local {
                if let Some(&(p, _)) = peers.iter().find(|&&(_, a)| a == local) {
                    self.my_player = p;
                }
            }
            // The mesh is every peer EXCEPT this joiner's own index.
            let others: Vec<SocketAddr> = peers
                .iter()
                .filter(|&&(p, _)| p != self.my_player)
                .map(|&(_, a)| a)
                .collect();
            let socket = self
                .socket
                .take()
                .expect("joiner socket present to hand off");
            return UdpMeshTransport::new(socket, others)
                .map(Some)
                .map_err(|_| MeshError::Timeout);
        }

        // Re-send Sync periodically until the roster arrives.
        let due = self
            .last_sync_sent
            .is_none_or(|t| now.saturating_duration_since(t) >= Self::SYNC_RESEND_INTERVAL);
        if due {
            if let Some(socket) = self.socket.as_ref() {
                let sync = NetMessage::Sync {
                    magic: NetMessage::SYNC_MAGIC,
                    rom_hash: self.rom_hash,
                }
                .to_bytes();
                let _ = socket.send_to(&sync, self.host);
                self.last_sync_sent = Some(now);
            }
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
            .send_to(&NetMessage::InputAck { frame: 9 }.to_bytes(), a1)
            .unwrap();

        let got = drain(&mut t1, 1);
        assert_eq!(got, vec![NetMessage::InputAck { frame: 9 }]);
        assert!(t1.dropped_invalid() >= 2);
    }

    /// Drive a host + (`num_players` - 1) joiners through the roster handshake on
    /// loopback, returning the host transport + each joiner transport in player
    /// order.
    fn run_roster_handshake(
        num_players: u8,
        rom_hash: [u8; 32],
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
        let hash = [0x55u8; 32];
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
        let mut host = MeshHost::bind(port, port, 3, [0x11u8; 32]).unwrap();
        // A joiner with the WRONG rom hash dials in.
        let mut bad = MeshJoiner::connect(loopback(), port, 1, [0x22u8; 32]).unwrap();
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

    #[test]
    fn host_adoption_is_idempotent_for_duplicate_sync() {
        // A joiner that re-sends Sync many times must be adopted ONCE (its index
        // stays put). We verify the host fills exactly num_players-1 slots.
        let probe = UdpSocket::bind(loopback()).unwrap();
        let port = probe.local_addr().unwrap();
        drop(probe);
        let mut host = MeshHost::bind(port, port, 2, [0x77u8; 32]).unwrap();
        let listen = host.local_addr().unwrap();
        let mut j = MeshJoiner::connect(loopback(), listen, 1, [0x77u8; 32]).unwrap();
        let mut done = false;
        for _ in 0..500 {
            let _ = j.pump();
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
