//! A minimal **TURN client** (RFC 8656) for the symmetric-NAT fallback.
//!
//! v1.8.7. Plus a [`RelayUdpSocket`] shim so the existing
//! [`UdpTransport`](crate::UdpTransport) can run **unchanged** over a TURN relay.
//!
//! # Why TURN
//!
//! Basic STUN + UDP hole punching ([`crate::stun`]) traverses *cone* NATs: a
//! peer's public mapping is the same for every destination, so the address it
//! learns from a STUN server is the one its peer can reach. A **symmetric** NAT
//! assigns a *different* external port per destination, so the STUN-discovered
//! address is useless to the peer — hole punching fails. The fallback is a
//! **TURN relay** (RFC 8656): the peer asks a TURN server to **allocate** a
//! public *relayed transport address* and forwards all of its traffic through
//! that server, which carries the media (unlike STUN, which only reports an
//! address). Both peers relay through the server and reach each other reliably,
//! at the cost of the server's bandwidth.
//!
//! # What this implements
//!
//! The **long-term-credential** subset of RFC 8656 that the common deployment
//! (a `coturn` instance, see `deploy/`) needs:
//!
//! - [`TurnClient::allocate`] — the two-step `Allocate` transaction: an
//!   unauthenticated probe that draws the server's `401` + `REALM` / `NONCE`,
//!   then the authenticated retry carrying a `MESSAGE-INTEGRITY` HMAC-SHA1 over
//!   the long-term key. Returns the server-assigned **`XOR-RELAYED-ADDRESS`** —
//!   the public address peers send to.
//! - [`TurnClient::create_permission`] — a `CreatePermission` for a specific
//!   peer address (a TURN server drops relayed data from/to a peer with no
//!   permission).
//! - [`TurnClient::wrap`] / [`TurnClient::unwrap`] — the **Send-Indication** /
//!   **Data-Indication** framing: `wrap(peer, payload)` encodes an outbound
//!   datagram the server forwards to `peer`; `unwrap(datagram)` decodes an
//!   inbound Data-Indication back to `(peer, payload)`. These are the only two
//!   operations on the gameplay hot path, so they are allocation-light and
//!   fully unit-tested.
//! - [`RelayUdpSocket`] — wraps a [`UdpSocket`] so every
//!   `send_to(peer)` is TURN-`wrap`ped to the server and every `recv_from`
//!   TURN-`unwrap`s, presenting the *peer's* address to the caller. This lets
//!   the existing [`UdpTransport`](crate::UdpTransport) — and therefore
//!   [`RollbackSession`](crate::session::RollbackSession) — run over a relay with
//!   **no new transport type**.
//!
//! All of this is native-only (`std::net`); STUN's encode/decode helpers in
//! [`crate::stun`] (header layout, the magic cookie, XOR-mapped-address decode)
//! are reused where they apply (TURN is a STUN method family on the same header).

#![cfg(not(target_arch = "wasm32"))]

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

use crate::rng::SplitMix64;
use crate::stun::{HEADER_LEN, MAGIC_COOKIE, TransactionId};

// ── STUN/TURN message types (RFC 5389 §6, RFC 8656 §5) ──────────────────────

/// `Allocate` request (TURN method `0x003`, class request).
const MSG_ALLOCATE_REQUEST: u16 = 0x0003;
/// `Allocate` success response.
const MSG_ALLOCATE_SUCCESS: u16 = 0x0103;
/// `Allocate` error response.
const MSG_ALLOCATE_ERROR: u16 = 0x0113;
/// `CreatePermission` request (TURN method `0x008`).
const MSG_CREATE_PERMISSION_REQUEST: u16 = 0x0008;
/// `CreatePermission` success response.
const MSG_CREATE_PERMISSION_SUCCESS: u16 = 0x0108;
/// `Send` indication (TURN method `0x006`, class indication).
const MSG_SEND_INDICATION: u16 = 0x0016;
/// `Data` indication (TURN method `0x007`, class indication).
const MSG_DATA_INDICATION: u16 = 0x0017;

// ── attribute types ─────────────────────────────────────────────────────────

const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
const ATTR_DATA: u16 = 0x0013;
const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;
const ATTR_LIFETIME: u16 = 0x000D;
const ATTR_USERNAME: u16 = 0x0006;
const ATTR_REALM: u16 = 0x0014;
const ATTR_NONCE: u16 = 0x0015;
const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;

/// Address-family markers inside a (XOR-)address attribute.
const FAMILY_IPV4: u8 = 0x01;
const FAMILY_IPV6: u8 = 0x02;

/// The `REQUESTED-TRANSPORT` value for UDP (protocol number 17, top byte).
const TRANSPORT_UDP: u8 = 17;

/// The default allocation lifetime to request, in seconds (the server may
/// shorten it; long enough for a session, refreshed by re-allocating).
const DEFAULT_LIFETIME_SECS: u32 = 600;

