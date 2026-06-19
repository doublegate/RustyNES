//! v1.7.0 "Forge" Workstream G4 — legacy NES TAS movie import.
//!
//! The historical pre-`.fm2` / pre-`.bk2` `TASVideos` corpus lives in a handful
//! of small binary containers. This module adds importers for the NES-relevant
//! ones so `RustyNES` can "play any NES TAS":
//!
//! - **`.fcm`** — FCEUX / FCE Ultra legacy binary movie (`FCM\x1A`, version 2).
//!   A *sparse toggle/delta* input stream, not a per-frame bitmask dump.
//! - **`.fmv`** — `Famtasia` movie (`FMV\x1A`, fixed 144-byte header). A full
//!   per-frame byte-per-controller dump with a `Famtasia`-specific bit order.
//! - **`.vmv`** — `VirtuaNES` movie (`VirtuaNES MV`). A full per-frame dump; the
//!   layout is documentation-derived (`TASVideos` `OtherEmulators/VMV`), since
//!   `BizHawk` never shipped a `.vmv` importer.
//!
//! Each parser mirrors the existing [`crate::movie_interop`] (`.fm2`) and
//! [`crate::bk2_interop`] (`.bk2`) design: a pure byte→[`Movie`] transform that
//! never panics on malformed input, returns [`StartPoint::PowerOn`] only, and
//! reuses the **canonical movie-import power-on alignment** the `.fm2` path
//! established (a deterministic cold boot via [`Movie::seek_to_start`]), so an
//! imported movie replays bit-for-bit.
//!
//! # `Mednafen` `.mc2` — deliberately rejected (it is a PC Engine format)
//!
//! The v1.7.0 plan lists `.mc2` under "`Mednafen` NES", but `BizHawk`'s
//! `Mc2Import.cs` is `[ImporterFor("PCEjin/Mednafen", ".mc2")]` and targets the
//! **PC Engine** (PCE buttons `B1/B2/Run/Select`, platform PCE/PCECD) — there is
//! no NES gamepad data in it. Rather than mis-map PCE buttons onto NES, the
//! `.mc2` path is a clean, documented rejection ([`import_mc2`]).
//!
//! # The native button bit order
//!
//! `RustyNES`'s [`Buttons`] bit layout is `A=0, B=1, Select=2, Start=3, Up=4,
//! Down=5, Left=6, Right=7` — the canonical NES order. The `.fcm` button *index*
//! order and the `.vmv` *bit* order are identical to it, so those map straight
//! through [`Buttons::from_bits_truncate`]. `Famtasia` `.fmv` uses a different
//! bit order (`Right=0, Left=1, Up=2, Down=3, B=4, A=5, Select=6, Start=7`) and
//! is permuted by [`fmv_byte_to_buttons`].
//!
//! This module is `no_std`-clean: it uses only `core` + `alloc`.

use alloc::vec::Vec;

use crate::Region;
use crate::controller::Buttons;
use crate::movie::{FrameInput, Movie, StartPoint};
use thiserror::Error;

/// `.fcm` signature: `FCM` + the DOS EOF byte.
const FCM_MAGIC: &[u8; 4] = b"FCM\x1A";
/// The only `.fcm` version this module parses.
const FCM_VERSION: u32 = 2;
/// `.fmv` signature: `FMV` + the DOS EOF byte.
const FMV_MAGIC: &[u8; 4] = b"FMV\x1A";
/// `Famtasia` fixed header length; input data begins here.
const FMV_HEADER_LEN: usize = 144;
/// `.vmv` signature.
const VMV_MAGIC: &[u8; 12] = b"VirtuaNES MV";

/// Errors produced by the legacy movie importers.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LegacyMovieError {
    /// The blob is shorter than the format's fixed header.
    #[error("legacy movie truncated: need at least {expected} bytes, got {got}")]
    Truncated {
        /// Bytes the header needs.
        expected: usize,
        /// Bytes available.
        got: usize,
    },

    /// The signature did not match the expected magic for this format.
    #[error("legacy movie magic mismatch (not a {format} movie)")]
    BadMagic {
        /// The format name we were trying to parse.
        format: &'static str,
    },

    /// The format version is outside the range we understand.
    #[error("legacy movie {format} version {got} not supported")]
    BadVersion {
        /// The format name.
        format: &'static str,
        /// The version we read.
        got: u32,
    },

    /// A structural problem decoding the input stream (a malformed record or an
    /// offset that runs past EOF).
    #[error("legacy movie {format} malformed: {reason}")]
    Malformed {
        /// The format name.
        format: &'static str,
        /// What was wrong.
        reason: &'static str,
    },

    /// A feature we deliberately do not support (a save-state / non-reset start,
    /// a four-score movie, or a non-NES container).
    #[error("legacy movie {format} unsupported: {reason}")]
    Unsupported {
        /// The format name.
        format: &'static str,
        /// What is unsupported.
        reason: &'static str,
    },
}

