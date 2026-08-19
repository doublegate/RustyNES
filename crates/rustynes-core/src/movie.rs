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
//!     format version  : u16 LE        (currently 1 = MOVIE_FORMAT_VERSION)
//!     region          : u8            (0 = NTSC, 1 = PAL, 2 = Dendy)
//!     flags           : u8            (bit0 = embedded save-state start point)
//!     rom sha-256     : [u8; 32]      (full hash — authoritative ROM identity)
//!     frame count     : u32 LE
//!     bytes per frame : u8            (currently 3: P1, P2, expansion-reserved)
//! START POINT (only when flags bit0 set):
//!     length-prefixed `.rns` save-state blob (u32 LE length + bytes)
//! INPUT STREAM:
//!     frame_count * bytes_per_frame raw bytes; each frame = [p1, p2, expansion]
//! ```
//!
//! This module is `no_std`-clean: it uses only `core` + `alloc` and the
//! `BinWriter` / `BinReader` primitives from [`crate::save_state`].

use alloc::vec::Vec;

use crate::Region;
use crate::controller::Buttons;
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
pub const MOVIE_FORMAT_VERSION: u16 = 2;

/// Peek a `.rnm` blob's header to learn its recording epoch.
///
/// Checks whether it was recorded on a pre-v2.0.0-timebase build
/// (`format_version < 2`), WITHOUT fully parsing the movie. Intended for
/// tooling/UI that wants to warn before relying on the determinism
/// (verify-replay) guarantee across the v2.0.0 engine-timebase boundary —
/// see [`MOVIE_FORMAT_VERSION`]'s v2 doc.
///
/// Playback itself is unaffected: [`Movie::deserialize`] still accepts and
/// plays any `format_version <= MOVIE_FORMAT_VERSION` movie as pure input
/// replay; this function exists only to let a caller decide whether to
/// additionally warn that the bit-identical guarantee is unverified for a
/// movie recorded across the boundary.
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

/// Bytes stored per recorded frame: player 1, player 2, and a reserved
/// expansion-port byte (always `0` in v1).
///
/// Stored explicitly in the header so a future device byte can grow the
/// record without a container-version bump.
pub const BYTES_PER_FRAME: u8 = 3;

/// Header flag: an embedded `.rns` save-state start point follows the header.
const FLAG_HAS_SAVE_STATE: u8 = 0x01;

/// Per-frame controller input: the `Buttons` bits for both standard ports
/// plus a reserved expansion byte. Bit layout matches FCEUX `.fm2`
/// (`bit0=A .. bit7=Right`), which is exactly [`Buttons::bits`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameInput {
    /// Player 1 (`$4016`) button state.
    pub p1: Buttons,
    /// Player 2 (`$4017`) button state.
    pub p2: Buttons,
    /// Reserved expansion-port byte (currently always `0`).
    pub expansion: u8,
}

impl FrameInput {
    /// Build a two-controller frame with no expansion byte.
    #[must_use]
    pub const fn new(p1: Buttons, p2: Buttons) -> Self {
        Self {
            p1,
            p2,
            expansion: 0,
        }
    }
}

/// Marker for the optional attestation tail: `"RNAT"` little-endian.
///
/// Read as a `u32` after the re-record count. A movie with no attestation simply
/// ends there, so the marker is what distinguishes "no attestation recorded" from
/// "attestation present" — absence is a fact, not a parse failure.
pub const ATTESTATION_MAGIC: u32 = u32::from_le_bytes(*b"RNAT");

/// Attestation tail schema version.
pub const ATTESTATION_VERSION: u16 = 1;

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
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            let w = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
            self.0 = (self.0 ^ w).wrapping_mul(Self::PRIME);
        }
        let rem = chunks.remainder();
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
        self.hash
            .write(&[input.p1.bits(), input.p2.bits(), input.expansion]);
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
}

