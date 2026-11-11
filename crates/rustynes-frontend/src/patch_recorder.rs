//! The live-patch recorder and regression replay (v3.7.0,
//! `T-HACK-BPS-RECORDER`).
//!
//! A ROM hacker edits a game a byte at a time: a PRG byte to change a rule, a
//! CHR byte to change a tile, a RAM pin to hold a value while testing. This
//! module turns those edits into the two artifacts the hacking community
//! already exchanges, and checks the result against reference play:
//!
//! - **A BPS patch** of the PRG and CHR edits ([`encode_bps`]), written from
//!   the public BPS 1.0 specification (byuu's "beat" format, public domain):
//!   the `BPS1` marker; the source size, target size and metadata size as the
//!   format's variable-length numbers (seven bits a byte, the final byte
//!   flagged by bit 7, one subtracted after each non-final byte so that no
//!   value has two encodings); the metadata; a stream of actions, each a
//!   number whose low two bits pick the action (0 `SourceRead`, 1
//!   `TargetRead`, 2 `SourceCopy`, 3 `TargetCopy`) and whose remaining bits
//!   are the length minus one; and three little-endian IEEE CRC-32s (source,
//!   target, and the patch up to that last one). No third-party patcher's
//!   source was read. The encoder needs only two of the four actions: an
//!   edit leaves the image's size and layout alone, so every byte is either
//!   the source's byte at the same offset (`SourceRead`) or an edited one
//!   carried in the patch (`TargetRead`). The applier this module is checked
//!   against, [`crate::patch::apply_bps`], implements all four.
//! - **A RAM-pin sidecar.** RAM is not part of the ROM image and BPS has no
//!   runtime semantics, so a RAM pin cannot live in the patch. It goes to a
//!   TOML file in the frontend's existing raw RAM cheat format (the `[[raw]]`
//!   table of `crate::cheats`: `address`, `value`, an optional `compare`,
//!   `enabled`), which the cheat panel already reads. The BPS metadata names
//!   the sidecar and carries its SHA-256 ([`RecordedPatch::metadata`]), so a
//!   pin file that does not belong to the patch is refused
//!   ([`verify_sidecar`]).
//! - **Regression replay** ([`replay_regression`]): a reference movie
//!   (`.rnm`) recorded on the clean ROM is replayed on both the clean ROM and
//!   the patched one (its RAM pins applied after every frame, exactly as the
//!   raw cheats are), and the first frame whose machines differ is reported
//!   with its cause: the first differing CPU RAM byte, else the first
//!   differing pixel, else the cycle counter.
//!
//! Frontend-only: nothing here runs inside the emulation core, so the
//! determinism contract and every golden are untouched.

use std::fmt;
use std::fmt::Write as _;

use rustynes_core::{Movie, Nes};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cheats::RawCheat;

/// The iNES header's length.
const HEADER_LEN: usize = 16;
/// The optional trainer's length.
const TRAINER_LEN: usize = 512;
/// The metadata format this recorder writes, named in its root element.
pub const METADATA_FORMAT: &str = "rustynes-live-patch/1";

/// IEEE CRC-32 (reflected, polynomial `0xEDB8_8320`), the CRC BPS uses.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Append a BPS variable-length number.
///
/// The specification's encoding: seven bits a byte, least significant first,
/// bit 7 set on the final byte, and one subtracted from the remainder after
/// each non-final byte.
pub fn write_number(out: &mut Vec<u8>, mut n: u64) {
    loop {
        #[allow(clippy::cast_possible_truncation)] // masked to seven bits
        let low = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(0x80 | low);
            return;
        }
        out.push(low);
        n -= 1;
    }
}

/// The two actions the encoder emits.
const SOURCE_READ: u64 = 0;
const TARGET_READ: u64 = 1;

/// Append one action header: `((length - 1) << 2) | action`.
fn write_action(out: &mut Vec<u8>, action: u64, length: usize) {
    write_number(out, ((length as u64 - 1) << 2) | action);
}

