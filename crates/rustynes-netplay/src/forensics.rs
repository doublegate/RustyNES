//! Automatic desync forensics (v3.7.0, `T-NETPLAY-DESYNC-FORENSICS`).
//!
//! The session's periodic [`Checksum`](crate::NetMessage::Checksum) says THAT
//! two peers diverged, at a checksum boundary, and whether the picture or only
//! the cycle count differs. It cannot say where: the gameplay digest covers
//! the framebuffer and the cycle counter, nothing else, so a corrupted RAM
//! byte that has not reached the picture yet is invisible to it, and by the
//! time it is visible the first cause is many frames back.
//!
//! Forensics answers the question the checksum cannot. It is **opt-in on each
//! peer** ([`RollbackSession::enable_forensics`]), because it sends parts of
//! the machine state to the peer, and a peer that has not opted in never sends
//! any. With both peers opted in:
//!
//! 1. **Per-frame section digests.** At every confirmed frame `g` the session
//!    already holds a full core snapshot of the machine entering `g` (the
//!    rollback window entry it is about to drop, which every peer reaches
//!    byte for byte). Forensics hashes each `.rns` section of it separately
//!    ([`section_digests`]; `CPU `, `BUS `, `MAP `, `PPU `, `APU `, the
//!    container's own tagged sections, `rustynes_core::save_state`) and keeps
//!    the snapshot for a bounded number of frames.
//! 2. **Exchange.** Each peer sends its digests for its newest confirmed
//!    frames in every tick ([`NetMessage::StateDigest`], each frame repeated
//!    in [`REDUNDANCY`] consecutive sends so a lost datagram does not skip a
//!    frame). The receiver compares them with its own, frame by frame.
//! 3. **First divergence.** The first frame whose digests differ is the first
//!    frame whose ENTERING state differs: whatever diverged happened while
//!    the frame before it ran. Both peers find the same frame from the same
//!    two digest streams.
//! 4. **State exchange.** Each peer then sends the other the bodies of the
//!    sections that differ at that frame, from its retained snapshot
//!    ([`NetMessage::ForensicChunk`], at most
//!    [`NetMessage::MAX_FORENSIC_CHUNK`] bytes each, acknowledged
//!    cumulatively by [`NetMessage::ForensicAck`]; a lost piece is resent
//!    from the acknowledged position, go-back-N). Only differing sections
//!    travel: a corrupted RAM byte costs one `BUS ` body, about 2.4 KiB.
//! 5. **Report.** Once every differing body is in, the bytes are compared and
//!    a [`ForensicReport`] names the frame, every differing section with its
//!    first differing byte and how many differ, and where that byte lives
//!    ([`StateLocation`]: a CPU RAM address for the `BUS ` RAM image).
//!
//! # What it costs, and what it does not touch
//!
//! Hashing a snapshot section by section per confirmed frame and copying it
//! into the retention ring, on the peer that opted in only. Nothing here
//! writes the emulator, the inputs or the gameplay checksum, so a session with
//! forensics on produces every frame, sample and checksum byte-identically to
//! one with it off. The retention ring is bounded by
//! [`ForensicsConfig::retain_frames`] (about 256 KiB per frame).
//!
//! # Reading a report
//!
//! The section order a report lists ([`SECTION_ORDER`]: CPU, BUS, MAP, PPU,
//! APU) runs from the state a game's code reads most directly to the state it
//! reads least, and [`ForensicReport::primary`] is the first in it. A report
//! is a statement about bytes, not about causes: two sections can differ at
//! one frame because one corrupted the other during the frame before.
//!
//! The `APU ` section also carries the host audio resampler's position and
//! output rate, which depend on the host, not on the game: two peers at
//! different output sample rates differ there on every frame without any
//! gameplay difference. A report whose only differing section is `APU ` says
//! so ([`ForensicReport::apu_only`]).
//!
//! # Limits
//!
//! - **One divergence per session**, against the first peer it is found with.
//!   A 3-4 player mesh reports the first pair to diverge.
//! - **The retention window.** A divergence found after its frame has left
//!   the ring (the peer's digests arrived more than `retain_frames` frames
//!   late) reports the frame and the differing sections from the digests
//!   alone, with no byte offsets ([`ForensicsStatus::Unavailable`] names it).
//! - **After a fatal desync** the session no longer advances, so the frontend
//!   keeps calling [`RollbackSession::pump_forensics`] until the report
//!   arrives or it gives up.
//!
//! [`RollbackSession::enable_forensics`]: crate::RollbackSession::enable_forensics
//! [`RollbackSession::pump_forensics`]: crate::RollbackSession::pump_forensics

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use rustynes_core::{SectionIter, parse_header, tag};