/// Retransmission timeout (RTO) for a STUN/TURN request transaction.
///
/// STUN/TURN run over **unreliable UDP**, so RFC 5389 §7.2.1 mandates that a
/// client retransmit an unacknowledged request rather than give up after a
/// single silent wait: a request *or* its response can be dropped on any lossy
/// path — and, in practice, even on `127.0.0.1` under load (a loaded CI runner
/// can overflow a small per-socket receive buffer and silently discard a
/// loopback datagram). Without retransmission a single dropped
/// `Allocate` / `CreatePermission` datagram hard-fails the entire NAT traversal;
/// with it, the transaction simply re-sends every [`RTO`] until the caller's
/// overall `timeout` deadline. Because STUN/TURN requests are idempotent (a
/// compliant server keys its reply on the transaction id / source and re-answers
/// a duplicate), a retransmit is always safe — a late duplicate response for a
/// completed transaction is discarded as a stray by the transaction-id filter in
/// [`TurnClient::request_response`]. 250 ms yields ~20 attempts inside the 5 s
/// Allocate budget and ~8 inside the 2 s `CreatePermission` budget — ample slack
/// against occasional loss without a busy-wait.
const RTO: Duration = Duration::from_millis(250);

/// Long-term credentials for the TURN server (RFC 8656 §9.2).
#[derive(Clone, Debug)]
pub struct TurnConfig {
    /// The TURN server's `IP:port` (resolved by the caller; never a bare IP in
    /// config — see [`crate::DEFAULT_STUN_SERVERS`]).
    pub server: SocketAddr,
    /// The long-term-credential username.
    pub username: String,
    /// The long-term-credential password / shared secret.
    pub credential: String,
}

/// A TURN client bound to a UDP socket: allocates a relayed address, installs
/// peer permissions, and frames gameplay datagrams through the server.
///
/// Native-only (it owns a [`UdpSocket`]). The framing helpers
/// ([`Self::wrap`] / [`Self::unwrap`]) are pure and unit-tested; the live
/// [`Self::allocate`] needs a reachable `coturn` and is exercised by the
/// `#[ignore]`d `turn_probe` integration test.
#[derive(Debug)]
pub struct TurnClient {
    server: SocketAddr,
    username: String,
    credential: String,
    realm: String,
    nonce: Vec<u8>,
    relayed: Option<SocketAddr>,
    rng: SplitMix64,
}

impl TurnClient {
    /// Allocate a relayed transport address on `cfg.server` using the
    /// long-term-credential flow, returning a client whose
    /// [`relayed_addr`](Self::relayed_addr) is the public address peers send to.
    ///
    /// `socket` is the bound UDP socket the relay traffic flows over (the same
    /// kind of socket the game would otherwise use directly). `timeout` bounds
    /// the whole two-step transaction. `seed` seeds the transaction-id PRNG
    /// (deterministic for tests).
    ///
    /// # Errors
    ///
    /// Returns a socket error, `TimedOut` if the server never answers, or
    /// `InvalidData` if the server rejects the allocation or sends a response
    /// without a usable `XOR-RELAYED-ADDRESS`.
    pub fn allocate(
        socket: &UdpSocket,
        cfg: &TurnConfig,
        timeout: Duration,
        seed: u64,
    ) -> io::Result<Self> {
        let mut client = Self {
            server: cfg.server,
            username: cfg.username.clone(),
            credential: cfg.credential.clone(),
            realm: String::new(),
            nonce: Vec::new(),
            relayed: None,
            rng: SplitMix64::new(seed),
        };
        let prev_timeout = socket.read_timeout()?;
        socket.set_read_timeout(Some(timeout))?;
        let result = client.allocate_inner(socket, timeout);
        let _ = socket.set_read_timeout(prev_timeout);
        result?;
        Ok(client)
    }

    /// The server-assigned relayed transport address (the public address peers
    /// send their gameplay datagrams to), once [`allocate`](Self::allocate) has
    /// succeeded.
    #[must_use]
    pub const fn relayed_addr(&self) -> Option<SocketAddr> {
        self.relayed
    }

    /// The TURN server's address (where `wrap`ped datagrams are sent).
    #[must_use]
    pub const fn server_addr(&self) -> SocketAddr {
        self.server
    }