/// Encode the BPS patch that turns `source` into `target`, with `metadata`
/// (written as is; the specification recommends XML 1.0 in UTF-8).
///
/// Runs where the target equals the source at the same offset are
/// `SourceRead`s, everything else (an edited byte, and any bytes past the
/// source's end) `TargetRead`s. That is not the smallest possible patch for a
/// moved block, which `SourceCopy` would express, but debugger edits move
/// nothing. A differing run is extended across an unchanged gap shorter than
/// three bytes, because a `SourceRead` header costs at least one byte and the
/// `TargetRead` header after it another.
#[must_use]
pub fn encode_bps(source: &[u8], target: &[u8], metadata: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + metadata.len());
    out.extend_from_slice(b"BPS1");
    write_number(&mut out, source.len() as u64);
    write_number(&mut out, target.len() as u64);
    write_number(&mut out, metadata.len() as u64);
    out.extend_from_slice(metadata);
    let same = |i: usize| source.get(i) == target.get(i);
    let mut i = 0;
    while i < target.len() {
        let start = i;
        if same(i) {
            while i < target.len() && same(i) {
                i += 1;
            }
            write_action(&mut out, SOURCE_READ, i - start);
        } else {
            loop {
                while i < target.len() && !same(i) {
                    i += 1;
                }
                // Swallow a short unchanged gap into this run.
                let gap = (i..target.len()).take_while(|&j| same(j)).count();
                if gap > 0 && gap < 3 && i + gap < target.len() {
                    i += gap;
                } else {
                    break;
                }
            }
            write_action(&mut out, TARGET_READ, i - start);
            out.extend_from_slice(&target[start..i]);
        }
    }
    out.extend_from_slice(&crc32(source).to_le_bytes());
    out.extend_from_slice(&crc32(target).to_le_bytes());
    let patch_crc = crc32(&out);
    out.extend_from_slice(&patch_crc.to_le_bytes());
    out
}

/// Where the PRG-ROM and CHR-ROM lie in an iNES / NES 2.0 image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomLayout {
    /// The file offset of PRG-ROM byte 0 (after the header and any trainer).
    pub prg_start: usize,
    /// PRG-ROM length in bytes.
    pub prg_len: usize,
    /// CHR-ROM length in bytes (0 for a CHR-RAM board, which has no CHR in
    /// the image to patch).
    pub chr_len: usize,
}

impl RomLayout {
    /// Read the layout from an iNES or NES 2.0 header. The NES 2.0
    /// exponent-multiplier size form is refused: no board with ROM sizes that
    /// need it is patched a byte at a time.
    ///
    /// # Errors
    ///
    /// Why the image is not a patchable cartridge.
    pub fn of(image: &[u8]) -> Result<Self, RecorderError> {
        let header = image.get(..HEADER_LEN).ok_or(RecorderError::NotInes)?;
        if &header[..4] != b"NES\x1A" {
            return Err(RecorderError::NotInes);
        }
        let nes2 = header[7] & 0x0C == 0x08;
        let (prg_hi, chr_hi) = if nes2 {
            (header[9] & 0x0F, header[9] >> 4)
        } else {
            (0, 0)
        };
        if prg_hi == 0x0F || chr_hi == 0x0F {
            return Err(RecorderError::ExponentSize);
        }
        let prg_len = (usize::from(prg_hi) << 8 | usize::from(header[4])) * 16 * 1024;
        let chr_len = (usize::from(chr_hi) << 8 | usize::from(header[5])) * 8 * 1024;
        let prg_start = HEADER_LEN
            + if header[6] & 0x04 != 0 {
                TRAINER_LEN
            } else {
                0
            };
        if image.len() < prg_start + prg_len + chr_len {
            return Err(RecorderError::Truncated);
        }
        Ok(Self {
            prg_start,
            prg_len,
            chr_len,
        })
    }

    /// The file offset of CHR-ROM byte 0.
    #[must_use]
    pub const fn chr_start(&self) -> usize {
        self.prg_start + self.prg_len
    }
}

/// Why a recording or a replay could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecorderError {
    /// The image has no iNES header.
    NotInes,
    /// The image is shorter than its header says.
    Truncated,
    /// A NES 2.0 exponent-multiplier ROM size.
    ExponentSize,
    /// An edit past the end of PRG-ROM or CHR-ROM.
    OutOfRange {
        /// `"PRG"` or `"CHR"`.
        space: &'static str,
        /// The offset asked for.
        offset: usize,
        /// The space's length.
        len: usize,
    },
    /// The sidecar's name or hash does not match the patch's metadata.
    SidecarMismatch(String),
    /// A ROM or movie the replay could not use.
    Replay(String),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInes => f.write_str("not an iNES image"),
            Self::Truncated => f.write_str("the image is shorter than its header says"),
            Self::ExponentSize => f.write_str("NES 2.0 exponent ROM sizes are not supported"),
            Self::OutOfRange { space, offset, len } => {
                write!(f, "{space} offset {offset:#x} is past its {len:#x} bytes")
            }
            Self::SidecarMismatch(why) => write!(f, "RAM-pin sidecar mismatch: {why}"),
            Self::Replay(why) => write!(f, "regression replay: {why}"),
        }
    }
}