/// Metadata recovered from a legacy movie that has no home on [`Movie`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LegacyMeta {
    /// Rerecord count (0 if absent / unknown).
    pub rerecord_count: u64,
    /// `true` if the source declared a PAL region.
    pub pal: bool,
}

/// Read a little-endian `u32` at `off`, or `None` if it runs past the end.
fn rd_u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    let b = bytes.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Import an FCEUX / FCE Ultra legacy `.fcm` movie.
///
/// `.fcm` is a sparse **toggle/delta** stream: each record advances some number
/// of frames (emitting the *current* held controller state for each), then
/// either toggles one button on one controller or issues a console command
/// (Reset / Power / FDS / VS). `RustyNES`'s [`FrameInput`] has no console-command
/// representation, so commands are decoded (so the stream stays in sync) but only
/// affect the frame count, exactly as the `.fm2` importer treats `MOVIECMD_RESET`.
///
/// The returned [`Movie`] always uses [`StartPoint::PowerOn`]; `rom_sha256` is the
/// authoritative ROM identity (the `.fcm`'s embedded MD5 is not validated here).
///
/// # Errors
///
/// [`LegacyMovieError`] for a bad magic / version, a save-state-anchored start, a
/// four-score (>2-controller) update, or a truncated stream. Never panics.
pub fn import_fcm(
    bytes: &[u8],
    rom_sha256: [u8; 32],
) -> Result<(Movie, LegacyMeta), LegacyMovieError> {
    const FMT: &str = "fcm";
    // Fixed header up to the ROM-name string starts at 0x34; we need at least the
    // fields we read (signature .. firstFrameOffset .. md5 .. emu-version = 0x34).
    const MIN_HEADER: usize = 0x34;
    if bytes.len() < MIN_HEADER {
        return Err(LegacyMovieError::Truncated {
            expected: MIN_HEADER,
            got: bytes.len(),
        });
    }
    if &bytes[0..4] != FCM_MAGIC {
        return Err(LegacyMovieError::BadMagic { format: FMT });
    }
    let version = rd_u32_le(bytes, 0x04).unwrap_or(0);
    if version != FCM_VERSION {
        return Err(LegacyMovieError::BadVersion {
            format: FMT,
            got: version,
        });
    }
    let flags = bytes[0x08];
    // bit1: 1 = reset/power-on start, 0 = begins from an embedded quicksave.
    let reset_based = flags & 0x02 != 0;
    if !reset_based {
        return Err(LegacyMovieError::Unsupported {
            format: FMT,
            reason: "begins from a save-state (cross-emulator save states are not portable)",
        });
    }
    // bit2: 0 = NTSC, 1 = PAL.
    let pal = flags & 0x04 != 0;
    let frame_count = rd_u32_le(bytes, 0x0C).unwrap_or(0) as usize;
    let rerecord_count = u64::from(rd_u32_le(bytes, 0x10).unwrap_or(0));
    // firstFrameOffset (the absolute offset of the input data) lives at 0x1C; the
    // 0x14 size field and the 0x18 savestate offset are read-and-discarded.
    let first_frame = rd_u32_le(bytes, 0x1C).unwrap_or(0) as usize;
    if first_frame == 0 || first_frame > bytes.len() {
        return Err(LegacyMovieError::Malformed {
            format: FMT,
            reason: "input-data offset is out of range",
        });
    }

    let stream = &bytes[first_frame..];
    let frames = decode_fcm_stream(stream, frame_count)?;
    let movie = Movie {
        region: if pal { Region::Pal } else { Region::Ntsc },
        rom_sha256,
        start: StartPoint::PowerOn,
        frames,
    };
    Ok((
        movie,
        LegacyMeta {
            rerecord_count,
            pal,
        },
    ))
}

