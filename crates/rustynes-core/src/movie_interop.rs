//! FCEUX `.fm2` movie interop: import + export of FCEUX's plain-text TAS
//! movie format to and from the native [`Movie`] type.
//!
//! `.fm2` is ASCII text (see `ref-proj/fceux/documentation/fm2.txt`): a block
//! of `key value` header lines (the first of which must be `version 3`),
//! followed by an input-log section whose every line begins and ends with a
//! `|` (pipe). The movie length is implicit -- it is the number of input-log
//! lines (FCEUX `.fm2` note A).
//!
//! # Scope and deliberate limitations
//!
//! - **Standard gamepads only.** `RustyNES`'s input model here maps FCEUX
//!   `SI_GAMEPAD` ports onto [`FrameInput`]. A `port0`/`port1` declaring a
//!   zapper (`SI_ZAPPER = 2`) is rejected with [`Fm2Error::Unsupported`]
//!   rather than silently mis-mapped.
//! - **Power-on start only.** A `savestate`-anchored `.fm2` is rejected on
//!   import (cross-emulator save-state blobs are not portable), and a
//!   [`StartPoint::SaveState`] [`Movie`] is rejected on export. Both surface
//!   [`Fm2Error::Unsupported`].
//! - **Two controllers stored.** [`FrameInput`] models players 1 and 2 only.
//!   A `fourscore` `.fm2` (four pads) is imported by keeping pads 1 and 2 and
//!   dropping pads 3 and 4; the fourscore flag is preserved in [`Fm2Meta`] so
//!   the caller is not silently misled. (TODO: carry P3/P4 once `FrameInput`
//!   grows beyond two ports.)
//! - **Soft reset has no home on [`FrameInput`].** The per-frame command
//!   field's `MOVIECMD_RESET` bit (value 1) is parsed without error but is
//!   *not* applied to any frame today; see [`import_fm2`].
//!
//! # The `RLDUTSBA` pad order (a classic footgun)
//!
//! Each gamepad field is exactly eight characters. Per FCEUX, the column
//! order is the deliberately-reversed `RLDUTSBA` = Right, Left, Down, Up,
//! sTart, Select, B, A (kept for back-compat with FCEUX's first release). So
//! character index 0 is the Right button and index 7 is the A button. A
//! character of `' '` (space) or `'.'` means released; any other character
//! (conventionally the button's own mnemonic letter) means pressed.
//!
//! This module is `no_std`-clean: it uses only `core` + `alloc`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::Region;
use crate::controller::Buttons;
use crate::movie::{FrameInput, Movie, StartPoint};
use thiserror::Error;

/// The only FCEUX `.fm2` format version this module understands.
pub const FM2_VERSION: u32 = 3;

/// FCEUX `port0`/`port1` value for a standard gamepad (`SI_GAMEPAD`).
const SI_GAMEPAD: u32 = 1;

/// FCEUX per-frame command bit: a soft reset occurred at the start of the
/// frame (`MOVIECMD_RESET`).
const MOVIECMD_RESET: u32 = 1;

/// The eight-character gamepad column order used by `.fm2`, paired with the
/// [`Buttons`] flag each column drives. Index 0 is the first character of a
/// pad field. Order is FCEUX's reversed `RLDUTSBA`.
const PAD_COLUMNS: [Buttons; 8] = [
    Buttons::RIGHT,  // index 0: R
    Buttons::LEFT,   // index 1: L
    Buttons::DOWN,   // index 2: D
    Buttons::UP,     // index 3: U
    Buttons::START,  // index 4: T (sTart)
    Buttons::SELECT, // index 5: S
    Buttons::B,      // index 6: B
    Buttons::A,      // index 7: A
];