impl std::error::Error for RecorderError {}

/// The sidecar's on-disk shape: the cheat file's `[[raw]]` array, so the cheat
/// panel's loader reads it as a cheat file with no Game Genie codes.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PinFile {
    #[serde(default)]
    raw: Vec<RawCheat>,
}

/// Records debugger edits against a clean ROM image.
#[derive(Clone, Debug)]
pub struct PatchRecorder {
    clean: Vec<u8>,
    edited: Vec<u8>,
    layout: RomLayout,
    pins: Vec<RawCheat>,
}

impl PatchRecorder {
    /// Start recording against `clean`, the ROM image as loaded (header
    /// included, which BPS patches cover like any other byte).
    ///
    /// # Errors
    ///
    /// [`RomLayout::of`]'s.
    pub fn new(clean: &[u8]) -> Result<Self, RecorderError> {
        Ok(Self {
            layout: RomLayout::of(clean)?,
            clean: clean.to_vec(),
            edited: clean.to_vec(),
            pins: Vec::new(),
        })
    }

    /// The image's layout.
    #[must_use]
    pub const fn layout(&self) -> RomLayout {
        self.layout
    }

    /// The clean image the recorder was started against.
    #[must_use]
    pub fn clean(&self) -> &[u8] {
        &self.clean
    }

    /// Whether any PRG or CHR byte differs from the clean image.
    #[must_use]
    pub fn has_edits(&self) -> bool {
        self.edited != self.clean
    }

    /// v3.7.0 — drop every PRG / CHR edit, returning `(section, offset, clean
    /// byte)` for each byte that differed, so a caller can write the clean
    /// bytes back into a live console. RAM pins are kept (they are not ROM
    /// edits). A header or trainer byte is never edited (`set_prg` / `set_chr`
    /// cannot reach them), so only the two sections are compared.
    pub fn revert(&mut self) -> Vec<(rustynes_core::RomSpace, usize, u8)> {
        let RomLayout {
            prg_start, prg_len, ..
        } = self.layout;
        let chr_start = self.layout.chr_start();
        let mut out = Vec::new();
        for (i, (e, c)) in self.edited.iter().zip(&self.clean).enumerate() {
            if e == c {
                continue;
            }
            if (prg_start..prg_start + prg_len).contains(&i) {
                out.push((rustynes_core::RomSpace::Prg, i - prg_start, *c));
            } else if i >= chr_start {
                out.push((rustynes_core::RomSpace::Chr, i - chr_start, *c));
            }
        }
        self.edited.clone_from(&self.clean);
        out
    }

    /// v3.7.0 — this recorder restarted with no edits, if `image_sha256` (a
    /// console's `Nes::image_sha256`, the whole file's SHA-256) is the clean
    /// image's, else `None`. For a frontend that reinstalls a console: the new
    /// one carries no ROM edit, so neither may the recorder, and a console of
    /// another image must not be recorded against this one. RAM pins are kept
    /// (they are not ROM edits).
    #[must_use]
    pub fn restarted_for(self, image_sha256: &[u8; 32]) -> Option<Self> {
        let same: [u8; 32] = Sha256::digest(&self.clean).into();
        (same == *image_sha256).then(|| Self {
            edited: self.clean.clone(),
            ..self
        })
    }

    /// The image with every edit so far applied.
    #[must_use]
    pub fn edited(&self) -> &[u8] {
        &self.edited
    }

    /// The RAM pins so far.
    #[must_use]
    pub fn pins(&self) -> &[RawCheat] {
        &self.pins
    }

    /// Set PRG-ROM byte `offset` (0-based in PRG-ROM, not a CPU address: the
    /// debugger's PRG-ROM view and `Nes::debug_poke_rom` use the same offsets,
    /// and `EmuCore` feeds every ROM write that lands here).
    ///
    /// # Errors
    ///
    /// [`RecorderError::OutOfRange`].
    pub fn set_prg(&mut self, offset: usize, value: u8) -> Result<(), RecorderError> {
        let len = self.layout.prg_len;
        if offset >= len {
            return Err(RecorderError::OutOfRange {
                space: "PRG",
                offset,
                len,
            });
        }
        self.edited[self.layout.prg_start + offset] = value;
        Ok(())
    }

