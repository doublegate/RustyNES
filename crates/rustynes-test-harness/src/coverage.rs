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

/// At most this many distinct colours means treat the frame as blank.
///
/// A real NES screen draws well more than four distinct RGBA values; a
/// backdrop-only or crashed boot shows <= 4 (the reset code's handful of
/// palette writes). This is the heuristic the `coverage_smoke` bin
/// flagged `SUSPICIOUS` at.
pub const BLANK_MAX_DISTINCT_COLORS: usize = 4;

/// Dominant-colour threshold for the blank verdict.
///
/// If a single colour fills at least this fraction of the frame, treat
/// it as backdrop-only even when a few stray colours bump the distinct
/// count above [`BLANK_MAX_DISTINCT_COLORS`]. Matches `render_smoke`'s
/// dominant-fraction sentinel (it used `< 0.95` for "rendered"; we use
/// the looser `>= 0.99` here so a deliberately small but real title
/// palette (e.g. Mito Koumon) is not falsely flagged by the coverage
/// gate, which must never panic on a genuine screen).
pub const BLANK_MIN_DOMINANT_FRACTION: f64 = 0.99;

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

/// Recursively collect every `.nes` file under `root`, appending to `out`.
///
/// Order is filesystem-dependent; callers that need determinism sort the
/// result. Missing / unreadable directories are silently skipped (a
/// fresh checkout's gitignored-absent `external/` tree returns nothing
/// rather than erroring). This is the walk the `coverage_smoke` bin uses
/// to sweep an arbitrary external directory.
pub fn walk_nes(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_nes(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nes")) {
            out.push(p);
        }
    }
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

    #[test]
    fn few_colours_is_blank() {
        // 4 distinct colours, evenly split: under the distinct-count
        // threshold, so blank even though no colour dominates.
        let mut fb = Vec::with_capacity(256 * 240 * 4);
        for i in 0u32..(256 * 240) {
            let v = u8::try_from(i % 4).unwrap() * 64;
            fb.extend_from_slice(&[v, v, v, 255]);
        }
        let h = frame_health(&fb);
        assert_eq!(h.distinct_colors, 4);
        assert!(h.looks_blank());
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
