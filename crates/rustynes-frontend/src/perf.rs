//! v2.8.0 Phase 0 — frame-pacing / presentation / audio instrumentation.
//!
//! The pre-v2.8.0 frontend's only timing instrument was a rolling mean FPS
//! over *produced* frames — which the sleep-then-spin pacer makes look
//! rock-steady even while the *display* duplicates or drops frames (the
//! judder the user actually sees). This module measures all three clocks:
//!
//! - **Produced-frame intervals** — time between `run_frame` completions
//!   (the pacer's output cadence).
//! - **Presented-frame intervals** — time between successful
//!   `surface.present()` calls (what the display actually samples).
//! - **Produce cost** — wall time spent inside one `produce_one_frame`
//!   (emulation + audio push + per-frame hooks), the budget run-ahead and
//!   the pacing modes must respect.
//!
//! plus the audio-queue health counters (occupancy / underruns / overruns)
//! the 10-minute soak gate watches, and pacer anomaly counters (catch-up
//! bursts, snap-forwards).
//!
//! Collection is allocation-free per sample (fixed-capacity rings); the
//! percentile sort happens only when a [`PerfView`] is built for the
//! debugger panel (~600 f32s, microseconds).

use std::collections::VecDeque;
use std::time::Duration;

use web_time::Instant;

/// Ring capacity: ~10 s of NTSC frames. Long enough to catch the ~10 s
/// Mailbox beat period, short enough that percentiles track regressions
/// quickly.
///
/// Public because it is the **turnover period of every statistic this module
/// reports**, and a consumer that acts on a change in one of them has to know
/// how long the ring takes to forget the previous regime. `update_runahead_throttle`
/// is exactly such a consumer, and it is the reason this is no longer private:
/// it had hardcoded its own idea of the window, which was wrong by 5x (F27).
pub const WINDOW: usize = 600;

/// Sparkline window (feature K): the number of most-recent frame-time samples
/// the Performance panel plots as a rolling line graph (~4 s of NTSC frames).
pub const SPARK_WINDOW: usize = 240;

/// Summary statistics over one interval/sample ring, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IntervalStats {
    /// Number of samples the stats were computed over.
    pub count: usize,
    /// Mean, ms.
    pub mean_ms: f32,
    /// 50th percentile, ms.
    pub p50_ms: f32,
    /// 95th percentile, ms.
    pub p95_ms: f32,
    /// 99th percentile, ms.
    pub p99_ms: f32,
    /// Maximum, ms.
    pub max_ms: f32,
}

/// Fixed-capacity ring of f32 millisecond samples with percentile summary.
#[derive(Debug, Default)]
struct SampleRing {
    samples_ms: VecDeque<f32>,
}

impl SampleRing {
    fn push(&mut self, ms: f32) {
        if self.samples_ms.len() >= WINDOW {
            self.samples_ms.pop_front();
        }
        self.samples_ms.push_back(ms);
    }

    fn stats(&self) -> IntervalStats {
        let n = self.samples_ms.len();
        if n == 0 {
            return IntervalStats::default();
        }
        let mut sorted: Vec<f32> = self.samples_ms.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let pick = |q: f32| -> f32 {
            // Nearest-rank on the sorted window.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let idx = (((n as f32) * q).ceil() as usize).clamp(1, n) - 1;
            sorted[idx]
        };
        let sum: f32 = sorted.iter().sum();
        #[allow(clippy::cast_precision_loss)] // window bounded by WINDOW.
        IntervalStats {
            count: n,
            mean_ms: sum / n as f32,
            p50_ms: pick(0.50),
            p95_ms: pick(0.95),
            p99_ms: pick(0.99),
            max_ms: *sorted.last().expect("n > 0"),
        }
    }

    fn clear(&mut self) {
        self.samples_ms.clear();
    }

    /// Copy the most-recent `n` samples (oldest-first) into a `Vec`, for the
    /// Performance-panel frame-time sparkline (feature K). Bounded by `n`, so
    /// it never copies the whole `WINDOW` ring.
    fn recent(&self, n: usize) -> Vec<f32> {
        let len = self.samples_ms.len();
        let start = len.saturating_sub(n);
        self.samples_ms.iter().skip(start).copied().collect()
    }
}

/// Interval recorder: turns a stream of timestamps into a ring of deltas.
#[derive(Debug, Default)]
struct IntervalRing {
    last: Option<Instant>,
    ring: SampleRing,
}

impl IntervalRing {
    fn record(&mut self, ts: Instant) {
        if let Some(prev) = self.last {
            self.ring
                .push(ts.duration_since(prev).as_secs_f32() * 1000.0);
        }
        self.last = Some(ts);
    }

    /// Forget the previous timestamp so the next `record` does not log the
    /// gap (ROM load, un-pause, window un-minimize) as a giant interval.
    const fn break_phase(&mut self) {
        self.last = None;
    }

    fn clear(&mut self) {
        self.ring.clear();
        self.last = None;
    }
}

/// Audio-queue health snapshot, set once per produced frame from the native
/// [`crate::audio::SampleQueue`] counters. Zeroed on wasm (Phase 6 wires the
/// `AudioWorklet` equivalents).
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioHealth {
    /// Samples currently buffered between producer and DAC callback.
    pub queued_samples: usize,
    /// Device sample rate (for converting occupancy to milliseconds).
    pub sample_rate: u32,
    /// Cumulative short callback fills (silence padded).
    pub underruns: u64,
    /// Cumulative samples dropped at the queue soft cap.
    pub overrun_dropped: u64,
}

impl AudioHealth {
    /// Occupancy expressed as milliseconds of buffered audio.
    #[must_use]
    pub fn queued_ms(&self) -> f32 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        {
            self.queued_samples as f32 * 1000.0 / self.sample_rate as f32
        }
    }
}

