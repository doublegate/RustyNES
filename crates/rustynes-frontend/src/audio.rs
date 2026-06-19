//! CPAL audio output stream + lock-free sample queue + dynamic rate control.
//!
//! The frontend's audio architecture (per `docs/frontend.md`, reworked in
//! v2.8.0 Phase 1):
//!
//! - The emulator runs on the main thread and produces samples once per
//!   video frame via [`rustynes_core::Nes::drain_audio_into`] at the device's
//!   nominal rate (the core's output is part of the determinism contract
//!   and never depends on wall-clock feedback).
//! - [`AudioOutput::push_samples`] routes them through a 4-tap Hermite
//!   resampler whose ratio is nudged ±0.5% by the queue occupancy
//!   (dynamic rate control — see [`crate::resampler`]), so the buffered
//!   latency tracks a target instead of drifting into underruns (silence
//!   gaps) or overruns (dropped-sample pops).
//! - CPAL's real-time callback consumes from a [`SampleQueue`] — a
//!   hand-rolled **lock-free SPSC ring** (power-of-two capacity, atomic
//!   f32-bit slots, acquire/release head/tail). The callback is
//!   allocation-free and never blocks (the v1.x `Mutex<VecDeque>` +
//!   per-callback `vec![]` are gone).
//! - **Start-gating**: the callback plays silence (without consuming) until
//!   the queue holds the full latency target, then starts; on a true
//!   underrun it re-gates and refills before resuming (Mesen2's
//!   start/resync discipline) — no startup-crackle spiral.
//!
//! The producer side is the [`SampleQueue`] (cloneable `Arc`); the consumer
//! side is owned by the audio callback closure. Single-producer /
//! single-consumer **by convention**: the app's produce path is the only
//! pusher and the CPAL callback the only popper.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio_dsp::{PAN_COUNT, StereoStage};
use crate::eq::{BAND_COUNT, EQ20_BAND_COUNT, Equalizer};
use crate::resampler::{HermiteResampler, drc_ratio};

/// Default queue capacity in samples when no latency target is supplied
/// (= the pre-v2.8.0 soft cap; ~341 ms @ 48 kHz).
const DEFAULT_CAPACITY: usize = 16_384;

/// How far above the latency target the occupancy may drift before the
/// producer hard-resyncs by skipping incoming batches (Mesen2 uses the same
/// 50 ms band for its stop+refill resync).
const RESYNC_EXCESS_MS: u32 = 50;

/// Errors from audio init.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// No default output device available.
    #[error("no default audio output device")]
    NoDevice,
    /// Device offered no supported configurations.
    #[error("no supported audio output config")]
    NoConfig,
    /// Underlying CPAL error.
    #[error("cpal: {0}")]
    Cpal(String),
}

/// Lock-free single-producer/single-consumer ring of f32 samples stored as
/// atomic bit patterns. Capacity is a power of two; head/tail are
/// monotonically increasing (wrapping) indices masked into the slot array.
struct Ring {
    slots: Box<[AtomicU64]>,
    mask: usize,
}

// Each slot packs one f32's bits into the low half of an AtomicU64; using
// 64-bit slots keeps the array uniform on 32-bit wasm targets too. (A plain
// AtomicU32 would also work; the cost difference is noise at 16 K slots.)
impl Ring {
    fn new(capacity_pow2: usize) -> Self {
        debug_assert!(capacity_pow2.is_power_of_two());
        let slots = (0..capacity_pow2)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            mask: capacity_pow2 - 1,
        }
    }

    const fn capacity(&self) -> usize {
        self.mask + 1
    }
}

/// Shared queue state.
struct QueueInner {
    ring: Ring,
    /// Consumer (read) index — advanced by the audio callback only.
    head: AtomicUsize,
    /// Producer (write) index — advanced by the app's produce path only.
    tail: AtomicUsize,
    /// Total samples dropped by `push` because the ring was full (overrun —
    /// the producer ran ahead of the DAC clock) or skipped by a hard
    /// resync.
    overrun_dropped: AtomicU64,
    /// Total callback fills that came up short and padded silence (underrun
    /// — the DAC clock ran ahead of the producer). Counted per short fill,
    /// not per sample. Only counted once the producer has pushed at least
    /// once (`started`), so the idle no-ROM state doesn't tick the counter.
    underruns: AtomicU64,
    /// Set by the first `push`; gates underrun counting.
    started: AtomicBool,
    /// v1.7.1 — explicit frontend-pause flag (sticky). When set, the cpal
    /// callback plays silence WITHOUT consuming the ring AND without running
    /// the `avail >= start_threshold` auto-reopen, so the buffered tail is
    /// preserved across a pause instead of drained. Independent of `playing`:
    /// in steady-state native playback the ring is typically full at the
    /// instant of a pause (`avail >= start_threshold`), so without this flag
    /// the next callback would immediately re-open the start-gate and keep
    /// draining the tail — defeating the pause gate (#152 review). Cleared on
    /// resume so the normal threshold-gated startup resumes with the tail
    /// intact (zero underrun on resume).
    paused: AtomicBool,
    /// Start-gate: the callback plays silence (without consuming) until the
    /// queue holds at least `start_threshold` samples, then flips this on;
    /// a true underrun flips it back off so playback resumes only after the
    /// buffer has been rebuilt (no crackle spiral).
    playing: AtomicBool,
    /// Samples required before playback (un)gates. 0 disables the gate
    /// (bare `SampleQueue::new()`, unit tests).
    start_threshold: AtomicUsize,
    /// v1.0.0 — master output gain (f32 bits, like the slot encoding).
    /// Applied at the single cpal consume point in [`Self::pop_or_silence`]
    /// (post-resampler, lock-free, affecting the buffered tail too). Default
    /// 1.0 = today's sound exactly; 0.0 = muted. The core still produces
    /// byte-identical samples — gain is an output-only multiply.
    gain: AtomicU64,
    /// v1.0.0 — the emulation-speed factor the audio DRC band centers on
    /// (f32 bits). 1.0 (default) is the classic band; the speed presets set
    /// it to the speed so the resampler consumes `speed`x input per output
    /// (no overrun at alt speed, natural pitch shift). Read by the producer's
    /// `push_samples` each call so the shared queue carries the setting across
    /// the winit/emu-thread boundary.
    base_ratio: AtomicU64,
    /// v1.1.0 beta.2 (T-110-D2) — graphic-EQ params, shared so the Settings UI
    /// (winit thread) can live-update them and the producer (which owns the
    /// stateful biquads) picks the change up. `eq_gen` bumps on every change;
    /// the producer rebuilds its `Equalizer` when it sees a new generation.
    eq_gen: AtomicU64,
    /// EQ enabled flag (when false the producer skips the stage entirely).
    eq_enabled: AtomicBool,
    /// Per-band gains in dB (f32 bits), five fixed bands.
    eq_bands: [AtomicU32; BAND_COUNT],
    /// v1.7.0 H3 — `true` selects the 20-band graphic EQ (using `eq_bands_20`);
    /// `false` (default) keeps the classic 5-band voicing (`eq_bands`). Shared
    /// so the Settings UI can switch modes live.
    eq_20_band: AtomicBool,
    /// v1.7.0 H3 — per-band gains in dB (f32 bits) for the 20-band graphic EQ.
    eq_bands_20: [AtomicU32; EQ20_BAND_COUNT],
    /// v1.7.0 H3 — stereo output DSP params, shared so the Settings UI (winit
    /// thread) live-updates them and the cpal callback (which owns the stateful
    /// reverb) picks the change up. `stereo_gen` bumps on every change.
    stereo_gen: AtomicU64,
    /// Per-APU-channel pan positions (`-1.0..=1.0` f32 bits); default all 0.0.
    pan: [AtomicU32; PAN_COUNT],
    /// Reverb wet mix `0.0..=1.0` (f32 bits); default 0.0 (dry/bypass).
    reverb_mix: AtomicU32,
    /// Reverb room size `0.0..=1.0` (f32 bits); default 0.5.
    reverb_room: AtomicU32,
    /// Headphone crossfeed `0.0..=1.0` (f32 bits); default 0.0 (bypass).
    crossfeed: AtomicU32,
}

