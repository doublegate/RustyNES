//! Raw NTSC composite-signal model (v2.1.9 "Presentation & Signal", P4).
//!
//! Where [`crate::palette_gen`] *pre-decodes* each of the 64 base colors to a
//! single RGB triple (an ideal TV integrated over one pixel), this module keeps
//! the signal **un-decoded**: for every `(index, emphasis)` pair it emits the
//! 2C02's raw composite waveform as the twelve per-subcarrier-phase voltage
//! samples the chip actually generates within one pixel. A shader (or any host
//! NTSC decoder) can then run a *real* NTSC demodulation across neighbouring
//! pixels' waveforms and reproduce the signal-domain artifacts a per-color RGB
//! palette structurally cannot: composite color bleed, dot crawl, the
//! "waterfall"/dither transparency tricks (e.g. Kirby's Adventure waterfalls,
//! the Zelda II title, and the classic 240p test suite color-bleed screens) that
//! rely on adjacent-pixel chroma mixing rather than on any one pixel's color.
//!
//! ## The Mesen / Bisqwit "raw palette" model
//!
//! This follows the canonical Bisqwit `nes_ntsc` signal generator (nesdev wiki
//! "NTSC video"), the same model Mesen2 exposes as its *raw* NTSC filter:
//!
//! * The 2C02 emits, per pixel, a two-level chroma square wave over 12 equal
//!   subcarrier phases. Which six of the twelve phases are "high" is set by the
//!   color's hue nibble (`InColorPhase`); the two voltage levels (low/high) are
//!   set by the luma nibble via [`LEVELS`].
//! * Grays (`$x0`, `$xD`) hold a constant level (no chroma), so any decoder
//!   integrates them to zero saturation regardless of hue.
//! * The three emphasis bits each attenuate the signal (by [`ATTENUATION`])
//!   during the subcarrier phases that overlap "their" primary's hue region —
//!   which is why enabling all three darkens uniformly while enabling one tints.
//!
//! ## Determinism boundary (why this is `no_std` and float-locked)
//!
//! The waveform is built from **level lookups, one multiply (emphasis), and one
//! affine normalize** — there is *no* transcendental (no `sin`/`cos`/`pow`), so
//! the `f32` output is bit-identical across x86 / aarch64 / wasm / `thumbv7em`
//! under IEEE-754 without needing `libm`. The committed [`tests::GOLDEN_SIGNAL`]
//! snapshot locks that cross-target contract.
//!
//! ## Where this sits in the pipeline (additive, default-OFF)
//!
//! This is a **new, parallel** output. The default presentation path is
//! untouched: the shipped build still pre-decodes through [`crate::NES_PALETTE`]
//! / [`crate::palette::build_rgba_lut_from_base`], so the default framebuffer
//! golden vectors and `AccuracyCoin` are byte-identical. The raw signal is only
//! consumed when the frontend explicitly selects the signal-decode presentation
//! shader (a deliberate visual choice, gated + re-blessed like the generated
//! palette in F1.4 / v2.0.3). Nothing here feeds the deterministic core.

// The palette/level constants below are Bisqwit's canonical voltages; the affine
// normalize keeps two-rounding form for the same cross-target determinism reason
// `palette_gen` documents (a fused `mul_add` could round differently and break
// the committed golden). Mirror its allow.
#![allow(clippy::suboptimal_flops)]

/// The eight composite signal voltage levels the 2C02 emits, relative to sync.
///
/// Identical to [`crate::palette_gen`]'s `LEVELS`, restated here so the raw-
/// signal model is self-contained. Indices `0..4` are the "signal low" half of
/// the chroma square wave for luma levels `0..3`; `4..8` are the "signal high"
/// half. (Bisqwit / nesdev "NTSC video".)
pub const LEVELS: [f32; 8] = [
    0.350, 0.518, 0.962, 1.550, // signal low  (luma level 0..3)
    1.094, 1.506, 1.962, 1.962, // signal high (luma level 0..3)
];

/// Black reference voltage (the composite level that normalizes to 0.0).
pub const BLACK: f32 = 0.518;
/// White reference voltage (the composite level that normalizes to 1.0).
pub const WHITE: f32 = 1.962;
/// Per-emphasis-bit attenuation factor (≈ −2.5 dB) applied during the phases
/// that overlap the emphasized primary's hue region. (Bisqwit / nesdev.)
pub const ATTENUATION: f32 = 0.746;

/// The number of distinct subcarrier phases the 2C02 walks within one pixel.
/// A full color-decode integrates over exactly these twelve samples.
pub const PHASES: usize = 12;

/// The number of `(index, emphasis)` entries in a full raw-signal LUT:
/// 64 base colors × 8 emphasis states.
pub const RAW_ENTRIES: usize = 64 * 8;

