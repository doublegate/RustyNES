//! Optional graphic/parametric equalizer — a **frontend output stage** (v1.1.0
//! beta.2, Workstream D, T-110-D2).
//!
//! This runs on the producer side *after* the dynamic-rate resampler and
//! *before* the lock-free queue, exactly like the master-gain stage: it touches
//! only the host-rate output samples, never the deterministic core synthesis.
//! With the EQ disabled (the default) the producer skips it entirely, so the
//! audio is byte-identical to a build without this module.
//!
//! Five fixed bands (60 / 240 / 1 k / 3.8 k / 12 k Hz) of RBJ-cookbook peaking
//! biquads in cascade, each with an independent gain in dB (−12..=+12). A band
//! at 0 dB is an identity filter; when every band is 0 dB the whole stage is a
//! no-op and is bypassed. Mono (the NES mixes to one channel).
//!
//! v1.7.0 "Forge" H3 — the EQ is now band-count-generic: the original 5-band
//! voicing is retained, and a **20-band graphic EQ** at the standard ISO octave
//! /third-octave center frequencies ([`EQ20_FREQS`]) is selectable. Either way
//! a flat (all-0-dB) bank is bypassed and bit-identical to a no-EQ build, so the
//! default (flat) output is byte-identical.
//!
//! Reference: Mesen2 `Utilities/Audio/Equalizer.h`; RBJ "Audio EQ Cookbook".

// Audio DSP: the FMA-vs-separate-ops rounding difference is inaudible, and the
// u32->f32 sample-rate cast is exact for any real device rate. Readability of the
// textbook biquad form wins here.
#![allow(clippy::suboptimal_flops, clippy::cast_precision_loss)]

use core::f32::consts::PI;

/// Center frequencies (Hz) of the five fixed bands.
pub const BAND_FREQS: [f32; 5] = [60.0, 240.0, 1_000.0, 3_800.0, 12_000.0];

/// Number of EQ bands (the classic 5-band voicing).
pub const BAND_COUNT: usize = 5;

/// v1.7.0 H3 — number of bands in the graphic EQ (20).
pub const EQ20_BAND_COUNT: usize = 20;

/// v1.7.0 H3 — center frequencies (Hz) of the 20-band graphic EQ, on (close to)
/// the ISO standard third-octave grid spanning the NES audible range.
pub const EQ20_FREQS: [f32; EQ20_BAND_COUNT] = [
    25.0, 40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1_000.0, 1_600.0, 2_500.0, 4_000.0,
    6_300.0, 8_000.0, 10_000.0, 12_500.0, 14_000.0, 16_000.0, 18_000.0, 20_000.0,
];

/// Per-band Q (bandwidth) for the classic 5-band voicing. A moderate value so
/// adjacent bands overlap smoothly.
const BAND_Q: f32 = 0.9;

/// Per-band Q for the 20-band graphic EQ. Denser bands want a higher Q so each
/// slider stays a recognisable third-octave control.
const BAND_Q_20: f32 = 2.1;

/// A single Direct-Form-I peaking biquad.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// RBJ peaking-EQ coefficients for `freq` (Hz) at `gain_db` and `sample_rate`.
    fn peaking(freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        // Guard against a band at or above Nyquist (possible at very low host
        // sample rates, e.g. an 8 kHz device): the RBJ math would yield an
        // unstable filter. The explicit `!is_finite()` check also falls back to
        // identity for a NaN / inf sample_rate (an uninitialized / corrupt host
        // device) rather than propagating NaNs through the audio thread —
        // expressed without a negated partial-ord comparison (clippy
        // `neg_cmp_op_on_partial_ord`).
        if !sample_rate.is_finite() || freq >= sample_rate * 0.5 {
            return Self {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
                x1: 0.0,
                x2: 0.0,
                y1: 0.0,
                y2: 0.0,
            };
        }
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * (freq / sample_rate);
        let (sin_w0, cos_w0) = (w0.sin(), w0.cos());
        let alpha = sin_w0 / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x0: f32) -> f32 {
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        y0
    }
}

/// A cascaded peaking equalizer over a mono host-rate stream.
///
/// Band-count-generic since v1.7.0 H3: the same cascade serves the classic
/// 5-band voicing ([`Equalizer::new`]) and the 20-band graphic EQ
/// ([`Equalizer::new_20`]). A flat bank (every gain 0 dB) is bypassed and
/// bit-identical to a no-EQ build.
pub struct Equalizer {
    bands: Vec<Biquad>,
    /// `true` when every band is 0 dB (the stage is a pure passthrough).
    bypass: bool,
}

impl Equalizer {
    /// Build a 5-band equalizer for `sample_rate` with per-band gains in dB.
    #[must_use]
    pub fn new(gains_db: [f32; BAND_COUNT], sample_rate: u32) -> Self {
        Self::from_bands(&BAND_FREQS, &gains_db, BAND_Q, sample_rate)
    }