/// Thread-safe sample queue between the emulator and the CPAL callback.
///
/// Cloneable handle over the shared SPSC ring; see the module docs for the
/// producer/consumer convention.
#[derive(Clone)]
pub struct SampleQueue {
    inner: Arc<QueueInner>,
}

impl SampleQueue {
    /// New empty queue with the default capacity and the start-gate
    /// disabled.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// New empty queue with at least `capacity` slots (rounded up to a
    /// power of two), start-gate disabled.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(2);
        Self {
            inner: Arc::new(QueueInner {
                ring: Ring::new(cap),
                head: AtomicUsize::new(0),
                tail: AtomicUsize::new(0),
                overrun_dropped: AtomicU64::new(0),
                underruns: AtomicU64::new(0),
                started: AtomicBool::new(false),
                paused: AtomicBool::new(false),
                playing: AtomicBool::new(false),
                start_threshold: AtomicUsize::new(0),
                gain: AtomicU64::new(u64::from(1.0f32.to_bits())),
                base_ratio: AtomicU64::new(u64::from(1.0f32.to_bits())),
                eq_gen: AtomicU64::new(0),
                eq_enabled: AtomicBool::new(false),
                eq_bands: core::array::from_fn(|_| AtomicU32::new(0)),
                eq_20_band: AtomicBool::new(false),
                eq_bands_20: core::array::from_fn(|_| AtomicU32::new(0)),
                stereo_gen: AtomicU64::new(0),
                pan: core::array::from_fn(|_| AtomicU32::new(0)),
                reverb_mix: AtomicU32::new(0),
                reverb_room: AtomicU32::new(0.5f32.to_bits()),
                crossfeed: AtomicU32::new(0),
            }),
        }
    }

    /// Arm the start-gate: playback waits until `samples` are buffered
    /// (clamped to half the ring so the gate can always open).
    pub fn set_start_threshold(&self, samples: usize) {
        let cap = self.inner.ring.capacity();
        self.inner
            .start_threshold
            .store(samples.min(cap / 2), Ordering::Relaxed);
    }

    /// v1.0.0 — set the master output gain (clamped to `0.0..=1.0`). Applied
    /// once per cpal callback in [`Self::pop_or_silence`]. Lock-free and live:
    /// the Settings slider calls this from the winit thread.
    pub fn set_gain(&self, gain: f32) {
        self.inner
            .gain
            .store(u64::from(gain.clamp(0.0, 1.0).to_bits()), Ordering::Relaxed);
    }

    /// v1.0.0 — the current master output gain.
    #[must_use]
    #[allow(dead_code)] // read in tests + as a UI mirror.
    pub fn gain(&self) -> f32 {
        #[allow(clippy::cast_possible_truncation)] // low 32 bits hold the f32.
        f32::from_bits(self.inner.gain.load(Ordering::Relaxed) as u32)
    }

    /// v1.0.0 — set the emulation-speed factor the DRC band centers on. Read
    /// by the producer's `push_samples`. Lock-free + live across the
    /// winit/emu-thread boundary (the producer owns the resampler).
    pub fn set_base_ratio(&self, base: f32) {
        self.inner
            .base_ratio
            .store(u64::from(base.to_bits()), Ordering::Relaxed);
    }

    /// v1.1.0 beta.2 — set the 5-band graphic-EQ params (enabled + per-band dB
    /// gains). Lock-free + live: the Settings UI calls this from the winit thread
    /// and the producer rebuilds its biquads on the next push. Bumps the
    /// generation. Selects the classic 5-band voicing.
    pub fn set_eq(&self, enabled: bool, bands: [f32; BAND_COUNT]) {
        store_db_bands(&self.inner.eq_bands, &bands);
        self.inner.eq_20_band.store(false, Ordering::Relaxed);
        self.inner.eq_enabled.store(enabled, Ordering::Relaxed);
        self.inner.eq_gen.fetch_add(1, Ordering::Release);
    }

    /// v1.7.0 H3 — set the full EQ params: `enabled`, whether the 20-band graphic
    /// EQ is active (`use_20_band`), and both band arrays. The producer reads the
    /// active array based on the mode flag. Lock-free + live (bumps the
    /// generation). Off / flat → byte-identical to a no-EQ build.
    pub fn set_eq_full(
        &self,
        enabled: bool,
        use_20_band: bool,
        bands_5: [f32; BAND_COUNT],
        bands_20: [f32; EQ20_BAND_COUNT],
    ) {
        store_db_bands(&self.inner.eq_bands, &bands_5);
        store_db_bands(&self.inner.eq_bands_20, &bands_20);
        self.inner.eq_20_band.store(use_20_band, Ordering::Relaxed);
        self.inner.eq_enabled.store(enabled, Ordering::Relaxed);
        self.inner.eq_gen.fetch_add(1, Ordering::Release);
    }

    /// v1.7.0 H3 — set the stereo output DSP params (per-channel pan, reverb
    /// mix/room, crossfeed). Lock-free + live: the Settings UI calls this from
    /// the winit thread; the cpal callback rebuilds its reverb on the next
    /// generation. All-center / 0 mix / 0 crossfeed (the default) is bypass.
    pub fn set_stereo(
        &self,
        pans: [f32; PAN_COUNT],
        reverb_mix: f32,
        reverb_room: f32,
        crossfeed: f32,
    ) {
        for (slot, &p) in self.inner.pan.iter().zip(pans.iter()) {
            let p = if p.is_nan() { 0.0 } else { p.clamp(-1.0, 1.0) };
            slot.store(p.to_bits(), Ordering::Relaxed);
        }
        let store01 = |slot: &AtomicU32, v: f32| {
            let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            slot.store(v.to_bits(), Ordering::Relaxed);
        };
        store01(&self.inner.reverb_mix, reverb_mix);
        // room defaults to 0.5, not 0.0, but a NaN still maps to a safe value.
        let room = if reverb_room.is_nan() {
            0.5
        } else {
            reverb_room.clamp(0.0, 1.0)
        };
        self.inner
            .reverb_room
            .store(room.to_bits(), Ordering::Relaxed);
        store01(&self.inner.crossfeed, crossfeed);
        self.inner.stereo_gen.fetch_add(1, Ordering::Release);
    }

    /// v1.1.0 beta.2 — current EQ generation counter (the producer compares this
    /// to the last value it built from to decide whether to rebuild).
    fn eq_gen(&self) -> u64 {
        self.inner.eq_gen.load(Ordering::Acquire)
    }

    /// v1.1.0 beta.2 / v1.7.0 H3 — snapshot the EQ params:
    /// `(enabled, use_20_band, 5-band gains, 20-band gains)`.
    fn eq_params(&self) -> (bool, bool, [f32; BAND_COUNT], [f32; EQ20_BAND_COUNT]) {
        let enabled = self.inner.eq_enabled.load(Ordering::Relaxed);
        let use_20 = self.inner.eq_20_band.load(Ordering::Relaxed);
        let bands = core::array::from_fn(|i| {
            f32::from_bits(self.inner.eq_bands[i].load(Ordering::Relaxed))
        });
        let bands_20 = core::array::from_fn(|i| {
            f32::from_bits(self.inner.eq_bands_20[i].load(Ordering::Relaxed))
        });
        (enabled, use_20, bands, bands_20)
    }

    /// v1.7.0 H3 — current stereo-DSP generation counter (the cpal callback
    /// compares this to the last value it built from).
    fn stereo_gen(&self) -> u64 {
        self.inner.stereo_gen.load(Ordering::Acquire)
    }

    /// v1.7.0 H3 — snapshot the stereo-DSP params:
    /// `(per-channel pans, reverb_mix, reverb_room, crossfeed)`.
    fn stereo_params(&self) -> ([f32; PAN_COUNT], f32, f32, f32) {
        let pans =
            core::array::from_fn(|i| f32::from_bits(self.inner.pan[i].load(Ordering::Relaxed)));
        let mix = f32::from_bits(self.inner.reverb_mix.load(Ordering::Relaxed));
        let room = f32::from_bits(self.inner.reverb_room.load(Ordering::Relaxed));
        let cf = f32::from_bits(self.inner.crossfeed.load(Ordering::Relaxed));
        (pans, mix, room, cf)
    }

    /// v1.0.0 — the current DRC base-ratio (emulation-speed) factor.
    #[must_use]
    fn base_ratio(&self) -> f64 {
        #[allow(clippy::cast_possible_truncation)] // low 32 bits hold the f32.
        f64::from(f32::from_bits(
            self.inner.base_ratio.load(Ordering::Relaxed) as u32,
        ))
    }

    /// Push samples produced by the emulator. Samples that don't fit in the
    /// ring are dropped (counted as overrun) — with DRC active the ring
    /// never approaches full in steady state.
    pub fn push(&self, samples: &[f32]) {
        self.inner.started.store(true, Ordering::Relaxed);
        let tail = self.inner.tail.load(Ordering::Relaxed);
        let head = self.inner.head.load(Ordering::Acquire);
        let free = self.inner.ring.capacity() - tail.wrapping_sub(head);
        let n = samples.len().min(free);
        for (i, &s) in samples[..n].iter().enumerate() {
            self.inner.ring.slots[tail.wrapping_add(i) & self.inner.ring.mask]
                .store(u64::from(s.to_bits()), Ordering::Relaxed);
        }
        self.inner
            .tail
            .store(tail.wrapping_add(n), Ordering::Release);
        let dropped = samples.len() - n;
        if dropped > 0 {
            self.inner
                .overrun_dropped
                .fetch_add(dropped as u64, Ordering::Relaxed);
        }
    }

    /// Pop samples into `out`, returning the number written. Slots beyond
    /// what the queue had are filled with silence.
    ///
    /// Start-gate semantics: before `start_threshold` samples have been
    /// buffered, outputs pure silence WITHOUT consuming (the buffer keeps
    /// building); a short fill while playing counts one underrun and
    /// re-gates so playback resumes only after the buffer refills.
    pub fn pop_or_silence(&self, out: &mut [f32]) -> usize {
        let head = self.inner.head.load(Ordering::Relaxed);
        let tail = self.inner.tail.load(Ordering::Acquire);
        let avail = tail.wrapping_sub(head);

        // v1.7.1 (#152 review) — an explicit frontend pause takes precedence
        // over the `playing`/`start_threshold` startup-buffering logic: output
        // silence WITHOUT consuming the ring and WITHOUT running the
        // `avail >= start_threshold` auto-reopen, so the buffered tail is
        // preserved (not drained) while paused. In steady-state native playback
        // the ring is typically full at the instant of a pause, so `avail`
        // already meets the threshold — without this short-circuit the next
        // callback would re-open the start-gate and keep draining the tail.
        if self.inner.paused.load(Ordering::Relaxed) {
            out.fill(0.0);
            return 0;
        }

        if !self.inner.playing.load(Ordering::Relaxed) {
            let threshold = self.inner.start_threshold.load(Ordering::Relaxed);
            if avail >= threshold && (avail > 0 || threshold == 0) {
                self.inner.playing.store(true, Ordering::Relaxed);
            } else {
                out.fill(0.0);
                return 0;
            }
        }

        // v1.0.0 — read the master gain ONCE per callback (a per-sample
        // reload would be a needless atomic load in the real-time path).
        #[allow(clippy::cast_possible_truncation)] // low 32 bits hold the f32.
        let gain = f32::from_bits(self.inner.gain.load(Ordering::Relaxed) as u32);
        let n = out.len().min(avail);
        for (i, o) in out[..n].iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)] // low 32 bits hold the f32.
            let bits = self.inner.ring.slots[head.wrapping_add(i) & self.inner.ring.mask]
                .load(Ordering::Relaxed) as u32;
            *o = f32::from_bits(bits) * gain;
        }
        self.inner
            .head
            .store(head.wrapping_add(n), Ordering::Release);
        if n < out.len() && !out.is_empty() {
            if self.inner.started.load(Ordering::Relaxed) {
                self.inner.underruns.fetch_add(1, Ordering::Relaxed);
            }
            // Re-gate: rebuild to the threshold before resuming so one
            // stall costs one clean gap, not a crackle spiral.
            self.inner.playing.store(false, Ordering::Relaxed);
        }
        for s in out.iter_mut().skip(n) {
            *s = 0.0;
        }
        n
    }

    /// Number of buffered samples (racy snapshot; informational).
    #[allow(dead_code)] // Used by tests + the Performance panel.
    pub fn len(&self) -> usize {
        let tail = self.inner.tail.load(Ordering::Acquire);
        let head = self.inner.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// True if the queue is empty.
    #[allow(dead_code)] // Used by tests + the Performance panel.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total samples dropped at the ring cap / by hard resync so far
    /// (overruns). v2.8.0 health counter for the Performance panel.
    #[allow(dead_code)] // wasm builds render the panel without native audio.
    pub fn overrun_dropped(&self) -> u64 {
        self.inner.overrun_dropped.load(Ordering::Relaxed)
    }

    /// Total short callback fills (underruns) so far. v2.8.0 health counter
    /// for the Performance panel.
    #[allow(dead_code)] // wasm builds render the panel without native audio.
    pub fn underruns(&self) -> u64 {
        self.inner.underruns.load(Ordering::Relaxed)
    }

    /// Record `n` producer-side skipped samples (hard resync) in the
    /// overrun counter.
    fn count_skipped(&self, n: usize) {
        self.inner
            .overrun_dropped
            .fetch_add(n as u64, Ordering::Relaxed);
    }

    /// v1.7.1 — engage the sticky pause gate for a frontend pause.
    ///
    /// While emulation is paused the producer stops pushing. The earlier fix
    /// only cleared `playing`, but [`Self::pop_or_silence`] re-opens the
    /// start-gate whenever `avail >= start_threshold`, and in steady-state
    /// native playback the ring is typically full at the instant of a pause —
    /// so the very next cpal callback re-opened the gate and kept *draining*
    /// the buffered tail, defeating the pause gate and still producing the
    /// underrun on resume that it was meant to prevent (#152 review).
    ///
    /// Setting an explicit, sticky `paused` flag makes the callback play
    /// silence *without* consuming the already-buffered tail and *without*
    /// running the threshold auto-reopen, so the ring is preserved rather than
    /// drained-then-starved: there is no short fill, no underrun, and on resume
    /// (see [`Self::resume_from_pause`]) the gate re-opens once the producer
    /// has refilled to the start threshold — with the tail intact. We also
    /// clear `playing` so resume always re-runs the clean threshold-gated
    /// startup. Lock-free and idempotent; a no-op effect when never pausing, so
    /// steady-state playback is byte-identical. Pause is a pure frontend
    /// concept (no core determinism surface).
    pub fn gate_for_pause(&self) {
        self.inner.paused.store(true, Ordering::Relaxed);
        self.inner.playing.store(false, Ordering::Relaxed);
    }

    /// v1.7.1 — clear the sticky pause gate on resume (see
    /// [`Self::gate_for_pause`]). With `paused` cleared the normal
    /// `playing`/`start_threshold` startup-buffering logic takes over again, so
    /// playback re-opens cleanly once the producer has refilled to the start
    /// threshold — with the buffered tail preserved across the pause (zero
    /// underrun on resume). Lock-free and idempotent.
    pub fn resume_from_pause(&self) {
        self.inner.paused.store(false, Ordering::Relaxed);
    }
}

