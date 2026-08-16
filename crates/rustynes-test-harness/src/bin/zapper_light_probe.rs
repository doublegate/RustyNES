//! v2.3.6 — Zapper light-timing probe: does a light-gun game ever see light?
//!
//! Diagnostic for the maintainer report that *Duck Hunt* fires but never hits.
//! For every frame it records, from the game's own bus traffic, each `$4017`
//! read and the byte the CPU actually received — bit 3 is **light NOT detected**
//! (inverted), bit 4 is the trigger — alongside whether the aperture at the aim
//! point was bright in that frame's completed framebuffer.
//!
//! What to look for: a frame where the framebuffer IS bright at the aim point
//! but every `$4017` read that frame reports "no light", followed by a *later*
//! frame reporting light after the target box is gone. That is a one-frame-stale
//! light signal, and it makes a hit impossible in any game that draws its target
//! and polls in the same frame — which is how *Duck Hunt* works.
//!
//! Run both models and compare:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms,debug-hooks \
//!   --bin zapper_light_probe -- "tests/roms/external/mapper-000-NROM/Duck Hunt.nes"
//! cargo run -p rustynes-test-harness --features commercial-roms,debug-hooks \
//!   --bin zapper_light_probe -- "…/Duck Hunt.nes" temporal
//! ```
//!
//! The dump is gitignored (commercial), so this is a local-only diagnostic.

use std::path::Path;

use rustynes_core::{Buttons, Nes};

/// Dump a frame so the probe's numbers can be checked against what is actually
/// on screen. Reasoning about a light-gun game from luma statistics alone is how
/// this investigation went wrong twice.
fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, 256, 240);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

/// Default aim point. Duck Hunt's ducks cross the upper-middle of the play area.
/// Overridable with `ZAPPER_PROBE_AIM=x,y` so a run can be pointed at the target
/// box the previous run located.
const AIM_X_DEFAULT: u16 = 128;
const AIM_Y_DEFAULT: u16 = 96;

/// Centre of the brightest run of pixels in a frame, if any pixel is bright.
///
/// On Duck Hunt's target frame the screen is black except for a small white box
/// over each duck, so this locates the box the game is testing — which is where
/// a player who actually hit the duck would have been aiming.
fn brightest_spot(fb: &[u8]) -> Option<(u16, u16)> {
    let (mut sx, mut sy, mut n) = (0u32, 0u32, 0u32);
    for y in 0..240u32 {
        for x in 0..256u32 {
            let idx = ((y * 256 + x) * 4) as usize;
            let luma = (77 * u32::from(fb[idx])
                + 150 * u32::from(fb[idx + 1])
                + 29 * u32::from(fb[idx + 2]))
                >> 8;
            if luma >= 0x80 {
                sx += x;
                sy += y;
                n += 1;
            }
        }
    }
    (n > 0).then(|| {
        (
            u16::try_from(sx / n).unwrap_or(0),
            u16::try_from(sy / n).unwrap_or(0),
        )
    })
}

/// Mirrors `ZapperState::aperture_is_bright` closely enough to say "the target
/// was visible here this frame". Deliberately re-derived rather than exported:
/// the probe asks about the framebuffer, not about what the Zapper concluded.
/// Returns `(centre_luma, pixels_over_threshold)` using the SAME constants the
/// core uses — `ZAPPER_LUMA_THRESHOLD` (`0x80`) over a 3x3 aperture, of which
/// `ZAPPER_APERTURE_MIN_BRIGHT` (2) must pass. Reproduced rather than exported
/// so the probe reports the raw measurement, not the core's verdict.
fn aperture_stats(fb: &[u8], x: u16, y: u16) -> (u16, u32) {
    const THRESHOLD: u16 = 0x80;
    let (ax, ay) = (i32::from(x), i32::from(y));
    let mut bright = 0u32;
    let mut centre = 0u16;
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            let (px, py) = (ax + dx, ay + dy);
            if !(0..256).contains(&px) || !(0..240).contains(&py) {
                continue;
            }
            let Ok(idx) = usize::try_from((py * 256 + px) * 4) else {
                continue;
            };
            if idx + 2 >= fb.len() {
                continue;
            }
            let luma = (77 * u16::from(fb[idx])
                + 150 * u16::from(fb[idx + 1])
                + 29 * u16::from(fb[idx + 2]))
                >> 8;
            if dx == 0 && dy == 0 {
                centre = luma;
            }
            if luma >= THRESHOLD {
                bright += 1;
            }
        }
    }
    (centre, bright)
}