    /// v1.7.0 H3 — build a 20-band graphic equalizer for `sample_rate` with
    /// per-band gains in dB at the [`EQ20_FREQS`] center frequencies.
    #[must_use]
    pub fn new_20(gains_db: [f32; EQ20_BAND_COUNT], sample_rate: u32) -> Self {
        Self::from_bands(&EQ20_FREQS, &gains_db, BAND_Q_20, sample_rate)
    }

    /// Build a cascade from matching `freqs` / `gains_db` slices at a shared `q`.
    fn from_bands(freqs: &[f32], gains_db: &[f32], q: f32, sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let bands = freqs
            .iter()
            .zip(gains_db.iter())
            .map(|(&f, &g)| Biquad::peaking(f, g, q, sr))
            .collect();
        let bypass = gains_db.iter().all(|&g| g.abs() < f32::EPSILON);
        Self { bands, bypass }
    }

    /// `true` when all bands are flat (caller may skip the stage).
    #[must_use]
    pub const fn is_bypass(&self) -> bool {
        self.bypass
    }

    /// Filter `samples` in place. A no-op when bypassed.
    pub fn process(&mut self, samples: &mut [f32]) {
        if self.bypass {
            return;
        }
        for s in samples {
            let mut v = *s;
            for band in &mut self.bands {
                v = band.process(v);
            }
            *s = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)] // intentional: bypass must be BIT-identical.
    fn flat_eq_is_bypassed_and_identity() {
        let mut eq = Equalizer::new([0.0; BAND_COUNT], 44_100);
        assert!(eq.is_bypass());
        let mut buf = [0.1, -0.2, 0.3, -0.4, 0.5];
        let orig = buf;
        eq.process(&mut buf);
        assert!(
            buf.iter()
                .zip(orig)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "flat EQ must not alter samples"
        );
    }

    #[test]
    fn nonflat_eq_is_active_and_stable() {
        // A boost band makes the stage active; output must stay finite/bounded
        // for a bounded input (filter stability sanity).
        let mut eq = Equalizer::new([6.0, 0.0, -6.0, 0.0, 3.0], 44_100);
        assert!(!eq.is_bypass());
        let mut buf: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        eq.process(&mut buf);
        assert!(
            buf.iter().all(|v| v.is_finite() && v.abs() < 8.0),
            "EQ output must remain finite and bounded"
        );
    }

    /// RMS of the settled tail of a sine at `freq` after the EQ.
    fn settled_rms(freq: f32, gains: [f32; BAND_COUNT]) -> f32 {
        let sr = 44_100.0_f32;
        let mut eq = Equalizer::new(gains, 44_100);
        let mut buf: Vec<f32> = (0..8192)
            .map(|i| (2.0 * PI * freq * i as f32 / sr).sin())
            .collect();
        eq.process(&mut buf);
        let tail = &buf[4096..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
    }

    #[test]
    #[allow(clippy::float_cmp)] // intentional: flat 20-band bypass must be BIT-identical.
    fn flat_20_band_eq_is_bypassed_and_identity() {
        let mut eq = Equalizer::new_20([0.0; EQ20_BAND_COUNT], 48_000);
        assert!(eq.is_bypass());
        let mut buf = [0.1, -0.2, 0.3, -0.4, 0.5, 0.9, -1.0];
        let orig = buf;
        eq.process(&mut buf);
        assert!(
            buf.iter()
                .zip(orig)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "flat 20-band EQ must not alter samples"
        );
    }

    #[test]
    fn nonflat_20_band_eq_is_active_and_stable() {
        let mut gains = [0.0f32; EQ20_BAND_COUNT];
        gains[5] = 9.0;
        gains[12] = -9.0;
        let mut eq = Equalizer::new_20(gains, 48_000);
        assert!(!eq.is_bypass());
        let mut buf: Vec<f32> = (0..2048).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        eq.process(&mut buf);
        assert!(
            buf.iter().all(|v| v.is_finite() && v.abs() < 8.0),
            "20-band EQ output must remain finite and bounded"
        );
    }

    #[test]
    fn band_boost_amplifies_its_center_frequency() {
        // A +12 dB boost on the 240 Hz band must raise the steady-state energy
        // of a 240 Hz tone relative to a flat EQ. (Peaking filters boost at the
        // band center, not at DC — hence a tone, not a constant.)
        let flat = settled_rms(240.0, [0.0; BAND_COUNT]);
        let boosted = settled_rms(240.0, [0.0, 12.0, 0.0, 0.0, 0.0]);
        assert!(
            boosted > flat * 1.5,
            "240 Hz boost should amplify a 240 Hz tone: flat={flat}, boosted={boosted}"
        );
    }
}