    fn allocate_inner(&mut self, socket: &UdpSocket, timeout: Duration) -> io::Result<()> {
        // Step 1: an unauthenticated Allocate. The server replies 401 with the
        // REALM + NONCE we must echo in the authenticated retry.
        let tx1 = self.next_tx();
        let probe = self.build_allocate(&tx1, false);
        let (msg_type, attrs) = self.request_response(socket, &probe, &tx1, timeout)?;
        if msg_type == MSG_ALLOCATE_SUCCESS {
            // An open relay answered the unauthenticated probe directly.
            return self.adopt_relayed(&attrs, &tx1);
        }
        if msg_type != MSG_ALLOCATE_ERROR {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TURN: unexpected Allocate response type",
            ));
        }
        // Pull REALM + NONCE for the authenticated retry.
        self.realm = find_attr(&attrs, ATTR_REALM)
            .and_then(|v| String::from_utf8(v.to_vec()).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TURN: 401 without REALM"))?;
        self.nonce = find_attr(&attrs, ATTR_NONCE)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TURN: 401 without NONCE"))?;

        // Step 2: the authenticated Allocate, carrying USERNAME/REALM/NONCE +
        // MESSAGE-INTEGRITY over the long-term key.
        let tx2 = self.next_tx();
        let req = self.build_allocate(&tx2, true);
        let (msg_type, attrs) = self.request_response(socket, &req, &tx2, timeout)?;
        if msg_type != MSG_ALLOCATE_SUCCESS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TURN: authenticated Allocate rejected",
            ));
        }
        self.adopt_relayed(&attrs, &tx2)
    }

    /// Adopt the `XOR-RELAYED-ADDRESS` from an Allocate success. `tx` is the
    /// success response's transaction id (needed to decode the v6 XOR key; the
    /// common v4 case ignores it).
    fn adopt_relayed(&mut self, attrs: &[u8], tx: &TransactionId) -> io::Result<()> {
        let relayed = find_attr(attrs, ATTR_XOR_RELAYED_ADDRESS)
            .and_then(|v| decode_xor_address(v, tx))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "TURN: Allocate success without XOR-RELAYED-ADDRESS",
                )
            })?;
        self.relayed = Some(relayed);
        Ok(())
    }

    /// Install a `CreatePermission` for `peer` so the server will relay data
    /// to/from it. Must be called for each peer before [`wrap`](Self::wrap)ping
    /// to it.
    ///
    /// # Errors
    ///
    /// Returns a socket error, `TimedOut`, or `InvalidData` on a rejected
    /// permission.
    pub fn create_permission(
        &mut self,
        socket: &UdpSocket,
        peer: SocketAddr,
        timeout: Duration,
    ) -> io::Result<()> {
        let prev_timeout = socket.read_timeout()?;
        let tx = self.next_tx();
        let req = self.build_create_permission(&tx, peer);
        // `request_response` owns the send + retransmit + bounded receive loop
        // (it sets the per-attempt read timeout itself), so no single dropped
        // request/response datagram can hard-fail the permission install.
        let result =
            self.request_response(socket, &req, &tx, timeout)
                .and_then(|(msg_type, _attrs)| {
                    if msg_type == MSG_CREATE_PERMISSION_SUCCESS {
                        Ok(())
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "TURN: CreatePermission rejected",
                        ))
                    }
                });
        let _ = socket.set_read_timeout(prev_timeout);
        result
    }

    /// Encode a **Send Indication** carrying `payload` destined for `peer`: the
    /// datagram to `send_to` the **TURN server**, which forwards `payload` to
    /// `peer`. Pure (no I/O); the gameplay-hot-path encoder.
    #[must_use]
    pub fn wrap(&mut self, peer: SocketAddr, payload: &[u8]) -> Vec<u8> {
        let tx = self.next_tx();
        let mut out = Vec::with_capacity(HEADER_LEN + 12 + payload.len() + 4);
        // Header: type, length (filled below), magic cookie, tx id.
        out.extend_from_slice(&MSG_SEND_INDICATION.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // placeholder length
        out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        out.extend_from_slice(&tx);
        push_xor_peer_address(&mut out, peer, &tx);
        push_attr(&mut out, ATTR_DATA, payload);
        patch_length(&mut out);
        out
    }

    /// Decode an inbound **Data Indication** from the TURN server back into the
    /// originating `peer` address and the `payload` it carried. Returns `None`
    /// for any datagram that is not a well-formed Data Indication (a stray STUN
    /// response, junk, a truncated frame) — never panics. Pure (no I/O); the
    /// gameplay-hot-path decoder.
    #[must_use]
    pub fn unwrap(datagram: &[u8]) -> Option<(SocketAddr, Vec<u8>)> {
        if datagram.len() < HEADER_LEN {
            return None;
        }
        let msg_type = u16::from_be_bytes([datagram[0], datagram[1]]);
        if msg_type != MSG_DATA_INDICATION {
            return None;
        }
        let cookie = u32::from_be_bytes([datagram[4], datagram[5], datagram[6], datagram[7]]);
        if cookie != MAGIC_COOKIE {
            return None;
        }
        let msg_len = u16::from_be_bytes([datagram[2], datagram[3]]) as usize;
        let tx: TransactionId = datagram[8..20].try_into().ok()?;
        let attrs = datagram.get(HEADER_LEN..HEADER_LEN.checked_add(msg_len)?)?;
        let mut peer: Option<SocketAddr> = None;
        let mut data: Option<Vec<u8>> = None;
        for (ty, val) in AttrIter::new(attrs) {
            match ty {
                ATTR_XOR_PEER_ADDRESS => peer = decode_xor_address(val, &tx),
                ATTR_DATA => data = Some(val.to_vec()),
                _ => {}
            }
        }
        Some((peer?, data?))
    }

    // ── internal encoders ───────────────────────────────────────────────────

    fn next_tx(&mut self) -> TransactionId {
        let mut tx = [0u8; 12];
        for chunk in tx.chunks_mut(4) {
            let r = self.rng.next_u64().to_le_bytes();
            let n = chunk.len();
            chunk.copy_from_slice(&r[..n]);
        }
        tx
    }

    fn build_allocate(&self, tx: &TransactionId, authed: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(&MSG_ALLOCATE_REQUEST.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        out.extend_from_slice(tx);
        // REQUESTED-TRANSPORT = UDP.
        push_attr(
            &mut out,
            ATTR_REQUESTED_TRANSPORT,
            &[TRANSPORT_UDP, 0, 0, 0],
        );
        // LIFETIME.
        push_attr(
            &mut out,
            ATTR_LIFETIME,
            &DEFAULT_LIFETIME_SECS.to_be_bytes(),
        );
        if authed {
            self.push_credentials(&mut out);
        }
        patch_length(&mut out);
        out
    }

    fn build_create_permission(&self, tx: &TransactionId, peer: SocketAddr) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(&MSG_CREATE_PERMISSION_REQUEST.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        out.extend_from_slice(tx);
        push_xor_peer_address(&mut out, peer, tx);
        self.push_credentials(&mut out);
        patch_length(&mut out);
        out
    }

    /// Append USERNAME, REALM, NONCE, then MESSAGE-INTEGRITY (HMAC-SHA1 over the
    /// long-term key, computed over the message so far with its length already
    /// covering the trailing integrity attribute — RFC 8489 §14.6).
    fn push_credentials(&self, out: &mut Vec<u8>) {
        push_attr(out, ATTR_USERNAME, self.username.as_bytes());
        push_attr(out, ATTR_REALM, self.realm.as_bytes());
        push_attr(out, ATTR_NONCE, &self.nonce);
        // The MESSAGE-INTEGRITY is computed with the message Length field set as
        // if the 24-byte integrity attribute were already appended.
        let key = long_term_key(&self.username, &self.realm, &self.credential);
        let prev_len = u16::try_from(out.len().saturating_sub(HEADER_LEN)).unwrap_or(u16::MAX);
        let with_mi = prev_len.saturating_add(24); // 4 header + 20 HMAC
        out[2..4].copy_from_slice(&with_mi.to_be_bytes());
        let mac = hmac_sha1(&key, out);
        push_attr(out, ATTR_MESSAGE_INTEGRITY, &mac);
    }

    /// Send `request` to the server and wait up to `timeout` for the response
    /// matching `tx`, **retransmitting the request every [`RTO`]** until either a
    /// match arrives or the overall deadline elapses. Strays (wrong source / tx /
    /// cookie, or a late duplicate for a prior transaction) are skipped.
    ///
    /// This is the resilience heart of the TURN client. UDP is unreliable, so a
    /// single `send_to` + one blocking `recv` (the previous behaviour) turns any
    /// dropped request-or-response datagram into a hard transaction failure —
    /// observed as an intermittent `TURN allocate failed` on loopback under load
    /// (a loaded CI runner can silently drop a `127.0.0.1` datagram when a socket
    /// receive buffer briefly overflows). Retransmitting on the RTO recovers from
    /// that loss in line with RFC 5389 §7.2.1 (a fixed 250 ms RTO here, not the RFC default 500 ms + exponential backoff), and because the server
    /// answers a retransmit idempotently the recovery is transparent.
    ///
    /// The method drives the socket's read timeout itself (bounding each blocking
    /// `recv_from` to the sooner of the next retransmit or the deadline); callers
    /// that care about the prior read timeout restore it after returning.
    fn request_response(
        &self,
        socket: &UdpSocket,
        request: &[u8],
        tx: &TransactionId,
        timeout: Duration,
    ) -> io::Result<(u16, Vec<u8>)> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 1500];
        // Initial transmission, then retransmit on the RTO cadence below.
        socket.send_to(request, self.server)?;
        let mut next_retx = Instant::now() + RTO;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "TURN: no matching response before timeout",
                ));
            }
            // Retransmit if the RTO elapsed without a matching response yet.
            if now >= next_retx {
                socket.send_to(request, self.server)?;
                next_retx = now + RTO;
            }
            // Bound this blocking recv to the sooner of the next retransmit and
            // the overall deadline, clamped to a positive minimum so a WouldBlock/
            // TimedOut simply loops back to (maybe) retransmit rather than failing.
            let slice = next_retx
                .min(deadline)
                .saturating_duration_since(now)
                .max(Duration::from_millis(1));
            socket.set_read_timeout(Some(slice))?;
            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                // Transient, non-fatal receive outcomes — loop on to retransmit /
                // re-check the deadline, do NOT fail the transaction:
                //   - WouldBlock (Unix) / TimedOut (Windows): a read-timeout expiry
                //     ("no datagram this slice").
                //   - ConnectionReset / ConnectionRefused: on Windows a `recv_from`
                //     after a `send_to` to a port that is closed / not-yet-bound
                //     surfaces the ICMP "Port Unreachable" as `ConnectionReset`
                //     (Unix can raise `ConnectionRefused`). This is exactly the
                //     kind of startup-window transient that intermittently red-lit
                //     the loopback test; retransmitting recovers once the peer's
                //     socket is up.
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            };
            if from != self.server || len < HEADER_LEN {
                continue;
            }
            let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
            if cookie != MAGIC_COOKIE || &buf[8..20] != tx {
                continue;
            }
            let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
            let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
            let end = HEADER_LEN.checked_add(msg_len).filter(|&e| e <= len);
            let attrs = end.map_or_else(Vec::new, |e| buf[HEADER_LEN..e].to_vec());
            return Ok((msg_type, attrs));
        }
    }
}