/// Header metadata parsed from an `.fm2` that has no home on [`Movie`] yet.
///
/// [`Movie`] carries only the data the native `.rnm` format needs (region,
/// ROM hash, start point, frames). The remaining `.fm2` header fields are
/// returned here so the caller can surface or persist them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Fm2Meta {
    /// The `rerecordCount` header value (0 if absent).
    pub rerecord_count: u64,
    /// The movie author, taken from a `comment author <name>` line if present.
    pub author: Option<String>,
    /// The `romFilename` header value, if present.
    pub rom_filename: Option<String>,
    /// The `romChecksum` header value, stored verbatim (an MD5, `base64:`- or
    /// hex-encoded). Not validated against the ROM -- the SHA-256 identity is
    /// supplied separately by the caller.
    pub rom_checksum_md5: Option<String>,
    /// `true` if the movie declared `fourscore 1` (four controllers). When
    /// set, only pads 1 and 2 made it into the [`Movie`]; pads 3 and 4 were
    /// dropped (see the module docs).
    pub fourscore: bool,
    /// `true` if the movie declared `palFlag 1`.
    pub pal: bool,
}

/// Errors produced by `.fm2` import / export.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Fm2Error {
    /// The header had no `version` line, or it was not the first key.
    #[error("fm2 missing required `version` header (must be the first key)")]
    MissingVersion,

    /// The `version` value was not [`FM2_VERSION`].
    #[error("fm2 version {got} not supported (only version {} is)", FM2_VERSION)]
    BadVersion {
        /// The version value we read.
        got: u32,
    },

    /// A header line declared an integer key whose value did not parse.
    #[error("fm2 header key `{key}` has an invalid integer value `{value}`")]
    BadInteger {
        /// The offending key.
        key: String,
        /// The text we failed to parse as an integer.
        value: String,
    },

    /// A structural problem with an input-log line (missing pipes, wrong
    /// field count, or a pad field of the wrong length). `line` is the
    /// 1-based input-log line number.
    #[error("fm2 malformed input-log line {line}: {reason}")]
    Malformed {
        /// 1-based index of the offending input-log line.
        line: usize,
        /// Human-readable description of what was wrong.
        reason: &'static str,
    },

    /// A feature of the `.fm2` (or of the [`Movie`] being exported) that this
    /// module deliberately does not support.
    #[error("fm2 unsupported: {0}")]
    Unsupported(&'static str),
}

/// Options the caller supplies on export that the [`Movie`] itself does not
/// carry. Mirrors the extra header fields surfaced by [`Fm2Meta`] on import.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Fm2ExportOpts {
    /// Value to emit for the `rerecordCount` header.
    pub rerecord_count: u64,
    /// Author to emit as a `comment author <name>` line, if any.
    pub author: Option<String>,
    /// Value to emit for the `romFilename` header, if any.
    pub rom_filename: Option<String>,
    /// Value to emit for the `romChecksum` header, if any.
    pub rom_checksum_md5: Option<String>,
    /// Emit `fourscore 1` and four pad columns per line when `true`. The
    /// extra pads (3 and 4) are always released, since [`FrameInput`] models
    /// only two controllers.
    pub fourscore: bool,
}

