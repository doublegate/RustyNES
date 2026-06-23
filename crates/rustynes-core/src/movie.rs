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
pub const MOVIE_FORMAT_VERSION: u16 = 1;

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
        if bytes_per_frame > BYTES_PER_FRAME {
            // A newer movie packs more device bytes than we understand; we
            // fail cleanly rather than mis-parse (the reserved byte exists
            // precisely so this stays a graceful error, not a corruption).
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
        // Input stream: `frame_count` records of `bytes_per_frame` bytes.
        let mut frames = Vec::with_capacity(frame_count);
        let width = usize::from(bytes_per_frame);
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
        Ok(Self {
            region,
            rom_sha256,
            start,
            frames,
            rerecord_count,
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
        }
    }

    /// Record the controller inputs currently held on `nes`. Call this each
    /// frame *before* [`Nes::run_frame`], after the frontend has applied its
    /// `set_buttons` calls — this captures exactly the inputs the upcoming
    /// frame consumes.
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
    fn deserialize_rejects_truncated_input_stream() {
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: [0; 32],
            start: StartPoint::PowerOn,
            frames: synthetic_inputs(10),
            rerecord_count: 0,
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
}
