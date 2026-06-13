//! Headless framebuffer capture + per-frame timing harness for the
//! v1.3.x left-edge sprite-tint / stutter regression investigation.
//!
//! NOT part of CI. Diagnostic tool only. Reuses the committed external
//! ROM dumps under `tests/roms/external/` (gitignored; never committed).
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms \
//!     --bin capture_left_edge -- <rom-path> <frames> <out-dir> [dump-frame ...]
//! ```
//!
//! Outputs, into `<out-dir>`:
//! - `<stem>_f<N>.png`        full 256x240 frame at each requested frame
//! - `<stem>_f<N>_left32.png` leftmost 32 columns, 8x horizontal zoom
//!
//! Always prints per-frame `run_frame` wall-time stats (min/median/p99/max
//! + the 10 slowest frames) so the stutter spike is visible.

// Diagnostic dev tool only: every cast here is on an NES-bounded dimension
// (W=256, H=240, small crop/frame counts) or a wall-clock timing stat, where
// the pedantic cast lints cannot represent a real truncation/precision bug.
// Allow them file-wide rather than annotating each diagnostic line.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::path::Path;
use std::time::Instant;

use rustynes_core::{Buttons, Nes};

const W: usize = 256;
const H: usize = 240;

fn write_png(path: &Path, fb: &[u8], width: u32, height: u32) {
    let file = std::fs::File::create(path).expect("create png");
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
    eprintln!("wrote {} ({width}x{height})", path.display());
}

/// Crop the leftmost `cols` columns and magnify horizontally by `zoom`
/// so single-pixel left-edge artifacts are obvious in the PNG.
fn left_crop_zoom(fb: &[u8], cols: usize, zoom: usize) -> (Vec<u8>, u32, u32) {
    let out_w = cols * zoom;
    let mut out = vec![0u8; out_w * H * 4];
    for y in 0..H {
        for x in 0..cols {
            let src = (y * W + x) * 4;
            let px = [fb[src], fb[src + 1], fb[src + 2], fb[src + 3]];
            for z in 0..zoom {
                let dst = (y * out_w + x * zoom + z) * 4;
                out[dst..dst + 4].copy_from_slice(&px);
            }
        }
    }
    (out, out_w as u32, H as u32)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: {} <rom> <frames> <out-dir> [dump-frame ...]",
            args[0]
        );
        std::process::exit(2);
    }
    let rom_path = &args[1];
    let frames: u64 = args[2].parse().expect("frames must be a number");
    let out_dir = &args[3];
    let dump_frames: Vec<u64> = args[4..]
        .iter()
        .map(|s| s.parse().expect("dump-frame must be a number"))
        .collect();

    std::fs::create_dir_all(out_dir).expect("mkdir out-dir");

    let bytes = std::fs::read(rom_path).expect("read rom");
    let mut nes = Nes::from_rom(&bytes).expect("parse rom");

    // When RUSTYNES_REWIND is set, enable the per-frame rewind capture so
    // the timing distribution reflects the frontend's real
    // rewind-enabled hot path (the keyframe-every-N-frames spike is the
    // stutter suspect). Honor RUSTYNES_REWIND_KEYFRAME for the period.
    if std::env::var_os("RUSTYNES_REWIND").is_some() {
        let kf: u32 = std::env::var("RUSTYNES_REWIND_KEYFRAME")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        // 60 s @ 60 fps budget, same shape as the frontend default.
        nes.enable_rewind_with(32 * 1024 * 1024, kf);
        eprintln!("rewind ENABLED (keyframe_period={kf})");
    }

    let stem = Path::new(rom_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("rom")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();

    // Scripted Start presses to reach in-game menus from a title screen.
    // `RUSTYNES_PRESS_START="t0,t1,..."` presses Start (player 1) on each
    // listed frame index for `RUSTYNES_PRESS_HOLD` frames (default 4), then
    // releases. Used to drive Mega Man 3 from title -> stage-select for the
    // MMC3 raster-split capture. Pure scripted input; the core stays
    // deterministic.
    let press_frames: Vec<u64> = std::env::var("RUSTYNES_PRESS_START")
        .ok()
        .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_default();
    let press_hold: u64 = std::env::var("RUSTYNES_PRESS_HOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    if !press_frames.is_empty() {
        eprintln!("scripted Start presses at frames {press_frames:?} (hold={press_hold})");
    }

    let mut times_ns: Vec<u128> = Vec::with_capacity(frames as usize);

    for f in 0..frames {
        // Apply scripted Start press window for this frame.
        let pressing = press_frames.iter().any(|&p| f >= p && f < p + press_hold);
        nes.set_buttons(
            0,
            if pressing {
                Buttons::START
            } else {
                Buttons::empty()
            },
        );

        let t0 = Instant::now();
        nes.run_frame();
        times_ns.push(t0.elapsed().as_nanos());

        if dump_frames.contains(&(f + 1)) {
            let fb = nes.framebuffer();
            let full = Path::new(out_dir).join(format!("{stem}_f{}.png", f + 1));
            write_png(&full, fb, W as u32, H as u32);
            let (crop, cw, ch) = left_crop_zoom(fb, 32, 8);
            let left = Path::new(out_dir).join(format!("{stem}_f{}_left32.png", f + 1));
            write_png(&left, &crop, cw, ch);
        }
    }

    // Timing stats.
    let mut sorted = times_ns.clone();
    sorted.sort_unstable();
    let n = sorted.len();
    let pct = |p: f64| sorted[((n as f64 * p) as usize).min(n - 1)];
    let to_ms = |ns: u128| ns as f64 / 1.0e6;
    eprintln!("--- per-frame run_frame() wall-time over {n} frames ---");
    eprintln!("min    {:.3} ms", to_ms(sorted[0]));
    eprintln!("median {:.3} ms", to_ms(pct(0.50)));
    eprintln!("p90    {:.3} ms", to_ms(pct(0.90)));
    eprintln!("p99    {:.3} ms", to_ms(pct(0.99)));
    eprintln!("max    {:.3} ms", to_ms(sorted[n - 1]));
    let sum: u128 = times_ns.iter().sum();
    eprintln!("mean   {:.3} ms", to_ms(sum / n as u128));

    // 10 slowest frames with their frame index.
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_unstable_by(|&a, &b| times_ns[b].cmp(&times_ns[a]));
    eprintln!("--- 10 slowest frames (index : ms) ---");
    for &i in idx.iter().take(10) {
        eprintln!("frame {i:>5} : {:.3} ms", to_ms(times_ns[i]));
    }
}
