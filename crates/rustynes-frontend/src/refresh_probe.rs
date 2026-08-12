//! v2.3.3 — turning a display refresh into a display-synchronised pacing
//! decision.
//!
//! # Why this exists
//!
//! Until v2.3.3 the frontend learned the display refresh from exactly one
//! source: winit's `current_monitor()` → `refresh_rate_millihertz()`. That has
//! two failure modes, and the reporting host for the v2.3.3 F1 investigation
//! hit both at once:
//!
//! 1. **The API can report nothing.** On the KDE Wayland session that surfaced
//!    the stutter report the compositor advertised no `wl_output` global at
//!    all (65 globals, none an output), so `current_monitor()` had nothing to
//!    return and the perf log recorded `monitor_refresh_hz = unknown`.
//! 2. **Even a known refresh was rejected.** Display-sync produced exactly one
//!    emulated frame per refresh, so it demanded a panel within 0.5% of the
//!    console rate. A 120 Hz or 144 Hz display — i.e. most modern hardware —
//!    could never qualify, and every such host silently fell back to the
//!    free-running wall-clock pacer.
//!
//! The consequence was measurable: the producer hit NTSC timing perfectly
//! (`produced_mean` 16.64 ms in all nine captures) while presents landed at
//! 17.1-17.9 ms, and the excess *was* the drop rate. Two unsynchronised clocks
//! beating against each other, with duplicates and drops accruing at the same
//! time — the signature of a phase beat rather than a shortfall of work.
//!
//! This module removes the second failure mode: [`best_divisor`](crate::refresh_probe::best_divisor)
//! finds the
//! integer `N` with `refresh / N` closest to the console rate, so a 120 Hz
//! panel locks at `N = 2` and a 144 Hz panel is honestly rejected rather than
//! half-working. The first is removed by [`crate::wayland_presentation`],
//! which asks the compositor for the number the windowing API would not give.
//!
//! # A note on where the refresh figure may come from
//!
//! [`estimate_hz_from_intervals`](crate::refresh_probe::estimate_hz_from_intervals)
//! deliberately takes a plain slice of periods
//! and does not care who produced them. It originally served a redraw-interval
//! probe, which was **removed** in v2.3.3: a redraw interval measures the
//! *application*, not the display, and the two diverge exactly when the
//! measurement matters (it returned 20.032 Hz on a 119.991 Hz panel while a
//! heavy ROM ran at ~14 ms/frame). The estimator itself was never the flawed
//! half — its input was — so it survives unchanged and now serves the
//! compositor's own reports. `docs/performance.md` v2.3.3 F4 records the
//! rejection with its numbers.

use std::time::Duration;

/// Lower plausibility bound for a real display, in Hz.
///
/// Anything outside the window is an artefact (a stalled compositor, a
/// minimised window, a debugger breakpoint) rather than a panel, and is
/// rejected rather than acted on.
const MIN_PLAUSIBLE_HZ: f64 = 20.0;
/// Upper plausibility bound — see [`MIN_PLAUSIBLE_HZ`].
const MAX_PLAUSIBLE_HZ: f64 = 400.0;

/// Fraction of samples that must sit within [`STABILITY_BAND`] of the median.
///
/// A compositor that is dropping frames, or a window being dragged across
/// outputs, fails this and yields `None` — which the caller reads as "stay on
/// the wall-clock pacer", the safe answer.
const STABILITY_QUORUM: f64 = 0.80;

/// Relative half-width of the band used by [`STABILITY_QUORUM`].
const STABILITY_BAND: f64 = 0.20;

/// Largest divisor considered by [`best_divisor`].
///
/// Four covers every ratio a console-rate signal meets in practice (60, 120,
/// 180, 240 Hz against ~60 Hz content). Allowing more would start matching
/// coincidences: at `N = 8` some unrelated refresh is always within tolerance
/// of *something*.
pub const MAX_DIVISOR: u32 = 4;

