//! `BizHawk` `.bk2` movie interop.
//!
//! Import + export of the **text payload** of a `.bk2` archive (the
//! `Header.txt` + `Input Log.txt` members) to and from the native [`Movie`]
//! type. It mirrors the FCEUX [`crate::movie_interop`] `.fm2` design, with one
//! structural difference: a `.bk2` is a **ZIP archive**, not a flat text file.
//!
//! # `no_std` and the ZIP split
//!
//! The `rustynes-core` chip stack is `#![no_std]` (`core` + `alloc` only), so it
//! does **not** open or write ZIP containers — that needs `std` + a zip crate.
//! Instead, the core handles the part that is `no_std`-clean and shared across
//! every frontend: parsing / emitting the two text members. The frontend reads
//! the two members out of the `.bk2` ZIP (and writes them back into one) and
//! hands their string contents here. The split is the same reason `.fm2`'s text
//! parse lives in core while file I/O lives in the frontend.
//!
//! # The `.bk2` text format (the subset we model)
//!
//! - **`Header.txt`** — `Key Value` lines (space-separated). The keys we read:
//!   `Platform` (must be an NES family token), `rerecordCount`, `Author`,
//!   `GameName`, `SHA1` (stored verbatim; the authoritative SHA-256 ROM identity
//!   is supplied separately by the caller, exactly like `.fm2`'s MD5), and the
//!   region flag `PAL`. A `StartsFromSavestate`/`StartsFromSaveRam` movie is
//!   rejected (cross-emulator save blobs are not portable — same policy as
//!   `.fm2`).
//! - **`Input Log.txt`** — a `[Input]` ... `[/Input]` block. The first line is a
//!   `LogKey:` declaration listing the `|`-separated controller column groups;
//!   subsequent lines are per-frame input, each `|`-delimited, every button
//!   rendered as its mnemonic letter (pressed) or `.` (released). The first
//!   group is the console-buttons group (Reset / Power); then one group per
//!   controller port.
//!
//! # The NES gamepad mnemonic order
//!
//! `BizHawk`'s NES standard-controller mnemonics are `U D L R S s B A`
//! (Up, Down, Left, Right, Start, select, B, A — note the lower-case `s` for
//! Select, distinct from the upper-case `S` for Start). Any non-`.`/non-space
//! character in a column means that button is pressed; the column's *position*
//! (not the specific letter) selects the button, so we tolerate either the
//! canonical mnemonic letter or any other pressed marker.
//!
//! # Deliberate limitations (mirroring `.fm2`)
//!
//! - **Standard gamepads, players 1 and 2 only.** [`FrameInput`] models two
//!   ports; extra controller groups are parsed but dropped (their presence is
//!   not silently misleading — only P1/P2 are mapped). The console Reset bit is
//!   parsed but not represented on [`FrameInput`] (it has no reset bit), exactly
//!   as in `.fm2`.
//! - **Power-on start only.** See above.
//!
//! This module is `no_std`-clean: it uses only `core` + `alloc`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::Region;
use crate::controller::Buttons;
use crate::movie::{FrameInput, Movie, StartPoint};
use thiserror::Error;

/// The filename of the header member inside a `.bk2` ZIP.
pub const BK2_HEADER_MEMBER: &str = "Header.txt";

/// The filename of the input-log member inside a `.bk2` ZIP.
pub const BK2_INPUT_LOG_MEMBER: &str = "Input Log.txt";

/// The NES standard-controller mnemonic column order, paired with the
/// [`Buttons`] flag each column drives. `BizHawk` order: `U D L R S s B A`.
const PAD_COLUMNS: [(u8, Buttons); 8] = [
    (b'U', Buttons::UP),
    (b'D', Buttons::DOWN),
    (b'L', Buttons::LEFT),
    (b'R', Buttons::RIGHT),
    (b'S', Buttons::START),  // upper-case S = Start
    (b's', Buttons::SELECT), // lower-case s = Select
    (b'B', Buttons::B),
    (b'A', Buttons::A),
];