impl Default for SampleQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// v1.7.0 H3 — store a dB-gain band array into shared atomic slots, NaN-guarding
/// and clamping each to `-12..=12` (config.toml is deserialized unvalidated, and
/// `f32::clamp` panics on a NaN argument). The slot array and the gain slice are
/// the same length by construction at each call site.
fn store_db_bands(slots: &[AtomicU32], gains: &[f32]) {
    for (slot, &g) in slots.iter().zip(gains.iter()) {
        let g = if g.is_nan() {
            0.0
        } else {
            g.clamp(-12.0, 12.0)
        };
        slot.store(g.to_bits(), Ordering::Relaxed);
    }
}

/// v1.1.0 beta.2 (T-110-D2) — the producer-side graphic-EQ stage. Owns the
/// stateful biquads; rebuilds them from the shared queue params whenever the
/// Settings UI bumps the EQ generation. Bypassed (zero overhead) when the EQ is
/// disabled or flat, so audio is byte-identical to a no-EQ build by default.
struct EqStage {
    sample_rate: u32,
    eq: Option<Equalizer>,
    seen_gen: u64,
}

impl EqStage {
    const fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            eq: None,
            seen_gen: 0,
        }
    }

    /// Re-sync the EQ from the shared `queue` params if they changed, and
    /// report whether a filter is currently engaged. When this returns `false`
    /// the caller can skip the (copy +) `run` entirely — keeping the no-DRC
    /// path zero-copy and byte-identical with the EQ off.
    fn engaged(&mut self, queue: &SampleQueue) -> bool {
        let r#gen = queue.eq_gen();
        if r#gen != self.seen_gen {
            self.seen_gen = r#gen;
            let (enabled, use_20, bands, bands_20) = queue.eq_params();
            self.eq = enabled.then(|| {
                if use_20 {
                    Equalizer::new_20(bands_20, self.sample_rate)
                } else {
                    Equalizer::new(bands, self.sample_rate)
                }
            });
        }
        // A flat (all-zero-gain) EQ is a no-op in `Equalizer::process`; report it
        // as disengaged so callers skip the copy/resample work entirely.
        self.eq.as_ref().is_some_and(|e| !e.is_bypass())
    }

    /// Filter `buf` in place (call only after [`Self::engaged`] returned `true`).
    fn run(&mut self, buf: &mut [f32]) {
        if let Some(eq) = self.eq.as_mut() {
            eq.process(buf);
        }
    }
}