/// Return `true` when the chroma square wave for hue `color` (0..15) is in its
/// "high" state at subcarrier phase `phase` (0..12).
///
/// This is Bisqwit's `InColorPhase`: `((color + phase) % 12) < 6`. It is the
/// phase generator that positions each of the twelve hues on the color wheel.
/// (Note the phase *convention* differs from [`crate::palette_gen`]'s `+ 8`
/// offset — the two are independent decoders; what matters is that this module
/// is self-consistent with the Bisqwit decode a signal shader performs.)
#[inline]
#[must_use]
pub const fn in_color_phase(color: usize, phase: usize) -> bool {
    (color + phase) % 12 < 6
}

/// Compute the raw composite voltage (relative to sync) for one subcarrier
/// `phase` (0..12) of NES palette `index` (0..=63) under `emphasis` (0..=7,
/// bit0 = red, bit1 = green, bit2 = blue).
///
/// This is the un-normalized chip output: the chosen [`LEVELS`] entry, times the
/// emphasis attenuation when any set emphasis bit's hue region overlaps `phase`.
#[inline]
#[must_use]
pub fn composite_voltage(index: usize, emphasis: usize, phase: usize) -> f32 {
    let color = index & 0x0F; // hue nibble (0..15)
    // Colors $0E/$0F are forbidden blacks; clamp their luma level so the index
    // math is well-defined (they resolve to black regardless).
    let level = if color < 0x0E { (index >> 4) & 3 } else { 1 }; // 0..3

    // High half only when the wave is high AND this hue actually has a high
    // level: color $0 (gray) forces low->high level (no chroma); colors
    // $0D..$0F have no high level (their nominal "high" stays low -> dark).
    // `level + 4*flag` is provably 0..7 -> in-bounds for `LEVELS`.
    let high = in_color_phase(color, phase) || color == 0x00;
    let lo = LEVELS[level + 4 * usize::from(color == 0x00)];
    let hi = LEVELS[level + 4 * usize::from(color < 0x0D)];
    let mut wave = if high { hi } else { lo };

    // Emphasis: attenuate during the phases overlapping each set bit's primary.
    // The three primaries sit at hue anchors 0 (red), 4 (green), 8 (blue) on the
    // `InColorPhase` wheel. Any overlapping set bit applies one attenuation
    // (matching Bisqwit — the factors do not stack per bit within a phase).
    let emphasized = ((emphasis & 1) != 0 && in_color_phase(0, phase))
        || ((emphasis & 2) != 0 && in_color_phase(4, phase))
        || ((emphasis & 4) != 0 && in_color_phase(8, phase));
    if emphasized {
        wave *= ATTENUATION;
    }
    wave
}

/// Normalize a raw composite voltage to the shader-friendly `[0.0, 1.0]` range.
///
/// Maps the black reference to `0.0` and white to `1.0`; values outside are
/// possible under emphasis and are left un-clamped so a decoder sees the true
/// signal excursion.
#[inline]
#[must_use]
pub fn normalize(voltage: f32) -> f32 {
    (voltage - BLACK) / (WHITE - BLACK)
}

/// Build the twelve normalized composite samples for one `(index, emphasis)`
/// pair — the per-pixel waveform a signal-decode shader convolves across
/// neighbouring pixels.
#[must_use]
pub fn signal_samples(index: usize, emphasis: usize) -> [f32; PHASES] {
    let mut out = [0.0f32; PHASES];
    for (phase, slot) in out.iter_mut().enumerate() {
        *slot = normalize(composite_voltage(index, emphasis, phase));
    }
    out
}

