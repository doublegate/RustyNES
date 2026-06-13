//! Band-limited synthesis for the APU's audio output.
//!
//! # What this is
//!
//! A streaming **BLEP (Band-Limited Step) decimator** that takes the
//! per-CPU-cycle mixer output and produces band-limited samples at the
//! host audio rate (default 44.1 kHz).
//!
//! The technique is the same one used by Shay Green's `blip_buf` (BSD/MIT)
//! and Mesen2's mixer:
//!
//! - Pre-compute a polyphase windowed-sinc kernel ([`crate::blip_kernel`])
//!   keyed by `PHASES = 32` sub-output-sample fractional offsets, with
//!   `TAPS = 32` coefficients per row giving the FIR a ±16-output-sample
//!   reach.
//! - For each CPU-rate input, compute the amplitude **delta** from the
//!   previous value.
//! - Position the delta at its fractional output-sample location and add
//!   `delta * kernel[phase][m]` to `TAPS` positions of a **host-rate
//!   delta buffer**.
//! - Integrate the delta buffer (running cumulative sum) to recover the
//!   reconstructed signal, then push through the existing analog
//!   [`crate::mixer::FilterChain`] (90 Hz HPF + 440 Hz HPF + 14 kHz LPF).
//!
//! This is the textbook BLEP structure: each abrupt step in the input is
//! replaced by the band-limited equivalent (`sinc * window`-shaped step
//! response), eliminating the alias products that a naive sample-and-
//! hold decimator leaves above Nyquist.
//!
//! # Why this replaces the previous ratio-counter decimator
//!
//! The pre-v0.9.x decimator was a ratio-counter with sample-and-hold
//! reconstruction — functionally a rectangular-window decimator that
//! left aliased energy above Nyquist (~22.05 kHz at 44.1 kHz host rate).
//! The existing 14 kHz LPF in the [`crate::mixer::FilterChain`] suppressed
//! the audible portion, but the band beyond 14 kHz contained alias-
//! pumping products that the analog filter could not fully kill. With
//! the v0.9.x mapper-audio extensions (VRC6 / Sunsoft 5B / Namco 163 /
//! MMC5) adding wavetable + envelope-modulated FM-like complexity, the
//! alias floor started clearing the LPF's stopband attenuation in spots.
//!
//! The polyphase BLEP decimator pushes the alias rejection well below
//! -60 dB across the audible band even for input frequencies above the
//! host Nyquist (verified by the spectral FFT regression test in
//! `tests/spectral.rs`).
//!
//! # API contract
//!
//! Unchanged from the previous decimator:
//!
//! - [`BlipBuf::new(sample_rate, cpu_rate)`] — same signature.
//! - [`BlipBuf::add_sample(value)`] — called once per CPU cycle.
//! - [`BlipBuf::drain`], [`BlipBuf::drain_all`], [`BlipBuf::len`],
//!   [`BlipBuf::is_empty`], [`BlipBuf::reset`] — same semantics.
//!
//! Save-state compat: the snapshot module reads/writes `sample_rate`,
//! `cpu_rate`, `phase`, `filter`, and `held_value`. All five are
//! preserved on this rewrite. The internal delta ring is NOT serialized
//! — same intentional behavior as before (the ring's contents are sub-
//! audio-sample-window detail; a fresh restored state begins emitting
//! samples as soon as `tick()` runs forward).
//!
//! # Determinism
//!
//! All math is `f32` with a fixed operation order. The kernel is pre-
//! computed bit-identically across builds (see
//! [`crate::blip_kernel::Kernel`]). No allocations on the hot path.

#[cfg(test)]
use crate::blip_kernel::PHASES;
use crate::blip_kernel::{Kernel, TAPS};
use crate::mixer::FilterChain;
use alloc::vec::Vec;

/// CPU cycles per second, NTSC.
pub const CPU_HZ_NTSC: f64 = 1_789_773.0;
/// CPU cycles per second, PAL (slightly slower).
pub const CPU_HZ_PAL: f64 = 1_662_607.0;

