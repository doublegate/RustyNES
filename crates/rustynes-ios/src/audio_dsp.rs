//! iOS host audio-depth DSP (v1.9.9 "Workshop").
//!
//! A **host, output-only** stereo-enrichment stage applied in the CoreAudio
//! callback *after* the mono APU master is drained from the core
//! (`NesController.drain_audio`) — exactly like the desktop frontend's
//! `audio_dsp` / `eq` stages. It never touches the deterministic core synthesis,
//! so the determinism contract (save-state / TAS / netplay / the `AccuracyCoin`
//! audio oracle) is preserved.
//!
//! Four stages, in order: a 5-band peaking **equalizer**, a mono -> stereo
//! **pan** image, a small Schroeder **reverb**, and a headphone **crossfeed**.
//! Each defaults to a true bypass:
//!
//! - every EQ band at 0 dB -> identity;
//! - pan center (0.0) -> equal L/R, the mono value duplicated bit-for-bit;
//! - reverb mix 0.0 -> dry passthrough;
//! - crossfeed 0.0 -> channels untouched.
//!
//! With everything at its default (or the whole stage disabled) the `AudioDepth`
//! stage is in **bypass**: its `process` returns the mono value duplicated
//! bit-for-bit on both channels, so the output is byte-identical to a build
//! without this module. The DSP math is ported from the desktop frontend's
//! `audio_dsp.rs` (pan / reverb / crossfeed) and `eq.rs` (RBJ peaking biquads).
//!
//! This module is host-safe (pure `f32` math, no Metal / CoreAudio / cpal
//! dependency), so it compiles and is unit-tested on the workspace host build;
//! `audio.rs` (iOS-only) wires it into the real-time sink and `ffi.rs` exposes
//! the live config setter to Swift.

// Audio DSP: the textbook biquad/reverb/pan forms read best directly; the
// FMA-vs-separate-ops rounding difference is inaudible and the buffer-length
// casts (delay-line lengths from a positive ms x rate) are exact and positive
// for any real device period.
#![allow(
    clippy::suboptimal_flops,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use core::f32::consts::PI;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Number of equalizer bands (the classic 5-band voicing).
pub const EQ_BAND_COUNT: usize = 5;

/// Number of pan positions (one per APU mixer channel).
///
/// The slots are Pulse1, Pulse2, Triangle, Noise, DMC, expansion. The active
/// stage applies the mean as a single master image over the pre-mixed mono
/// master (the core hands the host a single mono stream), so the per-channel
/// surface is forward-compatible: an all-center array is bit-exact identity.
pub const PAN_COUNT: usize = 6;

/// Center frequencies (Hz) of the five fixed EQ bands.
const BAND_FREQS: [f32; EQ_BAND_COUNT] = [60.0, 240.0, 1_000.0, 3_800.0, 12_000.0];

/// Per-band Q (bandwidth) — moderate so adjacent bands overlap smoothly.
const BAND_Q: f32 = 0.9;

/// `sqrt(2)` — scales the equal-power center-pan gain back to unity so a center
/// pan reproduces the mono value bit-for-bit.
const SQRT2: f32 = core::f32::consts::SQRT_2;

/// A plain, `Copy` snapshot of the audio-depth configuration. The neutral
/// default ([`DepthConfig::BYPASS`]) is a true bypass on every stage.
#[derive(Debug, Clone, Copy)]
pub struct DepthConfig {
    /// Master enable. When `false` the whole stage is bypassed regardless of the
    /// other fields (the mono value is duplicated bit-for-bit).
    pub enabled: bool,
    /// Per-band EQ gains in dB (`-12.0..=12.0`); all-zero = flat = bypass.
    pub eq_db: [f32; EQ_BAND_COUNT],
    /// Per-APU-channel pan in `-1.0..=1.0` (`-1` hard left, `0` center, `+1`
    /// hard right). The active stage uses the mean; all-center = identity.
    pub pan: [f32; PAN_COUNT],
    /// Reverb wet mix `0.0..=1.0` (0 = dry / bypass).
    pub reverb_mix: f32,
    /// Reverb room size `0.0..=1.0` (decay time).
    pub reverb_room: f32,
    /// Headphone crossfeed amount `0.0..=1.0` (0 = bypass).
    pub crossfeed: f32,
}

