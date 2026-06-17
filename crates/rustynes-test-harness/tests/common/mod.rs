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
        let path = external_rom_path(rom_rel);
        let bytes = fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "read {}: {} — is the ROM staged at tests/roms/external/?",
                path.display(),
                e
            )
        });
        let rom_sha256_hex = compute_rom_sha256(&bytes);
        let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_rel}: {e}"));

        let label_safe = sanitize(rom_rel);
        let mut checkpoints: Vec<Checkpoint> = Vec::new();
        let mut samples: Vec<f32> = Vec::new();

        // Inner helper: tick `n` frames with the given button state and
        // drain audio each frame.
        let mut frame_counter: u32 = 0;
        macro_rules! tick_with {
            ($buttons:expr, $count:expr) => {{
                let buttons: Buttons = $buttons;
                let n: u32 = $count;
                for _ in 0..n {
                    nes.set_buttons(0, buttons);
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
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
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
                // Phase 2: 1 frame START down.
                nes.set_buttons(0, Buttons::START);
                nes.run_frame();
                samples.extend(nes.drain_audio());
                frame_counter += 1;
                maybe_emit(frame_counter, &nes);
                // Phase 3: idle_post frames idle.
                for _ in 0..idle_post {
                    nes.set_buttons(0, Buttons::empty());
                    nes.run_frame();
                    samples.extend(nes.drain_audio());
                    frame_counter += 1;
                    maybe_emit(frame_counter, &nes);
                }
                // Phase 4: free-run.
                for _ in 0..free_run {
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

        let cycles = nes.cycle();
        let mut audio_bytes: Vec<u8> = Vec::with_capacity(samples.len() * 4);
        for s in &samples {
            audio_bytes.extend_from_slice(&s.to_le_bytes());
        }
        let audio_fnv1a64 = fnv1a64(&audio_bytes);

        CaptureResult {
            rom_sha256_hex,
            checkpoints,
            cycles,
            audio_samples: samples.len(),
            audio_fnv1a64,
        }
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
