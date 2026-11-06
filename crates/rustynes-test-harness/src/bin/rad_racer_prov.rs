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

/// The output mode, parsed and validated BEFORE the movie is replayed, so a
/// malformed `--events` / `--rect` is a usage error at once rather than a
/// silent fall-back to the band after a long replay (Antigravity on #640).
enum Mode {
    /// The automatic band below the horizon (`locate_band`).
    Band,
    /// v3.6.2: `--events SL0 SL1` prints the target frame's register events
    /// (scanline, dot, address, value) on scanlines `SL0..=SL1`: which write
    /// landed where, which is the question once provenance has named the tile.
    Events(i16, i16),
    /// v3.6.2: the `--rect X0 X1 Y0 Y1` rectangle: every pixel of it is
    /// printed instead of the automatic band. The band trusts `locate_band`'s
    /// horizon, the lowest row holding the sky colour, so a stray dash OF sky
    /// colour down in the sand (the v3.6.2 report) becomes the "horizon" and
    /// is never itself inspected. Selected before `locate_band` runs, so a
    /// frame with no sky/ground boundary can still be inspected (Copilot on
    /// #640).
    Rect(usize, usize, usize, usize),
}

const USAGE: &str = "usage: rad_racer_prov <rom> <movie.rnm> <target-frame> [--events SL0 SL1 | --rect X0 X1 Y0 Y1]";

fn usage_exit(why: &str) -> ! {
    eprintln!("rad_racer_prov: {why}\n{USAGE}");
    std::process::exit(2);
}

/// `--events`: the target frame's register events on scanlines `s0..=s1`.
fn print_events(nes: &Nes, s0: i16, s1: i16) {
    for e in nes.events() {
        if (s0..=s1).contains(&e.scanline) {
            println!(
                "sl {:>3} dot {:>3}  {:?} {:#06X} = {:#04X}",
                e.scanline, e.dot, e.kind, e.addr, e.value
            );
        }
    }
}

fn parse_mode(args: &[String]) -> Mode {
    fn num<T: core::str::FromStr>(s: &str, what: &str) -> T {
        s.parse()
            .unwrap_or_else(|_| usage_exit(&format!("{what}, got {s:?}")))
    }
    match args.get(4).map(String::as_str) {
        None => Mode::Band,
        Some("--events") if args.len() == 7 => {
            let s0 = num(&args[5], "--events expects two scanlines");
            let s1 = num(&args[6], "--events expects two scanlines");
            if s0 > s1 {
                usage_exit(&format!(
                    "--events SL0 SL1 needs SL0 <= SL1, got {s0} > {s1}"
                ));
            }
            Mode::Events(s0, s1)
        }
        Some("--events") => usage_exit("--events expects exactly two scanlines"),
        Some("--rect") if args.len() == 9 => {
            let n = |s: &str| num::<usize>(s, "--rect expects four integers");
            let (x0, x1, y0, y1) = (n(&args[5]), n(&args[6]), n(&args[7]), n(&args[8]));
            // The order is X0 X1 Y0 Y1, not X0 Y0 X1 Y1: an inverted pair is
            // almost always that transposition, and would print nothing.
            if x0 > x1 || y0 > y1 || x0 > 255 || y0 > 239 {
                usage_exit(&format!(
                    "--rect X0 X1 Y0 Y1 needs X0 <= X1 <= 255 and Y0 <= Y1 <= 239 (order X0 X1 Y0 Y1), got {x0} {x1} {y0} {y1}"
                ));
            }
            Mode::Rect(x0, x1, y0, y1)
        }
        Some("--rect") => usage_exit("--rect expects exactly four integers"),
        Some(other) => usage_exit(&format!("unknown option {other:?}")),
    }
}

/// Replay the movie to `target` with provenance and event logging armed on
/// that frame. Exits 2 when the movie ends first.
fn replay_to(rom_path: &str, movie_path: &str, target: u64) -> Nes {
    let rom = std::fs::read(rom_path).unwrap_or_else(|e| panic!("read rom: {e}"));
    let mut nes = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("load rom: {e}"));
    let blob = std::fs::read(movie_path).unwrap_or_else(|e| panic!("read movie: {e}"));
    let movie = Movie::deserialize(&blob).unwrap_or_else(|e| panic!("parse movie: {e}"));
    movie
        .seek_to_start(&mut nes)
        .unwrap_or_else(|e| panic!("seek movie start: {e}"));

    let mut player = MoviePlayer::new(&movie);
    let mut frame = 0u64;
    let mut reached = false;
    while player.apply_next(&mut nes) {
        // Arm provenance ON the target frame, immediately before the `run_frame`
        // that produces it -- the recording covers that frame's own rendering.
        // (An earlier version of this comment claimed it armed "one frame early";
        // it never did, and the code below is the correct behaviour.)
        if frame == target {
            nes.set_pixel_provenance(true);
            nes.set_event_logging(true);
        }
        nes.run_frame();
        if frame == target {
            reached = true;
            break;
        }
        frame += 1;
    }

    // A target past the end of the movie is a plausible thing to type, and it
    // used to arrive as an unconditional panic several steps later: the arm never
    // fired, so `pixel_provenance()` returned `None` and the tool blamed itself
    // for a "record [that] is empty". Report the actual mistake, here, where the
    // number that caused it is still in hand.
    if !reached {
        eprintln!("rad_racer_prov: the movie ended at frame {frame}; target {target} is past it");
        std::process::exit(2);
    }
    nes
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("{USAGE}");
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
    let mode = parse_mode(&args);

    let nes = replay_to(&args[1], &args[2], target);

    if let Mode::Events(s0, s1) = mode {
        print_events(&nes, s0, s1);
        return;
    }

    let idx = nes.index_framebuffer().to_vec();

    let Some(prov) = nes.pixel_provenance() else {
        // Unreachable given the arm above, so this really is an invariant
        // violation rather than a user error -- the reachable version of it is
        // handled at the loop exit.
        panic!("provenance armed on frame {target} but the core reports no record");
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
    if let Mode::Rect(x0, x1, y0, y1) = mode {
        for y in y0..=y1.min(239) {
            (x0..=x1.min(255)).for_each(|x| row(x, y, "rect"));
        }
        return;
    }
    let (sky, hz, ground) = locate_band(&idx, target);
    eprintln!("frame {target}: horizon y={hz}, ground index {ground:#06X}, sky {sky:#06X}");
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