/// v2.3.3 — render-loop cost, owned by the WINIT thread.
///
/// Separate from [`PerfStats`] (which lives behind the emulator mutex and
/// describes the producer) because this describes the *consumer*, and until
/// v2.3.3 nothing measured it at all. That was the blind spot behind a
/// user-visible judder report the produce-side metrics could not explain: the
/// console rate was exact and drops were ~0, yet `presented` p95 sat at 3+
/// display refreshes, meaning frames reached the screen unevenly. Uneven
/// delivery is what the eye reads as stutter, and no instrument covered the
/// path that delivers.
#[derive(Debug, Default)]
pub struct RenderPerf {
    /// egui shell build (`run_shell_ui`), which holds the emulator lock.
    ui: SampleRing,
    /// GPU encode + submit + present (`render_with_overlay`).
    gpu: SampleRing,
    /// The whole `RedrawRequested` handler, end to end.
    total: SampleRing,
    /// v2.3.3 F8 — the BLOCKING present alone (swapchain acquire + present).
    ///
    /// Split out because `total` conflates work with waiting: under Fifo a
    /// present that blocks until vblank is correct behaviour, not a stall, so
    /// a 16 ms `total` p95 could mean either and the metric could not say
    /// which.
    wait: SampleRing,
    /// v2.3.3 F8 — render WORK, recorded PER SAMPLE as `total - wait`.
    ///
    /// Its own series rather than a derived one, because the derivation people
    /// reach for is invalid: `work p95 = total p95 - wait p95` subtracts two
    /// percentiles and is not the percentile of the difference. The first F8
    /// write-up did exactly that and published a table whose `work p95` was
    /// BELOW its `work p50` — percentiles cannot decrease, which is how the
    /// error announced itself. Differencing the two timings of the same redraw
    /// and ranking the result is the only way to get a real work percentile.
    work: SampleRing,
    /// v2.3.3 — wall time the WINIT thread spent blocked acquiring the emulator
    /// mutex during one redraw, summed over every acquisition in the handler.
    ///
    /// The mirror of [`PerfStats::produce_wait`], which measures exactly this
    /// for the producer. Only the producer side was ever measured, so a redraw
    /// blocked behind a produce was billed entirely as render WORK — the third
    /// instance in this campaign of blocking recorded as work, after `cost_*`
    /// and `wait` itself. Six acquisitions sit in the redraw handler and none
    /// of them was timed.
    ///
    /// Subtracted out of `work` so that series finally means work alone:
    /// `work = total - wait - lock`.
    lock: SampleRing,
    /// v2.3.9 item B — redraws where the emulator advanced BETWEEN the
    /// framebuffer copy and the egui pass, and the total observed.
    ///
    /// The `needs_nes` render arm — taken exactly when a debugger or tool panel
    /// is open — acquires the emulator lock TWICE per redraw: once to copy the
    /// framebuffer the user will see, and again sixty lines later for
    /// `run_shell_ui`, where panels read `&mut Nes`. The guard is dropped
    /// between them so the composite work does not hold the emulator.
    ///
    /// If the emulation thread takes the lock in that gap, the screen shows
    /// frame `N` while a panel describes `N+1` — which would be a confidently
    /// wrong answer in Pixel Provenance, whose whole purpose is explaining the
    /// pixel you are looking at.
    ///
    /// Counted rather than assumed. The plan recorded it as a hypothesis from
    /// reading the lock structure, and this line has already retracted one
    /// conclusion drawn from reading rather than measuring, so it is measured:
    /// `hits` non-zero confirms the race fires, zero over a long capture with a
    /// panel open bounds it.
    #[cfg(feature = "debug-hooks")]
    lock_gap_hits: u64,
    /// Denominator for [`Self::lock_gap_hits`] — redraws where both
    /// observations were taken (a ROM is loaded and the arm ran twice).
    #[cfg(feature = "debug-hooks")]
    lock_gap_obs: u64,
    /// v2.3.9 item C — produce-to-visible latency, one sample per presented
    /// frame: from the emulation thread publishing the frame into the handoff to
    /// the present that puts it on screen.
    ///
    /// **A real series, not a sum.** The end-to-end figure needs `work + lock +
    /// wait` plus the frame's wait in the handoff, and those are separate
    /// percentile series — adding two p95s is not the p95 of the sum, the same
    /// defect `work` exists to avoid, in the addition direction rather than the
    /// subtraction one. A distribution can only be built from per-sample totals,
    /// so the total is measured per sample.
    ///
    /// Only the lock-free handoff path contributes. That is the only path where
    /// a frame crosses a thread boundary and can therefore wait; on the
    /// lock-holding path the frame is copied and presented inside one redraw,
    /// so there is no queueing to measure and a sample would describe something
    /// else.
    present_lat: SampleRing,
    /// v2.3.3 — CPU time actually CONSUMED across the `work` span.
    ///
    /// `work` is wall time, so it cannot tell 27 ms of computation from 27 ms
    /// spent off-CPU. This is `CLOCK_THREAD_CPUTIME_ID` differenced over the
    /// same span, so `work - cpu` is the time the winit thread was descheduled.
    /// Zero when the platform has no thread-CPU clock.
    cpu: SampleRing,
}

/// One snapshot of every [`RenderPerf`] series.
///
/// A named struct rather than the tuple this used to return: at four elements
/// the positional form was already easy to mis-destructure, and `lock` made it
/// six. A caller that swaps two `IntervalStats` in a tuple pattern gets no
/// compile error and silently mislabels its output — which, in an investigation
/// that has already retracted one conclusion to a mislabelled metric, is not a
/// theoretical risk.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    /// v2.3.9 item B — redraws where the emulator advanced between the
    /// framebuffer copy and the egui pass. See `RenderPerf::record_lock_gap`.
    #[cfg(feature = "debug-hooks")]
    pub lock_gap_hits: u64,
    /// Denominator for [`Self::lock_gap_hits`].
    #[cfg(feature = "debug-hooks")]
    pub lock_gap_obs: u64,
    /// egui shell build.
    pub ui: IntervalStats,
    /// GPU encode + submit + present.
    pub gpu: IntervalStats,
    /// Whole redraw handler, end to end.
    pub total: IntervalStats,
    /// Blocking present alone.
    pub wait: IntervalStats,
    /// v2.3.9 — produce-to-visible latency, one sample per presented frame.
    pub present_lat: IntervalStats,
    /// `total - wait - lock`.
    pub work: IntervalStats,
    /// Emulator-mutex blocking on the winit thread.
    pub lock: IntervalStats,
    /// CPU time consumed across the `work` span; `work - cpu` is deschedule.
    pub cpu: IntervalStats,
}

impl RenderPerf {
    /// Record the egui shell build.
    pub fn record_ui(&mut self, d: Duration) {
        self.ui.push(d.as_secs_f32() * 1000.0);
    }

    /// Record GPU encode + submit + present.
    pub fn record_gpu(&mut self, d: Duration) {
        self.gpu.push(d.as_secs_f32() * 1000.0);
    }

    /// Record the whole redraw handler.
    pub fn record_total(&mut self, d: Duration) {
        self.total.push(d.as_secs_f32() * 1000.0);
    }