use crate::message::NetMessage;
use crate::session::framebuffer_digest;

/// How many `.rns` sections a forensic digest covers.
pub const SECTION_COUNT: usize = 5;

/// The sections forensics compares, in report order.
///
/// From the state a game's code reads most directly (the CPU registers, then
/// RAM through the `BUS ` image) to the state it reads least (the APU, which
/// also carries host resampler state). [`NetMessage::ForensicChunk`] names a
/// section by its index here.
pub const SECTION_ORDER: [[u8; 4]; SECTION_COUNT] =
    [tag::CPU, tag::BUS, tag::MAP, tag::PPU, tag::APU];

/// One frame's per-section digests, in [`SECTION_ORDER`]. `0` for a section
/// the snapshot does not carry.
pub type SectionDigests = [u64; SECTION_COUNT];

/// The largest section body a peer may announce in a chunk.
///
/// The biggest real one is the `PPU ` section, about 250 KiB with its framebuffer; a mapper
/// with large CHR-RAM and PRG-RAM stays well under a megabyte. 4 MiB bounds
/// what a hostile peer can make the receiver allocate.
pub const MAX_SECTION_LEN: usize = 4 * 1024 * 1024;

/// How many consecutive `StateDigest` sends repeat each frame. A frame's
/// digests are lost only if this many datagrams in a row are.
pub const REDUNDANCY: u32 = 4;

/// Where in the `BUS ` section the 2 KiB CPU RAM image starts: after the
/// 8-byte cumulative cycle counter (`rustynes_core`'s `bus_snapshot`,
/// `encode_bus`, the BUS section version 3 layout).
pub const BUS_RAM_OFFSET: u32 = 8;
/// The CPU RAM image's length.
pub const BUS_RAM_LEN: u32 = 0x800;

/// `Some(index)` for a valid index into [`SECTION_ORDER`].
#[must_use]
pub const fn checked_section(index: u8) -> Option<u8> {
    if (index as usize) < SECTION_COUNT {
        Some(index)
    } else {
        None
    }
}

/// The body of section `tag` in a `.rns` snapshot, if it has one and parses.
#[must_use]
pub fn section_body(snapshot: &[u8], tag: [u8; 4]) -> Option<&[u8]> {
    let (_, start) = parse_header(snapshot).ok()?;
    SectionIter::new(snapshot.get(start..)?)
        .map_while(Result::ok)
        .find(|s| s.tag == tag)
        .map(|s| s.body)
}

/// The per-section digests of a `.rns` snapshot, in [`SECTION_ORDER`].
///
/// Each is the session's [`framebuffer_digest`] of each section's body (its length
/// included), `0` for a section the snapshot does not carry. Deterministic on
/// every host, like the gameplay digest.
#[must_use]
pub fn section_digests(snapshot: &[u8]) -> SectionDigests {
    let mut out = [0u64; SECTION_COUNT];
    for (slot, tag) in out.iter_mut().zip(SECTION_ORDER) {
        if let Some(body) = section_body(snapshot, tag) {
            // `| 1` keeps a present section from ever reading as "absent".
            *slot = framebuffer_digest(body) | 1;
        }
    }
    out
}

/// Where a byte of a section lives, in the terms the reader of a report can
/// act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateLocation {
    /// A byte of the `BUS ` section's cumulative cycle counter (little-endian
    /// `u64`, byte 0 the lowest).
    BusCycle {
        /// Which byte of the counter.
        byte: u8,
    },
    /// A byte of the `BUS ` section's CPU RAM image: CPU address `addr`
    /// (`$0000-$07FF`; the 2 KiB is mirrored to `$1FFF`).
    CpuRam {
        /// The CPU address.
        addr: u16,
    },
    /// Any other byte: its offset in the section body.
    Offset {
        /// The offset.
        offset: u32,
    },
}