#[allow(clippy::too_many_lines)] // a linear diagnostic script; splitting it
// would scatter the frame loop's shared state across helpers for no gain.
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .unwrap_or_else(|| "tests/roms/external/mapper-000-NROM/Duck Hunt.nes".into());
    let temporal = args.next().is_some_and(|s| s == "temporal");

    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipping: {path} absent (commercial dump, gitignored)");
        return;
    };
    let mut nes = Nes::from_rom(&bytes).expect("rom parses");
    nes.set_zapper_temporal_light(temporal);
    // The access log is per-frame and self-clearing, so each frame's reads are
    // exactly that frame's.
    nes.set_access_logging(true);
    println!("rom: {path}");
    println!(
        "light model: {}",
        if temporal {
            "beam-relative (temporal, opt-in)"
        } else {
            "frame-granular (SHIPPED DEFAULT)"
        }
    );

    // Aim point, overridable so a run can be pointed at the target box a
    // previous run located.
    let (aim_x_runtime, aim_y_runtime) = std::env::var("ZAPPER_PROBE_AIM")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .unwrap_or((AIM_X_DEFAULT, AIM_Y_DEFAULT));
    println!("aim: ({aim_x_runtime}, {aim_y_runtime})");

    let mut discard = vec![0.0f32; 8192];

    // Boot and get into gameplay. Duck Hunt's title screen needs Start on the
    // standard pad; the warmup is dumped too, because "is the game even in
    // gameplay?" turned out to be the question that mattered.
    let warmup: u32 = std::env::var("ZAPPER_PROBE_WARMUP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    for f in 0..warmup {
        // START only during the title screen. Holding it into gameplay PAUSES
        // Duck Hunt — which looks exactly like "the game ignores the Zapper",
        // and cost this investigation two wrong conclusions before a screenshot
        // showed the word PAUSE.
        let b = if (60..66).contains(&f) {
            Buttons::START
        } else {
            Buttons::empty()
        };
        nes.set_buttons(0, b);
        nes.set_zapper(1, aim_x_runtime, aim_y_runtime, false);
        nes.run_frame();
        let _ = nes.drain_audio_into(&mut discard);
        if let Ok(dir) = std::env::var("ZAPPER_PROBE_PNG_DIR")
            && f % 20 == 0
        {
            write_png(
                &Path::new(&dir).join(format!("warm{f:04}.png")),
                nes.framebuffer(),
            );
        }
    }

    println!();
    println!("frame  trig  fb_bright  $4017 reads (value: light?)");
    let mut saw_light = false;
    // Duck Hunt blanks the screen for its light test, so a dark frame marks a
    // shot actually being processed. Print a window around each one and stay
    // quiet otherwise — the interesting event is rare and easy to miss by hand.
    let mut window_left = 0u32;
    let frames: u32 = std::env::var("ZAPPER_PROBE_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(150);
    let mut blanks = 0u32;
    let trace_enabled = std::env::var("ZAPPER_PROBE_TRACE").is_ok();
    let mut traced = false;
    for f in 0..frames {
        // Repeated short pulls, so the run catches the game's post-trigger
        // light-test sequence wherever the ducks happen to be.
        let trigger = f % 40 < 6;
        nes.set_buttons(0, Buttons::empty());
        nes.set_zapper(1, aim_x_runtime, aim_y_runtime, trigger);
        // Snapshot before the frame so a light-test frame can be REPLAYED
        // instruction-by-instruction once it has been identified — determinism
        // is what makes that legitimate.
        let pre = if trace_enabled {
            nes.snapshot()
        } else {
            Vec::new()
        };
        nes.run_frame();
        let _ = nes.drain_audio_into(&mut discard);

        let (centre, bright_px) = aperture_stats(nes.framebuffer(), aim_x_runtime, aim_y_runtime);
        let bright = bright_px >= 2;
        let reads: Vec<u8> = nes
            .accesses()
            .iter()
            .filter(|a| a.addr == 0x4017 && !a.write)
            .map(|a| a.value)
            .collect();
        // Mean frame luma separates "the game blanked the screen for the light
        // test" from ordinary gameplay, which is how the Duck Hunt sequence is
        // recognisable at all.
        let fb = nes.framebuffer();
        let mean_luma: u32 = fb.chunks_exact(4).map(|p| u32::from(p[1])).sum::<u32>()
            / u32::try_from(fb.len() / 4).unwrap_or(1);
        let _ = bright;
        if let Ok(dir) = std::env::var("ZAPPER_PROBE_PNG_DIR") {
            write_png(
                &Path::new(&dir).join(format!("f{f:03}.png")),
                nes.framebuffer(),
            );
        }
        // Bit 3 CLEAR == light detected, but only if a Zapper actually answered
        // the read: a standard controller returns its shift bit in D0 with D3
        // clear, which is indistinguishable from "light" unless the byte is
        // checked as a whole. Print the distinct raw bytes and judge from those.
        let mut distinct: Vec<u8> = reads.clone();
        distinct.sort_unstable();
        distinct.dedup();
        let lit = reads.iter().filter(|v| *v & 0x08 == 0).count();
        saw_light |= lit > 0;
        if mean_luma < 20 {
            blanks += 1;
            window_left = 6;
            // On the target frame the screen is black except the box(es) the
            // game is testing. Report where, so a follow-up run can aim there —
            // the check that the sensor fires when it should, not just that it
            // stays quiet when it should.
            if let Some((bx, by)) = brightest_spot(nes.framebuffer()) {
                println!("      target box centre at ({bx}, {by})  <-- aim here to test a hit");
            }
            if trace_enabled && !traced {
                traced = true;
                println!();
                println!("--- instruction replay of light-test frame {f} ---");
                println!("sl   dot    $4017  aim_luma  note");
                nes.restore(&pre).expect("snapshot round-trips");
                nes.set_zapper(1, aim_x_runtime, aim_y_runtime, trigger);
                // No frame counter on `Nes`, so bound the replay by the
                // scanline wrapping back to the top after reaching vblank.
                let mut last_sl = i16::MIN;
                let mut seen_vblank = false;
                for _ in 0..40_000u32 {
                    nes.step_instruction();
                    let now = nes.bus().ppu().scanline();
                    if now > 240 {
                        seen_vblank = true;
                    }
                    if seen_vblank && now < 8 {
                        break;
                    }
                    let sl = nes.bus().ppu().scanline();
                    if sl == last_sl {
                        continue; // one sample per scanline shows the shape
                    }
                    last_sl = sl;
                    let dot = nes.bus().ppu().dot();
                    let byte = nes.peek(0x4017);
                    let fb = nes.framebuffer();
                    let idx = (usize::from(aim_y_runtime) * 256 + usize::from(aim_x_runtime)) * 4;
                    let luma = (77 * u32::from(fb[idx])
                        + 150 * u32::from(fb[idx + 1])
                        + 29 * u32::from(fb[idx + 2]))
                        >> 8;
                    let lit = byte & 0x08 == 0;
                    if lit || sl % 24 == 0 {
                        println!(
                            "{sl:<4} {dot:<6} {byte:02X}     {luma:<9} {}",
                            if lit { "LIGHT" } else { "" }
                        );
                    }
                }
                println!("--- end replay ---");
                println!();
            }
        }
        if window_left == 0 {
            continue;
        }
        window_left -= 1;
        println!(
            "{f:5}  {:4}  aim_luma {centre:3} bright {bright_px}/9  \
             screen_mean {mean_luma:3}  {} read(s)  bytes {:02X?}",
            if trigger { "yes" } else { "-" },
            reads.len(),
            distinct,
        );
    }
    println!();
    println!("blank (light-test) frames observed: {blanks}");
    println!(
        "bit 3 clear on at least one read: {}  \
         (NOT proof of light on its own — bit 6 is open bus, so a Zapper byte is \
         0x40 | trigger<<4 | no_light<<3)",
        if saw_light { "yes" } else { "no" }
    );
}
