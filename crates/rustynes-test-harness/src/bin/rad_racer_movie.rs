//! Scripted `.rnm` recorder for the Rad Racer roadside investigation (v3.6.2).
//!
//! NOT part of CI. Diagnostic tool only, like `rad_racer_probe` and
//! `rad_racer_prov`, and for the same reason: the ROM is a gitignored commercial
//! dump under `tests/roms/external/`.
//!
//! Those two probes replay a recorded movie, and none had been kept. This writes
//! one from a schedule instead of from a person at a keyboard, so the run that
//! shows the artifact is reproducible from its command line alone:
//!
//! ```text
//! rad_racer_movie <rom> <out.rnm> <frames> <schedule> [--png DIR F1,F2,...]
//! ```
//!
//! `<schedule>` is a comma-separated list of `FROM-TO:BUTTONS` entries. `TO`
//! may be omitted for "to the end" (`5-:A` holds A from frame 5 on), and a
//! bare frame is that frame only (`5:A`). BUTTONS is a `+`-joined subset of
//! `A B SELECT START UP DOWN LEFT RIGHT`. Overlapping entries OR together. The
//! movie starts from power-on ([`power_on_for_movie`]), so `rad_racer_probe`
//! replays it exactly.

use std::path::Path;

use rustynes_core::{Buttons, FrameInput, MovieRecorder, Nes, power_on_for_movie};

struct Entry {
    from: u64,
    to: u64,
    buttons: Buttons,
}

fn parse_buttons(s: &str) -> Buttons {
    let mut b = Buttons::empty();
    for name in s.split('+') {
        b |= match name {
            "A" => Buttons::A,
            "B" => Buttons::B,
            "SELECT" => Buttons::SELECT,
            "START" => Buttons::START,
            "UP" => Buttons::UP,
            "DOWN" => Buttons::DOWN,
            "LEFT" => Buttons::LEFT,
            "RIGHT" => Buttons::RIGHT,
            other => usage_exit(&format!("unknown button {other:?}")),
        };
    }
    b
}

fn parse_schedule(s: &str) -> Vec<Entry> {
    s.split(',')
        .filter(|e| !e.is_empty())
        .map(|e| {
            let Some((range, buttons)) = e.split_once(':') else {
                usage_exit(&format!("schedule entry {e:?} needs FROM-TO:BUTTONS"));
            };
            if range.is_empty() {
                usage_exit(&format!("schedule entry {e:?} has no frame range"));
            }
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            let num = |v: &str, dflt: u64| {
                if v.is_empty() {
                    dflt
                } else {
                    v.parse()
                        .unwrap_or_else(|_| usage_exit(&format!("bad frame number {v:?}")))
                }
            };
            let (from, to) = (num(from, 0), num(to, u64::MAX));
            // An inverted range would silently never fire (Antigravity on #640).
            if from > to {
                usage_exit(&format!("schedule entry {e:?} ends before it starts"));
            }
            Entry {
                from,
                to,
                buttons: parse_buttons(buttons),
            }
        })
        .collect()
}

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 256, 240);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

const USAGE: &str =
    "usage: rad_racer_movie <rom> <out.rnm> <frames> <schedule> [--png DIR F1,F2,...]";

/// Every argument mistake is a usage error with status 2, checked before the
/// ROM is run, as `rad_racer_prov` does (`CodeRabbit` and Antigravity on #640:
/// the frame count used to panic, a dangling `--png` was silently ignored and
/// a malformed PNG frame number silently dropped).
fn usage_exit(why: &str) -> ! {
    eprintln!("rad_racer_movie: {why}\n{USAGE}");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let frames: u64 = args[3]
        .parse()
        .unwrap_or_else(|_| usage_exit(&format!("<frames> expects an integer, got {:?}", args[3])));
    let schedule = parse_schedule(&args[4]);
    let (png_dir, png_frames): (Option<&str>, Vec<u64>) =
        match (args.get(5).map(String::as_str), args.len()) {
            (None, _) => (None, Vec::new()),
            (Some("--png"), 8) => {
                let list: Vec<u64> = args.get(7).map_or_else(Vec::new, |l| {
                    l.split(',')
                        .filter(|f| !f.is_empty())
                        .map(|f| {
                            f.parse().unwrap_or_else(|_| {
                                usage_exit(&format!("bad --png frame number {f:?}"))
                            })
                        })
                        .collect()
                });
                // A frame past the end is never reached; say so now rather
                // than after the run (Antigravity on #640).
                if let Some(&late) = list.iter().find(|&&f| f >= frames) {
                    usage_exit(&format!(
                        "--png frame {late} is past the movie's {frames} frames"
                    ));
                }
                (Some(args[6].as_str()), list)
            }
            (Some("--png"), _) => usage_exit("--png expects DIR and a frame list F1,F2,..."),
            (Some(other), _) => usage_exit(&format!("unknown option {other:?}")),
        };
    if let Some(dir) = png_dir {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("create png dir {dir}: {e}"));
    }
    let rom = std::fs::read(&args[1]).unwrap_or_else(|e| panic!("read rom: {e}"));

    let mut nes = Nes::from_rom(&rom).unwrap_or_else(|e| panic!("load rom: {e}"));
    power_on_for_movie(&mut nes);
    let mut rec = MovieRecorder::power_on(&nes);
    for f in 0..frames {
        let mut b = Buttons::empty();
        for e in &schedule {
            if (e.from..=e.to).contains(&f) {
                b |= e.buttons;
            }
        }
        let input =
            FrameInput::four_players(b, Buttons::empty(), Buttons::empty(), Buttons::empty());
        input.apply_to(&mut nes);
        rec.capture_input(input);
        let fb = nes.run_frame();
        if let Some(dir) = png_dir
            && png_frames.contains(&f)
        {
            write_png(&Path::new(dir).join(format!("f{f:05}.png")), fb);
        }
    }
    let movie = rec.finish();
    std::fs::write(&args[2], movie.serialize()).expect("write movie");
    eprintln!("wrote {} frames to {}", movie.len(), args[2]);
}
