//! Generic boot-and-render smoke harness — boots any ROM headless and reports,
//! at several frame checkpoints, whether it renders a real screen rather than a
//! backdrop-only colour. Handy for spot-checking that a newly-added mapper
//! actually runs against a real cartridge.
//!
//! NOT part of CI. Diagnostic tool only. Uses gitignored external ROM dumps
//! under `tests/roms/external/` (never committed).
//!
//! It reports objective render statistics per checkpoint: the distinct-colour
//! count and the dominant-colour fraction. A backdrop-only frame has ~1 colour
//! and a dominant fraction near 1.0; a rendered game screen has several distinct
//! colours and no single colour near-totally dominant.
//!
//! Motivating case — the v1.2.0 mapper-89 (Sunsoft-2) bus-conflict fix: the
//! documented bug (`docs/compatibility.md` §m89) left *Tenka no Goikenban: Mito
//! Koumon* with an empty background that degraded to a backdrop colour after
//! ~400 frames. The checkpoints straddle that mark so the fix (and any
//! regression) is detectable without eyeballing a PNG.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms \
//!     --bin render_smoke -- <rom-path> [frames] [out.png]
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown
)]

use std::collections::HashMap;
use std::path::Path;

use rustynes_core::{Buttons, Nes};

const W: u32 = 256;
const H: u32 = 240;

/// (distinct colour count, dominant-colour fraction) over the RGBA framebuffer.
fn frame_stats(fb: &[u8]) -> (usize, f64) {
    let mut counts: HashMap<[u8; 4], u32> = HashMap::new();
    for px in fb.chunks_exact(4) {
        *counts.entry([px[0], px[1], px[2], px[3]]).or_insert(0) += 1;
    }
    let total: u32 = counts.values().sum();
    let dominant = counts.values().copied().max().unwrap_or(0);
    (counts.len(), f64::from(dominant) / f64::from(total.max(1)))
}

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
    eprintln!("wrote {}", path.display());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: capture_m89 <rom-path> [frames] [out.png]");
        std::process::exit(2);
    }
    let rom_path = &args[1];
    let frames: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let out_png = args.get(3);

    let bytes = std::fs::read(rom_path).unwrap_or_else(|e| panic!("read {rom_path}: {e}"));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom_path}: {e}"));

    // Checkpoints straddle the ~400-frame degradation point from the bug report.
    let checkpoints = [60u64, 200, 450, frames];
    println!("frame |  colours | dominant% | verdict");
    println!("------+----------+-----------+--------");
    let mut all_rendered = true;
    for f in 1..=frames {
        nes.set_buttons(0, Buttons::empty());
        let fb = nes.run_frame();
        if checkpoints.contains(&f) {
            let (colours, dom) = frame_stats(fb);
            // Backdrop-only degradation (the bug) collapses the frame to one or
            // two colours filling almost the whole screen. A rendered screen has
            // several distinct colours and no single colour near-totally
            // dominant. (Mito Koumon's title uses a deliberately small palette.)
            let rendered = colours >= 4 && dom < 0.95;
            all_rendered &= rendered;
            println!(
                "{f:5} | {colours:8} | {:8.1}% | {}",
                dom * 100.0,
                if rendered {
                    "RENDERED"
                } else {
                    "BACKDROP-ONLY"
                }
            );
            if let Some(p) = out_png {
                if f == frames {
                    write_png(Path::new(p), fb);
                }
            }
        }
    }
    println!();
    if all_rendered {
        println!("PASS: a real screen renders at every checkpoint (no backdrop-only degradation).");
    } else {
        println!("FAIL: a checkpoint degraded to backdrop-only.");
        std::process::exit(1);
    }
}