/// Size of the host-rate delta ring buffer. Must be a power of two for
/// the modulo via bitmask. Held large enough to comfortably absorb a
/// burst of pending output samples plus the kernel's TAPS-sample reach.
/// 4096 samples ≈ 93 ms of audio at 44.1 kHz — far more than any single
/// frame's worth of output (~735 samples).
const RING_SIZE: usize = 4096;
const RING_MASK: usize = RING_SIZE - 1;

/// Streaming BLEP decimator + filter chain that feeds host-rate samples
/// to the frontend's audio thread.
#[derive(Debug, Clone)]
pub struct BlipBuf {
    /// Host sample rate (Hz).
    pub(crate) sample_rate: u32,
    /// CPU rate (Hz, fractional).
    pub(crate) cpu_rate: f64,
    /// Fractional output-sample position. Advances by `step = sample_rate
    /// / cpu_rate` per input sample. Whenever the integer part advances,
    /// one host-rate output sample is "ready" (its delta contributions
    /// have been written for at least `TAPS/2` future samples ahead, so
    /// it's safe to read out).
    pub(crate) phase: f64,
    /// Step per input sample (`sample_rate / cpu_rate`).
    step: f64,
    /// Output filter chain (90 Hz HPF + 440 Hz HPF + 14 kHz LPF).
    pub(crate) filter: FilterChain,
    /// Output ring of finalized post-filter samples awaiting drain.
    pub(crate) samples: Vec<f32>,
    /// Most recent input value handed to [`Self::add_sample`]. The next
    /// call's delta is `value - held_value`.
    pub(crate) held_value: f32,
    /// Pre-computed polyphase windowed-sinc kernel.
    kernel: Kernel,
    /// Host-rate delta ring buffer. Each input delta scatters its
    /// `kernel * delta` contributions across `TAPS` positions of this
    /// buffer; the buffer is then integrated (cumulative sum) to
    /// recover the actual sample stream.
    delta_ring: [f32; RING_SIZE],
    /// Integer output-sample index of the **current** input's scatter
    /// center. Each scatter writes to ring positions
    /// `[head - TAPS/2, head + TAPS/2)`. The next emit reads at
    /// `head - TAPS/2 - 1` (one past the leftmost scatter slot).
    /// Wraps within `RING_SIZE` via `& RING_MASK`.
    head: usize,
    /// True once `head` has advanced far enough that the leftmost
    /// scatter slot `head - TAPS/2` has settled (no future input can
    /// write to it). Output emission gated on this flag — for the very
    /// first `TAPS/2 + 1` inputs, the integrator is just warming up
    /// and no samples are emitted yet. This costs one frame of startup
    /// latency (~16 samples = 0.36 ms @ 44.1 kHz, well below human-
    /// perceptible).
    primed: bool,
    /// Running integrator state (cumulative sum of consumed delta-ring
    /// entries since reset). Converts the scattered delta stream back
    /// into an absolute-amplitude sample stream.
    integrator: f32,
}

impl BlipBuf {
    /// Create a new band-limited buffer.
    ///
    /// `sample_rate` is the host audio rate in Hz (typically 44 100).
    /// `cpu_rate` is the NES CPU rate in Hz ([`CPU_HZ_NTSC`] or
    /// [`CPU_HZ_PAL`]).
    #[must_use]
    pub fn new(sample_rate: u32, cpu_rate: f64) -> Self {
        let step = f64::from(sample_rate) / cpu_rate;
        let mut b = Self {
            sample_rate,
            cpu_rate,
            phase: 0.0,
            step,
            filter: FilterChain::new(sample_rate),
            samples: Vec::with_capacity(8192),
            held_value: 0.0,
            kernel: Kernel::new(),
            delta_ring: [0.0; RING_SIZE],
            // Start `head` at TAPS so the initial scatter (writing to
            // `head - TAPS/2 .. head + TAPS/2`) lands at indices
            // `[TAPS/2, 3*TAPS/2)` and never goes negative.
            head: TAPS,
            primed: false,
            integrator: 0.0,
        };
        b.reset();
        b
    }

