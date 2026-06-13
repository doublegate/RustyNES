//! v2.5.0 Phase C: a minimal STUN client (RFC 5389) for NAT traversal, plus a
//! UDP hole-punch coordination state machine.
//!
//! # What this is
//!
//! For two peers behind home NATs to reach each other directly, each must first
//! discover its own **public** (server-reflexive) `SocketAddr` — the `IP:port`
//! the NAT presents to the outside world. A [STUN](https://datatracker.ietf.org/doc/html/rfc5389)
//! server does exactly that: a peer sends a *Binding Request* from its UDP
//! socket, and the server replies with a *Binding Success Response* carrying the
//! source address it observed (the peer's public mapping) in an
//! **XOR-MAPPED-ADDRESS** attribute.
//!
//! This module implements:
//!
//! - [`build_binding_request`] — encodes the 20-byte STUN Binding Request header
//!   (no attributes; a basic request needs none) with a fresh random 96-bit
//!   transaction id.
//! - [`parse_binding_response`] — parses a Binding Success Response, extracting
//!   the public [`SocketAddr`] from **XOR-MAPPED-ADDRESS** (`0x0020`), falling
//!   back to the deprecated **MAPPED-ADDRESS** (`0x0001`) if that is all the
//!   server sent. IPv4 and IPv6 are both decoded. Malformed / truncated / wrong-
//!   cookie / non-success responses are rejected (`None`), never panic.
//! - [`StunClient`] — ties a request to its response: it remembers the
//!   transaction id it generated so a response with a different id (a stray
//!   datagram) is rejected.
//! - [`HolePunch`] — a small state machine modelling the
//!   discovering → punching → connected handshake two peers run once each has
//!   learned the other's public address (exchanged out of band / via signaling).
//!
//! # What is NOT here (documented-pending)
//!
//! Real cross-NAT traversal needs a reachable STUN server and two real NATs.
//! The encode/decode and the state machine are unit-tested headlessly here; the
//! live round-trip is an `#[ignore]`d integration probe (see the crate's
//! `tests/`), and the symmetric-NAT / TURN-relay fallback is out of scope. See
//! `docs/netplay-webrtc.md`.
//!
//! All socket use in this module is gated to native (`cfg(not(wasm32))`); the
//! pure encode/decode functions are portable but the [`StunClient`]'s
//! [`UdpSocket`](std::net::UdpSocket) round-trip is native-only.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::rng::SplitMix64;

/// The STUN magic cookie (RFC 5389 §6). Present in bytes 4..8 of every header
/// and `XOR`ed into the `XOR-MAPPED-ADDRESS` attribute.
pub const MAGIC_COOKIE: u32 = 0x2112_A442;

/// STUN message type: Binding Request (class = request, method = binding).
const MSG_BINDING_REQUEST: u16 = 0x0001;
/// STUN message type: Binding Success Response.
const MSG_BINDING_SUCCESS: u16 = 0x0101;

/// Attribute type: XOR-MAPPED-ADDRESS (RFC 5389 §15.2).
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
/// Attribute type: MAPPED-ADDRESS (deprecated; RFC 5389 §15.1). Fallback only.
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;

/// Address family marker for IPv4 inside a (XOR-)MAPPED-ADDRESS attribute.
const FAMILY_IPV4: u8 = 0x01;
/// Address family marker for IPv6.
const FAMILY_IPV6: u8 = 0x02;

/// The fixed STUN header length in bytes (RFC 5389 §6).
pub const HEADER_LEN: usize = 20;

/// A STUN transaction id: a random 96-bit value the client generates per
/// request and matches against the response so a stray datagram is rejected.
pub type TransactionId = [u8; 12];