/// Owns the live CPAL stream (kept around so it isn't dropped), the
/// producer-side queue handle, and the v2.8.0 DRC resampler stage.
pub struct AudioOutput {
    /// Sample rate the device opened at.
    pub sample_rate: u32,
    /// Number of channels (we render mono, but duplicate to fill stereo).
    /// Informational; the duplication happens inside the audio callback.
    #[allow(dead_code)]
    pub channels: u16,
    /// Producer-side queue handle (push from the emulator thread).
    pub queue: SampleQueue,
    /// v2.8.0 Phase 1 — the DRC Hermite resampler stage. `None` when DRC is
    /// disabled in config (bit-exact passthrough).
    resampler: Option<HermiteResampler>,
    /// Scratch for the resampler output (reused; no per-frame alloc).
    resample_buf: Vec<f32>,
    /// Latency target in samples (the DRC equilibrium point and the
    /// start-gate threshold).
    latency_samples: usize,
    /// Occupancy above which the producer hard-resyncs (skips batches).
    resync_samples: usize,
    /// v1.1.0 beta.2 — the optional graphic-EQ output stage.
    eq_stage: EqStage,
    /// Live stream handle. Dropping it stops audio.
    _stream: cpal::Stream,
}

impl AudioOutput {
    /// Open the default output device with the pre-v2.8.0 defaults (device
    /// default rate, 60 ms latency target, DRC on). Kept for tests.
    #[allow(dead_code)]
    pub fn try_default() -> Result<Self, AudioError> {
        Self::try_new(None, 60, true, None)
    }