impl DepthConfig {
    /// The all-bypass configuration (disabled, flat EQ, center pan, no reverb /
    /// crossfeed). Reverb room neutral default is `0.5`.
    pub const BYPASS: Self = Self {
        enabled: false,
        eq_db: [0.0; EQ_BAND_COUNT],
        pan: [0.0; PAN_COUNT],
        reverb_mix: 0.0,
        reverb_room: 0.5,
        crossfeed: 0.0,
    };
}

impl Default for DepthConfig {
    fn default() -> Self {
        Self::BYPASS
    }
}

/// A lock-free, atomically-shared mailbox for the live [`DepthConfig`].
///
/// The FFI thread writes via [`DepthParams::store`]; the real-time audio
/// callback reads via [`DepthParams::snapshot`] once per buffer and applies the
/// result. Floats are stored as their bit pattern in `AtomicU32`. There is no
/// cross-field tearing concern: the callback re-derives its filter state from
/// the snapshot, and a one-buffer-stale field only delays a slider by one
/// callback (inaudible).
#[derive(Debug)]
pub struct DepthParams {
    enabled: AtomicBool,
    eq_db: [AtomicU32; EQ_BAND_COUNT],
    pan: [AtomicU32; PAN_COUNT],
    reverb_mix: AtomicU32,
    reverb_room: AtomicU32,
    crossfeed: AtomicU32,
}

impl Default for DepthParams {
    fn default() -> Self {
        Self::new()
    }
}

impl DepthParams {
    /// A new mailbox holding the all-bypass configuration.
    #[must_use]
    pub fn new() -> Self {
        let cfg = DepthConfig::BYPASS;
        Self {
            enabled: AtomicBool::new(cfg.enabled),
            eq_db: core::array::from_fn(|i| AtomicU32::new(cfg.eq_db[i].to_bits())),
            pan: core::array::from_fn(|i| AtomicU32::new(cfg.pan[i].to_bits())),
            reverb_mix: AtomicU32::new(cfg.reverb_mix.to_bits()),
            reverb_room: AtomicU32::new(cfg.reverb_room.to_bits()),
            crossfeed: AtomicU32::new(cfg.crossfeed.to_bits()),
        }
    }

    /// Publish a new configuration (called off the audio thread).
    pub fn store(&self, cfg: &DepthConfig) {
        self.enabled.store(cfg.enabled, Ordering::Relaxed);
        for (slot, &v) in self.eq_db.iter().zip(cfg.eq_db.iter()) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        for (slot, &v) in self.pan.iter().zip(cfg.pan.iter()) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        self.reverb_mix
            .store(cfg.reverb_mix.to_bits(), Ordering::Relaxed);
        self.reverb_room
            .store(cfg.reverb_room.to_bits(), Ordering::Relaxed);
        self.crossfeed
            .store(cfg.crossfeed.to_bits(), Ordering::Relaxed);
    }

    /// Read the current configuration (called once per audio callback buffer).
    #[must_use]
    pub fn snapshot(&self) -> DepthConfig {
        DepthConfig {
            enabled: self.enabled.load(Ordering::Relaxed),
            eq_db: core::array::from_fn(|i| f32::from_bits(self.eq_db[i].load(Ordering::Relaxed))),
            pan: core::array::from_fn(|i| f32::from_bits(self.pan[i].load(Ordering::Relaxed))),
            reverb_mix: f32::from_bits(self.reverb_mix.load(Ordering::Relaxed)),
            reverb_room: f32::from_bits(self.reverb_room.load(Ordering::Relaxed)),
            crossfeed: f32::from_bits(self.crossfeed.load(Ordering::Relaxed)),
        }
    }
}