/// A UDP socket wrapper that frames every datagram through a [`TurnClient`].
///
/// This lets a [`UdpTransport`](crate::UdpTransport) talk to a single logical
/// peer over a TURN relay with **no change** to the transport or session.
///
/// `send_to(_, peer)` TURN-`wrap`s the bytes and actually sends them to the TURN
/// server; `recv_from` reads from the server and TURN-`unwrap`s, returning the
/// originating *peer's* address. The caller therefore sees plain peer-addressed
/// UDP — exactly the [`UdpSocket`] surface [`UdpTransport`](crate::UdpTransport)
/// expects — while every byte rides the relay.
#[derive(Debug)]
pub struct RelayUdpSocket {
    socket: UdpSocket,
    turn: TurnClient,
}

impl RelayUdpSocket {
    /// Wrap an allocated [`TurnClient`] + its bound socket. The socket should be
    /// the SAME one the allocation ran over (its source port is what the TURN
    /// server bound the allocation to).
    #[must_use]
    pub const fn new(socket: UdpSocket, turn: TurnClient) -> Self {
        Self { socket, turn }
    }

    /// The relayed transport address peers send to (the TURN allocation).
    #[must_use]
    pub const fn relayed_addr(&self) -> Option<SocketAddr> {
        self.turn.relayed_addr()
    }

