//! The host-testable half of the iOS audio sink (v2.7.4, frontend audit
//! IOS-02 / IOS-03 / IOS-09).
//!
//! Everything here is plain `core` + `std` math and atomics, so it compiles and
//! is unit-tested on the workspace host build, the way `audio_dsp` is. The
//! iOS-only `audio.rs` wires it to the cpal CoreAudio stream.
//!
//! - [`ring`](crate::audio_ring::ring) — the lock-free single-producer /
//!   single-consumer sample queue, moved here from `audio.rs` unchanged in its
//!   discipline, and given a batched
//!   [`RingRx::pop_into`](crate::audio_ring::RingRx::pop_into) (one index load
//!   and one index store per device buffer, not per sample; IOS-03), a fill
//!   level, and a start threshold, so playback starts from a filled buffer
//!   instead of underrunning its first callbacks. It is reached only through
//!   one [`RingTx`](crate::audio_ring::RingTx) and one
//!   [`RingRx`](crate::audio_ring::RingRx), neither `Clone`, both `&mut self`,
//!   so the single-producer / single-consumer rule its `unsafe` relies on is
//!   enforced by the types rather than asked of the caller.
//! - [`Producer`](crate::audio_ring::Producer) — dynamic rate control (IOS-02). The sink had none: the ring
//!   filled or drained at whatever rate the host clock and the audio clock
//!   drifted apart, and nothing pulled it back to a target. The producer runs a
//!   4-tap Hermite resampler whose ratio is steered by the ring's fill, the same
//!   law the desktop uses.
//! - [`fan_out`](crate::audio_ring::fan_out) — mono / stereo to the device's channel layout (IOS-09).
//!
//! The resampler is the same algorithm as `rustynes-frontend/src/resampler.rs`
//! (this project's own code, carrying no provenance header), copied because the
//! iOS crate cannot depend on the desktop frontend. The two should change
//! together. The core's samples are untouched: rate control only changes their
//! timing, so the determinism contract and the audio oracle are unaffected.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// A single-producer / single-consumer lock-free ring of mono `f32` samples.
///
/// The producer only advances `tail` and writes cells at and after it; the
/// consumer only advances `head` and reads cells at and after it. With exactly
/// one of each, no locks are needed — the `Acquire`/`Release` pairing publishes
/// each side's index to the other.
///
/// The buffer is `Box<[UnsafeCell<f32>]>` (a cell *per slot*), NOT
/// `UnsafeCell<Box<[f32]>>`: the two threads touch *different* cells, and a
/// per-cell `UnsafeCell` lets each form a raw `*mut f32` to only its own slot.
/// Forming a `&mut [f32]` / `&[f32]` over the *whole* boxed slice from the two
/// threads concurrently is undefined behaviour even for disjoint indices, so it
/// is avoided here.
///
/// Private: its `&self` [`Ring::push`] and [`Ring::pop_into`] are sound only
/// with ONE producer and ONE consumer, and `Ring` is `Sync`, so a public `Ring`
/// would let safe code race two producers on one cell. The handles returned by
/// [`ring`] are the only way in (found in review of v2.7.4, which had made it
/// public when moving it out of `audio.rs`).
struct Ring {
    buf: Box<[UnsafeCell<f32>]>,
    cap: usize,
    /// Read index, owned exclusively by the consumer.
    head: AtomicUsize,
    /// Write index, owned exclusively by the producer.
    tail: AtomicUsize,
    /// Samples that must be queued before the consumer starts (or restarts,
    /// after an underrun) draining. See [`Ring::pop_into`].
    start_threshold: usize,
    /// Whether the consumer is draining (the threshold has been reached since
    /// the last underrun). Written only by the consumer.
    primed: AtomicBool,
}

// SAFETY: SPSC discipline — the producer only ever writes cells in the free
// region starting at `tail`, then publishes `tail` with Release; the consumer
// only ever reads cells in the filled region starting at `head`, then publishes
// `head` with Release. `head` and `primed` are written ONLY by the consumer and
// `tail` ONLY by the producer, so there is no atomic race. A cell is read only
// after `tail` has advanced past it (Acquire on the consumer side orders the
// cell write before) and overwritten only after `head` has advanced past it
// (Acquire on the producer side), so the two never touch the same cell at once.
// Per-cell `UnsafeCell` access never forms a reference over the whole buffer.
// Sharing `&Ring` across the two threads is therefore sound.
unsafe impl Sync for Ring {}
// SAFETY: the ring owns its buffer outright; moving it to another thread moves
// that ownership with it, and the SPSC rules above govern any sharing.
unsafe impl Send for Ring {}

