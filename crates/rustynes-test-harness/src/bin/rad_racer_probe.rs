//! Rad Racer right-of-road artifact probe — replay a recorded `.rnm` and look at
//! what actually changes, frame to frame, in the roadside region.
//!
//! NOT part of CI. Diagnostic tool only. Uses gitignored external ROM dumps under
//! `tests/roms/external/` (never committed).
//!
//! # The report
//!
//! Rad Racer is **MMC1**, which has no scanline IRQ, so its per-scanline road
//! perspective is driven by sprite-0 hit plus timed CPU writes. A write that
//! lands a few dots late corrupts the END of a scanline — the right-hand side —
//! which is exactly where the artifact was reported.
//!
//! The core is deterministic, so "flickers frame to frame" cannot mean noise. It
//! means the emitted pixels genuinely differ between frames, and the question is
//! whether that difference is the game scrolling (expected) or the renderer
//! disagreeing with itself about where a scanline ends (a bug). This prints, per
//! frame:
//!
//! - `chg` — pixels in the roadside region that differ from the previous frame.
//! - `rowmax` — the largest single-row change count. Smooth scrolling spreads
//!   change evenly; a mid-scanline write fault concentrates it in a few rows.
//! - `edge` — changes confined to the RIGHTMOST 16 pixels. A scroll changes the
//!   whole region; a scanline-end fault changes mostly this.
//! - `hdisc` — horizontal discontinuities: adjacent pixels differing by a large
//!   palette-index jump, counted only in the right quarter. Tile-boundary
//!   garbage shows here and smooth gradients do not.
//!
//! With `--dump-from N --dump-count K` it also writes `K` consecutive full
//! frames as PNG so the artifact can be looked at rather than inferred.

use std::path::{Path, PathBuf};

use rustynes_core::{Movie, MoviePlayer, Nes};

const W: u32 = 256;
const H: u32 = 240;

/// The roadside strip: right of the road, below the horizon. Chosen to exclude
/// the HUD (top ~32 rows) and the horizon band the v2.3.0 investigation already
/// cleared, so a signal here is not that known-good behaviour.
const RX: usize = 168;
const RY: usize = 96;
const RW: usize = 256 - RX;
const RH: usize = 200 - RY;

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