    /// Borrow the underlying socket (e.g. to set non-blocking / read timeouts).
    #[must_use]
    pub const fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    /// Send `payload` to `peer` through the relay: encodes a Send Indication and
    /// transmits it to the TURN server, which forwards it to `peer`.
    ///
    /// # Errors
    ///
    /// Returns any socket send error.
    pub fn send_to(&mut self, payload: &[u8], peer: SocketAddr) -> io::Result<usize> {
        let framed = self.turn.wrap(peer, payload);
        self.socket.send_to(&framed, self.turn.server_addr())
    }

    /// Receive one relayed datagram, returning `(bytes_written, peer_addr)` —
    /// the originating peer's address, NOT the TURN server's. A datagram from
    /// the server that is not a Data Indication (e.g. a stray) yields
    /// `WouldBlock` so the caller treats it as "nothing to read".
    ///
    /// # Errors
    ///
    /// Returns any socket receive error; a non-Data-Indication datagram is
    /// reported as `WouldBlock`.
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut raw = [0u8; 1500];
        let (len, from) = self.socket.recv_from(&mut raw)?;
        if from != self.turn.server_addr() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "non-relay source",
            ));
        }
        match TurnClient::unwrap(&raw[..len]) {
            Some((peer, payload)) => {
                let n = payload.len().min(buf.len());
                buf[..n].copy_from_slice(&payload[..n]);
                Ok((n, peer))
            }
            None => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "non-Data-Indication relay datagram",
            )),
        }
    }

    /// One step of a non-blocking *drain* over the relay socket, distinguishing
    /// three outcomes the plain [`recv_from`](Self::recv_from) collapses:
    ///
    /// - `Ok(Some((n, peer)))` — a valid Data Indication was decoded into `buf`;
    /// - `Ok(None)` — a datagram was read but it was a *stray* (not from the
    ///   relay server, or not a Data Indication), so the caller should KEEP
    ///   draining rather than stop;
    /// - `Err(WouldBlock)` — the socket is genuinely empty (stop draining);
    /// - any other `Err` — a real socket error.
    ///
    /// This is the primitive a relay-backed [`UdpTransport`](crate::UdpTransport)
    /// poll loop is built on: it must skip strays but stop on a true `WouldBlock`,
    /// which the single `WouldBlock`-for-both [`recv_from`](Self::recv_from)
    /// cannot express.
    ///
    /// # Errors
    ///
    /// Returns `WouldBlock` when the socket is empty, or any other socket
    /// receive error verbatim.
    pub fn recv_step(&self, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>> {
        let mut raw = [0u8; 1500];
        let (len, from) = self.socket.recv_from(&mut raw)?;
        if from != self.turn.server_addr() {
            // A stray from some other source: consumed, keep draining.
            return Ok(None);
        }
        match TurnClient::unwrap(&raw[..len]) {
            Some((peer, payload)) => {
                let n = payload.len().min(buf.len());
                buf[..n].copy_from_slice(&payload[..n]);
                Ok(Some((n, peer)))
            }
            // A non-Data-Indication frame from the server (e.g. a stray STUN
            // response): consumed, keep draining.
            None => Ok(None),
        }
    }
}

// ── shared TLV / attribute helpers ──────────────────────────────────────────