impl StateLocation {
    /// Where byte `offset` of section `tag`'s body lives.
    #[must_use]
    pub fn of(tag: [u8; 4], offset: u32) -> Self {
        if tag == tag::BUS {
            if offset < BUS_RAM_OFFSET {
                #[allow(clippy::cast_possible_truncation)] // offset < 8
                return Self::BusCycle { byte: offset as u8 };
            }
            if offset < BUS_RAM_OFFSET + BUS_RAM_LEN {
                #[allow(clippy::cast_possible_truncation)] // < 0x800
                return Self::CpuRam {
                    addr: (offset - BUS_RAM_OFFSET) as u16,
                };
            }
        }
        Self::Offset { offset }
    }
}

impl fmt::Display for StateLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::BusCycle { byte } => write!(f, "cycle counter byte {byte}"),
            Self::CpuRam { addr } => write!(f, "CPU RAM ${addr:04X}"),
            Self::Offset { offset } => write!(f, "offset {offset:#x}"),
        }
    }
}

/// How one section differs between the two peers at the divergence frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionDiff {
    /// The section's tag (`b"BUS "`, ...).
    pub tag: [u8; 4],
    /// The first differing byte's offset in the body. When one body is a
    /// prefix of the other, the shorter one's length.
    pub first_offset: u32,
    /// Where that byte lives.
    pub location: StateLocation,
    /// Our byte there (`None` past the end of our body).
    pub local: Option<u8>,
    /// The peer's byte there (`None` past the end of its body).
    pub remote: Option<u8>,
    /// How many bytes differ in all, counting a length difference as that
    /// many differing bytes.
    pub differing_bytes: u32,
    /// `None` when the bodies were compared; otherwise why only the digests
    /// could be (the snapshot had left the retention window), and the offset
    /// fields are zero.
    pub digest_only: Option<&'static str>,
}

impl SectionDiff {
    /// Compare two bodies of section `tag`. `None` when they are equal.
    #[must_use]
    pub fn between(tag: [u8; 4], local: &[u8], remote: &[u8]) -> Option<Self> {
        let common = local.len().min(remote.len());
        let mut first = None;
        let mut differing: usize = local.len().max(remote.len()) - common;
        for (i, (a, b)) in local.iter().zip(remote).enumerate() {
            if a != b {
                first.get_or_insert(i);
                differing += 1;
            }
        }
        let first = match first {
            Some(i) => i,
            None if local.len() == remote.len() => return None,
            None => common,
        };
        let first_offset = u32::try_from(first).unwrap_or(u32::MAX);
        Some(Self {
            tag,
            first_offset,
            location: StateLocation::of(tag, first_offset),
            local: local.get(first).copied(),
            remote: remote.get(first).copied(),
            differing_bytes: u32::try_from(differing).unwrap_or(u32::MAX),
            digest_only: None,
        })
    }
}

impl fmt::Display for SectionDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = String::from_utf8_lossy(&self.tag);
        let name = name.trim_end();
        if let Some(why) = self.digest_only {
            return write!(f, "{name} differs (digest only: {why})");
        }
        let byte = |b: Option<u8>| b.map_or_else(|| "none".to_string(), |b| format!("${b:02X}"));
        write!(
            f,
            "{name} {} (ours {}, theirs {}), {} byte(s) differ",
            self.location,
            byte(self.local),
            byte(self.remote),
            self.differing_bytes
        )
    }
}

/// The result of a desync investigation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForensicReport {
    /// The first confirmed frame whose entering state differs: the
    /// divergence happened while frame `frame - 1` ran.
    pub frame: u32,
    /// The peer it was found against.
    pub peer: u8,
    /// Every differing section, in [`SECTION_ORDER`].
    pub sections: Vec<SectionDiff>,
}

impl ForensicReport {
    /// The first differing section in report order. Every report has one.
    #[must_use]
    pub fn primary(&self) -> Option<&SectionDiff> {
        self.sections.first()
    }

    /// `true` when the `APU ` section is the only one that differs, which two
    /// peers at different host output sample rates show without any gameplay
    /// difference (see the module docs).
    #[must_use]
    pub fn apu_only(&self) -> bool {
        self.sections.len() == 1 && self.sections[0].tag == tag::APU
    }
}

