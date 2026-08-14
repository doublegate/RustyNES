//! Shared boot-coverage primitives: the ROM walk + the distinct-colour
//! "is this a real screen?" health heuristic.
//!
//! Factored out of the `coverage_smoke` / `render_smoke` diagnostic bins so
//! the auto-discovering `external_coverage` integration test can apply the
//! SAME blank-frame detector instead of copy-pasting it.
//!
//! Gated on the `commercial-roms` feature: the only consumers are the
//! commercial-ROM boot survey tooling (the bins + the
//! `tests/external_coverage.rs` harness), all of which require that
//! feature already. Keeping it behind the gate means the default /
//! `no_std`-adjacent builds don't compile it.
//!
//! ## The blank/few-colour health check (promoted from `coverage_smoke`)
//!
//! A crashed, hung, or never-rendered boot collapses the 256x240
//! framebuffer to the backdrop colour (often a single solid colour, at
//! most the handful of palette entries the reset code wrote). A real
//! title / menu / gameplay screen draws dozens of distinct colours and
//! no single colour fills nearly the whole frame. `frame_health`
//! captures both signals; `FrameHealth::looks_blank` folds them into
//! one verdict with the thresholds the two bins converged on:
//! `distinct_colors <= 4` OR `dominant_fraction >= 0.99`.

#![cfg(feature = "commercial-roms")]

use std::path::{Path, PathBuf};

use rustynes_core::{Buttons, Nes};

/// At most this many distinct colours means treat the frame as blank.
///
/// A real NES screen draws well more than four distinct RGBA values; a
/// backdrop-only or crashed boot shows a UNIFORM frame.
///
/// **Was 4, measured wrong, lowered to 1 in v2.3.4.** The premise behind 4 was
/// that "a real title / menu / gameplay screen draws dozens of distinct
/// colours". Checked against the 699-ROM corpus, that is simply false --
/// plenty of correct screens are near-monochrome:
///
/// | ROM | reading | what it actually draws |
/// | --- | --- | --- |
/// | Adventures of Lolo | 4 colours, 73.1% | its mission-briefing text box |
/// | Image Fight | 4 colours, 99.3% | its "1P START" menu |
/// | Bandit Kings of Ancient China | 3 colours, 91.0% | its decorative border |
/// | Racer Mini Yonku | 3 colours, 89.7% | its player-count dialogue |
/// | Paperboy | 2 colours, 99.7% | its "MONDAY" day card |
///
/// Every one of those was reported as a failed boot. At 1, the test is the
/// thing the check is actually for: a frame of a single colour is, by
/// definition, a frame in which nothing was drawn.
pub const BLANK_MAX_DISTINCT_COLORS: usize = 1;

/// Dominant-colour threshold for the blank verdict.
///
/// If a single colour fills at least this fraction of the frame, treat it as
/// backdrop-only even when a few stray colours bump the distinct count above
/// [`BLANK_MAX_DISTINCT_COLORS`].
///
/// **Was 0.99, raised to 0.999 in v2.3.4** for the same reason the colour
/// count moved: Paperboy's "MONDAY" card is 99.7% black and is correct output,
/// so 0.99 failed it. The new value has a physical anchor rather than being a
/// round number -- a 256x240 frame is 61,440 pixels, so 0.999 leaves room for
/// 61 non-dominant pixels, just under the 64 in a single 8x8 tile. "Fewer
/// non-backdrop pixels than one tile" is a defensible definition of nothing
/// having been drawn; "fewer than 1%" (614 pixels, nearly ten tiles) is not.
pub const BLANK_MIN_DOMINANT_FRACTION: f64 = 0.999;

/// Objective per-frame render statistics over a 256x240 RGBA8 framebuffer.
///
/// How many distinct colours it contains and what fraction of pixels the
/// single most-common colour occupies.
#[derive(Clone, Copy, Debug)]
pub struct FrameHealth {
    /// Count of distinct RGBA colours in the frame.
    pub distinct_colors: usize,
    /// Fraction `[0.0, 1.0]` of pixels that are the single dominant
    /// colour. `1.0` for a solid-colour frame.
    pub dominant_fraction: f64,
}

