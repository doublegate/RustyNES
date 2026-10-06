//! Wire messages exchanged between two netplay peers.
//!
//! [`NetMessage`] is the network-agnostic protocol unit. A [`Transport`]
//! (see [`crate::transport`]) is responsible only for moving these between
//! peers; it never interprets them. Stage 2's UDP transport will serialize
//! these to bytes (a `to_bytes`/`from_bytes` pair is provided here so the
//! UDP layer has a canonical, versioned encoding to plug into) and the
//! session logic stays unchanged.
//!
//! [`Transport`]: crate::transport::Transport

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use rustynes_core::Nes;

/// v2.9.8 — what two peers must share to run one timeline: the ROM and the
/// machine configuration.
///
/// Until v2.9.8 the handshake compared only the ROM hash, so two peers on the
/// same game with different emulation options -- one on the Famicom model,
/// one with a seeded power-on RAM, one with a Game Genie code -- connected and
/// then desynced at the first frame the difference reached. Since v2.9.8
/// [`Nes::rom_sha256`] also excludes the 16-byte header, so two differently
/// corrected headers (another region, mapper or mirroring) shared a ROM hash
/// too. `config_hash` covers both: it is [`rustynes_core::config_digest`],
/// SHA-256 over the region, the parsed cartridge board and every
/// [`rustynes_core::HardwareOptions`] field.
///
/// The two hashes are kept apart, rather than folded into one, so a refusal
/// can say which kind of difference it is: another game, or the same game set
/// up differently.
///
/// # Refuse, do not adopt
///
/// Peers whose `config_hash` differs refuse to connect
/// ([`IdentityMismatch::Config`]); the guest does not adopt the host's
/// options. Adoption would mean the session silently rewriting the guest's
/// console model, RAM fill or cheats -- the same silent change this check
/// exists to stop, made in the other direction -- and several options act
/// only at power-on, so adopting them would also need a coordinated
/// re-power-on of the guest after the handshake. A refusal that tells both
/// players to match their settings is explicit and costs one retry.
///
/// # The emulator itself (v3.0.0, ADR 0045)
///
/// Two peers can agree on the game and every option and still emulate it
/// differently, when an accuracy fix lies between their versions. `epoch` is
/// [`rustynes_core::EMULATION_EPOCH`], checked before the hashes, and a
/// difference is refused as [`IdentityMismatch::Emulator`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionIdentity {
    /// v3.0.0 — [`rustynes_core::EMULATION_EPOCH`]: which emulator behaviour.
    pub epoch: u32,
    /// [`Nes::rom_sha256`] — which game.
    pub rom_hash: [u8; 32],
    /// [`rustynes_core::config_digest`] — which machine runs it.
    pub config_hash: [u8; 32],
}

/// What two peers' [`SessionIdentity`] values disagree on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityMismatch {
    /// v3.0.0 (ADR 0045) — a version of `RustyNES` that emulates differently.
    /// `theirs` is the peer's emulation epoch, or `None` when the peer
    /// predates the epoch (it announced protocol 4 or 5, v2.9.9 or earlier).
    Emulator {
        /// This build's [`rustynes_core::EMULATION_EPOCH`].
        ours: u32,
        /// The peer's epoch, if it sent one.
        theirs: Option<u32>,
    },
    /// Different games.
    Rom,
    /// The same game on a differently configured machine (emulation options,
    /// region or cartridge header).
    Config,
}

/// v3.0.0 (ADR 0045) — the one wording for an
/// [`IdentityMismatch::Emulator`] refusal, shared by every error type and
/// frontend so the players read the same sentence everywhere.
#[must_use]
pub fn emulator_mismatch_text(ours: u32, theirs: Option<u32>) -> String {
    theirs.map_or_else(
        || {
            String::from(
                "the peer runs an older version of RustyNES (v2.9.9 or earlier) that \
                 emulates differently; both players need the same version",
            )
        },
        |theirs| {
            format!(
                "the peer runs a version of RustyNES that emulates differently (emulation \
                 epoch {theirs}; this one is {ours}); both players need versions with the \
                 same epoch"
            )
        },
    )
}

