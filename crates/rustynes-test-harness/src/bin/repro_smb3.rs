//! SMB3 (MMC3) "Mario flashing in World 1-1" reproduction harness (v1.2.0).
//!
//! NOT part of CI. Diagnostic tool only; uses a gitignored external ROM dump.
//!
//! Scripts the input path from power-on to World 1-1 (tap Start past the title,
//! tap Start/A to start the game + enter the first level), then captures a run
//! of consecutive frames. It dumps a PNG filmstrip (every Nth frame) so the
//! progression can be eyeballed to confirm we reach 1-1, and reports a
//! frame-parity oscillation metric over the capture window: if Mario "flashes"
//! every other frame, the mean per-pixel difference between frame f and f-2
//! (same parity) is small while f vs f-1 (opposite parity) is large.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms \
//!     --bin repro_smb3 -- <rom-path> <out-dir>
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown
)]

use std::path::Path;

use rustynes_core::{Buttons, Nes};

const W: u32 = 256;
const H: u32 = 240;

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

/// Mean absolute per-byte difference between two RGBA frames.
fn frame_diff(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let sum: u64 = (0..n).map(|i| u64::from(a[i].abs_diff(b[i]))).sum();
    sum as f64 / n as f64
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: repro_smb3 <rom-path> <out-dir>");
        std::process::exit(2);
    }
    let rom_path = &args[1];
    let out_dir = Path::new(&args[2]);
    std::fs::create_dir_all(out_dir).expect("create out dir");

    let bytes = std::fs::read(rom_path).unwrap_or_else(|e| panic!("read {rom_path}: {e}"));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_path}: {e}"));

    // Scripted path to World 1-1. Timings are generous taps with idle gaps; the
    // filmstrip lets us verify/refine where we land.
    // (frame budget, buttons-held-for-first-8-frames-of-each-tap)
    let script: &[(u64, Buttons)] = &[
        (220, Buttons::empty()), // title / curtain demo
        (16, Buttons::START),    // tap Start -> past title
        (120, Buttons::empty()), // -> world map + "WORLD 1 MARIO x4" intro card
        (16, Buttons::START),    // tap Start -> dismiss the intro card
        (520, Buttons::empty()), // the "WORLD 1 MARIO x4" card persists ~hundreds
        // of frames before it auto-clears; wait it out (Mario starts on 1-1).
        (16, Buttons::A),        // tap A -> enter 1-1
        (220, Buttons::empty()), // 1-1 loads + Mario idle
    ];

    let mut frame = 0u64;
    let dump_every = 15u64;
    for &(dur, held) in script {
        for i in 0..dur {
            // Hold the button only for the first 8 frames of a tap, then release.
            let buttons = if i < 8 { held } else { Buttons::empty() };
            nes.set_buttons(0, buttons);
            let fb = nes.run_frame();
            if frame % dump_every == 0 {
                write_png(&out_dir.join(format!("f{frame:04}.png")), fb);
            }
            frame += 1;
        }
    }

    // Capture window: 90 consecutive frames in (hopefully) 1-1, idle.
    eprintln!("--- capture window (frame {frame}..) ---");
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for _ in 0..90 {
        nes.set_buttons(0, Buttons::empty());
        frames.push(nes.run_frame().to_vec());
        frame += 1;
    }
    // Dump the first few of the window for visual inspection.
    for (k, fb) in frames.iter().enumerate().take(6) {
        write_png(&out_dir.join(format!("win{k:02}.png")), fb);
    }

    // Oscillation metric: adjacent (opposite-parity) vs skip-one (same-parity).
    let adj: f64 = (1..frames.len())
        .map(|i| frame_diff(&frames[i], &frames[i - 1]))
        .sum::<f64>()
        / (frames.len() - 1) as f64;
    let skip: f64 = (2..frames.len())
        .map(|i| frame_diff(&frames[i], &frames[i - 2]))
        .sum::<f64>()
        / (frames.len() - 2) as f64;
    println!("mean adjacent-frame diff   (f vs f-1): {adj:.4}");
    println!("mean skip-one-frame diff   (f vs f-2): {skip:.4}");
    println!(
        "parity-oscillation ratio (adj/skip): {:.2}  (>~2 suggests a 2-frame flicker)",
        if skip > 0.0 { adj / skip } else { 0.0 }
    );
    eprintln!("wrote filmstrip + window PNGs to {}", out_dir.display());
}