    /// Record the blocking present (vblank wait included).
    pub fn record_wait(&mut self, d: Duration) {
        self.wait.push(d.as_secs_f32() * 1000.0);
    }

    /// Record one redraw's total, blocking present, and emulator-mutex wait
    /// together, deriving the work sample from the same three.
    ///
    /// Prefer this to recording the parts separately: the work series is only
    /// meaningful when every component comes from the SAME redraw, and pairing
    /// them here makes that structural rather than a convention a caller has to
    /// remember. Clamped at zero — the clocks stop at slightly different points,
    /// so a near-zero-work redraw can otherwise produce a small negative.
    pub fn record_redraw(
        &mut self,
        total: Duration,
        wait: Duration,
        lock: Duration,
        cpu: Option<Duration>,
    ) {
        let total_ms = total.as_secs_f32() * 1000.0;
        let wait_ms = wait.as_secs_f32() * 1000.0;
        let lock_ms = lock.as_secs_f32() * 1000.0;
        self.total.push(total_ms);
        self.wait.push(wait_ms);
        self.lock.push(lock_ms);
        self.work.push((total_ms - wait_ms - lock_ms).max(0.0));
        if let Some(c) = cpu {
            self.cpu.push(c.as_secs_f32() * 1000.0);
        }
    }

    /// `(ui, gpu, total, wait, work, lock)` summaries.
    #[must_use]
    pub fn stats(&self) -> RenderStats {
        RenderStats {
            ui: self.ui.stats(),
            gpu: self.gpu.stats(),
            total: self.total.stats(),
            wait: self.wait.stats(),
            work: self.work.stats(),
            lock: self.lock.stats(),
            cpu: self.cpu.stats(),
            present_lat: self.present_lat.stats(),
            #[cfg(feature = "debug-hooks")]
            lock_gap_hits: self.lock_gap_hits,
            #[cfg(feature = "debug-hooks")]
            lock_gap_obs: self.lock_gap_obs,
        }
    }

    /// v2.3.9 item C — record one presented frame's produce-to-visible latency.
    ///
    /// Called only when this redraw actually took a NEW frame from the handoff.
    /// A redraw that re-presents the previous frame contributes nothing: its age
    /// would be measured from a publish two redraws ago and would describe the
    /// display's cadence rather than the pipeline's latency.
    pub fn record_present_latency(&mut self, d: Duration) {
        self.present_lat.push(d.as_secs_f32() * 1000.0);
    }

    /// v2.3.9 item B — record one redraw's two-acquisition observation.
    ///
    /// `advanced` is whether `Nes::cycle()` differed between the framebuffer
    /// copy and the egui pass. The cycle counter is cumulative and monotonic,
    /// and `produce_one_frame` holds the lock across a WHOLE frame, so any
    /// change at all means at least one complete frame landed in the gap.
    #[cfg(feature = "debug-hooks")]
    pub const fn record_lock_gap(&mut self, advanced: bool) {
        // Plain `+= 1`: one increment per redraw, so a `u64` cannot overflow in
        // any run that ends. `saturating_add` here suggested a bound worth
        // reasoning about and there is none. (Review nitpick on #409.)
        self.lock_gap_obs += 1;
        if advanced {
            self.lock_gap_hits += 1;
        }
    }

    /// Drop all samples (new ROM / regime change).
    pub fn clear(&mut self) {
        self.ui.clear();
        self.gpu.clear();
        self.total.clear();
        // Must be cleared with the rest: leaving it behind carried blocking-
        // present samples from the previous ROM or pacing regime into the next
        // experiment's `rwait_*`, while every other render series started
        // fresh — silently mixing two populations in the one column used to
        // tell render work from vblank waiting.
        self.wait.clear();
        self.work.clear();
        self.lock.clear();
        self.cpu.clear();
        self.present_lat.clear();
        // v2.3.9 — and the lock-gap counters, for exactly the reason the `wait`
        // comment above gives. A rate is a ratio over a population; carrying the
        // numerator and denominator across a ROM change or a pacing-regime
        // change mixes two populations into one percentage and reports it as a
        // single measurement. Caught in review on #409 — the new counters had
        // been added to `stats()` and not to `clear()`, which is precisely how
        // `wait` went wrong before them.
        #[cfg(feature = "debug-hooks")]
        {
            self.lock_gap_hits = 0;
            self.lock_gap_obs = 0;
        }
    }
}

/// The live collector. Owned by the `App`; fed from the pacer / produce /
/// present paths; snapshotted into a [`PerfView`] once per frame for the
/// debugger.
#[derive(Debug, Default)]
pub struct PerfStats {
    produced: IntervalRing,
    presented: IntervalRing,
    produce_cost: SampleRing,
    /// v2.3.3 — wall time the producer spent BLOCKED on the emulator mutex
    /// before it could start work, split out of [`Self::produce_cost`].
    ///
    /// The two were conflated until v2.3.3: the produce paths started their
    /// timer *before* `emu.lock()`, so every millisecond the winit thread held
    /// the mutex was billed to the emulator as if the core had been slow. That
    /// made a contention stall indistinguishable from an expensive frame, and
    /// it is the reason the cost tail pinned to almost exactly one display
    /// refresh in every configuration measured — a signature of blocking, not
    /// of work. Recording the wait separately makes the distinction visible:
    /// `cost` is now emulation work alone, and `wait` is the queueing delay.
    produce_wait: SampleRing,
    /// v2.3.3 F15 — winit->emu display-tick hop: send to receipt, milliseconds.
    ///
    /// The last completely unmeasured step in the produce chain, and the only
    /// one that crosses a thread boundary. Instrumented because produce-interval
    /// standard deviation tracks missed presents at **r = 0.937** across
    /// eighteen captures (3.3 ms sd at best, 7.2 ms at worst, against a 16.64 ms
    /// period), so the interval's variance is where the display cadence errors
    /// come from — and it has exactly three terms: how regularly the trigger was
    /// SENT ([`Self::tick_iv`]), how long it took to ARRIVE (this), and how long
    /// the frame took to MAKE ([`Self::produce_cost`]).
    tick_lat: SampleRing,
    /// v2.3.3 F15 — interval between successive display-tick SENDS, milliseconds.
    ///
    /// The winit-side half: how regularly a frame was asked for, independent of
    /// how long the ask took to cross. Kept as its own ranked series rather than
    /// derived from the other two, per the F8 rule that a difference of
    /// percentiles is not a percentile.
    tick_iv: SampleRing,
    /// Paces that produced >= 2 frames (the wall-clock pacer catching up —
    /// each one is an uneven content cadence on screen).
    pub catchup_bursts: u64,
    /// Paces that abandoned catch-up and snapped `next_frame_time` to now
    /// (post-stall resets; hibernate, long UI stall, debugger pause).
    pub snap_forwards: u64,
    /// Working state: produced frames seen since the last present (reset to 0
    /// on each present). Not exposed — feeds `presented_dups` /
    /// `produced_dropped`.
    produced_since_present: u32,
    /// v1.3.0 Workstream B (diagnostic for B3): cumulative presents that showed
    /// NO newly-produced frame since the prior present — the display repeated a
    /// frame. Accrues when the producer is slower than the refresh (or a redraw
    /// was coalesced). Under display-sync it should stay ~0; under wall-clock it
    /// reveals the NTSC-60.0988-Hz-vs-refresh beat as a slow tick.
    pub presented_dups: u64,
    /// v1.3.0 Workstream B (diagnostic for B3): cumulative produced frames
    /// superseded by a newer produce before any present consumed them — the
    /// producer ran ahead of the refresh (≈ one every ~10 s for 60.0988 vs
    /// 60.000 Hz). The companion to `presented_dups`.
    pub produced_dropped: u64,
    /// Latest audio-queue health (native; zeroed on wasm until Phase 6).
    pub audio: AudioHealth,
    /// v2.3.3 — per-event trace buffer. `None` unless the trace is enabled.
    ///
    /// See [`FrameEvent`] for why a per-event record is needed at all when
    /// percentile summaries already exist.
    trace: Option<TraceBuf>,
}