    /// Set CHR-ROM byte `offset`.
    ///
    /// # Errors
    ///
    /// [`RecorderError::OutOfRange`], also for any offset on a CHR-RAM board.
    pub fn set_chr(&mut self, offset: usize, value: u8) -> Result<(), RecorderError> {
        let len = self.layout.chr_len;
        if offset >= len {
            return Err(RecorderError::OutOfRange {
                space: "CHR",
                offset,
                len,
            });
        }
        self.edited[self.layout.chr_start() + offset] = value;
        Ok(())
    }

    /// Pin CPU RAM `address` to `value`, optionally only while it reads
    /// `compare` (a raw RAM cheat). A second pin of one address replaces the
    /// first.
    pub fn pin_ram(&mut self, address: u16, value: u8, compare: Option<u8>) {
        self.pins.retain(|p| p.address != address);
        self.pins.push(RawCheat {
            address,
            value,
            compare,
            enabled: true,
        });
    }

    /// Finish: the BPS patch and, when there are RAM pins, the sidecar,
    /// named `<stem>.pins.toml` and referenced from the patch's metadata.
    #[must_use]
    pub fn finish(&self, stem: &str) -> RecordedPatch {
        let sidecar = (!self.pins.is_empty()).then(|| {
            let file = PinFile {
                raw: self.pins.clone(),
            };
            let text = toml::to_string(&file).unwrap_or_default();
            (format!("{stem}.pins.toml"), text)
        });
        let metadata = metadata_xml(sidecar.as_ref().map(|(n, t)| (n.as_str(), t.as_bytes())));
        RecordedPatch {
            bps: encode_bps(&self.clean, &self.edited, metadata.as_bytes()),
            metadata,
            sidecar,
        }
    }
}

/// v3.7.0 — "Save patch...": write a recorder's patch and its sidecar.
///
/// Writes `recorder`'s BPS patch to `bps_path` and, when `pins` (the session's enabled raw RAM cheats and freezes) are not
/// empty, the RAM-pin sidecar beside it, named from the patch's file stem
/// exactly as [`PatchRecorder::finish`] names and hashes it. The recorder
/// itself is not changed (the pins go into a copy), so saving twice writes the
/// same files. Returns the paths written, patch first.
///
/// # Errors
///
/// The first write that failed. Each file is written atomically
/// (`crate::atomic_write::write_atomic`), so a failure leaves any earlier copy
/// whole.
pub fn write_patch_files(
    recorder: &PatchRecorder,
    pins: &[RawCheat],
    bps_path: &std::path::Path,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut rec = recorder.clone();
    for pin in pins.iter().filter(|p| p.enabled) {
        rec.pin_ram(pin.address, pin.value, pin.compare);
    }
    let stem = bps_path
        .file_stem()
        .map_or_else(|| "patch".into(), |s| s.to_string_lossy().into_owned());
    let out = rec.finish(&stem);
    // Atomic, like every other user file (`tests/no_bare_fs_write.rs`): a
    // crash mid-write must not leave a truncated patch beside the ROM.
    crate::atomic_write::write_atomic(bps_path, &out.bps)?;
    let mut written = vec![bps_path.to_path_buf()];
    if let Some((name, text)) = out.sidecar {
        let side = bps_path.with_file_name(name);
        crate::atomic_write::write_atomic(&side, text.as_bytes())?;
        written.push(side);
    }
    Ok(written)
}

/// The recorder's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedPatch {
    /// The BPS patch of the PRG and CHR edits.
    pub bps: Vec<u8>,
    /// The metadata embedded in it.
    pub metadata: String,
    /// The RAM-pin sidecar, `(file name, TOML text)`, if any pins were made.
    pub sidecar: Option<(String, String)>,
}