/// Header metadata parsed from a `.bk2` that has no home on [`Movie`].
///
/// Mirrors [`crate::movie_interop::Fm2Meta`]: [`Movie`] carries only what `.rnm`
/// needs; the rest is surfaced here for the caller to display or persist.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Bk2Meta {
    /// The `rerecordCount` header value (0 if absent).
    pub rerecord_count: u64,
    /// The movie author (`Author` header), if present.
    pub author: Option<String>,
    /// The `GameName` header value, if present.
    pub game_name: Option<String>,
    /// The `SHA1` header value, stored verbatim (a hex SHA-1). Not validated
    /// against the ROM — the authoritative SHA-256 identity is supplied
    /// separately by the caller.
    pub sha1: Option<String>,
    /// `true` if the header declared a PAL region (`PAL 1`).
    pub pal: bool,
}

/// Options the caller supplies on export that the [`Movie`] does not carry.
/// Mirrors the extra header fields surfaced by [`Bk2Meta`] on import.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Bk2ExportOpts {
    /// Value to emit for the `rerecordCount` header.
    pub rerecord_count: u64,
    /// Author to emit as an `Author` line, if any.
    pub author: Option<String>,
    /// Value to emit for the `GameName` header, if any.
    pub game_name: Option<String>,
    /// Value to emit for the `SHA1` header, if any.
    pub sha1: Option<String>,
}

/// The two text members of a `.bk2` ZIP, returned by [`export_bk2`] for the
/// frontend to pack into the archive (and accepted by [`import_bk2`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bk2Text {
    /// The `Header.txt` contents.
    pub header: String,
    /// The `Input Log.txt` contents.
    pub input_log: String,
}

/// Errors produced by `.bk2` text import / export.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Bk2Error {
    /// The header declared a platform that is not an NES family.
    #[error("bk2 platform `{0}` is not an NES family movie")]
    WrongPlatform(String),

    /// A header line declared an integer key whose value did not parse.
    #[error("bk2 header key `{key}` has an invalid integer value `{value}`")]
    BadInteger {
        /// The offending key.
        key: String,
        /// The text we failed to parse as an integer.
        value: String,
    },

    /// The input log had no `LogKey:` declaration line.
    #[error("bk2 input log missing its `LogKey:` declaration")]
    MissingLogKey,

    /// A structural problem with an input-log line. `line` is the 1-based
    /// input-frame line number.
    #[error("bk2 malformed input-log line {line}: {reason}")]
    Malformed {
        /// 1-based index of the offending input-frame line.
        line: usize,
        /// Human-readable description of what was wrong.
        reason: &'static str,
    },

    /// A feature of the `.bk2` (or of the [`Movie`] being exported) that this
    /// module deliberately does not support.
    #[error("bk2 unsupported: {0}")]
    Unsupported(&'static str),
}

/// Parse the `Header.txt` + `Input Log.txt` text of a `.bk2` into a [`Movie`]
/// plus the leftover header [`Bk2Meta`].
///
/// `rom_sha256` is the SHA-256 of the ROM the caller intends to replay against.
/// `.bk2` carries only a SHA-1 (`SHA1` header), so the authoritative SHA-256
/// identity must come from the loaded ROM; it is stored verbatim on the returned
/// [`Movie`] and is *not* validated here.
///
/// The returned [`Movie`] always has [`StartPoint::PowerOn`] — `.bk2` movies
/// start from power-on unless a `StartsFromSavestate`/`StartsFromSaveRam` flag is
/// set, and such cross-emulator save blobs are not portable, so they are
/// rejected. This reuses the **canonical movie-import power-on alignment** the
/// `.fm2` path established (a deterministic zeroed-RAM cold boot via
/// [`Movie::seek_to_start`]), so imports never desync.
///
/// # Errors
///
/// Returns [`Bk2Error`] for a non-NES platform, an unparseable integer header, a
/// missing `LogKey:`, an unsupported save-anchored start, or a malformed
/// input-log line. Never panics on malformed input.
pub fn import_bk2(
    header: &str,
    input_log: &str,
    rom_sha256: [u8; 32],
) -> Result<(Movie, Bk2Meta), Bk2Error> {
    let meta = parse_header(header)?;
    let frames = parse_input_log(input_log)?;
    let movie = Movie {
        region: if meta.pal { Region::Pal } else { Region::Ntsc },
        rom_sha256,
        start: StartPoint::PowerOn,
        frames,
        // Carry the `.bk2` rerecordCount through (saturating into the `.rnm` u32).
        rerecord_count: u32::try_from(meta.rerecord_count).unwrap_or(u32::MAX),
    };
    Ok((movie, meta))
}