/// v2.3.3 — one traced frame event, buffered in memory and drained by the
/// logger.
///
/// **Why this exists.** Every other instrument in this frontend reports
/// *percentile summaries over a ring*, which destroys temporal order. A
/// `produced` p95 of 26 ms is equally consistent with an alternating 8/25 ms
/// cadence (a shudder), isolated hitches (a stutter), and a slow beat — three
/// symptoms that feel completely different to a player and have three different
/// causes. No summary can separate them; only the sequence can. Likewise
/// `presented_dups` folds the refreshes-per-frame *pattern* into one running
/// total, so at divisor 2 a healthy `2,2,2,2` and a ragged `1,3,2,2,1,3` are
/// indistinguishable — and the ragged one is what "frame forward/backward"
/// looks like.
#[derive(Debug, Clone, Copy)]
pub struct FrameEvent {
    /// Microseconds since the trace began.
    pub t_us: u64,
    /// Microseconds since the previous event of the SAME kind (0 for the
    /// first). This is the interval whose sequence the analysis reads.
    pub interval_us: u32,
    /// On a present: produced frames seen since the previous present — the
    /// refreshes-per-frame pattern. Always 0 on a produce event.
    pub since_present: u32,
    /// `true` for a present, `false` for a produce.
    pub is_present: bool,
}

/// Reserved capacity for the per-event trace buffer.
///
/// Headroom for a stall, not a working size — the buffer is drained every
/// produced frame, so steady-state occupancy is a handful of events. Sized at
/// ~1 s of both event kinds at 120 Hz, times 8.
const TRACE_CAPACITY: usize = 4096;

/// Trace state: the origin instant plus the pending records.
#[derive(Debug)]
struct TraceBuf {
    /// v2.3.3 — `CLOCK_MONOTONIC` nanoseconds at [`Self::origin`], when the
    /// platform can supply it.
    ///
    /// This is what makes the produce/present rows joinable to the `scanout`
    /// rows. Those carry the compositor's own presentation timestamps, which
    /// live in the clock named by `wp_presentation`'s `clock_id` (1 =
    /// `CLOCK_MONOTONIC`); `Instant` is the same clock on Linux but opaque, so
    /// one anchor pair taken at the origin converts the whole series. Without
    /// it the two halves of the trace describe the same run in incomparable
    /// units — which is exactly the mistake that invalidated F10.
    origin_mono_ns: Option<u64>,
    origin: Instant,
    last_produced: Option<Instant>,
    last_presented: Option<Instant>,
    recs: Vec<FrameEvent>,
}

impl PerfStats {
    /// Enable the per-event frame trace.
    ///
    /// Capacity is reserved up front so the hot path never reallocates:
    /// recording happens while the emulator mutex is held, and a `Vec` growth
    /// there would be exactly the kind of measurement artefact this trace
    /// exists to avoid. The buffer is drained **once per produced frame** by
    /// `App::post_produce_housekeeping`, so a handful of events is the real
    /// steady-state occupancy; the reservation is headroom for a stall, not a
    /// working size.
    pub fn enable_trace(&mut self, now: Instant, origin_mono_ns: Option<u64>) {
        self.trace = Some(TraceBuf {
            origin_mono_ns,
            origin: now,
            last_produced: None,
            last_presented: None,
            recs: Vec::with_capacity(TRACE_CAPACITY),
        });
    }

    /// Whether the per-event trace is active.
    #[must_use]
    pub const fn trace_enabled(&self) -> bool {
        self.trace.is_some()
    }

    /// `CLOCK_MONOTONIC` nanoseconds at the trace origin, if the platform
    /// supplied it. The anchor that makes produce/present rows comparable to
    /// compositor `scanout` rows. `wp_presentation` stamps presentations in the
    /// clock named by its `clock_id` event (1 = `CLOCK_MONOTONIC`), which is the
    /// same clock `Instant` uses on Linux but opaque — so one anchor pair taken
    /// at the origin converts the whole series.
    #[must_use]
    pub fn trace_origin_mono_ns(&self) -> Option<u64> {
        self.trace.as_ref().and_then(|t| t.origin_mono_ns)
    }

    /// Swap the buffered events into `spare`, leaving the trace enabled and its
    /// buffer empty **with its capacity intact**.
    ///
    /// A swap rather than `std::mem::take`, which was the first implementation
    /// and was wrong in a way that defeated the point of this whole struct:
    /// `take` leaves a `Vec::new()` behind — capacity **zero** — so every
    /// subsequent `trace_event` push started from nothing and reallocated,
    /// *while the emulator mutex was held*. That is precisely the measurement
    /// artefact `Vec::with_capacity` above exists to prevent, reintroduced one
    /// function later. Caught in review on PR #358 by two reviewers
    /// independently.
    ///
    /// Swapping recycles both buffers forever, so the steady state allocates
    /// nothing at all: the caller's drained `Vec` becomes the next frame's
    /// recording buffer.
    pub fn swap_trace(&mut self, spare: &mut Vec<FrameEvent>) {
        spare.clear();
        if let Some(t) = self.trace.as_mut() {
            std::mem::swap(&mut t.recs, spare);
        }
    }