/// Turn a set of per-refresh periods in milliseconds into a refresh rate in Hz,
/// or `None` if they do not describe a steady, plausible cadence.
///
/// The estimator is the **median**, not the mean: a single long period from a
/// compositor hiccup or a scheduler stall shifts a mean enough to change the
/// chosen divisor, while the median ignores it. The stability quorum then
/// rejects the case where the samples are not describing a steady cadence at
/// all.
///
/// Returning `None` is a first-class outcome, not an error path: the caller's
/// correct response is to keep the wall-clock pacer, which is exactly what it
/// was already doing.
///
/// `min_samples` is the caller's threshold, because how many reports are
/// needed depends on what they are: an authoritative period from the
/// compositor needs only enough to prove the output is not mid-change.
#[must_use]
pub fn estimate_hz_from_intervals(samples_ms: &[f64], min_samples: usize) -> Option<f64> {
    if samples_ms.is_empty() || samples_ms.len() < min_samples {
        return None;
    }
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    if median <= 0.0 {
        return None;
    }
    // Reject a cadence that is not actually steady — see STABILITY_QUORUM.
    let lo = median * (1.0 - STABILITY_BAND);
    let hi = median * (1.0 + STABILITY_BAND);
    let within = sorted.iter().filter(|&&s| s >= lo && s <= hi).count();
    #[allow(clippy::cast_precision_loss)] // bounded by the caller's sample cap.
    let ratio = within as f64 / sorted.len() as f64;
    if ratio < STABILITY_QUORUM {
        return None;
    }
    let hz = 1000.0 / median;
    (MIN_PLAUSIBLE_HZ..=MAX_PLAUSIBLE_HZ)
        .contains(&hz)
        .then_some(hz)
}

/// The integer `N` such that presenting one emulated frame every `N` display
/// refreshes best matches `console_hz`, or `None` when no `N` lands within
/// `max_skew`.
///
/// `max_skew` is a *relative* error on the resulting frame rate. Returning
/// `None` means the display and the console rate genuinely cannot be locked —
/// a 75 Hz panel against 60.0988 Hz content has no integer relationship, and
/// the wall-clock pacer with its small, evenly-spread beat is the better
/// answer than a 25%-wrong lock.
#[must_use]
pub fn best_divisor(refresh_hz: f64, console_hz: f64, max_skew: f64) -> Option<u32> {
    if !refresh_hz.is_finite() || !console_hz.is_finite() || refresh_hz <= 0.0 || console_hz <= 0.0
    {
        return None;
    }
    let mut best: Option<(u32, f64)> = None;
    for n in 1..=MAX_DIVISOR {
        let effective = refresh_hz / f64::from(n);
        let err = ((effective - console_hz) / console_hz).abs();
        if best.is_none_or(|(_, b)| err < b) {
            best = Some((n, err));
        }
    }
    best.filter(|&(_, err)| err <= max_skew).map(|(n, _)| n)
}