/// Serialize a [`Movie`] into the two text members of a `.bk2` ZIP.
///
/// Emits a `Header.txt` (`MovieVersion`, `Platform NES`, region `PAL` flag,
/// `rerecordCount`, optional `Author` / `GameName` / `SHA1`) and an
/// `Input Log.txt` (`[Input]`, a `LogKey:` declaration, one `|`-delimited frame
/// line per frame, `[/Input]`). The frontend writes both into the archive.
///
/// Only [`StartPoint::PowerOn`] movies export; a [`StartPoint::SaveState`] movie
/// has no portable `.bk2` representation.
///
/// # Errors
///
/// Returns [`Bk2Error::Unsupported`] if `movie` is anchored to an embedded save
/// state.
pub fn export_bk2(movie: &Movie, opts: &Bk2ExportOpts) -> Result<Bk2Text, Bk2Error> {
    if !matches!(movie.start, StartPoint::PowerOn) {
        return Err(Bk2Error::Unsupported(
            "save-state-anchored movie has no portable .bk2 representation",
        ));
    }

    let pal = matches!(movie.region, Region::Pal | Region::Dendy);

    // --- Header.txt ---
    let mut header = String::new();
    header.push_str("MovieVersion BizHawk v2.0\n");
    header.push_str("Platform NES\n");
    if pal {
        header.push_str("PAL 1\n");
    }
    let _ = writeln!(header, "rerecordCount {}", opts.rerecord_count);
    if let Some(name) = &opts.game_name {
        let _ = writeln!(header, "GameName {name}");
    }
    if let Some(sha1) = &opts.sha1 {
        let _ = writeln!(header, "SHA1 {sha1}");
    }
    if let Some(author) = &opts.author {
        let _ = writeln!(header, "Author {author}");
    }

    // --- Input Log.txt ---
    // The console-buttons group carries Reset / Power; RustyNES movies never
    // record either, so it is always released (`..`). Two controller groups.
    let mut input_log = String::new();
    input_log.push_str("[Input]\n");
    input_log.push_str("LogKey:#Reset|Power|#P1 Up|P1 Down|P1 Left|P1 Right|P1 Start|P1 Select|P1 B|P1 A|#P2 Up|P2 Down|P2 Left|P2 Right|P2 Start|P2 Select|P2 B|P2 A|\n");
    let mut pad = [0u8; 8];
    for frame in &movie.frames {
        // Console group: Reset + Power, both released.
        input_log.push_str("|..|");
        write_pad(frame.p1, &mut pad);
        input_log.push_str(core::str::from_utf8(&pad).expect("pad bytes are ASCII"));
        input_log.push('|');
        write_pad(frame.p2, &mut pad);
        input_log.push_str(core::str::from_utf8(&pad).expect("pad bytes are ASCII"));
        input_log.push_str("|\n");
    }
    input_log.push_str("[/Input]\n");

    Ok(Bk2Text { header, input_log })
}

/// Render `buttons` into an eight-byte `U D L R S s B A` pad field (mnemonic
/// letter when pressed, `.` when released).
fn write_pad(buttons: Buttons, out: &mut [u8; 8]) {
    for (i, (letter, flag)) in PAD_COLUMNS.iter().enumerate() {
        out[i] = if buttons.contains(*flag) {
            *letter
        } else {
            b'.'
        };
    }
}