/// Lower-case hex of a SHA-256.
fn sha256_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(64);
    for b in Sha256::digest(data) {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// The patch metadata: XML 1.0 in UTF-8, as the specification recommends.
fn metadata_xml(sidecar: Option<(&str, &[u8])>) -> String {
    let mut x = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<patch format=\"{METADATA_FORMAT}\" \
         generator=\"RustyNES {}\">\n",
        env!("CARGO_PKG_VERSION")
    );
    if let Some((name, bytes)) = sidecar {
        let _ = writeln!(
            x,
            "  <ram-pins file=\"{}\" sha256=\"{}\"/>",
            xml_escape(name),
            sha256_hex(bytes)
        );
    }
    x.push_str("</patch>\n");
    x
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The `(file, sha256)` a recorder-written metadata block names for its
/// RAM-pin sidecar, if it names one.
#[must_use]
pub fn sidecar_reference(metadata: &str) -> Option<(String, String)> {
    let at = metadata.find("<ram-pins ")?;
    let tag = &metadata[at..at + metadata[at..].find("/>")?];
    let attr = |name: &str| {
        let key = format!("{name}=\"");
        let start = tag.find(&key)? + key.len();
        let end = start + tag[start..].find('"')?;
        Some(
            tag[start..end]
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        )
    };
    Some((attr("file")?, attr("sha256")?))
}

/// v3.7.0 (#646 review) — the pins a regression replay runs with when no
/// sidecar was given.
///
/// None, unless the patch's metadata names one, which is an error naming
/// the file. A patch recorded with RAM pins replayed without
/// them reports the pins as a regression that is not there (or hides one).
///
/// # Errors
///
/// [`RecorderError::SidecarMismatch`] naming the sidecar the patch requires.
pub fn pins_without_sidecar(metadata: Option<&str>) -> Result<Vec<RawCheat>, RecorderError> {
    match metadata.and_then(sidecar_reference) {
        Some((name, _)) => Err(RecorderError::SidecarMismatch(format!(
            "the patch was recorded with RAM pins in {name}: pass it with --pins"
        ))),
        None => Ok(Vec::new()),
    }
}

/// Check a sidecar against the patch's metadata and parse its pins.
///
/// # Errors
///
/// [`RecorderError::SidecarMismatch`] when the metadata names no sidecar,
/// another file, or another hash, or the file does not parse.
pub fn verify_sidecar(
    metadata: &str,
    name: &str,
    text: &str,
) -> Result<Vec<RawCheat>, RecorderError> {
    let (want_name, want_hash) = sidecar_reference(metadata)
        .ok_or_else(|| RecorderError::SidecarMismatch("the patch names no sidecar".into()))?;
    if want_name != name {
        return Err(RecorderError::SidecarMismatch(format!(
            "the patch names {want_name}, not {name}"
        )));
    }
    let got = sha256_hex(text.as_bytes());
    if got != want_hash {
        return Err(RecorderError::SidecarMismatch(format!(
            "SHA-256 {got} is not the {want_hash} the patch records"
        )));
    }
    toml::from_str::<PinFile>(text)
        .map(|f| f.raw)
        .map_err(|e| RecorderError::SidecarMismatch(e.to_string()))
}

/// Apply raw RAM pins after a frame, as the frontend applies raw cheats:
/// each enabled pin writes its value, if its compare byte (when it has one)
/// matches.
pub fn apply_pins(nes: &mut Nes, pins: &[RawCheat]) {
    for pin in pins.iter().filter(|p| p.enabled) {
        if pin.compare.is_none_or(|c| nes.peek(pin.address) == c) {
            nes.poke_ram(pin.address, pin.value);
        }
    }
}

/// What first differs between the clean and the patched replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DivergenceCause {
    /// A CPU RAM byte (the first differing address).
    Ram {
        /// The address (`$0000-$07FF`).
        addr: u16,
        /// The clean run's byte.
        clean: u8,
        /// The patched run's byte.
        patched: u8,
    },
    /// The picture, with RAM equal: the first differing pixel.
    Picture {
        /// Column.
        x: u16,
        /// Row.
        y: u16,
    },
    /// Only the CPU cycle count (equal RAM and picture).
    Cycle {
        /// The clean run's cycle.
        clean: u64,
        /// The patched run's.
        patched: u64,
    },
}

/// The first divergence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regression {
    /// The movie frame (0-based) after whose run the machines first differ.
    pub frame: u32,
    /// What differs.
    pub cause: DivergenceCause,
}