/// Build a STUN Binding Request: the 20-byte header, no attributes.
///
/// Returns the encoded bytes plus a fresh random transaction id (the caller —
/// or `StunClient` — keeps the id to validate the matching response).
///
/// Layout (RFC 5389 §6):
/// - `[0..2]`  message type   = `0x0001` (Binding Request)
/// - `[2..4]`  message length = `0` (no attributes)
/// - `[4..8]`  magic cookie   = `0x2112A442`
/// - `[8..20]` transaction id = 96 random bits
#[must_use]
pub fn build_binding_request(rng: &mut SplitMix64) -> (Vec<u8>, TransactionId) {
    let mut tx_id = [0u8; 12];
    // Fill the 12-byte transaction id from the seeded PRNG (three u32 draws).
    for chunk in tx_id.chunks_mut(4) {
        let r = rng.next_u64().to_le_bytes();
        let n = chunk.len();
        chunk.copy_from_slice(&r[..n]);
    }
    let mut buf = Vec::with_capacity(HEADER_LEN);
    buf.extend_from_slice(&MSG_BINDING_REQUEST.to_be_bytes());
    buf.extend_from_slice(&0u16.to_be_bytes()); // message length: no attributes
    buf.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    buf.extend_from_slice(&tx_id);
    debug_assert_eq!(buf.len(), HEADER_LEN);
    (buf, tx_id)
}

/// Parse a STUN Binding Success Response, returning this peer's public
/// [`SocketAddr`] as the server observed it.
///
/// Prefers the **XOR-MAPPED-ADDRESS** (`0x0020`) attribute; falls back to the
/// deprecated **MAPPED-ADDRESS** (`0x0001`) if only that is present. Returns
/// `None` (never panics) if the buffer is too short, the magic cookie is wrong,
/// the message type is not a Binding Success, the declared length overruns the
/// buffer, `expected_tx` is supplied and does not match, or no usable address
/// attribute is found.
///
/// `expected_tx`, when `Some`, must equal the response's transaction id — this
/// is how a stray / replayed datagram is rejected.
#[must_use]
pub fn parse_binding_response(
    buf: &[u8],
    expected_tx: Option<&TransactionId>,
) -> Option<SocketAddr> {
    if buf.len() < HEADER_LEN {
        return None;
    }
    let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
    if msg_type != MSG_BINDING_SUCCESS {
        return None;
    }
    let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if cookie != MAGIC_COOKIE {
        return None;
    }
    let tx_id: TransactionId = buf[8..20].try_into().ok()?;
    if let Some(want) = expected_tx {
        if &tx_id != want {
            return None;
        }
    }
    // The attribute section must fit exactly within the buffer.
    let attrs = buf.get(HEADER_LEN..HEADER_LEN.checked_add(msg_len)?)?;

    // Walk the TLV attributes; remember a MAPPED-ADDRESS fallback but prefer
    // XOR-MAPPED-ADDRESS, returning the latter as soon as it decodes.
    let mut fallback: Option<SocketAddr> = None;
    let mut offset = 0usize;
    while offset + 4 <= attrs.len() {
        let attr_type = u16::from_be_bytes([attrs[offset], attrs[offset + 1]]);
        let attr_len = u16::from_be_bytes([attrs[offset + 2], attrs[offset + 3]]) as usize;
        let value_start = offset + 4;
        let value_end = value_start.checked_add(attr_len)?;
        let value = attrs.get(value_start..value_end)?;
        match attr_type {
            ATTR_XOR_MAPPED_ADDRESS => {
                if let Some(addr) = decode_address(value, &tx_id, true) {
                    return Some(addr);
                }
            }
            ATTR_MAPPED_ADDRESS if fallback.is_none() => {
                fallback = decode_address(value, &tx_id, false);
            }
            _ => {} // ignore unknown attributes (comprehension-optional)
        }
        // Attributes are padded to a 4-byte boundary (RFC 5389 §15).
        let padded = attr_len.div_ceil(4) * 4;
        offset = value_start.checked_add(padded)?;
    }
    fallback
}