/// Append a STUN attribute `(type, value)` to `out`, padding the value to a
/// 4-byte boundary (RFC 5389 §15).
fn push_attr(out: &mut Vec<u8>, attr_type: u16, value: &[u8]) {
    out.extend_from_slice(&attr_type.to_be_bytes());
    out.extend_from_slice(&u16::try_from(value.len()).unwrap_or(u16::MAX).to_be_bytes());
    out.extend_from_slice(value);
    let pad = (4 - value.len() % 4) % 4;
    out.extend(std::iter::repeat_n(0u8, pad));
}

/// Append an XOR-PEER-ADDRESS attribute for `peer`.
fn push_xor_peer_address(out: &mut Vec<u8>, peer: SocketAddr, tx: &TransactionId) {
    let value = encode_xor_address(peer, tx);
    push_attr(out, ATTR_XOR_PEER_ADDRESS, &value);
}

/// Overwrite the STUN header Length field (bytes 2..4) with the current
/// attribute-section length.
fn patch_length(out: &mut [u8]) {
    let attr_len = u16::try_from(out.len().saturating_sub(HEADER_LEN)).unwrap_or(u16::MAX);
    out[2..4].copy_from_slice(&attr_len.to_be_bytes());
}

/// Encode a (XOR-)address attribute value for `addr`: reserved byte, family,
/// XOR-port, XOR-address (RFC 5389 §15.2; the IPv6 key is the cookie ++ tx id).
fn encode_xor_address(addr: SocketAddr, tx: &TransactionId) -> Vec<u8> {
    let cookie_be = MAGIC_COOKIE.to_be_bytes();
    #[allow(clippy::cast_possible_truncation)]
    let cookie_hi16 = (MAGIC_COOKIE >> 16) as u16;
    let x_port = addr.port() ^ cookie_hi16;
    let mut out = Vec::with_capacity(20);
    out.push(0);
    match addr.ip() {
        IpAddr::V4(v4) => {
            out.push(FAMILY_IPV4);
            out.extend_from_slice(&x_port.to_be_bytes());
            let mut a = v4.octets();
            for (b, k) in a.iter_mut().zip(cookie_be.iter()) {
                *b ^= *k;
            }
            out.extend_from_slice(&a);
        }
        IpAddr::V6(v6) => {
            out.push(FAMILY_IPV6);
            out.extend_from_slice(&x_port.to_be_bytes());
            let mut key = [0u8; 16];
            key[..4].copy_from_slice(&cookie_be);
            key[4..].copy_from_slice(tx);
            let mut a = v6.octets();
            for (b, k) in a.iter_mut().zip(key.iter()) {
                *b ^= *k;
            }
            out.extend_from_slice(&a);
        }
    }
    out
}

/// Decode a (XOR-)address attribute value into a [`SocketAddr`]. `None` on a
/// short / unknown-family value.
fn decode_xor_address(value: &[u8], tx: &TransactionId) -> Option<SocketAddr> {
    if value.len() < 4 {
        return None;
    }
    let family = value[1];
    let cookie_be = MAGIC_COOKIE.to_be_bytes();
    #[allow(clippy::cast_possible_truncation)]
    let cookie_hi16 = (MAGIC_COOKIE >> 16) as u16;
    let port = u16::from_be_bytes([value[2], value[3]]) ^ cookie_hi16;
    match family {
        FAMILY_IPV4 => {
            let octets: [u8; 4] = value.get(4..8)?.try_into().ok()?;
            let mut a = octets;
            for (b, k) in a.iter_mut().zip(cookie_be.iter()) {
                *b ^= *k;
            }
            Some(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(a), port)))
        }
        FAMILY_IPV6 => {
            let octets: [u8; 16] = value.get(4..20)?.try_into().ok()?;
            let mut key = [0u8; 16];
            key[..4].copy_from_slice(&cookie_be);
            key[4..].copy_from_slice(tx);
            let mut a = octets;
            for (b, k) in a.iter_mut().zip(key.iter()) {
                *b ^= *k;
            }
            Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(a),
                port,
                0,
                0,
            )))
        }
        _ => None,
    }
}

/// Find the first attribute of `attr_type` in a STUN attribute section,
/// returning its (unpadded) value.
fn find_attr(attrs: &[u8], attr_type: u16) -> Option<&[u8]> {
    AttrIter::new(attrs).find_map(|(ty, val)| (ty == attr_type).then_some(val))
}

/// Iterate `(attr_type, value)` over a STUN attribute section, honoring the
/// 4-byte padding. Stops at the first truncated attribute.
struct AttrIter<'a> {
    rest: &'a [u8],
}

impl<'a> AttrIter<'a> {
    const fn new(attrs: &'a [u8]) -> Self {
        Self { rest: attrs }
    }
}

impl<'a> Iterator for AttrIter<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.len() < 4 {
            return None;
        }
        let ty = u16::from_be_bytes([self.rest[0], self.rest[1]]);
        let len = u16::from_be_bytes([self.rest[2], self.rest[3]]) as usize;
        let value = self.rest.get(4..4 + len)?;
        let padded = 4 + len.div_ceil(4) * 4;
        self.rest = self.rest.get(padded..).unwrap_or(&[]);
        Some((ty, value))
    }
}

// ── HMAC-SHA1 + the long-term key (RFC 8489 §9.2.2) ─────────────────────────

