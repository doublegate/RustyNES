//! Headless batch runner (v1.8.9 "Backlog") — run a manifest of ROMs, capturing a
//! screenshot, a deterministic framebuffer hash, and a blank-frame health verdict
//! for each, and emit one JSON object per ROM. Consolidates the ad-hoc `scripts/`
//! boot-smoke verifiers behind a single reusable, scriptable entry point.
//!
//! Usage:
//!   `batch_runner [--frames N] [--start-at F] [--out DIR] <input>...`
//!
//! Each `<input>` is a ROM file, a directory (walked for `.nes` / `.unf` / …), or a
//! manifest text file (one path per line, optional `<TAB>label`; `#` comments and
//! blank lines ignored). Screenshots go under `--out` (default `/tmp/rustynes-batch`).
//! Deterministic — the same ROM + `--frames` always yields the same `fb_hash`, so
//! the JSON output doubles as a boot-regression baseline.
//!
//! Built only with `--features commercial-roms` (it writes PNGs via the optional
//! `png` dep), like the sibling `coverage_smoke` bin.

use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use rustynes_test_harness::coverage::{frame_health, run_rom_headless, walk_nes};

/// FNV-1a 64-bit — a tiny, dependency-free, stable framebuffer fingerprint.
fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Minimal JSON string escaping (paths / labels may carry `\`, `"`, control chars).
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A filesystem-safe label from a ROM path (file stem, `/` -> `__`).
fn label_for(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "rom".to_owned(),
        |s| s.to_string_lossy().replace('/', "__"),
    )
}

/// A manifest is a text-ish file (no extension, or `.txt` / `.manifest` / `.list`).
fn is_manifest(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_none_or(|e| matches!(e.to_ascii_lowercase().as_str(), "txt" | "manifest" | "list"))
}

/// Expand one CLI input into `(rom_path, label)` pairs.
fn collect(input: &str, out: &mut Vec<(PathBuf, String)>) {
    let path = Path::new(input);
    if path.is_dir() {
        let mut roms = Vec::new();
        walk_nes(path, &mut roms);
        roms.sort();
        out.extend(roms.into_iter().map(|r| {
            let label = label_for(&r);
            (r, label)
        }));
    } else if is_manifest(path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("batch_runner: cannot read manifest {}: {e}", path.display());
                return;
            }
        };
        // Relative entries resolve against the manifest's own directory (absolute
        // paths stay as-is) — the least-surprising behaviour for a portable list.
        let base = path.parent();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (p, lbl) = line
                .split_once('\t')
                .map_or((line, None), |(p, l)| (p, Some(l)));
            let entry = Path::new(p);
            let pb = match base {
                Some(b) if entry.is_relative() => b.join(entry),
                _ => entry.to_path_buf(),
            };
            let label = lbl.map_or_else(|| label_for(&pb), str::to_owned);
            out.push((pb, label));
        }
    } else {
        out.push((path.to_path_buf(), label_for(path)));
    }
}

/// Write a 256x240 RGBA8 framebuffer to `path` as a PNG. Returns an error string
/// (rather than panicking) so a single bad screenshot doesn't abort the batch.
fn write_png(path: &Path, fb: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("create png: {e}"))?;
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, 256, 240);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(fb)
        .map_err(|e| format!("png data: {e}"))?;
    Ok(())
}

/// A filesystem-safe filename component: map anything but `[A-Za-z0-9_-]` to `_`,
/// so a manifest-supplied label can never escape `--out` or break path joins.
fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Emit a one-line JSON error record for a ROM.
fn print_error(label: &str, path: &str, err: &str) {
    println!(
        "{{\"label\":{},\"path\":{},\"error\":{}}}",
        json_str(label),
        json_str(path),
        json_str(err)
    );
}

fn main() {
    let mut frames: u64 = 280;
    let mut start_at: u64 = 90;
    let mut out_dir = String::from("/tmp/rustynes-batch");
    let mut inputs: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--frames" => frames = args.next().and_then(|s| s.parse().ok()).unwrap_or(frames),
            "--start-at" => {
                start_at = args.next().and_then(|s| s.parse().ok()).unwrap_or(start_at);
            }
            "--out" => out_dir = args.next().unwrap_or(out_dir),
            "-h" | "--help" => {
                eprintln!(
                    "usage: batch_runner [--frames N] [--start-at F] [--out DIR] <rom|dir|manifest>..."
                );
                return;
            }
            other => inputs.push(other.to_owned()),
        }
    }
    if inputs.is_empty() {
        eprintln!("batch_runner: no inputs given. Try --help.");
        std::process::exit(2);
    }
    std::fs::create_dir_all(&out_dir).expect("mkdir out");

    let mut roms: Vec<(PathBuf, String)> = Vec::new();
    for input in &inputs {
        collect(input, &mut roms);
    }

    let (mut rendered, mut blank, mut failed) = (0u32, 0u32, 0u32);
    for (rom, label) in &roms {
        let path_str = rom.to_string_lossy();
        let bytes = match std::fs::read(rom) {
            Ok(b) => b,
            Err(e) => {
                failed += 1;
                print_error(label, &path_str, &format!("read error: {e}"));
                continue;
            }
        };
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            run_rom_headless(&bytes, frames, start_at)
        }));
        match result {
            Ok(Ok(fb)) => {
                let hash = fnv1a(&fb);
                let health = frame_health(&fb);
                let is_blank = health.looks_blank();
                let shot = Path::new(&out_dir).join(format!("{}.png", sanitize_label(label)));
                if let Err(e) = write_png(&shot, &fb) {
                    failed += 1;
                    print_error(label, &path_str, &e);
                    continue;
                }
                if is_blank {
                    blank += 1;
                } else {
                    rendered += 1;
                }
                println!(
                    "{{\"label\":{},\"path\":{},\"frames\":{frames},\"fb_hash\":\"{hash:016x}\",\"distinct_colors\":{},\"blank\":{is_blank},\"screenshot\":{}}}",
                    json_str(label),
                    json_str(&path_str),
                    health.distinct_colors,
                    json_str(&shot.to_string_lossy()),
                );
            }
            Ok(Err(e)) => {
                failed += 1;
                print_error(label, &path_str, &e);
            }
            Err(_) => {
                failed += 1;
                print_error(label, &path_str, "panicked");
            }
        }
    }
    eprintln!(
        "batch_runner: {} ROMs - {rendered} rendered, {blank} blank/suspicious, {failed} failed",
        roms.len()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
