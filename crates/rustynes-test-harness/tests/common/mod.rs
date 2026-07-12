//! Shared helpers for the regression-detection harnesses.
//!
//! Cargo's integration-test layout treats `tests/<name>.rs` as
//! independent crates; sharing code requires a `tests/common/mod.rs`
//! sub-module referenced via `mod common;` from each test file. (See
//! <https://doc.rust-lang.org/book/ch11-03-test-organization.html>.)
//!
//! This module factors out:
//!
//! - [`fnv1a64`]: the 64-bit FNV-1a hash function used by every
//!   framebuffer-baseline test in the harness (`visual_regression`,
//!   `external_real_games`, `audio_tests`, `m22`, `mmc1_a12`).
//! - [`write_png`] + [`dump_frame_if_requested`]: opt-in PNG dump for
//!   visual verification of baselines, gated on the
//!   `RUSTYNES_DUMP_FRAMES` env var.
//! - [`run_and_hash`]: load-rom + run-N-frames + hash convenience.
//! - [`snapshot_line`]: stable one-line snapshot format
//!   (`rom=… frames=… fb_bytes=… fnv1a64=…`) matching the
//!   `visual_regression.rs` convention.
//!
//! All helpers are deliberately `pub` so any test file can call them;
//! Rust warns about unused items per-test-binary if the test file only
//! uses a subset, so callers add `#[allow(dead_code)]` to the `mod
//! common;` line — or simply use everything they import.
//!
//! Per the `feedback_emulator_fsm_mid_cycle_clobber` memory: this
//! infrastructure exists because the parallel-impl equivalence harness
//! that compared only END-OF-STEP state missed a mid-scanline clobber.
//! Real-ROM framebuffer baselines catch that class of bug.

#![allow(dead_code)]
#![allow(clippy::doc_markdown)]

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use rustynes_core::Nes;

/// Standard NES framebuffer width (RGBA8).
pub const FB_WIDTH: u32 = 256;
/// Standard NES framebuffer height.
pub const FB_HEIGHT: u32 = 240;

/// Compute the FNV-1a 64-bit hash of `bytes`. Standard constants per
/// <http://www.isthe.com/chongo/tech/comp/fnv/>.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Resolve `<workspace>/tests/roms/<rel>` for a test ROM filename
/// relative to the project's committed test-corpus root.
pub fn rom_path(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel)
}

/// Write a 256×240 RGBA8 framebuffer to `path` as a PNG. Used by the
/// opt-in screenshot dump.
///
/// # Errors
///
/// Wraps `std::io::Error` (file open / write) and translates
/// `png::EncodingError` via `std::io::Error::other`.
pub fn write_png(path: &Path, fb: &[u8]) -> std::io::Result<()> {
    let file = fs::File::create(path)?;
    let w = BufWriter::new(file);
    let mut enc = png::Encoder::new(w, FB_WIDTH, FB_HEIGHT);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(fb).map_err(std::io::Error::other)?;
    Ok(())
}

/// When `RUSTYNES_DUMP_FRAMES=1`, write the framebuffer to
/// `<DUMP_ROOT>/<corpus>/<rom>_<frame>.png`. Otherwise no-op.
///
/// `<DUMP_ROOT>` defaults to `/tmp/rustynes-baseline-screenshots/`
/// (regenerable, wiped on CachyOS reboot). To regenerate the committed
/// `screenshots/` corpus at the repo root, override via the
/// `RUSTYNES_DUMP_DIR` env var:
///
/// ```text
/// RUSTYNES_DUMP_FRAMES=1 RUSTYNES_DUMP_DIR="$PWD/screenshots" \
///     cargo test -p rustynes-test-harness --features test-roms,commercial-roms \
///     -- --nocapture
/// ```
///
/// I/O errors are logged to stderr but never panic — screenshot dumps
/// are diagnostic, never part of the assertion path.
pub fn dump_frame_if_requested(corpus: &str, rom_label: &str, frame_label: &str, fb: &[u8]) {
    if std::env::var_os("RUSTYNES_DUMP_FRAMES").is_none() {
        return;
    }
    let root = std::env::var("RUSTYNES_DUMP_DIR")
        .unwrap_or_else(|_| "/tmp/rustynes-baseline-screenshots".to_string());
    let dir = format!("{root}/{corpus}");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("[common] dump dir create failed ({dir}): {e}");
        return;
    }
    let safe: String = rom_label
        .trim_end_matches(".nes")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = format!("{dir}/{}_{frame_label}.png", safe.trim_matches('_'));
    if let Err(e) = write_png(Path::new(&path), fb) {
        eprintln!("[common] png write failed for {path}: {e}");
        return;
    }
    eprintln!("[common] wrote {path}");
}