    /// Push one event into the trace, if enabled.
    fn trace_event(&mut self, ts: Instant, is_present: bool, since_present: u32) {
        let Some(t) = self.trace.as_mut() else {
            return;
        };
        let last = if is_present {
            &mut t.last_presented
        } else {
            &mut t.last_produced
        };
        let interval_us = last.map_or(0, |p| {
            u32::try_from(ts.saturating_duration_since(p).as_micros()).unwrap_or(u32::MAX)
        });
        *last = Some(ts);
        // Bound the buffer: a caller that enables the trace and never drains
        // must not grow it without limit. Dropping the NEWEST here (rather than
        // sliding) is deliberate — a trace with a gap at a known point is
        // honest, whereas a silently re-based window would misreport intervals.
        if t.recs.len() < 1 << 20 {
            t.recs.push(FrameEvent {
                t_us: u64::try_from(ts.saturating_duration_since(t.origin).as_micros())
                    .unwrap_or(u64::MAX),
                interval_us,
                since_present,
                is_present,
            });
        }
    }

    /// Record a produced-frame completion timestamp.
    pub fn record_produced(&mut self, ts: Instant) {
        self.produced.record(ts);
        self.produced_since_present = self.produced_since_present.saturating_add(1);
        self.trace_event(ts, false, 0);
    }

    /// Record a successful surface present. Also derives the present/produce
    /// mismatch diagnostics: a present with no new produce is a duplicate
    /// (display repeated a frame); >1 produce since the last present means the
    /// extra produced frames were dropped (never shown).
    pub fn record_presented(&mut self, ts: Instant) {
        self.presented.record(ts);
        match self.produced_since_present {
            0 => self.presented_dups = self.presented_dups.saturating_add(1),
            n => self.produced_dropped = self.produced_dropped.saturating_add(u64::from(n - 1)),
        }
        // Traced BEFORE the reset: the count at this instant is the
        // refreshes-per-frame datum the whole trace exists to capture.
        self.trace_event(ts, true, self.produced_since_present);
        self.produced_since_present = 0;
    }

    /// Record the wall cost of one `produce_one_frame` call.
    ///
    /// From v2.3.3 this is the work alone — the caller times it from *after*
    /// the mutex is acquired and reports the blocking separately through
    /// [`Self::record_produce_wait`].
    pub fn record_produce_cost(&mut self, d: Duration) {
        self.produce_cost.push(d.as_secs_f32() * 1000.0);
    }

    /// v2.3.3 F15 — the two halves of the display trigger, in milliseconds.
    ///
    /// `lat_ns` is the winit->emu hop (tick send to receipt) and `iv_ns` the
    /// interval between successive sends. Recorded as two INDEPENDENT ranked
    /// series alongside the existing `produce_cost`, so the produce interval —
    /// whose standard deviation tracks missed presents at r = 0.937 — can be
    /// attributed to the trigger, the hop, or the work without ever subtracting
    /// one percentile from another (the F8 mistake).
    ///
    /// A zero means "not measured for this frame" (no tick drove it, or the
    /// monotonic clock was unavailable) and is skipped rather than pushed:
    /// entering it as a sample would drag every percentile toward zero and make
    /// a watchdog-driven session look like a perfectly-triggered one.
    pub fn record_tick_timing(&mut self, lat_ns: u64, iv_ns: u64) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "nanosecond counts here are at most seconds; f32 is exact well past that"
        )]
        if lat_ns > 0 {
            self.tick_lat.push(lat_ns as f32 / 1.0e6);
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "nanosecond counts here are at most seconds; f32 is exact well past that"
        )]
        if iv_ns > 0 {
            self.tick_iv.push(iv_ns as f32 / 1.0e6);
        }
    }

    /// Record how long the producer was blocked on the emulator mutex before
    /// it could begin the frame, separately from the work itself — the split
    /// that showed the measured wait is 0.00 ms at every percentile. Surfaced
    /// as `PerfView::produce_wait`.
    pub fn record_produce_wait(&mut self, d: Duration) {
        self.produce_wait.push(d.as_secs_f32() * 1000.0);
    }

    /// Break interval phase after a discontinuity (ROM load, un-pause) so
    /// the gap is not logged as a giant frame interval.
    pub const fn break_phase(&mut self) {
        self.produced.break_phase();
        self.presented.break_phase();
        // Don't count the discontinuity (ROM load / un-pause) as a dup or drop.
        self.produced_since_present = 0;
    }

    /// Clear all rings + counters (new ROM).
    pub fn clear(&mut self) {
        self.produced.clear();
        self.presented.clear();
        self.produce_cost.clear();
        self.produce_wait.clear();
        self.catchup_bursts = 0;
        self.snap_forwards = 0;
        self.produced_since_present = 0;
        self.presented_dups = 0;
        self.produced_dropped = 0;
        self.audio = AudioHealth::default();
    }

    /// Mean produced-frame interval in milliseconds (0.0 with no samples) —
    /// the fps readout's source (fps = 1000 / mean).
    #[must_use]
    pub fn view_produced_mean_ms(&self) -> f32 {
        self.produced.ring.stats().mean_ms
    }

    /// Build the per-frame snapshot for the debugger Performance panel.
    #[must_use]
    pub fn view(&self) -> PerfView {
        PerfView {
            produced: self.produced.ring.stats(),
            presented: self.presented.ring.stats(),
            produce_cost: self.produce_cost.stats(),
            produce_wait: self.produce_wait.stats(),
            tick_lat: self.tick_lat.stats(),
            tick_iv: self.tick_iv.stats(),
            catchup_bursts: self.catchup_bursts,
            snap_forwards: self.snap_forwards,
            presented_dups: self.presented_dups,
            produced_dropped: self.produced_dropped,
            audio: self.audio,
            // feature K — the last ~4 s of frame-time samples for the panel
            // sparkline (bounded copies; the percentile tables stay primary).
            recent_presented_ms: self.presented.ring.recent(SPARK_WINDOW),
            recent_produced_ms: self.produced.ring.recent(SPARK_WINDOW),
            ..PerfView::default()
        }
    }
}