    /// v1.7.0 H3 — enumerate the names of the available output devices.
    /// The Settings device picker lists these alongside a "System default"
    /// entry. Returns an empty list when the host exposes none.
    #[must_use]
    pub fn output_device_names() -> Vec<String> {
        let host = cpal::default_host();
        // cpal 0.18: the device name is its `Display` form (`to_string()`); the
        // structured `description()` is richer but we only need the picker label.
        host.output_devices()
            .map(|it| it.map(|d: cpal::Device| d.to_string()).collect())
            .unwrap_or_default()
    }

    /// Open an output device.
    ///
    /// - `preferred_rate` — request this sample rate when the device
    ///   supports it (the previously dead `[audio] sample_rate` config);
    ///   `None` / unsupported falls back to the device default.
    /// - `latency_ms` — the buffered-audio target the DRC servo holds and
    ///   the start-gate waits for (clamped to 20..=250 ms).
    /// - `drc` — dynamic rate control on/off (off = bit-exact passthrough
    ///   of the core's samples to the DAC).
    /// - `device_name` — v1.7.0 H3: open the output device with this name;
    ///   `None` (the default) or an unmatched / now-absent name falls back to
    ///   the host's default device (graceful device-gone handling).
    ///
    /// # Errors
    ///
    /// [`AudioError`] when no device / no config / the stream fails to
    /// build.
    pub fn try_new(
        preferred_rate: Option<u32>,
        latency_ms: u32,
        drc: bool,
        device_name: Option<&str>,
    ) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        // v1.7.0 H3 — pick the named device when it is present; otherwise fall
        // back to the host default (covers `None` and a device that has since
        // been unplugged).
        let device = device_name
            .and_then(|want| {
                host.output_devices()
                    .ok()
                    .and_then(|mut it| it.find(|d: &cpal::Device| d.to_string() == want))
            })
            .or_else(|| host.default_output_device())
            .ok_or(AudioError::NoDevice)?;
        let default_cfg = device
            .default_output_config()
            .map_err(|e| AudioError::Cpal(e.to_string()))?;

        // Honor the configured sample rate when the device can do it with
        // the same channel count; otherwise keep the device default.
        // cpal 0.18: `SampleRate` is a `u32` alias (no longer a tuple struct).
        let mut sample_rate = default_cfg.sample_rate();
        let channels = default_cfg.channels();
        let format = default_cfg.sample_format();
        if let Some(want) = preferred_rate {
            let supported = device
                .supported_output_configs()
                .map_err(|e| AudioError::Cpal(e.to_string()))?
                .any(|range| {
                    range.channels() == channels
                        && range.sample_format() == format
                        && range.min_sample_rate() <= want
                        && want <= range.max_sample_rate()
                });
            if supported {
                sample_rate = want;
            } else {
                eprintln!(
                    "rustynes: audio device does not support {want} Hz \
                     (using device default {sample_rate} Hz)"
                );
            }
        }
        let config = cpal::StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let latency_ms = latency_ms.clamp(20, 250);
        let latency_samples = (sample_rate as usize * latency_ms as usize) / 1000;
        let resync_samples =
            latency_samples + (sample_rate as usize * RESYNC_EXCESS_MS as usize) / 1000;
        // Ring sized to 4x the latency target: the DRC holds occupancy at
        // 1x, the resync rule caps excursions at ~1.3x — full is unreachable
        // in steady state, so push never drops.
        let queue = SampleQueue::with_capacity(latency_samples * 4);
        queue.set_start_threshold(latency_samples);

