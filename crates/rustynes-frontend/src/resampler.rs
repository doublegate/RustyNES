//! v2.8.0 Phase 1 — 4-tap Hermite (Catmull-Rom) audio resampler with a
//! dynamic rate-control ratio.
//!
//! This is the frontend half of the canonical "video master + audio dynamic
//! rate control" architecture (Near's DRC article; Mesen2
//! `SoundResampler.cpp`; ares `ruby/audio/audio.cpp`): the emulator core
//! keeps synthesizing at the *nominal* device rate (byte-identical core —
//! the determinism contract requires the core's emitted samples never depend
//! on wall-clock feedback), and this stage stretches or squeezes the stream
//! by up to ±0.5% on its way into the output ring so the queue occupancy
//! tracks a latency target instead of drifting into underruns (silence gaps)
//! or overruns (dropped-sample pops). A ±0.5% pitch deviation is far below
//! audibility; a dropped buffer is not.
//!
//! The interpolation kernel is the classic 4-point Catmull-Rom Hermite
//! spline — the same shape Mesen2's `HermiteResampler` and ares's
//! `nall::DSP::Resampler::Cubic` use. At `ratio == 1.0` the output is the
//! input delayed by two samples (the kernel's center tap), so the bypass
//! path and the DRC path stay phase-comparable.

/// Maximum rate deviation the DRC may request (±0.5% — Near's `maxDelta`,
/// `RetroArch`'s `audio_rate_control_delta` default).
pub const MAX_DRC_DELTA: f64 = 0.005;

/// 4-tap Hermite resampler. `ratio` = input samples consumed per output
/// sample produced (`> 1` squeezes / drains the queue, `< 1` stretches /
/// fills it).
#[derive(Debug)]
pub struct HermiteResampler {
    /// Last four input samples, oldest first.
    hist: [f32; 4],
    /// Fractional read position within the current input step, in `[0, 1)`
    /// after each input sample is processed.
    frac: f64,
    /// Input-per-output step.
    ratio: f64,
}

impl Default for HermiteResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl HermiteResampler {
    /// New resampler at the neutral 1:1 ratio.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            hist: [0.0; 4],
            frac: 0.0,
            ratio: 1.0,
        }
    }

    /// Set the input-per-output ratio, clamped to the DRC band
    /// `[1 - MAX_DRC_DELTA, 1 + MAX_DRC_DELTA]`.
    pub fn set_ratio(&mut self, ratio: f64) {
        self.ratio = ratio.clamp(1.0 - MAX_DRC_DELTA, 1.0 + MAX_DRC_DELTA);
    }

    /// Current ratio (for the Performance panel readout).
    #[must_use]
    pub const fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Feed `input`, appending resampled output to `out`.
    ///
    /// Standard accumulator form: each input sample shifts into the 4-tap
    /// history; every output sample advances the fractional position by
    /// `ratio`; outputs are emitted while the position lies within the
    /// current step. Output length per call is `len/ratio ± 1`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &s in input {
            self.hist = [self.hist[1], self.hist[2], self.hist[3], s];
            // The float accumulator is the standard resampler form; ratio
            // is clamped to ~1.0, so the loop runs 0-2 iterations.
            #[allow(clippy::while_float)]
            while self.frac < 1.0 {
                #[allow(clippy::cast_possible_truncation)] // frac in [0,1).
                let t = self.frac as f32;
                out.push(hermite4(&self.hist, t));
                self.frac += self.ratio;
            }
            self.frac -= 1.0;
        }
    }
}