impl fmt::Display for ForensicReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "first divergence entering frame {} (against player {}): ",
            self.frame,
            self.peer + 1
        )?;
        for (i, s) in self.sections.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{s}")?;
        }
        if self.apu_only() {
            f.write_str(
                " (only the APU differs, which peers at different output sample rates \
                 show without a gameplay difference)",
            )?;
        }
        Ok(())
    }
}

/// Configuration for desync forensics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForensicsConfig {
    /// How many confirmed frames of snapshots to keep, so the divergence
    /// frame's bytes are still at hand when the peer's digests reveal it.
    /// About 256 KiB each; at least 1. The default, 64 frames (about 16
    /// MiB), is far more than the gap between a frame confirming on both
    /// peers, which the rollback window bounds.
    pub retain_frames: u32,
    /// How many chunks to send per tick while the state exchange runs.
    pub chunks_per_tick: u32,
}

impl Default for ForensicsConfig {
    fn default() -> Self {
        Self {
            retain_frames: 64,
            chunks_per_tick: 32,
        }
    }
}

/// Where an investigation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ForensicsStatus {
    /// Forensics is off on this peer.
    Off,
    /// Comparing digests; nothing differs yet.
    Watching {
        /// The newest frame compared with a peer's digest, if any.
        compared_through: Option<u32>,
    },
    /// The digests differ at `frame`; the bodies are being exchanged.
    Collecting {
        /// The divergence frame.
        frame: u32,
        /// Bytes received of the differing bodies so far.
        received: u64,
        /// Bytes expected in all, once every body's length is known.
        expected: Option<u64>,
    },
    /// The report is ready ([`RollbackSession::forensics_report`](crate::RollbackSession::forensics_report)).
    Reported,
    /// The report is ready but carries digests only: the divergence frame
    /// had left the retention window.
    Unavailable(&'static str),
}

/// One differing section's body arriving from the peer.
#[derive(Clone, Debug, Default)]
struct Incoming {
    /// The body's announced length, once a chunk has said it.
    total: Option<u32>,
    /// The bytes received, contiguously from offset 0.
    buf: Vec<u8>,
}

impl Incoming {
    fn complete(&self) -> bool {
        self.total.is_some_and(|t| self.buf.len() == t as usize)
    }
}

/// The investigation of one divergence.
#[derive(Clone, Debug)]
struct Divergence {
    peer: u8,
    frame: u32,
    /// Bit `i` set when section `SECTION_ORDER[i]` differs.
    mask: u8,
    /// Our bodies of the differing sections, to send.
    outgoing: [Option<Vec<u8>>; SECTION_COUNT],
    /// How much of each body the peer has acknowledged.
    peer_have: [u32; SECTION_COUNT],
    /// The peer's bodies arriving.
    incoming: [Incoming; SECTION_COUNT],
}

/// The per-session forensics state (`RollbackSession` holds one when
/// enabled). Transport-free: the session feeds it messages and sends what
/// [`Self::outbox`] returns.
#[derive(Debug)]
pub(crate) struct Forensics {
    config: ForensicsConfig,
    local_player: u8,
    /// Our digests, `local[i]` being frame `local_base + i`.
    local: VecDeque<SectionDigests>,
    local_base: u32,
    /// Snapshots of the newest `retain_frames` confirmed frames, entering
    /// `snap_base + i`.
    snapshots: VecDeque<Vec<u8>>,
    snap_base: u32,
    spare: Vec<Vec<u8>>,
    /// Peers' digests for frames we have not confirmed yet, keyed by
    /// `(frame, player)`. Bounded by `pending_window`.
    pending: BTreeMap<(u32, u8), SectionDigests>,
    /// The newest frame compared.
    compared_through: Option<u32>,
    /// The first frame whose digests have not been sent at all yet.
    digest_cursor: u32,
    divergence: Option<Divergence>,
    report: Option<ForensicReport>,
    unavailable: Option<&'static str>,
}

/// How many digest entries kept for frames we have not confirmed yet; a peer
/// cannot get further ahead of us than the rollback window, so this is ample
/// for an honest one and bounds a hostile one.
const PENDING_CAP: usize = 4096;

/// How many digest entries are retained for comparison (cheap: 40 bytes
/// each), independently of the snapshot ring.
const DIGEST_RETAIN: usize = 4096;