/// Load a ROM by relative path under `tests/roms/`, run `frames`
/// frames with no controller input, and return the framebuffer hash.
///
/// Panics if the ROM file can't be read or parsed — these are
/// hard-fail conditions in a regression harness.
pub fn run_and_hash(rom_rel: &str, frames: u64) -> u64 {
    let path = rom_path(rom_rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_rel}: {e}"));
    for _ in 0..frames {
        nes.run_frame();
    }
    fnv1a64(nes.framebuffer())
}

/// Same as [`run_and_hash`] but also dumps the framebuffer at the end
/// when `RUSTYNES_DUMP_FRAMES=1`. The `corpus` argument is used to
/// namespace the screenshot output directory.
pub fn run_and_hash_with_dump(corpus: &str, rom_rel: &str, frames: u64) -> u64 {
    let path = rom_path(rom_rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_rel}: {e}"));
    for _ in 0..frames {
        nes.run_frame();
    }
    let label = Path::new(rom_rel)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(rom_rel);
    dump_frame_if_requested(corpus, label, &format!("f{frames}"), nes.framebuffer());
    fnv1a64(nes.framebuffer())
}

/// One-line snapshot format compatible with the
/// `tests/visual_regression.rs` corpus: `rom=… frames=… fb_bytes=…
/// fnv1a64=…`. Keeping it text-form means a PR diff shows the hash
/// change inline.
pub fn snapshot_line(rom: &str, frames: u64, hash: u64) -> String {
    // 256 * 240 * 4 = 245760 — invariant for the RustyNES framebuffer.
    format!("rom={rom} frames={frames} fb_bytes=245760 fnv1a64={hash:016x}")
}

/// Full-state regression capture: framebuffer hash + CPU cycles + audio
/// sample count + audio waveform hash. For audio-test ROMs (the
/// bbbradsmith corpus), the framebuffer hash alone is a weak sentinel
/// because most of those ROMs hold a uniform palette frame for the
/// entire test. Including the audio-buffer hash makes the test
/// actually exercise the APU + mapper-audio mixer paths it was
/// designed to validate.
///
/// Returned tuple: `(fb_hash, cycles, audio_sample_count, audio_hash)`.
///
/// `audio_hash` is computed by re-interpreting the `f32` samples as
/// their raw IEEE-754 little-endian bytes and FNV-1a-hashing those.
/// Deterministic and roundtrip-safe; an audio regression of even one
/// sample value or one boundary shift surfaces as a different hash.
pub fn run_and_capture_full(corpus: &str, rom_rel: &str, frames: u64) -> (u64, u64, usize, u64) {
    let path = rom_path(rom_rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_rel}: {e}"));
    let mut samples: Vec<f32> = Vec::new();
    for _ in 0..frames {
        nes.run_frame();
        let chunk = nes.drain_audio();
        samples.extend(chunk);
    }
    let fb_hash = fnv1a64(nes.framebuffer());
    let cycles = nes.cycle();
    let label = Path::new(rom_rel)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(rom_rel);
    dump_frame_if_requested(corpus, label, &format!("f{frames}"), nes.framebuffer());

    let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
    for s in &samples {
        audio_bytes.extend_from_slice(&s.to_le_bytes());
    }
    let audio_hash = fnv1a64(&audio_bytes);
    (fb_hash, cycles, samples.len(), audio_hash)
}

/// Snapshot line for [`run_and_capture_full`] — fb + cycles + audio.
/// Stable text form mirroring [`snapshot_line`].
pub fn snapshot_line_full(
    rom: &str,
    frames: u64,
    fb_hash: u64,
    cycles: u64,
    audio_samples: usize,
    audio_hash: u64,
) -> String {
    format!(
        "rom={rom} frames={frames} fb_bytes=245760 fb_fnv1a64={fb_hash:016x} \
         cycles={cycles} audio_samples={audio_samples} audio_fnv1a64={audio_hash:016x}"
    )
}

/// Run `rom_rel` for `frames` frames and return the per-frame **peak audio
/// envelope**: `peaks[i]` is `max(|sample|)` over every audio sample the core
/// emitted during frame `i` (0.0 for a silent frame).
///
/// This is the measurement primitive behind the `audio_expansion` decibel
/// oracle. The bbbradsmith `db_*` "hotswap" ROMs play a sustained full-volume
/// **2A03 reference square** in one time segment and the **expansion-chip
/// square** in a later segment; on real hardware you compare them by ear /
/// oscilloscope, and in the emulator the equivalent is comparing the peak
/// amplitude of the two segments in the rendered waveform. Returning a
/// per-frame envelope lets a caller pick the (deterministic) frame windows for
/// each segment and take the ratio — a machine-verifiable level criterion.
///
/// The whole core is deterministic, so the returned envelope is byte-stable
/// run-to-run for a given ROM + frame count.
pub fn capture_frame_peaks(rom_rel: &str, frames: u64) -> Vec<f32> {
    let path = rom_path(rom_rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_rel}: {e}"));
    // Bounded capacity hint: fall back to 0 (let the Vec grow) rather than
    // `usize::MAX` if `frames` somehow exceeds `usize` on a 32-bit target, so a
    // pathological `frames` can never trigger a huge speculative allocation.
    let mut peaks = Vec::with_capacity(usize::try_from(frames).unwrap_or(0));
    for _ in 0..frames {
        nes.run_frame();
        let chunk = nes.drain_audio();
        let peak = chunk.iter().fold(0.0_f32, |m, &s| m.max(s.abs()));
        peaks.push(peak);
    }
    peaks
}