impl Ring {
    /// A ring holding up to `cap - 1` samples, draining only once
    /// `start_threshold` are queued.
    fn new(cap: usize, start_threshold: usize) -> Self {
        let cap = cap.max(2);
        let buf = (0..cap)
            .map(|_| UnsafeCell::new(0.0f32))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            buf,
            cap,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            start_threshold: start_threshold.min(cap - 1),
            primed: AtomicBool::new(false),
        }
    }

    /// Samples currently queued (a snapshot; either side may move it next).
    fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        if tail >= head {
            tail - head
        } else {
            self.cap - head + tail
        }
    }

    /// Producer: enqueue mono samples. When the ring is full the INCOMING
    /// (newest) samples are dropped — the consumer-owned `head` is never written
    /// here, which is what keeps this a sound SPSC queue. Returns how many were
    /// queued.
    fn push(&self, samples: &[f32]) -> usize {
        let mut tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let mut queued = 0;
        for &s in samples {
            let next = if tail + 1 == self.cap { 0 } else { tail + 1 };
            if next == head {
                break; // full: drop the rest (keep `head` consumer-exclusive)
            }
            // SAFETY: `next != head`, so the cell at `tail` is a free slot the
            // consumer is not reading (it reads only up to the published
            // `tail`); writing through the per-cell `UnsafeCell` forms no
            // reference over the whole buffer.
            unsafe { *self.buf[tail].get() = s }
            tail = next;
            queued += 1;
        }
        self.tail.store(tail, Ordering::Release);
        queued
    }

    /// Consumer: fill `out` from the queue, one index load and one index store
    /// for the whole buffer (IOS-03: it was an Acquire load and a Release store
    /// per SAMPLE in the real-time callback). Returns how many real samples
    /// were written; the rest of `out` is silence.
    ///
    /// Nothing drains until `start_threshold` samples are queued, and after an
    /// underrun it waits for the threshold again: restarting on the first
    /// sample back would play a crackle of tiny bursts instead of one gap.
    ///
    /// The batching is a cost property with no observable output: a mutation
    /// back to one index load and store per sample is expected to pass every
    /// test here.
    fn pop_into(&self, out: &mut [f32]) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let avail = if tail >= head {
            tail - head
        } else {
            self.cap - head + tail
        };
        if !self.primed.load(Ordering::Relaxed) {
            if avail < self.start_threshold.max(1) {
                out.fill(0.0);
                return 0;
            }
            self.primed.store(true, Ordering::Relaxed);
        }
        let n = avail.min(out.len());
        let mut h = head;
        for slot in &mut out[..n] {
            // SAFETY: `h` stays within the `avail` samples published by the
            // producer's Release store of `tail`, which the Acquire load above
            // observed; the producer will not write these cells until `head`
            // moves past them. Per-cell read, no whole-buffer reference.
            *slot = unsafe { *self.buf[h].get() };
            h = if h + 1 == self.cap { 0 } else { h + 1 };
        }
        out[n..].fill(0.0);
        self.head.store(h, Ordering::Release);
        if n < out.len() {
            // Underrun: wait for the threshold again before resuming.
            self.primed.store(false, Ordering::Relaxed);
        }
        n
    }
}

/// A ring of `cap` slots (holding `cap - 1` samples) that drains only once
/// `start_threshold` are queued, returned as its only producer and consumer.
#[must_use]
pub fn ring(cap: usize, start_threshold: usize) -> (RingTx, RingRx) {
    let shared = Arc::new(Ring::new(cap, start_threshold));
    (RingTx(Arc::clone(&shared)), RingRx(shared))
}

/// The producer end of a [`ring`]: the only handle that can enqueue.
pub struct RingTx(Arc<Ring>);

impl RingTx {
    /// Enqueue mono samples; when the ring is full the newest are dropped.
    /// Returns how many were queued.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        self.0.push(samples)
    }

    /// Samples currently queued.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The consumer end of a [`ring`]: the only handle that can dequeue.
pub struct RingRx(Arc<Ring>);

