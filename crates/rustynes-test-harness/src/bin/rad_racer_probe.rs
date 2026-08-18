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

/// Left edge of the right-hand roadside strip the below-horizon detector works
/// in. The reported artifact sits there; the road is narrow and central at that
/// height, so excluding everything left of this keeps the road and the car out
/// of both the anomaly count AND the modal-colour baseline it is measured
/// against. Those two must use the same window or the baseline describes a
/// different part of the screen than the measurement.
const STRIP_X0: usize = 200;

/// Parse a numeric option value, exiting with a usage message rather than
/// panicking -- these come from a human typing a command line.
fn parse_u64(raw: &str, flag: &str) -> u64 {
    raw.parse().unwrap_or_else(|_| {
        eprintln!("rad_racer_probe: {flag} expects a non-negative integer, got {raw:?}");
        std::process::exit(2);
    })
}

/// Parse the optional flags after the two positional arguments.
///
/// Every option takes a value, and each is read through `next()` rather than an
/// `args[i + 1]` index: a flag left last on the line
/// (`rad_racer_probe rom movie --out`) is a plausible typo, and an
/// index-out-of-bounds panic is a confusing way to report it. Unknown flags and
/// missing values both exit 2 with a message.
fn parse_options(rest: &[String], out_dir: &mut PathBuf, from: &mut u64, count: &mut u64) {
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        let mut value = || -> &String {
            it.next().unwrap_or_else(|| {
                eprintln!("rad_racer_probe: {flag} needs a value");
                std::process::exit(2);
            })
        };
        match flag.as_str() {
            "--out" => *out_dir = PathBuf::from(value()),
            "--dump-from" => *from = parse_u64(value(), "--dump-from"),
            "--dump-count" => *count = parse_u64(value(), "--dump-count"),
            other => {
                eprintln!("rad_racer_probe: unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
}

/// One frame's below-horizon anomaly measurement.
struct Anomaly {
    /// Stray pixels found in the strip.
    count: usize,
    /// Bounding box of those pixels.
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    /// The scanline the sky/ground boundary was found on this frame.
    horizon: usize,
}

/// Count stray pixels in the right-hand roadside strip just below the horizon.
///
/// The reported artifact is a short band of pixels immediately BELOW the
/// sky/ground boundary, on the right. Find the horizon per frame (the last row
/// still containing sky), take the modal ground colour beneath it, and count
/// what is neither that nor sky.
///
/// Both the modal-colour baseline and the count are taken from the SAME
/// [`STRIP_X0`]-bounded window, which is load-bearing. Across the full scanline
/// the road can win the mode, and then every sand pixel reads as an anomaly --
/// the 5.8M-stray-pixel result the first version of this detector produced.
/// Restricting the count without restricting the baseline leaves half of that
/// in place.
fn detect_below_horizon_anomaly(idx: &[u16]) -> Option<Anomaly> {
    let sky = idx[8 * 256 + 8]; // top-left is sky in this scene
    let hz = (100..180)
        .rev()
        .find(|&y| (0..256).any(|x| idx[y * 256 + x] == sky))?;
    let lo = hz + 3;
    let hi = (hz + 40).min(200);

    let mut hist = std::collections::HashMap::<u16, usize>::new();
    for y in lo..hi {
        for x in STRIP_X0..256usize {
            *hist.entry(idx[y * 256 + x]).or_default() += 1;
        }
    }
    let (&ground, _) = hist.iter().max_by_key(|&(_, n)| *n)?;

    // A TIGHT window: the artifact is a short band in the first few rows under
    // the horizon, well right of the road (which is narrow and central at that
    // height). Bounding it to exactly that is what lets a real regression be
    // the only thing that trips this.
    let mut count = 0usize;
    let (mut x0, mut x1, mut y0, mut y1) = (255usize, 0usize, 255usize, 0usize);
    for y in (hz + 1)..(hz + 6).min(hi) {
        for x in STRIP_X0..256usize {
            let v = idx[y * 256 + x];
            if v != ground && v != sky {
                count += 1;
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    (count > 0).then_some(Anomaly {
        count,
        x0,
        x1,
        y0,
        y1,
        horizon: hz,
    })
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
    // `temp_dir()` rather than a literal `/tmp`, so the default works on
    // Windows too (review nitpick, and free to honour).
    let mut out_dir = std::env::temp_dir().join("rad_racer_probe");
    let mut dump_from = u64::MAX;
    let mut dump_count = 0u64;
    parse_options(&args[3..], &mut out_dir, &mut dump_from, &mut dump_count);
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
        if let Some(a) = detect_below_horizon_anomaly(idx) {
            anomaly_frames += 1;
            anomaly_px += a.count;
            rows_seen.insert((a.y0, a.y1));
            eprintln!(
                "ANOM f{frame} n={} x={}..{} y={}..{} hz={}",
                a.count, a.x0, a.x1, a.y0, a.y1, a.horizon
            );
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