/// Peak of a per-frame envelope over the half-open frame window
/// `[start, end)` (see [`capture_frame_peaks`]). Panics if the window is out
/// of range so a mis-specified segment fails loudly rather than silently
/// reading 0.0.
pub fn window_peak(peaks: &[f32], start: usize, end: usize) -> f32 {
    assert!(
        end <= peaks.len() && start < end,
        "window [{start}, {end}) out of range for {} captured frames",
        peaks.len()
    );
    peaks[start..end].iter().fold(0.0_f32, |m, &s| m.max(s))
}

/// Helpers specific to the commercial-roms harness
/// (`tests/external_real_games.rs`). Gated behind the `commercial-roms`
/// feature so the `sha2` dev-dep is only pulled in when the harness is
/// actually compiled — `default-features = false` builds (e.g. plain
/// `cargo test --workspace --features test-roms`) stay unaffected.
///
/// All public so `external_real_games.rs` can call them across the
/// integration-test crate boundary; `#[allow(dead_code)]` on the parent
/// module covers the case where another harness file imports `common`
/// without using these.
#[cfg(feature = "commercial-roms")]
pub mod external {
    use std::fs;
    use std::path::PathBuf;

    use rustynes_core::{Buttons, Nes};
    use rustynes_test_harness::coverage::{FrameHealth, frame_health};
    use sha2::{Digest, Sha256};

    use super::{FB_HEIGHT, FB_WIDTH, dump_frame_if_requested, fnv1a64};

    /// Corpus name used for [`dump_frame_if_requested`] output paths
    /// (`/tmp/rustynes-baseline-screenshots/external/...`).
    pub const CORPUS: &str = "external";

    /// Deterministic input script driven by the commercial-roms harness.
    ///
    /// - [`IdleOnly`](InputScript::IdleOnly): no buttons pressed for
    ///   `frames` frames. Captures one fb hash at the end. Default for
    ///   most ROMs — title screens / demo loops are deterministic.
    /// - [`StartTap`](InputScript::StartTap): idle `idle_pre` frames →
    ///   one frame with START held → idle `idle_post` frames → free-run
    ///   `free_run` frames. Captures a fb hash at every checkpoint in
    ///   `checkpoints` (frame numbers, 1-indexed, post-script-start).
    ///   Preserves the original `Super Mario Bros.` / `Excitebike` /
    ///   `Kid Icarus` regression-bisect coverage at frames 120 / 240 /
    ///   600.
    #[derive(Clone, Copy, Debug)]
    pub enum InputScript {
        /// Boot + run N frames with no input. Single checkpoint at end.
        IdleOnly { frames: u32 },
        /// Idle → 1-frame START tap → idle → free-run with multiple
        /// captured checkpoints. Mirrors the original 3-ROM oracle.
        StartTap {
            idle_pre: u32,
            idle_post: u32,
            free_run: u32,
            checkpoints: &'static [u32],
        },
        /// Idle → 1-frame START → `gap` idle → 1-frame START → idle →
        /// free-run. For games whose intro needs TWO START presses to
        /// reach gameplay (e.g. Kid Icarus: title → story → game).
        DoubleStartTap {
            idle_pre: u32,
            gap: u32,
            idle_post: u32,
            free_run: u32,
            checkpoints: &'static [u32],
        },
        /// 1-frame START tap at `warmup`, then again every `period` frames
        /// for `taps` total taps, then free-run. For games with a long
        /// multi-stage intro (publisher splash → story → title → menu) that
        /// needs several presses to reach the menu / stage-select (e.g. Mega
        /// Man 4/6 → Robot Master select; Bandit Kings → main menu).
        RepeatStartTap {
            warmup: u32,
            period: u32,
            taps: u32,
            free_run: u32,
            checkpoints: &'static [u32],
        },
    }