impl RingRx {
    /// Fill `out` from the queue (see the private `Ring::pop_into` for the
    /// start-threshold and batching rules). Returns how many real samples were
    /// written; the rest of `out` is silence.
    pub fn pop_into(&mut self, out: &mut [f32]) -> usize {
        self.0.pop_into(out)
    }

    /// Samples currently queued.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Maximum rate deviation the DRC may request: ±1% (about 17 cents), the
/// desktop's figure; see `rustynes-frontend/src/resampler.rs` for why it is not
/// the classic ±0.5%.
pub const MAX_DRC_DELTA: f64 = 0.01;

/// 4-tap Hermite (Catmull-Rom) resampler. `ratio` = input samples consumed per
/// output sample produced (`> 1` squeezes / drains the queue, `< 1` stretches /
/// fills it).
#[derive(Debug)]
pub struct HermiteResampler {
    hist: [f32; 4],
    frac: f64,
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

    /// Set the ratio, clamped to `[1 - MAX_DRC_DELTA, 1 + MAX_DRC_DELTA]`.
    pub fn set_ratio(&mut self, ratio: f64) {
        self.ratio = ratio.clamp(1.0 - MAX_DRC_DELTA, 1.0 + MAX_DRC_DELTA);
    }

    /// The current ratio.
    #[must_use]
    pub const fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Feed `input`, appending resampled output to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &s in input {
            self.hist = [self.hist[1], self.hist[2], self.hist[3], s];
            #[allow(clippy::while_float)] // the standard accumulator form
            while self.frac < 1.0 {
                #[allow(clippy::cast_possible_truncation)] // frac in [0, 1)
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
#[inline]
#[allow(clippy::suboptimal_flops)] // plain mul/add, as on the desktop
fn hermite4(h: &[f32; 4], t: f32) -> f32 {
    let c0 = h[1];
    let c1 = 0.5 * (h[2] - h[0]);
    let c2 = h[0] - 2.5 * h[1] + 2.0 * h[2] - 0.5 * h[3];
    let c3 = 0.5 * (h[3] - h[0]) + 1.5 * (h[1] - h[2]);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// The buffer-fill law: `fill` in `[0, 1]` (0.5 = on target) maps to a ratio
/// in `[1 - delta, 1 + delta]`; above target it consumes input faster, below
/// it stretches.
#[must_use]
pub fn drc_ratio(fill: f64) -> f64 {
    let fill = fill.clamp(0.0, 1.0);
    2.0f64.mul_add(fill * MAX_DRC_DELTA, 1.0 - MAX_DRC_DELTA)
}

/// The producer side of the sink, with dynamic rate control.
///
/// Resamples each pushed block at a ratio steered by how full the ring is, so
/// the queue settles at `target` samples instead of drifting with the gap
/// between the host clock and the audio clock.
pub struct Producer {
    tx: RingTx,
    resampler: HermiteResampler,
    scratch: Vec<f32>,
    target: usize,
}

impl Producer {
    /// A producer feeding `tx`, steering its ring towards `target` queued
    /// samples.
    #[must_use]
    pub fn new(target: usize, tx: RingTx) -> Self {
        Self {
            tx,
            resampler: HermiteResampler::new(),
            scratch: Vec::with_capacity(2048),
            target: target.max(1),
        }
    }

    /// Resample `samples` at the ratio the ring's fill calls for, and queue
    /// them. Returns how many were queued.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        #[allow(clippy::cast_precision_loss)] // sample counts, far below 2^52
        let fill = self.tx.len() as f64 / (2.0 * self.target as f64);
        self.resampler.set_ratio(drc_ratio(fill));
        self.scratch.clear();
        self.resampler.process(samples, &mut self.scratch);
        self.tx.push(&self.scratch)
    }

    /// The ratio used for the last block (for diagnostics and tests).
    #[must_use]
    pub const fn ratio(&self) -> f64 {
        self.resampler.ratio()
    }
}

/// Write one output frame from the processed stereo pair `(l, r)` (IOS-09).
///
/// Mono gets the average; stereo gets `l` / `r`; any further channels get the
/// centre average rather than the left image, which is what they received
/// before v2.7.4. (Unreachable with cpal 0.18 on iOS, which always negotiates
/// stereo, but a surround route would otherwise play only the left side there.)
pub fn fan_out(frame: &mut [f32], l: f32, r: f32) {
    let centre = 0.5 * (l + r);
    match frame {
        [] => {}
        [c0] => *c0 = centre,
        [c0, c1, rest @ ..] => {
            *c0 = l;
            *c1 = r;
            for ch in rest {
                *ch = centre;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_waits_for_its_start_threshold_then_drains_in_order() {
        let (mut tx, mut rx) = ring(64, 8);
        let mut out = [9.0f32; 4];
        tx.push(&[1.0, 2.0, 3.0]);
        assert_eq!(rx.pop_into(&mut out), 0, "below the threshold: silence");
        assert_eq!(out, [0.0; 4]);
        tx.push(&[4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(rx.pop_into(&mut out), 4);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(rx.len(), 4);
    }

    #[test]
    fn an_underrun_waits_for_the_threshold_again() {
        let (mut tx, mut rx) = ring(64, 4);
        tx.push(&[1.0; 4]);
        let mut out = [0.0f32; 6];
        assert_eq!(rx.pop_into(&mut out), 4, "partial buffer, then silence");
        assert_eq!(&out[4..], &[0.0, 0.0]);
        tx.push(&[2.0; 2]);
        assert_eq!(rx.pop_into(&mut out), 0, "two samples back is not enough");
        tx.push(&[2.0; 2]);
        assert_eq!(rx.pop_into(&mut out), 4);
    }

    #[test]
    fn a_full_ring_drops_the_newest_samples() {
        let (mut tx, mut rx) = ring(8, 1); // holds 7
        assert_eq!(tx.push(&[1.0; 10]), 7);
        assert_eq!(tx.len(), 7);
        let mut out = [0.0f32; 7];
        assert_eq!(rx.pop_into(&mut out), 7);
        assert!(rx.is_empty() && tx.is_empty());
        // Wrap-around keeps order.
        tx.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let mut three = [0.0f32; 3];
        rx.pop_into(&mut three);
        assert_eq!(three, [1.0, 2.0, 3.0]);
        assert_eq!(rx.len(), 2);
    }

    /// IOS-02: the servo pulls a ring that sits away from the target back
    /// towards it. The producer runs at exactly the consumer's nominal rate, so
    /// without rate control the fill would stay wherever it started.
    #[test]
    fn rate_control_steers_the_ring_to_its_target() {
        let target = 4800;
        for start in [0usize, 9_000] {
            let (mut tx, mut rx) = ring(48_000, 1);
            tx.push(&vec![0.0; start]);
            let mut producer = Producer::new(target, tx);
            let block = vec![0.25f32; 800];
            let mut out = vec![0.0f32; 800];
            for _ in 0..3000 {
                producer.push(&block);
                rx.pop_into(&mut out);
            }
            let fill = rx.len();
            assert!(
                fill.abs_diff(target) < target / 4,
                "started at {start}, settled at {fill}, target {target}"
            );
        }
    }

    #[test]
    fn the_ratio_follows_the_fill() {
        let (tx, _rx) = ring(48_000, 1);
        let mut p = Producer::new(1000, tx);
        p.push(&[0.0; 10]);
        assert!(p.ratio() < 1.0, "an empty ring stretches");
        p.tx.push(&vec![0.0; 5000]);
        p.push(&[0.0; 10]);
        assert!(p.ratio() > 1.0, "an over-full ring squeezes");
    }

    /// IOS-09: extra channels get the centre, not the left image.
    #[test]
    fn surround_channels_get_the_centre() {
        let mut six = [9.0f32; 6];
        fan_out(&mut six, 1.0, 0.0);
        assert_eq!(six, [1.0, 0.0, 0.5, 0.5, 0.5, 0.5]);
        let mut mono = [9.0f32];
        fan_out(&mut mono, 1.0, 0.0);
        assert_eq!(mono, [0.5]);
        let mut stereo = [9.0f32; 2];
        fan_out(&mut stereo, 0.25, 0.75);
        assert_eq!(stereo, [0.25, 0.75]);
    }

    #[test]
    fn unity_ratio_reproduces_input_with_two_sample_delay() {
        let mut r = HermiteResampler::new();
        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut out = Vec::new();
        r.process(&input, &mut out);
        assert_eq!(out.len(), input.len());
        for (i, &o) in out.iter().enumerate().skip(2) {
            assert_eq!(o, input[i - 2], "sample {i}");
        }
    }
}