/// v3.0.0 — what a received [`NetMessage::Sync`] means for the handshake
/// ([`SessionIdentity::check_sync`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncVerdict {
    /// Not `RustyNES`'s handshake (an unknown magic, a stray or corrupt
    /// datagram): ignore it, as before.
    Ignore,
    /// Our protocol, and the identities agree.
    Accept,
    /// Refuse the peer, for this reason.
    Refuse(IdentityMismatch),
}

impl SessionIdentity {
    /// Build from explicit hashes (tests, or a host that computed them
    /// elsewhere).
    /// The epoch is this build's [`rustynes_core::EMULATION_EPOCH`]; set the
    /// public field to model another build.
    #[must_use]
    pub const fn new(rom_hash: [u8; 32], config_hash: [u8; 32]) -> Self {
        Self {
            epoch: rustynes_core::EMULATION_EPOCH,
            rom_hash,
            config_hash,
        }
    }

    /// The identity of the machine `nes` is right now. Take it after the
    /// host's options are applied and before the session starts; a change
    /// made afterwards is not covered by the handshake (the periodic desync
    /// checksum is what catches it then).
    #[must_use]
    pub fn of(nes: &Nes) -> Self {
        Self {
            epoch: rustynes_core::EMULATION_EPOCH,
            rom_hash: *nes.rom_sha256(),
            config_hash: rustynes_core::config_digest(nes),
        }
    }

    /// Compare a peer's announced identity with ours. The emulator epoch is
    /// checked first, then the ROM: the more fundamental the difference, the
    /// more useful its message (no setting can reconcile two emulators).
    ///
    /// # Errors
    ///
    /// What differs.
    pub fn check(&self, peer: &Self) -> Result<(), IdentityMismatch> {
        if peer.epoch != self.epoch {
            Err(IdentityMismatch::Emulator {
                ours: self.epoch,
                theirs: Some(peer.epoch),
            })
        } else if peer.rom_hash != self.rom_hash {
            Err(IdentityMismatch::Rom)
        } else if peer.config_hash != self.config_hash {
            Err(IdentityMismatch::Config)
        } else {
            Ok(())
        }
    }

    /// v3.0.0 — the one decision every handshake site makes on a received
    /// `Sync`, kept here so the sites cannot disagree.
    ///
    /// Our magic: [`Self::check`] decides. One of `RustyNES`'s own older
    /// magics (protocol 4 `"RNES"`, protocol 5 `"RNE5"`): refused as another
    /// emulator version. Until v3.0.0 those were ignored like any foreign
    /// datagram, so a session across versions simply timed out with no
    /// reason. Anything else: ignored, as before.
    #[must_use]
    pub fn check_sync(&self, magic: u32, peer: &Self) -> SyncVerdict {
        if magic == NetMessage::SYNC_MAGIC {
            match self.check(peer) {
                Ok(()) => SyncVerdict::Accept,
                Err(why) => SyncVerdict::Refuse(why),
            }
        } else if NetMessage::OLDER_SYNC_MAGICS.contains(&magic) {
            SyncVerdict::Refuse(IdentityMismatch::Emulator {
                ours: self.epoch,
                theirs: None,
            })
        } else {
            SyncVerdict::Ignore
        }
    }
}

/// Protocol version. Bumped if the wire layout of [`NetMessage`] changes so
/// the [`NetMessage::SYNC_MAGIC`] handshake can reject mismatched peers.
///
/// `2` (v2.5.0): [`NetMessage::Input`] gained a `player: u8` field so a peer
/// can tag which player index an input belongs to — the N-player (up to 4)
/// rollback generalization. The wire layout of `Input` changed (one extra
/// byte), so peers running the v1 (2-player) layout would mis-parse it; the
/// version bump documents that.
///
/// `3` (v2.6.0): added [`NetMessage::Roster`] — the host distributes the full
/// peer roster (each peer's [`SocketAddr`] + player
/// index) to every joiner so an N-peer UDP session can form the fully-connected
/// mesh. The new variant carries a new tag byte; older peers' [`from_bytes`]
/// rejects the unknown tag cleanly (returns `None`), so a v2 peer simply drops
/// a v3 `Roster` rather than mis-parsing it.
///
/// `5` (v2.9.8): [`NetMessage::Sync`] carries a [`SessionIdentity`] -- the
/// ROM hash plus a 32-byte configuration hash -- instead of the ROM hash
/// alone, under a new magic (`"RNE5"`, [`NetMessage::SYNC_MAGIC`]). A v4 `Sync`
/// is 32 bytes shorter and decodes to `None`, and a v4 peer rejects v5's
/// magic, so neither side mistakes the other for a match; they never sync.
///
/// `6` (v3.0.0, ADR 0045): [`SessionIdentity`] gains the emulation epoch, so
/// a `Sync` is 4 bytes longer, under the magic `"RNE6"`. A v5 or v4 `Sync`
/// still decodes (each at exactly its own length, under its own magic) so the
/// handshake can refuse it *with a reason*, "another emulator version",
/// instead of ignoring it until the session times out
/// ([`SessionIdentity::check_sync`]). A v5 peer ignores our magic, as v4
/// ignored v5's, so on its side the session still times out.
///
/// [`from_bytes`]: NetMessage::from_bytes
pub const PROTOCOL_VERSION: u32 = 6;

