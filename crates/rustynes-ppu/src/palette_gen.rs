//! Generated NTSC base palette (v2.1.2 "Fathom" F1.4).
//!
//! The hand-authored [`crate::NES_PALETTE`] is one artist's calibration of a
//! Sony PVM reference. This module instead *synthesizes* the 64-entry base
//! palette from a model of the 2C02's composite-video output, following the
//! Bisqwit / ares (`fc/ppu/color.cpp`) approach: for each of the 64 colors,
//! integrate the PPU's two-level chroma square wave over the 12 subcarrier
//! phases of one pixel, demodulate to YIQ, and convert to RGB through the FCC
//! matrix with a gamma correction. The result is deterministic, parameterized
//! (saturation / hue / contrast / brightness / gamma), and — because every
//! transcendental goes through `libm` rather than `std` — **byte-identical on
//! every target** (x86 / aarch64 / wasm / `thumbv7em`).
//!
//! ## Where this sits in the pipeline (determinism boundary)
//!
//! This function produces a 64-entry `[[u8; 3]; 64]` base, exactly the shape a
//! loaded `.pal` file yields. The frontend feeds it to
//! `Nes::set_custom_palette(Some(base))`, and the PPU applies the *same*
//! [`crate::palette::build_rgba_lut_from_base`] emphasis model it uses for the
//! hand palette and any `.pal` — so there is **no new emphasis path** and the
//! generated palette is a drop-in alternative base. It is **off by default**:
//! the shipped build keeps [`crate::NES_PALETTE`], so the default-build
//! framebuffer golden vectors (and `AccuracyCoin`) are unchanged. Selecting the
//! generated palette changes framebuffer output and is therefore gated in the
//! frontend behind an explicit palette-source choice with a deliberate visual
//! re-bless (F1.4 / F2.2 plan, v2.0.3 precedent).
//!
//! ## Model reference
//!
//! The waveform constants (the eight composite voltage levels, the
//! sync/black/white references, and the FCC YIQ→RGB matrix) are Bisqwit's
//! canonical NES palette generator as published on the nesdev wiki ("NTSC
//! video"); ares' `PPU::Color` uses the same integration. The `hue` parameter
//! is a global tint in subcarrier-phase units (each unit = 30°); grays are
//! hue-independent because a constant signal integrates to zero chroma.

// The pedantic `suboptimal_flops` lint suggests `mul_add` for the `a*b + c`
// spots in the integration + YIQ→RGB matrix. We deliberately keep plain
// mul-then-add: `mul_add` fuses to a single rounding whose result can differ
// from the two-rounding form, and this palette feeds a committed cross-target
// golden snapshot — determinism beats the micro-optimization. Mirrors the APU
// crate's identical allow (`rustynes-apu/src/lib.rs`).
#![allow(clippy::suboptimal_flops)]

use libm::{cos, pow, round, sin};

/// Core-math constant: π. `libm` takes radians; we build phase angles from this
/// so the whole synthesizer is `no_std` and does not depend on `core::f64`
/// associated consts being stable in a `const` context.
const PI: f64 = core::f64::consts::PI;

/// The eight composite signal voltage levels the 2C02 emits, relative to the
/// sync tip. Indices `0..4` are the "signal low" half of the chroma square
/// wave for luma levels `0..3`; indices `4..8` are the "signal high" half.
/// (Bisqwit / nesdev "NTSC video".)
const LEVELS: [f64; 8] = [
    0.350, 0.518, 0.962, 1.550, // signal low  (luma level 0..3)
    1.094, 1.506, 1.962, 1.962, // signal high (luma level 0..3)
];

/// Black reference voltage (the composite level that maps to RGB 0).
const BLACK: f64 = 0.518;
/// White reference voltage (the composite level that maps to full RGB).
const WHITE: f64 = 1.962;

/// Tunable parameters for [`generate_base_palette`].
///
/// All are pure inputs to a deterministic function; the same params always
/// yield the same 64-entry base. [`Self::default`] is the neutral calibration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NtscPaletteParams {
    /// Chroma gain. `1.0` is neutral; higher is more saturated, `0.0` is
    /// grayscale.
    pub saturation: f64,
    /// Global hue rotation, in subcarrier-phase units (1 unit = 30°). `0.0` is
    /// the standard orientation. Grays are unaffected.
    pub hue: f64,
    /// Luma contrast about mid-gray. `1.0` is neutral.
    pub contrast: f64,
    /// Overall luma gain. `1.0` is neutral.
    pub brightness: f64,
    /// Display gamma used for the `f^(2.2/gamma)` correction. `2.2` is a
    /// no-op; values below `2.2` darken the mid-tones (CRT-like). Default
    /// `1.8` matches the common Bisqwit-generator look on sRGB displays.
    pub gamma: f64,
}