/// Decode a (XOR-)MAPPED-ADDRESS attribute value into a [`SocketAddr`].
///
/// Value layout: `[0]` reserved, `[1]` family, `[2..4]` port, `[4..]` address.
/// When `xor` is true (`XOR-MAPPED-ADDRESS`), the port is `XOR`ed with the high
/// 16 bits of the magic cookie and each address byte with the cookie-then-
/// transaction-id key (RFC 5389 §15.2). When false (`MAPPED-ADDRESS`) the bytes
/// are used verbatim.
fn decode_address(value: &[u8], tx_id: &TransactionId, xor: bool) -> Option<SocketAddr> {
    if value.len() < 4 {
        return None;
    }
    let family = value[1];
    let raw_port = u16::from_be_bytes([value[2], value[3]]);
    let cookie_be = MAGIC_COOKIE.to_be_bytes();
    // High 16 bits of the magic cookie, used to XOR the port.
    #[allow(clippy::cast_possible_truncation)]
    let cookie_hi16 = (MAGIC_COOKIE >> 16) as u16;
    let port = if xor {
        // X-Port = port XOR (most-significant 16 bits of the magic cookie).
        raw_port ^ cookie_hi16
    } else {
        raw_port
    };
    match family {
        FAMILY_IPV4 => {
            let octets: [u8; 4] = value.get(4..8)?.try_into().ok()?;
            let addr = if xor {
                // X-Address = address XOR magic cookie (big-endian).
                let mut a = octets;
                for (b, k) in a.iter_mut().zip(cookie_be.iter()) {
                    *b ^= *k;
                }
                a
            } else {
                octets
            };
            Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from(addr),
                port,
            )))
        }
        FAMILY_IPV6 => {
            let octets: [u8; 16] = value.get(4..20)?.try_into().ok()?;
            let addr = if xor {
                // X-Address = address XOR (magic cookie ++ transaction id).
                let mut key = [0u8; 16];
                key[..4].copy_from_slice(&cookie_be);
                key[4..].copy_from_slice(tx_id);
                let mut a = octets;
                for (b, k) in a.iter_mut().zip(key.iter()) {
                    *b ^= *k;
                }
                a
            } else {
                octets
            };
            Some(SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(addr),
                port,
                0,
                0,
            )))
        }
        _ => None,
    }
}

/// A STUN client bound to a UDP socket: sends one Binding Request and matches
/// the response by transaction id to discover this peer's public address.
///
/// Native-only (it owns a [`UdpSocket`](std::net::UdpSocket)). The encode /
/// decode functions ([`build_binding_request`] / [`parse_binding_response`])
/// are portable and used directly by the tests.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct StunClient {
    socket: std::net::UdpSocket,
    last_tx: Option<TransactionId>,
    rng: SplitMix64,
}

#[cfg(not(target_arch = "wasm32"))]
impl StunClient {
    /// Wrap an already-bound UDP socket. `seed` seeds the transaction-id PRNG.
    /// The socket is **not** reconfigured (the caller may share it with a
    /// [`UdpTransport`](crate::UdpTransport)); blocking vs. non-blocking is the
    /// caller's choice — [`discover`](Self::discover) drives the round-trip with
    /// an explicit timeout regardless.
    #[must_use]
    pub const fn new(socket: std::net::UdpSocket, seed: u64) -> Self {
        Self {
            socket,
            last_tx: None,
            rng: SplitMix64::new(seed),
        }
    }

    /// The transaction id of the most recent request, if one has been sent.
    /// Exposed so a caller draining a shared socket can match responses itself.
    #[must_use]
    pub const fn last_transaction_id(&self) -> Option<TransactionId> {
        self.last_tx
    }

    /// Encode + send one Binding Request to `stun_server`, recording the
    /// transaction id for later matching. Does not wait for the response — call
    /// [`recv_response`](Self::recv_response) (or drain a shared socket and call
    /// [`parse_binding_response`] with [`last_transaction_id`](Self::last_transaction_id)).
    ///
    /// # Errors
    ///
    /// Returns any socket send error.
    pub fn send_request(&mut self, stun_server: SocketAddr) -> std::io::Result<()> {
        let (req, tx) = build_binding_request(&mut self.rng);
        self.last_tx = Some(tx);
        self.socket.send_to(&req, stun_server)?;
        Ok(())
    }