/// Decode the `.fcm` toggle/delta stream into a dense per-frame input log.
///
/// The running state of both controllers is held across records; a controller
/// update flips one button bit, a control command (bit7 set) is consumed but not
/// represented. `frame_hint` is the header's frame count; we honour it as a cap
/// and stop early if the stream ends.
fn decode_fcm_stream(
    stream: &[u8],
    frame_hint: usize,
) -> Result<Vec<FrameInput>, LegacyMovieError> {
    const FMT: &str = "fcm";
    // Cap allocation by the declared frame count (defensive: a hostile header
    // can't force a huge pre-allocation, and a runaway stream can't grow past it).
    let cap = frame_hint.min(1 << 24);
    let mut frames: Vec<FrameInput> = Vec::with_capacity(cap.min(4096));
    // Running held state for P1/P2.
    let mut held = [Buttons::empty(); 2];
    let mut i = 0usize;

    let emit = |frames: &mut Vec<FrameInput>, held: &[Buttons; 2], n: usize| {
        for _ in 0..n {
            if frames.len() >= frame_hint && frame_hint != 0 {
                break;
            }
            frames.push(FrameInput::new(held[0], held[1]));
        }
    };

    while i < stream.len() {
        if frame_hint != 0 && frames.len() >= frame_hint {
            break;
        }
        let update = stream[i];
        i += 1;
        // Bits 5-6: number of following delta bytes (0..=3), little-endian frame
        // advance.
        let delta_bytes = usize::from((update >> 5) & 0x3);
        if i + delta_bytes > stream.len() {
            return Err(LegacyMovieError::Malformed {
                format: FMT,
                reason: "delta bytes run past end of stream",
            });
        }
        let mut advance: usize = 0;
        for b in 0..delta_bytes {
            advance |= usize::from(stream[i + b]) << (8 * b);
        }
        i += delta_bytes;
        // Advance `advance` frames emitting the current held state.
        emit(&mut frames, &held, advance);

        if update & 0x80 != 0 {
            // Control update (`1aabbbbb`): the low 5 bits are a console command
            // (Reset / Power / FDS / VS). The byte is already consumed above, so
            // the stream stays aligned; FrameInput has no console-command
            // representation, so it does not alter held state. We then emit one
            // frame (below), exactly as the `.fm2` importer treats a reset.
        } else {
            // Controller update (`0aabbccc`): player = ((update >> 3) & 0x3) + 1,
            // button index = update & 0x7. The button index order is the canonical
            // NES order, identical to RustyNES's Buttons bit layout.
            let player = ((update >> 3) & 0x3) as usize; // 0 or 1 for P1/P2
            let button_idx = update & 0x7;
            if player >= 2 {
                return Err(LegacyMovieError::Unsupported {
                    format: FMT,
                    reason: "four-score (>2 controllers) not supported",
                });
            }
            let bit = Buttons::from_bits_truncate(1u8 << button_idx);
            held[player] ^= bit; // toggle
        }
        // Each update byte is followed by one emitted frame.
        emit(&mut frames, &held, 1);
    }

    Ok(frames)
}

/// Permute a `Famtasia` `.fmv` controller byte into `RustyNES` [`Buttons`].
///
/// Famtasia bit order: `Right=0, Left=1, Up=2, Down=3, B=4, A=5, Select=6,
/// Start=7` (differs from the canonical NES order, so it cannot pass through
/// untouched).
#[must_use]
pub fn fmv_byte_to_buttons(byte: u8) -> Buttons {
    let mut b = Buttons::empty();
    if byte & 0x01 != 0 {
        b |= Buttons::RIGHT;
    }
    if byte & 0x02 != 0 {
        b |= Buttons::LEFT;
    }
    if byte & 0x04 != 0 {
        b |= Buttons::UP;
    }
    if byte & 0x08 != 0 {
        b |= Buttons::DOWN;
    }
    if byte & 0x10 != 0 {
        b |= Buttons::B;
    }
    if byte & 0x20 != 0 {
        b |= Buttons::A;
    }
    if byte & 0x40 != 0 {
        b |= Buttons::SELECT;
    }
    if byte & 0x80 != 0 {
        b |= Buttons::START;
    }
    b
}