impl Default for NtscPaletteParams {
    fn default() -> Self {
        Self {
            saturation: 1.0,
            hue: 0.0,
            contrast: 1.0,
            brightness: 1.0,
            gamma: 1.8,
        }
    }
}

/// Return `true` when the chroma square wave for `color` is in its "high" state
/// at subcarrier phase `p` (0..12). This is the phase generator that gives each
/// of the 12 hues its position on the color wheel; the `+ 8` aligns hue index 1
/// to the standard orientation (Bisqwit / nesdev). All operands are small and
/// non-negative, so the arithmetic stays in `usize`.
#[inline]
const fn wave_high(p: usize, color: usize) -> bool {
    ((color + p + 8) % 12) < 6
}

/// Synthesize the 64-entry RGB888 base palette from `params`.
///
/// The output is a drop-in replacement for [`crate::NES_PALETTE`] (emphasis is
/// **not** baked in here — the PPU's existing `build_rgba_lut_from_base` applies
/// it). Deterministic and `no_std`; see the module docs for the model.
#[must_use]
pub fn generate_base_palette(params: &NtscPaletteParams) -> [[u8; 3]; 64] {
    let mut out = [[0u8; 3]; 64];
    for (pixel, slot) in out.iter_mut().enumerate() {
        *slot = generate_one(pixel, params);
    }
    out
}

/// Synthesize a single color (`pixel` = the 6-bit NES index, 0..64).
fn generate_one(pixel: usize, params: &NtscPaletteParams) -> [u8; 3] {
    let color = pixel & 0x0F; // chroma / hue nibble (0..15)
    // Colors $0E/$0F are "forbidden" blacks; clamp their luma level to 1 so the
    // math is well-defined (they resolve to black regardless).
    let level = if color < 0x0E { (pixel >> 4) & 3 } else { 1 }; // 0..3

    // The two composite voltage levels this color alternates between:
    //   lo (wave in its low state), hi (wave in its high state).
    // Color $0 (gray) forces the low state up to the high level (no chroma);
    // colors $0D..$0F have no high level (their high state stays low → dark).
    // `level + 4*flag` is provably 0..7, indexing `LEVELS` in-bounds.
    let lo = LEVELS[level + 4 * usize::from(color == 0x00)];
    let hi = LEVELS[level + 4 * usize::from(color < 0x0D)];

    // Integrate over the 12 subcarrier phases of one pixel, demodulating to
    // YIQ (an ideal TV NTSC decoder).
    let mut y = 0.0f64;
    let mut i_acc = 0.0f64;
    let mut q_acc = 0.0f64;
    for ph in 0..12usize {
        let spot = if wave_high(ph, color) { hi } else { lo };
        // Normalize composite voltage to a 0..1 signal, then apply
        // contrast (about mid-gray) and brightness (averaged over 12 phases).
        let mut signal = (spot - BLACK) / (WHITE - BLACK);
        signal = (signal - 0.5) * params.contrast + 0.5;
        signal *= params.brightness / 12.0;

        #[allow(clippy::cast_precision_loss)] // ph < 12 → exact in f64.
        let angle = PI * (params.hue + ph as f64) / 6.0;
        y += signal;
        i_acc += signal * cos(angle);
        q_acc += signal * sin(angle);
    }
    i_acc *= params.saturation;
    q_acc *= params.saturation;

    // FCC-sanctioned YIQ→RGB matrix, with gamma correction and 0..255 clamp.
    let r = fcc_channel(y + 0.946_882 * i_acc + 0.623_557 * q_acc, params.gamma);
    let g = fcc_channel(y - 0.274_788 * i_acc - 0.635_691 * q_acc, params.gamma);
    let b = fcc_channel(y - 1.108_545 * i_acc + 1.709_007 * q_acc, params.gamma);
    [r, g, b]
}

