//! v3.6.2 (`T-RADRACER-COHERENT-COPY`) — *Rad Racer*'s right-side wrong tiles,
//! with and without the per-game coherent mid-line copy correction.
//!
//! Needs your own dump at `tests/roms/external/mapper-001-MMC1/Rad Racer.nes`
//! (gitignored, never committed); without it the test says so and passes,
//! like the other `commercial-roms` suites.
//!
//! The game writes `$2006` mid-scanline from sprite-0-timed loops. When the
//! delayed `t -> v` copy lands inside an 8-dot fetch group, the hardware
//! fetches that tile from two addresses; on line 139 (the road's mirroring
//! split) it draws an 8-pixel dash of backdrop in the sand right of the road
//! on about one frame in eleven. The correction, on by default for this dump,
//! holds such a copy to the next group boundary. This test drives the same
//! scripted race as `rad_racer_movie` (START four times, then the accelerator
//! from frame 600) and counts frames whose line 139 has one whole wrong tile
//! (8 pixels on an 8-pixel boundary) in the sand right of the road:
//!
//! - correction ON (the default for this dump): none;
//! - correction OFF (the hardware's picture): some, so the counter can see a
//!   dash and the zero above is not vacuous.

#![cfg(feature = "commercial-roms")]

use rustynes_core::{Buttons, FrameInput, Nes, power_on_for_movie};

const ROM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/roms/external/mapper-001-MMC1/Rad Racer.nes"
);

/// Frames 700..1100 of the race: the car is driving and the road splits on
/// line 139 every frame.
const FIRST: u64 = 700;
const LAST: u64 = 1100;

const fn buttons_at(f: u64) -> Buttons {
    match f {
        120..=125 | 240..=245 | 360..=365 | 480..=485 => Buttons::START,
        600.. => Buttons::A,
        _ => Buttons::empty(),
    }
}

/// Whether line 139 of `idx` (a 256 x 240 palette-index framebuffer) has the
/// dash: one whole wrong TILE in the sand right of the road, a non-sand run
/// exactly 8 pixels wide starting on an 8-pixel boundary, with sand on both
/// sides. The shape matters: roadside post sprites cross line 139 too (2-3
/// pixels of black and white, moving a few pixels a frame), and a looser
/// "any isolated run" counter flagged ten such frames in this race. The sand's
/// index is the mode of a strip right of the road below the horizon, which is
/// sand on every driving frame (a `BTreeMap`, so a tie breaks the same way on
/// every run).
fn line_139_has_a_dash(idx: &[u16]) -> bool {
    let mut counts = std::collections::BTreeMap::<u16, usize>::new();
    for y in 125..150 {
        for x in 236..256 {
            *counts.entry(idx[y * 256 + x]).or_default() += 1;
        }
    }
    let sand = counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .map(|(c, _)| c)
        .unwrap();
    let row = &idx[139 * 256..140 * 256];
    // From the right edge: skip sand, take the first non-sand run, and call it
    // a dash if sand lies on its left as well (the road edge has no sand left
    // of it before the road).
    let mut x = 255usize;
    while x > 0 && row[x] == sand {
        x -= 1;
    }
    let end = x;
    while x > 0 && row[x] != sand {
        x -= 1;
    }
    let width = end - x;
    let start = x + 1;
    width == 8 && start.is_multiple_of(8) && x >= 4 && row[x - 3..=x].iter().all(|&p| p == sand)
}

fn count_dash_frames(coherent: Option<bool>) -> u64 {
    let rom = std::fs::read(ROM).expect("read the Rad Racer dump");
    let mut nes = Nes::from_rom(&rom).expect("load Rad Racer");
    power_on_for_movie(&mut nes);
    if let Some(on) = coherent {
        nes.set_coherent_midline_copy(on);
    }
    let mut dashes = 0;
    for f in 0..=LAST {
        FrameInput::four_players(
            buttons_at(f),
            Buttons::empty(),
            Buttons::empty(),
            Buttons::empty(),
        )
        .apply_to(&mut nes);
        nes.run_frame();
        if f >= FIRST && line_139_has_a_dash(nes.index_framebuffer()) {
            dashes += 1;
        }
    }
    dashes
}

#[test]
fn rad_racer_draws_no_dash_with_the_correction_and_does_without_it() {
    if !std::path::Path::new(ROM).exists() {
        eprintln!("rad_racer_coherent_copy: no dump at {ROM}; nothing to check");
        return;
    }
    let rom = std::fs::read(ROM).unwrap();
    let nes = Nes::from_rom(&rom).unwrap();
    assert!(
        nes.coherent_midline_copy(),
        "the correction is on by default for this dump"
    );
    let with = count_dash_frames(None);
    let without = count_dash_frames(Some(false));
    eprintln!("line-139 dash frames, {FIRST}..={LAST}: correction on {with}, off {without}");
    assert_eq!(with, 0, "the correction leaves no dash on line 139");
    assert!(
        without > 0,
        "without the correction the hardware's dash appears, so the counter can see one"
    );
}