impl FrameHealth {
    /// Blank / failed-to-render verdict: too few colours OR one colour
    /// almost totally dominant. See the module-level threshold rationale.
    #[must_use]
    pub fn looks_blank(&self) -> bool {
        self.distinct_colors <= BLANK_MAX_DISTINCT_COLORS
            || self.dominant_fraction >= BLANK_MIN_DOMINANT_FRACTION
    }
}

/// Compute the distinct-colour count + dominant-colour fraction of an
/// RGBA8 framebuffer.
///
/// The shared core of every boot-coverage blank detector.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn frame_health(fb: &[u8]) -> FrameHealth {
    // Pack each RGBA8 pixel into a u32, sort, and do one linear pass counting
    // runs — far cheaper than a SipHash `HashMap` over ~61k pixels/frame (no
    // per-pixel hashing, a single allocation). The distinct-colour count and
    // dominant-colour fraction are identical to the map-based tally.
    let mut pixels: Vec<u32> = fb
        .chunks_exact(4)
        .map(|px| u32::from_le_bytes([px[0], px[1], px[2], px[3]]))
        .collect();
    let total = pixels.len();
    if total == 0 {
        return FrameHealth {
            distinct_colors: 0,
            dominant_fraction: 0.0,
        };
    }
    pixels.sort_unstable();
    let mut distinct = 1usize;
    let mut run = 1usize;
    let mut max_run = 1usize;
    for i in 1..total {
        if pixels[i] == pixels[i - 1] {
            run += 1;
        } else {
            distinct += 1;
            run = 1;
        }
        max_run = max_run.max(run);
    }
    FrameHealth {
        distinct_colors: distinct,
        dominant_fraction: max_run as f64 / total as f64,
    }
}

/// Recursively collect every loadable ROM file under `root` (a `.nes` iNES /
/// NES 2.0 image or a `.unf` / `.unif` UNIF container), appending to `out`.
///
/// Order is filesystem-dependent; callers that need determinism sort the
/// result. Missing / unreadable directories are silently skipped (a
/// fresh checkout's gitignored-absent `external/` tree returns nothing
/// rather than erroring). This is the walk the `coverage_smoke` bin and the
/// data-driven `external_coverage` harness use to sweep an external directory.
/// UNIF (`.unf`) is included as of v1.6.0 (E2) so the UNIF-only dumps staged
/// under `external/` are boot-captured alongside the iNES ones.
pub fn walk_nes(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_nes(&p, out);
        } else if p.extension().is_some_and(|x| {
            x.eq_ignore_ascii_case("nes")
                || x.eq_ignore_ascii_case("unf")
                || x.eq_ignore_ascii_case("unif")
        }) {
            out.push(p);
        }
    }
}