/// Parse `.fm2` text into a [`Movie`] plus the leftover header [`Fm2Meta`].
///
/// `rom_sha256` is the SHA-256 of the ROM the caller intends to replay the
/// movie against. The `.fm2` format carries only an MD5 (`romChecksum`), so
/// the authoritative SHA-256 ROM identity must come from the loaded ROM; it is
/// stored verbatim on the returned [`Movie`] and is *not* validated here.
///
/// The returned [`Movie`] always has [`StartPoint::PowerOn`] -- FCEUX note B
/// says movies start from power-on unless a `savestate` key is present, and
/// such cross-emulator save-state blobs are not portable, so a `savestate`
/// header is rejected.
///
/// # Soft reset handling
///
/// The per-frame command field's `MOVIECMD_RESET` bit (value 1) is parsed (so
/// such lines do not error) but is **not** represented anywhere on the
/// resulting [`Movie`], because [`FrameInput`] has no reset bit. A reset
/// command therefore affects neither the frame count nor playback today.
///
/// # Errors
///
/// Returns [`Fm2Error`] for a missing/wrong `version`, an unparseable integer
/// header, an unsupported device or `savestate` start point, or a malformed
/// input-log line (bad pipes, wrong field count, wrong pad length). Never
/// panics on malformed input.
pub fn import_fm2(text: &str, rom_sha256: [u8; 32]) -> Result<(Movie, Fm2Meta), Fm2Error> {
    let mut meta = Fm2Meta::default();
    let mut saw_version = false;
    let mut port0_gamepad = true;
    let mut port1_gamepad = true;
    let mut frames: Vec<FrameInput> = Vec::new();
    let mut input_line_no = 0usize;

    for raw in text.lines() {
        // Trim a trailing '\r' so CRLF and LF both work; leave interior
        // content alone.
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if line.starts_with('|') {
            // Input-log line.
            input_line_no += 1;
            let input = parse_input_line(line, input_line_no, meta.fourscore)?;
            frames.push(input);
            continue;
        }

        // Header line. Blank lines in the header are tolerated.
        if line.trim().is_empty() {
            continue;
        }

        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            // A bare key with no value (e.g. an empty string field): treat the
            // value as empty.
            None => (line, ""),
        };

        // `version` must be the very first header key.
        if !saw_version && key != "version" {
            return Err(Fm2Error::MissingVersion);
        }

        match key {
            "version" => {
                let v = parse_int(key, value)?;
                if v != FM2_VERSION {
                    return Err(Fm2Error::BadVersion { got: v });
                }
                saw_version = true;
            }
            "rerecordCount" => meta.rerecord_count = u64::from(parse_int(key, value)?),
            "palFlag" => meta.pal = parse_int(key, value)? != 0,
            "fourscore" => meta.fourscore = parse_int(key, value)? != 0,
            "port0" => port0_gamepad = parse_int(key, value)? == SI_GAMEPAD,
            "port1" => port1_gamepad = parse_int(key, value)? == SI_GAMEPAD,
            "port2" => {
                // SIFC_NONE = 0 is the only expansion-port device we model.
                let _ = parse_int(key, value)?;
            }
            "romFilename" => meta.rom_filename = Some(value.to_string()),
            "romChecksum" => meta.rom_checksum_md5 = Some(value.to_string()),
            "savestate" => {
                return Err(Fm2Error::Unsupported(
                    "savestate-anchored .fm2 (cross-emulator save states are not portable)",
                ));
            }
            "comment" => {
                // By convention `comment author <name>` carries the author.
                if let Some(rest) = value.strip_prefix("author ") {
                    meta.author = Some(rest.to_string());
                }
            }
            _ => {
                // `emuVersion`, `guid`, and any unknown header keys are
                // ignored (forward-compatible).
            }
        }
    }

    if !saw_version {
        return Err(Fm2Error::MissingVersion);
    }
    // Reject non-gamepad standard ports only after we know `version` was OK,
    // so the error reflects the real obstacle. Fourscore implies all-gamepad
    // (FCEUX note C), so the port checks only matter when not fourscore.
    if !meta.fourscore && (!port0_gamepad || !port1_gamepad) {
        return Err(Fm2Error::Unsupported(
            "non-gamepad input device (only SI_GAMEPAD ports are supported)",
        ));
    }

    let movie = Movie {
        region: if meta.pal { Region::Pal } else { Region::Ntsc },
        rom_sha256,
        start: StartPoint::PowerOn,
        frames,
    };
    Ok((movie, meta))
}