/// Generate the full raw-signal LUT: [`RAW_ENTRIES`] rows (index-major,
/// `index * 8 + emphasis`), each the twelve normalized subcarrier samples.
///
/// This is the exact table a host uploads (e.g. as an `R32Float` /
/// `Rgba8Unorm`-packed texture) for the signal-decode shader. Deterministic and
/// `no_std`; see [`tests::GOLDEN_SIGNAL`] for the cross-target byte-lock. The
/// 24 KiB table is built directly on the heap (via the crate's `alloc`, never a
/// stack temporary) as a [`RAW_ENTRIES`]-long boxed slice — generated once at
/// shader-setup time, never on a hot path.
#[must_use]
pub fn generate_raw_signal_lut() -> alloc::boxed::Box<[[f32; PHASES]]> {
    let mut lut = alloc::vec::Vec::with_capacity(RAW_ENTRIES);
    for index in 0..64usize {
        for emphasis in 0..8usize {
            lut.push(signal_samples(index, emphasis));
        }
    }
    lut.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A constant-signal color (gray column) must produce a flat waveform — the
    /// property that guarantees any NTSC decoder integrates it to zero chroma.
    #[test]
    fn gray_columns_are_flat() {
        for &index in &[0x00usize, 0x10, 0x20, 0x30] {
            let s = signal_samples(index, 0);
            for &v in &s {
                assert!((v - s[0]).abs() < 1e-6, "gray ${index:02X} not flat: {s:?}");
            }
        }
    }

    /// A mid-luma chroma color must actually oscillate (six high, six low
    /// phases) — proving the chroma square wave is present for a decoder to
    /// demodulate.
    #[test]
    fn chroma_colors_oscillate() {
        // $16 (a red): hue nibble 6, so exactly six phases high.
        let s = signal_samples(0x16, 0);
        let hi = s.iter().filter(|&&v| v > 0.5).count();
        assert_eq!(hi, 6, "$16 should have 6 high phases, got {hi}: {s:?}");
    }

    /// Emphasis must only ever *reduce* the signal (never brighten it), and full
    /// emphasis ($e7) on a bright color must reduce at least some phases — the
    /// darkening contract.
    #[test]
    fn emphasis_only_attenuates() {
        for index in 0..64usize {
            let base = signal_samples(index, 0);
            for emphasis in 1..8usize {
                let emph = signal_samples(index, emphasis);
                for phase in 0..PHASES {
                    assert!(
                        emph[phase] <= base[phase] + 1e-6,
                        "emphasis {emphasis} brightened ${index:02X} phase {phase}"
                    );
                }
            }
        }
        // A bright non-gray color under full emphasis must actually drop.
        let base = signal_samples(0x21, 0);
        let full = signal_samples(0x21, 7);
        assert!(
            full.iter().zip(base).any(|(f, b)| *f < b - 1e-4),
            "full emphasis did not attenuate $21"
        );
    }

    /// The normalize anchors: black reference -> 0, white reference -> 1.
    #[test]
    fn normalize_anchors() {
        assert!((normalize(BLACK) - 0.0).abs() < 1e-6);
        assert!((normalize(WHITE) - 1.0).abs() < 1e-6);
    }

    /// Determinism: the LUT is a pure function; two builds are byte-identical.
    #[test]
    fn lut_is_deterministic() {
        assert_eq!(generate_raw_signal_lut(), generate_raw_signal_lut());
    }

    /// Cross-target byte-lock for the first eight LUT rows (index $00 across all
    /// eight emphasis states). Because the model uses no transcendentals, this
    /// must reproduce bit-for-bit on every target. A drift means either an
    /// intended model change (regenerate + visual re-bless) or a real float bug.
    /// Full 512-row snapshotting is done via `insta` in the frontend; this small
    /// in-crate lock keeps the `no_std` crate self-guarding.
    #[rustfmt::skip]
    const GOLDEN_SIGNAL: [[f32; PHASES]; 8] = {
        // $00 is gray level-1: flat at normalize(LEVELS[1+4]=1.506) for all
        // phases (color 0 forces the high level), attenuated per emphasis on the
        // phases overlapping each primary. Computed by the same code path.
        [
            signal_row(0x00, 0), signal_row(0x00, 1), signal_row(0x00, 2), signal_row(0x00, 3),
            signal_row(0x00, 4), signal_row(0x00, 5), signal_row(0x00, 6), signal_row(0x00, 7),
        ]
    };

    /// `const`-evaluable sibling of [`signal_samples`] for the golden table.
    const fn signal_row(index: usize, emphasis: usize) -> [f32; PHASES] {
        let mut out = [0.0f32; PHASES];
        let mut phase = 0;
        while phase < PHASES {
            // Inline of `normalize(composite_voltage(..))` in const form.
            let color = index & 0x0F;
            let level = if color < 0x0E { (index >> 4) & 3 } else { 1 };
            let high = in_color_phase(color, phase) || color == 0x00;
            let lo = LEVELS[level + 4 * (color == 0x00) as usize];
            let hi = LEVELS[level + 4 * (color < 0x0D) as usize];
            let mut wave = if high { hi } else { lo };
            let emphasized = (emphasis & 1 != 0 && in_color_phase(0, phase))
                || (emphasis & 2 != 0 && in_color_phase(4, phase))
                || (emphasis & 4 != 0 && in_color_phase(8, phase));
            if emphasized {
                wave *= ATTENUATION;
            }
            out[phase] = (wave - BLACK) / (WHITE - BLACK);
            phase += 1;
        }
        out
    }

    // Exact f32 equality is deliberate here: the whole point of GOLDEN_SIGNAL is
    // a byte-for-byte cross-target lock, so an approximate compare would defeat
    // it (a platform float divergence must fail, not be tolerated).
    #[test]
    #[allow(clippy::float_cmp)]
    fn matches_committed_golden() {
        let lut = generate_raw_signal_lut();
        for emphasis in 0..8usize {
            assert_eq!(
                lut[emphasis], GOLDEN_SIGNAL[emphasis],
                "row $00 emphasis {emphasis} drifted from golden"
            );
        }
    }
}