/// A complete TAS movie: a versioned header, a start point, and the
/// per-frame input stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Movie {
    /// Cartridge region the movie was recorded under.
    pub region: Region,
    /// Full SHA-256 of the ROM the movie was recorded against.
    pub rom_sha256: [u8; 32],
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
        w.u8(region_to_byte(self.region));
        let flags = match &self.start {
            StartPoint::PowerOn => 0,
            StartPoint::SaveState(_) => FLAG_HAS_SAVE_STATE,
        };
        w.u8(flags);
        w.bytes(&self.rom_sha256);
        w.u32(frame_count);
        w.u8(BYTES_PER_FRAME);
        if let StartPoint::SaveState(blob) = &self.start {
            w.lp_bytes(blob);
        }
        for f in &self.frames {
            w.u8(f.p1.bits());
            w.u8(f.p2.bits());
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
        // Fixed header: magic(8) + version(2) + region(1) + flags(1) +
        // sha256(32) + frame_count(4) + bytes_per_frame(1) = 49 bytes.
        const HEADER_LEN: usize = 8 + 2 + 1 + 1 + 32 + 4 + 1;
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
        // Region + flags.
        let region = region_from_byte(r.u8().map_err(map_eof)?)?;
        let flags = r.u8().map_err(map_eof)?;
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
        // 49-byte header claim a multi-gigabyte allocation — an OOM DoS (found
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
            // rec[0] = p1, rec[1] = p2 (present whenever width >= 2, which it
            // always is for v1's width of 3); rec[2] = expansion when width
            // >= 3. Lower widths default the missing fields.
            let p1 = Buttons::from_bits_truncate(rec.first().copied().unwrap_or(0));
            let p2 = Buttons::from_bits_truncate(rec.get(1).copied().unwrap_or(0));
            let expansion = rec.get(2).copied().unwrap_or(0);
            frames.push(FrameInput { p1, p2, expansion });
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
            start,
            frames,
            rerecord_count,
            attestation,
        })
    }

    /// Rewind a running emulator to this movie's start point, ready to replay
    /// from frame 0.
    ///
    /// For [`StartPoint::PowerOn`] this power-cycles `nes`. For
    /// [`StartPoint::SaveState`] it restores the embedded snapshot. In both
    /// cases the ROM hash is checked against the movie's recorded hash.
    ///
    /// # Errors
    ///
    /// Returns [`MovieError::RomMismatch`] if `nes` is running a different
    /// ROM, or [`MovieError::BadSaveState`] if the embedded snapshot is
    /// malformed.
    pub fn seek_to_start(&self, nes: &mut Nes) -> Result<(), MovieError> {
        if nes.rom_sha256() != &self.rom_sha256 {
            return Err(MovieError::RomMismatch);
        }
        match &self.start {
            StartPoint::PowerOn => nes.power_cycle(),
            StartPoint::SaveState(blob) => nes.restore(blob)?,
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
    /// [`MovieError::RomMismatch`] if `nes` is running a different ROM, or
    /// [`MovieError::BadSaveState`] if an embedded start point is malformed.
    /// A movie with no attestation is **not** an error; it returns
    /// [`VerifyOutcome::NotAttested`], because "this movie makes no claim" and
    /// "this movie makes a false claim" are different answers.
    pub fn verify(&self, nes: &mut Nes) -> Result<VerifyOutcome, MovieError> {
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
    start: StartPoint,
    frames: Vec<FrameInput>,
    /// v2.3.2 "Lucid" — optional attestation accumulator. `None` (the default)
    /// records a plain movie, byte-for-byte what previous versions produced.
    attestation: Option<AttestationBuilder>,
}

impl MovieRecorder {
    /// Begin recording a movie that starts from a fresh power-on of the ROM
    /// `nes` is running. The caller is responsible for power-cycling `nes`
    /// before the first captured frame so the recording starts from the same
    /// state a replay will reconstruct.
    #[must_use]
    pub const fn power_on(nes: &Nes) -> Self {
        Self {
            region: nes.region(),
            rom_sha256: *nes.rom_sha256(),
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
    /// # Two ports only, including under a Four Score
    ///
    /// [`FrameInput`] models ports 0 and 1, so this reads `nes.buttons(0)` and
    /// `nes.buttons(1)` and **nothing else**. The core itself carries four —
    /// the frontend calls `set_buttons(2)` / `set_buttons(3)` whenever the Four
    /// Score adapter is active — so recording a four-player session captures
    /// half of what drove it, and replaying that movie diverges from the run it
    /// came from.
    ///
    /// Stated here rather than left to be discovered, because the failure is
    /// silent at record time: nothing about a `.rnm` says which ports it could
    /// not hold, and the divergence only appears on playback. The frontend's
    /// Replay panel says so where the topology is displayed, and
    /// `Movie::verify`'s attestation catches it after the fact.
    ///
    /// Widening [`FrameInput`] is a `.rnm` format epoch change (ADR 0028), not
    /// an additive one, which is why this is a documented limit rather than a
    /// fix. The `.fm2` importer already takes the same position for the same
    /// reason — it keeps pads 1 and 2, drops 3 and 4, and preserves the
    /// `fourscore` flag so the caller is not silently misled.
    pub fn capture(&mut self, nes: &Nes) {
        self.frames.push(FrameInput {
            p1: nes.buttons(0),
            p2: nes.buttons(1),
            expansion: 0,
        });
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
        nes.set_buttons(0, input.p1);
        nes.set_buttons(1, input.p2);
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

    #[test]
    fn format_round_trip_power_on() {
        let inputs = synthetic_inputs(120);
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: [0xAB; 32],
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
            region: Region::Ntsc,
            rom_sha256: [0x5A; 32],
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
            region: Region::Pal,
            rom_sha256: [0x11; 32],
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
        let mut bytes = vec![0u8; 49];
        bytes[..8].copy_from_slice(b"NOTAMOVI");
        assert!(matches!(
            Movie::deserialize(&bytes),
            Err(MovieError::BadMagic { .. })
        ));
    }

    #[test]
    fn deserialize_rejects_too_new_format_cleanly() {
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: [0; 32],
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
        const FRAME_COUNT_OFF: usize = 8 + 2 + 1 + 1 + 32;
        // A tiny (header-only) movie whose `frame_count` field claims ~4.3
        // billion frames. The old `Vec::with_capacity(frame_count)` would try to
        // reserve multiple gigabytes before the input-stream read failed (an OOM
        // DoS found by the `movie` fuzz target). It must now reject cleanly with
        // an EOF: the capacity is capped at the remaining bytes / width.
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: [0; 32],
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
            region: Region::Ntsc,
            rom_sha256: [0; 32],
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
            region: Region::Ntsc,
            rom_sha256: *Nes::from_rom(&rom).unwrap().rom_sha256(),
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
            region: Region::Ntsc,
            rom_sha256: [0xFF; 32], // deliberately wrong
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
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            start: StartPoint::PowerOn,
            frames: vec![FrameInput::new(
                Buttons::A | Buttons::RIGHT,
                Buttons::B | Buttons::START,
            )],
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        // Input stream begins right after the 49-byte fixed header (no
        // save state).
        let p1 = bytes[49];
        let p2 = bytes[50];
        assert_eq!(p1, (Buttons::A | Buttons::RIGHT).bits());
        assert_eq!(p2, (Buttons::B | Buttons::START).bits());
        assert_eq!(bytes[51], 0, "expansion byte reserved/zero");
    }

    #[test]
    fn recorded_before_v2_timebase_flags_pre_promote_movies() {
        // ADR 0028: a freshly-serialized movie carries the current
        // MOVIE_FORMAT_VERSION (>= 2) and must NOT be flagged.
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            start: StartPoint::PowerOn,
            frames: vec![],
            rerecord_count: 0,
            attestation: None,
        };
        let bytes = movie.serialize();
        assert!(matches!(recorded_before_v2_timebase(&bytes), Ok(false)));

        // A v1-tagged blob (format_version = 1, the only value that existed
        // pre-v2.0.0) must be flagged, even though it still parses fine.
        let mut v1_bytes = bytes;
        v1_bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        assert!(matches!(recorded_before_v2_timebase(&v1_bytes), Ok(true)));
        assert!(
            Movie::deserialize(&v1_bytes).is_ok(),
            "a v1-tagged movie must still parse and play as input"
        );

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
}