/// Immutable snapshot rendered by the debugger Performance panel. The
/// present-mode fields are filled in by the app (it owns `Gfx`).
#[derive(Debug, Clone, Default)]
pub struct PerfView {
    /// Produced-frame interval stats (pacer output cadence).
    pub produced: IntervalStats,
    /// Presented-frame interval stats (display-visible cadence).
    pub presented: IntervalStats,
    /// `produce_one_frame` wall-cost stats (work only, from v2.3.3).
    pub produce_cost: IntervalStats,
    /// v2.3.3 — emulator-mutex blocking stats for the producer, recorded by
    /// `PerfStats::record_produce_wait`.
    pub produce_wait: IntervalStats,
    /// v2.3.3 F15 — winit->emu display-tick hop, milliseconds. See
    /// `PerfStats::tick_lat` (plain code span: the field is private, and a link
    /// from public docs to a private item fails the `-D warnings` rustdoc gate).
    pub tick_lat: IntervalStats,
    /// v2.3.3 F15 — interval between display-tick sends, milliseconds. See
    /// `PerfStats::tick_iv`.
    pub tick_iv: IntervalStats,
    /// v2.3.9 item B — redraws where the emulator advanced between the
    /// framebuffer copy and the egui pass, and the total observed. See
    /// `RenderPerf::record_lock_gap` (plain code span: the method is
    /// `debug-hooks`-gated, so a default doc build cannot resolve a link).
    #[cfg(feature = "debug-hooks")]
    pub lock_gap_hits: u64,
    /// Denominator for [`Self::lock_gap_hits`].
    #[cfg(feature = "debug-hooks")]
    pub lock_gap_obs: u64,
    /// v2.3.3 — egui shell build cost (winit thread). See [`RenderPerf`].
    pub render_ui: IntervalStats,
    /// v2.3.3 — GPU encode + present cost (winit thread). See [`RenderPerf`].
    pub render_gpu: IntervalStats,
    /// v2.3.3 F8 — blocking-present (vblank wait) cost. See [`RenderPerf`].
    pub render_wait: IntervalStats,
    /// v2.3.3 F8 — render work, `total - wait - lock` per redraw. A real
    /// series, not a percentile-wise subtraction. See [`RenderPerf`].
    pub render_work: IntervalStats,
    /// v2.3.3 — emulator-mutex blocking on the WINIT thread during a redraw.
    /// The mirror of [`Self::produce_wait`]. See [`RenderPerf`].
    pub render_lock: IntervalStats,
    /// v2.3.3 — CPU time consumed across the render-work span.
    /// `render_work - render_cpu` is time the winit thread spent DESCHEDULED.
    pub render_cpu: IntervalStats,
    /// v2.3.3 — display-tick accounting, read lock-free from
    /// `EmuControl::tick_counts`: `(present-driven, watchdog, dropped)`.
    ///
    /// Diagnostic for the shudder investigation. Under a healthy display-sync
    /// the watchdog count should stay ~0 — every frame driven by an actual
    /// present. A non-trivial watchdog count means frames are being paced by
    /// the 25 ms `DISPLAY_TICK_TIMEOUT` instead, which would show up as exactly
    /// the 25 ms+ `produced` intervals under investigation.
    pub tick_ok: u64,
    /// Watchdog-driven frames. See [`Self::tick_ok`].
    pub tick_timeout: u64,
    /// Present ticks dropped on a full depth-1 channel. See [`Self::tick_ok`].
    pub tick_dropped: u64,
    /// v2.3.3 F21 — cumulative run-ahead depth changes made by the budget
    /// throttle, either direction.
    ///
    /// The only metric that sees the artefact matching the reported shudder: a
    /// depth change displaces the displayed frame by the run-ahead depth, so the
    /// picture jumps forward N frames and back. Hold-duration statistics are
    /// blind to it — the frames either side are each shown for exactly the right
    /// number of refreshes. Cumulative; difference successive rows for a rate.
    pub runahead_toggles: u64,
    /// v2.3.3 F22 — of those toggles, how many REDUCED the depth.
    pub runahead_engages: u64,
    /// v2.3.3 F22 — of those toggles, how many RESTORED it. Engages and
    /// releases have different causes, so the split is what makes the
    /// oscillation diagnosable rather than merely visible.
    pub runahead_releases: u64,
    /// v2.3.3 F23 — measured median cost at the last engage, ms.
    pub thr_engage_cost_ms: f32,
    /// v2.3.3 F23 — measured median cost at the last release, ms.
    pub thr_release_cost_ms: f32,
    /// v2.3.3 F23 — predicted one-depth-up cost the last release accepted, ms.
    ///
    /// Together these say whether the two throttle arms are judging the same
    /// quantity. Near-identical engage/release measured costs with a predicted
    /// value under the release band means one cost is producing two verdicts.
    pub thr_release_pred_ms: f32,
    /// v2.3.3 F16 — frames the compositor reported as **discarded**: composited
    /// but never scanned out (an occluded, minimized or otherwise unpresented
    /// surface).
    ///
    /// **Cumulative for the life of the presentation clock**, not per-sample:
    /// successive rows must be DIFFERENCED to get a rate. A single row's value
    /// answers "has this surface ever gone unpresented", not "is it now".
    ///
    /// Surfaced because its absence made a real failure completely silent — but
    /// state the chain precisely, because the first version of this comment did
    /// not. Sustained `discarded` prevents the **measured** refresh from ever
    /// settling: the estimator needs 24 `presented` reports and gets none.
    /// Whether that costs display-sync depends on the OTHER source
    /// `resolve_pacing` consults — a **declared** refresh from
    /// `current_monitor()`, which it prefers when available. Only when both are
    /// absent does `refresh_source` stay `none` and pacing hold the wall-clock
    /// fallback for the session. Those coincide on the compositor this was
    /// measured on (it advertises no `wl_output`, so `current_monitor()` is
    /// `None`); on a compositor that declares a refresh, display-sync can engage
    /// with discards ongoing.
    ///
    /// So read it as: **this surface was not being scanned out**. That is the
    /// fact it reports. The pacing consequence is conditional, and the stakes
    /// when it does apply are on record — wall-clock dropped 61-147 frames per
    /// 45 s where display-sync dropped 6-15.
    ///
    /// Zero is **not** proof of health: [`PerfView`] reports zero both when
    /// nothing was discarded and when there is no presentation clock at all
    /// (non-Wayland, or the global never bound). Distinguish with
    /// `refresh_source`.
    ///
    /// The counter existed in `PresentationClock` from the start and was read by
    /// nobody. A diagnostic that is never surfaced is not a diagnostic.
    pub present_discarded: u64,
    /// v2.3.3 — whole redraw handler cost (winit thread). See [`RenderPerf`].
    pub render_total: IntervalStats,
    /// See [`PerfStats::catchup_bursts`].
    pub catchup_bursts: u64,
    /// See [`PerfStats::snap_forwards`].
    pub snap_forwards: u64,
    /// See [`PerfStats::presented_dups`] — duplicate presents (display repeated
    /// a frame); the NTSC-vs-refresh beat diagnostic.
    pub presented_dups: u64,
    /// See [`PerfStats::produced_dropped`] — produced frames never presented.
    pub produced_dropped: u64,
    /// Audio-queue health.
    pub audio: AudioHealth,
    /// Effective present mode (e.g. "Mailbox", "Fifo"), from `Gfx`.
    pub present_mode: String,
    /// True when the configured present mode fell back to Fifo.
    pub present_mode_fell_back: bool,
    /// Target frame interval, ms (region-dependent; 16.639 NTSC).
    pub target_ms: f32,
    /// Most recent GPU pass time, ms (`gpu-timing` feature; `None` when the
    /// feature is off / unsupported / not yet resolved).
    pub gpu_ms: Option<f32>,
    /// v2.8.0 Phase 2 — the active pacing regime ("wallclock" /
    /// "display-sync" / "vrr" / "raf" on wasm), with a fallback note when
    /// display-sync disengaged.
    pub pacing: String,
    /// v1.5.0 "Lens" Workstream H8 — the live audio DRC servo ratio (input
    /// samples consumed per output; `1.0` = neutral / DRC off). Drifts within
    /// ±0.5%·`MAX_DRC_DELTA` of the speed factor as the servo tracks the
    /// latency target. Filled by the app from the active `AudioProducer`.
    pub drc_ratio: f64,
    /// H8 — the audio latency target in ms the DRC servos toward (the
    /// `[audio] latency_ms` setpoint after clamping; `0` when no stream).
    pub audio_latency_target_ms: f32,
    /// H8 — the configured run-ahead depth (`[input] run_ahead`, frames).
    pub run_ahead: u32,
    /// H8 — whether run-ahead is currently budget-throttled (produce cost too
    /// high to afford the extra speculative frames).
    pub run_ahead_throttled: bool,
    /// H8 — rewind enabled this session.
    pub rewind_enabled: bool,
    /// H8 — frames currently buffered in the rewind ring (`0` when rewind is
    /// off / the ring is empty).
    pub rewind_frames: usize,
    /// feature K — the most-recent presented-frame interval samples (ms,
    /// oldest-first, up to [`SPARK_WINDOW`]) plotted as the panel's frame-time
    /// sparkline. The presented series is where visible judder lives.
    pub recent_presented_ms: Vec<f32>,
    /// feature K — the most-recent produced-frame interval samples (ms,
    /// oldest-first) plotted as a secondary, fainter line.
    pub recent_produced_ms: Vec<f32>,
}