    impl InputScript {
        /// Total frame count this script runs (sum of all phases). Used
        /// by the snapshot-line `frames=N` field so the snapshot makes
        /// the script length explicit.
        #[must_use]
        pub const fn total_frames(&self) -> u32 {
            match *self {
                Self::IdleOnly { frames } => frames,
                Self::StartTap {
                    idle_pre,
                    idle_post,
                    free_run,
                    ..
                } => idle_pre + 1 + idle_post + free_run,
                Self::DoubleStartTap {
                    idle_pre,
                    gap,
                    idle_post,
                    free_run,
                    ..
                } => idle_pre + 1 + gap + 1 + idle_post + free_run,
                Self::RepeatStartTap {
                    warmup,
                    period,
                    taps,
                    free_run,
                    ..
                } => warmup + taps.saturating_sub(1) * period + 1 + free_run,
            }
        }
    }

    /// One captured checkpoint: frame number (1-indexed) + fb hash.
    #[derive(Clone, Copy, Debug)]
    pub struct Checkpoint {
        pub frame: u32,
        pub fb_hash: u64,
    }

    /// Result of running one ROM through an [`InputScript`].
    pub struct CaptureResult {
        /// SHA-256 of the input ROM bytes (hex).
        pub rom_sha256_hex: String,
        /// Checkpoints captured during the run, in order.
        pub checkpoints: Vec<Checkpoint>,
        /// CPU cycle count at the final frame.
        pub cycles: u64,
        /// Number of audio samples produced over the run.
        pub audio_samples: usize,
        /// FNV-1a 64-bit hash of the drained audio samples (raw f32 LE
        /// bytes). Same audio-hash convention as [`super::run_and_capture_full`].
        pub audio_fnv1a64: u64,
        /// Distinct-colour / dominant-colour health of the FINAL
        /// framebuffer, computed via the shared
        /// [`frame_health`](rustynes_test_harness::coverage::frame_health)
        /// helper. The auto-discovering coverage harness asserts this is
        /// not [`looks_blank`](FrameHealth::looks_blank); the curated
        /// harnesses ignore it.
        pub final_frame_health: FrameHealth,
    }

    /// Resolve `<workspace>/tests/roms/external/<rel>`. The
    /// commercial-roms harness uses a separate resolver (vs.
    /// [`super::rom_path`]) so the `external/` prefix is implicit.
    pub fn external_rom_path(rel: &str) -> PathBuf {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .join("tests")
            .join("roms")
            .join("external")
            .join(rel)
    }

    /// 8 KiB Famicom Disk System BIOS (`disksys.rom`) length.
    const FDS_BIOS_LEN: usize = 8192;

    /// Detect whether `bytes` is a Famicom Disk System disk image.
    ///
    /// Mirrors the frontend's `is_fds_image`: recognizes both the fwNES
    /// 16-byte-header form (ASCII magic `"FDS\x1A"`) and the headerless raw
    /// form (first side opens with `\x01*NINTENDO-HVC*`). A standard iNES /
    /// NES 2.0 cartridge opens with `"NES\x1A"`, so this never misfires on the
    /// cartridge path.
    fn is_fds_image(bytes: &[u8]) -> bool {
        bytes.starts_with(b"FDS\x1A") || bytes.starts_with(b"\x01*NINTENDO-HVC*")
    }

    /// Resolve the FDS BIOS bytes for a disk-image capture, returning `None`
    /// (so the caller can SKIP) when no usable BIOS is available.
    ///
    /// Resolution order — the BIOS is Nintendo IP and is never committed:
    /// 1. the `RUSTYNES_FDS_BIOS` env var (the convention `tests/fds.rs`
    ///    already uses for its real-BIOS path); else
    /// 2. the conventional staged copy `tests/roms/external/fds/disksys.rom`.
    ///
    /// Either source must be exactly 8 KiB. Returns `None` when neither
    /// resolves so an FDS capture degrades to a clean skip rather than a hard
    /// failure on a checkout that has the disks but not the BIOS.
    fn resolve_fds_bios() -> Option<Vec<u8>> {
        let try_path = |path: PathBuf| -> Option<Vec<u8>> {
            match fs::read(&path) {
                Ok(b) if b.len() == FDS_BIOS_LEN => Some(b),
                Ok(b) => {
                    eprintln!(
                        "[external] FDS BIOS {} is {} bytes (expected {FDS_BIOS_LEN}); ignoring",
                        path.display(),
                        b.len()
                    );
                    None
                }
                Err(_) => None,
            }
        };
        if let Ok(p) = std::env::var("RUSTYNES_FDS_BIOS")
            && let Some(b) = try_path(PathBuf::from(p))
        {
            return Some(b);
        }
        try_path(external_rom_path("fds/disksys.rom"))
    }