/// Parse the `Header.txt` member into a [`Bk2Meta`].
fn parse_header(header: &str) -> Result<Bk2Meta, Bk2Error> {
    let mut meta = Bk2Meta::default();
    let mut saw_platform = false;
    for raw in header.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = match line.split_once(' ') {
            Some((k, v)) => (k, v.trim()),
            None => (line, ""),
        };
        match key {
            "Platform" => {
                // Accept the NES family; reject anything else (a SNES/GB/etc.
                // movie has the wrong controller model entirely).
                let plat = value.to_ascii_uppercase();
                if plat != "NES" && plat != "FAMICOM" && plat != "FDS" {
                    return Err(Bk2Error::WrongPlatform(value.to_string()));
                }
                saw_platform = true;
            }
            "rerecordCount" => meta.rerecord_count = u64::from(parse_int(key, value)?),
            "PAL" => meta.pal = parse_int(key, value)? != 0,
            "Author" => meta.author = Some(value.to_string()),
            "GameName" => meta.game_name = Some(value.to_string()),
            "SHA1" => meta.sha1 = Some(value.to_string()),
            "StartsFromSavestate" | "StartsFromSaveRam" if parse_int(key, value)? != 0 => {
                return Err(Bk2Error::Unsupported(
                    "save-anchored .bk2 (cross-emulator save blobs are not portable)",
                ));
            }
            _ => {
                // MovieVersion, Core, GUID, BoardName, FourScore, and any other
                // header keys are ignored (forward-compatible).
            }
        }
    }
    // A `.bk2` without a Platform line is tolerated as NES (some minimal movies
    // omit it); only an explicit non-NES platform is rejected above.
    let _ = saw_platform;
    Ok(meta)
}

/// The standard-controller column map (`U D L R S s B A`), used as the fallback
/// when a `LogKey:` group is absent or unrecognized. Each slot maps an input-line
/// character *position* to the [`Buttons`] flag it drives.
fn default_pad_columns() -> Vec<Option<Buttons>> {
    PAD_COLUMNS.iter().map(|(_, b)| Some(*b)).collect()
}

/// Map a `LogKey:` column *name* (e.g. `"P1 Up"`, `"Up"`, `"A"`, `"Select"`) to
/// the NES standard-controller button it drives. The `"Pn "` port label (or any
/// other prefix) is ignored — only the final word matters. Columns that are not
/// standard-controller buttons (`"Reset"`, `"Power"`, `"FDS Insert Disk"`, mic,
/// …) return `None`: they still occupy a character position in the input line but
/// drive nothing `RustyNES` models.
fn button_for_column(name: &str) -> Option<Buttons> {
    match name.trim().rsplit(' ').next().unwrap_or("") {
        "Up" | "U" => Some(Buttons::UP),
        "Down" | "D" => Some(Buttons::DOWN),
        "Left" | "L" => Some(Buttons::LEFT),
        "Right" | "R" => Some(Buttons::RIGHT),
        "Start" | "S" => Some(Buttons::START),
        "Select" | "s" => Some(Buttons::SELECT),
        "B" => Some(Buttons::B),
        "A" => Some(Buttons::A),
        _ => None,
    }
}

/// Per-port `(P1, P2)` position→button column maps parsed from a `LogKey:`.
type PadColumnMaps = (Vec<Option<Buttons>>, Vec<Option<Buttons>>);

/// Parse the `LogKey:` declaration into per-port position→button column maps.
///
/// The `LogKey` is `#`-separated controller groups, each a `|`-separated column
/// list: `LogKey:#Reset|Power|#P1 Up|P1 Down|…|P1 A|#P2 Up|…|`. Group 1 is the
/// console (dropped), group 2 is P1, group 3 is P2. Reading the *declared* order
/// (rather than assuming the fixed `U D L R S s B A`) is what lets a `.bk2`
/// authored with a different column order or extra columns play back correctly
/// (the NESdev-forum "`.bk2` did not play back" report). A group that yields no
/// recognized buttons falls back to [`default_pad_columns`], so a truncated or
/// exotic `LogKey` still maps a standard controller.
fn parse_log_key(log_key: &str) -> PadColumnMaps {
    let trimmed = log_key.trim();
    let body = trimmed.strip_prefix("LogKey:").unwrap_or(trimmed);
    // The body opens with a single `#` delimiter, then `#`-separated groups.
    // Strip ONLY that leading delimiter and split without dropping empties: an
    // empty console group (`##P1...`) must keep its slot so P1/P2 don't shift
    // left into it. groups[0] = console, groups[1] = P1, groups[2] = P2.
    let body = body.strip_prefix('#').unwrap_or(body);
    // Read ONLY the three groups we consume (console, P1, P2) straight from the
    // split iterator rather than collecting every `#`-group: a hostile `.bk2`
    // padded with `#` delimiters would otherwise allocate one `&str` slot per
    // empty group (~16 bytes each) and could exhaust memory on import. `split`
    // still yields empty groups, so `next()` preserves the empty console slot
    // (`##P1...`) and keeps P1/P2 from shifting left into it.
    let mut groups = body.split('#');
    let _console = groups.next(); // groups[0] = console (unused)
    let cols = |g: Option<&str>| -> Vec<Option<Buttons>> {
        let mapped: Vec<Option<Buttons>> = g.map_or_else(Vec::new, |grp| {
            // Strip only the trailing `|` delimiter each group carries; keep
            // interior empty columns (`P1 Up||P1 A`) so a button's column index
            // stays aligned with the frame-value index (else `A` would map to the
            // empty column's slot and a frame `U.A` would replay as `Up` alone).
            grp.strip_suffix('|')
                .unwrap_or(grp)
                .split('|')
                .map(button_for_column)
                .collect()
        });
        // If nothing in this group is a recognized controller button, the LogKey
        // was truncated/exotic — fall back to the fixed standard order.
        if mapped.iter().any(Option::is_some) {
            mapped
        } else {
            default_pad_columns()
        }
    };
    // groups[1] = P1; groups[2] = P2, read in order from the same iterator.
    (cols(groups.next()), cols(groups.next()))
}

