//! TAS movie (`.rnm`) recording and playback.
//!
//! A movie is a *reproducible start point* plus the *per-frame input stream*
//! applied on top of it. Because the core honours the hard determinism
//! contract (same seed + ROM + input sequence ⇒ bit-identical framebuffer
//! and audio — see `CLAUDE.md`), replaying the recorded inputs from the
//! recorded start point re-derives every pixel and sample bit-for-bit. No
//! state deltas or frame hashes are stored.
//!
//! See `docs/adr/0008-tas-movie-format.md` for the format spec, the
//! structural references (Mesen2 `.mmo`, FCEUX `.fm2`, `TetaNES` `.replay`),
//! and the forward-compatibility story (layered on ADR 0003).
//!
//! # On-wire layout
//!
//! ```text
//! HEADER:
//!     magic           : "RNESMOV1"   (8 bytes)
//!     format version  : u16 LE        (currently 5 = MOVIE_FORMAT_VERSION)
//!     emulation epoch : u32 LE        (format 5+; `EMULATION_EPOCH`, ADR 0045)
//!     region          : u8            (0 = NTSC, 1 = PAL, 2 = Dendy)
//!     flags           : u8            (bit0 = embedded save-state start point,
//!                                      bit1 = board description recorded)
//!     rom sha-256     : [u8; 32]      (`Nes::rom_sha256`: the image after its
//!                                      16-byte header — the ROM identity)
//!     frame count     : u32 LE
//!     bytes per frame : u8            (currently 5: P1, P2, P3, P4,
//!                                      expansion-reserved; 3 before format 3)
//! OPTIONS (format 3+): u32 LE length + that many bytes:
//!     the `HardwareOptions` encoding, then (flags bit1) the
//!     `BoardDescription` encoding — see `crate::hardware_options`
//! START POINT (only when flags bit0 set):
//!     length-prefixed `.rns` save-state blob (u32 LE length + bytes)
//! INPUT STREAM:
//!     frame_count * bytes_per_frame raw bytes; each frame =
//!     [p1, p2, p3, p4, expansion]
//! ```
//!
//! This module is `no_std`-clean: it uses only `core` + `alloc` and the
//! `BinWriter` / `BinReader` primitives from [`crate::save_state`].

use alloc::vec::Vec;

use alloc::string::String;

use crate::Region;
use crate::controller::Buttons;
use crate::hardware_options::{BoardDescription, HardwareOptions, OptionsDecodeError};
use crate::nes::Nes;
use crate::save_state::{BinReader, BinWriter, SnapshotError};
use thiserror::Error;

/// Magic header bytes — first 8 bytes of every `.rnm` movie file.
pub const MOVIE_MAGIC: &[u8; 8] = b"RNESMOV1";

/// Current movie container-format version.
///
/// - v1 (v1.1.0 ..): the format documented above.
/// - **v2 (v2.0.0 "Timebase" rc.1, ADR 0028)**: on-wire layout unchanged —
///   this is purely an epoch marker. A `.rnm` with `format_version < 2` was
///   necessarily recorded on a pre-promote (pre-beta.4) build; per the
///   determinism contract, its INPUT STREAM still replays fine (nothing
///   about frame timing or button semantics changed), but the
///   frame-for-frame bit-identical reproduction guarantee the movie format
///   depends on is only proven within a single engine timebase — the
///   one-clock promote changed how master-clock/PPU/CPU phase advances
///   internally, so a v1-recorded movie's *exact* framebuffer/audio replay
///   on the v2.0.0-line engine is unverified, not guaranteed. Do NOT
///   attempt timeline transcoding (re-deriving a v2-native recording from
///   a v1 one) — that is out of scope; the honest move is surfacing the
///   epoch, not silently promising equivalence. See
///   [`recorded_before_v2_timebase`] for the check callers (TAS tooling,
///   frontend movie-load UI) should use before relying on verify-replay.
/// - **v3 (v2.9.8, ADR 0028's epoch rule)**: an OPTIONS block follows the
///   fixed header, carrying every emulation-affecting host option the movie
///   was recorded with ([`HardwareOptions`]) and, for a recorded movie, the
///   cartridge board the header described ([`BoardDescription`]). Playback
///   applies the options before frame 0 and refuses a board or region that
///   differs, so a replay runs the recorded machine whatever the player's own
///   settings are. A v1 or v2 movie does not say which machine it ran on, so
///   it is refused ([`MIN_MOVIE_FORMAT_VERSION`]) rather than replayed on a
///   guess; the maintainer accepted breaking them (2026-10-01).
/// - **v4 (v2.9.9, core re-audit NC-10)**: the [`BoardDescription`] gains the
///   PRG-ROM and CHR-ROM sizes and the raw header nametable bits. v3 could
///   not tell two headers that split one body differently, or that differ
///   only in the bits mappers 30 and 218 wire from, so a movie replayed
///   silently on a different machine. v3 is refused, as v1 and v2 are, under
///   the same "enduring over compatible" rule.
/// - **v5 (v3.0.0, ADR 0045)**: the fixed header carries the
///   [`EMULATION_EPOCH`](crate::EMULATION_EPOCH) the movie was recorded under,
///   straight after the format version so a tool can read it without parsing
///   the rest. v2.9.9 and v3.0.0 emulate MMC3 games with the background at
///   `$1000` differently (T-MMC3-BG-A12), and a format-4 movie does not say
///   which behaviour it assumes, so v4 is refused. A v5 movie from another
///   epoch is refused with [`MovieError::EpochMismatch`].
/// - **v6 (v3.1.0)**: the [`crate::HardwareOptions`] record gains the
///   CPU-multiplier overclock and the sprite-limit option (`T-CPU-OVERCLOCK`,
///   `T-SPRITE-LIMIT`). A v5 options record is one field shorter and would
///   decode as garbage, so v5 is refused. No replayable movie is lost: every
///   v5 movie was recorded under epoch 1 or 2, which v3.1.0 (epoch 3) refuses
///   anyway.
pub const MOVIE_FORMAT_VERSION: u16 = 6;

/// The oldest container version this build replays: v6.
///
/// v6 is the first whose options record carries the CPU overclock and the
/// sprite-limit option (v5 first recorded the emulation epoch, v4 the board,
/// v3 the options). Older movies fail with [`MovieError::FormatTooOld`].
pub const MIN_MOVIE_FORMAT_VERSION: u16 = 6;

/// Peek a `.rnm` blob's header to learn its recording epoch.
///
/// Checks whether it was recorded on a pre-v2.0.0-timebase build
/// (`format_version < 2`), WITHOUT fully parsing the movie. Intended for
/// tooling/UI that wants to warn before relying on the determinism
/// (verify-replay) guarantee across the v2.0.0 engine-timebase boundary —
/// see [`MOVIE_FORMAT_VERSION`]'s v2 doc.
///
/// Since v2.9.8 [`Movie::deserialize`] refuses every movie older than
/// [`MIN_MOVIE_FORMAT_VERSION`], so a movie that parses is never pre-v2; the
/// function survives for tooling that inspects raw files, and still answers
/// for any header it is shown.
///
/// # Errors
///
/// Returns [`MovieError::HeaderTruncated`] or [`MovieError::BadMagic`] if
/// the blob doesn't even have a valid movie header.
pub fn recorded_before_v2_timebase(bytes: &[u8]) -> Result<bool, MovieError> {
    const MIN_LEN: usize = 8 + 2;
    if bytes.len() < MIN_LEN {
        return Err(MovieError::HeaderTruncated {
            expected: MIN_LEN,
            got: bytes.len(),
        });
    }
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&bytes[..8]);
    if &magic != MOVIE_MAGIC {
        return Err(MovieError::BadMagic { got: magic });
    }
    let format_version = u16::from_le_bytes([bytes[8], bytes[9]]);
    Ok(format_version < 2)
}

/// Bytes stored per recorded frame: players 1-4, then a reserved
/// expansion-port byte (always `0` today).
///
/// Format 3 (v2.9.8) widened the record from 3 bytes (P1, P2, expansion) to 5
/// so Four Score players 3 and 4 are recorded; the order puts the players
/// first, so a narrower record (width 2 = two pads, width 4 = four pads) reads
/// as "the later fields absent". Stored explicitly in the header so a future
/// device byte can grow the record without a container-version bump.
pub const BYTES_PER_FRAME: u8 = 5;

/// Header flag: an embedded `.rns` save-state start point follows the header.
const FLAG_HAS_SAVE_STATE: u8 = 0x01;

/// Header flag (format 3+): the OPTIONS block carries a [`BoardDescription`]
/// after the [`HardwareOptions`]. Clear for a foreign import, whose source
/// format records no header.
const FLAG_HAS_BOARD: u8 = 0x02;

/// The header flags this build understands. Any other bit set is refused, so a
/// later format cannot be half-read.
const KNOWN_FLAGS: u8 = FLAG_HAS_SAVE_STATE | FLAG_HAS_BOARD;

/// Per-frame controller input: four controller ports plus an expansion byte.
///
/// Each port is a `Buttons` value. Bit layout matches FCEUX `.fm2`
/// (`bit0=A .. bit7=Right`), which is exactly [`Buttons::bits`].
///
/// # Players 3 and 4 (v2.9.8)
///
/// `p3` / `p4` are the Four Score's players, polled through `$4016` / `$4017`
/// only while the adapter is plugged in (which the movie's
/// [`HardwareOptions::four_score`] records). Until v2.9.8 the struct held two
/// ports, so a four-player recording captured half of what drove it and the
/// `.fm2` importer dropped pads 3 and 4.
///
/// # Why `#[non_exhaustive]`
///
/// Adding `p3` / `p4` broke every caller that built the struct by literal,
/// which is why the change waited for a breaking release. The next field is
/// foreseeable -- an expansion-port device byte, a soft-reset command -- and
/// `.rnm` already stores its per-frame width so the FORMAT side of such a
/// field is additive; `#[non_exhaustive]` makes the API side additive too.
/// Outside this crate a frame is built with [`Self::new`],
/// [`Self::four_players`] or [`Default`] and then edited through its public
/// fields, all of which keep compiling when a field is added.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct FrameInput {
    /// Player 1 (`$4016`) button state.
    pub p1: Buttons,
    /// Player 2 (`$4017`) button state.
    pub p2: Buttons,
    /// Player 3 (Four Score, multiplexed on `$4016`) button state.
    pub p3: Buttons,
    /// Player 4 (Four Score, multiplexed on `$4017`) button state.
    pub p4: Buttons,
    /// Reserved expansion-port byte (currently always `0`).
    pub expansion: u8,
}

impl FrameInput {
    /// Build a two-controller frame (players 3/4 released, no expansion byte).
    #[must_use]
    pub const fn new(p1: Buttons, p2: Buttons) -> Self {
        Self::four_players(p1, p2, Buttons::empty(), Buttons::empty())
    }

    /// v2.9.8 — build a four-controller frame (no expansion byte).
    #[must_use]
    pub const fn four_players(p1: Buttons, p2: Buttons, p3: Buttons, p4: Buttons) -> Self {
        Self {
            p1,
            p2,
            p3,
            p4,
            expansion: 0,
        }
    }

    /// v2.9.8 — the buttons on controller `port` (0-3 = players 1-4).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`, matching [`Nes::set_buttons`].
    #[must_use]
    pub const fn port(&self, port: usize) -> Buttons {
        match port {
            0 => self.p1,
            1 => self.p2,
            2 => self.p3,
            3 => self.p4,
            _ => panic!("controller port out of range"),
        }
    }

    /// v2.9.8 — mutable access to controller `port` (0-3 = players 1-4).
    ///
    /// # Panics
    ///
    /// Panics if `port` is not in `0..=3`.
    pub const fn port_mut(&mut self, port: usize) -> &mut Buttons {
        match port {
            0 => &mut self.p1,
            1 => &mut self.p2,
            2 => &mut self.p3,
            3 => &mut self.p4,
            _ => panic!("controller port out of range"),
        }
    }

    /// v2.9.8 — drive all four controller ports of `nes` from this frame.
    ///
    /// Ports 2 and 3 are only polled while the Four Score is plugged in, so
    /// setting them on a two-pad machine changes nothing it reads.
    pub const fn apply_to(&self, nes: &mut Nes) {
        nes.set_buttons(0, self.p1);
        nes.set_buttons(1, self.p2);
        nes.set_buttons(2, self.p3);
        nes.set_buttons(3, self.p4);
    }