impl Forensics {
    pub(crate) fn new(config: ForensicsConfig, local_player: u8) -> Self {
        Self {
            config: ForensicsConfig {
                retain_frames: config.retain_frames.max(1),
                chunks_per_tick: config.chunks_per_tick.max(1),
            },
            local_player,
            local: VecDeque::new(),
            local_base: 0,
            snapshots: VecDeque::new(),
            snap_base: 0,
            spare: Vec::new(),
            pending: BTreeMap::new(),
            compared_through: None,
            digest_cursor: 0,
            divergence: None,
            report: None,
            unavailable: None,
        }
    }

    pub(crate) const fn report(&self) -> Option<&ForensicReport> {
        self.report.as_ref()
    }

    pub(crate) fn status(&self) -> ForensicsStatus {
        if let Some(why) = self.unavailable {
            return ForensicsStatus::Unavailable(why);
        }
        if self.report.is_some() {
            return ForensicsStatus::Reported;
        }
        if let Some(d) = &self.divergence {
            let received = d.incoming.iter().map(|i| i.buf.len() as u64).sum();
            let expected = (0..SECTION_COUNT)
                .filter(|&i| d.mask & (1 << i) != 0)
                .map(|i| d.incoming[i].total.map(u64::from))
                .sum();
            return ForensicsStatus::Collecting {
                frame: d.frame,
                received,
                expected,
            };
        }
        ForensicsStatus::Watching {
            compared_through: self.compared_through,
        }
    }

    /// Record the machine entering confirmed frame `frame`. Frames arrive
    /// in order, once each.
    pub(crate) fn record_local(&mut self, frame: u32, snapshot: &[u8]) {
        if self.local.is_empty() {
            self.local_base = frame;
        }
        self.local.push_back(section_digests(snapshot));
        if self.local.len() > DIGEST_RETAIN {
            self.local.pop_front();
            self.local_base += 1;
        }
        // Once a divergence is under investigation the ring has served its
        // purpose; keeping it frozen also keeps the frame's snapshot.
        if self.divergence.is_none() && self.report.is_none() {
            if self.snapshots.is_empty() {
                self.snap_base = frame;
            }
            let mut buf = self.spare.pop().unwrap_or_default();
            buf.clear();
            buf.extend_from_slice(snapshot);
            self.snapshots.push_back(buf);
            while self.snapshots.len() > self.config.retain_frames as usize {
                if let Some(old) = self.snapshots.pop_front() {
                    self.spare.push(old);
                }
                self.snap_base += 1;
            }
            self.spare.truncate(2);
        }
        // Peers' digests that were waiting for this frame.
        let waiting: Vec<(u8, SectionDigests)> = self
            .pending
            .range((frame, 0)..=(frame, u8::MAX))
            .map(|(&(_, p), &d)| (p, d))
            .collect();
        for (player, digests) in waiting {
            self.pending.remove(&(frame, player));
            self.compare(player, frame, digests);
        }
    }

    fn local_digests(&self, frame: u32) -> Option<SectionDigests> {
        let i = frame.checked_sub(self.local_base)? as usize;
        self.local.get(i).copied()
    }

    fn snapshot(&self, frame: u32) -> Option<&[u8]> {
        let i = frame.checked_sub(self.snap_base)? as usize;
        self.snapshots.get(i).map(Vec::as_slice)
    }

    /// The newest frame we hold digests for.
    fn newest_local(&self) -> Option<u32> {
        let len = u32::try_from(self.local.len()).ok()?;
        len.checked_sub(1).map(|n| self.local_base + n)
    }

    fn compare(&mut self, player: u8, frame: u32, theirs: SectionDigests) {
        if self.divergence.is_some() || self.report.is_some() {
            return;
        }
        let Some(ours) = self.local_digests(frame) else {
            return;
        };
        self.compared_through = Some(self.compared_through.map_or(frame, |c| c.max(frame)));
        let mut mask = 0u8;
        for i in 0..SECTION_COUNT {
            if ours[i] != theirs[i] {
                mask |= 1 << i;
            }
        }
        if mask != 0 {
            self.begin(player, frame, mask);
        }
    }