impl fmt::Display for Regression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first divergence after frame {}: ", self.frame)?;
        match self.cause {
            DivergenceCause::Ram {
                addr,
                clean,
                patched,
            } => write!(
                f,
                "CPU RAM ${addr:04X} is ${clean:02X} clean, ${patched:02X} patched"
            ),
            DivergenceCause::Picture { x, y } => {
                write!(f, "the picture differs first at ({x}, {y}); RAM is equal")
            }
            DivergenceCause::Cycle { clean, patched } => write!(
                f,
                "only the cycle count differs ({clean} clean, {patched} patched)"
            ),
        }
    }
}

/// The regression replay's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegressionReport {
    /// Frames replayed (the movie's length, or up to the divergence).
    pub frames: u32,
    /// The first divergence, if any.
    pub regression: Option<Regression>,
}

/// Replay `movie` (recorded on `clean`) on `clean` and on `patched` with
/// `pins` applied after every frame, and report the first frame after which
/// the two machines differ.
///
/// The movie's ROM identity is checked against the clean ROM only; on the
/// patched side it is re-stamped with the patched ROM's identity, because a
/// patched ROM is a different ROM by design.
///
/// # Errors
///
/// [`RecorderError::Replay`] when a ROM does not load or the movie does not
/// start on the clean ROM.
pub fn replay_regression(
    clean: &[u8],
    patched: &[u8],
    movie: &Movie,
    pins: &[RawCheat],
) -> Result<RegressionReport, RecorderError> {
    let load = |rom: &[u8]| Nes::from_rom(rom).map_err(|e| RecorderError::Replay(e.to_string()));
    let mut a = load(clean)?;
    let mut b = load(patched)?;
    movie
        .seek_to_start(&mut a)
        .map_err(|e| RecorderError::Replay(format!("the movie does not start: {e}")))?;
    let mut on_patched = movie.clone();
    on_patched.rom_sha256 = *b.rom_sha256();
    on_patched
        .seek_to_start(&mut b)
        .map_err(|e| RecorderError::Replay(format!("the patched ROM does not start: {e}")))?;
    for (i, input) in movie.frames.iter().enumerate() {
        let frame = u32::try_from(i).unwrap_or(u32::MAX);
        input.apply_to(&mut a);
        input.apply_to(&mut b);
        let _ = a.run_frame();
        let _ = b.run_frame();
        apply_pins(&mut b, pins);
        if let Some(cause) = first_difference(&a, &b) {
            return Ok(RegressionReport {
                frames: frame + 1,
                regression: Some(Regression { frame, cause }),
            });
        }
    }
    Ok(RegressionReport {
        frames: u32::try_from(movie.frames.len()).unwrap_or(u32::MAX),
        regression: None,
    })
}