/// The long-term-credential key: `MD5(username ":" realm ":" password)`.
fn long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    md5(format!("{username}:{realm}:{password}").as_bytes())
}

/// HMAC-SHA1 of `data` under `key`, the MESSAGE-INTEGRITY MAC (20 bytes).
#[allow(clippy::needless_range_loop)]
fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let digest = sha1(key);
        k[..20].copy_from_slice(&digest);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Vec::with_capacity(BLOCK + data.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(data);
    let inner_digest = sha1(&inner);
    let mut outer = Vec::with_capacity(BLOCK + 20);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_digest);
    sha1(&outer)
}

/// SHA-1 (FIPS 180-4) of `data`, 20 bytes. Used only for the TURN
/// MESSAGE-INTEGRITY HMAC (not security-sensitive beyond matching the server).
#[allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::needless_range_loop,
    clippy::tuple_array_conversions
)]
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let ml = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// MD5 (RFC 1321) of `data`, 16 bytes. Used only to derive the TURN long-term
/// key (`MD5(user:realm:pass)`) — required by the protocol, not as a security
/// primitive.
#[allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]
fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76a_a478,
        0xe8c7_b756,
        0x2420_70db,
        0xc1bd_ceee,
        0xf57c_0faf,
        0x4787_c62a,
        0xa830_4613,
        0xfd46_9501,
        0x6980_98d8,
        0x8b44_f7af,
        0xffff_5bb1,
        0x895c_d7be,
        0x6b90_1122,
        0xfd98_7193,
        0xa679_438e,
        0x49b4_0821,
        0xf61e_2562,
        0xc040_b340,
        0x265e_5a51,
        0xe9b6_c7aa,
        0xd62f_105d,
        0x0244_1453,
        0xd8a1_e681,
        0xe7d3_fbc8,
        0x21e1_cde6,
        0xc337_07d6,
        0xf4d5_0d87,
        0x455a_14ed,
        0xa9e3_e905,
        0xfcef_a3f8,
        0x676f_02d9,
        0x8d2a_4c8a,
        0xfffa_3942,
        0x8771_f681,
        0x6d9d_6122,
        0xfde5_380c,
        0xa4be_ea44,
        0x4bde_cfa9,
        0xf6bb_4b60,
        0xbebf_bc70,
        0x289b_7ec6,
        0xeaa1_27fa,
        0xd4ef_3085,
        0x0488_1d05,
        0xd9d4_d039,
        0xe6db_99e5,
        0x1fa2_7cf8,
        0xc4ac_5665,
        0xf429_2244,
        0x432a_ff97,
        0xab94_23a7,
        0xfc93_a039,
        0x655b_59c3,
        0x8f0c_cc92,
        0xffef_f47d,
        0x8584_5dd1,
        0x6fa8_7e4f,
        0xfe2c_e6e0,
        0xa301_4314,
        0x4e08_11a1,
        0xf753_7e82,
        0xbd3a_f235,
        0x2ad7_d2bb,
        0xeb86_d391,
    ];
    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;
    let ml = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_le_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
