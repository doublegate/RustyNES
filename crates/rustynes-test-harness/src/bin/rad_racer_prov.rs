//! Rad Racer artifact — causal chain for the stray pixels below the horizon.
//!
//! NOT part of CI. Diagnostic tool only; needs a gitignored external dump.
//!
//! Replays the recorded `.rnm` to a chosen frame with **Pixel Provenance armed**,
//! then prints, for every anomalous pixel in the band and for a matched control
//! pixel on the same scanline, the full record the v2.3.2 feature captures: the
//! layer that won, the dot and scanline that emitted it, the nametable /
//! attribute / pattern addresses of the tile actually on screen, and the palette
//! entry.
//!
//! The question this settles in one shot: are the stray pixels **background**
//! (a tile fetch went wrong at the end of a scanline) or **sprite** (roadside
//! object drawn wrong)? Every other hypothesis follows from that answer, and
//! guessing between them from colours alone is how an investigation goes wide.

use rustynes_core::{Movie, MoviePlayer, Nes};

/// The layer, rendered from its `Debug` form.
///
/// `rustynes-core` does not re-export `PixelLayer`, and adding a direct
/// `rustynes-ppu` edge to the harness for a diagnostic binary would violate the
/// workspace's one-directional dependency rule for the sake of a label.
fn layer_name(l: impl core::fmt::Debug) -> String {
    format!("{l:?}")
}

/// Locate the sky colour, the horizon scanline, and the modal ground colour of
/// the right-hand sand strip -- the same band the sibling `rad_racer_probe`
/// measures, computed the same way so the two tools describe one region.
///
/// Exits 0 with a message when there is no sky/ground boundary at all. That is a
/// legitimate answer for a menu, a fade, or a frame past the end of the movie --
/// not a crash.
fn locate_band(idx: &[u16], target: u64) -> (u16, usize, u16) {
    let sky = idx[8 * 256 + 8];
    let Some(hz) = (100..180).rfind(|&y| (0..256).any(|x| idx[y * 256 + x] == sky)) else {
        eprintln!(
            "rad_racer_prov: no sky/ground boundary on frame {target} \
             (sky index {sky:#06X} absent from rows 100..180) -- nothing to inspect"
        );
        std::process::exit(0);
    };
    // Sample only the RIGHT-HAND sand. Sampling the full width lets the road
    // (palette index 0, and wide just below the horizon) win the mode, which
    // makes every sand pixel read as an "anomaly" -- a false positive that
    // reported 280 stray pixels on a frame whose real count is around a dozen.
    let mut hist = std::collections::HashMap::<u16, usize>::new();
    for y in (hz + 3)..(hz + 30).min(200) {
        for x in 200..256usize {
            *hist.entry(idx[y * 256 + x]).or_default() += 1;
        }
    }
    let ground = *hist
        .iter()
        .max_by_key(|&(_, n)| *n)
        .expect("the strip is non-empty, so a modal colour always exists")
        .0;
    (sky, hz, ground)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: rad_racer_prov <rom> <movie.rnm> <target-frame>");
        std::process::exit(2);
    }
    // A typo in the frame number is the one wrong thing a user of this tool is
    // actually likely to type, so report it rather than panicking at them.
    let target: u64 = args[3].parse().unwrap_or_else(|_| {
        eprintln!(
            "rad_racer_prov: <target-frame> expects a non-negative integer, got {:?}",
            args[3]
        );
        std::process::exit(2);
    });

    let rom = std::fs::read(&args[1]).unwrap_or_else(|e| panic!("read rom: {e}"));
    let mut nes = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("load rom: {e}"));
    let blob = std::fs::read(&args[2]).unwrap_or_else(|e| panic!("read movie: {e}"));
    let movie = Movie::deserialize(&blob).unwrap_or_else(|e| panic!("parse movie: {e}"));
    movie
        .seek_to_start(&mut nes)
        .unwrap_or_else(|e| panic!("seek movie start: {e}"));

    let mut player = MoviePlayer::new(&movie);
    let mut frame = 0u64;
    while player.apply_next(&mut nes) {
        // Arm one frame early so the TARGET frame is the one recorded.
        if frame == target {
            nes.set_pixel_provenance(true);
        }
        nes.run_frame();
        if frame == target {
            break;
        }
        frame += 1;
    }

    let idx = nes.index_framebuffer().to_vec();
    let (sky, hz, ground) = locate_band(&idx, target);
    eprintln!("frame {target}: horizon y={hz}, ground index {ground:#06X}, sky {sky:#06X}");

    let Some(prov) = nes.pixel_provenance() else {
        panic!("provenance not armed — the record is empty");
    };

    println!(
        "{:>4} {:>4}  {:>8} {:>6} {:>4}  {:>6} {:>6} {:>6}  {:>5} {:>5}  note",
        "x", "y", "layer", "scanln", "dot", "nt", "at", "pattern", "pal", "idx"
    );
    let mut shown = 0;
    // Two DIFFERENT ways a query has no answer, and conflating them is what hid
    // the provenance defect for four releases:
    //
    //   * `get` returns `None` only for an OFF-SCREEN coordinate.
    //   * an on-screen pixel that was never recorded (or was cleared) returns
    //     `Some(PixelProvenance::default())`, every field of which reads as a
    //     confident "scanline 0, dot 0, backdrop, palette $0000" — indis-
    //     tinguishable from a real backdrop pixel unless `is_recorded()` is
    //     consulted, which `get` deliberately does not do.
    //
    // So filter on `is_recorded` here. An earlier version of this comment
    // claimed `get` itself carried the validity marker; it does not, and the
    // tool printed cleared records as fact. Caught in review.
    let row = |x: usize, y: usize, note: &str| {
        let cell = prov
            .get(x, y)
            .filter(rustynes_core::rustynes_ppu::PixelProvenance::is_recorded);
        match cell {
            Some(p) => println!(
                "{x:>4} {y:>4}  {:>9} {:>6} {:>4}  {:#06X} {:#06X} {:#06X}  {:>5} {:>5}  {note}",
                layer_name(p.layer),
                p.scanline,
                p.dot,
                p.nt_addr,
                p.at_addr,
                p.pattern_addr,
                p.palette_addr,
                idx[y * 256 + x]
            ),
            None => println!(
                "{x:>4} {y:>4}  {:>9} {:>6} {:>4}  {:>6} {:>6} {:>6}  {:>5} {:>5}  {note}",
                "NO-RECORD",
                "-",
                "-",
                "-",
                "-",
                "-",
                "-",
                idx[y * 256 + x]
            ),
        }
    };
    for y in (hz + 1)..(hz + 6).min(200) {
        row(210, y, "control");
        for x in 200..256usize {
            if idx[y * 256 + x] != ground && idx[y * 256 + x] != sky {
                row(x, y, "ANOMALY");
                shown += 1;
                if shown > 40 {
                    println!("... (truncated)");
                    return;
                }
            }
        }
    }
}
