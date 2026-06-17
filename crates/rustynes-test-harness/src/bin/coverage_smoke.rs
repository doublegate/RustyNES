//! Boot-and-screenshot smoke harness for the commercial-title compatibility
//! survey (v2.4.0). NOT part of CI. Diagnostic only.
//!
//! Walks `tests/roms/external/` (gitignored; never committed), boots every
//! `.nes` for N frames with a brief Start pulse, dumps a 256x240 PNG per ROM,
//! and prints a per-ROM health line: PANIC, or OK with the distinct-colour
//! count (a blank/crashed boot shows <=4 colours; a real frame shows dozens),
//! flagging SUSPICIOUS frames for visual review.
//!
//! The walk and the blank/few-colour health verdict are shared with the
//! `external_coverage` integration test via
//! `rustynes_test_harness::coverage` (`walk_nes` + `frame_health` /
//! `FrameHealth::looks_blank`) so the bin and the test apply the SAME
//! heuristic.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms --release \
//!     --bin coverage_smoke -- <external-dir> <frames> <out-dir> [name-filter]
//! ```

use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use rustynes_core::{Buttons, Nes};
use rustynes_test_harness::coverage::{FrameHealth, frame_health, walk_nes};

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, 256, 240);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ext_dir = args.get(1).map_or("tests/roms/external", |s| s.as_str());
    let frames: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(280);
    let out_dir = args.get(3).map_or("/tmp/rustynes-coverage", |s| s.as_str());
    let filter = args.get(4).cloned().unwrap_or_default();
    let start_at: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(90);
    std::fs::create_dir_all(out_dir).expect("mkdir out");

    let mut roms = Vec::new();
    walk_nes(Path::new(ext_dir), &mut roms);
    roms.sort();
    roms.retain(|p| filter.is_empty() || p.to_string_lossy().contains(&filter));

    let (mut ok, mut sus, mut panicked) = (0u32, 0u32, 0u32);
    for rom in &roms {
        let label = rom
            .strip_prefix(ext_dir)
            .unwrap_or(rom)
            .to_string_lossy()
            .replace('/', "__")
            .replace(".nes", "");
        let bytes = std::fs::read(rom).expect("read rom");
        let vs_coin = std::env::var("RUSTYNES_VS_COIN").is_ok();
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
            let mut nes = Nes::from_rom(&bytes).map_err(|e| e.to_string())?;
            // Vs. carts: apply the per-game DB's RGB-PPU type (palette LUT) so
            // the frame renders the right colours, and set DIP 0. Mirrors the
            // `external_coverage` harness + the frontend's `apply_vs_db`.
            if nes.is_vs_system() {
                let dip = rustynes_core::vs_db::lookup(nes.rom_sha256()).map_or(0, |entry| {
                    nes.set_vs_ppu_type(entry.vs_ppu_type);
                    entry.vs_dip
                });
                nes.set_vs_dip(dip);
            }
            for f in 0..frames {
                // Vs. System (mapper 99) games sit on an insert-coin attract
                // loop; pulse a coin on acceptor #1 every ~120 frames when
                // RUSTYNES_VS_COIN is set so they leave the boot screen.
                if vs_coin && nes.is_vs_system() {
                    if f % 120 == 30 {
                        nes.insert_coin(0);
                    }
                    if f % 120 == 34 {
                        nes.clear_coin();
                    }
                }
                // Start pulses to advance title/intro screens: a configurable
                // window (arg 5, default 90) plus repeats every 600 frames so a
                // long intro that needs several taps still advances.
                let btn = if (start_at..start_at + 10).contains(&(f % 600)) {
                    Buttons::START
                } else {
                    Buttons::empty()
                };
                nes.set_buttons(0, btn);
                nes.run_frame();
            }
            Ok(nes.framebuffer().to_vec())
        }));
        match result {
            Ok(Ok(fb)) => {
                let health = frame_health(&fb);
                let FrameHealth {
                    distinct_colors: n,
                    dominant_fraction,
                } = health;
                write_png(&Path::new(out_dir).join(format!("{label}.png")), &fb);
                if health.looks_blank() {
                    sus += 1;
                    println!(
                        "SUSPICIOUS  {n:3} colours  dom={:5.1}%  {label}",
                        dominant_fraction * 100.0
                    );
                } else {
                    ok += 1;
                    println!(
                        "ok          {n:3} colours  dom={:5.1}%  {label}",
                        dominant_fraction * 100.0
                    );
                }
            }
            Ok(Err(e)) => {
                panicked += 1;
                println!("PARSE-ERR   {label}: {e}");
            }
            Err(_) => {
                panicked += 1;
                println!("PANIC       {label}");
            }
        }
    }
    println!(
        "\n{} ROMs: ok={ok} suspicious={sus} panic/err={panicked} -> {out_dir}",
        roms.len()
    );
}