    /// Try to receive + parse a Binding Success Response on the socket, matching
    /// it against the last request's transaction id. Returns `Ok(Some(addr))` on
    /// a valid matching response, `Ok(None)` if a datagram arrived but was not a
    /// matching success response (a stray packet — the caller may retry), and an
    /// error only on a real socket failure (`WouldBlock` is surfaced so a
    /// non-blocking caller can poll).
    ///
    /// # Errors
    ///
    /// Returns any socket receive error (including `WouldBlock`).
    pub fn recv_response(&mut self) -> std::io::Result<Option<SocketAddr>> {
        let mut buf = [0u8; 512];
        let (len, _from) = self.socket.recv_from(&mut buf)?;
        Ok(parse_binding_response(&buf[..len], self.last_tx.as_ref()))
    }

    /// Blocking convenience: send a request and wait up to `timeout` for a
    /// matching Binding Success Response, returning the discovered public
    /// address. Sets a read timeout on the socket for the duration.
    ///
    /// This is the simplest entry point for native NAT discovery; for a socket
    /// shared with the live netplay [`UdpTransport`](crate::UdpTransport), prefer
    /// the non-blocking [`send_request`](Self::send_request) +
    /// [`parse_binding_response`] path so STUN traffic and game traffic share one
    /// drain.
    ///
    /// # Errors
    ///
    /// Returns a socket error, or `TimedOut` if no matching response arrived.
    pub fn discover(
        &mut self,
        stun_server: SocketAddr,
        timeout: std::time::Duration,
    ) -> std::io::Result<SocketAddr> {
        use std::time::Instant;
        let prev_timeout = self.socket.read_timeout()?;
        self.socket.set_read_timeout(Some(timeout))?;
        self.send_request(stun_server)?;
        let deadline = Instant::now() + timeout;
        let result = loop {
            if Instant::now() >= deadline {
                break Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "STUN: no matching Binding Success Response before timeout",
                ));
            }
            match self.recv_response() {
                Ok(Some(addr)) => break Ok(addr),
                Ok(None) => {} // stray datagram; keep waiting until the deadline
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break Err(e),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    break Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "STUN: no matching Binding Success Response before timeout",
                    ));
                }
                Err(e) => break Err(e),
            }
        };
        // Restore the socket's prior read timeout regardless of outcome.
        let _ = self.socket.set_read_timeout(prev_timeout);
        result
    }
}

/// The phase of a [`HolePunch`] coordination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PunchState {
    /// Still learning this peer's public reflexive address via STUN. No peer
    /// address is known yet.
    Discovering,
    /// Both public addresses are known; we are sending punch packets at the
    /// peer's public address to open the NAT mapping in both directions, and
    /// waiting to receive the peer's punch packet.
    Punching,
    /// A punch packet has been received from the peer's public address: the
    /// bidirectional UDP mapping is open and direct traffic can flow.
    Connected,
}

/// A minimal UDP hole-punch coordination state machine.
///
/// # Protocol
///
/// 1. Each peer discovers its own public reflexive address via STUN
///    ([`local_discovered`](Self::local_discovered) records it).
/// 2. The two public addresses are exchanged **out of band** — through the
///    existing [`Sync`](crate::NetMessage::Sync) handshake once a relay/signaling
///    path is available, or a documented manual step. Each peer records the
///    other's via [`peer_discovered`](Self::peer_discovered), moving to
///    [`PunchState::Punching`].
/// 3. Both peers send a punch packet (a [`Sync`](crate::NetMessage::Sync) is a
///    fine choice — it doubles as the handshake) to the other's public address
///    *simultaneously*. The first outbound packet from each side opens its own
///    NAT's mapping; the peer's matching packet then traverses it.
/// 4. On receiving the peer's punch packet ([`punch_received`](Self::punch_received)),
///    the state becomes [`PunchState::Connected`] and the live
///    [`UdpTransport`](crate::UdpTransport) can use the peer's public address as
///    its remote.
///
/// This struct is pure state — it performs no I/O — so it is portable and fully
/// unit-testable. The caller wires it to a socket (sending punch packets while
/// in [`PunchState::Punching`], feeding received packets to
/// [`punch_received`](Self::punch_received)).
#[derive(Clone, Copy, Debug)]
pub struct HolePunch {
    state: PunchState,
    local_public: Option<SocketAddr>,
    peer_public: Option<SocketAddr>,
}