    fn begin(&mut self, peer: u8, frame: u32, mask: u8) {
        let Some(snapshot) = self.snapshot(frame) else {
            // The frame's bytes are gone; the digests still say which
            // sections differ.
            let why = "the divergence frame had left the retention window";
            self.unavailable = Some(why);
            self.report = Some(ForensicReport {
                frame,
                peer,
                sections: (0..SECTION_COUNT)
                    .filter(|&i| mask & (1 << i) != 0)
                    .map(|i| SectionDiff {
                        tag: SECTION_ORDER[i],
                        first_offset: 0,
                        location: StateLocation::Offset { offset: 0 },
                        local: None,
                        remote: None,
                        differing_bytes: 0,
                        digest_only: Some(why),
                    })
                    .collect(),
            });
            return;
        };
        let mut outgoing: [Option<Vec<u8>>; SECTION_COUNT] = Default::default();
        for (i, out) in outgoing.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *out = Some(
                    section_body(snapshot, SECTION_ORDER[i])
                        .unwrap_or_default()
                        .to_vec(),
                );
            }
        }
        self.divergence = Some(Divergence {
            peer,
            frame,
            mask,
            outgoing,
            peer_have: [0; SECTION_COUNT],
            incoming: Default::default(),
        });
        // The ring is no longer needed.
        self.spare.clear();
        self.snapshots.clear();
    }

    /// Fold in one forensics message. Anything that is not one is ignored.
    pub(crate) fn on_message(&mut self, msg: &NetMessage) {
        match *msg {
            NetMessage::StateDigest {
                player,
                start_frame,
                ref digests,
            } => {
                if player == self.local_player {
                    return;
                }
                for (i, d) in digests.iter().enumerate() {
                    #[allow(clippy::cast_possible_truncation)] // <= MAX_STATE_DIGESTS
                    let Some(frame) = start_frame.checked_add(i as u32) else {
                        break;
                    };
                    if self.local_digests(frame).is_some() {
                        self.compare(player, frame, *d);
                    } else if self.newest_local().is_none_or(|n| frame > n)
                        && self.pending.len() < PENDING_CAP
                    {
                        self.pending.insert((frame, player), *d);
                    }
                }
            }
            NetMessage::ForensicChunk {
                player,
                frame,
                section,
                total,
                offset,
                ref data,
            } => {
                let Some(d) = self.divergence.as_mut() else {
                    return;
                };
                let s = usize::from(section);
                if player != d.peer || frame != d.frame || d.mask & (1 << s) == 0 {
                    return;
                }
                let inc = &mut d.incoming[s];
                if inc.total.is_some_and(|t| t != total) {
                    return;
                }
                inc.total = Some(total);
                // Contiguous only: a piece that starts past what we have is
                // dropped and arrives again once the sender goes back to our
                // acknowledged position.
                let have = inc.buf.len();
                let start = offset as usize;
                let end = start + data.len();
                if start <= have && end > have {
                    inc.buf.extend_from_slice(&data[have - start..]);
                }
                self.try_finish();
            }
            NetMessage::ForensicAck {
                player,
                frame,
                section,
                have,
            } => {
                if let Some(d) = self.divergence.as_mut()
                    && player == d.peer
                    && frame == d.frame
                {
                    // `have` is the peer's to choose. Record it only for a
                    // section this divergence sends, and never past the end
                    // of our body: the send loop slices from it.
                    let s = usize::from(section);
                    if d.mask & (1 << s) != 0
                        && let Some(body) = &d.outgoing[s]
                    {
                        let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
                        d.peer_have[s] = d.peer_have[s].max(have.min(len));
                    }
                }
            }
            _ => {}
        }
    }

    fn try_finish(&mut self) {
        let Some(d) = &self.divergence else {
            return;
        };
        let all = (0..SECTION_COUNT)
            .filter(|&i| d.mask & (1 << i) != 0)
            .all(|i| d.incoming[i].complete());
        if !all || self.report.is_some() {
            return;
        }
        let mut sections = Vec::new();
        for (i, tag) in SECTION_ORDER.into_iter().enumerate() {
            if d.mask & (1 << i) == 0 {
                continue;
            }
            let ours = d.outgoing[i].as_deref().unwrap_or_default();
            if let Some(diff) = SectionDiff::between(tag, ours, &d.incoming[i].buf) {
                sections.push(diff);
            }
        }
        self.report = Some(ForensicReport {
            frame: d.frame,
            peer: d.peer,
            sections,
        });
    }

    /// What to send this tick: our newest digests (each frame in
    /// [`REDUNDANCY`] consecutive sends) and, while an exchange runs, a
    /// window of chunks from where the peer's acknowledgement stands, plus
    /// our own acknowledgements. Sending continues after our report is ready,
    /// so the peer can finish its own.
    pub(crate) fn outbox(&mut self) -> Vec<NetMessage> {
        let mut out = Vec::new();
        if let Some(newest) = self.newest_local() {
            // Everything not yet sent once, and the newest REDUNDANCY frames
            // again, so a burst of confirmations skips no frame and a lost
            // datagram is covered by the next ticks' repeats.
            let first = self
                .digest_cursor
                .min(newest.saturating_sub(REDUNDANCY - 1))
                .max(self.local_base);
            let mut start = first;
            while start <= newest {
                #[allow(clippy::cast_possible_truncation)] // a small constant
                let end = newest.min(start + NetMessage::MAX_STATE_DIGESTS as u32 - 1);
                let digests: Vec<SectionDigests> = (start..=end)
                    .filter_map(|f| self.local_digests(f))
                    .collect();
                out.push(NetMessage::StateDigest {
                    player: self.local_player,
                    start_frame: start,
                    digests,
                });
                start = end + 1;
            }
            self.digest_cursor = newest + 1;
        }
        let Some(d) = &self.divergence else {
            return out;
        };
        let mut budget = self.config.chunks_per_tick as usize;
        for i in 0..SECTION_COUNT {
            #[allow(clippy::cast_possible_truncation)] // < SECTION_COUNT
            let section = i as u8;
            if let Some(body) = &d.outgoing[i] {
                let total = u32::try_from(body.len()).unwrap_or(u32::MAX);
                // Clamped again here so no stored ack can reach past the
                // body, whatever wrote it.
                let mut at = (d.peer_have[i] as usize).min(body.len());
                // An empty body still announces its length once, unless the
                // peer has already acknowledged it.
                let mut first = d.peer_have[i] == 0;
                while budget > 0 && (at < body.len() || (first && body.is_empty())) {
                    let end = (at + NetMessage::MAX_FORENSIC_CHUNK).min(body.len());
                    out.push(NetMessage::ForensicChunk {
                        player: self.local_player,
                        frame: d.frame,
                        section,
                        total,
                        offset: u32::try_from(at).unwrap_or(u32::MAX),
                        data: body[at..end].to_vec(),
                    });
                    budget -= 1;
                    at = end;
                    first = false;
                }
                // Re-announce a finished body now and then is unnecessary:
                // the peer's ack says it has it all.
            }
            if d.mask & (1 << i) != 0 {
                out.push(NetMessage::ForensicAck {
                    player: self.local_player,
                    frame: d.frame,
                    section,
                    have: u32::try_from(d.incoming[i].buf.len()).unwrap_or(u32::MAX),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_name_the_bus_ram_and_cycle() {
        assert_eq!(
            StateLocation::of(tag::BUS, 0),
            StateLocation::BusCycle { byte: 0 }
        );
        assert_eq!(
            StateLocation::of(tag::BUS, 8),
            StateLocation::CpuRam { addr: 0 }
        );
        assert_eq!(
            StateLocation::of(tag::BUS, 8 + 0x7FF),
            StateLocation::CpuRam { addr: 0x7FF }
        );
        assert_eq!(
            StateLocation::of(tag::BUS, 8 + 0x800),
            StateLocation::Offset { offset: 0x808 }
        );
        assert_eq!(
            StateLocation::of(tag::PPU, 9),
            StateLocation::Offset { offset: 9 }
        );
    }

    #[test]
    fn a_diff_finds_the_first_byte_and_counts_them_all() {
        assert_eq!(SectionDiff::between(tag::CPU, &[1, 2, 3], &[1, 2, 3]), None);
        let d = SectionDiff::between(tag::BUS, &[0; 20], &{
            let mut b = [0u8; 20];
            b[12] = 7;
            b[15] = 9;
            b
        })
        .expect("differs");
        assert_eq!(d.first_offset, 12);
        assert_eq!(d.location, StateLocation::CpuRam { addr: 4 });
        assert_eq!(
            (d.local, d.remote, d.differing_bytes),
            (Some(0), Some(7), 2)
        );
        let short = SectionDiff::between(tag::MAP, &[1, 2], &[1, 2, 3, 4]).expect("differs");
        assert_eq!(
            (short.first_offset, short.local, short.remote),
            (2, None, Some(3))
        );
        assert_eq!(short.differing_bytes, 2);
    }

    #[test]
    fn section_digests_cover_each_section_separately() {
        let mut nes = rustynes_core::Nes::from_rom(&tiny_rom()).expect("rom");
        let _ = nes.run_frame();
        let mut a = Vec::new();
        nes.snapshot_core_into(&mut a);
        let base = section_digests(&a);
        assert!(base.iter().all(|&d| d != 0), "every section present");
        let flipped = nes.peek(0x0123) ^ 0xFF;
        nes.poke_ram(0x0123, flipped);
        let mut b = Vec::new();
        nes.snapshot_core_into(&mut b);
        let poked = section_digests(&b);
        for i in 0..SECTION_COUNT {
            assert_eq!(
                base[i] != poked[i],
                SECTION_ORDER[i] == tag::BUS,
                "only BUS moves for a RAM byte ({:?})",
                String::from_utf8_lossy(&SECTION_ORDER[i])
            );
        }
        let diff = SectionDiff::between(
            tag::BUS,
            section_body(&a, tag::BUS).expect("bus"),
            section_body(&b, tag::BUS).expect("bus"),
        )
        .expect("differs");
        assert_eq!(diff.location, StateLocation::CpuRam { addr: 0x0123 });
        assert_eq!(diff.differing_bytes, 1);
    }

    /// A peer's `ForensicAck` carries a `have` of its choosing. Past the end
    /// of our body it must not reach a slice: with an empty body (a section
    /// the snapshot lacks, `begin`'s `unwrap_or_default`) the send loop once
    /// sliced `body[have..0]` and panicked, and on a 32-bit target a `have`
    /// near `u32::MAX` overflowed `at + MAX_FORENSIC_CHUNK`. A hostile opted-in
    /// peer could crash the session that way (security review, v3.7.0).
    #[test]
    fn an_ack_past_the_body_never_reaches_a_slice() {
        let mut f = Forensics::new(ForensicsConfig::default(), 0);
        let mut outgoing: [Option<Vec<u8>>; SECTION_COUNT] = Default::default();
        outgoing[0] = Some(Vec::new());
        outgoing[1] = Some(vec![0xAB; 10]);
        f.divergence = Some(Divergence {
            peer: 1,
            frame: 5,
            mask: 0b11,
            outgoing,
            peer_have: [0; SECTION_COUNT],
            incoming: Default::default(),
        });
        for section in [0u8, 1, 2] {
            f.on_message(&NetMessage::ForensicAck {
                player: 1,
                frame: 5,
                section,
                have: u32::MAX,
            });
        }
        // Must not panic. Every chunk sent lies inside its body, and the
        // non-empty body the peer acknowledged in full is not resent. (The
        // empty body still makes its one length announcement, at offset 0
        // with no data: a clamped ack of 0 cannot say "I have it", and the
        // announcement is in bounds.)
        let out = f.outbox();
        for m in &out {
            if let NetMessage::ForensicChunk {
                section,
                offset,
                data,
                total,
                ..
            } = m
            {
                assert!(*offset as usize + data.len() <= *total as usize, "{m:?}");
                assert_ne!(*section, 1, "section 1 was resent past its ack: {m:?}");
            }
        }
        // The stored ack is clamped to the body, and a section outside the
        // divergence's mask is not recorded at all.
        let d = f.divergence.as_ref().unwrap();
        assert_eq!(d.peer_have[..3], [0, 10, 0]);
    }

    /// A 16 KiB NROM image that spins in place.
    fn tiny_rom() -> Vec<u8> {
        let mut rom = b"NES\x1A\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        let mut prg = vec![0xEAu8; 16 * 1024];
        prg[..3].copy_from_slice(&[0x4C, 0x00, 0xC0]);
        for v in [0x3FFA, 0x3FFC, 0x3FFE] {
            prg[v] = 0x00;
            prg[v + 1] = 0xC0;
        }
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 8 * 1024]);
        rom
    }
}
