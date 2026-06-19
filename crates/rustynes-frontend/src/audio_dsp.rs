//! v1.7.0 "Forge" Workstream H3 — frontend stereo output DSP.
//!
//! The NES APU mixes to a single mono channel, and the deterministic core hands
//! the frontend one mono sample stream (`Nes::drain_audio_into`). Everything in
//! this module is a **frontend, output-only** stage applied in the real-time
//! cpal callback *after* the mono sample has been popped from the lock-free
//! queue — exactly like the master-gain and EQ stages, it never touches the
//! core synthesis, so the determinism contract (save-state / TAS / netplay /
//! the `AccuracyCoin` audio oracle) is unaffected.
//!
//! Three stages, in order: a mono → stereo **pan** image, a small Schroeder
//! **reverb**, and a headphone **crossfeed**. Each defaults to a true bypass:
//!
//! - pan center (0.0) → equal L/R, the mono value duplicated bit-for-bit;
//! - reverb mix 0.0 → dry passthrough;
//! - crossfeed 0.0 → channels untouched.
//!
//! With all three at their defaults [`StereoStage::is_bypass`] is `true` and the
//! caller skips the stage entirely, so the output is the **byte-identical**
//! mono-duplicated-to-stereo stream `RustyNES` produced before this workstream
//! (see `docs/adr/0020`).
//!
//! Per-APU-channel pan: the config carries a pan position per APU channel
//! (Pulse1/Pulse2/Triangle/Noise/DMC/expansion), but because the core hands the
//! frontend a single *pre-mixed* mono master (splitting it would require core
//! changes, deferred to the v2.0 every-cycle rewrite), the active stage applies
//! the **average** of the enabled channels' pans as one master image. The
//! per-channel surface is forward-compatible: when the master pan is center
//! (every channel at 0.0, the default) the image is bit-exact identity.

// Audio DSP: the textbook reverb/pan math reads best in the direct form; the
// FMA-vs-separate-ops rounding difference is inaudible and the buffer-length
// casts (delay-line lengths from a positive ms × rate) are exact and positive
// for any real device period.
#![allow(
    clippy::suboptimal_flops,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

/// Number of pan positions (one per APU mixer channel: Pulse1, Pulse2,
/// Triangle, Noise, DMC, expansion).
pub const PAN_COUNT: usize = 6;

/// `sqrt(2)` — scales the equal-power center-pan gain back to unity so a center
/// pan reproduces the mono value bit-for-bit.
const SQRT2: f32 = core::f32::consts::SQRT_2;

/// Equal-power pan gains for a mono sample at pan position `p` in `-1.0..=1.0`
/// (`-1` = hard left, `0` = center, `+1` = hard right). At center this returns
/// `(SQRT_1_2, SQRT_1_2)`; the caller scales by `SQRT_2` so center is unity per
/// channel (the mono value duplicated, not attenuated).
#[inline]
fn pan_gains(p: f32) -> (f32, f32) {
    // Map -1..1 to an angle 0..PI/2; cos/sin give the constant-power law.
    let p = p.clamp(-1.0, 1.0);
    let theta = (p + 1.0) * 0.25 * core::f32::consts::PI;
    (theta.cos(), theta.sin())
}

/// A simple comb filter (one delay line + feedback) — a Schroeder reverb tap.
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    feedback: f32,
}

impl Comb {
    fn new(len: usize, feedback: f32) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            feedback,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.buf[self.idx];
        self.buf[self.idx] = x + y * self.feedback;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        y
    }
}

/// A Schroeder all-pass filter (smears the comb output's metallic ring).
struct AllPass {
    buf: Vec<f32>,
    idx: usize,
    gain: f32,
}

impl AllPass {
    fn new(len: usize, gain: f32) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            gain,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let buffered = self.buf[self.idx];
        let y = -x + buffered;
        self.buf[self.idx] = x + buffered * self.gain;
        self.idx += 1;
        if self.idx >= self.buf.len() {
            self.idx = 0;
        }
        y
    }
}

/// A classic 4-comb + 2-allpass Schroeder reverb over a mono send.
struct Reverb {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
}

