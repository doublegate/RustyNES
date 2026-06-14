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
//! Reference: Mesen2 `Utilities/Audio/Equalizer.h`; RBJ "Audio EQ Cookbook".

// Audio DSP: the FMA-vs-separate-ops rounding difference is inaudible, and the
// u32->f32 sample-rate cast is exact for any real device rate. Readability of the
// textbook biquad form wins here.
#![allow(clippy::suboptimal_flops, clippy::cast_precision_loss)]

use core::f32::consts::PI;

/// Center frequencies (Hz) of the five fixed bands.
pub const BAND_FREQS: [f32; 5] = [60.0, 240.0, 1_000.0, 3_800.0, 12_000.0];

/// Number of EQ bands.
pub const BAND_COUNT: usize = 5;

/// Per-band Q (bandwidth). A moderate value so adjacent bands overlap smoothly.
const BAND_Q: f32 = 0.9;

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

/// A cascaded 5-band peaking equalizer over a mono host-rate stream.
pub struct Equalizer {
    bands: [Biquad; BAND_COUNT],
    /// `true` when every band is 0 dB (the stage is a pure passthrough).
    bypass: bool,
}

impl Equalizer {
    /// Build an equalizer for `sample_rate` with per-band gains in dB.
    #[must_use]
    pub fn new(gains_db: [f32; BAND_COUNT], sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let bands =
            core::array::from_fn(|i| Biquad::peaking(BAND_FREQS[i], gains_db[i], BAND_Q, sr));
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
