//! v3.4.0 (`T-NETPLAY-DATAGRAM-AUTH`, protocol 9, ADR 0046): a per-session
//! secret on every netplay datagram.
//!
//! # Why
//!
//! Until v3.4.0 any host that could reach a peer's UDP port could put a
//! datagram into its session: one forged `Checksum` ended the match as a
//! desync, a forged `Sync` as a ROM mismatch, a forged `Input` corrupted a
//! prediction (Round 2 NET-R2-07). The v3.2.0 import tried source-address
//! filters; they were reverted because they ended a session on a NAT
//! rebinding (the peer's public port changes mid-session) and authenticated
//! nothing, a UDP source being trivially spoofed.
//!
//! # What
//!
//! Each peer draws a random 16-byte **key share** ([`fresh_key_share`],
//! `getrandom`) and sends it in its handshake `Sync`
//! ([`crate::NetMessage::Sync::key_share`]). Once the handshake completes, both
//! sides derive the same session secret from every share and the
//! [`SessionIdentity`] (so the key also binds the game, the configuration and
//! the emulation epoch), and from it one HMAC-SHA-256 key per sender. From then
//! on every datagram is **sealed**:
//!
//! ```text
//! 0xA9 | sender (u8) | sequence (u64 LE) | payload (a NetMessage) | tag (16 bytes)
//! ```
//!
//! `tag` is HMAC-SHA-256 under the sender's key over everything before it,
//! truncated to 128 bits. [`DatagramAuth::open`] checks, in order and before
//! the payload is parsed at all: the length, the marker, that the sender is a
//! known peer and not this one, that the sequence is inside the replay window
//! and unseen, and then the tag (a constant-time comparison, inside the `hmac`
//! crate). Only then is the sequence marked seen and the payload handed to
//! [`crate::NetMessage::from_bytes`].
//!
//! The source address plays no part in the decision, which is the point: a
//! peer whose NAT rebinds keeps its key, so its datagrams from the new address
//! still open, and the transports then follow the new address (as `WireGuard`
//! does); a third party that can reach the port but does not hold the key
//! cannot produce a datagram that opens, and cannot replay one, from any
//! address.
//!
//! # What it does not do
//!
//! The shares cross the network in the clear, so a party that **observes the
//! handshake** (on the path, or on the same broadcast LAN segment) can derive
//! the secret. This defends against off-path forgery, the threat NET-R2-07
//! names, not against an on-path attacker; that needs a key exchange (X25519)
//! and a way to authenticate it, both recorded as follow-ups in ADR 0046.
//! Nothing is encrypted: the payload is readable, it is only made
//! unforgeable.
//!
//! # Crypto
//!
//! No primitive is hand-rolled: the MAC is the `RustCrypto` `hmac` crate over
//! `sha2`'s SHA-256 (the `sha2` the core already uses for the ROM and
//! configuration hashes), the shares come from `getrandom`, and the derivation
//! is HMAC used as a PRF in the HKDF extract-then-expand shape (RFC 5869: one
//! extract, then one expand block per sender).

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::message::SessionIdentity;

pub use crate::message::KEY_SHARE_LEN;

/// One peer's handshake contribution to the session secret.
pub type KeyShare = [u8; KEY_SHARE_LEN];

/// The first byte of every sealed datagram. No [`crate::NetMessage`] tag uses
/// it, so an unsealed datagram is never mistaken for a sealed one.
pub const SEALED_MARKER: u8 = 0xA9;

/// Marker, sender, sequence.
const HEADER_LEN: usize = 1 + 1 + 8;

/// The truncated HMAC-SHA-256 tag (128 bits, the RFC 2104 section 5
/// recommendation of at least half the output).
const TAG_LEN: usize = 16;

/// Bytes sealing adds to a payload.
pub const SEAL_OVERHEAD: usize = HEADER_LEN + TAG_LEN;

/// How far behind the newest sequence seen from a sender a datagram may be
/// and still be accepted (once).
///
/// UDP reorders; a session sends a handful of
/// datagrams per frame to each peer, so 1024 covers well over a second of
/// reordering while the bitmap stays 128 bytes per sender.
pub const REPLAY_WINDOW: u64 = 1024;

/// The most senders a session has: the Four Score's four players.
const MAX_SENDERS: usize = 4;

/// Domain-separation label for the extract step. A different protocol version
/// must change it, so a key never means the same thing under two layouts.
const EXTRACT_LABEL: &[u8] = b"RustyNES netplay datagram auth, protocol 9";

/// Why [`DatagramAuth::open`] refused a datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// It does not start with [`SEALED_MARKER`]: an unsealed datagram (a
    /// handshake straggler, an older peer, or a forgery).
    Unsealed,
    /// Too short to hold a header, a payload and a tag.
    Malformed,
    /// The sender byte names no peer of this session, or this peer itself.
    UnknownSender,
    /// The tag does not verify: not sealed with the sender's key.
    BadTag,
    /// The sequence was already accepted, or is older than the replay window.
    Replayed,
}