/// Index-framebuffer view of the region, one palette index per pixel.
fn region_indices(idx: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(RW * RH);
    for y in RY..RY + RH {
        for x in RX..RX + RW {
            out.push(idx[y * 256 + x]);
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "usage: rad_racer_probe <rom> <movie.rnm> [--out DIR] \
             [--dump-from N] [--dump-count K]"
        );
        std::process::exit(2);
    }
    let rom_path = &args[1];
    let movie_path = &args[2];
    let mut out_dir = PathBuf::from("/tmp/rad_racer_probe");
    let mut dump_from = u64::MAX;
    let mut dump_count = 0u64;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                out_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--dump-from" => {
                dump_from = args[i + 1].parse().expect("frame index");
                i += 2;
            }
            "--dump-count" => {
                dump_count = args[i + 1].parse().expect("count");
                i += 2;
            }
            other => panic!("unknown argument: {other}"),
        }
    }
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let rom = std::fs::read(rom_path).unwrap_or_else(|e| panic!("read rom: {e}"));
    let mut nes = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("load rom: {e}"));

    let blob = std::fs::read(movie_path).unwrap_or_else(|e| panic!("read movie: {e}"));
    let movie = Movie::deserialize(&blob).unwrap_or_else(|e| panic!("parse movie: {e}"));
    movie
        .seek_to_start(&mut nes)
        .unwrap_or_else(|e| panic!("seek movie start (ROM mismatch?): {e}"));
    eprintln!(
        "replaying {} frames (region {:?}) from {}",
        movie.len(),
        movie.region,
        movie_path
    );
    println!("frame  chg  rowmax  edge  hdisc");

    let mut player = MoviePlayer::new(&movie);
    let mut prev: Option<Vec<u16>> = None;
    let mut anomaly_frames = 0usize;
    let mut anomaly_px = 0usize;
    let mut rows_seen = std::collections::BTreeSet::<(usize, usize)>::new();
    let mut frame = 0u64;
    while player.apply_next(&mut nes) {
        let fb = nes.run_frame().to_vec();
        let idx = nes.index_framebuffer();
        let cur = region_indices(idx);

        // --- below-horizon anomaly detector -------------------------------
        // The reported artifact is a short band of stray pixels immediately
        // BELOW the sky/ground boundary, on the right. Find the horizon per
        // frame (the last row still containing sky), take the modal ground
        // colour beneath it, and count pixels that are neither.
        {
            let sky = idx[8 * 256 + 8]; // top-left is sky in this scene
            let horizon = (100..180)
                .filter(|&y| (0..256).any(|x| idx[y * 256 + x] == sky))
                .next_back();
            if let Some(hz) = horizon {
                let lo = hz + 3;
                let hi = (hz + 40).min(200);
                let mut hist = std::collections::HashMap::<u16, usize>::new();
                for y in lo..hi {
                    for x in 0..256usize {
                        *hist.entry(idx[y * 256 + x]).or_default() += 1;
                    }
                }
                if let Some((&ground, _)) = hist.iter().max_by_key(|&(_, n)| *n) {
                    // TIGHT window. The first detector counted the road, the
                    // car and the roadside stripes as "not ground" and reported
                    // 5.8M stray pixels across the movie, which is noise, not
                    // signal. The reported artifact is a SHORT band in the first
                    // few rows under the horizon, well right of the road (which
                    // is narrow and central up there), so bound it to exactly
                    // that and let a real regression be the only thing that
                    // trips it.
                    let mut n = 0usize;
                    let mut minx = 255usize;
                    let mut maxx = 0usize;
                    let mut miny = 255usize;
                    let mut maxy = 0usize;
                    for y in (hz + 1)..(hz + 6).min(hi) {
                        for x in 200..256usize {
                            let v = idx[y * 256 + x];
                            if v != ground && v != sky {
                                n += 1;
                                minx = minx.min(x);
                                maxx = maxx.max(x);
                                miny = miny.min(y);
                                maxy = maxy.max(y);
                            }
                        }
                    }
                    if n > 0 {
                        anomaly_frames += 1;
                        anomaly_px += n;
                        rows_seen.insert((miny, maxy));
                        eprintln!("ANOM f{frame} n={n} x={minx}..{maxx} y={miny}..{maxy} hz={hz}");
                    }
                }
            }
        }

        if let Some(p) = &prev {
            let mut chg = 0usize;
            let mut rowmax = 0usize;
            let mut edge = 0usize;
            for row in 0..RH {
                let mut rowchg = 0usize;
                for col in 0..RW {
                    if p[row * RW + col] != cur[row * RW + col] {
                        rowchg += 1;
                        if col >= RW - 16 {
                            edge += 1;
                        }
                    }
                }
                chg += rowchg;
                rowmax = rowmax.max(rowchg);
            }
            // Horizontal discontinuity in the right quarter: a large jump between
            // horizontally-adjacent palette indices. Smooth dithered ground does
            // not do this; a corrupted tile fetch at the end of a scanline does.
            let mut hdisc = 0usize;
            for row in 0..RH {
                for col in (RW * 3 / 4)..RW - 1 {
                    let a = cur[row * RW + col];
                    let b = cur[row * RW + col + 1];
                    if a.abs_diff(b) >= 3 {
                        hdisc += 1;
                    }
                }
            }
            println!("{frame:5}  {chg:5}  {rowmax:5}  {edge:5}  {hdisc:5}");
        }
        prev = Some(cur);

        if frame >= dump_from && frame < dump_from.saturating_add(dump_count) {
            write_png(&out_dir.join(format!("rr_{frame:04}.png")), &fb);
        }
        frame += 1;
    }
    eprintln!(
        "done: {frame} frames; PNGs (if any) in {}",
        out_dir.display()
    );
    eprintln!(
        "ANOMALY SUMMARY: {anomaly_frames} of {frame} frames affected, {anomaly_px} stray pixels total"
    );
    eprintln!("distinct (miny,maxy) bands: {rows_seen:?}");
}