/// Messages exchanged between two peers.
///
/// Hand-rolled byte encoding (see [`NetMessage::to_bytes`] /
/// [`NetMessage::from_bytes`]) — deliberately no `serde` dependency in
/// Stage 1, keeping the crate lean. Each variant is length-discriminated by
/// a leading tag byte. The encoding is little-endian and versioned via
/// [`PROTOCOL_VERSION`] (carried in [`NetMessage::Sync`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetMessage {
    /// A remote player's confirmed input for `frame`. To tolerate packet
    /// loss, the UDP transport (Stage 2) will batch a small sliding window
    /// of recent inputs per datagram; the session ingests each
    /// `(player, frame, input)` triple idempotently, so duplicates and
    /// reorderings are harmless. Here `input` is the raw `Buttons::bits()`
    /// value and `player` is the 0-based player/controller index the input
    /// belongs to (0..=3; the N-player generalization — a 2-player session
    /// only ever sends `player` 0 or 1).
    Input {
        /// The 0-based player index this input belongs to (0..=3). Lets a
        /// peer route a remote input to the correct controller port; in a
        /// mesh topology every peer's input carries its own player index.
        player: u8,
        /// The frame this input applies to.
        frame: u32,
        /// `Buttons::bits()` for that player on that frame.
        input: u8,
    },

    /// Acknowledges receipt of the remote's input up to `frame`. Lets the
    /// sender stop retransmitting already-confirmed inputs (Stage 2 uses
    /// this for the sliding-window retransmit cutoff).
    InputAck {
        /// Highest contiguous frame the sender has received.
        frame: u32,
    },

    /// Connection handshake: confirms protocol compatibility, an identical
    /// ROM and (v2.9.8) an identically configured machine. Sent at session
    /// start and until the peer replies.
    Sync {
        /// Magic constant — must equal [`NetMessage::SYNC_MAGIC`].
        magic: u32,
        /// The ROM and machine configuration both peers must be running.
        identity: SessionIdentity,
    },

    /// Periodic state checksum for desync detection. Both peers compute a
    /// hash of their emulator state at confirmed frames and exchange it; a
    /// mismatch on a confirmed frame is a fatal desync.
    Checksum {
        /// The confirmed frame this checksum covers.
        frame: u32,
        /// The combined gameplay digest (framebuffer hash XOR a cycle term) —
        /// the value compared for desync detection.
        hash: u64,
        /// The framebuffer-only hash (FNV-1a 64), carried alongside `hash` so a
        /// desync can report WHICH component diverged: equal `fb_hash` with
        /// unequal `hash` ⇒ a timing/cycle divergence (same picture, different
        /// cycle count); unequal `fb_hash` ⇒ a state divergence.
        fb_hash: u64,
    },

    /// Connection-quality / time-sync hint. `frame_advantage` is how many
    /// frames ahead the sender believes it is relative to the receiver;
    /// the receiver uses it to decide whether to stall a frame and keep the
    /// two peers inside the rollback window.
    Quality {
        /// Round-trip estimate in milliseconds (0 for the in-memory
        /// transport, which is instantaneous).
        ping_ms: u32,
        /// Sender's local frame minus its last-confirmed remote frame.
        frame_advantage: i32,
    },

    /// The full peer **roster** for an N-player UDP session (v2.6.0 / protocol
    /// 3). The host sends this to every joiner once all expected joiners have
    /// completed their handshake: it lists every peer's player index and
    /// `IP:port`, so each joiner can form the **fully-connected mesh** (open a
    /// directed link to every *other* peer and broadcast its input to all of
    /// them). The host's own entry is included (player 0) so a joiner learns
    /// the host's gameplay address too.
    ///
    /// Encoded as a count byte followed by that many `(player: u8, addr)`
    /// entries (each `addr` is a 1-byte IP-family tag + the address bytes + a
    /// little-endian `u16` port — see [`Self::to_bytes`]). Bounded to
    /// [`Self::MAX_ROSTER`] entries; a longer or malformed roster decodes to
    /// `None`.
    Roster {
        /// Every peer's `(player_index, socket_addr)`, host first. The host's
        /// gameplay address is the address a joiner should `send_to` for the
        /// host's player; each other entry is a fellow joiner.
        peers: Vec<(u8, SocketAddr)>,
    },
}