/// Draw a fresh key share from the operating system's CSPRNG.
///
/// A share of all zeros is redrawn: zero is what a `Sync` sent inside a
/// running session (or as a NAT punch packet) carries, and the handshake
/// ignores such a `Sync` as a key source.
///
/// # Panics
///
/// Panics if the operating system has no randomness source at all, which
/// `getrandom` reports only on platforms this crate does not support.
#[must_use]
pub fn fresh_key_share() -> KeyShare {
    loop {
        let mut share = [0u8; KEY_SHARE_LEN];
        getrandom::fill(&mut share).expect("the OS provides randomness");
        if share != [0; KEY_SHARE_LEN] {
            return share;
        }
    }
}

/// Seen-sequence tracking for one sender: the newest sequence plus a ring of
/// [`REPLAY_WINDOW`] bits, bit `s % REPLAY_WINDOW` meaning "sequence `s`
/// accepted" for `s` within the window below the newest (the `IPsec` / DTLS
/// sliding window, RFC 4303 section 3.4.3, as a ring rather than a shift).
#[derive(Clone, Debug)]
struct ReplayWindow {
    newest: Option<u64>,
    seen: [u64; (REPLAY_WINDOW / 64) as usize],
}

impl ReplayWindow {
    const fn new() -> Self {
        Self {
            newest: None,
            seen: [0; (REPLAY_WINDOW / 64) as usize],
        }
    }

    const fn bit(seq: u64) -> (usize, u64) {
        let i = seq % REPLAY_WINDOW;
        ((i / 64) as usize, 1u64 << (i % 64))
    }

    /// `true` if `seq` could be accepted (not marked, not too old).
    const fn fresh(&self, seq: u64) -> bool {
        match self.newest {
            None => true,
            Some(n) if seq > n => true,
            Some(n) if n - seq >= REPLAY_WINDOW => false,
            Some(_) => {
                let (w, b) = Self::bit(seq);
                self.seen[w] & b == 0
            }
        }
    }

    /// Mark `seq` accepted. Call only after `fresh(seq)` and a verified tag.
    fn mark(&mut self, seq: u64) {
        match self.newest {
            Some(n) if seq <= n => {}
            Some(n) if seq - n < REPLAY_WINDOW => {
                // Clear the bits of the sequences skipped over, which now
                // stand for numbers a whole window later than before.
                for s in n + 1..=seq {
                    let (w, b) = Self::bit(s);
                    self.seen[w] &= !b;
                }
                self.newest = Some(seq);
            }
            _ => {
                self.seen = [0; (REPLAY_WINDOW / 64) as usize];
                self.newest = Some(seq);
            }
        }
        let (w, b) = Self::bit(seq);
        self.seen[w] |= b;
    }
}

/// The sealing state of one peer's end of an authenticated session: its own
/// sender index and send counter, every sender's key, and a replay window per
/// remote sender.
#[derive(Clone)]
pub struct DatagramAuth {
    local: u8,
    keys: [Option<Hmac<Sha256>>; MAX_SENDERS],
    next_seq: u64,
    windows: [ReplayWindow; MAX_SENDERS],
}

impl std::fmt::Debug for DatagramAuth {
    // Never prints a key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatagramAuth")
            .field("local", &self.local)
            .field("senders", &self.remote_senders().collect::<Vec<_>>())
            .field("next_seq", &self.next_seq)
            .finish_non_exhaustive()
    }
}

impl DatagramAuth {
    /// The two-peer form, for the 2-player handshake
    /// ([`crate::NetplayConnection`] and the joiner's fallback in
    /// [`crate::MeshJoiner`]): sender `0` is the peer whose share sorts
    /// first, so both ends agree without either knowing which one hosted.
    /// `None` if the shares are equal (a chance of 2^-128, or a peer echoing
    /// ours back) or either is all zeros.
    #[must_use]
    pub fn pair(identity: &SessionIdentity, ours: KeyShare, theirs: KeyShare) -> Option<Self> {
        if ours == theirs || ours == [0; KEY_SHARE_LEN] || theirs == [0; KEY_SHARE_LEN] {
            return None;
        }
        let (local, shares) = if ours < theirs {
            (0, [(0, ours), (1, theirs)])
        } else {
            (1, [(0, theirs), (1, ours)])
        };
        Self::mesh(identity, &shares, local)
    }