#[cfg(all(test, feature = "debug-hooks"))]
mod lock_gap_tests {
    use super::RenderPerf;

    /// The counter must distinguish "the race did not fire" from "nothing was
    /// observed". Both read as zero hits, and only the denominator separates
    /// them — which is the whole reason a rate is reported rather than a count.
    #[test]
    fn an_unobserved_redraw_is_not_a_clean_one() {
        let p = RenderPerf::default();
        let s = p.stats();
        assert_eq!((s.lock_gap_hits, s.lock_gap_obs), (0, 0));
    }

    /// `clear()` must reset the counters with everything else. A rate is a
    /// ratio over a population, and carrying the numerator and denominator
    /// across a ROM or pacing-regime change mixes two populations into one
    /// percentage and reports it as a single measurement. Caught in review on
    /// #409, which is the same defect the `wait` series had before it.
    #[test]
    fn clear_resets_the_lock_gap_counters_too() {
        let mut p = RenderPerf::default();
        p.record_lock_gap(true);
        p.record_lock_gap(false);
        assert_eq!(p.stats().lock_gap_obs, 2, "premise: something was counted");

        p.clear();

        let s = p.stats();
        assert_eq!(
            (s.lock_gap_hits, s.lock_gap_obs),
            (0, 0),
            "a rate carried across a regime change is two populations in one number"
        );
    }