impl NetMessage {
    /// The expected value of [`NetMessage::Sync::magic`].
    ///
    /// `"RNE5"` since protocol 5 (v2.9.8); `"RNES"` before. A v4 decoder reads
    /// the magic and the 32-byte ROM hash and ignores the rest, so had v5 kept
    /// v4's magic, a v4 peer would accept v5's longer `Sync` as its own and
    /// consider the session synced while the v5 side waited for a reply it
    /// refuses. Every version compares the magic, so a changed one is
    /// rejected on both sides.
    pub const SYNC_MAGIC: u32 = 0x524E_4536; // "RNE6"

    /// `RustyNES`'s own earlier `Sync` magics: protocol 4 (`"RNES"`, v2.5.x to
    /// v2.9.7) and protocol 5 (`"RNE5"`, v2.9.8 and v2.9.9). Recognised so a
    /// peer on an older version is refused with a reason
    /// ([`SessionIdentity::check_sync`]).
    pub const OLDER_SYNC_MAGICS: [u32; 2] = [0x524E_4553, 0x524E_4535];

    // Tag bytes for the hand-rolled encoding.
    const TAG_INPUT: u8 = 0;
    const TAG_INPUT_ACK: u8 = 1;
    const TAG_SYNC: u8 = 2;
    const TAG_CHECKSUM: u8 = 3;
    const TAG_QUALITY: u8 = 4;
    const TAG_ROSTER: u8 = 5;

    // IP-family tags inside a `Roster` entry's address encoding.
    const IP_V4: u8 = 4;
    const IP_V6: u8 = 6;

    /// The largest peer roster a [`Self::Roster`] may carry. Four players is the
    /// Four Score ceiling; a count byte beyond this is rejected as malformed so
    /// a hostile datagram cannot make [`Self::from_bytes`] allocate unbounded.
    pub const MAX_ROSTER: usize = 4;