    /// The N-peer form, for the mesh roster handshake: `shares` lists every
    /// peer's `(player index, key share)` and the sender index of each peer is
    /// its player index. `None` if `local` is not listed, an index repeats or
    /// exceeds the Four Score's four, a share is all zeros, or fewer than two
    /// peers are listed.
    #[must_use]
    pub fn mesh(identity: &SessionIdentity, shares: &[(u8, KeyShare)], local: u8) -> Option<Self> {
        let mut ordered: [Option<KeyShare>; MAX_SENDERS] = [None; MAX_SENDERS];
        for &(player, share) in shares {
            let slot = ordered.get_mut(usize::from(player))?;
            if slot.is_some() || share == [0; KEY_SHARE_LEN] {
                return None;
            }
            *slot = Some(share);
        }
        if ordered.get(usize::from(local))?.is_none() || shares.len() < 2 {
            return None;
        }
        // Extract: one pseudorandom key from every share, in player order,
        // and the identity both peers agreed on in the handshake.
        let mut extract = <Hmac<Sha256> as KeyInit>::new_from_slice(EXTRACT_LABEL).ok()?;
        extract.update(&identity.epoch.to_le_bytes());
        extract.update(&identity.rom_hash);
        extract.update(&identity.config_hash);
        for (player, share) in ordered.iter().enumerate() {
            if let Some(share) = share {
                extract.update(&[u8::try_from(player).ok()?]);
                extract.update(share);
            }
        }
        let prk = extract.finalize().into_bytes();
        // Expand: one key per sender, labelled with its index.
        let mut keys: [Option<Hmac<Sha256>>; MAX_SENDERS] = [None, None, None, None];
        for (player, share) in ordered.iter().enumerate() {
            if share.is_some() {
                let mut expand = <Hmac<Sha256> as KeyInit>::new_from_slice(&prk).ok()?;
                expand.update(b"sender key");
                expand.update(&[u8::try_from(player).ok()?, 0x01]);
                let key = expand.finalize().into_bytes();
                keys[player] = Some(<Hmac<Sha256> as KeyInit>::new_from_slice(&key).ok()?);
            }
        }
        Some(Self {
            local,
            keys,
            next_seq: 0,
            windows: [
                ReplayWindow::new(),
                ReplayWindow::new(),
                ReplayWindow::new(),
                ReplayWindow::new(),
            ],
        })
    }

    /// This peer's sender index.
    #[must_use]
    pub const fn local_sender(&self) -> u8 {
        self.local
    }