        let stream = build_stream(&device, &config, format, queue.clone(), channels)?;
        stream.play().map_err(|e| AudioError::Cpal(e.to_string()))?;
        Ok(Self {
            sample_rate,
            channels,
            queue,
            resampler: drc.then(HermiteResampler::new),
            resample_buf: Vec::with_capacity(2048),
            latency_samples,
            resync_samples,
            eq_stage: EqStage::new(sample_rate),
            _stream: stream,
        })
    }

    /// v2.8.0 Phase 5 — build a `Send` producer half over this output's
    /// queue: the DRC resampler stage + the push/resync policy, detached
    /// from the (!Send) cpal stream so the emulation thread can own it.
    /// Multiple producers may exist (the emu thread's + the App-side one
    /// the netplay path uses); only one is ever active at a time, and each
    /// carries its own resampler warm-up (inaudible on switch).
    #[must_use]
    pub fn make_producer(&self, drc: bool) -> AudioProducer {
        AudioProducer {
            sample_rate: self.sample_rate,
            queue: self.queue.clone(),
            resampler: drc.then(HermiteResampler::new),
            resample_buf: Vec::with_capacity(2048),
            latency_samples: self.latency_samples,
            resync_samples: self.resync_samples,
            eq_stage: EqStage::new(self.sample_rate),
        }
    }

    /// Push one frame's worth of core samples through the DRC stage into
    /// the queue.
    ///
    /// With DRC enabled, the queue occupancy sets the resampler ratio per
    /// Near's law (`crate::resampler::drc_ratio`) so the buffered latency
    /// servos to the target; a stall that overshoots the resync band skips
    /// batches until occupancy returns (counted as overrun-dropped). With
    /// DRC off this is a straight push.
    pub fn push_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        // Hard resync: a long produce stall (debugger pause, window drag)
        // followed by catch-up can put far more than the target in flight.
        // Skipping whole batches converges in a handful of frames and is
        // ONE clean discontinuity instead of a crackle tail.
        if self.queue.len() > self.resync_samples {
            self.queue.count_skipped(samples.len());
            return;
        }
        match &mut self.resampler {
            Some(rs) => {
                // v1.0.0 — center the DRC band on the emulation-speed factor.
                rs.set_base_ratio(self.queue.base_ratio());
                #[allow(clippy::cast_precision_loss)] // sample counts << 2^52.
                let fill = self.queue.len() as f64 / (2.0 * self.latency_samples as f64);
                rs.set_ratio(drc_ratio(fill) * rs.base_ratio());
                self.resample_buf.clear();
                rs.process(samples, &mut self.resample_buf);
                // v1.1.0 beta.2 — optional EQ output stage (post-resampler).
                self.eq_stage.engaged(&self.queue);
                self.eq_stage.run(&mut self.resample_buf);
                self.queue.push(&self.resample_buf);
            }
            // DRC off: stay zero-copy unless the EQ is engaged.
            None if self.eq_stage.engaged(&self.queue) => {
                self.resample_buf.clear();
                self.resample_buf.extend_from_slice(samples);
                self.eq_stage.run(&mut self.resample_buf);
                self.queue.push(&self.resample_buf);
            }
            None => self.queue.push(samples),
        }
    }

    /// v1.5.0 "Lens" Workstream H8 — the live DRC servo ratio derived from the
    /// shared queue occupancy (Performance panel + perf-log readout).
    ///
    /// The DRC resampler itself lives in the detached emu-thread
    /// [`AudioProducer`], so the winit-side `AudioOutput` recomputes the ratio
    /// the producer would apply from the same lock-free queue occupancy +
    /// base-ratio inputs (`drc_ratio(len / 2·latency) · base_ratio`). This
    /// mirrors `push_samples` exactly, so the readout matches what the DAC
    /// hears. `1.0` when DRC is off.
    #[must_use]
    pub fn drc_ratio_now(&self) -> f64 {
        if self.resampler.is_none() {
            return 1.0;
        }
        let base = self.queue.base_ratio();
        #[allow(clippy::cast_precision_loss)] // sample counts << 2^52.
        let fill = self.queue.len() as f64 / (2.0 * self.latency_samples.max(1) as f64);
        drc_ratio(fill) * base
    }

    /// H8 — the latency target in milliseconds the DRC servos toward (the
    /// clamped `[audio] latency_ms` setpoint).
    #[must_use]
    pub fn latency_target_ms(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        {
            self.latency_samples as f32 * 1000.0 / self.sample_rate as f32
        }
    }

    /// The latency target in samples (Performance panel readout).
    #[allow(dead_code)]
    #[must_use]
    pub const fn latency_target_samples(&self) -> usize {
        self.latency_samples
    }
}

/// v2.8.0 Phase 5 — the `Send` producer half of an [`AudioOutput`].
///
/// The DRC resampler stage + the push/resync policy over the shared SPSC
/// queue, detached from the (!Send) cpal stream so the emulation thread
/// can own it. Built via [`AudioOutput::make_producer`].
pub struct AudioProducer {
    /// Sample rate the device opened at (the core synthesizes at this).
    pub sample_rate: u32,
    queue: SampleQueue,
    resampler: Option<HermiteResampler>,
    resample_buf: Vec<f32>,
    latency_samples: usize,
    resync_samples: usize,
    /// v1.1.0 beta.2 — the optional graphic-EQ output stage.
    eq_stage: EqStage,
}

impl AudioProducer {
    /// Push one frame's worth of core samples through the DRC stage into
    /// the queue (identical policy to [`AudioOutput::push_samples`]).
    pub fn push_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        if self.queue.len() > self.resync_samples {
            self.queue.count_skipped(samples.len());
            return;
        }
        match &mut self.resampler {
            Some(rs) => {
                // v1.0.0 — center the DRC band on the emulation-speed factor.
                rs.set_base_ratio(self.queue.base_ratio());
                #[allow(clippy::cast_precision_loss)] // sample counts << 2^52.
                let fill = self.queue.len() as f64 / (2.0 * self.latency_samples as f64);
                rs.set_ratio(drc_ratio(fill) * rs.base_ratio());
                self.resample_buf.clear();
                rs.process(samples, &mut self.resample_buf);
                // v1.1.0 beta.2 — optional EQ output stage (post-resampler).
                self.eq_stage.engaged(&self.queue);
                self.eq_stage.run(&mut self.resample_buf);
                self.queue.push(&self.resample_buf);
            }
            // DRC off: stay zero-copy unless the EQ is engaged.
            None if self.eq_stage.engaged(&self.queue) => {
                self.resample_buf.clear();
                self.resample_buf.extend_from_slice(samples);
                self.eq_stage.run(&mut self.resample_buf);
                self.queue.push(&self.resample_buf);
            }
            None => self.queue.push(samples),
        }
    }
}

/// v2.8.0 Phase 5 — the produce path's audio-sink abstraction.
///
/// The synchronous (winit-thread) drive feeds the `!Send` [`AudioOutput`]
/// directly, while the emulation thread feeds its owned `Send`
/// [`AudioProducer`]. Identical push policy either way (both delegate to
/// their inherent `push_samples`).
pub trait AudioSink {
    /// Device sample rate (the core synthesizes at this).
    fn sample_rate(&self) -> u32;
    /// Push one frame's worth of core samples (through the DRC stage).
    fn push_samples(&mut self, samples: &[f32]);
}

impl AudioSink for AudioOutput {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn push_samples(&mut self, samples: &[f32]) {
        Self::push_samples(self, samples);
    }
}

impl AudioSink for AudioProducer {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn push_samples(&mut self, samples: &[f32]) {
        Self::push_samples(self, samples);
    }
}