/// A single Direct-Form-I RBJ peaking biquad (one EQ band).
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
    /// RBJ peaking-EQ coefficients for `freq` (Hz) at `gain_db` and
    /// `sample_rate`. Falls back to identity for a band at / above Nyquist or a
    /// non-finite sample rate (so a corrupt device rate cannot inject NaNs).
    fn peaking(freq: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        if !sample_rate.is_finite() || freq >= sample_rate * 0.5 {
            return Self {
                b0: 1.0,
                ..Self::default()
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
    fn new(sample_rate: u32, room: f32) -> Self {
        let sr = sample_rate.max(1) as f32;
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

    #[inline]
    fn room_feedback(room: f32) -> f32 {
        0.7 + 0.28 * room.clamp(0.0, 1.0)
    }

    /// Re-voice for a new room size in place — only the comb feedback changes;
    /// the delay-line lengths are fixed, so no allocation occurs (real-time-safe).
    #[inline]
    fn set_room(&mut self, room: f32) {
        let feedback = Self::room_feedback(room);
        for c in &mut self.combs {
            c.set_feedback(feedback);
        }
    }

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

/// The full host audio-depth processor: 5-band EQ -> pan -> reverb -> crossfeed.
///
/// The reverb (and its comb / all-pass delay lines) is allocated **once** in
/// [`AudioDepth::new`]; nothing on the real-time `process` path allocates (a
/// room-size change only re-voices the fixed combs in place). [`AudioDepth::apply`]
/// mirrors a live [`DepthConfig`] in once per callback. When bypassed, `process`
/// returns the mono value duplicated bit-for-bit on both channels.
pub struct AudioDepth {
    enabled: bool,
    eq: [Biquad; EQ_BAND_COUNT],
    eq_bypass: bool,
    built_eq_db: [f32; EQ_BAND_COUNT],
    sample_rate: u32,
    pan: f32,
    reverb_mix: f32,
    reverb_room: f32,
    crossfeed: f32,
    reverb: Reverb,
    built_room: f32,
}

impl AudioDepth {
    /// A new all-bypass processor for `sample_rate`. Allocates the reverb delay
    /// lines once here so the real-time `process` path never allocates.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let cfg = DepthConfig::BYPASS;
        let sr = sample_rate.max(1);
        Self {
            enabled: cfg.enabled,
            eq: Self::build_eq(cfg.eq_db, sr),
            eq_bypass: true,
            built_eq_db: cfg.eq_db,
            sample_rate: sr,
            pan: 0.0,
            reverb_mix: cfg.reverb_mix,
            reverb_room: cfg.reverb_room,
            crossfeed: cfg.crossfeed,
            reverb: Reverb::new(sr, cfg.reverb_room),
            built_room: cfg.reverb_room,
        }
    }

    fn build_eq(gains_db: [f32; EQ_BAND_COUNT], sample_rate: u32) -> [Biquad; EQ_BAND_COUNT] {
        let sr = sample_rate as f32;
        core::array::from_fn(|i| {
            Biquad::peaking(BAND_FREQS[i], nan_to_zero(gains_db[i]), BAND_Q, sr)
        })
    }

    /// Mirror a live configuration in. Re-voices the EQ only when a band gain
    /// actually changes (resetting the biquad history), and the reverb only when
    /// the room size changes (feedback only, no realloc). NaN / Inf config values
    /// are mapped to their neutral defaults before clamping, so a corrupt config
    /// can never poison the DSP state.
    pub fn apply(&mut self, cfg: &DepthConfig) {
        self.enabled = cfg.enabled;

        let eq_db: [f32; EQ_BAND_COUNT] =
            core::array::from_fn(|i| nan_to_zero(cfg.eq_db[i]).clamp(-12.0, 12.0));
        // Exact comparison is intentional: `built_eq_db` is the cache key set
        // from the same source values, so a bit-difference is a genuine change.
        #[allow(clippy::float_cmp)]
        let eq_changed = eq_db != self.built_eq_db;
        if eq_changed {
            self.eq = Self::build_eq(eq_db, self.sample_rate);
            self.built_eq_db = eq_db;
        }
        self.eq_bypass = eq_db.iter().all(|&g| g.abs() < f32::EPSILON);

        // Master pan is the mean of the per-channel pans (all-center => center).
        let mean = cfg
            .pan
            .iter()
            .map(|&p| nan_to_zero(p).clamp(-1.0, 1.0))
            .sum::<f32>()
            / PAN_COUNT as f32;
        self.pan = mean;
        self.reverb_mix = nan_to_zero(cfg.reverb_mix).clamp(0.0, 1.0);
        self.reverb_room = nan_to_default(cfg.reverb_room, 0.5).clamp(0.0, 1.0);
        self.crossfeed = nan_to_zero(cfg.crossfeed).clamp(0.0, 1.0);
    }

    /// `true` when the whole stage is at bypass (disabled, or flat EQ + center
    /// pan + no reverb + no crossfeed), so the caller can skip the work and emit
    /// the mono value duplicated bit-for-bit.
    #[must_use]
    pub fn is_bypass(&self) -> bool {
        !self.enabled
            || (self.eq_bypass
                && self.pan == 0.0
                && self.reverb_mix == 0.0
                && self.crossfeed == 0.0)
    }

    /// Process one mono frame into a stereo `(left, right)` pair. Returns the
    /// mono value duplicated when [`Self::is_bypass`] (bit-exact passthrough).
    #[inline]
    pub fn process(&mut self, mono: f32) -> (f32, f32) {
        if self.is_bypass() {
            return (mono, mono);
        }

        // EQ (mono, cascaded peaking biquads) before the stereo image.
        let mut m = mono;
        if !self.eq_bypass {
            for band in &mut self.eq {
                m = band.process(m);
            }
        }

        // Pan: constant-power, scaled so center == unity (mono duplicated).
        let (lg, rg) = pan_gains(self.pan);
        let mut l = m * lg * SQRT2;
        let mut r = m * rg * SQRT2;

        // Reverb (mono send summed equally into both channels).
        if self.reverb_mix > 0.0 {
            #[allow(clippy::float_cmp)]
            let stale = self.built_room != self.reverb_room;
            if stale {
                self.reverb.set_room(self.reverb_room);
                self.built_room = self.reverb_room;
            }
            let wet = self.reverb.process(m);
            let dry = 1.0 - self.reverb_mix;
            l = l * dry + wet * self.reverb_mix;
            r = r * dry + wet * self.reverb_mix;
        }

        // Crossfeed: blend a fraction of each channel into the other.
        if self.crossfeed > 0.0 {
            let cf = self.crossfeed * 0.5;
            let (nl, nr) = (l * (1.0 - cf) + r * cf, r * (1.0 - cf) + l * cf);
            l = nl;
            r = nr;
        }
        (l, r)
    }
}

/// Equal-power pan gains for a mono sample at pan position `p` in `-1.0..=1.0`.
/// At center this returns `(SQRT_1_2, SQRT_1_2)`; the caller scales by `SQRT_2`
/// so center is unity per channel.
#[inline]
fn pan_gains(p: f32) -> (f32, f32) {
    let p = p.clamp(-1.0, 1.0);
    let theta = (p + 1.0) * 0.25 * PI;
    (theta.cos(), theta.sin())
}

/// Map a non-finite input (NaN or +/-Inf) to `default`, otherwise pass through.
#[inline]
const fn nan_to_default(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

/// Non-finite -> `0.0` (the neutral value for EQ gain / pan / mix / crossfeed).
#[inline]
const fn nan_to_zero(x: f32) -> f32 {
    nan_to_default(x, 0.0)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn default_is_bypass_and_duplicates_mono() {
        let mut d = AudioDepth::new(48_000);
        assert!(d.is_bypass());
        for &v in &[0.0f32, 0.1, -0.5, 1.0, -1.0, 0.333] {
            let (l, r) = d.process(v);
            assert_eq!(l.to_bits(), v.to_bits());
            assert_eq!(r.to_bits(), v.to_bits());
        }
    }

    #[test]
    fn enabled_but_neutral_config_is_bypass() {
        let mut d = AudioDepth::new(44_100);
        d.apply(&DepthConfig {
            enabled: true,
            ..DepthConfig::BYPASS
        });
        assert!(d.is_bypass());
        let (l, r) = d.process(0.42);
        assert_eq!(l.to_bits(), 0.42f32.to_bits());
        assert_eq!(r.to_bits(), 0.42f32.to_bits());
    }

    #[test]
    fn disabled_overrides_active_stages() {
        let mut d = AudioDepth::new(48_000);
        // Active settings, but disabled -> still a bit-exact passthrough.
        d.apply(&DepthConfig {
            enabled: false,
            pan: [-1.0; PAN_COUNT],
            reverb_mix: 0.5,
            crossfeed: 0.5,
            eq_db: [6.0; EQ_BAND_COUNT],
            ..DepthConfig::BYPASS
        });
        assert!(d.is_bypass());
        let (l, r) = d.process(0.5);
        assert_eq!(l.to_bits(), 0.5f32.to_bits());
        assert_eq!(r.to_bits(), 0.5f32.to_bits());
    }

    #[test]
    fn hard_left_pan_silences_right() {
        let mut d = AudioDepth::new(48_000);
        d.apply(&DepthConfig {
            enabled: true,
            pan: [-1.0; PAN_COUNT],
            ..DepthConfig::BYPASS
        });
        assert!(!d.is_bypass());
        let (l, r) = d.process(0.5);
        assert!(l > 0.5, "left should carry the energy: {l}");
        assert!(r.abs() < 1e-4, "right should be ~silent: {r}");
    }

    #[test]
    fn eq_boost_makes_stage_active_and_stays_bounded() {
        let mut d = AudioDepth::new(48_000);
        d.apply(&DepthConfig {
            enabled: true,
            eq_db: [6.0, 0.0, -6.0, 0.0, 3.0],
            ..DepthConfig::BYPASS
        });
        assert!(!d.is_bypass());
        let mut peak = 0.0f32;
        for i in 0..4096 {
            let x = (i as f32 * 0.05).sin() * 0.5;
            let (l, r) = d.process(x);
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 8.0, "EQ output must stay bounded: {peak}");
    }

    #[test]
    fn reverb_stays_finite_and_bounded() {
        let mut d = AudioDepth::new(48_000);
        d.apply(&DepthConfig {
            enabled: true,
            reverb_mix: 0.4,
            reverb_room: 0.7,
            ..DepthConfig::BYPASS
        });
        assert!(!d.is_bypass());
        let mut peak = 0.0f32;
        for i in 0..20_000 {
            let x = (i as f32 * 0.03).sin() * 0.4;
            let (l, r) = d.process(x);
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
        }
        assert!(peak < 4.0, "reverb must stay bounded: {peak}");
    }

    #[test]
    fn params_round_trip_through_atomics() {
        let params = DepthParams::new();
        let cfg = DepthConfig {
            enabled: true,
            eq_db: [1.0, -2.0, 3.0, -4.0, 5.0],
            pan: [0.1, -0.2, 0.3, -0.4, 0.5, -0.6],
            reverb_mix: 0.25,
            reverb_room: 0.8,
            crossfeed: 0.15,
        };
        params.store(&cfg);
        let got = params.snapshot();
        assert!(got.enabled);
        assert_eq!(got.eq_db, cfg.eq_db);
        assert_eq!(got.pan, cfg.pan);
        assert_eq!(got.reverb_mix, cfg.reverb_mix);
        assert_eq!(got.reverb_room, cfg.reverb_room);
        assert_eq!(got.crossfeed, cfg.crossfeed);
    }

    #[test]
    fn corrupt_nan_config_cannot_poison_state() {
        let mut d = AudioDepth::new(48_000);
        d.apply(&DepthConfig {
            enabled: true,
            eq_db: [f32::NAN; EQ_BAND_COUNT],
            pan: [f32::INFINITY; PAN_COUNT],
            reverb_mix: f32::NAN,
            reverb_room: f32::NAN,
            crossfeed: f32::NAN,
        });
        // NaN EQ -> 0 dB (flat), Inf pan clamps to hard right, NaN mix/crossfeed
        // -> 0. Output must stay finite.
        let (l, r) = d.process(0.5);
        assert!(l.is_finite() && r.is_finite());
    }
}