/// Import a `Famtasia` `.fmv` movie.
///
/// Fixed 144-byte header (`FMV\x1A` + flags). Flags byte 2 (`0x05`) selects which
/// of P1 / P2 / FDS streams are present; the per-frame record is one byte per
/// active stream in [P1, P2, FDS] order. `Famtasia` has no reliable PAL flag, so
/// the region is reported as NTSC. A save-state-anchored movie (flags1 bit2) is
/// rejected. The FDS byte (if present) is read to keep alignment but not decoded.
///
/// # Errors
///
/// [`LegacyMovieError`] for a bad magic, a save-state start, or a truncated body.
pub fn import_fmv(
    bytes: &[u8],
    rom_sha256: [u8; 32],
) -> Result<(Movie, LegacyMeta), LegacyMovieError> {
    const FMT: &str = "fmv";
    if bytes.len() < FMV_HEADER_LEN {
        return Err(LegacyMovieError::Truncated {
            expected: FMV_HEADER_LEN,
            got: bytes.len(),
        });
    }
    if &bytes[0..4] != FMV_MAGIC {
        return Err(LegacyMovieError::BadMagic { format: FMT });
    }
    let flags1 = bytes[0x04];
    // bit2 = save-state-based start.
    if flags1 & 0x04 != 0 {
        return Err(LegacyMovieError::Unsupported {
            format: FMT,
            reason: "begins from a save-state (cross-emulator save states are not portable)",
        });
    }
    let flags2 = bytes[0x05];
    let has_fds = flags2 & 0x20 != 0;
    let has_p2 = flags2 & 0x40 != 0;
    let has_p1 = flags2 & 0x80 != 0;
    // Rerecord count is stored as (value - 1); BizHawk adds 1 back.
    let rerecord_count = u64::from(rd_u32_le(bytes, 0x0A).unwrap_or(0)).wrapping_add(1);

    // Bytes per frame = number of active streams (P1, P2, FDS).
    let bpf = usize::from(has_p1) + usize::from(has_p2) + usize::from(has_fds);
    if bpf == 0 {
        return Err(LegacyMovieError::Malformed {
            format: FMT,
            reason: "no active controller streams declared",
        });
    }

    let body = &bytes[FMV_HEADER_LEN..];
    let frame_count = body.len() / bpf;
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let base = f * bpf;
        let mut p1 = Buttons::empty();
        let mut p2 = Buttons::empty();
        // Streams are stored in [P1, P2, FDS] order; advance `col` past each
        // active stream. The FDS byte (if present) is consumed for alignment but
        // not decoded (FrameInput has no FDS command).
        let mut col = 0usize;
        if has_p1 {
            p1 = fmv_byte_to_buttons(body[base + col]);
            col += 1;
        }
        if has_p2 {
            p2 = fmv_byte_to_buttons(body[base + col]);
            col += 1;
        }
        // Account for the FDS byte's column so the (unused) `col` reflects the
        // full record width; silences `unused_assignments` and documents intent.
        let _ = (col, has_fds);
        frames.push(FrameInput::new(p1, p2));
    }

    let movie = Movie {
        region: Region::Ntsc, // Famtasia carries no reliable PAL flag.
        rom_sha256,
        start: StartPoint::PowerOn,
        frames,
    };
    Ok((
        movie,
        LegacyMeta {
            rerecord_count,
            pal: false,
        },
    ))
}