    /// Serialize to a canonical, versioned little-endian byte buffer.
    ///
    /// Provided so the Stage 2 UDP transport has a stable encoding without
    /// touching the session. The session itself moves `NetMessage` values
    /// directly (the in-memory transport never serializes), so this is not
    /// on any Stage 1 hot path.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(40);
        match *self {
            Self::Input {
                player,
                frame,
                input,
            } => {
                out.push(Self::TAG_INPUT);
                out.push(player);
                out.extend_from_slice(&frame.to_le_bytes());
                out.push(input);
            }
            Self::InputAck { frame } => {
                out.push(Self::TAG_INPUT_ACK);
                out.extend_from_slice(&frame.to_le_bytes());
            }
            Self::Sync { magic, identity } => {
                out.push(Self::TAG_SYNC);
                out.extend_from_slice(&magic.to_le_bytes());
                // v3.0.0 (protocol 6): the emulation epoch.
                out.extend_from_slice(&identity.epoch.to_le_bytes());
                out.extend_from_slice(&identity.rom_hash);
                out.extend_from_slice(&identity.config_hash);
            }
            Self::Checksum {
                frame,
                hash,
                fb_hash,
            } => {
                out.push(Self::TAG_CHECKSUM);
                out.extend_from_slice(&frame.to_le_bytes());
                out.extend_from_slice(&hash.to_le_bytes());
                out.extend_from_slice(&fb_hash.to_le_bytes());
            }
            Self::Quality {
                ping_ms,
                frame_advantage,
            } => {
                out.push(Self::TAG_QUALITY);
                out.extend_from_slice(&ping_ms.to_le_bytes());
                out.extend_from_slice(&frame_advantage.to_le_bytes());
            }
            Self::Roster { ref peers } => {
                out.push(Self::TAG_ROSTER);
                // A count byte (peers.len() <= MAX_ROSTER fits a u8 trivially),
                // then each `(player, addr)` entry.
                out.push(u8::try_from(peers.len().min(u8::MAX as usize)).unwrap_or(u8::MAX));
                for &(player, addr) in peers {
                    out.push(player);
                    Self::encode_addr(&mut out, addr);
                }
            }
        }
        out
    }

    /// Append one [`SocketAddr`] to `out`: a 1-byte family tag, the raw address
    /// bytes (4 for v4, 16 for v6), then the port as a little-endian `u16`.
    fn encode_addr(out: &mut Vec<u8>, addr: SocketAddr) {
        match addr.ip() {
            IpAddr::V4(v4) => {
                out.push(Self::IP_V4);
                out.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                out.push(Self::IP_V6);
                out.extend_from_slice(&v6.octets());
            }
        }
        out.extend_from_slice(&addr.port().to_le_bytes());
    }

    /// Decode one [`SocketAddr`] from the front of `buf`, returning it plus the
    /// remaining bytes. `None` on a truncated / unknown-family encoding.
    fn decode_addr(buf: &[u8]) -> Option<(SocketAddr, &[u8])> {
        let (&family, rest) = buf.split_first()?;
        match family {
            Self::IP_V4 => {
                let octets: [u8; 4] = rest.get(0..4)?.try_into().ok()?;
                let port = u16::from_le_bytes(rest.get(4..6)?.try_into().ok()?);
                let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port);
                Some((addr, &rest[6..]))
            }
            Self::IP_V6 => {
                let octets: [u8; 16] = rest.get(0..16)?.try_into().ok()?;
                let port = u16::from_le_bytes(rest.get(16..18)?.try_into().ok()?);
                let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::from(octets)), port);
                Some((addr, &rest[18..]))
            }
            _ => None,
        }
    }

    /// Parse a buffer produced by [`Self::to_bytes`]. Returns `None` on a
    /// malformed / truncated / unknown-tag buffer (the UDP transport drops
    /// such datagrams rather than panicking).
    #[must_use]
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        let (&tag, rest) = buf.split_first()?;
        match tag {
            Self::TAG_INPUT => {
                let player = *rest.first()?;
                let frame = u32::from_le_bytes(rest.get(1..5)?.try_into().ok()?);
                let input = *rest.get(5)?;
                Some(Self::Input {
                    player,
                    frame,
                    input,
                })
            }
            Self::TAG_INPUT_ACK => {
                let frame = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
                Some(Self::InputAck { frame })
            }
            Self::TAG_SYNC => {
                let magic = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
                match rest.len() {
                    // Protocol 6: magic + epoch + two hashes. Exactly that
                    // length: a longer payload is a later protocol's, not this
                    // one's with junk on the end.
                    72 => {
                        let epoch = u32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
                        let rom_hash: [u8; 32] = rest.get(8..40)?.try_into().ok()?;
                        let config_hash: [u8; 32] = rest.get(40..72)?.try_into().ok()?;
                        Some(Self::Sync {
                            magic,
                            identity: SessionIdentity {
                                epoch,
                                rom_hash,
                                config_hash,
                            },
                        })
                    }
                    // An older RustyNES's `Sync`, accepted only at exactly its
                    // own length AND under its own magic, so the handshake can
                    // refuse it with a reason (`check_sync`). Its hashes are
                    // never compared; the epoch field is meaningless (0).
                    68 if magic == Self::OLDER_SYNC_MAGICS[1] => Some(Self::Sync {
                        magic,
                        identity: SessionIdentity {
                            epoch: 0,
                            rom_hash: rest.get(4..36)?.try_into().ok()?,
                            config_hash: rest.get(36..68)?.try_into().ok()?,
                        },
                    }),
                    36 if magic == Self::OLDER_SYNC_MAGICS[0] => Some(Self::Sync {
                        magic,
                        identity: SessionIdentity {
                            epoch: 0,
                            rom_hash: rest.get(4..36)?.try_into().ok()?,
                            config_hash: [0; 32],
                        },
                    }),
                    _ => None,
                }
            }
            Self::TAG_CHECKSUM => {
                let frame = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
                let hash = u64::from_le_bytes(rest.get(4..12)?.try_into().ok()?);
                let fb_hash = u64::from_le_bytes(rest.get(12..20)?.try_into().ok()?);
                Some(Self::Checksum {
                    frame,
                    hash,
                    fb_hash,
                })
            }
            Self::TAG_QUALITY => {
                let ping_ms = u32::from_le_bytes(rest.get(0..4)?.try_into().ok()?);
                let frame_advantage = i32::from_le_bytes(rest.get(4..8)?.try_into().ok()?);
                Some(Self::Quality {
                    ping_ms,
                    frame_advantage,
                })
            }
            Self::TAG_ROSTER => {
                let (&count, mut cursor) = rest.split_first()?;
                let count = count as usize;
                // Reject an oversized count before allocating (hostile input).
                if count > Self::MAX_ROSTER {
                    return None;
                }
                let mut peers = Vec::with_capacity(count);
                for _ in 0..count {
                    let (&player, after_player) = cursor.split_first()?;
                    let (addr, after_addr) = Self::decode_addr(after_player)?;
                    peers.push((player, addr));
                    cursor = after_addr;
                }
                // Trailing bytes after a well-formed roster ⇒ malformed.
                if cursor.is_empty() {
                    Some(Self::Roster { peers })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// FNV-1a 64-bit hash.
///
/// Used for the [`NetMessage::Checksum`] state digest — cheap, allocation-
/// free, and good enough to catch desyncs (a divergent snapshot almost
/// certainly hashes differently). Not used for any security purpose.
#[must_use]
pub fn fnv1a64(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in data {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: &NetMessage) {
        let bytes = msg.to_bytes();
        let back = NetMessage::from_bytes(&bytes).expect("decode");
        assert_eq!(*msg, back);
    }

    #[test]
    fn all_variants_roundtrip() {
        roundtrip(&NetMessage::Input {
            player: 3,
            frame: 0x1234_5678,
            input: 0xAB,
        });
        roundtrip(&NetMessage::InputAck { frame: 99 });
        roundtrip(&NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: SessionIdentity::new([7u8; 32], [9u8; 32]),
        });
        roundtrip(&NetMessage::Checksum {
            frame: 42,
            hash: 0xDEAD_BEEF_CAFE_F00D,
            fb_hash: 0x0123_4567_89AB_CDEF,
        });
        roundtrip(&NetMessage::Quality {
            ping_ms: 33,
            frame_advantage: -4,
        });
    }

    /// v2.9.8 (`CodeRabbit` on the review slice #580) — a v4 peer cannot take a
    /// v5 `Sync` for its own. A v4 decoder reads the magic and the 32-byte ROM
    /// hash and ignores what follows, so with v4's magic it accepted v5's
    /// longer message and marked the session synced while the v5 side, which
    /// refuses v4's shorter reply, timed out. v5 changes the magic, which v4
    /// compares, and decodes only a payload of exactly its own length.
    #[test]
    fn a_v5_sync_is_not_a_v4_sync() {
        const V4_SYNC_MAGIC: u32 = 0x524E_4553; // "RNES"
        assert_ne!(NetMessage::SYNC_MAGIC, V4_SYNC_MAGIC);
        let mut bytes = NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: SessionIdentity::new([7u8; 32], [9u8; 32]),
        }
        .to_bytes();
        bytes.push(0);
        assert_eq!(
            NetMessage::from_bytes(&bytes),
            None,
            "trailing byte accepted"
        );
    }

    /// v3.0.0 (ADR 0045) — a protocol-6 `Sync` carries the emulation epoch
    /// (72-byte payload), and a peer on another epoch is refused as another
    /// emulator, checked before the ROM and the configuration.
    #[test]
    fn a_sync_carries_the_epoch_and_another_epoch_is_refused() {
        let ours = SessionIdentity::new([7u8; 32], [9u8; 32]);
        assert_eq!(ours.epoch, rustynes_core::EMULATION_EPOCH);
        let bytes = NetMessage::Sync {
            magic: NetMessage::SYNC_MAGIC,
            identity: ours,
        }
        .to_bytes();
        assert_eq!(bytes.len(), 1 + 72);
        assert_eq!(&bytes[5..9], &ours.epoch.to_le_bytes());

        let mut theirs = ours;
        theirs.epoch += 1;
        theirs.rom_hash = [1u8; 32]; // a ROM difference too: the epoch wins
        assert_eq!(
            ours.check_sync(NetMessage::SYNC_MAGIC, &theirs),
            SyncVerdict::Refuse(IdentityMismatch::Emulator {
                ours: ours.epoch,
                theirs: Some(theirs.epoch),
            })
        );
        assert_eq!(
            ours.check_sync(NetMessage::SYNC_MAGIC, &ours),
            SyncVerdict::Accept
        );
    }

    /// v3.0.0 — `RustyNES`'s own older `Sync` messages (protocol 5 `"RNE5"`,
    /// 68 bytes; protocol 4 `"RNES"`, 36 bytes) decode at exactly their own
    /// length under their own magic, and are refused as another emulator
    /// version with no epoch. Until v3.0.0 they were dropped as foreign, so a
    /// session across versions timed out with no reason. A foreign magic is
    /// still ignored, and an older length under the wrong magic is not
    /// decoded at all.
    #[test]
    fn an_older_rustynes_sync_is_refused_with_a_reason() {
        let ours = SessionIdentity::new([7u8; 32], [9u8; 32]);
        for (magic, payload) in [
            (NetMessage::OLDER_SYNC_MAGICS[1], 68usize), // protocol 5
            (NetMessage::OLDER_SYNC_MAGICS[0], 36usize), // protocol 4
        ] {
            let mut bytes = vec![NetMessage::TAG_SYNC];
            bytes.extend_from_slice(&magic.to_le_bytes());
            bytes.resize(1 + payload, 0x5A);
            let Some(NetMessage::Sync {
                magic: got,
                identity,
            }) = NetMessage::from_bytes(&bytes)
            else {
                panic!("an older RustyNES Sync of {payload} bytes must decode");
            };
            assert_eq!(got, magic);
            assert_eq!(
                ours.check_sync(got, &identity),
                SyncVerdict::Refuse(IdentityMismatch::Emulator {
                    ours: ours.epoch,
                    theirs: None,
                })
            );
            // The same length under a magic that is not that protocol's.
            bytes[1..5].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
            assert_eq!(NetMessage::from_bytes(&bytes), None);
        }
        assert_eq!(ours.check_sync(0xDEAD_BEEF, &ours), SyncVerdict::Ignore);
    }

    #[test]
    fn roster_roundtrips_v4_and_v6() {
        use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
        // A mixed v4/v6 roster of the full four players.
        roundtrip(&NetMessage::Roster {
            peers: vec![
                (
                    0,
                    SocketAddr::new(Ipv4Addr::new(192, 168, 1, 5).into(), 7000),
                ),
                (1, SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0xABCD)),
                (
                    2,
                    SocketAddr::new(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).into(), 65535),
                ),
                (3, SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 1)),
            ],
        });
        // An empty roster is well-formed too.
        roundtrip(&NetMessage::Roster { peers: vec![] });
    }

    #[test]
    fn roster_rejects_oversized_count() {
        // A count byte beyond MAX_ROSTER must be rejected before allocating.
        let buf = [NetMessage::TAG_ROSTER, 200];
        assert!(NetMessage::from_bytes(&buf).is_none());
    }

    #[test]
    fn roster_rejects_truncated_and_trailing() {
        use std::net::{Ipv4Addr, SocketAddr};
        // Claims one entry but provides no body.
        assert!(NetMessage::from_bytes(&[NetMessage::TAG_ROSTER, 1]).is_none());
        // A valid one-entry roster with one extra trailing byte ⇒ malformed.
        let mut buf = NetMessage::Roster {
            peers: vec![(0, SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 5))],
        }
        .to_bytes();
        buf.push(0xFF);
        assert!(NetMessage::from_bytes(&buf).is_none());
    }

    #[test]
    fn truncated_decodes_to_none() {
        assert!(NetMessage::from_bytes(&[]).is_none());
        assert!(NetMessage::from_bytes(&[NetMessage::TAG_SYNC, 1, 2]).is_none());
        assert!(NetMessage::from_bytes(&[250, 0, 0]).is_none());
    }

    #[test]
    fn fnv1a64_known_vector() {
        // FNV-1a of the empty string is the offset basis.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        // Distinct inputs hash differently.
        assert_ne!(fnv1a64(b"a"), fnv1a64(b"b"));
    }
}