    /// v2.9.8 — the input currently held on all four ports of `nes`.
    #[must_use]
    pub const fn held_on(nes: &Nes) -> Self {
        Self::four_players(
            nes.buttons(0),
            nes.buttons(1),
            nes.buttons(2),
            nes.buttons(3),
        )
    }
}

/// Marker for the optional attestation tail: `"RNAT"` little-endian.
///
/// Read as a `u32` after the re-record count. A movie with no attestation simply
/// ends there, so the marker is what distinguishes "no attestation recorded" from
/// "attestation present" — absence is a fact, not a parse failure.
pub const ATTESTATION_MAGIC: u32 = u32::from_le_bytes(*b"RNAT");

/// Attestation tail schema version.
///
/// 2 (v2.9.8): each frame folds in all four players' bytes, not two, so a
/// version-1 tail describes a different hash and is not compared.
pub const ATTESTATION_VERSION: u16 = 2;

/// Frames between recorded checkpoint hashes.
///
/// The final hash alone would answer "did this run reproduce?"; the checkpoints
/// answer "and if not, roughly where did it stop reproducing?", which is the
/// difference between a verdict and a diagnosis. At 8 bytes per checkpoint a
/// ten-minute run costs about 4.5 KiB.
pub const ATTESTATION_CHECKPOINT_INTERVAL: u32 = 64;

/// A rolling hash of a run's video output, and the checkpoints along the way.
///
/// # What is attested
///
/// Per frame, **the input applied and the framebuffer it produced**, folded into
/// one rolling hash. Both halves are load-bearing:
///
/// - The framebuffer is the user-visible output the determinism contract
///   promises is bit-identical for the same ROM, seed, and input sequence.
/// - The input is folded in because output alone does not pin the input stream.
///   A ROM that ignores the controller — a test ROM, an attract-mode demo, a
///   cutscene — produces identical video no matter what buttons the movie
///   claims were pressed, so an output-only hash would confirm a tampered input
///   log as genuine. Found by exactly that: an end-to-end tamper test flipped a
///   button bit in a movie for an input-ignoring ROM and the run still verified.
///
/// Together they attest the real claim: *these inputs, applied to this ROM,
/// produced this output.*
///
/// Hashing the core snapshot instead would be strictly stronger at detecting
/// divergence, and was rejected for one reason: the snapshot schema is versioned
/// and bumps between releases (`PPU_SNAPSHOT_VERSION` has reached 8), so every
/// schema bump would silently invalidate every previously-recorded attestation.
/// A 256x240 RGBA framebuffer is stable for as long as the NES is the NES. An
/// attestation is only worth recording if it can still be checked years later.
///
/// Audio is **not** covered: samples are drained by the host as they are
/// produced, so the core cannot see a whole run's audio without the frontend
/// cooperating. Saying so is better than implying coverage that is not there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attestation {
    /// Number of frames the attestation covers. Cross-checked against the input
    /// stream on load, so a tail that describes a different run is rejected
    /// rather than compared against the wrong frame count.
    pub frame_count: u32,
    /// Rolling hash after the final frame.
    pub final_hash: u64,
    /// Rolling hash after frames `INTERVAL-1`, `2*INTERVAL-1`, ... in order.
    pub checkpoints: Vec<u64>,
}

/// FNV-1a-style rolling hash over 64-bit words.
///
/// # This is a tamper-EVIDENT digest, not a cryptographic one
///
/// 64-bit FNV-1a is not collision resistant, and its round function is
/// invertible (`PRIME` is odd, so multiplication is a bijection mod 2^64). It
/// reliably detects accidental divergence — a different build, a real
/// nondeterminism bug, a truncated file — and casual edits, which is what
/// [`Movie::verify`] is for. It does **not** resist a motivated forger: anyone
/// who edits the movie can recompute the digest, and nothing here binds the
/// record to an author.
///
/// Say "reproduces the recorded run", not "proves the run is genuine". Making a
/// forgery-resistant claim would need a signature over the whole record with a
/// key the verifier trusts, which is a different feature. Flagged in review on
/// PR #356, where the surrounding prose had drifted into the stronger claim.
///
/// Written here rather than reused: the two existing `fnv1a64` helpers in this
/// workspace are behind `rustynes-ppu`'s `ppu-state-trace` feature and in the
/// test harness respectively, and neither is reachable from `no_std` core code
/// on the default build. It is six lines; taking a feature dependency to avoid
/// them would cost more than it saves.
///
/// Words rather than bytes because a framebuffer is 245,760 bytes and this runs
/// once per frame during both recording and verification.
#[derive(Clone, Copy, Debug)]
struct RollingHash(u64);

impl RollingHash {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    /// Fold a byte slice in, 8 bytes at a time. A trailing partial word is
    /// zero-padded, which is unambiguous here because every input is a
    /// fixed-size framebuffer.
    fn write(&mut self, bytes: &[u8]) {
        let (words, rem) = bytes.as_chunks::<8>();
        for w in words {
            self.0 = (self.0 ^ u64::from_le_bytes(*w)).wrapping_mul(Self::PRIME);
        }
        if !rem.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rem.len()].copy_from_slice(rem);
            self.0 = (self.0 ^ u64::from_le_bytes(buf)).wrapping_mul(Self::PRIME);
        }
    }
}

/// Accumulates an [`Attestation`] one frame at a time.
///
/// Feed it the framebuffer after each `run_frame`; it maintains the rolling hash
/// and emits a checkpoint every [`ATTESTATION_CHECKPOINT_INTERVAL`] frames.
#[derive(Clone, Debug)]
pub struct AttestationBuilder {
    hash: RollingHash,
    frame_count: u32,
    checkpoints: Vec<u64>,
}

impl AttestationBuilder {
    /// Start a fresh attestation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            hash: RollingHash::new(),
            frame_count: 0,
            checkpoints: Vec::new(),
        }
    }

    /// Fold in one frame: the input applied, then the video it produced.
    pub fn push_frame(&mut self, input: FrameInput, framebuffer: &[u8]) {
        self.hash.write(&[
            input.p1.bits(),
            input.p2.bits(),
            input.p3.bits(),
            input.p4.bits(),
            input.expansion,
        ]);
        self.hash.write(framebuffer);
        self.frame_count = self.frame_count.saturating_add(1);
        if self
            .frame_count
            .is_multiple_of(ATTESTATION_CHECKPOINT_INTERVAL)
        {
            self.checkpoints.push(self.hash.0);
        }
    }

    /// Frames folded in so far.
    #[must_use]
    pub const fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// The rolling hash as it stands.
    #[must_use]
    pub const fn current_hash(&self) -> u64 {
        self.hash.0
    }

    /// Finish and produce the attestation.
    #[must_use]
    pub fn finish(self) -> Attestation {
        Attestation {
            frame_count: self.frame_count,
            final_hash: self.hash.0,
            checkpoints: self.checkpoints,
        }
    }
}

impl Default for AttestationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// The result of replaying an attested movie and comparing it to its record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyOutcome {
    /// The replay reproduced the recorded run exactly.
    Match {
        /// Frames replayed.
        frames: u32,
        /// The hash both the record and the replay produced.
        hash: u64,
    },
    /// The replay diverged.
    Mismatch {
        /// Frames replayed.
        frames: u32,
        /// The hash the movie claims.
        expected: u64,
        /// The hash this replay produced.
        got: u64,
        /// Index of the first checkpoint that disagreed, if any did. The
        /// divergence began somewhere in the
        /// [`ATTESTATION_CHECKPOINT_INTERVAL`] frames ending at
        /// `(index + 1) * INTERVAL - 1`. `None` means every recorded checkpoint
        /// matched and only the final hash differs — i.e. the divergence is in
        /// the tail after the last checkpoint.
        first_bad_checkpoint: Option<u32>,
    },
    /// The movie carries no attestation, so there is nothing to verify against.
    /// Not an error: most movies are recorded without one.
    NotAttested,
}

/// Read the optional attestation tail, if one is present and coherent.
///
/// Returns `None` — never an error — for every way the tail can be absent or
/// unusable: no bytes left, a different marker, a schema version this build does
/// not know, a truncated body, or a frame count that disagrees with the input
/// stream. A movie without a usable attestation is a perfectly good movie; the
/// only wrong answer would be to report an attestation that does not describe
/// this run, so a `frame_count` mismatch drops it rather than comparing against
/// the wrong length.
///
/// The checkpoint count is bounded by what the remaining input could actually
/// hold before reserving, for the same reason `frame_count` is: a hostile
/// four-byte field must not be able to request a multi-gigabyte allocation.
fn read_attestation(r: &mut BinReader<'_>, frames: usize) -> Option<Attestation> {
    if r.u32().ok()? != ATTESTATION_MAGIC {
        return None;
    }
    if r.u16().ok()? != ATTESTATION_VERSION {
        return None;
    }
    let frame_count = r.u32().ok()?;
    if frame_count as usize != frames {
        return None;
    }
    let final_hash = r.u64().ok()?;
    let declared = r.u32().ok()? as usize;
    let max_plausible = r.remaining() / core::mem::size_of::<u64>();
    let mut checkpoints = Vec::with_capacity(declared.min(max_plausible));
    for _ in 0..declared {
        checkpoints.push(r.u64().ok()?);
    }
    Some(Attestation {
        frame_count,
        final_hash,
        checkpoints,
    })
}

/// Where a movie begins. Clean-room analogue of Mesen2's `RecordMovieFrom`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartPoint {
    /// Power-on the ROM fresh, then apply inputs from frame 0. The most
    /// durable start point across version transitions (depends only on the
    /// ROM and the deterministic power-on).
    PowerOn,
    /// Restore this embedded `.rns` snapshot, then apply inputs from there.
    /// Enables save-state branching (a movie that begins mid-game).
    SaveState(Vec<u8>),
}