/// Import a `VirtuaNES` `.vmv` movie.
///
/// **Documentation-derived** (`TASVideos` `OtherEmulators/VMV`): `BizHawk` never
/// shipped a `.vmv` importer. Header layout per `VirtuaNES` 0.93: 12-byte magic, a
/// movie-data offset at `0x34`, a frame count at `0x38`, a controller-enable +
/// reset flag word at `0x10`, and a video-mode byte (`0`=NTSC, `1`=PAL) at
/// `0x23`. The per-frame record is one byte per enabled controller; the bit order
/// is the canonical NES order, so each byte maps straight to [`Buttons`].
///
/// We seek to the movie-data offset (rather than assuming a fixed header size) so
/// the parse is robust across the older header variants whose exact layout is not
/// authoritatively documented.
///
/// # Errors
///
/// [`LegacyMovieError`] for a bad magic, a save-state start, or a truncated body.
pub fn import_vmv(
    bytes: &[u8],
    rom_sha256: [u8; 32],
) -> Result<(Movie, LegacyMeta), LegacyMovieError> {
    const FMT: &str = "vmv";
    const MIN_HEADER: usize = 0x40;
    if bytes.len() < MIN_HEADER {
        return Err(LegacyMovieError::Truncated {
            expected: MIN_HEADER,
            got: bytes.len(),
        });
    }
    if &bytes[0..12] != VMV_MAGIC {
        return Err(LegacyMovieError::BadMagic { format: FMT });
    }
    let flags = rd_u32_le(bytes, 0x10).unwrap_or(0);
    // bits 0..3 = controllers 1..4 enabled; bit6 = reset-based (1) vs
    // save-state-based (0).
    let reset_based = flags & (1 << 6) != 0;
    if !reset_based {
        return Err(LegacyMovieError::Unsupported {
            format: FMT,
            reason: "begins from a save-state (cross-emulator save states are not portable)",
        });
    }
    let ctrl_count = (usize::from(flags & 0x1 != 0))
        + usize::from(flags & 0x2 != 0)
        + usize::from(flags & 0x4 != 0)
        + usize::from(flags & 0x8 != 0);
    // Default to a single controller if the flag word declares none (some 0.93
    // movies leave the bits clear and imply P1).
    let ctrl_count = ctrl_count.max(1);
    let rerecord_count = u64::from(rd_u32_le(bytes, 0x1C).unwrap_or(0));
    // Video mode byte: 0 = NTSC, 1 = PAL.
    let pal = bytes[0x23] == 1;
    let frame_count = rd_u32_le(bytes, 0x38).unwrap_or(0) as usize;
    let data_off = rd_u32_le(bytes, 0x34).unwrap_or(0) as usize;
    let data_off = if data_off == 0 || data_off > bytes.len() {
        // Fall back to the documented 0.93 reset-based offset.
        MIN_HEADER
    } else {
        data_off
    };

    let body = &bytes[data_off..];
    // Honour the header frame count when present, else derive from the body size.
    let derived = body.len() / ctrl_count;
    let frame_count = if frame_count == 0 {
        derived
    } else {
        frame_count.min(derived)
    };
    let mut frames = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let base = f * ctrl_count;
        let p1 = Buttons::from_bits_truncate(*body.get(base).unwrap_or(&0));
        let p2 = if ctrl_count >= 2 {
            Buttons::from_bits_truncate(*body.get(base + 1).unwrap_or(&0))
        } else {
            Buttons::empty()
        };
        frames.push(FrameInput::new(p1, p2));
    }

    let movie = Movie {
        region: if pal { Region::Pal } else { Region::Ntsc },
        rom_sha256,
        start: StartPoint::PowerOn,
        frames,
    };
    Ok((
        movie,
        LegacyMeta {
            rerecord_count,
            pal,
        },
    ))
}