impl Default for HolePunch {
    fn default() -> Self {
        Self::new()
    }
}

impl HolePunch {
    /// A fresh coordination in [`PunchState::Discovering`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: PunchState::Discovering,
            local_public: None,
            peer_public: None,
        }
    }

    /// The current phase.
    #[must_use]
    pub const fn state(&self) -> PunchState {
        self.state
    }

    /// This peer's own public reflexive address, once STUN has discovered it.
    #[must_use]
    pub const fn local_public(&self) -> Option<SocketAddr> {
        self.local_public
    }

    /// The remote peer's public address, once it has been exchanged.
    #[must_use]
    pub const fn peer_public(&self) -> Option<SocketAddr> {
        self.peer_public
    }

    /// Record this peer's own public address (the STUN discovery result). This
    /// is the address shared with the remote out of band. If the peer's address
    /// is already known, the state advances to [`PunchState::Punching`].
    pub const fn local_discovered(&mut self, addr: SocketAddr) {
        self.local_public = Some(addr);
        self.maybe_start_punching();
    }

    /// Record the remote peer's public address (received out of band). Once both
    /// addresses are known, the state advances to [`PunchState::Punching`] and
    /// the caller should begin sending punch packets at `addr`.
    pub const fn peer_discovered(&mut self, addr: SocketAddr) {
        self.peer_public = Some(addr);
        self.maybe_start_punching();
    }

    /// Advance Discovering → Punching once both public addresses are known.
    const fn maybe_start_punching(&mut self) {
        if self.local_public.is_some()
            && self.peer_public.is_some()
            && matches!(self.state, PunchState::Discovering)
        {
            self.state = PunchState::Punching;
        }
    }

    /// Whether the caller should currently be sending punch packets (true only
    /// in [`PunchState::Punching`]).
    #[must_use]
    pub const fn should_punch(&self) -> bool {
        matches!(self.state, PunchState::Punching)
    }

    /// Feed a packet received from `from`. If we are punching and `from` matches
    /// the peer's known public address, the mapping is confirmed open and the
    /// state advances to [`PunchState::Connected`], returning `true`. Any other
    /// source (a stray) is ignored, returning `false` — it cannot hijack the
    /// punch.
    pub fn punch_received(&mut self, from: SocketAddr) -> bool {
        if matches!(self.state, PunchState::Punching) && self.peer_public == Some(from) {
            self.state = PunchState::Connected;
            true
        } else {
            false
        }
    }

    /// `true` once the bidirectional mapping is open.
    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self.state, PunchState::Connected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn binding_request_has_correct_header() {
        let mut rng = SplitMix64::new(0x1234_5678);
        let (req, tx) = build_binding_request(&mut rng);
        assert_eq!(req.len(), HEADER_LEN);
        // Message type = Binding Request (0x0001), big-endian.
        assert_eq!(u16::from_be_bytes([req[0], req[1]]), MSG_BINDING_REQUEST);
        // Message length = 0 (no attributes).
        assert_eq!(u16::from_be_bytes([req[2], req[3]]), 0);
        // Magic cookie.
        assert_eq!(
            u32::from_be_bytes([req[4], req[5], req[6], req[7]]),
            MAGIC_COOKIE
        );
        // Transaction id occupies bytes 8..20 and is echoed back to the caller.
        assert_eq!(&req[8..20], &tx);
    }

    #[test]
    fn transaction_id_is_fresh_per_request() {
        let mut rng = SplitMix64::new(0xABCD);
        let (_r1, tx1) = build_binding_request(&mut rng);
        let (_r2, tx2) = build_binding_request(&mut rng);
        assert_ne!(tx1, tx2, "each request must use a fresh transaction id");
    }

    /// Build a synthetic Binding Success Response carrying an XOR-MAPPED-ADDRESS
    /// for `addr`, with the given transaction id.
    /// Wrap a TLV attribute + header around an attribute value, producing a full
    /// Binding Success Response.
    fn wrap_success(attr_type: u16, value: &[u8], tx: &TransactionId) -> Vec<u8> {
        let mut attr = Vec::new();
        attr.extend_from_slice(&attr_type.to_be_bytes());
        attr.extend_from_slice(&u16::try_from(value.len()).unwrap().to_be_bytes());
        attr.extend_from_slice(value);
        let mut msg = Vec::new();
        msg.extend_from_slice(&MSG_BINDING_SUCCESS.to_be_bytes());
        msg.extend_from_slice(&u16::try_from(attr.len()).unwrap().to_be_bytes());
        msg.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        msg.extend_from_slice(tx);
        msg.extend_from_slice(&attr);
        msg
    }

    fn synth_xor_mapped_v4(addr: SocketAddrV4, tx: &TransactionId) -> Vec<u8> {
        let cookie_be = MAGIC_COOKIE.to_be_bytes();
        let cookie_hi16 = u16::try_from(MAGIC_COOKIE >> 16).unwrap();
        let x_port = addr.port() ^ cookie_hi16;
        let mut x_addr = addr.ip().octets();
        for (b, k) in x_addr.iter_mut().zip(cookie_be.iter()) {
            *b ^= *k;
        }
        // Attribute value: reserved, family, x-port, x-address (8 bytes).
        let mut value = vec![0u8, FAMILY_IPV4];
        value.extend_from_slice(&x_port.to_be_bytes());
        value.extend_from_slice(&x_addr);
        wrap_success(ATTR_XOR_MAPPED_ADDRESS, &value, tx)
    }

    #[test]
    fn xor_mapped_address_decodes_to_known_socketaddr() {
        let tx = [0xAAu8; 12];
        let want = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 51_234);
        let resp = synth_xor_mapped_v4(want, &tx);
        let got = parse_binding_response(&resp, Some(&tx)).expect("decode");
        assert_eq!(got, SocketAddr::V4(want));
    }

    #[test]
    fn xor_mapped_address_ipv6_decodes() {
        // Hand-build an IPv6 XOR-MAPPED-ADDRESS and decode it back.
        let tx = [0x5Au8; 12];
        let ip = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x42);
        let port = 40_000u16;
        let cookie_be = MAGIC_COOKIE.to_be_bytes();
        let mut key = [0u8; 16];
        key[..4].copy_from_slice(&cookie_be);
        key[4..].copy_from_slice(&tx);
        let mut x_addr = ip.octets();
        for (b, k) in x_addr.iter_mut().zip(key.iter()) {
            *b ^= *k;
        }
        let x_port = port ^ u16::try_from(MAGIC_COOKIE >> 16).unwrap();
        let mut value = vec![0u8, FAMILY_IPV6];
        value.extend_from_slice(&x_port.to_be_bytes());
        value.extend_from_slice(&x_addr);
        let msg = wrap_success(ATTR_XOR_MAPPED_ADDRESS, &value, &tx);
        let got = parse_binding_response(&msg, Some(&tx)).expect("ipv6 decode");
        assert_eq!(got, SocketAddr::V6(SocketAddrV6::new(ip, port, 0, 0)));
    }

    #[test]
    fn mapped_address_fallback_decodes() {
        // Only a (plain) MAPPED-ADDRESS present: must still decode.
        let tx = [0x11u8; 12];
        let ip = Ipv4Addr::new(198, 51, 100, 9);
        let port = 12_345u16;
        let mut value = vec![0u8, FAMILY_IPV4];
        value.extend_from_slice(&port.to_be_bytes());
        value.extend_from_slice(&ip.octets());
        let msg = wrap_success(ATTR_MAPPED_ADDRESS, &value, &tx);
        let got = parse_binding_response(&msg, Some(&tx)).expect("mapped decode");
        assert_eq!(got, SocketAddr::V4(SocketAddrV4::new(ip, port)));
    }

    #[test]
    fn short_response_is_rejected() {
        assert!(parse_binding_response(&[], None).is_none());
        assert!(parse_binding_response(&[0u8; 10], None).is_none());
    }

    #[test]
    fn wrong_magic_cookie_is_rejected() {
        let tx = [0u8; 12];
        let mut resp = synth_xor_mapped_v4(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 5), &tx);
        // Corrupt the magic cookie (bytes 4..8).
        resp[4] ^= 0xFF;
        assert!(parse_binding_response(&resp, Some(&tx)).is_none());
    }

    #[test]
    fn wrong_transaction_id_is_rejected() {
        let tx = [0x01u8; 12];
        let resp = synth_xor_mapped_v4(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 5), &tx);
        let other = [0x02u8; 12];
        assert!(
            parse_binding_response(&resp, Some(&other)).is_none(),
            "a response with a non-matching transaction id is a stray; reject it"
        );
        // But with no expected id (or the right one) it decodes.
        assert!(parse_binding_response(&resp, None).is_some());
        assert!(parse_binding_response(&resp, Some(&tx)).is_some());
    }

    #[test]
    fn overrun_attribute_length_is_rejected() {
        let tx = [0u8; 12];
        let mut resp = synth_xor_mapped_v4(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 5), &tx);
        // Inflate the declared message length far beyond the buffer.
        resp[2] = 0xFF;
        resp[3] = 0xFF;
        assert!(parse_binding_response(&resp, Some(&tx)).is_none());
    }

    #[test]
    fn non_success_message_type_is_rejected() {
        let tx = [0u8; 12];
        let mut resp = synth_xor_mapped_v4(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 5), &tx);
        // Flip the message type to a request, not a success response.
        resp[0..2].copy_from_slice(&MSG_BINDING_REQUEST.to_be_bytes());
        assert!(parse_binding_response(&resp, Some(&tx)).is_none());
    }

    #[test]
    fn hole_punch_transitions_discovering_punching_connected() {
        let local = SocketAddr::from((Ipv4Addr::new(203, 0, 113, 1), 5000));
        let peer = SocketAddr::from((Ipv4Addr::new(198, 51, 100, 2), 6000));
        let mut hp = HolePunch::new();
        assert_eq!(hp.state(), PunchState::Discovering);
        assert!(!hp.should_punch());

        // Learning only the peer address (no local yet) stays Discovering.
        hp.peer_discovered(peer);
        assert_eq!(hp.state(), PunchState::Discovering);

        // Once the local public address is known too, we move to Punching
        // (peer-then-local ordering).
        hp.local_discovered(local);
        assert_eq!(hp.state(), PunchState::Punching);
        assert!(hp.should_punch());

        // The other ordering (local-then-peer) advances identically.
        let mut hp2 = HolePunch::new();
        hp2.local_discovered(local);
        assert_eq!(hp2.state(), PunchState::Discovering);
        hp2.peer_discovered(peer);
        assert_eq!(hp2.state(), PunchState::Punching);
        assert!(hp2.should_punch());

        // A punch from a stray source does not connect.
        let stray = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 9), 9999));
        assert!(!hp2.punch_received(stray));
        assert_eq!(hp2.state(), PunchState::Punching);

        // A punch from the real peer connects.
        assert!(hp2.punch_received(peer));
        assert_eq!(hp2.state(), PunchState::Connected);
        assert!(hp2.is_connected());
        assert!(!hp2.should_punch());
    }
}