impl Reverb {
    /// Build a reverb voiced for `sample_rate`. `room` (0..=1) scales the comb
    /// feedback (decay time); larger = longer tail.
    fn new(sample_rate: u32, room: f32) -> Self {
        let sr = sample_rate.max(1) as f32;
        // Schroeder's reference comb delays (ms), prime-ish to avoid flutter.
        let comb_ms = [29.7, 37.1, 41.1, 43.7];
        let allpass_ms = [5.0, 1.7];
        let feedback = 0.7 + 0.28 * room.clamp(0.0, 1.0);
        let combs = comb_ms
            .iter()
            .map(|&ms| Comb::new(((ms / 1000.0) * sr) as usize, feedback))
            .collect();
        let allpasses = allpass_ms
            .iter()
            .map(|&ms| AllPass::new(((ms / 1000.0) * sr) as usize, 0.5))
            .collect();
        Self { combs, allpasses }
    }

    /// One mono wet sample for one dry input sample.
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let mut acc = 0.0;
        for c in &mut self.combs {
            acc += c.process(x);
        }
        acc /= self.combs.len().max(1) as f32;
        for a in &mut self.allpasses {
            acc = a.process(acc);
        }
        acc
    }
}

/// The full frontend stereo output stage: pan image + reverb + crossfeed.
///
/// Owned by the cpal callback closure; live params are pushed in from the
/// Settings UI via [`StereoStage::set_params`] (the caller mirrors the shared
/// atomics into here once per callback). Built lazily so a pure-bypass config
/// allocates nothing.
pub struct StereoStage {
    sample_rate: u32,
    /// Master pan in `-1.0..=1.0` (the per-channel average; center = identity).
    pan: f32,
    /// Reverb wet mix `0.0..=1.0` (0 = dry/bypass).
    reverb_mix: f32,
    /// Reverb room size `0.0..=1.0`.
    reverb_room: f32,
    /// Headphone crossfeed amount `0.0..=1.0` (0 = bypass).
    crossfeed: f32,
    reverb: Option<Reverb>,
    /// Room the live `reverb` was built for (rebuild on change).
    built_room: f32,
}

impl StereoStage {
    /// New all-bypass stage (center pan, no reverb, no crossfeed) for
    /// `sample_rate`.
    #[must_use]
    pub const fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            pan: 0.0,
            reverb_mix: 0.0,
            reverb_room: 0.5,
            crossfeed: 0.0,
            reverb: None,
            built_room: -1.0,
        }
    }

    /// Live-update the params. `pans` is the per-APU-channel pan array; the
    /// master pan is the mean of the channels (so the default all-center array
    /// stays center). `reverb_mix` / `crossfeed` in `0.0..=1.0`.
    pub fn set_params(
        &mut self,
        pans: [f32; PAN_COUNT],
        reverb_mix: f32,
        reverb_room: f32,
        crossfeed: f32,
    ) {
        // NaN-guard each (config is deserialized unvalidated).
        let mean = pans
            .iter()
            .map(|p| if p.is_nan() { 0.0 } else { p.clamp(-1.0, 1.0) })
            .sum::<f32>()
            / PAN_COUNT as f32;
        self.pan = mean;
        self.reverb_mix = nan_to_zero(reverb_mix).clamp(0.0, 1.0);
        self.reverb_room = if reverb_room.is_nan() {
            0.5
        } else {
            reverb_room.clamp(0.0, 1.0)
        };
        self.crossfeed = nan_to_zero(crossfeed).clamp(0.0, 1.0);
    }

    /// `true` when every stage is at its bypass setting, so the caller can skip
    /// the work and emit the mono value duplicated bit-for-bit.
    #[must_use]
    pub fn is_bypass(&self) -> bool {
        self.pan == 0.0 && self.reverb_mix == 0.0 && self.crossfeed == 0.0
    }

    /// Process one mono frame into a stereo `(left, right)` pair. Returns the
    /// mono value duplicated when [`Self::is_bypass`] (the caller normally
    /// checks `is_bypass` and skips this, but the duplicate is bit-exact too).
    #[inline]
    pub fn process(&mut self, mono: f32) -> (f32, f32) {
        if self.is_bypass() {
            return (mono, mono);
        }
        // Pan: constant-power, scaled so center == unity (mono duplicated).
        let (lg, rg) = pan_gains(self.pan);
        let mut l = mono * lg * SQRT2;
        let mut r = mono * rg * SQRT2;

        // Reverb (mono send summed equally into both channels).
        if self.reverb_mix > 0.0 {
            // Exact equality is intentional: `built_room` is a cache key set from
            // the same `reverb_room` value, so a bit-difference means a genuine
            // change requiring a rebuild.
            #[allow(clippy::float_cmp)]
            let stale = self.built_room != self.reverb_room;
            if self.reverb.is_none() || stale {
                self.reverb = Some(Reverb::new(self.sample_rate, self.reverb_room));
                self.built_room = self.reverb_room;
            }
            if let Some(rev) = self.reverb.as_mut() {
                let wet = rev.process(mono);
                let dry = 1.0 - self.reverb_mix;
                l = l * dry + wet * self.reverb_mix;
                r = r * dry + wet * self.reverb_mix;
            }
        }

        // Crossfeed: blend a fraction of each channel into the other (narrows
        // the hard-panned image for comfortable headphone listening).
        if self.crossfeed > 0.0 {
            let cf = self.crossfeed * 0.5;
            let (nl, nr) = (l * (1.0 - cf) + r * cf, r * (1.0 - cf) + l * cf);
            l = nl;
            r = nr;
        }
        (l, r)
    }
}