#[allow(clippy::tuple_array_conversions)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_known_vectors() {
        // FIPS 180-4 / well-known vectors.
        assert_eq!(
            sha1(b""),
            [
                0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09
            ]
        );
        assert_eq!(
            sha1(b"abc"),
            [
                0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e, 0x25, 0x71, 0x78, 0x50,
                0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d
            ]
        );
    }

    #[test]
    fn md5_matches_known_vectors() {
        assert_eq!(
            md5(b""),
            [
                0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
                0x42, 0x7e
            ]
        );
        assert_eq!(
            md5(b"abc"),
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
                0x7f, 0x72
            ]
        );
    }

    #[test]
    fn hmac_sha1_matches_rfc2202_vector() {
        // RFC 2202 test case 1: key = 0x0b * 20, data = "Hi There".
        let key = [0x0bu8; 20];
        let mac = hmac_sha1(&key, b"Hi There");
        assert_eq!(
            mac,
            [
                0xb6, 0x17, 0x31, 0x86, 0x55, 0x05, 0x72, 0x64, 0xe2, 0x8b, 0xc0, 0xb6, 0xfb, 0x37,
                0x8c, 0x8e, 0xf1, 0x46, 0xbe, 0x00
            ]
        );
    }

    #[test]
    fn xor_address_roundtrips_v4_and_v6() {
        let tx = [0x5Au8; 12];
        for addr in [
            SocketAddr::from((Ipv4Addr::new(203, 0, 113, 7), 51_234)),
            SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x42), 40_000)),
        ] {
            let value = encode_xor_address(addr, &tx);
            let back = decode_xor_address(&value, &tx).expect("decode");
            assert_eq!(back, addr);
        }
    }

    /// Build a synthetic Data Indication (as a TURN server would send) and assert
    /// `unwrap` recovers the peer + payload.
    #[test]
    fn unwrap_decodes_a_data_indication() {
        let tx = [0x11u8; 12];
        let peer = SocketAddr::from((Ipv4Addr::new(198, 51, 100, 9), 12_345));
        let payload = b"netplay-frame";
        let mut dg = Vec::new();
        dg.extend_from_slice(&MSG_DATA_INDICATION.to_be_bytes());
        dg.extend_from_slice(&0u16.to_be_bytes());
        dg.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        dg.extend_from_slice(&tx);
        push_xor_peer_address(&mut dg, peer, &tx);
        push_attr(&mut dg, ATTR_DATA, payload);
        patch_length(&mut dg);

        let (got_peer, got_payload) = TurnClient::unwrap(&dg).expect("unwrap data indication");
        assert_eq!(got_peer, peer);
        assert_eq!(got_payload, payload);
    }

    /// `wrap` then `unwrap` round-trips a payload + peer (the encoder is the
    /// inverse of the decoder over a Send/Data indication body).
    #[test]
    fn wrap_then_unwrap_roundtrips() {
        let cfg = TurnConfig {
            server: SocketAddr::from((Ipv4Addr::LOCALHOST, 3478)),
            username: "user".into(),
            credential: "pass".into(),
        };
        // Construct a client WITHOUT allocating (we only exercise the framing).
        let mut client = TurnClient {
            server: cfg.server,
            username: cfg.username,
            credential: cfg.credential,
            realm: String::new(),
            nonce: Vec::new(),
            relayed: None,
            rng: SplitMix64::new(0xABCD),
        };
        let peer = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 33), 6000));
        let payload = b"hello over turn";
        // A Send Indication and a Data Indication share the body layout
        // (XOR-PEER-ADDRESS + DATA); only the method differs, so re-tag the
        // wrapped Send Indication as a Data Indication for the unwrap test.
        let mut framed = client.wrap(peer, payload);
        framed[0..2].copy_from_slice(&MSG_DATA_INDICATION.to_be_bytes());
        let (got_peer, got_payload) = TurnClient::unwrap(&framed).expect("roundtrip");
        assert_eq!(got_peer, peer);
        assert_eq!(got_payload, payload);
    }

    #[test]
    fn unwrap_rejects_non_data_indications() {
        assert!(TurnClient::unwrap(&[]).is_none());
        assert!(TurnClient::unwrap(&[0u8; 10]).is_none());
        // A well-formed Allocate success is not a Data Indication.
        let mut dg = Vec::new();
        dg.extend_from_slice(&MSG_ALLOCATE_SUCCESS.to_be_bytes());
        dg.extend_from_slice(&0u16.to_be_bytes());
        dg.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        dg.extend_from_slice(&[0u8; 12]);
        assert!(TurnClient::unwrap(&dg).is_none());
    }

    /// A loopback "mock relay": one socket plays the TURN server, echoing each
    /// Send Indication back as a Data Indication. Proves the [`RelayUdpSocket`]
    /// shim sends + receives peer-addressed datagrams over the relay framing.
    #[test]
    fn relay_socket_loopback_roundtrip() {
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let server_addr = server.local_addr().unwrap();
        let client_sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();

        let turn = TurnClient {
            server: server_addr,
            username: "u".into(),
            credential: "p".into(),
            realm: String::new(),
            nonce: Vec::new(),
            relayed: Some(SocketAddr::from((Ipv4Addr::new(203, 0, 113, 1), 49_000))),
            rng: SplitMix64::new(1),
        };
        let mut relay = RelayUdpSocket::new(client_sock, turn);
        let peer = SocketAddr::from((Ipv4Addr::new(198, 51, 100, 2), 6000));

        // Client sends a payload to `peer` via the relay.
        relay.send_to(b"ping", peer).unwrap();

        // The mock server receives the Send Indication, decodes it, and echoes a
        // Data Indication carrying the same peer + payload back to the client.
        let mut buf = [0u8; 1500];
        let (len, from) = server.recv_from(&mut buf).unwrap();
        let (got_peer, payload) = TurnClient::unwrap(&buf[..len])
            .or_else(|| {
                // Re-tag the Send Indication as Data for decoding (same body).
                let mut v = buf[..len].to_vec();
                v[0..2].copy_from_slice(&MSG_DATA_INDICATION.to_be_bytes());
                TurnClient::unwrap(&v)
            })
            .expect("server decodes send indication");
        assert_eq!(got_peer, peer);
        assert_eq!(&payload, b"ping");

        // Build the echo Data Indication and send it back.
        let mut echo = Vec::new();
        echo.extend_from_slice(&MSG_DATA_INDICATION.to_be_bytes());
        echo.extend_from_slice(&0u16.to_be_bytes());
        echo.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        echo.extend_from_slice(&[0x22u8; 12]);
        push_xor_peer_address(&mut echo, got_peer, &[0x22u8; 12]);
        push_attr(&mut echo, ATTR_DATA, &payload);
        patch_length(&mut echo);
        server.send_to(&echo, from).unwrap();

        // The client's RelayUdpSocket recv_from returns the PEER address + bytes.
        let (n, recv_peer) = relay.recv_from(&mut buf).unwrap();
        assert_eq!(recv_peer, peer);
        assert_eq!(&buf[..n], b"ping");
    }
}