/// Errors produced by movie encode / decode / playback.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MovieError {
    /// The blob is shorter than the fixed header.
    #[error("movie truncated: header needs {expected} bytes, got {got}")]
    HeaderTruncated {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        got: usize,
    },

    /// The magic prefix is wrong.
    #[error("movie magic mismatch: expected {:?}, got {got:?}", MOVIE_MAGIC)]
    BadMagic {
        /// Bytes observed at the magic offset.
        got: [u8; 8],
    },

    /// The container format version is outside the range we understand.
    #[error("movie container format version {got} not supported (max {max})")]
    UnsupportedFormat {
        /// Version we read.
        got: u16,
        /// Highest version we accept.
        max: u16,
    },

    /// v2.9.8 — the movie was written by an older release whose format does
    /// not record everything a faithful replay needs: before format 3 the
    /// emulation options, before 4 the full board, before 5 (v3.0.0) the
    /// emulation epoch. (Until v3.0.0 the message named only the options,
    /// which stopped being the whole story at format 4.)
    #[error(
        "movie format version {got} was written by an older release of RustyNES \
         (this version replays format {min} and later), and it does not record \
         everything a faithful replay needs; re-record it with this version"
    )]
    FormatTooOld {
        /// Version we read.
        got: u16,
        /// Oldest version this build replays.
        min: u16,
    },

    /// v2.9.8 — the header flags carry a bit this build does not know.
    #[error("movie header flags {0:#04x} carry bits this build does not understand")]
    UnknownFlags(u8),

    /// v2.9.8 — the OPTIONS block is malformed (an unknown enum byte, a bad
    /// Game Genie code, a truncated field).
    #[error("movie emulation options are malformed: {0}")]
    BadOptions(OptionsDecodeError),

    /// v2.9.8 — the movie was recorded on another region (from a header that
    /// said PAL, say, where this one says NTSC). A region is built into the
    /// machine at load, so it cannot be applied; the movie is refused.
    #[error(
        "movie was recorded on {movie:?} timing but this ROM runs as {host:?}; \
         load a dump whose header declares {movie:?}"
    )]
    RegionMismatch {
        /// The movie's region.
        movie: Region,
        /// The running machine's region.
        host: Region,
    },

    /// v2.9.8 — same ROM, different header: the cartridge the movie ran on
    /// had another mapper, submapper, mirroring, RAM size, battery or
    /// trainer. Since v2.9.8 the ROM identity excludes the header, so this is
    /// what tells a re-headered dump (or a changed database correction)
    /// apart.
    #[error(
        "movie was recorded on the same ROM with a different header: the {field} \
         differs; load the dump (or game-database correction) it was made with"
    )]
    BoardMismatch {
        /// The first board field that differs.
        field: &'static str,
    },

    /// v2.9.8 — an option could not be applied to this machine (a Game Genie
    /// code that does not decode, in a hand-built movie).
    #[error("movie emulation option could not be applied: Game Genie code {0:?}")]
    OptionNotApplicable(String),

    /// v2.9.8 — the movie's embedded start-point save state predates the
    /// `.rns` container epoch 3 (ADR 0042), which v2.9.8 refuses. Kept apart
    /// from [`MovieError::BadSaveState`] so the message says what to do, in
    /// the same words as [`MovieError::FormatTooOld`].
    #[error(
        "movie starts from a save state written by an older release (container \
         version {got}, this build reads {min} and later); re-record it with this \
         version"
    )]
    StartStateTooOld {
        /// The embedded state's container version.
        got: u16,
        /// Oldest container version this build reads.
        min: u16,
    },

    /// The header declared more bytes-per-frame than this build understands.
    #[error("movie declares {got} bytes/frame; this build understands {max}")]
    UnsupportedFrameWidth {
        /// Declared width.
        got: u8,
        /// Width this build can parse.
        max: u8,
    },

    /// The region byte is not a value this build understands.
    #[error("movie region byte {0} is not a known region")]
    BadRegion(u8),

    /// The body (start point and/or input stream) ran past EOF.
    #[error("movie truncated mid-body at offset {0}")]
    Eof(usize),

    /// The embedded start-point save state failed to apply.
    #[error("movie start-point save state invalid: {0}")]
    BadSaveState(#[from] SnapshotError),

    /// The running ROM's hash does not match the movie's recorded hash.
    #[error("movie ROM hash mismatch (this movie was recorded against a different ROM)")]
    RomMismatch,

    /// v3.0.0 (ADR 0045) — the movie was recorded by a version of `RustyNES`
    /// that emulates differently: its [`EMULATION_EPOCH`](crate::EMULATION_EPOCH)
    /// is not this build's. Replaying it would run the recorded inputs on
    /// different timing, so it is refused rather than allowed to diverge.
    #[error(
        "movie was recorded by a version of RustyNES that emulates differently \
         (emulation epoch {movie}; this version is epoch {core}); replay it with \
         that version, or re-record it with this one"
    )]
    EpochMismatch {
        /// The epoch the movie records.
        movie: u32,
        /// This build's [`EMULATION_EPOCH`](crate::EMULATION_EPOCH).
        core: u32,
    },
}

/// A complete TAS movie: a versioned header, a start point, and the
/// per-frame input stream.
///
/// `#[non_exhaustive]` since v3.0.0 (T-API-EXTENSIBLE): build one with
/// [`Movie::new`] (or a [`MovieRecorder`] / an importer), then set the public
/// fields that differ from its defaults. A later field is then not a break.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct Movie {
    /// Cartridge region the movie was recorded under. Checked, not applied:
    /// [`Movie::seek_to_start`] refuses a machine of another region.
    pub region: Region,
    /// [`Nes::rom_sha256`] of the ROM the movie was recorded against.
    pub rom_sha256: [u8; 32],
    /// v3.0.0 (ADR 0045) — the [`EMULATION_EPOCH`](crate::EMULATION_EPOCH)
    /// the movie was recorded under. [`Movie::new`] and every recorder and
    /// importer stamp the current one; [`Movie::deserialize`] refuses
    /// another.
    pub epoch: u32,
    /// v2.9.8 — every emulation-affecting host option the movie was recorded
    /// with. [`Movie::seek_to_start`] applies them before frame 0, so the
    /// replay does not depend on the player's settings. A foreign import
    /// records [`HardwareOptions::default`], the stock NES.
    pub options: HardwareOptions,
    /// v2.9.8 — the cartridge board the recording machine was built from.
    /// `None` for a foreign import (its format records no header), in which
    /// case only the ROM identity and the region are checked.
    pub board: Option<BoardDescription>,
    /// Where playback begins.
    pub start: StartPoint,
    /// Per-frame controller inputs, in playback order.
    pub frames: Vec<FrameInput>,
    /// TAS re-record count — how many times the author re-recorded a frame
    /// (the TAS piano-roll editor's edit tally; 0 for a straight linear
    /// recording). Round-trips through `.rnm` (appended after the input stream,
    /// so older readers ignore it) and the `.fm2` / `.bk2` `rerecordCount` header.
    pub rerecord_count: u32,
    /// Optional replay attestation (v2.3.2 "Lucid"): a rolling hash of the run's
    /// video output plus periodic checkpoints, letting a third party replay the
    /// movie and prove it reproduces the recorded run.
    ///
    /// `None` for every movie recorded without one, which is most of them.
    /// Appended after [`Self::rerecord_count`] behind [`ATTESTATION_MAGIC`], so
    /// an older reader stops at the re-record count and never sees it — the same
    /// additive-tail trick that field itself used, and the reason no container
    /// version bump was needed.
    pub attestation: Option<Attestation>,
}

impl Movie {
    /// v3.0.0 — a movie at the current [`EMULATION_EPOCH`](crate::EMULATION_EPOCH)
    /// with no re-records and no attestation. Set
    /// [`Self::rerecord_count`] or [`Self::attestation`] afterwards when they
    /// apply; the struct is `#[non_exhaustive]`, so this is how code outside
    /// `rustynes-core` builds one.
    #[must_use]
    pub const fn new(
        region: Region,
        rom_sha256: [u8; 32],
        options: HardwareOptions,
        board: Option<BoardDescription>,
        start: StartPoint,
        frames: Vec<FrameInput>,
    ) -> Self {
        Self {
            region,
            rom_sha256,
            epoch: crate::EMULATION_EPOCH,
            options,
            board,
            start,
            frames,
            rerecord_count: 0,
            attestation: None,
        }
    }