/// Gamma-correct one YIQ→RGB channel value and quantize to `u8` (0..255).
///
/// The `scaled as u8` in the mid branch is guarded: `scaled` is provably in the
/// open interval `(0, 255)` there, so the truncation + sign-loss the cast would
/// otherwise risk cannot occur (hence the justified `allow`).
#[inline]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn fcc_channel(f: f64, gamma: f64) -> u8 {
    let corrected = if f <= 0.0 { 0.0 } else { pow(f, 2.2 / gamma) };
    let scaled = round(255.0 * corrected);
    // Clamp; `round` keeps determinism (libm), the clamp guards overflow.
    if scaled <= 0.0 {
        0
    } else if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Luma of an RGB triple (integer approximation, for ordering assertions).
    fn luma(rgb: [u8; 3]) -> u32 {
        u32::from(rgb[0]) + u32::from(rgb[1]) + u32::from(rgb[2])
    }

    #[test]
    fn generation_is_deterministic() {
        // Same params ⇒ byte-identical output, every call. This is the hard
        // contract that lets the committed golden snapshot hold across targets.
        let p = NtscPaletteParams::default();
        let a = generate_base_palette(&p);
        let b = generate_base_palette(&p);
        assert_eq!(a, b);
    }

    #[test]
    fn grays_are_neutral_regardless_of_hue() {
        // Color nibble 0 ($x0) is a chroma-free luma column: a constant signal
        // integrates to zero chroma, so R==G==B for any hue/saturation.
        for hue in [-2.0, 0.0, 1.0, 3.5] {
            let p = NtscPaletteParams {
                hue,
                saturation: 1.5,
                ..NtscPaletteParams::default()
            };
            let pal = generate_base_palette(&p);
            for &pixel in &[0x00usize, 0x10, 0x20, 0x30] {
                let [r, g, b] = pal[pixel];
                assert!(
                    r == g && g == b,
                    "gray ${pixel:02X} not neutral: {r},{g},{b} (hue={hue})"
                );
            }
        }
    }

    #[test]
    fn white_black_anchors() {
        let pal = generate_base_palette(&NtscPaletteParams::default());
        // $0F is a forbidden black → pure black.
        assert_eq!(pal[0x0F], [0, 0, 0], "$0F must be black");
        // $20 is the brightest gray column entry → white (all channels max).
        assert_eq!(pal[0x20], [255, 255, 255], "$20 must be white");
        // $30 saturates at the same top level as $20 (color-0 column tops out).
        assert_eq!(
            pal[0x30], pal[0x20],
            "$30 == $20 (color-0 column saturates)"
        );
    }

    #[test]
    fn gray_column_is_a_monotonic_luma_ramp() {
        // $00 < $10 < $20 in luma (the color-0 column climbs, then saturates).
        let pal = generate_base_palette(&NtscPaletteParams::default());
        assert!(luma(pal[0x00]) < luma(pal[0x10]), "$00 !< $10");
        assert!(luma(pal[0x10]) < luma(pal[0x20]), "$10 !< $20");
    }

    #[test]
    fn saturation_zero_is_grayscale() {
        // With no chroma gain every entry collapses to neutral gray.
        let p = NtscPaletteParams {
            saturation: 0.0,
            ..NtscPaletteParams::default()
        };
        let pal = generate_base_palette(&p);
        for (idx, &[r, g, b]) in pal.iter().enumerate() {
            assert!(r == g && g == b, "idx ${idx:02X} not gray at saturation 0");
        }
    }

    /// The default-parameter generated palette, captured once. This locks the
    /// exact cross-target output: because every transcendental goes through
    /// `libm`, this array must reproduce byte-for-byte on x86 / aarch64 / wasm /
    /// `thumbv7em`. Regenerate **only** on a deliberate, reviewed model/param
    /// change (a visual re-bless), never incidentally.
    #[rustfmt::skip]
    const GOLDEN_DEFAULT: [[u8; 3]; 64] = [
        [83,83,83],[2,27,81],[16,15,102],[36,7,99],[54,3,75],[65,4,38],[63,10,5],[51,20,0],
        [31,32,0],[12,43,0],[0,48,0],[0,46,10],[0,38,46],[0,0,0],[0,0,0],[0,0,0],
        [160,160,160],[31,74,158],[57,55,189],[89,41,185],[117,34,149],[133,36,92],[131,46,36],[111,63,1],
        [81,83,0],[50,99,0],[26,107,5],[15,105,47],[16,93,104],[0,0,0],[0,0,0],[0,0,0],
        [255,255,255],[106,158,252],[137,136,255],[175,118,255],[207,110,242],[225,112,179],[222,125,113],[201,145,62],
        [166,168,38],[129,187,40],[100,196,71],[85,193,125],[87,179,192],[60,60,60],[0,0,0],[0,0,0],
        [255,255,255],[191,214,254],[205,204,255],[221,196,255],[235,192,250],[242,194,223],[241,199,194],[232,208,170],
        [218,218,158],[201,226,159],[188,230,174],[181,229,200],[182,223,229],[169,169,169],[0,0,0],[0,0,0],
    ];

    #[test]
    fn matches_committed_golden() {
        // Cross-target byte-lock (see GOLDEN_DEFAULT). A drift here means either
        // a model change (intended → regenerate the const in the same PR, with a
        // visual re-bless) or a platform float divergence (a real bug).
        let pal = generate_base_palette(&NtscPaletteParams::default());
        assert_eq!(pal, GOLDEN_DEFAULT);
    }

    #[test]
    fn colored_entries_are_actually_colored() {
        // A mid-luma chroma entry (e.g. $16, a red) must have a real channel
        // spread at default saturation — proving chroma demodulation works.
        let pal = generate_base_palette(&NtscPaletteParams::default());
        let [r, g, b] = pal[0x16];
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        assert!(max - min > 20, "$16 not colored enough: {r},{g},{b}");
    }
}