/// Parse the `Input Log.txt` member into the per-frame [`FrameInput`] stream.
///
/// The first non-blank line inside `[Input]` must be a `LogKey:` declaration,
/// which supplies the per-port column order. Every subsequent `|`-delimited line
/// up to `[/Input]` is one frame; the first `|`-group is the console-buttons group
/// (parsed but dropped), then one group per controller port. Only P1 and P2 are
/// mapped.
fn parse_input_log(input_log: &str) -> Result<Vec<FrameInput>, Bk2Error> {
    let mut frames = Vec::new();
    let mut columns: Option<PadColumnMaps> = None;
    let mut frame_line_no = 0usize;
    for raw in input_log.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "[Input]" || trimmed == "[/Input]" {
            continue;
        }
        if trimmed.starts_with("LogKey:") {
            columns = Some(parse_log_key(trimmed));
            continue;
        }
        if line.starts_with('|') {
            let cols = columns.as_ref().ok_or(Bk2Error::MissingLogKey)?;
            frame_line_no += 1;
            frames.push(parse_input_line(line, &cols.0, &cols.1, frame_line_no)?);
        }
        // Any other line (comments / unknown sections) is ignored.
    }
    if columns.is_none() {
        return Err(Bk2Error::MissingLogKey);
    }
    Ok(frames)
}

/// Parse a single `|`-delimited input-log line into a [`FrameInput`] using the
/// per-port column maps from the `LogKey`. The first group is the console-buttons
/// group (dropped); groups 2 and 3 are P1 and P2.
fn parse_input_line(
    line: &str,
    p1_cols: &[Option<Buttons>],
    p2_cols: &[Option<Buttons>],
    line_no: usize,
) -> Result<FrameInput, Bk2Error> {
    if !line.ends_with('|') {
        return Err(Bk2Error::Malformed {
            line: line_no,
            reason: "input-log line must end with `|`",
        });
    }
    // `|console|p1|p2|...|` splits (on `|`) to ["", console, p1, p2, ..., ""].
    let mut groups = line.split('|');
    // Leading empty field (before the first `|`).
    if groups.next() != Some("") {
        return Err(Bk2Error::Malformed {
            line: line_no,
            reason: "input-log line must start with `|`",
        });
    }
    // Console-buttons group (Reset / Power / …); parsed-and-dropped — FrameInput
    // has no reset bit, mirroring the `.fm2` path.
    if groups.next().is_none() {
        return Err(Bk2Error::Malformed {
            line: line_no,
            reason: "missing console-buttons group",
        });
    }
    // P1 then P2 (extra controller groups, if any, are dropped).
    let p1 = match groups.next() {
        Some(g) => parse_pad(g, p1_cols, line_no)?,
        None => {
            return Err(Bk2Error::Malformed {
                line: line_no,
                reason: "missing player-1 controller group",
            });
        }
    };
    // P2 is optional (a 1-player movie); default to released when absent or an
    // empty trailing field.
    let p2 = match groups.next() {
        Some(g) if !g.is_empty() => parse_pad(g, p2_cols, line_no)?,
        _ => Buttons::empty(),
    };
    Ok(FrameInput::new(p1, p2))
}