    /// Number of input frames in the movie.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.frames.len()
    }

    /// `true` if the movie has no input frames.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Serialize the movie to its `.rnm` byte representation.
    ///
    /// Deterministic: the same `Movie` always produces identical bytes.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let frame_count = u32::try_from(self.frames.len()).expect("frame count exceeds u32");
        let body_hint = self.frames.len() * usize::from(BYTES_PER_FRAME);
        let mut w = BinWriter::with_capacity(48 + body_hint);
        w.bytes(MOVIE_MAGIC);
        w.u16(MOVIE_FORMAT_VERSION);
        // v3.0.0 (format 5, ADR 0045): the emulation epoch, beside the
        // version so a tool can read both without parsing further.
        w.u32(self.epoch);
        w.u8(region_to_byte(self.region));
        let mut flags = match &self.start {
            StartPoint::PowerOn => 0,
            StartPoint::SaveState(_) => FLAG_HAS_SAVE_STATE,
        };
        if self.board.is_some() {
            flags |= FLAG_HAS_BOARD;
        }
        w.u8(flags);
        w.bytes(&self.rom_sha256);
        w.u32(frame_count);
        w.u8(BYTES_PER_FRAME);
        // v2.9.8 (format 3) — the OPTIONS block, length-prefixed so the
        // decoder can bound it and confirm it consumed exactly what was
        // written.
        let mut opts = BinWriter::with_capacity(48);
        self.options.write_to(&mut opts);
        if let Some(board) = &self.board {
            board.write_to(&mut opts);
        }
        w.lp_bytes(&opts.into_vec());
        if let StartPoint::SaveState(blob) = &self.start {
            w.lp_bytes(blob);
        }
        for f in &self.frames {
            w.u8(f.p1.bits());
            w.u8(f.p2.bits());
            w.u8(f.p3.bits());
            w.u8(f.p4.bits());
            w.u8(f.expansion);
        }
        // Trailing re-record count (v1.8.9). Appended AFTER the fixed-count input
        // stream so a reader that stops at `frame_count` records — including older
        // builds — simply ignores it; deserialize below reads it when present and
        // defaults to 0 otherwise. No format-version bump needed.
        w.u32(self.rerecord_count);
        // Optional attestation tail (v2.3.2 "Lucid"). Written only when present,
        // so a movie without one is byte-for-byte what previous versions wrote.
        if let Some(att) = &self.attestation {
            w.u32(ATTESTATION_MAGIC);
            w.u16(ATTESTATION_VERSION);
            w.u32(att.frame_count);
            w.u64(att.final_hash);
            w.u32(u32::try_from(att.checkpoints.len()).unwrap_or(u32::MAX));
            for &c in &att.checkpoints {
                w.u64(c);
            }
        }
        w.into_vec()
    }

    /// Parse a `.rnm` movie from its byte representation.
    ///
    /// # Errors
    ///
    /// Returns [`MovieError`] for a bad magic, an unsupported container
    /// version, an unknown region byte, a frame width this build can't
    /// parse, or a truncated body. Never panics on malformed input.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, MovieError> {
        // Fixed header: magic(8) + version(2) + epoch(4, format 5+) +
        // region(1) + flags(1) + sha256(32) + frame_count(4) +
        // bytes_per_frame(1) = 53 bytes.
        const HEADER_LEN: usize = 8 + 2 + 4 + 1 + 1 + 32 + 4 + 1;
        if bytes.len() < HEADER_LEN {
            return Err(MovieError::HeaderTruncated {
                expected: HEADER_LEN,
                got: bytes.len(),
            });
        }
        let mut r = BinReader::new(bytes);
        // Magic.
        let mut magic = [0u8; 8];
        r.read_into(&mut magic).map_err(map_eof)?;
        if &magic != MOVIE_MAGIC {
            return Err(MovieError::BadMagic { got: magic });
        }
        // Version.
        let format_version = r.u16().map_err(map_eof)?;
        if format_version > MOVIE_FORMAT_VERSION {
            return Err(MovieError::UnsupportedFormat {
                got: format_version,
                max: MOVIE_FORMAT_VERSION,
            });
        }
        if format_version < MIN_MOVIE_FORMAT_VERSION {
            return Err(MovieError::FormatTooOld {
                got: format_version,
                min: MIN_MOVIE_FORMAT_VERSION,
            });
        }
        // v3.0.0 (format 5, ADR 0045): the emulation epoch. Checked before
        // anything else is parsed: a movie from a core that emulates
        // differently is refused whatever the rest of it holds.
        let epoch = r.u32().map_err(map_eof)?;
        if epoch != crate::EMULATION_EPOCH {
            return Err(MovieError::EpochMismatch {
                movie: epoch,
                core: crate::EMULATION_EPOCH,
            });
        }
        // Region + flags.
        let region = region_from_byte(r.u8().map_err(map_eof)?)?;
        let flags = r.u8().map_err(map_eof)?;
        if flags & !KNOWN_FLAGS != 0 {
            return Err(MovieError::UnknownFlags(flags));
        }
        // ROM hash.
        let mut rom_sha256 = [0u8; 32];
        r.read_into(&mut rom_sha256).map_err(map_eof)?;
        // Frame count + width.
        let frame_count = r.u32().map_err(map_eof)? as usize;
        let bytes_per_frame = r.u8().map_err(map_eof)?;
        if bytes_per_frame == 0 || bytes_per_frame > BYTES_PER_FRAME {
            // A newer movie packs more device bytes than we understand; we
            // fail cleanly rather than mis-parse (the reserved byte exists
            // precisely so this stays a graceful error, not a corruption).
            //
            // SECURITY: a `bytes_per_frame` of 0 is likewise rejected. With a
            // zero-width record each frame read (`r.take(0)`) consumes no input,
            // so the `for _ in 0..frame_count` loop below would push
            // `frame_count` (an untrusted u32, up to ~4.3 billion) empty frames
            // out of a finite file — an OOM DoS (found by the `movie` fuzz
            // target). A real movie always writes the fixed `BYTES_PER_FRAME`
            // (>= 1), so rejecting 0 costs no legitimate file.
            return Err(MovieError::UnsupportedFrameWidth {
                got: bytes_per_frame,
                max: BYTES_PER_FRAME,
            });
        }
        // v2.9.8 — the OPTIONS block. Decoded from its own bounded slice, and
        // required to be consumed exactly: trailing bytes would mean a later
        // format added a field this build would otherwise silently ignore.
        let block = r.lp_bytes().map_err(map_eof)?;
        let mut br = BinReader::new(block);
        let options = HardwareOptions::read_from(&mut br).map_err(MovieError::BadOptions)?;
        let board = if flags & FLAG_HAS_BOARD != 0 {
            Some(BoardDescription::read_from(&mut br).map_err(MovieError::BadOptions)?)
        } else {
            None
        };
        if br.remaining() != 0 {
            return Err(MovieError::BadOptions("unexpected bytes after the options"));
        }
        // Start point.
        let start = if flags & FLAG_HAS_SAVE_STATE != 0 {
            let blob = r.lp_bytes().map_err(map_eof)?;
            StartPoint::SaveState(blob.to_vec())
        } else {
            StartPoint::PowerOn
        };
        // Input stream: `frame_count` records of `bytes_per_frame` bytes
        // (`width >= 1`, enforced above).
        let width = usize::from(bytes_per_frame);
        // SECURITY: `frame_count` is an untrusted 4-byte field (up to ~4.3
        // billion). Pre-sizing `Vec::with_capacity(frame_count)` from it lets a
        // 53-byte header claim a multi-gigabyte allocation — an OOM DoS (found
        // by the `movie` fuzz target). A real movie carries exactly
        // `frame_count * width` more bytes, so cap the reservation at what the
        // remaining input could actually hold: for a valid file this equals
        // `frame_count` (identical allocation, byte-for-byte the same result),
        // and for a truncated / hostile one the `r.take(width)` below still
        // fails cleanly with an EOF error once the real bytes run out.
        let max_plausible_frames = r.remaining() / width;
        let mut frames = Vec::with_capacity(frame_count.min(max_plausible_frames));
        for _ in 0..frame_count {
            let rec = r.take(width).map_err(map_eof)?;
            // Format 3: rec = [p1, p2, p3, p4, expansion]. A narrower record
            // defaults the fields it does not reach (released pads, no
            // expansion byte).
            let pad = |i: usize| Buttons::from_bits_truncate(rec.get(i).copied().unwrap_or(0));
            let mut frame = FrameInput::four_players(pad(0), pad(1), pad(2), pad(3));
            frame.expansion = rec.get(4).copied().unwrap_or(0);
            frames.push(frame);
        }
        // Optional trailing re-record count (v1.8.9). Absent in pre-v1.8.9 `.rnm`
        // files, which stop exactly at the input stream — default to 0.
        let rerecord_count = r.u32().unwrap_or(0);
        // Optional attestation tail (v2.3.2 "Lucid"). Absent in every movie
        // recorded before it existed, and in any recorded without it — so a
        // missing or unrecognized marker yields `None` rather than an error.
        let attestation = read_attestation(&mut r, frames.len());
        Ok(Self {
            region,
            rom_sha256,
            epoch,
            options,
            board,
            start,
            frames,
            rerecord_count,
            attestation,
        })
    }

    /// Rewind a running emulator to this movie's start point, ready to replay
    /// from frame 0.
    ///
    /// Checks first, then changes the machine: the ROM identity, the region
    /// and (for a recorded movie) the [`BoardDescription`] must match, or the
    /// call fails with `nes` untouched. Then it applies the movie's
    /// [`HardwareOptions`] -- v2.9.8: the recorded console model, die
    /// revisions, power-on fills, overclock, Four Score, Vs. settings,
    /// mirroring override and Game Genie codes replace the player's -- and
    /// moves to the start point: for [`StartPoint::PowerOn`] a power cycle
    /// with cleared cartridge RAM ([`power_on_for_movie`]), for
    /// [`StartPoint::SaveState`] the embedded snapshot.
    ///
    /// A start refused after the checks -- an old or malformed start state,
    /// an undecodable code -- also leaves `nes` as it was: the call takes a
    /// rollback point first and restores it on any error.
    ///
    /// After a successful start the player's options are not restored here;
    /// a host that wants them back captures them first ([`HardwareOptions::capture`]) and calls
    /// [`HardwareOptions::restore_after_playback`] when playback ends.
    ///
    /// # Errors
    ///
    /// [`MovieError::EpochMismatch`] for a movie recorded under another
    /// emulation epoch (ADR 0045); [`MovieError::RomMismatch`],
    /// [`MovieError::RegionMismatch`] or [`MovieError::BoardMismatch`] for a
    /// different machine;
    /// [`MovieError::OptionNotApplicable`] for a Game Genie code that does
    /// not decode; [`MovieError::BadSaveState`] if the embedded snapshot is
    /// malformed.
    pub fn seek_to_start(&self, nes: &mut Nes) -> Result<(), MovieError> {
        self.check_epoch()?;
        if nes.rom_sha256() != &self.rom_sha256 {
            return Err(MovieError::RomMismatch);
        }
        if nes.region() != self.region {
            return Err(MovieError::RegionMismatch {
                movie: self.region,
                host: nes.region(),
            });
        }
        if let Some(board) = &self.board
            && let Some(field) = board.first_difference(&BoardDescription::capture(nes))
        {
            return Err(MovieError::BoardMismatch { field });
        }
        // A refused start must leave the player's game as it was. The options
        // are applied before the start point is reached, and `apply` rewrites
        // work RAM and palette RAM, so take a rollback point first. A
        // snapshot per movie start costs one serialisation; seeking is not a
        // per-frame call.
        let prior_options = HardwareOptions::capture(nes);
        let prior_state = nes.snapshot();
        let result = self.enter_start(nes);
        if result.is_err() {
            // This order: the player's `apply` rewrites the fills, then the
            // restore puts the running game's RAM and palette back over them.
            // Neither can fail: the codes decoded when they were captured,
            // and the blob is this machine's own snapshot.
            let options_back = prior_options.apply(nes);
            let state_back = nes.restore_quiet(&prior_state);
            debug_assert!(options_back.is_ok() && state_back.is_ok());
        }
        result
    }

    /// v3.0.0 (ADR 0045): refuse a movie recorded under another emulation
    /// epoch. [`Self::deserialize`] checks the field it parses; this checks
    /// the field as it stands, because `epoch` is public and a [`Movie`] can
    /// be built or edited in memory, and playback must not trust that it came
    /// through `deserialize` (a review finding on #588).
    const fn check_epoch(&self) -> Result<(), MovieError> {
        if self.epoch != crate::EMULATION_EPOCH {
            return Err(MovieError::EpochMismatch {
                movie: self.epoch,
                core: crate::EMULATION_EPOCH,
            });
        }
        Ok(())
    }

    /// The mutating half of [`Self::seek_to_start`], after the identity
    /// checks; its caller rolls the machine back if this fails.
    fn enter_start(&self, nes: &mut Nes) -> Result<(), MovieError> {
        // Applied BEFORE the start point is reached. For a power-on start the
        // power cycle must see the movie's stored power-on knobs (fills,
        // console model, die revision); for a save-state start the restore
        // then replaces whatever RAM / palette the fills wrote.
        self.options
            .apply(nes)
            .map_err(MovieError::OptionNotApplicable)?;
        match &self.start {
            StartPoint::PowerOn => power_on_for_movie(nes),
            StartPoint::SaveState(blob) => {
                nes.restore(blob).map_err(|e| match e {
                    SnapshotError::FormatTooOld { got, min } => {
                        MovieError::StartStateTooOld { got, min }
                    }
                    other => MovieError::BadSaveState(other),
                })?;
                // The options are configuration, not save-state, so a restore
                // leaves them alone; re-asserting the live ones is cheap and
                // keeps that a checked fact rather than an assumption.
                self.options
                    .apply_live(nes)
                    .map_err(MovieError::OptionNotApplicable)?;
            }
        }
        Ok(())
    }

    /// v2.3.2 "Lucid" — replay this movie and check it reproduces its
    /// attestation.
    ///
    /// Seeks `nes` to the movie's start point, replays the whole input stream,
    /// and compares the resulting rolling hash (and every checkpoint along the
    /// way) against what the movie recorded. Anyone with the ROM and the `.rnm`
    /// can run it and get the same answer, so an accidental divergence — a
    /// different build, a nondeterminism bug, a corrupted file — or a casual
    /// edit to the input stream, the start point, or the claimed hash shows up
    /// as a [`VerifyOutcome::Mismatch`].
    ///
    /// **Reproducibility, not provenance.** The digest is a 64-bit FNV-1a
    /// variant: tamper-evident, not forgery-resistant. A `Match` means "these inputs,
    /// applied to this ROM, on a verifier configured like the recorder, produce
    /// this video". It does not establish who produced the movie, and a
    /// motivated forger can edit the movie and recompute the digest.
    ///
    /// Consumes real emulation time — it runs every frame of the movie.
    ///
    /// # Errors
    ///
    /// [`MovieError::EpochMismatch`] for a movie from another emulation epoch
    /// (checked first, attested or not), [`MovieError::RomMismatch`] if `nes`
    /// is running a different ROM, or [`MovieError::BadSaveState`] if an
    /// embedded start point is malformed.
    /// A movie with no attestation is **not** an error; it returns
    /// [`VerifyOutcome::NotAttested`], because "this movie makes no claim" and
    /// "this movie makes a false claim" are different answers.
    pub fn verify(&self, nes: &mut Nes) -> Result<VerifyOutcome, MovieError> {
        // Before the attestation question: a movie from another epoch cannot
        // be replayed here at all, which is a stronger answer than "makes no
        // claim".
        self.check_epoch()?;
        let Some(att) = self.attestation.as_ref() else {
            return Ok(VerifyOutcome::NotAttested);
        };
        self.seek_to_start(nes)?;
        let mut builder = AttestationBuilder::new();
        let mut first_bad_checkpoint = None;
        let mut player = MoviePlayer::new(self);
        let mut idx = 0usize;
        while player.apply_next(nes) {
            let input = self.frames.get(idx).copied().unwrap_or_default();
            idx += 1;
            let fb = nes.run_frame();
            builder.push_frame(input, fb);
            // Compare each checkpoint as it is produced rather than collecting
            // and diffing afterwards: the first disagreement is the useful one,
            // and it localizes the divergence to a 64-frame window.
            if builder
                .frame_count()
                .is_multiple_of(ATTESTATION_CHECKPOINT_INTERVAL)
                && first_bad_checkpoint.is_none()
            {
                let idx = builder.frame_count() / ATTESTATION_CHECKPOINT_INTERVAL - 1;
                // Compare ONLY against a checkpoint the movie actually recorded.
                // `get()` returning `None` means "no recorded value here", not
                // "mismatch": treating absence as disagreement made a short
                // checkpoint list report a divergence even when every hash the
                // movie does carry — including the final one — matched. The
                // final hash is the gate; the checkpoints only localize.
                if let Some(&want) = att.checkpoints.get(idx as usize)
                    && want != builder.current_hash()
                {
                    first_bad_checkpoint = Some(idx);
                }
            }
        }
        let got = builder.current_hash();
        let frames = builder.frame_count();
        if got == att.final_hash && first_bad_checkpoint.is_none() {
            Ok(VerifyOutcome::Match { frames, hash: got })
        } else {
            Ok(VerifyOutcome::Mismatch {
                frames,
                expected: att.final_hash,
                got,
                first_bad_checkpoint,
            })
        }
    }
}

/// Records the per-frame input stream applied to an emulator.
///
/// Usage (caller-driven, mirrors the frontend's per-frame loop):
///
/// ```ignore
/// let mut rec = MovieRecorder::power_on(&nes);
/// loop {
///     nes.set_buttons(0, p1);
///     nes.set_buttons(1, p2);
///     rec.capture(&nes); // BEFORE run_frame — captures the inputs it consumes
///     nes.run_frame();
/// }
/// let movie = rec.finish();
/// ```
#[derive(Clone, Debug)]
pub struct MovieRecorder {
    region: Region,
    rom_sha256: [u8; 32],
    options: HardwareOptions,
    board: BoardDescription,
    start: StartPoint,
    frames: Vec<FrameInput>,
    /// v2.3.2 "Lucid" — optional attestation accumulator. `None` (the default)
    /// records a plain movie, byte-for-byte what previous versions produced.
    attestation: Option<AttestationBuilder>,
}