/// 4-point Catmull-Rom Hermite interpolation at `t` in `[0, 1)` between
/// `h[1]` and `h[2]`.
///
/// Deliberately plain mul/add (no `mul_add`): `f32::mul_add` lowers to a
/// slow libm software path on x86-64 targets without guaranteed FMA, and
/// this runs per output sample.
#[inline]
#[allow(clippy::suboptimal_flops)]
fn hermite4(h: &[f32; 4], t: f32) -> f32 {
    let c0 = h[1];
    let c1 = 0.5 * (h[2] - h[0]);
    let c2 = h[0] - 2.5 * h[1] + 2.0 * h[2] - 0.5 * h[3];
    let c3 = 0.5 * (h[3] - h[0]) + 1.5 * (h[1] - h[2]);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Near's buffer-fill proportional DRC law: `fill` in `[0, 1]` (0 = empty,
/// 0.5 = on target, 1 = full scale) maps to an input-frequency ratio in
/// `[1 - delta, 1 + delta]`.
///
/// At `fill > 0.5` the queue is running high, so the ratio exceeds 1 and the
/// resampler consumes input faster (fewer output samples → the queue
/// drains); at `fill < 0.5` it stretches. The caller computes `fill` as
/// `occupied / (2 * latency_target)` so the equilibrium sits exactly at the
/// latency target.
#[must_use]
pub fn drc_ratio(fill: f64) -> f64 {
    let fill = fill.clamp(0.0, 1.0);
    2.0f64.mul_add(fill * MAX_DRC_DELTA, 1.0 - MAX_DRC_DELTA)
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    #[test]
    fn drc_ratio_endpoints_and_center() {
        assert_eq!(drc_ratio(0.5), 1.0);
        assert_eq!(drc_ratio(0.0), 1.0 - MAX_DRC_DELTA);
        assert_eq!(drc_ratio(1.0), 1.0 + MAX_DRC_DELTA);
        // Out-of-range fills clamp.
        assert_eq!(drc_ratio(-3.0), 1.0 - MAX_DRC_DELTA);
        assert_eq!(drc_ratio(7.0), 1.0 + MAX_DRC_DELTA);
    }

    #[test]
    fn unity_ratio_reproduces_input_with_two_sample_delay() {
        let mut r = HermiteResampler::new();
        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut out = Vec::new();
        r.process(&input, &mut out);
        // One output per input at ratio 1.0.
        assert_eq!(out.len(), input.len());
        // At t == 0 the kernel returns the center tap h[1], i.e. the input
        // delayed by two samples (the first two outputs interpolate the
        // zeroed warm-up history).
        for (i, &o) in out.iter().enumerate().skip(2) {
            assert_eq!(o, input[i - 2], "sample {i}");
        }
    }

    #[test]
    fn squeeze_ratio_emits_fewer_samples_and_stretch_more() {
        let input = vec![0.25f32; 10_000];
        for (ratio, expect_fewer) in [(1.0 + MAX_DRC_DELTA, true), (1.0 - MAX_DRC_DELTA, false)] {
            let mut r = HermiteResampler::new();
            r.set_ratio(ratio);
            let mut out = Vec::new();
            r.process(&input, &mut out);
            // 10_000 / 1.005 ≈ 9950; 10_000 / 0.995 ≈ 10050.
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            #[allow(clippy::cast_sign_loss)]
            let expected = (input.len() as f64 / ratio) as usize;
            assert!(
                out.len().abs_diff(expected) <= 2,
                "ratio {ratio}: got {} expected ~{expected}",
                out.len()
            );
            assert_eq!(expect_fewer, out.len() < input.len());
        }
    }

    #[test]
    fn set_ratio_clamps_to_drc_band() {
        let mut r = HermiteResampler::new();
        r.set_ratio(2.0);
        assert_eq!(r.ratio(), 1.0 + MAX_DRC_DELTA);
        r.set_ratio(0.1);
        assert_eq!(r.ratio(), 1.0 - MAX_DRC_DELTA);
    }

    #[test]
    fn steady_dc_input_resamples_to_same_dc_level() {
        // Catmull-Rom reproduces constants exactly (partition of unity), so
        // a DC stream must come out at the same level for any ratio in the
        // band — i.e. DRC never changes loudness, only timing.
        let input = vec![0.5f32; 4_000];
        let mut r = HermiteResampler::new();
        r.set_ratio(1.003);
        let mut out = Vec::new();
        r.process(&input, &mut out);
        for &o in out.iter().skip(4) {
            assert!((o - 0.5).abs() < 1e-6);
        }
    }
}