/// Run `bytes` as a ROM headless for `frames` frames and return its 256x240 RGBA8
/// framebuffer.
///
/// Deterministic: the same ROM + frame count yields the same frame (the seeded
/// power-on alignment is fixed), so the framebuffer hash is a stable regression key.
///
/// Vs. System carts get their per-game RGB-PPU type + DIP from `vs_db` so colours
/// render correctly; a START press is pulsed in a 10-frame window from `start_at`,
/// repeating every 600 frames, to advance title / intro screens. Returns the load
/// error string if the ROM can't be parsed.
///
/// # Errors
/// Returns `Err(message)` when [`Nes::from_rom`] rejects `bytes`.
pub fn run_rom_headless(bytes: &[u8], frames: u64, start_at: u64) -> Result<Vec<u8>, String> {
    let mut nes = Nes::from_rom(bytes).map_err(|e| e.to_string())?;
    if nes.is_vs_system() {
        let dip = rustynes_core::vs_db::lookup(nes.rom_sha256()).map_or(0, |entry| {
            nes.set_vs_ppu_type(entry.vs_ppu_type);
            entry.vs_dip
        });
        nes.set_vs_dip(dip);
    }
    for f in 0..frames {
        let btn = if (start_at..start_at.saturating_add(10)).contains(&(f % 600)) {
            Buttons::START
        } else {
            Buttons::empty()
        };
        nes.set_buttons(0, btn);
        nes.run_frame();
    }
    Ok(nes.framebuffer().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_frame_is_blank() {
        let fb = vec![0u8; 256 * 240 * 4]; // all-black, 1 colour
        let h = frame_health(&fb);
        assert_eq!(h.distinct_colors, 1);
        assert!((h.dominant_fraction - 1.0).abs() < f64::EPSILON);
        assert!(h.looks_blank());
    }

    /// v2.3.4 — a few colours spread over the frame is CONTENT, not a blank.
    ///
    /// This test previously asserted the opposite, encoding the premise that
    /// "a real screen draws dozens of colours". Measured against the 699-ROM
    /// corpus that premise is false: Adventures of Lolo's briefing text (4
    /// colours), Bandit Kings' border (3), and Paperboy's "MONDAY" card (2) are
    /// all correct output that the old rule failed. A crashed boot does not
    /// produce an even four-way dither across the screen; it produces a
    /// UNIFORM frame, which `solid_frame_is_blank` covers.
    #[test]
    fn few_colours_evenly_spread_is_not_blank() {
        let mut fb = Vec::with_capacity(256 * 240 * 4);
        for i in 0u32..(256 * 240) {
            let v = u8::try_from(i % 4).unwrap() * 64;
            fb.extend_from_slice(&[v, v, v, 255]);
        }
        let h = frame_health(&fb);
        assert_eq!(h.distinct_colors, 4);
        assert!(
            !h.looks_blank(),
            "four colours covering the whole frame is a pattern, not a blank screen"
        );
    }

    /// The dominant-fraction arm, pinned at its physical anchor: fewer
    /// non-backdrop pixels than a single 8x8 tile (64) means nothing was drawn.
    #[test]
    fn near_uniform_frame_is_blank() {
        const TOTAL: u32 = 256 * 240;
        let mut fb = Vec::with_capacity((TOTAL as usize) * 4);
        // 32 stray pixels — half a tile's worth — on an otherwise solid frame.
        for i in 0..TOTAL {
            let v = u8::from(i < 32) * 255;
            fb.extend_from_slice(&[v, v, v, 255]);
        }
        let h = frame_health(&fb);
        assert_eq!(h.distinct_colors, 2);
        assert!(
            h.looks_blank(),
            "32 stray pixels is less than one tile: nothing was drawn"
        );
    }

    /// ...and one tile's worth of content clears it.
    #[test]
    fn one_tile_of_content_is_not_blank() {
        const TOTAL: u32 = 256 * 240;
        let mut fb = Vec::with_capacity((TOTAL as usize) * 4);
        for i in 0..TOTAL {
            let v = u8::from(i < 128) * 255;
            fb.extend_from_slice(&[v, v, v, 255]);
        }
        let h = frame_health(&fb);
        assert!(
            !h.looks_blank(),
            "two tiles' worth of drawn pixels is content, however dark the rest"
        );
    }

    #[test]
    fn many_colours_is_not_blank() {
        // 64 distinct colours spread evenly: a "real screen".
        let mut fb = Vec::with_capacity(256 * 240 * 4);
        for i in 0u32..(256 * 240) {
            let v = u8::try_from(i % 64).unwrap() * 4;
            fb.extend_from_slice(&[v, 255 - v, v / 2, 255]);
        }
        let h = frame_health(&fb);
        assert!(h.distinct_colors > BLANK_MAX_DISTINCT_COLORS);
        assert!(h.dominant_fraction < BLANK_MIN_DOMINANT_FRACTION);
        assert!(!h.looks_blank());
    }
}