/// v2.9.0 — the state every [`StartPoint::PowerOn`] movie starts from.
///
/// A power cycle, then cartridge RAM zeroed: what a fresh load with no save
/// file holds (every board allocates it zeroed).
///
/// # Why this is not just `power_cycle`
///
/// From v2.9.0 [`Nes::power_cycle`] KEEPS battery-backed RAM, as a console
/// does. Before, it rebuilt the mapper with all cartridge RAM cleared, which
/// made a Power Cycle erase the player's `.sav` once the desktop began
/// persisting saves (v2.7.3). A power-on movie still has to start from clean
/// save RAM -- the maintainer's decision of 2026-09-26, `TASVideos`'
/// convention -- or it would replay differently with and without a save. So
/// hosts call this before [`MovieRecorder::power_on`], and
/// [`Movie::seek_to_start`] calls it on playback.
///
/// Only the emulator's copy is cleared. The `.sav` file on disk is untouched,
/// but a host that persists save RAM will write the cleared contents back if
/// the movie session runs long enough to reach the game's own save routine --
/// the same as that routine running during the movie. FDS disk sides are not
/// cartridge RAM and are not reset by this.
///
/// # The options survive the power cycle (v2.9.8)
///
/// Before v2.9.8 [`Nes::power_cycle`] rebuilt the PPU and dropped the
/// PPU-held knobs (OAM decay, the overclock, the fast dot path) to their
/// defaults, so a recording started with OAM decay off whatever the player
/// had set, and nothing recorded that. The options are captured first and the
/// live ones re-applied after the cycle, so the machine a movie starts on is
/// the one its [`HardwareOptions`] describe, on the recording side and the
/// playback side alike. Since v2.9.8 the cycle keeps every setting itself
/// (the PPU and APU ones as well as the bus-held console model, die
/// revisions, power-on fills, Four Score, Game Genie ...), so the
/// re-application is a guarantee rather than a repair: it holds whatever a
/// future cycle might drop. The power-on fills are not re-applied: the cycle
/// itself already filled RAM and palette RAM from the stored selection.
pub fn power_on_for_movie(nes: &mut Nes) {
    let options = HardwareOptions::capture(nes);
    nes.power_cycle();
    // Not `sram_mut().fill(0)`: on a flash board (v2.9.6) the save is the PRG
    // image, and a never-saved flash is the ROM as loaded, not zeros.
    nes.clear_save_data();
    // Codes captured from this same machine always decode again.
    let reapplied = options.apply_live(nes);
    debug_assert!(reapplied.is_ok(), "captured options re-apply");
    // The fast dot path is not an option (it selects a code path, not a
    // behaviour) and the cycle keeps it since v2.9.8, so starting a movie does
    // not change which PPU path the player runs. Until v2.9.8 it was captured
    // and re-set here by hand.
}

impl MovieRecorder {
    /// Begin recording a movie that starts from a fresh power-on of the ROM
    /// `nes` is running. The caller is responsible for calling
    /// [`power_on_for_movie`] on `nes` before the first captured frame so the
    /// recording starts from the same state a replay will reconstruct.
    ///
    /// v2.9.8: the machine's [`HardwareOptions`] and [`BoardDescription`] are
    /// captured here and written into the movie, so a host must set its
    /// options before this call, not after.
    #[must_use]
    pub fn power_on(nes: &Nes) -> Self {
        Self {
            region: nes.region(),
            rom_sha256: *nes.rom_sha256(),
            options: HardwareOptions::capture(nes),
            board: BoardDescription::capture(nes),
            start: StartPoint::PowerOn,
            frames: Vec::new(),
            attestation: None,
        }
    }

    /// Begin recording a movie that starts from `nes`'s *current* state (a
    /// branch point). Captures a snapshot now and embeds it as the start
    /// point; the input stream is recorded from here forward.
    #[must_use]
    pub fn from_current_state(nes: &Nes) -> Self {
        Self {
            region: nes.region(),
            rom_sha256: *nes.rom_sha256(),
            options: HardwareOptions::capture(nes),
            board: BoardDescription::capture(nes),
            start: StartPoint::SaveState(nes.snapshot()),
            frames: Vec::new(),
            attestation: None,
        }
    }

    /// Record the controller inputs currently held on `nes`. Call this each
    /// frame *before* [`Nes::run_frame`], after the frontend has applied its
    /// `set_buttons` calls — this captures exactly the inputs the upcoming
    /// frame consumes.
    ///
    /// # All four ports (v2.9.8)
    ///
    /// Reads `nes.buttons(0..=3)`. Until v2.9.8 [`FrameInput`] modelled ports
    /// 0 and 1 only, so a four-player session recorded half of what drove it
    /// and diverged on playback; the `.rnm` format 3 epoch widened the record
    /// and the struct together. Inputs that are not controller buttons --
    /// expansion devices (Zapper, Vaus, keyboards ...), the Famicom
    /// microphone, Vs. coins / service, FDS disk swaps -- are still not
    /// recorded; see `docs/frontend.md` § "What a movie records".
    pub fn capture(&mut self, nes: &Nes) {
        self.frames.push(FrameInput::held_on(nes));
    }

    /// Record an explicit frame of input (for callers that drive input
    /// programmatically rather than through `set_buttons`).
    pub fn capture_input(&mut self, input: FrameInput) {
        self.frames.push(input);
    }

    /// Number of frames captured so far.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.frames.len()
    }

    /// `true` if no frames have been captured.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// v2.9.8 — the options this recording was started with, which the movie
    /// will carry. A host holds them in place for the length of the recording
    /// ([`HardwareOptions::apply_live`] each frame), because the movie records
    /// them once, at the start.
    #[must_use]
    pub const fn options(&self) -> &HardwareOptions {
        &self.options
    }

    /// v2.3.2 "Lucid" — start accumulating a replay attestation.
    ///
    /// Call before the first frame. The caller must then call
    /// [`Self::attest_frame`] after every `run_frame`, in lockstep with
    /// [`Self::capture`], or the recorded hash will describe a different run
    /// than the input stream does — which [`Movie::verify`] would then report as
    /// a mismatch, correctly but unhelpfully.
    pub fn enable_attestation(&mut self) {
        self.attestation = Some(AttestationBuilder::new());
    }

    /// Abandon an in-progress attestation, keeping the recording itself.
    ///
    /// For a host that rewinds or otherwise moves the emulator off the timeline
    /// the accumulated hash describes. Once dropped it is not resumed: the
    /// prefix already folded in cannot be un-folded, and a partial hash that
    /// silently covers only part of the run would be worse than none.
    pub fn disable_attestation(&mut self) {
        self.attestation = None;
    }

    /// Fold this frame's video output into the attestation.
    ///
    /// A no-op unless [`Self::enable_attestation`] was called. Pass the slice
    /// `Nes::run_frame` returned (or `Nes::framebuffer()`), AFTER the frame ran.
    ///
    /// Uses the input recorded by the matching [`Self::capture`], so the two
    /// must stay in lockstep — one `capture` then one `attest_frame` per frame.
    /// If they drift the recorded frame counts disagree and `Movie::deserialize`
    /// drops the tail, which is the safe direction.
    pub fn attest_frame(&mut self, framebuffer: &[u8]) {
        // The input for THIS frame is the one `capture` just pushed.
        let input = self.frames.last().copied().unwrap_or_default();
        if let Some(a) = self.attestation.as_mut() {
            a.push_frame(input, framebuffer);
        }
    }

    /// Finish recording and produce the [`Movie`].
    #[must_use]
    pub fn finish(self) -> Movie {
        Movie {
            region: self.region,
            rom_sha256: self.rom_sha256,
            epoch: crate::EMULATION_EPOCH,
            options: self.options,
            board: Some(self.board),
            start: self.start,
            frames: self.frames,
            // A linear recording has no re-records by construction; TAStudio
            // sets a real count when it exports an edited movie.
            rerecord_count: 0,
            attestation: self.attestation.map(AttestationBuilder::finish),
        }
    }
}

/// Plays a movie back, feeding its recorded inputs into an emulator one frame
/// at a time.
///
/// Usage (caller-driven; the player applies `set_buttons`, the caller runs
/// the frame):
///
/// ```ignore
/// movie.seek_to_start(&mut nes)?;
/// let mut player = MoviePlayer::new(&movie);
/// while player.apply_next(&mut nes) {
///     nes.run_frame();
/// }
/// ```
#[derive(Clone, Debug)]
pub struct MoviePlayer<'a> {
    movie: &'a Movie,
    cursor: usize,
}

impl<'a> MoviePlayer<'a> {
    /// Create a player positioned at frame 0 of `movie`.
    #[must_use]
    pub const fn new(movie: &'a Movie) -> Self {
        Self { movie, cursor: 0 }
    }

    /// Total frames in the movie.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.movie.frames.len()
    }

    /// `true` if the movie has no frames.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.movie.frames.is_empty()
    }

    /// Index of the frame that [`Self::apply_next`] will apply next.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// `true` if every frame has been played.
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.cursor >= self.movie.frames.len()
    }

    /// Peek the next frame's input without advancing.
    #[must_use]
    pub fn peek(&self) -> Option<FrameInput> {
        self.movie.frames.get(self.cursor).copied()
    }

    /// Apply the next frame's recorded input to `nes` via `set_buttons` and
    /// advance the cursor. Returns `false` (without applying anything) once
    /// the movie is exhausted — the caller stops its replay loop on `false`.
    ///
    /// Call this *before* [`Nes::run_frame`], mirroring the record-side
    /// `capture` ordering, so the same inputs are applied to the same frame.
    pub fn apply_next(&mut self, nes: &mut Nes) -> bool {
        let Some(input) = self.movie.frames.get(self.cursor).copied() else {
            return false;
        };
        input.apply_to(nes);
        self.cursor += 1;
        true
    }

    /// Reset the cursor back to frame 0 (the caller is responsible for
    /// re-seeking `nes` via [`Movie::seek_to_start`]).
    pub const fn rewind(&mut self) {
        self.cursor = 0;
    }
}

const fn region_to_byte(region: Region) -> u8 {
    match region {
        Region::Ntsc => 0,
        Region::Pal => 1,
        Region::Dendy => 2,
    }
}

const fn region_from_byte(b: u8) -> Result<Region, MovieError> {
    match b {
        0 => Ok(Region::Ntsc),
        1 => Ok(Region::Pal),
        2 => Ok(Region::Dendy),
        other => Err(MovieError::BadRegion(other)),
    }
}