/// What first differs between two machines, in the order RAM, picture,
/// cycle.
fn first_difference(a: &Nes, b: &Nes) -> Option<DivergenceCause> {
    if let Some((i, (&x, &y))) = a
        .wram()
        .iter()
        .zip(b.wram())
        .enumerate()
        .find(|(_, (x, y))| x != y)
    {
        return Some(DivergenceCause::Ram {
            addr: u16::try_from(i).unwrap_or(u16::MAX),
            clean: x,
            patched: y,
        });
    }
    let (pa, _) = a.framebuffer().as_chunks::<4>();
    let (pb, _) = b.framebuffer().as_chunks::<4>();
    if let Some(i) = pa.iter().zip(pb).position(|(x, y)| x != y) {
        return Some(DivergenceCause::Picture {
            x: u16::try_from(i % 256).unwrap_or(u16::MAX),
            y: u16::try_from(i / 256).unwrap_or(u16::MAX),
        });
    }
    // `Nes::cycle` by path: clippy's `maybe_infinite_iter` reads a bare
    // `.cycle()` as `Iterator::cycle`.
    let (clean, patched) = (Nes::cycle(a), Nes::cycle(b));
    if clean == patched {
        None
    } else {
        Some(DivergenceCause::Cycle { clean, patched })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::apply_bps;
    use rustynes_core::{Buttons, FrameInput, MovieRecorder};

    fn nestest() -> Vec<u8> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("tests/roms/nestest/nestest.nes");
        std::fs::read(&root).unwrap_or_else(|e| panic!("read {}: {e}", root.display()))
    }

    /// A 37-byte program written for these tests (public-domain by
    /// construction): every frame it waits for vblank, strobes the first
    /// controller, reads its A button and stores it at `$0010`. It writes no
    /// other RAM.
    fn probe_rom() -> Vec<u8> {
        const PROGRAM: [u8; 37] = [
            0x78, 0xA2, 0xFF, 0x9A, 0xA9, 0x00, 0x8D, 0x00, 0x20, 0x8D, 0x01, 0x20, // init
            0x2C, 0x02, 0x20, 0x10, 0xFB, // wait for vblank
            0xA9, 0x01, 0x8D, 0x16, 0x40, 0xA9, 0x00, 0x8D, 0x16, 0x40, // strobe
            0xAD, 0x16, 0x40, 0x29, 0x01, 0x85, 0x10, // A -> $10
            0x4C, 0x0C, 0xC0, // loop
        ];
        let mut rom = b"NES\x1A\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        let mut prg = vec![0xEAu8; 16 * 1024];
        prg[..PROGRAM.len()].copy_from_slice(&PROGRAM);
        for v in [0x3FFA, 0x3FFC, 0x3FFE] {
            prg[v] = 0x00;
            prg[v + 1] = 0xC0;
        }
        rom.extend_from_slice(&prg);
        rom.extend_from_slice(&[0u8; 8 * 1024]);
        rom
    }

    /// The probe program's `STA $10` operand: PRG offset 33.
    const STA_OPERAND: usize = 33;

    /// A movie on `rom`: `frames` frames, A held from frame `press` on.
    fn movie(rom: &[u8], frames: u32, press: u32) -> Movie {
        let nes = Nes::from_rom(rom).expect("rom");
        let mut rec = MovieRecorder::power_on(&nes);
        for f in 0..frames {
            let b = if f >= press {
                Buttons::A
            } else {
                Buttons::empty()
            };
            rec.capture_input(FrameInput::four_players(
                b,
                Buttons::empty(),
                Buttons::empty(),
                Buttons::empty(),
            ));
        }
        rec.finish()
    }

    #[test]
    fn numbers_use_the_specification_encoding() {
        let enc = |n| {
            let mut v = Vec::new();
            write_number(&mut v, n);
            v
        };
        assert_eq!(enc(0), [0x80]);
        assert_eq!(enc(127), [0xFF]);
        // 128 = 0x00 then (128 >> 7) - 1 = 0 with the end flag.
        assert_eq!(enc(128), [0x00, 0x80]);
        assert_eq!(enc(129), [0x01, 0x80]);
        // 16,511 is the largest two-byte value: 127 + 128 * 128.
        assert_eq!(enc(16_511), [0x7F, 0xFF]);
        assert_eq!(enc(16_512), [0x00, 0x00, 0x80]);
    }

    /// The plan's first gate: a recorded BPS applied to the clean ROM
    /// reproduces the edited image byte for byte.
    #[test]
    fn a_recorded_patch_reproduces_the_edited_image() {
        let clean = nestest();
        let mut rec = PatchRecorder::new(&clean).expect("nestest is iNES");
        let layout = rec.layout();
        assert_eq!((layout.prg_len, layout.chr_len), (16 * 1024, 8 * 1024));
        // Scattered single bytes, adjacent runs, a short-gap pair, and the
        // first and last bytes of each space.
        let prg_edits: &[usize] = &[0, 1, 2, 100, 102, 5_000, 16 * 1024 - 1];
        let chr_edits: &[usize] = &[0, 7, 8, 9, 4_096, 8 * 1024 - 1];
        for (n, &o) in prg_edits.iter().enumerate() {
            let v = clean[layout.prg_start + o] ^ (0x5A + u8::try_from(n).expect("small"));
            rec.set_prg(o, v).expect("in range");
        }
        for &o in chr_edits {
            let v = !clean[layout.chr_start() + o];
            rec.set_chr(o, v).expect("in range");
        }
        assert!(rec.set_prg(16 * 1024, 0).is_err());
        assert!(rec.set_chr(8 * 1024, 0).is_err());
        let out = rec.finish("nestest-hack");
        assert_eq!(out.sidecar, None, "no pins, no sidecar");
        let applied = apply_bps(&clean, &out.bps).expect("the recorded patch applies");
        assert_eq!(applied, rec.edited(), "byte for byte");
        assert_ne!(applied, clean);
        assert!(out.bps.len() < 200, "{} bytes for 13 edits", out.bps.len());
        // And the unedited image is a patch that changes nothing.
        let none = PatchRecorder::new(&clean).expect("ok").finish("none");
        assert_eq!(apply_bps(&clean, &none.bps).expect("applies"), clean);
    }

    /// The plan's second gate: a RAM-only fixture (one pinned address, no ROM
    /// edit) replays with the pin applied from the sidecar, read back at the
    /// pinned frame.
    #[test]
    fn a_ram_pin_travels_in_the_sidecar_and_is_applied_on_replay() {
        let clean = probe_rom();
        let mut rec = PatchRecorder::new(&clean).expect("iNES");
        rec.pin_ram(0x0400, 0x5C, None);
        let out = rec.finish("probe");
        let patched = apply_bps(&clean, &out.bps).expect("applies");
        assert_eq!(patched, clean, "a RAM pin is not a ROM edit");
        let (name, text) = out.sidecar.clone().expect("a sidecar");
        assert_eq!(name, "probe.pins.toml");
        assert!(
            text.contains("[[raw]]") && text.contains("address = 1024"),
            "{text}"
        );
        // The metadata names it by name and hash, and refuses another.
        let pins = verify_sidecar(&out.metadata, &name, &text).expect("matches");
        assert!(verify_sidecar(&out.metadata, "other.toml", &text).is_err());
        // Without the sidecar, a regression replay refuses, naming it; a patch
        // with no pins needs none.
        let why = pins_without_sidecar(Some(&out.metadata)).expect_err("pins required");
        assert!(why.to_string().contains("probe.pins.toml"), "{why}");
        let plain = PatchRecorder::new(&clean).expect("iNES").finish("plain");
        assert_eq!(
            pins_without_sidecar(Some(&plain.metadata)).expect("no pins"),
            Vec::new()
        );
        assert_eq!(pins_without_sidecar(None).expect("no metadata"), Vec::new());
        assert!(verify_sidecar(&out.metadata, &name, &text.replace("92", "93")).is_err());
        // The metadata is in the patch itself, where `bps_metadata` finds it.
        let meta = crate::patch::bps_metadata(&out.bps).expect("metadata");
        assert_eq!(meta, out.metadata.as_bytes());
        assert!(out.metadata.contains("<ram-pins file=\"probe.pins.toml\""));
        // Replayed, the pin holds at the frame it is read back, and nothing
        // else differs: the divergence is that address, after frame 0.
        let m = movie(&clean, 60, 1_000);
        let report = replay_regression(&clean, &patched, &m, &pins).expect("replays");
        let r = report.regression.expect("the pin is a difference");
        assert_eq!(
            r.cause,
            DivergenceCause::Ram {
                addr: 0x0400,
                clean: 0,
                patched: 0x5C
            },
            "{r}"
        );
        assert_eq!(r.frame, 0);
        // Read back directly at the pinned frame on a replay of its own.
        let mut nes = Nes::from_rom(&patched).expect("rom");
        m.seek_to_start(&mut nes).expect("starts");
        for input in m.frames.iter().take(30) {
            input.apply_to(&mut nes);
            let _ = nes.run_frame();
            apply_pins(&mut nes, &pins);
        }
        assert_eq!(nes.peek(0x0400), 0x5C, "pinned at frame 30");
    }

    /// The plan's third gate: a planted regression is located. The patch
    /// moves the program's `STA $10` to `STA $11`; with nothing pressed both
    /// store zero, so the first divergence is the frame A is first held, at
    /// `$0010` (1 clean, still 0 patched).
    #[test]
    fn a_planted_regression_is_located_at_its_frame_and_cause() {
        let clean = probe_rom();
        let mut rec = PatchRecorder::new(&clean).expect("iNES");
        rec.set_prg(STA_OPERAND, 0x11).expect("in range");
        let out = rec.finish("probe-bug");
        let patched = apply_bps(&clean, &out.bps).expect("applies");
        let m = movie(&clean, 120, 40);
        let report = replay_regression(&clean, &patched, &m, &[]).expect("replays");
        let r = report.regression.expect("a regression");
        assert_eq!(r.frame, 40, "{r}");
        assert_eq!(
            r.cause,
            DivergenceCause::Ram {
                addr: 0x0010,
                clean: 0x01,
                patched: 0x00
            },
            "{r}"
        );
        assert!(r.to_string().contains("after frame 40"));
        // An unpatched replay finds nothing over the whole movie.
        let none = replay_regression(&clean, &clean, &m, &[]).expect("replays");
        assert_eq!(
            none,
            RegressionReport {
                frames: 120,
                regression: None
            }
        );
    }
}
