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
//! `<schedule>` is a comma-separated list of `FROM-TO:BUTTONS` entries (`TO`
//! may be omitted for "to the end"), BUTTONS a `+`-joined subset of
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
            other => {
                eprintln!("unknown button {other:?}");
                std::process::exit(2);
            }
        };
    }
    b
}

fn parse_schedule(s: &str) -> Vec<Entry> {
    s.split(',')
        .filter(|e| !e.is_empty())
        .map(|e| {
            let Some((range, buttons)) = e.split_once(':') else {
                eprintln!("schedule entry {e:?} needs FROM-TO:BUTTONS");
                std::process::exit(2);
            };
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            let num = |v: &str, dflt: u64| {
                if v.is_empty() {
                    dflt
                } else {
                    v.parse().unwrap_or_else(|_| {
                        eprintln!("bad frame number {v:?}");
                        std::process::exit(2);
                    })
                }
            };
            Entry {
                from: num(from, 0),
                to: num(to, u64::MAX),
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: rad_racer_movie <rom> <out.rnm> <frames> <schedule> [--png DIR F1,F2,...]"
        );
        std::process::exit(2);
    }
    let rom = std::fs::read(&args[1]).unwrap_or_else(|e| panic!("read rom: {e}"));
    let frames: u64 = args[3].parse().expect("frame count");
    let schedule = parse_schedule(&args[4]);
    let (png_dir, png_frames): (Option<&str>, Vec<u64>) = if args.len() >= 7 && args[5] == "--png" {
        std::fs::create_dir_all(&args[6]).expect("create png dir");
        let list = args
            .get(7)
            .map(|l| l.split(',').filter_map(|f| f.parse().ok()).collect())
            .unwrap_or_default();
        (Some(args[6].as_str()), list)
    } else {
        (None, Vec::new())
    };

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
        let fb = nes.run_frame().to_vec();
        if let Some(dir) = png_dir
            && png_frames.contains(&f)
        {
            write_png(&Path::new(dir).join(format!("f{f:05}.png")), &fb);
        }
    }
    let movie = rec.finish();
    std::fs::write(&args[2], movie.serialize()).expect("write movie");
    eprintln!("wrote {} frames to {}", movie.len(), args[2]);
}