#[inline]
const fn nan_to_zero(x: f32) -> f32 {
    if x.is_nan() { 0.0 } else { x }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn default_stage_is_bypass_and_duplicates_mono() {
        let mut s = StereoStage::new(48_000);
        assert!(s.is_bypass());
        for &v in &[0.0f32, 0.1, -0.5, 1.0, -1.0, 0.333] {
            let (l, r) = s.process(v);
            // Bit-exact: bypass must reproduce the mono value on both channels.
            assert_eq!(l.to_bits(), v.to_bits());
            assert_eq!(r.to_bits(), v.to_bits());
        }
    }

    #[test]
    fn all_center_pans_stay_bypass() {
        let mut s = StereoStage::new(44_100);
        s.set_params([0.0; PAN_COUNT], 0.0, 0.5, 0.0);
        assert!(s.is_bypass());
        let (l, r) = s.process(0.42);
        assert_eq!(l.to_bits(), 0.42f32.to_bits());
        assert_eq!(r.to_bits(), 0.42f32.to_bits());
    }

    #[test]
    fn hard_left_pan_silences_right() {
        let mut s = StereoStage::new(48_000);
        s.set_params([-1.0; PAN_COUNT], 0.0, 0.5, 0.0);
        assert!(!s.is_bypass());
        let (l, r) = s.process(0.5);
        assert!(l > 0.5, "left should carry the energy: {l}");
        assert!(r.abs() < 1e-4, "right should be ~silent: {r}");
    }

    #[test]
    fn center_pan_with_unity_law_is_unity() {
        // A non-bypass config (reverb on) still pans center at unity: with the
        // reverb mix at 0 the pan alone must keep center == mono.
        let mut s = StereoStage::new(48_000);
        // crossfeed forces non-bypass but is symmetric; pan stays center.
        s.set_params([0.0; PAN_COUNT], 0.0, 0.5, 0.0001);
        let (l, r) = s.process(0.5);
        // Center pan * SQRT2 == unity, crossfeed of equal L==R is a no-op.
        assert!((l - 0.5).abs() < 1e-5, "l={l}");
        assert!((r - 0.5).abs() < 1e-5, "r={r}");
    }

    #[test]
    fn reverb_off_is_dry_passthrough() {
        let mut s = StereoStage::new(48_000);
        // Pan center, no crossfeed, reverb mix 0 -> bypass, dry.
        s.set_params([0.0; PAN_COUNT], 0.0, 0.8, 0.0);
        assert!(s.is_bypass());
        for &v in &[0.2f32, -0.7, 0.9] {
            let (l, r) = s.process(v);
            assert_eq!(l.to_bits(), v.to_bits());
            assert_eq!(r.to_bits(), v.to_bits());
        }
    }

    #[test]
    fn reverb_on_stays_finite_and_bounded() {
        let mut s = StereoStage::new(48_000);
        s.set_params([0.0; PAN_COUNT], 0.4, 0.7, 0.0);
        assert!(!s.is_bypass());
        let mut peak = 0.0f32;
        for i in 0..20_000 {
            let x = (i as f32 * 0.03).sin() * 0.4;
            let (l, r) = s.process(x);
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 4.0, "reverb must stay bounded: peak={peak}");
    }
}