/// Serialize a [`Movie`] to `.fm2` text.
///
/// Emits a `version 3` header, then `emuVersion`, `rerecordCount`, `palFlag`
/// (from [`Movie::region`] -- both [`Region::Pal`] and [`Region::Dendy`] are
/// PAL-timed, so both export `palFlag 1`), `fourscore`, `port0`/`port1`/`port2`
/// (all gamepad / none), the optional `romFilename` / `romChecksum`, an
/// optional `comment author` line, then the `|c|RLDUTSBA|RLDUTSBA||` input log
/// (one line per frame, with a trailing empty `port2` field per the spec).
///
/// Only [`StartPoint::PowerOn`] movies export; a [`StartPoint::SaveState`]
/// movie has no portable `.fm2` representation.
///
/// # Errors
///
/// Returns [`Fm2Error::Unsupported`] if `movie` is anchored to an embedded
/// save state.
pub fn export_fm2(movie: &Movie, opts: &Fm2ExportOpts) -> Result<String, Fm2Error> {
    if !matches!(movie.start, StartPoint::PowerOn) {
        return Err(Fm2Error::Unsupported(
            "save-state-anchored movie has no portable .fm2 representation",
        ));
    }

    let pal = matches!(movie.region, Region::Pal | Region::Dendy);
    let mut out = String::new();

    // Header. `version` must be first. Writing into a `String` via the
    // `core::fmt::Write` impl is infallible, so the `write!` results are
    // discarded.
    out.push_str("version 3\n");
    let _ = writeln!(out, "emuVersion {}", emu_version_tag());
    let _ = writeln!(out, "rerecordCount {}", opts.rerecord_count);
    let _ = writeln!(out, "palFlag {}", u8::from(pal));
    let _ = writeln!(out, "fourscore {}", u8::from(opts.fourscore));
    let _ = writeln!(out, "port0 {SI_GAMEPAD}");
    let _ = writeln!(out, "port1 {SI_GAMEPAD}");
    out.push_str("port2 0\n");
    if let Some(name) = &opts.rom_filename {
        let _ = writeln!(out, "romFilename {name}");
    }
    if let Some(sum) = &opts.rom_checksum_md5 {
        let _ = writeln!(out, "romChecksum {sum}");
    }
    if let Some(author) = &opts.author {
        let _ = writeln!(out, "comment author {author}");
    }

    // Input log: one line per frame. RustyNES movies never carry a per-frame
    // reset command, so field `c` is always 0.
    let mut pad = [0u8; 8];
    for frame in &movie.frames {
        out.push_str("|0|");
        write_pad(frame.p1, &mut pad);
        out.push_str(core::str::from_utf8(&pad).expect("pad bytes are ASCII"));
        out.push('|');
        write_pad(frame.p2, &mut pad);
        out.push_str(core::str::from_utf8(&pad).expect("pad bytes are ASCII"));
        out.push('|');
        if opts.fourscore {
            // Players 3 and 4 are always released (FrameInput has no P3/P4).
            write_pad(Buttons::empty(), &mut pad);
            let empty = core::str::from_utf8(&pad).expect("pad bytes are ASCII");
            out.push_str(empty);
            out.push('|');
            out.push_str(empty);
            out.push('|');
        }
        // Trailing empty `port2` field (SIFC_NONE is always empty).
        out.push_str("|\n");
    }

    Ok(out)
}

/// Render `buttons` into an eight-byte `RLDUTSBA` pad field. A pressed button
/// is written as its mnemonic letter; a released one as `'.'`.
fn write_pad(buttons: Buttons, out: &mut [u8; 8]) {
    // Mnemonic letters in column order, matching `PAD_COLUMNS`.
    const LETTERS: [u8; 8] = [b'R', b'L', b'D', b'U', b'T', b'S', b'B', b'A'];
    for i in 0..8 {
        out[i] = if buttons.contains(PAD_COLUMNS[i]) {
            LETTERS[i]
        } else {
            b'.'
        };
    }
}

/// Parse a single input-log line (already known to start with `|`) into a
/// [`FrameInput`]. `line_no` is the 1-based input-log line number used in
/// errors; `fourscore` selects the 4-pad layout.
fn parse_input_line(line: &str, line_no: usize, fourscore: bool) -> Result<FrameInput, Fm2Error> {
    if !line.ends_with('|') {
        return Err(Fm2Error::Malformed {
            line: line_no,
            reason: "input-log line must end with `|`",
        });
    }
    // `|c|p0|p1|port2|` splits (on `|`) to ["", c, p0, p1, port2, ""]; the
    // fourscore form has p2/p3 between p1 and port2. Both leading and trailing
    // empty strings are expected.
    let mut fields = line.split('|');
    // Leading empty field (before the first `|`).
    if fields.next() != Some("") {
        return Err(Fm2Error::Malformed {
            line: line_no,
            reason: "input-log line must start with `|`",
        });
    }
    // Command field.
    let cmd_field = fields.next().ok_or(Fm2Error::Malformed {
        line: line_no,
        reason: "missing command field",
    })?;
    let _cmd = parse_command(cmd_field, line_no)?;

    let pad_count = if fourscore { 4 } else { 2 };
    let mut pads = [Buttons::empty(); 4];
    for pad in pads.iter_mut().take(pad_count) {
        let field = fields.next().ok_or(Fm2Error::Malformed {
            line: line_no,
            reason: "missing gamepad field",
        })?;
        *pad = parse_pad(field, line_no)?;
    }

    // Remaining fields: the `port2` field then the trailing empty string. We
    // tolerate the `port2` field being present-and-empty (SIFC_NONE) or
    // omitted entirely, but anything non-empty there is unsupported.
    for field in fields {
        if !field.is_empty() {
            return Err(Fm2Error::Malformed {
                line: line_no,
                reason: "unexpected non-empty trailing field (only SIFC_NONE supported)",
            });
        }
    }

    // pads[0] = P1, pads[1] = P2 (pads 2/3 dropped for fourscore).
    Ok(FrameInput::new(pads[0], pads[1]))
}