/// Build a CPAL stream that pulls from `queue` into the device's native
/// sample format. `f32` and `i16`/`u16` are supported (the three CPAL
/// publishes via the `Sample` trait).
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    queue: SampleQueue,
    channels: u16,
) -> Result<cpal::Stream, AudioError> {
    let err_fn = |e| eprintln!("cpal stream error: {e}");
    let chans = usize::from(channels.max(1));
    // Reused mono scratch buffer — the real-time callback must not allocate
    // (v2.8.0 Phase 1; the old `vec![0.0; frames]` per callback is gone).
    let mut mono: Vec<f32> = Vec::new();
    // v1.7.0 H3 — the stateful stereo output stage (pan / reverb / crossfeed),
    // owned by the callback. Built at the stream's sample rate; live params are
    // pulled from the shared queue once per callback when its generation moves.
    // NOTE: in cpal 0.18 `cpal::SampleRate` is a plain `u32` type alias (not the
    // `SampleRate(u32)` newtype of older releases), so `config.sample_rate` is
    // already the integer Hz here — no `.0` unwrap, and the reverb/EQ filter
    // center-frequency math receives true Hz directly.
    let sr = config.sample_rate;
    let mut stereo = StereoStage::new(sr);
    let mut seen_stereo_gen = u64::MAX; // force a first sync.
    match format {
        SampleFormat::F32 => device
            .build_output_stream(
                *config,
                move |data: &mut [f32], _| {
                    fill::<f32>(
                        data,
                        &queue,
                        chans,
                        &mut mono,
                        &mut stereo,
                        &mut seen_stereo_gen,
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Cpal(e.to_string())),
        SampleFormat::I16 => device
            .build_output_stream(
                *config,
                move |data: &mut [i16], _| {
                    fill::<i16>(
                        data,
                        &queue,
                        chans,
                        &mut mono,
                        &mut stereo,
                        &mut seen_stereo_gen,
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Cpal(e.to_string())),
        SampleFormat::U16 => device
            .build_output_stream(
                *config,
                move |data: &mut [u16], _| {
                    fill::<u16>(
                        data,
                        &queue,
                        chans,
                        &mut mono,
                        &mut stereo,
                        &mut seen_stereo_gen,
                    );
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Cpal(e.to_string())),
        _ => Err(AudioError::NoConfig),
    }
}

/// Drain the queue into the device's interleaved output buffer. With the stereo
/// DSP stage at its defaults (center pan, no reverb, no crossfeed) the mono
/// sample is replicated to every channel **bit-for-bit** — byte-identical to
/// the pre-v1.7.0 mono-duplicated output. When the stage is engaged it widens
/// the mono master into a stereo (L/R) image written to the first two channels
/// (with extra channels fed the L/R average) (v1.7.0 H3).
///
/// `mono` is the closure-owned reusable scratch (grown once to the device
/// period, then stable — no allocation in steady state). `stereo` is the
/// stateful pan/reverb/crossfeed stage; `seen_stereo_gen` tracks the shared
/// param generation so the stage re-syncs only when the UI changes a setting.
fn fill<S: cpal::SizedSample + cpal::FromSample<f32>>(
    data: &mut [S],
    queue: &SampleQueue,
    channels: usize,
    mono: &mut Vec<f32>,
    stereo: &mut StereoStage,
    seen_stereo_gen: &mut u64,
) {
    let frames = data.len() / channels.max(1);
    mono.resize(frames, 0.0);
    queue.pop_or_silence(mono);

    // Re-sync the stereo stage from the shared params only when they changed.
    let r#gen = queue.stereo_gen();
    if r#gen != *seen_stereo_gen {
        *seen_stereo_gen = r#gen;
        let (pans, mix, room, cf) = queue.stereo_params();
        stereo.set_params(pans, mix, room, cf);
    }

    // Bypass fast path: bit-exact mono duplication (today's output exactly), and
    // mono devices (1 channel) never need the stereo image regardless.
    if channels < 2 || stereo.is_bypass() {
        for (frame_idx, sample) in mono.iter().enumerate() {
            for c in 0..channels {
                let out_idx = frame_idx * channels + c;
                if out_idx < data.len() {
                    data[out_idx] = S::from_sample(*sample);
                }
            }
        }
        return;
    }

    // Engaged: widen each mono frame into a stereo (L/R) pair.
    for (frame_idx, &sample) in mono.iter().enumerate() {
        let (l, r) = stereo.process(sample);
        let base = frame_idx * channels;
        for c in 0..channels {
            let out_idx = base + c;
            if out_idx >= data.len() {
                break;
            }
            // Channel 0 = L, channel 1 = R; any extra channels get the centre
            // (L/R average) so surround layouts stay coherent.
            let v = match c {
                0 => l,
                1 => r,
                _ => 0.5 * (l + r),
            };
            data[out_idx] = S::from_sample(v);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::redundant_clone
)]
mod tests {
    use super::*;

    #[test]
    fn push_then_pop_returns_samples_in_order() {
        let q = SampleQueue::new();
        q.push(&[0.1, 0.2, 0.3, 0.4]);
        let mut out = [0.0f32; 4];
        let n = q.pop_or_silence(&mut out);
        assert_eq!(n, 4);
        // Compare each sample exactly — no arithmetic was performed.
        for (got, want) in out.iter().zip([0.1, 0.2, 0.3, 0.4]) {
            assert_eq!(*got, want);
        }
        assert!(q.is_empty());
    }

    #[test]
    fn pop_into_larger_buffer_fills_remainder_with_silence() {
        let q = SampleQueue::new();
        q.push(&[0.5, 0.5]);
        let mut out = [9.0f32; 5];
        let n = q.pop_or_silence(&mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0], 0.5);
        assert_eq!(out[1], 0.5);
        assert_eq!(out[2], 0.0);
        assert_eq!(out[3], 0.0);
        assert_eq!(out[4], 0.0);
        // The short fill after a real push counts as one underrun.
        assert_eq!(q.underruns(), 1);
    }

    #[test]
    fn pop_from_empty_queue_yields_full_silence() {
        let q = SampleQueue::new();
        let mut out = [9.0f32; 8];
        let n = q.pop_or_silence(&mut out);
        assert_eq!(n, 0);
        assert!(out.iter().all(|&s| s == 0.0));
        // Nothing was ever pushed — the idle state must not count
        // underruns.
        assert_eq!(q.underruns(), 0);
    }

    #[test]
    fn ring_full_drops_newest_and_counts_overrun() {
        let q = SampleQueue::with_capacity(64);
        let big: Vec<f32> = (0..200).map(|i| i as f32).collect();
        q.push(&big);
        assert_eq!(q.len(), 64);
        assert_eq!(q.overrun_dropped(), 200 - 64);
        // SPSC drop-newest: the OLDEST samples are retained.
        let mut out = vec![0.0f32; 64];
        q.pop_or_silence(&mut out);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[63], 63.0);
    }

    #[test]
    fn start_gate_holds_silence_until_threshold_then_plays() {
        let q = SampleQueue::with_capacity(256);
        q.set_start_threshold(8);
        q.push(&[1.0; 4]);
        let mut out = [9.0f32; 4];
        // Below threshold: silence, nothing consumed.
        assert_eq!(q.pop_or_silence(&mut out), 0);
        assert!(out.iter().all(|&s| s == 0.0));
        assert_eq!(q.len(), 4);
        // Reaching the threshold opens the gate.
        q.push(&[2.0; 4]);
        assert_eq!(q.pop_or_silence(&mut out), 4);
        assert!(out.iter().all(|&s| s == 1.0));
    }

    #[test]
    fn underrun_regates_until_buffer_rebuilt() {
        let q = SampleQueue::with_capacity(256);
        q.set_start_threshold(8);
        q.push(&[1.0; 8]);
        let mut out = [0.0f32; 8];
        assert_eq!(q.pop_or_silence(&mut out), 8); // gate opened, drained
        let mut small = [9.0f32; 2];
        // Empty while playing -> short fill -> underrun + re-gate.
        assert_eq!(q.pop_or_silence(&mut small), 0);
        assert_eq!(q.underruns(), 1);
        // Now below threshold again: silence without consuming.
        q.push(&[3.0; 4]);
        assert_eq!(q.pop_or_silence(&mut small), 0);
        assert_eq!(q.len(), 4);
        // Refilled to threshold: plays again.
        q.push(&[3.0; 4]);
        assert_eq!(q.pop_or_silence(&mut small), 2);
        assert_eq!(small[0], 3.0);
    }

    #[test]
    fn gain_scales_output_and_defaults_to_unity() {
        let q = SampleQueue::new();
        // Default gain is 1.0 — output is byte-identical to the input.
        assert_eq!(q.gain(), 1.0);
        q.push(&[0.5, -0.25, 1.0]);
        let mut out = [0.0f32; 3];
        assert_eq!(q.pop_or_silence(&mut out), 3);
        assert_eq!(out, [0.5, -0.25, 1.0]);
        // Half gain halves every sample.
        q.set_gain(0.5);
        q.push(&[0.5, -0.25, 1.0]);
        assert_eq!(q.pop_or_silence(&mut out), 3);
        assert_eq!(out, [0.25, -0.125, 0.5]);
        // Muted = 0.0 (and clamps above 1.0 / below 0.0).
        q.set_gain(0.0);
        q.push(&[0.5, -0.25, 1.0]);
        assert_eq!(q.pop_or_silence(&mut out), 3);
        assert_eq!(out, [0.0, 0.0, 0.0]);
        q.set_gain(5.0);
        assert_eq!(q.gain(), 1.0);
        q.set_gain(-3.0);
        assert_eq!(q.gain(), 0.0);
    }

    #[test]
    fn gate_for_pause_holds_buffer_and_avoids_underrun() {
        // v1.7.1 (#152 review) — the sticky pause gate must take precedence over
        // the `avail >= start_threshold` auto-reopen. This reproduces the exact
        // steady-state condition the reviewers flagged: the ring is FULLER than
        // the start threshold at the instant of the pause, so a non-sticky
        // `playing = false` would be immediately re-opened by `pop_or_silence`
        // and keep draining the buffered tail. With the sticky `paused` flag the
        // callback returns silence WITHOUT consuming and WITHOUT counting an
        // underrun; occupancy is unchanged across repeated paused callbacks.
        let q = SampleQueue::with_capacity(256);
        q.set_start_threshold(8);
        q.push(&[1.0; 16]); // 16 buffered; threshold is 8
        let mut out = [0.0f32; 4];
        // Gate opens, plays normally; 12 remain, still >= threshold (8).
        assert_eq!(q.pop_or_silence(&mut out), 4);
        assert!(out.iter().all(|&s| s == 1.0));
        assert_eq!(q.underruns(), 0);
        let buffered = q.len();
        assert!(
            buffered >= 8,
            "precondition: avail ({buffered}) >= start_threshold (8) at pause time"
        );

        // Pause: engage the sticky gate. Several callbacks while paused must each
        // return silence and consume NOTHING — even though avail >= threshold the
        // whole time (the defeat the reviewers found). Occupancy is invariant.
        q.gate_for_pause();
        let mut paused_out = [9.0f32; 4];
        for _ in 0..3 {
            assert_eq!(
                q.pop_or_silence(&mut paused_out),
                0,
                "paused callback must not consume"
            );
            assert!(
                paused_out.iter().all(|&s| s == 0.0),
                "paused callback must output silence"
            );
            assert_eq!(
                q.len(),
                buffered,
                "buffered tail preserved unchanged across paused callbacks"
            );
        }
        assert_eq!(q.underruns(), 0, "pause must not count an underrun");

        // Resume: clear the sticky gate. The buffered tail is intact, so the
        // normal threshold-gated startup re-opens immediately (avail >=
        // threshold) and plays the PRESERVED samples — zero underrun on resume.
        q.resume_from_pause();
        assert_eq!(q.pop_or_silence(&mut out), 4);
        assert!(
            out.iter().all(|&s| s == 1.0),
            "resume plays the preserved buffered tail"
        );
        assert_eq!(q.underruns(), 0, "clean pause/resume = zero underruns");
    }

    #[test]
    fn base_ratio_defaults_to_unity_and_round_trips() {
        let q = SampleQueue::new();
        assert_eq!(q.base_ratio(), 1.0);
        q.set_base_ratio(2.0);
        assert_eq!(q.base_ratio(), 2.0);
        q.set_base_ratio(0.5);
        assert_eq!(q.base_ratio(), 0.5);
    }

    #[test]
    fn cloned_queue_shares_state() {
        let q1 = SampleQueue::new();
        let q2 = q1.clone();
        q1.push(&[1.0, 2.0]);
        assert_eq!(q2.len(), 2);
        let mut out = [0.0; 2];
        q2.pop_or_silence(&mut out);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], 2.0);
        assert!(q1.is_empty());
    }

    #[test]
    fn wrapping_indices_stream_many_times_capacity() {
        // Stream 8x the capacity through a small ring in interleaved
        // push/pop chunks and verify sample-exact FIFO order across the
        // index wrap.
        let q = SampleQueue::with_capacity(64);
        let mut next_in = 0u32;
        let mut next_out = 0u32;
        let mut out = [0.0f32; 16];
        while next_out < 512 {
            let batch: Vec<f32> = (0..16).map(|i| (next_in + i) as f32).collect();
            next_in += 16;
            q.push(&batch);
            let n = q.pop_or_silence(&mut out);
            for &got in &out[..n] {
                assert_eq!(got, next_out as f32);
                next_out += 1;
            }
        }
    }
}