    /// Extract the first NES / FDS / UNIF entry from a `.zip` archive's bytes,
    /// returning its inner bytes. Mirrors the frontend's `extract_rom_from_zip`
    /// (same recognized extensions, same zip-bomb guard). Returns `None` if the
    /// archive is unreadable or holds no recognized ROM entry.
    fn extract_rom_from_zip(zip_bytes: &[u8]) -> Option<Vec<u8>> {
        use std::io::Read;
        // Bound both the declared size AND the actual read — the declared size
        // can lie (matches the frontend's PR #74 hardening).
        const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).ok()?;
        let idx = (0..archive.len()).find(|&i| {
            archive.by_index(i).is_ok_and(|f| {
                std::path::Path::new(f.name()).extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("nes")
                        || e.eq_ignore_ascii_case("fds")
                        || e.eq_ignore_ascii_case("unf")
                        || e.eq_ignore_ascii_case("unif")
                })
            })
        })?;
        let file = archive.by_index(idx).ok()?;
        if file.size() > MAX_ENTRY_BYTES {
            return None;
        }
        let cap = usize::try_from(file.size()).unwrap_or(0);
        let mut out = Vec::with_capacity(cap);
        file.take(MAX_ENTRY_BYTES).read_to_end(&mut out).ok()?;
        Some(out)
    }

    /// Extract the first NES / FDS / UNIF entry from a `.7z` archive at `path`
    /// via the system `7z` CLI (no Rust 7z dep — matches
    /// `scripts/coverage/coverage.py`, which indexes/stages `.7z` the same
    /// way). Returns `None` if `7z` is missing, the archive is unreadable, or
    /// it holds no recognized ROM entry.
    fn extract_rom_from_7z(path: &std::path::Path) -> Option<Vec<u8>> {
        use std::io::Read;
        use std::process::Command;
        // Same hard cap as the `.zip` path so a 7z-bomb (a huge or
        // maliciously-sized member) can't OOM the harness.
        const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
        // List entries (`-slt` = technical, machine-readable) and pick the
        // first NES/FDS/UNIF member.
        let listing = Command::new("7z")
            .arg("l")
            .arg("-slt")
            .arg(path)
            .output()
            .ok()?;
        if !listing.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&listing.stdout);
        let entry = text
            .lines()
            .filter_map(|l| l.strip_prefix("Path = "))
            .find(|name| {
                std::path::Path::new(name).extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("nes")
                        || e.eq_ignore_ascii_case("fds")
                        || e.eq_ignore_ascii_case("unf")
                        || e.eq_ignore_ascii_case("unif")
                })
            })?;
        // Stream that single entry to stdout (`e` = extract, `-so` = to
        // stdout) and read it under the cap above so an oversize member is
        // rejected, not buffered whole.
        let mut child = Command::new("7z")
            .arg("e")
            .arg("-so")
            .arg(path)
            .arg(entry)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut out = Vec::new();
        // Read one byte past the cap so an oversize member is detected (and
        // rejected) rather than silently truncated.
        let mut stdout = child.stdout.take()?;
        let read = stdout
            .by_ref()
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut out);
        // Drop our read end; if the member is oversize the child takes SIGPIPE.
        drop(stdout);
        let oversize = out.len() as u64 > MAX_ENTRY_BYTES;
        if oversize {
            let _ = child.kill();
        }
        let status = child.wait().ok()?;
        if read.is_err() || oversize || !status.success() || out.is_empty() {
            return None;
        }
        Some(out)
    }

    /// Outcome of resolving + loading a staged ROM path into a [`Nes`].
    pub enum Load {
        /// The ROM loaded successfully.
        Ok(Box<Nes>),
        /// The ROM was an FDS disk image but no BIOS could be resolved; the
        /// caller should treat this as a clean SKIP, not a failure.
        SkipNoBios,
    }

    /// Load the staged ROM at `external/`-relative `rom_rel` into a [`Nes`],
    /// mirroring the frontend's load dispatch so EVERY loadable form is
    /// covered, not just bare `.nes`:
    ///
    /// - `.zip` / `.7z`: extract the first NES / FDS / UNIF entry, then treat
    ///   the extracted bytes exactly as a loose file (FDS-magic detection
    ///   still applies to a `.fds` inside the archive).
    /// - FDS disk image (`.fds`, or FDS magic after archive extraction): build
    ///   via [`Nes::from_disk`] with a resolved BIOS (`RUSTYNES_FDS_BIOS` or the
    ///   staged `fds/disksys.rom`); a missing BIOS yields [`Load::SkipNoBios`].
    /// - everything else (`.nes` / `.unf` / `.unif` / raw): [`Nes::from_rom`].
    ///
    /// Panics on a file-read / archive-extract / parse failure — those are
    /// hard-fail in a regression harness (the ROM is expected to be staged and
    /// valid). A missing FDS BIOS is the one soft case (skip).
    pub fn load_nes(rom_rel: &str) -> Load {
        let path = external_rom_path(rom_rel);
        let ext_is = |e: &str| path.extension().is_some_and(|x| x.eq_ignore_ascii_case(e));

        // 1. Resolve to the raw ROM/disk bytes, unwrapping any archive.
        let bytes = if ext_is("zip") {
            let raw = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            extract_rom_from_zip(&raw)
                .unwrap_or_else(|| panic!("no NES/FDS/UNIF entry in archive {rom_rel}"))
        } else if ext_is("7z") {
            extract_rom_from_7z(&path).unwrap_or_else(|| {
                panic!("no NES/FDS/UNIF entry in (or 7z CLI missing for) archive {rom_rel}")
            })
        } else {
            fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        };

        // 2. Dispatch FDS vs. cartridge by content magic (covers a `.fds`
        //    loose file AND a `.fds` extracted from an archive).
        if ext_is("fds") || is_fds_image(&bytes) {
            let Some(bios) = resolve_fds_bios() else {
                eprintln!(
                    "[external] SKIP {rom_rel}: FDS disk but no BIOS \
                     (set RUSTYNES_FDS_BIOS or stage tests/roms/external/fds/disksys.rom)."
                );
                return Load::SkipNoBios;
            };
            let nes = Nes::from_disk(&bytes, &bios)
                .unwrap_or_else(|e| panic!("load FDS disk {rom_rel}: {e}"));
            return Load::Ok(Box::new(nes));
        }

        let nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_rel}: {e}"));
        Load::Ok(Box::new(nes))
    }

    /// Compute the SHA-256 of `bytes` as a lower-case hex string.
    #[must_use]
    pub fn compute_rom_sha256(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        hex_encode(&digest)
    }

    /// Lower-case hex encoding without pulling in the `hex` crate (one
    /// more dev-dep we'd rather avoid). 32 bytes → 64 ASCII chars.
    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
        out
    }

    /// Build a "safe" filename component from a ROM-relative path —
    /// keep alphanumerics, replace everything else with `_`. Used for
    /// PNG-dump filenames so spaces/apostrophes/parentheses don't break
    /// shell tooling that consumes the dump dir.
    fn sanitize(label: &str) -> String {
        label
            .trim_end_matches(".nes")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    }

    /// Run one ROM through `script`, capturing fb hashes at each
    /// checkpoint plus cycle / audio-sample / audio-hash totals.
    ///
    /// PNGs are written when `RUSTYNES_DUMP_FRAMES=1` — one per
    /// checkpoint into `/tmp/rustynes-baseline-screenshots/external/`.
    /// Filenames embed the mapper subdir so a quick `ls` of the dump
    /// dir groups by mapper visually.
    ///
    /// Panics on file-read / ROM-parse failure — these are hard-fail
    /// in a regression harness (the user is expected to have staged
    /// the ROMs at `tests/roms/external/`).
    #[allow(clippy::too_many_lines)]
    pub fn run_capture(rom_rel: &str, script: InputScript) -> CaptureResult {
        run_capture_opt(rom_rel, script).unwrap_or_else(|| {
            panic!(
                "run_capture({rom_rel}): FDS disk but no BIOS — set RUSTYNES_FDS_BIOS \
                 or stage tests/roms/external/fds/disksys.rom"
            )
        })
    }

    /// Like [`run_capture`] but returns `None` (instead of panicking) when the
    /// ROM is an FDS disk image and no BIOS could be resolved — the
    /// auto-discovering coverage harness uses this so an FDS disk on a
    /// BIOS-less checkout SKIPs cleanly rather than failing the whole sweep.
    ///
    /// Loads via [`load_nes`], so `.zip` / `.7z` archives and `.fds` disks are
    /// covered identically to loose `.nes` files.
    ///
    /// Panics on a file-read / archive-extract / parse failure (hard-fail in a
    /// regression harness); a missing FDS BIOS is the one soft case.
    #[allow(clippy::too_many_lines)]
    pub fn run_capture_opt(rom_rel: &str, script: InputScript) -> Option<CaptureResult> {
        let path = external_rom_path(rom_rel);
        // The SHA-256 pins the *staged file* (archive bytes for a `.zip`/`.7z`,
        // disk bytes for a `.fds`, ROM bytes for a `.nes`) — a stable identity
        // for the on-disk dump regardless of container. This is intentional and
        // shared with `assert_rom_sha256_or_recapture`, which fails-fast on a
        // wrong/corrupt dump by hashing the *same* on-disk bytes; deriving this
        // from the extracted ROM image would break that coherence. A given dump
        // is staged exactly one way, so there is no per-ROM churn.
        let rom_sha256_hex = {
            let raw = fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "read {}: {} — is the ROM staged at tests/roms/external/?",
                    path.display(),
                    e
                )
            });
            compute_rom_sha256(&raw)
        };
        let mut nes = match load_nes(rom_rel) {
            Load::Ok(nes) => *nes,
            Load::SkipNoBios => return None,
        };

        // Vs. System setup. Vs. arcade carts (iNES mapper 99, mapper 151, or
        // the NES-2.0/header Vs. console flag) boot to an attract loop and
        // need (a) the correct RGB-PPU palette LUT — iNES-1.0 dumps default to
        // the 2C03 and the per-game DB ([`rustynes_core::vs_db`]) is
        // authoritative for the real 2C04-000x / 2C05 type — and (b) a coin on
        // an acceptor to leave the insert-coin screen. Mirrors the frontend's
        // `apply_vs_db`. A no-op (both setters ignore) on non-Vs. carts, so a
        // plain NES capture is byte-identical to before. Coins are pulsed in
        // the tick loop below.
        let is_vs = nes.is_vs_system();
        if is_vs {
            // Apply the per-game DB's PPU type (palette LUT, authoritative) and
            // its game-config DSW0 default (e.g. Vs. Super Mario Bros. needs
            // DSW0=0x10 to leave the attract loop; a forced 0 leaves it blank).
            // Falls back to DIP 0 for a Vs. cart not in the DB.
            let dip = rustynes_core::vs_db::lookup(nes.rom_sha256()).map_or(0, |entry| {
                nes.set_vs_ppu_type(entry.vs_ppu_type);
                entry.vs_dip
            });
            nes.set_vs_dip(dip);
        }

        let label_safe = sanitize(rom_rel);
        let mut checkpoints: Vec<Checkpoint> = Vec::new();
        let mut samples: Vec<f32> = Vec::new();

        // Inner helper: tick `n` frames with the given button state and
        // drain audio each frame. For Vs. carts a coin is pulsed on acceptor
        // #1 every ~120 frames (4 frames down, then released) so the game
        // leaves its insert-coin attract loop.
        let mut frame_counter: u32 = 0;
        macro_rules! pulse_vs_coin {
            () => {{
                if is_vs {
                    if frame_counter % 120 == 30 {
                        nes.insert_coin(0);
                    } else if frame_counter % 120 == 34 {
                        nes.clear_coin();
                    }
                }
            }};
        }
        macro_rules! tick_with {
            ($buttons:expr, $count:expr) => {{
                let buttons: Buttons = $buttons;
                let n: u32 = $count;
                for _ in 0..n {
                    nes.set_buttons(0, buttons);
                    pulse_vs_coin!();
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                }
            }};
        }

        match script {
            InputScript::IdleOnly { frames } => {
                tick_with!(Buttons::empty(), frames);
                let fb_hash = fnv1a64(nes.framebuffer());
                checkpoints.push(Checkpoint {
                    frame: frames,
                    fb_hash,
                });
                dump_frame(&label_safe, &format!("f{frames}"), nes.framebuffer());
            }
            InputScript::StartTap {
                idle_pre,
                idle_post,
                free_run,
                checkpoints: checkpoint_frames,
            } => {
                // Use a HashSet would be tidier but const-array is fine
                // for the 3..=5 checkpoint values we ever pass.
                let mut maybe_emit = |frame: u32, nes: &Nes| {
                    if checkpoint_frames.contains(&frame) {
                        let fb_hash = fnv1a64(nes.framebuffer());
                        checkpoints.push(Checkpoint { frame, fb_hash });
                        dump_frame(&label_safe, &format!("f{frame}"), nes.framebuffer());
                    }
                };

                // Phase 1: idle_pre frames idle.
                for _ in 0..idle_pre {
                    nes.set_buttons(0, Buttons::empty());
                    pulse_vs_coin!();
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
                // Phase 2: 1 frame START down.
                nes.set_buttons(0, Buttons::START);
                pulse_vs_coin!();
                nes.run_frame();
                samples.extend(nes.drain_audio());
                frame_counter += 1;
                maybe_emit(frame_counter, &nes);
                // Phase 3: idle_post frames idle.
                for _ in 0..idle_post {
                    nes.set_buttons(0, Buttons::empty());
                    pulse_vs_coin!();
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
                // Phase 4: free-run.
                for _ in 0..free_run {
                    pulse_vs_coin!();
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
            }
            InputScript::DoubleStartTap {
                idle_pre,
                gap,
                idle_post,
                free_run,
                checkpoints: checkpoint_frames,
            } => {
                let mut maybe_emit = |frame: u32, nes: &Nes| {
                    if checkpoint_frames.contains(&frame) {
                        let fb_hash = fnv1a64(nes.framebuffer());
                        checkpoints.push(Checkpoint { frame, fb_hash });
                        dump_frame(&label_safe, &format!("f{frame}"), nes.framebuffer());
                    }
                };
                // (idle_pre idle) → START → (gap idle) → START → (idle_post
                // idle) → free-run. `down` marks the two single-frame taps.
                let pre = idle_pre;
                let second_tap_at = idle_pre + 1 + gap;
                let total = idle_pre + 1 + gap + 1 + idle_post + free_run;
                for f in 0..total {
                    let down = f == pre || f == second_tap_at;
                    nes.set_buttons(
                        0,
                        if down {
                            Buttons::START
                        } else {
                            Buttons::empty()
                        },
                    );
                    pulse_vs_coin!();
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
            }
            InputScript::RepeatStartTap {
                warmup,
                period,
                taps,
                free_run,
                checkpoints: checkpoint_frames,
            } => {
                let mut maybe_emit = |frame: u32, nes: &Nes| {
                    if checkpoint_frames.contains(&frame) {
                        let fb_hash = fnv1a64(nes.framebuffer());
                        checkpoints.push(Checkpoint { frame, fb_hash });
                        dump_frame(&label_safe, &format!("f{frame}"), nes.framebuffer());
                    }
                };
                let total = warmup + taps.saturating_sub(1) * period + 1 + free_run;
                for f in 0..total {
                    // single-frame START tap at warmup, warmup+period, ... (taps total)
                    let is_tap =
                        f >= warmup && (f - warmup) % period == 0 && (f - warmup) / period < taps;
                    nes.set_buttons(
                        0,
                        if is_tap {
                            Buttons::START
                        } else {
                            Buttons::empty()
                        },
                    );
                    pulse_vs_coin!();
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
            }
        }

        // Defensive: ensure the framebuffer is the canonical size.
        // Catches a hypothetical core refactor that changes the
        // pixel-format constants out from under the harness.
        let fb = nes.framebuffer();
        assert_eq!(
            fb.len(),
            (FB_WIDTH as usize) * (FB_HEIGHT as usize) * 4,
            "framebuffer size mismatch: expected 256x240x4, got {}",
            fb.len()
        );
        // Distinct-colour / dominant-colour health of the FINAL frame,
        // via the shared coverage helper. Lets the auto-discovering
        // coverage harness reject a blank / failed-to-render boot.
        let final_frame_health = frame_health(fb);

        let cycles = nes.cycle();
        let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
        for s in &samples {
            audio_bytes.extend_from_slice(&s.to_le_bytes());
        }
        let audio_fnv1a64 = fnv1a64(&audio_bytes);

        Some(CaptureResult {
            rom_sha256_hex,
            checkpoints,
            cycles,
            audio_samples: samples.len(),
            audio_fnv1a64,
            final_frame_health,
        })
    }

    /// Convenience: dump PNG via the common helper but with the
    /// commercial-roms corpus tag baked in.
    fn dump_frame(rom_label: &str, frame_label: &str, fb: &[u8]) {
        dump_frame_if_requested(CORPUS, rom_label, frame_label, fb);
    }

    /// Build the snapshot text for one [`CaptureResult`]. Multi-line
    /// form so each checkpoint stays on its own line for clean diffs:
    ///
    /// ```text
    /// rom=mapper-000-NROM/Super Mario Bros.nes
    /// rom_sha256=abc...
    /// frames=600
    /// fb_bytes=245760
    /// cycles=10741428
    /// audio_samples=240880
    /// audio_fnv1a64=0123456789abcdef
    /// checkpoint f120 fb_fnv1a64=...
    /// checkpoint f240 fb_fnv1a64=...
    /// checkpoint f600 fb_fnv1a64=...
    /// ```
    #[must_use]
    pub fn snapshot_text(rom: &str, script: InputScript, result: &CaptureResult) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "rom={rom}");
        let _ = writeln!(out, "rom_sha256={}", result.rom_sha256_hex);
        let _ = writeln!(out, "frames={}", script.total_frames());
        out.push_str("fb_bytes=245760\n");
        let _ = writeln!(out, "cycles={}", result.cycles);
        let _ = writeln!(out, "audio_samples={}", result.audio_samples);
        let _ = writeln!(out, "audio_fnv1a64={:016x}", result.audio_fnv1a64);
        for cp in &result.checkpoints {
            let _ = writeln!(
                out,
                "checkpoint f{} fb_fnv1a64={:016x}",
                cp.frame, cp.fb_hash
            );
        }
        // Trim trailing newline for tidier snapshot text.
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    /// Compute SHA-256 of the ROM on disk and compare against the
    /// expected SHA from the snapshot. Used by tests that want to fail
    /// fast on a wrong ROM dump (typical case: user has a different
    /// region's dump, or the file was corrupted in transit).
    ///
    /// Returns `Ok(())` on match, `Err(actual_hex)` on mismatch.
    pub fn assert_rom_sha256_or_recapture(rom_rel: &str, expected_hex: &str) -> Result<(), String> {
        let path = external_rom_path(rom_rel);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => return Err(format!("read failed: {e}")),
        };
        let actual = compute_rom_sha256(&bytes);
        if actual == expected_hex {
            Ok(())
        } else {
            Err(actual)
        }
    }
}