/// Parse one gamepad group into [`Buttons`] using its port's `LogKey` column map.
///
/// Each character *position* is the column at that index of `columns`; a pressed
/// marker (any char other than space or `.`) sets that column's button (columns
/// that map to `None` — non-controller buttons — are consumed but ignored). The
/// group may be *longer* than the map (extra trailing columns we don't model are
/// tolerated) but not shorter (a truncated line is structurally malformed).
fn parse_pad(
    group: &str,
    columns: &[Option<Buttons>],
    line_no: usize,
) -> Result<Buttons, Bk2Error> {
    let bytes = group.as_bytes();
    if bytes.len() < columns.len() {
        return Err(Bk2Error::Malformed {
            line: line_no,
            reason: "gamepad group shorter than its LogKey column count",
        });
    }
    let mut buttons = Buttons::empty();
    for (i, col) in columns.iter().enumerate() {
        if let Some(flag) = col {
            let b = bytes[i];
            if b != b' ' && b != b'.' {
                buttons |= *flag;
            }
        }
    }
    Ok(buttons)
}

/// Parse an integer-typed header value, attaching the key for diagnostics.
fn parse_int(key: &str, value: &str) -> Result<u32, Bk2Error> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| Bk2Error::BadInteger {
            key: key.to_string(),
            value: value.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const TEST_SHA: [u8; 32] = [0x7B; 32];

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
            rerecord_count: 0,
        };
        let opts = Bk2ExportOpts {
            rerecord_count: 99,
            author: Some("tester".to_string()),
            game_name: Some("game".to_string()),
            sha1: Some("abc123".to_string()),
        };
        let text = export_bk2(&movie, &opts).expect("export");
        let (back, meta) = import_bk2(&text.header, &text.input_log, TEST_SHA).expect("import");

        assert_eq!(back.frames, movie.frames, "frames survive round-trip");
        assert_eq!(back.region, Region::Ntsc);
        assert_eq!(back.start, StartPoint::PowerOn);
        assert_eq!(back.rom_sha256, TEST_SHA);
        assert!(!meta.pal);
        assert_eq!(meta.rerecord_count, 99);
        assert_eq!(meta.author.as_deref(), Some("tester"));
        assert_eq!(meta.game_name.as_deref(), Some("game"));
        assert_eq!(meta.sha1.as_deref(), Some("abc123"));
    }

    #[test]
    fn exact_bit_and_char_mapping() {
        // Only A set -> char index 7 pressed (the last column), others released.
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: TEST_SHA,
            start: StartPoint::PowerOn,
            frames: vec![
                FrameInput::new(Buttons::A, Buttons::empty()),
                FrameInput::new(Buttons::UP, Buttons::empty()),
            ],
            rerecord_count: 0,
        };
        let text = export_bk2(&movie, &Bk2ExportOpts::default()).expect("export");
        let lines: Vec<&str> = text
            .input_log
            .lines()
            .filter(|l| l.starts_with("|.."))
            .collect();
        assert_eq!(lines.len(), 2);

        // |..|<p1>|<p2>| -> p1 group is split index 2.
        let p1_a = lines[0].split('|').nth(2).unwrap();
        assert_eq!(p1_a.len(), 8);
        for (i, c) in p1_a.chars().enumerate() {
            if i == 7 {
                assert_eq!(c, 'A', "A is the last column");
            } else {
                assert_eq!(c, '.', "non-A columns released");
            }
        }
        let p1_up = lines[1].split('|').nth(2).unwrap();
        for (i, c) in p1_up.chars().enumerate() {
            if i == 0 {
                assert_eq!(c, 'U', "Up is the first column");
            } else {
                assert_eq!(c, '.');
            }
        }

        // Start (upper S) vs Select (lower s) are distinct columns 4 and 5.
        let hand = "[Input]\nLogKey:#Reset|Power|...\n|..|....S...|.....s..|\n[/Input]\n";
        let (m, _) = import_bk2("Platform NES\n", hand, TEST_SHA).expect("import");
        assert_eq!(m.frames[0].p1, Buttons::START);
        assert_eq!(m.frames[0].p2, Buttons::SELECT);
    }

    #[test]
    fn log_key_column_order_is_honored() {
        // v2.2.9 "Studio II": a `.bk2` whose P1 columns are declared in a
        // NON-standard order must map by the `LogKey` order, not the fixed
        // `U D L R S s B A` positions. Here column 0 = A and column 1 = B, so a
        // press at character position 0 is A and at position 1 is B — the opposite
        // of the standard layout. This is the fix for the "`.bk2` did not play
        // back" report (a movie whose buttons all mapped to the wrong bits).
        let log = "[Input]\n\
            LogKey:#Reset|Power|#P1 A|P1 B|P1 Up|P1 Down|P1 Left|P1 Right|P1 Start|P1 Select|\n\
            |..|A.......|\n\
            |..|.B......|\n\
            [/Input]\n";
        let (m, _) = import_bk2("Platform NES\n", log, TEST_SHA).expect("import");
        assert_eq!(
            m.frames[0].p1,
            Buttons::A,
            "position 0 = LogKey column 0 = A"
        );
        assert_eq!(
            m.frames[1].p1,
            Buttons::B,
            "position 1 = LogKey column 1 = B"
        );
        // A pad group LONGER than the modeled columns (extra buttons like a mic)
        // is tolerated: extra trailing chars are ignored, no malformed error.
        let extra = "[Input]\n\
            LogKey:#Reset|Power|#P1 Up|P1 Down|P1 Left|P1 Right|P1 Start|P1 Select|P1 B|P1 A|P1 Mic|\n\
            |..|.......AX|\n\
            [/Input]\n";
        let (m2, _) = import_bk2("Platform NES\n", extra, TEST_SHA).expect("import extra-col");
        assert_eq!(
            m2.frames[0].p1,
            Buttons::A,
            "column 7 = A pressed; the 9th (Mic) col is ignored"
        );
    }

    #[test]
    fn log_key_preserves_empty_columns_and_groups() {
        // v2.2.9 fix: empty interior `LogKey` fields must KEEP their positions,
        // or later columns/groups shift left and buttons re-map silently.
        //
        // Empty interior COLUMN (`P1 Up||P1 A`): the empty middle column is a real
        // slot, so `A` stays at column index 2. A frame `U.A` must press Up (col 0)
        // and A (col 2); the pre-fix filter dropped the empty column, mapping A to
        // index 1 so `U.A` replayed as Up alone.
        let empty_col = "[Input]\n\
            LogKey:#Reset|Power|#P1 Up||P1 A|\n\
            |..|U.A|\n\
            [/Input]\n";
        let (m, _) = import_bk2("Platform NES\n", empty_col, TEST_SHA).expect("import empty-col");
        assert_eq!(
            m.frames[0].p1,
            Buttons::UP | Buttons::A,
            "empty middle column keeps its slot: Up (col 0) + A (col 2) both press"
        );

        // Empty CONSOLE group (`##P1…`): must not shift P1's map into the dropped
        // console slot. The pre-fix filter dropped the empty group, promoting P1
        // into the console position and losing it entirely.
        let empty_console = "[Input]\n\
            LogKey:##P1 Up|P1 Down|P1 Left|P1 Right|P1 Start|P1 Select|P1 B|P1 A|\n\
            ||U.......|\n\
            [/Input]\n";
        let (m2, _) =
            import_bk2("Platform NES\n", empty_console, TEST_SHA).expect("import empty-console");
        assert_eq!(
            m2.frames[0].p1,
            Buttons::UP,
            "empty console group keeps its slot; P1 col 0 = Up still maps to P1"
        );
    }

    #[test]
    fn pal_flag_maps_to_region() {
        let text = "Platform NES\nPAL 1\n";
        let log = "[Input]\nLogKey:x\n|..|........|........|\n[/Input]\n";
        let (movie, meta) = import_bk2(text, log, TEST_SHA).expect("import");
        assert_eq!(movie.region, Region::Pal);
        assert!(meta.pal);

        let pal_movie = Movie {
            region: Region::Pal,
            rom_sha256: TEST_SHA,
            start: StartPoint::PowerOn,
            frames: vec![FrameInput::new(Buttons::empty(), Buttons::empty())],
            rerecord_count: 0,
        };
        let out = export_bk2(&pal_movie, &Bk2ExportOpts::default()).expect("export");
        assert!(out.header.lines().any(|l| l == "PAL 1"));

        let ntsc_movie = Movie {
            region: Region::Ntsc,
            ..pal_movie
        };
        let out = export_bk2(&ntsc_movie, &Bk2ExportOpts::default()).expect("export");
        assert!(!out.header.lines().any(|l| l == "PAL 1"));
    }

    #[test]
    fn malformed_inputs_never_panic() {
        // Non-NES platform.
        assert!(matches!(
            import_bk2("Platform SNES\n", "[Input]\nLogKey:x\n[/Input]\n", TEST_SHA),
            Err(Bk2Error::WrongPlatform(_))
        ));

        // Missing LogKey.
        assert!(matches!(
            import_bk2(
                "Platform NES\n",
                "[Input]\n|..|........|........|\n",
                TEST_SHA
            ),
            Err(Bk2Error::MissingLogKey)
        ));

        // Bad integer header.
        assert!(matches!(
            import_bk2("Platform NES\nrerecordCount nope\n", "LogKey:x\n", TEST_SHA),
            Err(Bk2Error::BadInteger { .. })
        ));

        // Input line not ending with `|`.
        assert!(matches!(
            import_bk2(
                "Platform NES\n",
                "LogKey:x\n|..|........|........\n",
                TEST_SHA
            ),
            Err(Bk2Error::Malformed { .. })
        ));

        // 7-char pad group.
        assert!(matches!(
            import_bk2(
                "Platform NES\n",
                "LogKey:x\n|..|.......|........|\n",
                TEST_SHA
            ),
            Err(Bk2Error::Malformed { .. })
        ));

        // Save-anchored movie is unsupported.
        assert!(matches!(
            import_bk2(
                "Platform NES\nStartsFromSavestate 1\n",
                "LogKey:x\n",
                TEST_SHA
            ),
            Err(Bk2Error::Unsupported(_))
        ));
    }

    #[test]
    fn log_key_bounded_against_pathological_group_padding() {
        // Hardening regression (v2.2.9): `parse_log_key` reads only the console,
        // P1, and P2 groups straight from the `split('#')` iterator instead of
        // collecting every `#`-group, so a hostile `.bk2` padded with a large
        // number of `#` delimiters cannot amplify into an unbounded `Vec<&str>`
        // on import. The trailing empty groups must be ignored and P1/P2 must
        // still map correctly.
        let mut log = String::from("[Input]\nLogKey:#Reset|Power|#P1 Up|P1 A|#P2 Up|P2 A|");
        log.push_str(&"#".repeat(100_000)); // pathological trailing delimiters
        log.push_str("\n|..|U.|.A|\n[/Input]\n");
        let (m, _) = import_bk2("Platform NES\n", &log, TEST_SHA).expect("import padded LogKey");
        assert_eq!(
            m.frames[0].p1,
            Buttons::UP,
            "P1 col 0 = Up maps despite trailing `#` padding"
        );
        assert_eq!(
            m.frames[0].p2,
            Buttons::A,
            "P2 col 1 = A maps despite trailing `#` padding"
        );
    }

    #[test]
    fn one_player_movie_defaults_p2_released() {
        // A line with only the console group + P1 (no P2 group).
        let log = "[Input]\nLogKey:x\n|..|.......A|\n[/Input]\n";
        let (movie, _) = import_bk2("Platform NES\n", log, TEST_SHA).expect("import");
        assert_eq!(movie.frames.len(), 1);
        assert_eq!(movie.frames[0].p1, Buttons::A);
        assert_eq!(movie.frames[0].p2, Buttons::empty());
    }

    #[test]
    fn export_rejects_save_state_movie() {
        let movie = Movie {
            region: Region::Ntsc,
            rom_sha256: TEST_SHA,
            start: StartPoint::SaveState(vec![1, 2, 3]),
            frames: vec![],
            rerecord_count: 0,
        };
        assert!(matches!(
            export_bk2(&movie, &Bk2ExportOpts::default()),
            Err(Bk2Error::Unsupported(_))
        ));
    }
}