    /// Every remote sender index this session accepts.
    pub fn remote_senders(&self) -> impl Iterator<Item = u8> + '_ {
        (0u8..)
            .zip(self.keys.iter())
            .filter(move |&(s, k)| k.is_some() && s != self.local)
            .map(|(s, _)| s)
    }

    /// Seal `payload` (an encoded [`crate::NetMessage`]) under this peer's
    /// key and the next sequence number.
    #[must_use]
    pub fn seal(&mut self, payload: &[u8]) -> Vec<u8> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut out = Vec::with_capacity(payload.len() + SEAL_OVERHEAD);
        out.push(SEALED_MARKER);
        out.push(self.local);
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(payload);
        if let Some(key) = self.keys[usize::from(self.local)].as_ref() {
            let mut mac = key.clone();
            mac.update(&out);
            let tag = mac.finalize().into_bytes();
            out.extend_from_slice(&tag[..TAG_LEN]);
        }
        out
    }

    /// Verify a received datagram and, if it is genuine and new, mark its
    /// sequence seen and return `(sender, payload)`. Nothing past the header
    /// is interpreted before the tag verifies.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] that stopped it. A refused datagram changes no state.
    pub fn open<'a>(&mut self, datagram: &'a [u8]) -> Result<(u8, &'a [u8]), Refusal> {
        if datagram.first() != Some(&SEALED_MARKER) {
            return Err(Refusal::Unsealed);
        }
        // At least a header, a one-byte payload (a message tag) and a tag.
        if datagram.len() < SEAL_OVERHEAD + 1 {
            return Err(Refusal::Malformed);
        }
        let sender = datagram[1];
        if sender == self.local {
            return Err(Refusal::UnknownSender);
        }
        let Some(key) = self.keys.get(usize::from(sender)).and_then(Option::as_ref) else {
            return Err(Refusal::UnknownSender);
        };
        let mut seq_bytes = [0u8; 8];
        seq_bytes.copy_from_slice(&datagram[2..HEADER_LEN]);
        let seq = u64::from_le_bytes(seq_bytes);
        let window = &mut self.windows[usize::from(sender)];
        if !window.fresh(seq) {
            return Err(Refusal::Replayed);
        }
        let (signed, tag) = datagram.split_at(datagram.len() - TAG_LEN);
        let mut mac = key.clone();
        mac.update(signed);
        mac.verify_truncated_left(tag)
            .map_err(|_| Refusal::BadTag)?;
        window.mark(seq);
        Ok((sender, &signed[HEADER_LEN..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> SessionIdentity {
        SessionIdentity::new([3; 32], [4; 32])
    }

    fn pair() -> (DatagramAuth, DatagramAuth) {
        let (a, b) = ([1u8; 16], [2u8; 16]);
        (
            DatagramAuth::pair(&id(), a, b).unwrap(),
            DatagramAuth::pair(&id(), b, a).unwrap(),
        )
    }

    #[test]
    fn both_ends_of_a_pair_agree_and_a_datagram_opens_once() {
        let (mut a, mut b) = pair();
        assert_ne!(a.local_sender(), b.local_sender());
        let sealed = a.seal(b"\x01hello");
        assert_eq!(b.open(&sealed), Ok((a.local_sender(), &b"\x01hello"[..])));
        assert_eq!(
            b.open(&sealed),
            Err(Refusal::Replayed),
            "a replay is refused"
        );
        // A's own datagram reflected back to A is not accepted either.
        assert_eq!(a.open(&sealed), Err(Refusal::UnknownSender));
    }

    #[test]
    fn a_tampered_or_foreign_datagram_is_refused() {
        let (mut a, mut b) = pair();
        let sealed = a.seal(b"\x03payload");
        for i in 2..sealed.len() {
            let mut t = sealed.clone();
            t[i] ^= 0x01;
            assert!(b.open(&t).is_err(), "a flip at byte {i} must not open");
        }
        // Another session's key (another share) does not open it.
        let mut other = DatagramAuth::pair(&id(), [9; 16], [1; 16]).unwrap();
        assert_eq!(other.open(&sealed), Err(Refusal::BadTag));
        // Another identity (game, settings, epoch) does not either.
        let mut other_id = SessionIdentity::new([3; 32], [5; 32]);
        other_id.epoch = id().epoch;
        let mut c = DatagramAuth::pair(&other_id, [2; 16], [1; 16]).unwrap();
        assert_eq!(c.open(&sealed), Err(Refusal::BadTag));
        assert_eq!(b.open(b"\x03unsealed"), Err(Refusal::Unsealed));
        assert_eq!(b.open(&sealed[..SEAL_OVERHEAD]), Err(Refusal::Malformed));
        // The genuine datagram still opens after all the refusals: a refusal
        // changes no state.
        assert!(b.open(&sealed).is_ok());
    }

    #[test]
    fn the_replay_window_accepts_reordering_and_refuses_the_old() {
        let (mut a, mut b) = pair();
        let d: Vec<Vec<u8>> = (0..1100).map(|_| a.seal(b"\x01x")).collect();
        // Out of order inside the window: each once.
        assert!(b.open(&d[5]).is_ok());
        assert!(b.open(&d[2]).is_ok());
        assert_eq!(b.open(&d[2]), Err(Refusal::Replayed));
        assert!(b.open(&d[1099]).is_ok());
        // 1099 - 5 = 1094 >= the window: too old now, never accepted.
        assert_eq!(b.open(&d[5]), Err(Refusal::Replayed));
        assert_eq!(b.open(&d[10]), Err(Refusal::Replayed));
        // Inside the window below the newest and unseen: accepted once.
        assert!(b.open(&d[1000]).is_ok());
        assert_eq!(b.open(&d[1000]), Err(Refusal::Replayed));
    }

    #[test]
    fn mesh_keys_are_per_sender() {
        let shares = [(0u8, [1u8; 16]), (1, [2; 16]), (2, [3; 16]), (3, [4; 16])];
        let mut peers: Vec<DatagramAuth> = (0..4)
            .map(|p| DatagramAuth::mesh(&id(), &shares, p).unwrap())
            .collect();
        let from2 = peers[2].seal(b"\x01m");
        for p in [0usize, 1, 3] {
            assert_eq!(peers[p].open(&from2), Ok((2, &b"\x01m"[..])));
        }
        // Re-labelled as sender 1, the same bytes do not verify under 1's key.
        let mut relabel = from2;
        relabel[1] = 1;
        assert_eq!(peers[0].open(&relabel), Err(Refusal::BadTag));
        assert!(DatagramAuth::mesh(&id(), &shares[..1], 0).is_none());
        assert!(DatagramAuth::mesh(&id(), &[(0, [1; 16]), (0, [2; 16])], 0).is_none());
        assert!(DatagramAuth::mesh(&id(), &[(0, [1; 16]), (5, [2; 16])], 0).is_none());
    }

    #[test]
    fn shares_are_random_and_never_zero() {
        let a = fresh_key_share();
        let b = fresh_key_share();
        assert_ne!(a, [0; KEY_SHARE_LEN]);
        assert_ne!(a, b);
        assert!(DatagramAuth::pair(&id(), a, a).is_none());
        assert!(DatagramAuth::pair(&id(), a, [0; KEY_SHARE_LEN]).is_none());
    }
}
