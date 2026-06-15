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

fn write_png_sized(path: &Path, rgba: &[u8], width: u32, height: u32) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}

/// Crop a `cw`x`ch` region at (`cx`,`cy`) and magnify `zoom`x (nearest), so a
/// running-Mario sprite (camera-locked near screen centre) is legible + any
/// per-frame missing-sprite-portion flicker is obvious.
fn crop_zoom(fb: &[u8], cx: usize, cy: usize, cw: usize, ch: usize, zoom: usize) -> Vec<u8> {
    let w = W as usize;
    let out_w = cw * zoom;
    let mut out = vec![0u8; out_w * ch * zoom * 4];
    for y in 0..ch {
        for x in 0..cw {
            let src = ((cy + y) * w + (cx + x)) * 4;
            let px = [fb[src], fb[src + 1], fb[src + 2], fb[src + 3]];
            for zy in 0..zoom {
                for zx in 0..zoom {
                    let dst = (((y * zoom + zy) * out_w) + (x * zoom + zx)) * 4;
                    out[dst..dst + 4].copy_from_slice(&px);
                }
            }
        }
    }
    out
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

    // Optional `--state <file>`: restore a save-state (e.g. one taken in the app
    // with F1 while Mario is mid-run and flickering) and capture straight from
    // there, skipping the scripted path to 1-1.
    let state_path = args
        .iter()
        .position(|a| a == "--state")
        .and_then(|i| args.get(i + 1).cloned());

    let bytes = std::fs::read(rom_path).unwrap_or_else(|e| panic!("read {rom_path}: {e}"));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_path}: {e}"));

    // Scripted path to World 1-1. Timings are generous taps with idle gaps; the
    // filmstrip lets us verify/refine where we land.
    // (frame budget, buttons-held-for-first-8-frames-of-each-tap)
    // Path to World 1-1 (per the player's known sequence): wait out the title +
    // the long "WORLD 1 MARIO x4" intro card, then on the map press RIGHT once,
    // UP once (to land on the Stage 1 node), then A to enter the level.
    let script: &[(u64, Buttons)] = &[
        (220, Buttons::empty()), // title / curtain demo
        (16, Buttons::START),    // tap Start -> "1/2 PLAYER GAME" select menu
        (80, Buttons::empty()),  // menu appears
        (16, Buttons::START),    // tap Start -> confirm 1-player -> world map + card
        (560, Buttons::empty()), // the intro card auto-clears (~500+ frames) and
        // absorbs inputs while up; wait it out fully before navigating.
        (8, Buttons::RIGHT),    // map: move right one node
        (40, Buttons::empty()), // auto-walk completes
        (8, Buttons::UP),       // map: move up one node -> land on Stage 1
        (40, Buttons::empty()), // auto-walk completes
        (8, Buttons::A),        // enter Stage 1 (World 1-1)
        (40, Buttons::empty()),
        (8, Buttons::B),         // fallback: B enters if A opened the item menu
        (260, Buttons::empty()), // 1-1 loads + Mario idle
    ];

    let mut frame = 0u64;
    let dump_every = 15u64;
    if let Some(sp) = &state_path {
        let blob = std::fs::read(sp).unwrap_or_else(|e| panic!("read state {sp}: {e}"));
        nes.restore(&blob)
            .unwrap_or_else(|e| panic!("restore state {sp}: {e:?}"));
        eprintln!("restored save-state from {sp}; capturing from there");
    } else {
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
    }

    capture_window(&mut nes, out_dir, frame);
}

/// Parity-oscillation metric over a frame sequence: mean adjacent (f vs f-1,
/// opposite parity) vs skip-one (f vs f-2, same parity). A per-other-frame
/// flicker pushes the adj/skip ratio well above 1.
fn parity_ratio(frames: &[Vec<u8>]) -> (f64, f64, f64) {
    let adj: f64 = (1..frames.len())
        .map(|i| frame_diff(&frames[i], &frames[i - 1]))
        .sum::<f64>()
        / (frames.len().saturating_sub(1)).max(1) as f64;
    let skip: f64 = (2..frames.len())
        .map(|i| frame_diff(&frames[i], &frames[i - 2]))
        .sum::<f64>()
        / (frames.len().saturating_sub(2)).max(1) as f64;
    (adj, skip, if skip > 0.0 { adj / skip } else { 0.0 })
}

/// Run Mario right (B = run) for a window — the flicker only shows while he's
/// moving — dumping each full frame + a zoomed Mario-band crop, and report the
/// parity metric for both the full frame and the crop band.
fn capture_window(nes: &mut Nes, out_dir: &Path, start_frame: u64) {
    eprintln!("--- capture window (frame {start_frame}.., running right) ---");
    // Centre band where SMB3 camera-locks a running Mario (~screen-centre X).
    let (cx, cy, cw, ch, zoom) = (88usize, 96usize, 80usize, 112usize, 3usize);
    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut crops: Vec<Vec<u8>> = Vec::new();
    for k in 0..40 {
        nes.set_buttons(0, Buttons::RIGHT | Buttons::B);
        let fb = nes.run_frame().to_vec();
        write_png(&out_dir.join(format!("win{k:02}.png")), &fb);
        let crop = crop_zoom(&fb, cx, cy, cw, ch, zoom);
        write_png_sized(
            &out_dir.join(format!("crop{k:02}.png")),
            &crop,
            (cw * zoom) as u32,
            (ch * zoom) as u32,
        );
        crops.push(crop);
        frames.push(fb);
    }
    let (cadj, cskip, cratio) = parity_ratio(&crops);
    println!("Mario-band crop: adj={cadj:.4} skip={cskip:.4} ratio={cratio:.2}");
    let (adj, skip, ratio) = parity_ratio(&frames);
    println!("mean adjacent-frame diff   (f vs f-1): {adj:.4}");
    println!("mean skip-one-frame diff   (f vs f-2): {skip:.4}");
    println!("parity-oscillation ratio (adj/skip): {ratio:.2}  (>~2 suggests a 2-frame flicker)");
    eprintln!("wrote filmstrip + window PNGs to {}", out_dir.display());
}