/// "Import" a `Mednafen` `.mc2` movie — always an error.
///
/// `.mc2` (`PCEjin` / `Mednafen`) is a **PC Engine** movie format (PCE buttons
/// `B1/B2/Run/Select`, platform PCE/PCECD), not an NES container. It carries no
/// NES gamepad data, so there is nothing to map. This entry point exists so the
/// frontend dispatcher can give a precise diagnostic instead of mis-parsing.
///
/// # Errors
///
/// Always [`LegacyMovieError::Unsupported`].
pub const fn import_mc2(
    _bytes: &[u8],
    _rom_sha256: [u8; 32],
) -> Result<(Movie, LegacyMeta), LegacyMovieError> {
    Err(LegacyMovieError::Unsupported {
        format: "mc2",
        reason: "`.mc2` is a PC Engine (PCEjin/Mednafen) movie, not an NES movie",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const TEST_SHA: [u8; 32] = [0x33; 32];

    /// Build a minimal `.fcm` header with the given flags, frame count, and
    /// input stream (placed right after a 0x34-byte header).
    fn synth_fcm(flags: u8, frame_count: u32, stream: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 0x34];
        b[0..4].copy_from_slice(FCM_MAGIC);
        b[0x04..0x08].copy_from_slice(&FCM_VERSION.to_le_bytes());
        b[0x08] = flags;
        b[0x0C..0x10].copy_from_slice(&frame_count.to_le_bytes());
        b[0x10..0x14].copy_from_slice(&7u32.to_le_bytes()); // rerecord
        let first_frame = u32::try_from(b.len()).unwrap();
        b[0x1C..0x20].copy_from_slice(&first_frame.to_le_bytes());
        b.extend_from_slice(stream);
        b
    }

    #[test]
    fn fcm_rejects_bad_magic_and_version() {
        let mut b = synth_fcm(0x02, 0, &[]);
        b[0] = b'X';
        assert!(matches!(
            import_fcm(&b, TEST_SHA),
            Err(LegacyMovieError::BadMagic { .. })
        ));
        let mut b = synth_fcm(0x02, 0, &[]);
        b[0x04] = 9; // version 9
        assert!(matches!(
            import_fcm(&b, TEST_SHA),
            Err(LegacyMovieError::BadVersion { .. })
        ));
    }

    #[test]
    fn fcm_rejects_savestate_start() {
        // flags bit1 clear -> save-state-based.
        let b = synth_fcm(0x00, 0, &[]);
        assert!(matches!(
            import_fcm(&b, TEST_SHA),
            Err(LegacyMovieError::Unsupported { .. })
        ));
    }

    #[test]
    fn fcm_toggle_stream_decodes() {
        // reset-based, NTSC. Stream:
        //   byte 0x07 -> controller update, player 0, button idx 7 = RIGHT toggle
        //                (delta 0). Emits 1 frame with RIGHT held.
        //   byte 0x07 -> toggles RIGHT off again. Emits 1 frame with nothing.
        // frame_count = 2.
        let stream = [0x07u8, 0x07u8];
        let b = synth_fcm(0x02, 2, &stream);
        let (movie, meta) = import_fcm(&b, TEST_SHA).expect("fcm import");
        assert_eq!(movie.region, Region::Ntsc);
        assert_eq!(movie.frames.len(), 2);
        assert_eq!(movie.frames[0].p1, Buttons::RIGHT);
        assert_eq!(movie.frames[1].p1, Buttons::empty());
        assert_eq!(meta.rerecord_count, 7);
        assert_eq!(movie.start, StartPoint::PowerOn);
    }

    #[test]
    fn fcm_delta_advances_frames() {
        // A control byte (bit7) with delta count 1 and a 1-byte delta of 3:
        //   update = 1010_0000 = 0xA0 -> bit7 set (command), delta_bytes=1.
        //   delta byte 0x03 -> advance 3 frames (held = empty), then emit 1 frame
        //   for the command. Total 4 frames.
        let stream = [0xA0u8, 0x03u8];
        let b = synth_fcm(0x02, 4, &stream);
        let (movie, _) = import_fcm(&b, TEST_SHA).expect("fcm import");
        assert_eq!(movie.frames.len(), 4);
        assert!(movie.frames.iter().all(|f| f.p1 == Buttons::empty()));
    }

    #[test]
    fn fcm_pal_flag() {
        let b = synth_fcm(0x02 | 0x04, 0, &[]);
        let (movie, meta) = import_fcm(&b, TEST_SHA).expect("fcm import");
        assert_eq!(movie.region, Region::Pal);
        assert!(meta.pal);
    }

    /// Build a `.fmv` with the given flags2 and a body of raw per-frame bytes.
    fn synth_fmv(flags1: u8, flags2: u8, body: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; FMV_HEADER_LEN];
        b[0..4].copy_from_slice(FMV_MAGIC);
        b[0x04] = flags1;
        b[0x05] = flags2;
        b[0x0A..0x0E].copy_from_slice(&4u32.to_le_bytes()); // rerecord-1 = 4 -> 5
        b.extend_from_slice(body);
        b
    }

    #[test]
    fn fmv_p1_only_full_dump() {
        // flags2 bit7 = P1 present. Two frames: A then RIGHT.
        // Famtasia bits: A=0x20, RIGHT=0x01.
        let body = [0x20u8, 0x01u8];
        let b = synth_fmv(0x00, 0x80, &body);
        let (movie, meta) = import_fmv(&b, TEST_SHA).expect("fmv import");
        assert_eq!(movie.frames.len(), 2);
        assert_eq!(movie.frames[0].p1, Buttons::A);
        assert_eq!(movie.frames[1].p1, Buttons::RIGHT);
        assert_eq!(movie.region, Region::Ntsc);
        assert_eq!(meta.rerecord_count, 5);
    }

    #[test]
    fn fmv_two_controllers_interleave() {
        // P1 + P2 present (bits 7 and 6). One frame: P1=B (0x10), P2=START (0x80).
        let body = [0x10u8, 0x80u8];
        let b = synth_fmv(0x00, 0xC0, &body);
        let (movie, _) = import_fmv(&b, TEST_SHA).expect("fmv import");
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(movie.frames[0].p1, Buttons::B);
        assert_eq!(movie.frames[0].p2, Buttons::START);
    }

    #[test]
    fn fmv_rejects_savestate() {
        let b = synth_fmv(0x04, 0x80, &[]);
        assert!(matches!(
            import_fmv(&b, TEST_SHA),
            Err(LegacyMovieError::Unsupported { .. })
        ));
    }

    #[test]
    fn fmv_byte_permutation_is_correct() {
        assert_eq!(fmv_byte_to_buttons(0x01), Buttons::RIGHT);
        assert_eq!(fmv_byte_to_buttons(0x20), Buttons::A);
        assert_eq!(fmv_byte_to_buttons(0x10), Buttons::B);
        assert_eq!(fmv_byte_to_buttons(0x80), Buttons::START);
        assert_eq!(
            fmv_byte_to_buttons(0xFF),
            Buttons::all(),
            "all bits set -> all buttons"
        );
    }

    /// Build a `.vmv` (0.93-style) with the given flag word + video mode + a
    /// per-frame body. Data offset points right after the 0x40 header.
    fn synth_vmv(flags: u32, video_mode: u8, frame_count: u32, body: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 0x40];
        b[0..12].copy_from_slice(VMV_MAGIC);
        b[0x10..0x14].copy_from_slice(&flags.to_le_bytes());
        b[0x1C..0x20].copy_from_slice(&11u32.to_le_bytes()); // rerecord
        b[0x23] = video_mode;
        let data_off = u32::try_from(b.len()).unwrap();
        b[0x34..0x38].copy_from_slice(&data_off.to_le_bytes());
        b[0x38..0x3C].copy_from_slice(&frame_count.to_le_bytes());
        b.extend_from_slice(body);
        b
    }

    #[test]
    fn vmv_canonical_bit_order() {
        // reset-based (bit6) + P1 enabled (bit0). One frame: A | RIGHT.
        // VMV canonical order = RustyNES Buttons layout: A=0x01, RIGHT=0x80.
        let flags = (1u32 << 6) | 0x1;
        let body = [Buttons::A.bits() | Buttons::RIGHT.bits()];
        let b = synth_vmv(flags, 0, 1, &body);
        let (movie, meta) = import_vmv(&b, TEST_SHA).expect("vmv import");
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(movie.frames[0].p1, Buttons::A | Buttons::RIGHT);
        assert_eq!(movie.region, Region::Ntsc);
        assert_eq!(meta.rerecord_count, 11);
    }

    #[test]
    fn vmv_pal_video_mode() {
        let flags = (1u32 << 6) | 0x1;
        let b = synth_vmv(flags, 1, 1, &[0u8]);
        let (movie, meta) = import_vmv(&b, TEST_SHA).expect("vmv import");
        assert_eq!(movie.region, Region::Pal);
        assert!(meta.pal);
    }

    #[test]
    fn vmv_rejects_savestate() {
        // bit6 clear -> save-state-based.
        let b = synth_vmv(0x1, 0, 1, &[0u8]);
        assert!(matches!(
            import_vmv(&b, TEST_SHA),
            Err(LegacyMovieError::Unsupported { .. })
        ));
    }

    #[test]
    fn vmv_rejects_bad_magic() {
        let mut b = synth_vmv((1u32 << 6) | 1, 0, 1, &[0u8]);
        b[0] = b'X';
        assert!(matches!(
            import_vmv(&b, TEST_SHA),
            Err(LegacyMovieError::BadMagic { .. })
        ));
    }

    #[test]
    fn mc2_is_rejected_as_pce() {
        assert!(matches!(
            import_mc2(&[0u8; 16], TEST_SHA),
            Err(LegacyMovieError::Unsupported { format: "mc2", .. })
        ));
    }

    #[test]
    fn truncated_inputs_never_panic() {
        assert!(matches!(
            import_fcm(&[0u8; 4], TEST_SHA),
            Err(LegacyMovieError::Truncated { .. })
        ));
        assert!(matches!(
            import_fmv(&[0u8; 4], TEST_SHA),
            Err(LegacyMovieError::Truncated { .. })
        ));
        assert!(matches!(
            import_vmv(&[0u8; 4], TEST_SHA),
            Err(LegacyMovieError::Truncated { .. })
        ));
    }
}