/// Parse the variable-length decimal command bitfield. Returns whether the
/// reset bit was set (currently informational only).
fn parse_command(field: &str, line_no: usize) -> Result<bool, Fm2Error> {
    // The command field is conventionally empty or a small decimal integer.
    let value: u32 = if field.is_empty() {
        0
    } else {
        field.parse().map_err(|_| Fm2Error::Malformed {
            line: line_no,
            reason: "command field is not a decimal integer",
        })?
    };
    Ok(value & MOVIECMD_RESET != 0)
}

/// Parse one eight-character `RLDUTSBA` gamepad field into [`Buttons`].
fn parse_pad(field: &str, line_no: usize) -> Result<Buttons, Fm2Error> {
    let bytes = field.as_bytes();
    if bytes.len() != 8 {
        return Err(Fm2Error::Malformed {
            line: line_no,
            reason: "gamepad field must be exactly 8 characters",
        });
    }
    let mut buttons = Buttons::empty();
    for (i, &b) in bytes.iter().enumerate() {
        // Space or '.' = released; anything else = pressed.
        if b != b' ' && b != b'.' {
            buttons |= PAD_COLUMNS[i];
        }
    }
    Ok(buttons)
}

/// Parse an integer-typed header value, attaching the key for diagnostics.
fn parse_int(key: &str, value: &str) -> Result<u32, Fm2Error> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| Fm2Error::BadInteger {
            key: key.to_string(),
            value: value.to_string(),
        })
}

