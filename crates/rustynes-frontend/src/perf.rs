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
const WINDOW: usize = 600;

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

/// The live collector. Owned by the `App`; fed from the pacer / produce /
/// present paths; snapshotted into a [`PerfView`] once per frame for the
/// debugger.
#[derive(Debug, Default)]
pub struct PerfStats {
    produced: IntervalRing,
    presented: IntervalRing,
    produce_cost: SampleRing,
    /// Paces that produced >= 2 frames (the wall-clock pacer catching up —
    /// each one is an uneven content cadence on screen).
    pub catchup_bursts: u64,
    /// Paces that abandoned catch-up and snapped `next_frame_time` to now
    /// (post-stall resets; hibernate, long UI stall, debugger pause).
    pub snap_forwards: u64,
    /// Latest audio-queue health (native; zeroed on wasm until Phase 6).
    pub audio: AudioHealth,
}

impl PerfStats {
    /// Record a produced-frame completion timestamp.
    pub fn record_produced(&mut self, ts: Instant) {
        self.produced.record(ts);
    }

    /// Record a successful surface present.
    pub fn record_presented(&mut self, ts: Instant) {
        self.presented.record(ts);
    }

    /// Record the wall cost of one `produce_one_frame` call.
    pub fn record_produce_cost(&mut self, d: Duration) {
        self.produce_cost.push(d.as_secs_f32() * 1000.0);
    }

    /// Break interval phase after a discontinuity (ROM load, un-pause) so
    /// the gap is not logged as a giant frame interval.
    pub const fn break_phase(&mut self) {
        self.produced.break_phase();
        self.presented.break_phase();
    }

    /// Clear all rings + counters (new ROM).
    pub fn clear(&mut self) {
        self.produced.clear();
        self.presented.clear();
        self.produce_cost.clear();
        self.catchup_bursts = 0;
        self.snap_forwards = 0;
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
            catchup_bursts: self.catchup_bursts,
            snap_forwards: self.snap_forwards,
            audio: self.audio,
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
    /// `produce_one_frame` wall-cost stats.
    pub produce_cost: IntervalStats,
    /// See [`PerfStats::catchup_bursts`].
    pub catchup_bursts: u64,
    /// See [`PerfStats::snap_forwards`].
    pub snap_forwards: u64,
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