    #[test]
    fn hits_and_observations_are_counted_separately() {
        let mut p = RenderPerf::default();
        p.record_lock_gap(false);
        p.record_lock_gap(true);
        p.record_lock_gap(false);
        let s = p.stats();
        assert_eq!(s.lock_gap_obs, 3, "every observation counts");
        assert_eq!(s.lock_gap_hits, 1, "only the advancing redraw is a hit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_ring_yields_zeroed_stats() {
        let r = SampleRing::default();
        assert_eq!(r.stats(), IntervalStats::default());
    }

    #[test]
    fn percentiles_over_known_distribution() {
        let mut r = SampleRing::default();
        // 1..=100 ms — nearest-rank percentiles are exact.
        for i in 1..=100 {
            #[allow(clippy::cast_precision_loss)]
            r.push(i as f32);
        }
        let s = r.stats();
        assert_eq!(s.count, 100);
        assert!((s.p50_ms - 50.0).abs() < f32::EPSILON);
        assert!((s.p95_ms - 95.0).abs() < f32::EPSILON);
        assert!((s.p99_ms - 99.0).abs() < f32::EPSILON);
        assert!((s.max_ms - 100.0).abs() < f32::EPSILON);
        assert!((s.mean_ms - 50.5).abs() < 0.01);
    }

    /// Percentiles cannot decrease. Trivially true of a ranked series, and
    /// asserted anyway because **this repo published a table where it was
    /// false** — the v2.3.3 F8 write-up derived `work p95` as
    /// `total p95 - wait p95`, which is a difference of percentiles and not a
    /// percentile of the difference, and printed a `p95` BELOW its `p50`. The
    /// impossibility sat in the document through authoring, review and a
    /// commit message before a reviewer caught the underlying error.
    ///
    /// This pins the invariant at the only place it can be checked cheaply, so
    /// any future series that reaches `IntervalStats` by a derivation rather
    /// than by ranking fails here instead of in a published table.
    #[test]
    fn percentiles_never_decrease() {
        // Several shapes: uniform, heavy-tailed, single-valued, tiny.
        let shapes: [&[f32]; 4] = [
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
            &[0.01, 0.01, 0.01, 0.02, 0.02, 0.03, 16.0, 22.0],
            &[5.0; 12],
            &[42.0],
        ];
        for shape in shapes {
            let mut r = SampleRing::default();
            for v in shape {
                r.push(*v);
            }
            let s = r.stats();
            assert!(
                s.p50_ms <= s.p95_ms,
                "p50 {} > p95 {} for {shape:?}",
                s.p50_ms,
                s.p95_ms
            );
            assert!(
                s.p95_ms <= s.p99_ms,
                "p95 {} > p99 {} for {shape:?}",
                s.p95_ms,
                s.p99_ms
            );
            assert!(
                s.p99_ms <= s.max_ms,
                "p99 {} > max {} for {shape:?}",
                s.p99_ms,
                s.max_ms
            );
        }
    }

    /// The work series must be a real ranked series, not a subtraction of
    /// percentiles — the F8 defect above, pinned at the recording API.
    #[test]
    fn render_work_is_a_ranked_series_not_a_percentile_difference() {
        let mut p = RenderPerf::default();
        // A redraw population where the naive derivation goes wrong: work is
        // large exactly when wait is small, so `p95(total) - p95(wait)`
        // understates the work tail badly.
        for i in 0..100 {
            let (total, wait) = if i % 10 == 0 {
                (Duration::from_millis(20), Duration::from_micros(100))
            } else {
                (Duration::from_micros(16_600), Duration::from_millis(16))
            };
            p.record_redraw(total, wait, Duration::ZERO, None);
        }
        let s = p.stats();
        // The real work p95 must see the 19.9 ms redraws, not ~0.6 ms.
        assert!(
            s.work.p95_ms > 10.0,
            "work p95 {} lost the tail — is it being derived rather than ranked?",
            s.work.p95_ms
        );
        assert!(s.work.p50_ms <= s.work.p95_ms);
    }

    #[test]
    fn recent_returns_last_n_oldest_first() {
        let mut r = SampleRing::default();
        for i in 0..10 {
            #[allow(clippy::cast_precision_loss)]
            r.push(i as f32);
        }
        // Fewer than available -> the last `n`, oldest-first.
        assert_eq!(r.recent(3), vec![7.0, 8.0, 9.0]);
        // More than available -> the whole ring.
        assert_eq!(r.recent(100).len(), 10);
        // Empty ring -> empty vec.
        assert!(SampleRing::default().recent(5).is_empty());
    }

    // v1.3.0 Workstream B — the present/produce mismatch diagnostics (the
    // NTSC-vs-refresh beat signal) count duplicate presents and dropped produces.
    #[test]
    fn present_produce_mismatch_diagnostics() {
        let mut p = PerfStats::default();
        let t = Instant::now();
        // 1:1 produce:present — clean, no dup, no drop.
        p.record_produced(t);
        p.record_presented(t);
        assert_eq!((p.presented_dups, p.produced_dropped), (0, 0));
        // A present with no new produce since the last one — duplicate frame.
        p.record_presented(t);
        assert_eq!((p.presented_dups, p.produced_dropped), (1, 0));
        // Two produces then one present — the first produce was dropped (unshown).
        p.record_produced(t);
        p.record_produced(t);
        p.record_presented(t);
        assert_eq!((p.presented_dups, p.produced_dropped), (1, 1));
        // break_phase clears the working counter so the in-flight produce is not
        // later mis-counted as a drop across a ROM-load / un-pause discontinuity.
        p.record_produced(t);
        p.break_phase();
        p.record_produced(t);
        p.record_presented(t);
        assert_eq!((p.presented_dups, p.produced_dropped), (1, 1));
        // clear() zeroes the cumulative counters (new ROM).
        p.clear();
        assert_eq!((p.presented_dups, p.produced_dropped), (0, 0));
    }

    #[test]
    fn unpause_break_phase_drops_the_paused_gap() {
        // v1.7.1 — reproduce the pause/unpause pacing glitch: a few healthy
        // 60 fps produced frames, then a long wall-clock gap (the pause), then
        // resume. `break_phase()` on resume must keep the paused gap out of the
        // produced-interval ring, so `produced_max_ms` reflects steady-state
        // cadence (~16.6 ms) and NOT the 675 ms / 1395 ms pause spike.
        let mut p = PerfStats::default();
        let t0 = Instant::now();
        let frame = Duration::from_micros(16_639); // 60.0988 Hz
        for i in 0..4 {
            p.record_produced(t0 + frame * i);
        }
        // Steady-state max is ~one frame interval.
        assert!(p.produced.ring.stats().max_ms < 20.0);
        // The user pauses for ~675 ms, then resumes. On resume the frontend
        // rebases the pacer AND breaks the interval phase.
        let resume = t0 + frame * 3 + Duration::from_millis(675);
        p.break_phase();
        // First post-resume produce: must NOT log the 675 ms paused gap.
        p.record_produced(resume);
        p.record_produced(resume + frame);
        let stats = p.produced.ring.stats();
        assert!(
            stats.max_ms < 20.0,
            "paused gap leaked into produced_max_ms: {} ms",
            stats.max_ms
        );
    }

    #[test]
    fn ring_caps_at_window() {
        let mut r = SampleRing::default();
        for _ in 0..(WINDOW + 50) {
            r.push(1.0);
        }
        assert_eq!(r.stats().count, WINDOW);
    }

    #[test]
    fn interval_ring_breaks_phase_without_logging_gap() {
        let mut ir = IntervalRing::default();
        let t0 = Instant::now();
        ir.record(t0);
        ir.break_phase();
        // The next record must NOT produce an interval (no prev timestamp).
        ir.record(t0 + Duration::from_secs(100));
        assert_eq!(ir.ring.stats().count, 0);
        // ...but the one after it does.
        ir.record(t0 + Duration::from_secs(100) + Duration::from_millis(16));
        assert_eq!(ir.ring.stats().count, 1);
    }

    #[test]
    fn audio_health_queued_ms() {
        let h = AudioHealth {
            queued_samples: 4800,
            sample_rate: 48_000,
            ..AudioHealth::default()
        };
        assert!((h.queued_ms() - 100.0).abs() < 0.001);
        assert!((AudioHealth::default().queued_ms() - 0.0).abs() < f32::EPSILON);
    }
}