/// Map a `SnapshotError::Eof`-style truncation reading the movie body into a
/// movie-level [`MovieError::Eof`]. Other snapshot errors cannot arise from
/// the `BinReader` calls in this module (they only read fixed primitives).
fn map_eof(e: SnapshotError) -> MovieError {
    match e {
        SnapshotError::Eof(off) => MovieError::Eof(off),
        other => MovieError::BadSaveState(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nes::PowerOnRam;
    use alloc::vec;

    // ----------------------------------------------------------------- v2.3.2
    // Replay attestation ("Lucid" phase 4)
    // -------------------------------------------------------------------------

    /// Record a short attested run and prove it replays to the same hash.
    ///
    /// This is the whole feature in one test: the claim is not "the hash is
    /// stable" but "an independent replay re-derives it", which is what makes
    /// the record evidence rather than decoration.
    #[test]
    fn attested_movie_verifies_against_an_independent_replay() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..8 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let movie = rec.finish();
        let att = movie.attestation.as_ref().expect("attestation recorded");
        assert_eq!(att.frame_count, 8);

        // A SEPARATE emulator instance, as a third party would use.
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        match movie.verify(&mut fresh).expect("verify runs") {
            VerifyOutcome::Match { frames, hash } => {
                assert_eq!(frames, 8);
                assert_eq!(hash, att.final_hash);
            }
            other => panic!("expected a match, got {other:?}"),
        }
    }

    /// The attestation survives the `.rnm` round-trip, and a movie WITHOUT one
    /// still serializes to exactly the bytes it did before this feature existed.
    #[test]
    fn attestation_round_trips_and_is_absent_when_not_recorded() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");

        // Plain recording: no attestation, and the tail is not written.
        let mut plain = MovieRecorder::power_on(&nes);
        plain.capture(&nes);
        let _ = nes.run_frame();
        let plain = plain.finish();
        assert!(plain.attestation.is_none());
        let plain_bytes = plain.serialize();
        let reparsed = Movie::deserialize(&plain_bytes).expect("round-trip");
        assert_eq!(reparsed, plain);
        assert!(reparsed.attestation.is_none());

        // Attested recording: the tail round-trips intact.
        let mut nes2 = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes2);
        rec.enable_attestation();
        // Enough frames to cross a checkpoint boundary.
        for _ in 0..(ATTESTATION_CHECKPOINT_INTERVAL + 3) {
            rec.capture(&nes2);
            let fb = nes2.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let attested = rec.finish();
        assert_eq!(attested.attestation.as_ref().unwrap().checkpoints.len(), 1);
        let bytes = attested.serialize();
        assert_eq!(Movie::deserialize(&bytes).expect("round-trip"), attested);

        // The attested file is strictly longer, and the plain one is unchanged
        // by this feature existing.
        assert!(bytes.len() > plain_bytes.len());
    }

    /// A reader that stops at the re-record count — i.e. every build before this
    /// feature — must still parse an attested movie as a plain one. Simulated by
    /// truncating the tail, which is exactly what such a reader sees.
    #[test]
    fn attested_movie_stays_readable_as_a_plain_movie() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..4 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let attested = rec.finish();
        let full = attested.serialize();

        // Everything an older reader consumes: header + inputs + rerecord count.
        // The attestation tail is 4 + 2 + 4 + 8 + 4 = 22 bytes plus checkpoints
        // (none here, 4 frames < the interval).
        let tail_len = 4 + 2 + 4 + 8 + 4;
        let older_view = &full[..full.len() - tail_len];
        let parsed = Movie::deserialize(older_view).expect("older readers still parse it");
        assert_eq!(parsed.frames, attested.frames);
        assert_eq!(parsed.rerecord_count, attested.rerecord_count);
        assert!(parsed.attestation.is_none());
    }

    /// Tampering with the input stream must be detected. This is the property
    /// that makes an attestation worth anything.
    #[test]
    fn tampering_with_the_input_stream_fails_verification() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..6 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let mut movie = rec.finish();

        // Forge the claimed hash. A replay must refuse to confirm it.
        let real = movie.attestation.as_ref().unwrap().final_hash;
        movie.attestation.as_mut().unwrap().final_hash = real ^ 1;
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        match movie.verify(&mut fresh).expect("verify runs") {
            VerifyOutcome::Mismatch { expected, got, .. } => {
                assert_eq!(expected, real ^ 1);
                assert_eq!(got, real, "the replay re-derives the TRUE hash");
            }
            other => panic!("a forged hash must not verify, got {other:?}"),
        }
    }

    /// A flipped INPUT bit must fail verification even when the ROM ignores
    /// input entirely and the video output is therefore unchanged.
    ///
    /// This is the case an output-only hash gets wrong, and it is not
    /// hypothetical: an end-to-end `rustynes verify` run against a test ROM that
    /// never reads the controller happily confirmed a movie whose input log had
    /// been edited. The fix was to fold the per-frame input into the hash; this
    /// test is what keeps it folded in.
    #[test]
    fn flipped_input_fails_even_when_the_rom_ignores_input() {
        // `synth_nrom` is an infinite `JMP` — it never reads $4016, so its video
        // output is identical for every possible input stream.
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..6 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let mut movie = rec.finish();

        // Sanity: the honest movie verifies.
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        assert!(matches!(
            movie.verify(&mut fresh).expect("verify runs"),
            VerifyOutcome::Match { .. }
        ));

        // Now edit the input log. The video output will be bit-identical,
        // because this ROM never looks at the controller.
        movie.frames[3].p1 = Buttons::A;
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        match movie.verify(&mut fresh).expect("verify runs") {
            VerifyOutcome::Mismatch { .. } => {}
            other => panic!("an edited input log must not verify, got {other:?}"),
        }
    }

    /// An attestation whose frame count disagrees with the input stream
    /// describes a different run, so it is dropped rather than compared against
    /// the wrong length.
    #[test]
    fn attestation_with_a_mismatched_frame_count_is_rejected_on_load() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..3 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let mut movie = rec.finish();
        movie.attestation.as_mut().unwrap().frame_count = 999;
        let bytes = movie.serialize();
        let parsed = Movie::deserialize(&bytes).expect("the movie itself is fine");
        assert_eq!(parsed.frames.len(), 3);
        assert!(
            parsed.attestation.is_none(),
            "a tail describing a different run must be dropped, not trusted"
        );
    }

    /// Drive `first_bad_checkpoint` to `Some(_)` — the path bug #12 lived on,
    /// which no test previously exercised.
    #[test]
    fn a_corrupted_checkpoint_is_localized() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        // Three checkpoint windows.
        for _ in 0..(ATTESTATION_CHECKPOINT_INTERVAL * 3) {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let mut movie = rec.finish();
        assert_eq!(movie.attestation.as_ref().unwrap().checkpoints.len(), 3);

        // Corrupt the SECOND checkpoint only. The final hash still matches, so
        // the checkpoint comparison is the only thing that can catch this.
        movie.attestation.as_mut().unwrap().checkpoints[1] ^= 0xFF;
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        match movie.verify(&mut fresh).expect("verify runs") {
            VerifyOutcome::Mismatch {
                first_bad_checkpoint,
                expected,
                got,
                ..
            } => {
                assert_eq!(
                    first_bad_checkpoint,
                    Some(1),
                    "the SECOND checkpoint is the first bad one"
                );
                assert_eq!(expected, got, "the final hash still agrees");
            }
            other => panic!("a corrupted checkpoint must not verify, got {other:?}"),
        }
    }

    /// A short checkpoint list must NOT manufacture a mismatch.
    ///
    /// Regression for bug #12: `checkpoints.get(idx)` returning `None` was
    /// compared against `Some(&hash)` and read as disagreement, so an
    /// attestation carrying fewer checkpoints than the replay produces failed
    /// even when every hash it did record — and the final hash — matched.
    #[test]
    fn a_short_checkpoint_list_still_verifies() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..(ATTESTATION_CHECKPOINT_INTERVAL * 2) {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let mut movie = rec.finish();
        assert_eq!(movie.attestation.as_ref().unwrap().checkpoints.len(), 2);

        // Drop the trailing checkpoint, leaving the final hash intact.
        movie.attestation.as_mut().unwrap().checkpoints.pop();
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        match movie.verify(&mut fresh).expect("verify runs") {
            VerifyOutcome::Match { frames, .. } => {
                assert_eq!(frames, ATTESTATION_CHECKPOINT_INTERVAL * 2);
            }
            other => panic!(
                "a missing checkpoint is 'nothing recorded here', not a \
                 disagreement; got {other:?}"
            ),
        }
    }

    /// A rewind mid-recording must drop the attestation rather than ship one
    /// that describes a timeline the input log no longer encodes.
    #[test]
    fn disabling_attestation_mid_recording_yields_an_unattested_movie() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.enable_attestation();
        for _ in 0..4 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        // What the frontend does on a successful `rewind_step_back`.
        rec.disable_attestation();
        for _ in 0..4 {
            rec.capture(&nes);
            let fb = nes.run_frame().to_vec();
            rec.attest_frame(&fb);
        }
        let movie = rec.finish();
        assert!(
            movie.attestation.is_none(),
            "a partial hash covering only part of the run is worse than none"
        );
        assert_eq!(movie.frames.len(), 8, "the recording itself is unaffected");
    }

    /// A movie with no attestation reports that, rather than passing or failing.
    #[test]
    fn unattested_movie_reports_not_attested() {
        let rom = synth_nrom();
        let mut nes = Nes::from_rom(&rom).expect("parse");
        let mut rec = MovieRecorder::power_on(&nes);
        rec.capture(&nes);
        let _ = nes.run_frame();
        let movie = rec.finish();
        let mut fresh = Nes::from_rom(&rom).expect("parse");
        assert_eq!(
            movie.verify(&mut fresh).expect("verify runs"),
            VerifyOutcome::NotAttested
        );
    }

    /// Minimal NROM ROM that runs an infinite loop (same shape as the
    /// `nes.rs` test fixture). Deterministic boot, no input dependence in
    /// the program itself — the movie machinery is what we exercise.
    fn synth_nrom() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NES\x1A");
        bytes.push(1); // 16 KiB PRG
        bytes.push(1); // 8 KiB CHR
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00;
        prg[len - 3] = 0xC0;
        prg[len - 6] = 0x00;
        prg[len - 5] = 0xC0;
        prg[len - 2] = 0x00;
        prg[len - 1] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
        bytes
    }

    fn fnv(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    fn audio_fnv(samples: &[f32]) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for s in samples {
            for &b in &s.to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }

    /// A fixed, varied synthetic input sequence (deterministic, no RNG).
    fn synthetic_inputs(n: usize) -> Vec<FrameInput> {
        (0..n)
            .map(|i| {
                let i = u8::try_from(i % 256).unwrap();
                let p1 = Buttons::from_bits_truncate(i.wrapping_mul(37));
                let p2 = Buttons::from_bits_truncate(i.wrapping_mul(101).rotate_left(3));
                FrameInput::new(p1, p2)
            })
            .collect()
    }

    /// v2.9.0 (maintainer decision 2026-09-26) — a power-on movie starts from
    /// cleared cartridge RAM. `power_cycle` keeps cartridge RAM, as a console
    /// keeps a battery save; since v2.7.3 the desktop also loads a `.sav` into
    /// it at ROM load, so a power-on movie recorded without a save diverged on
    /// a machine that had one. Seeking to a `PowerOn` start must therefore
    /// leave cartridge RAM exactly as a fresh load with no save does: zeroed.
    /// `synth_nrom` with the header's battery bit set. The battery matters:
    /// `power_cycle` keeps battery-backed RAM (v2.9.0) and clears volatile
    /// RAM, so only a battery cart can show whether the MOVIE clears it.
    fn synth_nrom_battery() -> Vec<u8> {
        let mut rom = synth_nrom();
        rom[6] |= 0x02;
        rom
    }

    #[test]
    fn seeking_a_power_on_movie_clears_cartridge_ram() {
        let mut nes = Nes::from_rom(&synth_nrom_battery()).unwrap();
        assert!(nes.has_battery());
        assert!(!nes.sram().is_empty(), "NROM exposes its PRG-RAM");
        nes.sram_mut().fill(0xA5); // a loaded .sav, or a previous session
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: nes.region(),
            rom_sha256: *nes.rom_sha256(),
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: synthetic_inputs(1),
            rerecord_count: 0,
            attestation: None,
        };
        movie.seek_to_start(&mut nes).unwrap();
        assert!(nes.sram().iter().all(|&b| b == 0), "cartridge RAM cleared");
    }

    /// The same rule on the RECORDING side: `power_on_for_movie` is what a
    /// host calls before `MovieRecorder::power_on`, so what is recorded and
    /// what `seek_to_start` reconstructs are the same state.
    #[test]
    fn power_on_for_movie_matches_a_fresh_load() {
        let fresh = Nes::from_rom(&synth_nrom_battery()).unwrap();
        let mut nes = Nes::from_rom(&synth_nrom_battery()).unwrap();
        nes.sram_mut().fill(0x5A);
        nes.power_cycle();
        assert_eq!(nes.sram()[0], 0x5A, "a plain power cycle keeps battery RAM");
        power_on_for_movie(&mut nes);
        assert_eq!(nes.sram(), fresh.sram());
    }

    #[test]
    fn format_round_trip_power_on() {
        let inputs = synthetic_inputs(120);
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0xAB; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: inputs,
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        let back = Movie::deserialize(&bytes).expect("round-trip");
        assert_eq!(movie, back);
    }

    #[test]
    fn rerecord_count_round_trips_and_defaults_for_legacy_rnm() {
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0x5A; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: synthetic_inputs(10),
            rerecord_count: 4242,
            attestation: None,
        };
        let bytes = movie.serialize();
        // A full round-trip preserves the count.
        assert_eq!(Movie::deserialize(&bytes).unwrap().rerecord_count, 4242);
        // A pre-v1.8.9 `.rnm` ends exactly at the input stream (no trailing
        // count). Dropping the appended u32 must still parse, defaulting the
        // count to 0 rather than erroring — the back-compat contract.
        let legacy = &bytes[..bytes.len() - 4];
        let back = Movie::deserialize(legacy).expect("legacy .rnm still parses");
        assert_eq!(back.rerecord_count, 0);
        assert_eq!(back.frames.len(), 10);
    }

    #[test]
    fn format_round_trip_with_save_state_start() {
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Pal,
            rom_sha256: [0x11; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::SaveState(vec![1, 2, 3, 4, 5, 6, 7, 8]),
            frames: synthetic_inputs(8),
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        let back = Movie::deserialize(&bytes).expect("round-trip");
        assert_eq!(movie, back);
    }

    #[test]
    fn deserialize_rejects_bad_magic_cleanly() {
        let mut bytes = vec![0u8; 53];
        bytes[..8].copy_from_slice(b"NOTAMOVI");
        assert!(matches!(
            Movie::deserialize(&bytes),
            Err(MovieError::BadMagic { .. })
        ));
    }

    #[test]
    fn deserialize_rejects_too_new_format_cleanly() {
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: Vec::new(),
            rerecord_count: 0,
            attestation: None,
        };
        let mut bytes = movie.serialize();
        // Bump the format-version field (offset 8) past what we support.
        bytes[8] = 0xFF;
        bytes[9] = 0xFF;
        assert!(matches!(
            Movie::deserialize(&bytes),
            Err(MovieError::UnsupportedFormat { .. })
        ));
    }

    #[test]
    fn deserialize_rejects_truncated_header() {
        assert!(matches!(
            Movie::deserialize(&[0u8; 10]),
            Err(MovieError::HeaderTruncated { .. })
        ));
    }

    #[test]
    fn deserialize_hostile_frame_count_does_not_oom() {
        // frame_count is the 4-byte LE field right after the 32-byte rom hash
        // (offset 8 + 2 + 1 + 1 + 32 = 44).
        // magic + version + epoch (format 5) + region + flags + sha256.
        const FRAME_COUNT_OFF: usize = 8 + 2 + 4 + 1 + 1 + 32;
        // A tiny (header-only) movie whose `frame_count` field claims ~4.3
        // billion frames. The old `Vec::with_capacity(frame_count)` would try to
        // reserve multiple gigabytes before the input-stream read failed (an OOM
        // DoS found by the `movie` fuzz target). It must now reject cleanly with
        // an EOF: the capacity is capped at the remaining bytes / width.
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: Vec::new(),
            rerecord_count: 0,
            attestation: None,
        };
        let mut bytes = movie.serialize();
        bytes[FRAME_COUNT_OFF..FRAME_COUNT_OFF + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        // Deserialize must return promptly with an error, not exhaust memory.
        assert!(matches!(
            Movie::deserialize(&bytes),
            Err(MovieError::Eof(_))
        ));
    }

    #[test]
    fn deserialize_rejects_truncated_input_stream() {
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: synthetic_inputs(10),
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        // Lop off the last few input bytes — must error, not panic.
        let truncated = &bytes[..bytes.len() - 5];
        assert!(matches!(
            Movie::deserialize(truncated),
            Err(MovieError::Eof(_))
        ));
    }

    /// Drive a ROM with a fixed input sequence, recording as we go; then
    /// replay from the movie's start point and assert framebuffer + audio +
    /// cycle count are byte-identical.
    #[test]
    fn determinism_round_trip_power_on() {
        let rom = synth_nrom();
        let inputs = synthetic_inputs(30);

        // ----- Original run (recording). -----
        let mut nes = Nes::from_rom(&rom).expect("boot");
        nes.power_cycle(); // start point a replay will reconstruct
        let mut rec = MovieRecorder::power_on(&nes);
        let mut orig_fb = 0u64;
        let mut orig_audio = Vec::new();
        for f in &inputs {
            nes.set_buttons(0, f.p1);
            nes.set_buttons(1, f.p2);
            rec.capture(&nes);
            orig_fb = fnv(nes.run_frame());
            orig_audio.extend(nes.drain_audio());
        }
        let orig_cycle = nes.cycle();
        let orig_audio_hash = audio_fnv(&orig_audio);
        let movie = rec.finish();
        assert_eq!(movie.len(), inputs.len());

        // ----- Replay from the movie's start point. -----
        let mut replay = Nes::from_rom(&rom).expect("boot");
        movie.seek_to_start(&mut replay).expect("seek");
        let mut player = MoviePlayer::new(&movie);
        let mut replay_fb = 0u64;
        let mut replay_audio = Vec::new();
        while player.apply_next(&mut replay) {
            replay_fb = fnv(replay.run_frame());
            replay_audio.extend(replay.drain_audio());
        }

        assert_eq!(orig_fb, replay_fb, "framebuffer must replay bit-identical");
        assert_eq!(
            orig_audio_hash,
            audio_fnv(&replay_audio),
            "audio must replay bit-identical"
        );
        assert_eq!(
            orig_cycle,
            replay.cycle(),
            "cumulative cycle count must replay bit-identical"
        );
    }

    /// Replaying the same movie twice must yield identical output (the movie
    /// itself is internally deterministic).
    #[test]
    fn replay_is_internally_deterministic() {
        let rom = synth_nrom();
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: *Nes::from_rom(&rom).unwrap().rom_sha256(),
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: synthetic_inputs(20),
            rerecord_count: 0,
            attestation: None,
        };

        let run = |movie: &Movie| -> (u64, u64, u64) {
            let mut nes = Nes::from_rom(&rom).unwrap();
            movie.seek_to_start(&mut nes).unwrap();
            let mut player = MoviePlayer::new(movie);
            let mut fb = 0u64;
            let mut audio = Vec::new();
            while player.apply_next(&mut nes) {
                fb = fnv(nes.run_frame());
                audio.extend(nes.drain_audio());
            }
            (fb, audio_fnv(&audio), nes.cycle())
        };

        assert_eq!(run(&movie), run(&movie));
    }

    /// Save-state branch: run a base movie partway, snapshot, start a new
    /// branch recorder from that snapshot, and assert the branch replay is
    /// internally deterministic and reconstructs the branch start point.
    #[test]
    fn save_state_branch_round_trip() {
        let rom = synth_nrom();

        // Base run: advance some frames with a fixed input, then branch.
        let base_inputs = synthetic_inputs(10);
        let mut nes = Nes::from_rom(&rom).unwrap();
        nes.power_cycle();
        for f in &base_inputs {
            nes.set_buttons(0, f.p1);
            nes.set_buttons(1, f.p2);
            nes.run_frame();
        }
        let branch_cycle = nes.cycle();
        let branch_fb = fnv(nes.framebuffer());

        // Start a branch recorder from the current state, record more frames.
        let mut branch_rec = MovieRecorder::from_current_state(&nes);
        let branch_inputs = synthetic_inputs(15);
        for f in &branch_inputs {
            nes.set_buttons(0, f.p1);
            nes.set_buttons(1, f.p2);
            branch_rec.capture(&nes);
            nes.run_frame();
        }
        let branch_end_cycle = nes.cycle();
        let branch_end_fb = fnv(nes.framebuffer());
        let branch_movie = branch_rec.finish();
        assert!(matches!(branch_movie.start, StartPoint::SaveState(_)));

        // Replay the branch from its embedded snapshot.
        let run_branch = || -> (u64, u64) {
            let mut replay = Nes::from_rom(&rom).unwrap();
            branch_movie.seek_to_start(&mut replay).unwrap();
            // After seeking, we are back at the branch start point.
            assert_eq!(replay.cycle(), branch_cycle, "branch start cycle");
            assert_eq!(fnv(replay.framebuffer()), branch_fb, "branch start fb");
            let mut player = MoviePlayer::new(&branch_movie);
            let mut fb = 0u64;
            while player.apply_next(&mut replay) {
                fb = fnv(replay.run_frame());
            }
            (fb, replay.cycle())
        };

        let first = run_branch();
        let second = run_branch();
        assert_eq!(first, second, "branch replay internally deterministic");
        // And it reconstructs the live branch end state bit-identically.
        assert_eq!(first.0, branch_end_fb, "branch end fb matches live run");
        assert_eq!(
            first.1, branch_end_cycle,
            "branch end cycle matches live run"
        );

        // Format round-trip survives the embedded save state.
        let bytes = branch_movie.serialize();
        let back = Movie::deserialize(&bytes).unwrap();
        assert_eq!(branch_movie, back);
    }

    #[test]
    fn seek_rejects_rom_mismatch() {
        let rom = synth_nrom();
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0xFF; 32], // deliberately wrong
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: Vec::new(),
            rerecord_count: 0,
            attestation: None,
        };
        let mut nes = Nes::from_rom(&rom).unwrap();
        assert!(matches!(
            movie.seek_to_start(&mut nes),
            Err(MovieError::RomMismatch)
        ));
    }

    #[test]
    fn frame_input_bit_layout_matches_buttons() {
        // The on-wire byte for a frame is exactly Buttons::bits() (FCEUX
        // .fm2 layout). Verify the serialize path preserves it.
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: vec![FrameInput::four_players(
                Buttons::A | Buttons::RIGHT,
                Buttons::B | Buttons::START,
                Buttons::UP,
                Buttons::SELECT,
            )],
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        // Input stream begins after the 53-byte fixed header and the
        // length-prefixed OPTIONS block (no board, no save state).
        let at = 53 + 4 + crate::HardwareOptions::default().to_bytes().len();
        assert_eq!(bytes[at], (Buttons::A | Buttons::RIGHT).bits());
        assert_eq!(bytes[at + 1], (Buttons::B | Buttons::START).bits());
        assert_eq!(bytes[at + 2], Buttons::UP.bits(), "player 3");
        assert_eq!(bytes[at + 3], Buttons::SELECT.bits(), "player 4");
        assert_eq!(bytes[at + 4], 0, "expansion byte reserved/zero");
        assert_eq!(bytes[52], BYTES_PER_FRAME, "the header states the width");
    }

    /// A narrower record still reads: the fields it does not reach default.
    /// (Format 3 writes 5 bytes; the width field is what lets a later build
    /// grow the record additively.)
    #[test]
    fn a_two_byte_record_defaults_players_three_and_four() {
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: vec![FrameInput::four_players(
                Buttons::A,
                Buttons::B,
                Buttons::UP,
                Buttons::DOWN,
            )],
            rerecord_count: 0,
            attestation: None,
        };
        let mut bytes = movie.serialize();
        let at = 53 + 4 + crate::HardwareOptions::default().to_bytes().len();
        bytes[52] = 2; // bytes_per_frame, the fixed header's last byte
        bytes.drain(at + 2..at + 5);
        let back = Movie::deserialize(&bytes).expect("narrow record");
        assert_eq!(back.frames, [FrameInput::new(Buttons::A, Buttons::B)]);
    }

    /// v3.0.0 (ADR 0045): a movie records the emulation epoch beside its
    /// format version, and a movie from another epoch is refused before
    /// anything else is parsed, naming both epochs. Without the check, a
    /// movie recorded by a core that emulates differently (v2.9.9's MMC3
    /// timing, say) would replay its inputs on the new timing and diverge
    /// with no explanation.
    #[test]
    fn a_movie_from_another_emulation_epoch_is_refused() {
        let movie = Movie::new(
            Region::Ntsc,
            [7; 32],
            crate::HardwareOptions::default(),
            None,
            StartPoint::PowerOn,
            vec![FrameInput::new(Buttons::A, Buttons::B)],
        );
        assert_eq!(movie.epoch, crate::EMULATION_EPOCH);
        let bytes = movie.serialize();
        // The epoch sits straight after the format version (bytes 10..14).
        assert_eq!(&bytes[8..10], &MOVIE_FORMAT_VERSION.to_le_bytes());
        assert_eq!(&bytes[10..14], &crate::EMULATION_EPOCH.to_le_bytes());
        assert_eq!(
            Movie::deserialize(&bytes).expect("same epoch").epoch,
            crate::EMULATION_EPOCH
        );

        let mut other = bytes;
        other[10..14].copy_from_slice(&(crate::EMULATION_EPOCH + 1).to_le_bytes());
        assert!(matches!(
            Movie::deserialize(&other),
            Err(MovieError::EpochMismatch { movie, core })
                if movie == crate::EMULATION_EPOCH + 1 && core == crate::EMULATION_EPOCH
        ));
    }

    /// v3.0.0: the epoch is enforced on PLAYBACK too, not only when parsing.
    /// `Movie::epoch` is a public field and a `Movie` can be built in memory,
    /// so a deserialize-only check left `seek_to_start` and `verify` open to
    /// a movie from another epoch (a review finding on #588). Both refuse it, with
    /// the machine untouched, and `verify` refuses it before reporting that
    /// the movie is unattested.
    #[test]
    fn playback_refuses_a_movie_from_another_epoch() {
        let mut nes = Nes::from_rom(&synth_nrom_battery()).unwrap();
        let mut movie = Movie::new(
            nes.region(),
            *nes.rom_sha256(),
            crate::HardwareOptions::default(),
            None,
            StartPoint::PowerOn,
            synthetic_inputs(1),
        );
        movie.epoch = crate::EMULATION_EPOCH + 1;
        nes.sram_mut().fill(0xA5);
        let refused = |r: &Result<(), MovieError>| {
            matches!(r, Err(MovieError::EpochMismatch { movie, core })
                if *movie == crate::EMULATION_EPOCH + 1 && *core == crate::EMULATION_EPOCH)
        };
        assert!(
            refused(&movie.seek_to_start(&mut nes)),
            "seek_to_start refuses"
        );
        assert!(
            nes.sram().iter().all(|&b| b == 0xA5),
            "the machine is untouched"
        );
        assert!(
            refused(&movie.verify(&mut nes).map(|_| ())),
            "verify refuses before NotAttested"
        );
    }

    /// v3.0.0: a format-4 movie (v2.9.9) does not record the epoch, so it is
    /// refused as too old rather than replayed under a timing it may not
    /// have been recorded on.
    #[test]
    fn a_format_4_movie_is_refused_as_too_old() {
        let mut bytes = Movie::new(
            Region::Ntsc,
            [0; 32],
            crate::HardwareOptions::default(),
            None,
            StartPoint::PowerOn,
            vec![],
        )
        .serialize();
        bytes[8..10].copy_from_slice(&4u16.to_le_bytes());
        assert!(matches!(
            Movie::deserialize(&bytes),
            Err(MovieError::FormatTooOld {
                got: 4,
                min: MIN_MOVIE_FORMAT_VERSION
            })
        ));
    }

    /// v3.1.0: a format-5 movie (v3.0.x) carries the options record without
    /// the CPU overclock and the sprite-limit option, so it is refused as too
    /// old rather than decoded one field short.
    #[test]
    fn a_format_5_movie_is_refused_as_too_old() {
        assert_eq!(MIN_MOVIE_FORMAT_VERSION, 6);
        let mut bytes = Movie::new(
            Region::Ntsc,
            [0; 32],
            crate::HardwareOptions::default(),
            None,
            StartPoint::PowerOn,
            vec![],
        )
        .serialize();
        bytes[8..10].copy_from_slice(&5u16.to_le_bytes());
        assert!(matches!(
            Movie::deserialize(&bytes),
            Err(MovieError::FormatTooOld { got: 5, min: 6 })
        ));
    }

    #[test]
    fn recorded_before_v2_timebase_flags_pre_promote_movies() {
        // ADR 0028: a freshly-serialized movie carries the current
        // MOVIE_FORMAT_VERSION (>= 2) and must NOT be flagged.
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: vec![],
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        assert!(matches!(recorded_before_v2_timebase(&bytes), Ok(false)));

        // A v1-tagged blob (format_version = 1, the only value that existed
        // pre-v2.0.0) must be flagged. Since v2.9.8 it no longer parses: it
        // predates the options record (MIN_MOVIE_FORMAT_VERSION).
        let mut v1_bytes = bytes;
        v1_bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        assert!(matches!(recorded_before_v2_timebase(&v1_bytes), Ok(true)));
        assert!(matches!(
            Movie::deserialize(&v1_bytes),
            Err(MovieError::FormatTooOld {
                got: 1,
                min: MIN_MOVIE_FORMAT_VERSION
            })
        ));

        // Malformed input still surfaces the normal header errors.
        assert!(matches!(
            recorded_before_v2_timebase(&[0u8; 4]),
            Err(MovieError::HeaderTruncated { .. })
        ));
        assert!(matches!(
            recorded_before_v2_timebase(&[0xFFu8; 10]),
            Err(MovieError::BadMagic { .. })
        ));
    }

    // ----------------------------------------------------------------- v2.9.8
    // Emulation options travel with the movie (maintainer decision 2026-10-01)
    // -------------------------------------------------------------------------

    /// Record `frames` frames on `nes` (already configured by the caller) from
    /// a power-on start, returning the movie and the machine's full snapshot at
    /// the end. The snapshot is the strongest available "replays identically"
    /// oracle: it covers work RAM, the PPU's warm-up counter and every other
    /// piece of serialized state, so a difference a test ROM never draws on
    /// screen still shows up.
    ///
    /// Returned as an FNV hash so a failing comparison prints two numbers
    /// rather than two multi-kilobyte byte arrays.
    fn record_power_on(nes: &mut Nes, frames: usize) -> (Movie, u64) {
        power_on_for_movie(nes);
        let mut rec = MovieRecorder::power_on(nes);
        for f in synthetic_inputs(frames) {
            nes.set_buttons(0, f.p1);
            nes.set_buttons(1, f.p2);
            rec.capture(nes);
            nes.run_frame();
        }
        (rec.finish(), fnv(&nes.snapshot()))
    }

    /// Replay `movie` on `nes` from its start point and return the snapshot's
    /// hash.
    fn replay(movie: &Movie, nes: &mut Nes) -> u64 {
        movie.seek_to_start(nes).expect("seek");
        let mut player = MoviePlayer::new(movie);
        while player.apply_next(nes) {
            nes.run_frame();
        }
        fnv(&nes.snapshot())
    }

    /// `synth_nrom` whose reset handler writes `$1E` to PPUMASK before it
    /// loops. On the NES model that write lands inside the PPU's warm-up and
    /// is ignored; on the Famicom model the warm-up is already over and it
    /// takes effect -- so the two console models leave different PPU state.
    fn synth_nrom_ppumask() -> Vec<u8> {
        let mut rom = synth_nrom();
        // LDA #$1E ; STA $2001 ; JMP $C005
        let prog = [0xA9, 0x1E, 0x8D, 0x01, 0x20, 0x4C, 0x05, 0xC0];
        rom[16..16 + prog.len()].copy_from_slice(&prog);
        rom
    }

    /// A movie recorded on the Famicom console model replays identically on a
    /// player whose own setting is the NES model, through a `.rnm` round trip.
    #[test]
    fn a_famicom_movie_replays_identically_on_an_nes_configured_player() {
        let rom = synth_nrom_ppumask();
        let mut rec_nes = Nes::from_rom(&rom).unwrap();
        rec_nes.set_console_model(crate::ConsoleModel::Famicom);
        let (movie, recorded) = record_power_on(&mut rec_nes, 12);
        let movie = Movie::deserialize(&movie.serialize()).expect("round trip");

        let mut player = Nes::from_rom(&rom).unwrap();
        assert_eq!(player.console_model(), crate::ConsoleModel::Nes);
        assert_eq!(replay(&movie, &mut player), recorded);
    }

    /// A movie recorded with a seeded power-on RAM fill replays identically on
    /// a player configured with a different fill, through a `.rnm` round trip.
    #[test]
    fn a_seeded_power_on_ram_movie_replays_identically_on_a_differently_configured_player() {
        let rom = synth_nrom();
        let mut rec_nes = Nes::from_rom(&rom).unwrap();
        rec_nes.set_power_on_ram(crate::PowerOnRam::Seeded(0x5EED_1234));
        let (movie, recorded) = record_power_on(&mut rec_nes, 12);
        let movie = Movie::deserialize(&movie.serialize()).expect("round trip");

        let mut player = Nes::from_rom(&rom).unwrap();
        player.set_power_on_ram(crate::PowerOnRam::Filled(0xFF));
        assert_eq!(replay(&movie, &mut player), recorded);
    }

    /// Every other option travels too: OAM decay, both die revisions, the
    /// power-up palette, the overclock, the Four Score, the Zapper light model
    /// and a Game Genie code, recorded on one machine and replayed on a player
    /// whose settings are all at their defaults.
    #[test]
    fn every_recorded_option_replays_on_a_default_player() {
        let rom = synth_nrom();
        let mut rec_nes = Nes::from_rom(&rom).unwrap();
        rec_nes.set_oam_decay(true);
        rec_nes.set_ppu_revision(crate::PpuRevision::Rp2c02G);
        rec_nes.set_cpu_2a03_revision(crate::Cpu2A03Revision::Rp2A03H);
        rec_nes.set_power_up_palette(crate::PaletteInit::Blargg);
        rec_nes.set_extra_scanlines(20);
        rec_nes.set_four_score(true);
        rec_nes.set_zapper_temporal_light(false);
        rec_nes.add_genie_code("SXIOPO").unwrap();
        let (movie, recorded) = record_power_on(&mut rec_nes, 12);
        let movie = Movie::deserialize(&movie.serialize()).expect("round trip");

        let mut player = Nes::from_rom(&rom).unwrap();
        assert_eq!(replay(&movie, &mut player), recorded);
        assert!(player.oam_decay_enabled());
        assert_eq!(player.extra_scanlines(), 20);
        assert_eq!(player.genie_codes().count(), 1);
    }

    /// A movie written before the options were recorded is refused with a
    /// clear error rather than replayed under whatever the player has set.
    #[test]
    fn a_movie_older_than_the_options_epoch_is_rejected() {
        let movie = Movie {
            epoch: crate::EMULATION_EPOCH,
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            options: crate::HardwareOptions::default(),
            board: None,
            start: StartPoint::PowerOn,
            frames: synthetic_inputs(2),
            rerecord_count: 0,
            attestation: None,
        };
        let mut bytes = movie.serialize();
        bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
        let err = Movie::deserialize(&bytes).expect_err("a v2 movie must be refused");
        let text = alloc::format!("{err}");
        assert!(
            text.contains("re-record"),
            "the error says what to do: {text}"
        );
    }

    /// v2.9.8 — a movie whose embedded start state predates the `.rns`
    /// epoch 3 is refused with the same "re-record" advice as an old movie.
    #[test]
    fn a_movie_with_an_old_start_state_says_to_re_record() {
        let rom = synth_nrom();
        let nes = Nes::from_rom(&rom).unwrap();
        let mut movie = MovieRecorder::from_current_state(&nes).finish();
        if let StartPoint::SaveState(blob) = &mut movie.start {
            // The container version follows the 8-byte "RUSTYNES" magic.
            blob[8..10].copy_from_slice(&2u16.to_le_bytes());
        }
        let movie = Movie::deserialize(&movie.serialize()).expect("the movie itself parses");
        let mut player = Nes::from_rom(&rom).unwrap();
        let err = movie
            .seek_to_start(&mut player)
            .expect_err("old start state");
        assert!(
            matches!(err, MovieError::StartStateTooOld { got: 2, .. }),
            "got {err:?}"
        );
        assert!(alloc::format!("{err}").contains("re-record"));
    }

    /// v2.9.8 (`CodeRabbit` on the review slice #580) — a refused start leaves
    /// the player's machine as it was. `seek_to_start` applies the movie's
    /// options before it reaches the start point, and `apply` refills work RAM
    /// and palette RAM; without a rollback, a movie refused for an old start
    /// state or an undecodable Game Genie code still left the running game
    /// with the movie's RAM fill and configuration.
    #[test]
    fn a_refused_start_leaves_the_player_untouched() {
        let rom = synth_nrom();
        let nes = Nes::from_rom(&rom).unwrap();
        let recorded = MovieRecorder::from_current_state(&nes).finish();

        let mut old_start = recorded.clone();
        if let StartPoint::SaveState(blob) = &mut old_start.start {
            blob[8..10].copy_from_slice(&2u16.to_le_bytes());
        }
        let mut bad_code = recorded;
        bad_code.options.genie_codes.push("QQQQQQ".into());

        for (what, movie) in [("old start state", old_start), ("bad code", bad_code)] {
            // A player configured differently from the movie, mid-game.
            let mut player = Nes::from_rom(&rom).unwrap();
            player.set_four_score(true);
            player.set_power_on_ram(PowerOnRam::Filled(0xA5));
            player.run_frame();
            let before_state = player.snapshot();
            let before_options = HardwareOptions::capture(&player);

            movie.seek_to_start(&mut player).expect_err(what);
            assert_eq!(
                HardwareOptions::capture(&player),
                before_options,
                "{what}: the player's options changed"
            );
            assert!(
                player.snapshot() == before_state,
                "{what}: the player's machine state changed"
            );
        }
    }
}