/// Convenience: the effective emulated frame period when `divisor` refreshes
/// of a `refresh_hz` display drive one frame.
#[must_use]
pub fn effective_period(refresh_hz: f64, divisor: u32) -> Duration {
    Duration::from_secs_f64(f64::from(divisor) / refresh_hz)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NTSC console rate, the target every divisor decision is measured
    /// against.
    const NTSC: f64 = 60.0988;

    /// Sample count the estimator tests use — the same order of magnitude as
    /// [`crate::wayland_presentation`]'s real threshold.
    const N: usize = 24;

    fn steady(interval_ms: f64, n: usize) -> Vec<f64> {
        vec![interval_ms; n]
    }

    #[test]
    fn a_60hz_cadence_is_estimated_as_60hz() {
        let hz = estimate_hz_from_intervals(&steady(1000.0 / 60.0, N), N)
            .expect("steady cadence estimates");
        assert!((hz - 60.0).abs() < 0.5, "got {hz}");
    }

    #[test]
    fn a_120hz_cadence_is_estimated_as_120hz() {
        let hz = estimate_hz_from_intervals(&steady(1000.0 / 120.0, N), N)
            .expect("steady cadence estimates");
        assert!((hz - 120.0).abs() < 1.0, "got {hz}");
    }

    #[test]
    fn too_few_samples_offer_no_estimate() {
        assert!(estimate_hz_from_intervals(&steady(1000.0 / 60.0, N / 2), N).is_none());
        assert!(estimate_hz_from_intervals(&[], N).is_none());
    }

    /// The whole point of the median + quorum: a cadence that is not steady
    /// must be refused rather than averaged into a confident wrong answer.
    #[test]
    fn an_unsteady_cadence_is_refused() {
        // Alternating 8.3 ms and 33 ms — a compositor dropping frames.
        let samples: Vec<f64> = (0..N)
            .map(|i| if i % 2 == 0 { 8.3 } else { 33.0 })
            .collect();
        assert!(
            estimate_hz_from_intervals(&samples, N).is_none(),
            "unsteady cadence must not estimate"
        );
    }

    /// A few outliers must not move the estimate — the reason for a median.
    #[test]
    fn isolated_stalls_do_not_move_the_estimate() {
        let samples: Vec<f64> = (0..N)
            .map(|i| if i % 12 == 11 { 100.0 } else { 1000.0 / 120.0 })
            .collect();
        let hz = estimate_hz_from_intervals(&samples, N).expect("mostly-steady cadence estimates");
        assert!((hz - 120.0).abs() < 1.0, "got {hz}");
    }

    /// A period outside the plausible window is an artefact, not a panel.
    #[test]
    fn an_implausible_rate_is_refused() {
        assert!(
            estimate_hz_from_intervals(&steady(500.0, N), N).is_none(),
            "2 Hz"
        );
        assert!(
            estimate_hz_from_intervals(&steady(1.0, N), N).is_none(),
            "1000 Hz"
        );
        assert!(
            estimate_hz_from_intervals(&steady(0.0, N), N).is_none(),
            "zero"
        );
    }

    #[test]
    fn sixty_hz_locks_at_one_to_one() {
        assert_eq!(best_divisor(60.0, NTSC, 0.005), Some(1));
    }

    /// The case that motivated the whole change: a 120 Hz panel was rejected
    /// outright before v2.3.3 because only 1:1 existed.
    #[test]
    fn one_hundred_twenty_hz_locks_at_two() {
        assert_eq!(best_divisor(120.0, NTSC, 0.005), Some(2));
    }

    #[test]
    fn two_forty_hz_locks_at_four() {
        assert_eq!(best_divisor(240.0, NTSC, 0.005), Some(4));
    }

    /// 144 Hz has no integer relationship with ~60 Hz content (144/2 = 72,
    /// 144/3 = 48). Refusing is correct; the wall-clock pacer handles it.
    #[test]
    fn one_forty_four_hz_has_no_lock_and_is_refused() {
        assert_eq!(best_divisor(144.0, NTSC, 0.005), None);
    }

    #[test]
    fn seventy_five_hz_has_no_lock_and_is_refused() {
        assert_eq!(best_divisor(75.0, NTSC, 0.005), None);
    }

    /// A real 119.99 Hz panel (the reporting host) must still lock at 2.
    #[test]
    fn a_slightly_off_120hz_panel_still_locks() {
        assert_eq!(best_divisor(119.99, NTSC, 0.005), Some(2));
    }

    #[test]
    fn pal_content_locks_against_a_100hz_panel() {
        assert_eq!(best_divisor(100.0, 50.007, 0.005), Some(2));
    }

    #[test]
    fn nonsense_inputs_are_refused() {
        assert_eq!(best_divisor(f64::NAN, NTSC, 0.005), None);
        assert_eq!(best_divisor(0.0, NTSC, 0.005), None);
        assert_eq!(best_divisor(-60.0, NTSC, 0.005), None);
    }

    #[test]
    fn effective_period_matches_the_divisor() {
        let p = effective_period(120.0, 2);
        assert!((p.as_secs_f64() - 1.0 / 60.0).abs() < 1e-9);
    }
}
