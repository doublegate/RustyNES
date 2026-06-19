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
//! the **average of the configured pan array** (all [`PAN_COUNT`] slots,
//! unconditionally — there is no enabled/mute mask plumbed in at this stage) as
//! one master image over the mono master. The per-channel surface is
//! forward-compatible: when the master pan is center (every channel at 0.0, the
//! default) the image is bit-exact identity.

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

    /// Update the feedback coefficient in place (no reallocation). Only the
    /// feedback depends on room size; the delay-line length is fixed, so this is
    /// real-time-safe to call from the audio callback.
    #[inline]
    const fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback;
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
        // Canonical Schroeder all-pass, transfer function
        //   H(z) = (-g + z^-D) / (1 - g·z^-D).
        // Single-delay difference equations (g = feedback gain, D = delay len):
        //   v[n] = x[n] + g·v[n-D]      (delayed value read first)
        //   y[n] = -g·v[n] + v[n-D]
        // then push v[n] into the delay line. This yields a unit-magnitude
        // (flat) response for |g| < 1; g = 0 collapses to a pure delay
        // (y[n] = v[n-D] = x[n-D]).
        let v_delayed = self.buf[self.idx];
        let v = x + self.gain * v_delayed;
        let y = -self.gain * v + v_delayed;
        self.buf[self.idx] = v;
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
        let feedback = Self::room_feedback(room);
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

    /// Map a room size `0..=1` to a comb feedback coefficient (decay time).
    #[inline]
    fn room_feedback(room: f32) -> f32 {
        0.7 + 0.28 * room.clamp(0.0, 1.0)
    }

    /// Re-voice the reverb for a new room size **in place** — only the comb
    /// feedback coefficients change; the delay-line lengths are fixed, so no
    /// allocation occurs. This is the real-time-safe path called from the audio
    /// callback when the room-size parameter moves (the all-pass smear stays
    /// fixed, as in the reference Schroeder topology).
    #[inline]
    fn set_room(&mut self, room: f32) {
        let feedback = Self::room_feedback(room);
        for c in &mut self.combs {
            c.set_feedback(feedback);
        }
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
/// atomics into here once per callback).
///
/// The reverb (and its comb/all-pass delay lines) is allocated **once** up
/// front in [`StereoStage::new`], so nothing on the real-time `process` path
/// ever allocates: a room-size change only re-voices the fixed-length combs in
/// place (`Reverb::set_room`). The bypass path still emits the mono value
/// duplicated bit-for-bit, so a pure-bypass config pays only the one-time
/// construction cost.
pub struct StereoStage {
    /// Master pan in `-1.0..=1.0` (the per-channel average; center = identity).
    pan: f32,
    /// Reverb wet mix `0.0..=1.0` (0 = dry/bypass).
    reverb_mix: f32,
    /// Reverb room size `0.0..=1.0`.
    reverb_room: f32,
    /// Headphone crossfeed amount `0.0..=1.0` (0 = bypass).
    crossfeed: f32,
    /// Pre-allocated reverb (delay-line lengths fixed for `sample_rate`); only
    /// the comb feedback is re-voiced on a room change, never reallocated.
    reverb: Reverb,
    /// Room the live `reverb` is currently voiced for (re-voice on change).
    built_room: f32,
}

impl StereoStage {
    /// New all-bypass stage (center pan, no reverb, no crossfeed) for
    /// `sample_rate`. Allocates the reverb delay lines once here so the
    /// real-time `process` path never allocates.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let reverb_room = 0.5;
        Self {
            pan: 0.0,
            reverb_mix: 0.0,
            reverb_room,
            crossfeed: 0.0,
            reverb: Reverb::new(sample_rate, reverb_room),
            built_room: reverb_room,
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
        // Non-finite guard each (config is deserialized unvalidated): map any
        // NaN/Inf to the param's neutral value *before* clamping, so a corrupt
        // config can never poison the DSP state.
        let mean = pans
            .iter()
            .map(|&p| nan_to_zero(p).clamp(-1.0, 1.0))
            .sum::<f32>()
            / PAN_COUNT as f32;
        self.pan = mean;
        self.reverb_mix = nan_to_zero(reverb_mix).clamp(0.0, 1.0);
        // Reverb room's neutral default is 0.5 (not 0.0), so guard to that.
        self.reverb_room = nan_to_default(reverb_room, 0.5).clamp(0.0, 1.0);
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

        // Reverb (mono send summed equally into both channels). The reverb is
        // pre-allocated in `new`, so a room-size change only re-voices the comb
        // feedback in place — no allocation occurs on this real-time path.
        if self.reverb_mix > 0.0 {
            // Exact equality is intentional: `built_room` is a cache key set from
            // the same `reverb_room` value, so a bit-difference means a genuine
            // change requiring a re-voice.
            #[allow(clippy::float_cmp)]
            let stale = self.built_room != self.reverb_room;
            if stale {
                self.reverb.set_room(self.reverb_room);
                self.built_room = self.reverb_room;
            }
            let wet = self.reverb.process(mono);
            let dry = 1.0 - self.reverb_mix;
            l = l * dry + wet * self.reverb_mix;
            r = r * dry + wet * self.reverb_mix;
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

/// Map a non-finite input (NaN or ±Inf) to `default`, otherwise pass it
/// through. `f32::is_finite` is const-stable since Rust 1.83 (our MSRV is
/// 1.96), so this runs in `const` context and guards every config float before
/// it is clamped into range.
#[inline]
const fn nan_to_default(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

/// Non-finite → `0.0` (the neutral value for pan/mix/crossfeed); see
/// [`nan_to_default`].
#[inline]
const fn nan_to_zero(x: f32) -> f32 {
    nan_to_default(x, 0.0)
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

    #[test]
    fn reverb_room_change_does_not_reallocate() {
        // The reverb is pre-allocated once; a room-size change must only
        // re-voice the comb feedback in place. Assert the comb/all-pass buffer
        // capacities are invariant across the full room-size sweep, proving no
        // realloc happens on the real-time `process` path.
        let mut s = StereoStage::new(48_000);
        s.set_params([0.0; PAN_COUNT], 0.4, 0.0, 0.0);
        let _ = s.process(0.1); // build/voice path engaged.
        let comb_caps: Vec<usize> = s.reverb.combs.iter().map(|c| c.buf.capacity()).collect();
        let ap_caps: Vec<usize> = s
            .reverb
            .allpasses
            .iter()
            .map(|a| a.buf.capacity())
            .collect();
        for &room in &[0.0f32, 0.1, 0.5, 0.9, 1.0, 0.25] {
            s.set_params([0.0; PAN_COUNT], 0.4, room, 0.0);
            let _ = s.process(0.1);
            let new_comb: Vec<usize> = s.reverb.combs.iter().map(|c| c.buf.capacity()).collect();
            let new_ap: Vec<usize> = s
                .reverb
                .allpasses
                .iter()
                .map(|a| a.buf.capacity())
                .collect();
            assert_eq!(
                new_comb, comb_caps,
                "comb buffers reallocated at room={room}"
            );
            assert_eq!(
                new_ap, ap_caps,
                "all-pass buffers reallocated at room={room}"
            );
        }
    }

    #[test]
    fn allpass_zero_gain_is_pure_delay() {
        // With g = 0 the Schroeder all-pass collapses to y[n] = x[n-D].
        let len = 8;
        let mut ap = AllPass::new(len, 0.0);
        // Feed a unit impulse, then zeros; the impulse must reappear after D.
        let mut out = Vec::new();
        for n in 0..(len * 2) {
            let x = if n == 0 { 1.0 } else { 0.0 };
            out.push(ap.process(x));
        }
        for (n, &y) in out.iter().enumerate() {
            if n == len {
                assert!((y - 1.0).abs() < 1e-6, "delayed impulse at D: {y}");
            } else {
                assert!(
                    y.abs() < 1e-6,
                    "pure delay must be silent off-tap at n={n}: {y}"
                );
            }
        }
    }

    #[test]
    fn allpass_has_flat_magnitude_and_unit_energy() {
        // A lossless all-pass preserves energy: the output energy of an impulse
        // response equals the input energy (1.0). Also assert boundedness.
        let len = 11;
        let g = 0.6f32;
        let mut ap = AllPass::new(len, g);
        let mut energy = 0.0f32;
        let mut peak = 0.0f32;
        // Long enough for the IIR tail to decay below the energy tolerance.
        for n in 0..4000 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            let y = ap.process(x);
            assert!(y.is_finite());
            energy += y * y;
            peak = peak.max(y.abs());
        }
        // All-pass impulse-response energy == input impulse energy (Parseval:
        // |H(e^jw)| == 1 everywhere => sum(y^2) == sum(x^2) == 1).
        assert!(
            (energy - 1.0).abs() < 1e-3,
            "all-pass must be lossless: energy={energy}"
        );
        assert!(
            peak <= 1.0 + 1e-6,
            "all-pass output must stay bounded: peak={peak}"
        );
    }
}