    /// Reset to silence. Empties the input ring, the pending output
    /// queue, and the filter chain state.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.filter.reset();
        self.samples.clear();
        self.held_value = 0.0;
        self.delta_ring = [0.0; RING_SIZE];
        self.head = TAPS;
        self.primed = false;
        self.integrator = 0.0;
    }

    /// Add one mixed sample at CPU resolution. The buffer accumulates
    /// host-rate samples internally; drain via [`Self::drain`] or
    /// [`Self::drain_all`].
    ///
    /// The mixer's output is approximately in `[-0.5, 0.5]` for the 2A03
    /// alone, with on-cart audio expansions pushing the absolute range
    /// up somewhat. We clamp `value` defensively so a stray NaN/Inf or
    /// runaway mapper can't propagate non-finite values into the FIR
    /// state. The clamp range is far outside any sane mixer output, so
    /// the gate is purely a saturation backstop, not a normal-operation
    /// limiter.
    #[inline]
    pub fn add_sample(&mut self, value: f32) {
        // Defensive saturation. Real mixer output never exceeds ~1.5; the
        // clamp catches NaN/Inf from a runaway mapper-audio path so the
        // FIR can't propagate non-finite values through the ring.
        let value = if value.is_finite() {
            value.clamp(-4.0, 4.0)
        } else {
            0.0
        };
        let delta = value - self.held_value;
        self.held_value = value;

        // Scatter the delta into the host-rate ring via the kernel row
        // matching the input's fractional output-sample position.
        //
        // `self.phase` ∈ [0, 1) is the fractional output-sample position
        // of THIS input within the current output-grid interval. The
        // kernel row matching this fractional offset spreads the delta
        // across `TAPS` output samples centered at `head`. The center
        // sits at `head + TAPS/2 - 1` (the right-half tap that's closest
        // to the input's position); we walk the kernel from `head` for
        // `TAPS` slots.
        if delta != 0.0 {
            #[allow(clippy::cast_possible_truncation)]
            let row = self.kernel.row(self.phase as f32);
            // Center the scatter at `head` (the current integer output
            // index). Tap 0 lands at `head - TAPS/2`; tap TAPS-1 lands
            // at `head + TAPS/2 - 1`. The kernel itself encodes the
            // sub-sample fractional shift via the row index.
            //
            // v2.8.0 Phase 4b — the 32-tap window wraps the ring at most
            // once, so split it into (at most) two CONTIGUOUS runs instead
            // of masking every index: each run is a plain SAXPY LLVM
            // auto-vectorizes (SSE2/NEON/wasm-simd), which the per-tap
            // `& RING_MASK` form structurally prevented. Per-slot math is
            // unchanged (`slot += delta * coeff`, one touch per slot, mul
            // then add — no FMA contraction), so output is bit-identical
            // to the scalar form.
            let start = self.head.wrapping_sub(TAPS / 2) & RING_MASK;
            let first = (RING_SIZE - start).min(TAPS);
            for (slot, &coeff) in self.delta_ring[start..start + first]
                .iter_mut()
                .zip(&row[..first])
            {
                *slot += delta * coeff;
            }
            for (slot, &coeff) in self.delta_ring[..TAPS - first]
                .iter_mut()
                .zip(&row[first..])
            {
                *slot += delta * coeff;
            }
        }

        // Advance phase. When it crosses 1.0, advance `head` and
        // emit the output sample that just fell out of the scatter
        // window (one slot to the left of `head - TAPS/2`).
        self.phase += self.step;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.head = self.head.wrapping_add(1);

            // The slot at `head - TAPS/2 - 1` is now permanently
            // finalized: any future scatter writes to `[new_head -
            // TAPS/2, new_head + TAPS/2)`, which doesn't reach back
            // this far. Emit it.
            //
            // During warm-up (the first TAPS/2 + 1 head advances), the
            // initial slots haven't been written by any scatter yet
            // (they're still zero from construction); we burn through
            // them silently to flush the startup phase, then start
            // emitting once `primed` is set.
            let emit_idx = self.head.wrapping_sub(TAPS / 2 + 1) & RING_MASK;
            self.integrator += self.delta_ring[emit_idx];
            self.delta_ring[emit_idx] = 0.0;

            if !self.primed {
                // Have we advanced past the initial dead zone? The
                // first scatter wrote at `head_initial = TAPS` (i.e.,
                // covering `[TAPS/2, 3*TAPS/2)`). We start emitting
                // once `head >= TAPS + TAPS/2 + 1`, i.e., the emit
                // slot has caught up with the first scatter's right
                // edge.
                if self.head > TAPS + TAPS / 2 {
                    self.primed = true;
                }
                continue;
            }

            let filtered = self.filter.process(self.integrator);
            self.samples.push(filtered);
        }
    }

    /// Drain finalized samples into `out`. Returns the number written.
    /// Excess pending samples are kept; if `out.len() < self.samples.len()`
    /// the remainder waits for the next drain.
    pub fn drain(&mut self, out: &mut [f32]) -> usize {
        let n = self.samples.len().min(out.len());
        out[..n].copy_from_slice(&self.samples[..n]);
        self.samples.drain(..n);
        n
    }

    /// Drain all finalized samples into a new `Vec`.
    #[must_use]
    pub fn drain_all(&mut self) -> Vec<f32> {
        // `core::mem::take` is `std::mem::take` (the std path re-exports).
        // Using the `core` path keeps this module portable to `#![no_std]`
        // builds. See `docs/architecture.md` §no_std boundary.
        core::mem::take(&mut self.samples)
    }

    /// Number of samples currently buffered, awaiting drain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the pending-output queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Total CPU cycles in 1 second of NTSC playback. Used as a "long-
    /// run" fixture for the determinism / DC tests.
    const ONE_SECOND_NTSC: usize = 1_789_773;

    /// 10 frames at 60 Hz (used by spectral tests + decay tests).
    const TEN_FRAMES_NTSC: usize = ONE_SECOND_NTSC / 6;

    #[test]
    fn empty_buffer_reads_zero_samples() {
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        let mut out = [0.0_f32; 16];
        assert_eq!(b.drain(&mut out), 0);
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn emits_expected_sample_count() {
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..ONE_SECOND_NTSC {
            b.add_sample(0.0);
        }
        let drained = b.drain_all();
        let n = drained.len();
        // Expected: 44_100 samples ± a small warm-up loss. The BLEP
        // structure delays output by `TAPS/2 + 1` host samples after
        // construction so each emitted sample has received its full
        // kernel scatter — that's ~17 samples of startup delay at our
        // TAPS=32 configuration. After 1 s, output count is
        // `44_100 - 17 ± rounding`.
        let max_loss = TAPS / 2 + 4;
        assert!(
            n <= 44_100 && n + max_loss >= 44_100,
            "expected 44100 - {max_loss}..=44100 samples in 1 s, got {n}"
        );
    }

    #[test]
    fn dc_passes_through_with_settled_gain() {
        // A constant 0.5 input. The FIR scatters NO delta (the input
        // never changes after the first sample), so the integrator
        // converges to 0.5. The HPFs then attenuate the DC; after
        // ~200 000 samples the output is near zero.
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..200_000 {
            b.add_sample(0.5);
        }
        let drained = b.drain_all();
        let last = *drained.last().unwrap();
        assert!(last.abs() < 0.05, "DC not removed; last sample = {last}");
    }

    #[test]
    fn drain_empties_buffer() {
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..1000 {
            b.add_sample(0.0);
        }
        let _ = b.drain_all();
        assert!(b.is_empty());
    }

    #[test]
    fn single_delta_produces_band_limited_step() {
        // Feed silence, then a unit step. The output should be a
        // band-limited ramp (sinc-ringing) reaching the new amplitude
        // over ~TAPS host-rate samples.
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..10_000 {
            b.add_sample(0.0);
        }
        for _ in 0..10_000 {
            b.add_sample(1.0);
        }
        // Push enough samples to fully settle the FIR + HPF response.
        for _ in 0..50_000 {
            b.add_sample(1.0);
        }
        let drained = b.drain_all();
        // After 50_000+ samples of constant 1.0, the HPFs drive output
        // back to ~0 (DC blocked).
        let last = *drained.last().unwrap();
        assert!(
            last.abs() < 0.05,
            "constant input not DC-blocked: last = {last}"
        );
        // And the step transient produced finite peak < the saturation
        // clip (1.5 is our cap before clamp).
        let max_abs = drained.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(
            max_abs > 0.0 && max_abs < 1.5,
            "step transient max = {max_abs}, expected (0, 1.5)"
        );
    }

    #[test]
    fn opposing_deltas_cancel_to_dc() {
        // Equal amounts of +1 and -1 in long runs, end with HPF-blocked
        // output. The integrator must NOT drift.
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..100_000 {
            b.add_sample(1.0);
        }
        for _ in 0..100_000 {
            b.add_sample(-1.0);
        }
        let drained = b.drain_all();
        let last = *drained.last().unwrap();
        assert!(last.abs() < 0.05, "didn't cancel; last = {last}");
    }

    #[test]
    fn deterministic_across_runs() {
        // Same input sequence produces bit-identical output.
        let drive = |b: &mut BlipBuf| {
            for i in 0..TEN_FRAMES_NTSC {
                #[allow(clippy::cast_precision_loss)]
                let v = ((i as f32) * 0.0001).sin() * 0.5;
                b.add_sample(v);
            }
        };
        let mut a = BlipBuf::new(44_100, CPU_HZ_NTSC);
        drive(&mut a);
        let av = a.drain_all();
        let mut c = BlipBuf::new(44_100, CPU_HZ_NTSC);
        drive(&mut c);
        let cv = c.drain_all();
        assert_eq!(av.len(), cv.len(), "same input, different output length");
        for (i, (x, y)) in av.iter().zip(cv.iter()).enumerate() {
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "non-determinism at index {i}: {x} vs {y}"
            );
        }
    }

    #[test]
    fn saturation_clips_extreme_values() {
        // A pathological NaN / Inf input must not poison the output ring.
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        b.add_sample(f32::NAN);
        b.add_sample(f32::INFINITY);
        b.add_sample(f32::NEG_INFINITY);
        for _ in 0..200 {
            b.add_sample(1.0e20);
        }
        // No panics, drained samples are all finite.
        let drained = b.drain_all();
        for v in &drained {
            assert!(v.is_finite(), "output poisoned by extreme input: {v}");
        }
        assert!(b.held_value.is_finite());
    }

    #[test]
    fn reset_clears_all_state() {
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..1000 {
            b.add_sample(0.5);
        }
        b.reset();
        assert_eq!(b.phase, 0.0);
        assert_eq!(b.held_value, 0.0);
        assert!(b.is_empty());
        for v in &b.delta_ring {
            assert_eq!(*v, 0.0);
        }
        assert_eq!(b.integrator, 0.0);
    }

    #[test]
    fn drain_slice_handles_partial_read() {
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for _ in 0..10_000 {
            b.add_sample(0.0);
        }
        let queued = b.len();
        assert!(queued > 0);
        let mut out = [0.0_f32; 16];
        let n = b.drain(&mut out);
        assert_eq!(n, 16);
        assert_eq!(b.len(), queued - 16);
    }

    #[test]
    fn long_run_produces_finite_output() {
        // Sweep amplitudes so the FIR convolution + integrator path is
        // exercised. All drained samples must be finite (no NaN/Inf
        // leakage from the kernel-row lookup or integrator drift).
        let mut b = BlipBuf::new(44_100, CPU_HZ_NTSC);
        for i in 0..(ONE_SECOND_NTSC / 100) {
            #[allow(clippy::cast_precision_loss)]
            let v = ((i % 100) as f32) / 100.0 - 0.5;
            b.add_sample(v);
        }
        let drained = b.drain_all();
        for (i, v) in drained.iter().enumerate() {
            assert!(v.is_finite(), "non-finite at index {i}: {v}");
        }
    }

    #[test]
    fn kernel_phases_accessible() {
        // Sanity: PHASES + TAPS are the kernel's dimensional parameters.
        // Powers of two keep the delta_ring's RING_MASK modulo working
        // and allow the kernel-row lookup to stay branch-free under
        // the hood. PHASES ≥ 32 ensures sub-sample-phase quantization
        // noise stays below the spectral acceptance gate (verified by
        // `tests/spectral.rs`).
        assert!(PHASES.is_power_of_two() && PHASES >= 32);
        assert!(TAPS.is_power_of_two() && TAPS >= 16);
    }
}