/// The `emuVersion` tag emitted on export. An FCEUX-style numeric emulator
/// version is not meaningful for a different emulator, so we emit a stable
/// sentinel that round-trips harmlessly (the importer ignores `emuVersion`).
const fn emu_version_tag() -> u32 {
    // RustyNES is not FCEUX; a fixed sentinel keeps export deterministic and
    // the field is ignored on import.
    20000
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const TEST_SHA: [u8; 32] = [0x5A; 32];

    /// A fixed, varied input sequence touching every button.
    fn varied_frames() -> Vec<FrameInput> {
        vec![
            FrameInput::new(Buttons::A, Buttons::B),
            FrameInput::new(Buttons::RIGHT | Buttons::A, Buttons::LEFT | Buttons::START),
            FrameInput::new(
                Buttons::UP | Buttons::DOWN | Buttons::SELECT,
                Buttons::empty(),
            ),
            FrameInput::new(
                Buttons::A | Buttons::B | Buttons::SELECT | Buttons::START,
                Buttons::UP | Buttons::DOWN | Buttons::LEFT | Buttons::RIGHT,
            ),
        ]
    }

    #[test]
    fn round_trip_power_on_ntsc() {
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: TEST_SHA,
            start: StartPoint::PowerOn,
            frames: varied_frames(),
        };
        let opts = Fm2ExportOpts {
            rerecord_count: 42,
            author: Some("tester".to_string()),
            rom_filename: Some("game.nes".to_string()),
            rom_checksum_md5: Some("base64:deadbeef".to_string()),
            fourscore: false,
        };
        let text = export_fm2(&movie, &opts).expect("export");
        let (back, meta) = import_fm2(&text, TEST_SHA).expect("import");

        assert_eq!(back.frames, movie.frames, "frames survive round-trip");
        assert_eq!(back.region, Region::Ntsc);
        assert_eq!(back.start, StartPoint::PowerOn);
        assert_eq!(back.rom_sha256, TEST_SHA);
        assert!(!meta.fourscore);
        assert!(!meta.pal);
        assert_eq!(meta.rerecord_count, 42);
        assert_eq!(meta.author.as_deref(), Some("tester"));
        assert_eq!(meta.rom_filename.as_deref(), Some("game.nes"));
        assert_eq!(meta.rom_checksum_md5.as_deref(), Some("base64:deadbeef"));
    }

    #[test]
    fn exact_bit_and_char_mapping() {
        // Only A set -> char index 7 pressed, others released.
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: TEST_SHA,
            start: StartPoint::PowerOn,
            frames: vec![
                FrameInput::new(Buttons::A, Buttons::empty()),
                FrameInput::new(Buttons::RIGHT, Buttons::empty()),
            ],
        };
        let text = export_fm2(&movie, &Fm2ExportOpts::default()).expect("export");
        // Pull the two input-log lines.
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with('|')).collect();
        assert_eq!(lines.len(), 2);

        // |0|<pad p1>|<pad p2>||  -> the first pad field is between pipe 2 & 3.
        let p1_field_a = lines[0].split('|').nth(2).unwrap();
        assert_eq!(p1_field_a.len(), 8);
        for (i, c) in p1_field_a.chars().enumerate() {
            if i == 7 {
                assert_ne!(c, '.', "A button is char index 7 and must be pressed");
            } else {
                assert_eq!(c, '.', "non-A columns must be released");
            }
        }

        let p1_field_right = lines[1].split('|').nth(2).unwrap();
        for (i, c) in p1_field_right.chars().enumerate() {
            if i == 0 {
                assert_ne!(c, '.', "RIGHT button is char index 0 and must be pressed");
            } else {
                assert_eq!(c, '.', "non-RIGHT columns must be released");
            }
        }

        // Import the reverse: a hand-built log with only index 0 (RIGHT) and
        // only index 7 (A) set, assert the right Buttons come back.
        let imported = "version 3\nport0 1\nport1 1\nport2 0\n\
                        |0|R.......|.......A||\n";
        let (movie, _) = import_fm2(imported, TEST_SHA).expect("import");
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(movie.frames[0].p1, Buttons::RIGHT);
        assert_eq!(movie.frames[0].p2, Buttons::A);
    }

    #[test]
    fn pal_flag_maps_to_region() {
        // Import: palFlag 1 -> Region::Pal.
        let text = "version 3\npalFlag 1\nport0 1\nport1 1\nport2 0\n|0|........|........||\n";
        let (movie, meta) = import_fm2(text, TEST_SHA).expect("import");
        assert_eq!(movie.region, Region::Pal);
        assert!(meta.pal);

        // Export of a Pal movie emits palFlag 1.
        let pal_movie = Movie {
            region: Region::Pal,
            rom_sha256: TEST_SHA,
            start: StartPoint::PowerOn,
            frames: vec![FrameInput::new(Buttons::empty(), Buttons::empty())],
        };
        let out = export_fm2(&pal_movie, &Fm2ExportOpts::default()).expect("export");
        assert!(
            out.lines().any(|l| l == "palFlag 1"),
            "Pal movie must export palFlag 1"
        );

        // Ntsc exports palFlag 0.
        let ntsc_movie = Movie {
            region: Region::Ntsc,
            ..pal_movie
        };
        let out = export_fm2(&ntsc_movie, &Fm2ExportOpts::default()).expect("export");
        assert!(out.lines().any(|l| l == "palFlag 0"));
    }

    #[test]
    fn reset_command_parses_without_error() {
        // c = 1 means MOVIECMD_RESET. We parse it (don't crash) but it is not
        // represented on FrameInput, so the frame is otherwise a normal frame.
        let text = "version 3\nport0 1\nport1 1\nport2 0\n|1|........|........||\n";
        let (movie, _) = import_fm2(text, TEST_SHA).expect("reset command must parse");
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(movie.frames[0].p1, Buttons::empty());
    }

    #[test]
    fn fourscore_layout_parses_two_of_four_pads() {
        // Four pad fields; only P1/P2 are retained. P1 = A, P2 = B, P3/P4 set
        // (and dropped). fourscore must survive in meta.
        let text = "version 3\nfourscore 1\nport0 1\nport1 1\nport2 0\n\
                    |0|.......A|......B.|R.......|.L......||\n";
        let (movie, meta) = import_fm2(text, TEST_SHA).expect("fourscore import");
        assert!(meta.fourscore);
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(movie.frames[0].p1, Buttons::A);
        assert_eq!(movie.frames[0].p2, Buttons::B);

        // Export with fourscore emits four pad fields.
        let out = export_fm2(
            &movie,
            &Fm2ExportOpts {
                fourscore: true,
                ..Default::default()
            },
        )
        .expect("export");
        let log_line = out.lines().find(|l| l.starts_with('|')).unwrap();
        // |0|p1|p2|p3|p4||  -> split has ["",0,p1,p2,p3,p4,"",""]; four of the
        // fields are 8-char pads.
        let pad_count = log_line.split('|').filter(|p| p.len() == 8).count();
        assert_eq!(pad_count, 4, "fourscore export must emit four pad fields");
    }

    #[test]
    fn malformed_inputs_never_panic() {
        // Missing version line entirely.
        assert!(matches!(
            import_fm2("emuVersion 1\nport0 1\n", TEST_SHA),
            Err(Fm2Error::MissingVersion)
        ));

        // First key is not version.
        assert!(matches!(
            import_fm2("palFlag 0\nversion 3\n", TEST_SHA),
            Err(Fm2Error::MissingVersion)
        ));

        // Wrong version.
        assert!(matches!(
            import_fm2("version 2\nport0 1\nport1 1\nport2 0\n", TEST_SHA),
            Err(Fm2Error::BadVersion { got: 2 })
        ));

        // A bad integer header value.
        assert!(matches!(
            import_fm2("version 3\npalFlag notanint\n", TEST_SHA),
            Err(Fm2Error::BadInteger { .. })
        ));

        // An input line that starts with `|` but does not end with one.
        assert!(matches!(
            import_fm2(
                "version 3\nport0 1\nport1 1\nport2 0\n|0|........|........\n",
                TEST_SHA
            ),
            Err(Fm2Error::Malformed { .. })
        ));

        // A truncated pad field (7 chars).
        assert!(matches!(
            import_fm2(
                "version 3\nport0 1\nport1 1\nport2 0\n|0|.......|........||\n",
                TEST_SHA
            ),
            Err(Fm2Error::Malformed { .. })
        ));

        // A zapper port is unsupported.
        assert!(matches!(
            import_fm2("version 3\nport0 2\nport1 1\nport2 0\n", TEST_SHA),
            Err(Fm2Error::Unsupported(_))
        ));

        // A savestate-anchored movie is unsupported.
        assert!(matches!(
            import_fm2("version 3\nsavestate 0xDEAD\nport0 1\n", TEST_SHA),
            Err(Fm2Error::Unsupported(_))
        ));
    }

    #[test]
    fn representative_header_parses() {
        let text = "version 3\n\
            emuVersion 22020\n\
            rerecordCount 1234\n\
            palFlag 0\n\
            fourscore 0\n\
            port0 1\n\
            port1 1\n\
            port2 0\n\
            romFilename Super Demo.nes\n\
            romChecksum base64:abc123==\n\
            comment author Jane Doe\n\
            comment subject A speedrun\n\
            guid 452DE2C3-EF43-2FA9-77AC-0677FC51543B\n\
            |0|........|........||\n\
            |0|.......A|........||\n";
        let (movie, meta) = import_fm2(text, TEST_SHA).expect("header parse");
        assert_eq!(movie.frames.len(), 2);
        assert_eq!(movie.region, Region::Ntsc);
        assert_eq!(meta.rerecord_count, 1234);
        assert!(!meta.fourscore);
        assert!(!meta.pal);
        assert_eq!(meta.author.as_deref(), Some("Jane Doe"));
        assert_eq!(meta.rom_filename.as_deref(), Some("Super Demo.nes"));
        assert_eq!(meta.rom_checksum_md5.as_deref(), Some("base64:abc123=="));
        // Frame 1 had A on P1.
        assert_eq!(movie.frames[1].p1, Buttons::A);
    }

    #[test]
    fn export_rejects_save_state_movie() {
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: TEST_SHA,
            start: StartPoint::SaveState(vec![1, 2, 3]),
            frames: vec![],
        };
        assert!(matches!(
            export_fm2(&movie, &Fm2ExportOpts::default()),
            Err(Fm2Error::Unsupported(_))
        ));
    }
}
