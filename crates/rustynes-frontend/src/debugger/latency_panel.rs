//! Latency Oracle panel (v2.3.6) — measure the loaded game's **own** input lag
//! and recommend a run-ahead depth.
//!
//! Every emulator makes finding this number a manual ritual: hold a direction,
//! frame-advance until the sprite moves, subtract one. `RetroArch` documents
//! exactly that procedure; this project's own settings panel says "1 fits most
//! games". [`rustynes_probe::latency`] measures it instead, by replaying one
//! anchor with a button held and without it and finding the first frame that
//! differs.
//!
//! # Two deliberate choices
//!
//! **It recommends; it does not apply.** A measured depth is never written to
//! the config on its own. Run-ahead is linear in the core's frame cost (roughly
//! 34% / 52% / 78% of the NTSC budget at depth 0 / 1 / 2), so silently raising it
//! can push a marginal host into dropped frames for a change the user never
//! asked for. The number appears with an explicit **Apply** button next to it.
//!
//! **It reports its own uncertainty.** The measurement returns `None` rather
//! than a guess whenever the probe buttons disagree or nothing reacts, and this
//! panel shows that as "inconclusive" with the per-button evidence — never as
//! "0 frames". A latency tool that cannot say "I don't know" is worse than none,
//! because its wrong answers are indistinguishable from its right ones.
//!
//! The measurement runs **synchronously under the emu lock** on the button
//! press, like `BasicBot`'s search, and restores the live timeline before
//! returning. It drives several hundred frames, so the UI pauses briefly; the
//! button says so rather than pretending the work is free.

use rustynes_core::Nes;
use rustynes_probe::latency::{self, Confidence, LatencyConfig, LatencyReport};

use crate::icons::{glyph, label as ic};

/// Highest depth this panel will ever recommend.
///
/// A game measuring higher than this is reported honestly and the recommendation
/// clamped, rather than the measurement being silently discarded.
///
/// Re-exported from [`crate::emu`] rather than declared as its own `3`: that
/// constant exists precisely because `effective_run_ahead`'s cap and the
/// throttle's cap were once separate literals that drifted apart (PR #358), and
/// a third copy here would reopen the same seam. (PR #385 review.)
use crate::emu::MAX_RUN_AHEAD_DEPTH as MAX_DEPTH;

/// Persistent panel state.
#[derive(Default)]
pub struct LatencyPanel {
    /// The most recent measurement, if one has been run for this session.
    report: Option<LatencyReport>,
    /// Milliseconds per frame **of the console the report was measured on**,
    /// captured at measurement time from `Nes::frame_duration`.
    ///
    /// Recorded here rather than read at render time because it is a property of
    /// the measurement, not of the current session: unloading the ROM, or
    /// loading a PAL one after measuring an NTSC one, must not silently restate
    /// an old result in the new region's units.
    frame_ms: f64,
    /// "Measure" was clicked this frame; [`show`] runs it after the render, so
    /// `nes` is never captured by the viewport callback.
    measure_requested: bool,
    /// A depth the user asked to apply; drained by the caller into the config.
    pending_apply: Option<u32>,
    /// Status / error line.
    status: String,
    /// v2.3.9 — what has already been written to the config this session.
    ///
    /// Session-only, never serialized. Two jobs, and the second is why it is a
    /// value rather than a bool: it makes "has anything changed?" answerable
    /// WITHOUT computing the ROM key, and that key is a SHA-256 hex string
    /// allocated fresh each time. Computing it unconditionally allocated once
    /// per frame for the whole time the panel was open. (Review on #410.)
    persisted: Option<crate::config::RememberedLatency>,
}

impl LatencyPanel {
    /// Discard everything bound to the previous ROM.
    ///
    /// A latency report describes one game. Left standing across a ROM change it
    /// becomes a confident statement about a cartridge it was never measured on
    /// — and worse, its **Apply** button stays live, so a depth measured for game
    /// A is one click from being applied while game B is running. Clearing
    /// `pending_apply` matters as much as clearing `report`.
    ///
    /// Called from the same ROM-transition points that end a `TAStudio` session,
    /// for the same reason: that state anchored on an emulator instance which no
    /// longer exists. (PR #385 review.)
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// v2.3.9 — the measurement worth remembering across sessions, if any.
    ///
    /// `None` for an inconclusive report, deliberately: a stored "I could not
    /// tell" is indistinguishable from a stored answer once it has lost the
    /// context that produced it, and keeping those apart is this panel's whole
    /// discipline. Better to re-measure than to show a remembered shrug.
    ///
    /// The frame duration travels with it so the millisecond figure is later
    /// re-derived under the region it was MEASURED in — storing milliseconds
    /// would silently restate a PAL measurement in NTSC units.
    #[must_use]
    pub fn remembered(&self) -> Option<crate::config::RememberedLatency> {
        let report = self.report.as_ref()?;
        let frames = report.frames?;
        if report.confidence == Confidence::Inconclusive {
            return None;
        }
        Some(crate::config::RememberedLatency {
            frames,
            unanimous: report.confidence == Confidence::Unanimous,
            // Microseconds from the frame duration captured at measurement time.
            // `frame_ms` is only ever set from `Nes::frame_duration`, so this is
            // a unit conversion rather than a new source of truth.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a console frame is ~16-20 ms; 1000x that is far inside u32"
            )]
            frame_micros: (self.frame_ms * 1000.0) as u32,
        })
    }

    /// v2.3.9 — the measurement to write to the config, if it differs from what
    /// is already there.
    ///
    /// Returns `Some` at most once per distinct result, so the caller writes and
    /// saves on a real change instead of every frame. Marks it persisted as a
    /// side effect, which is why it takes `&mut self` — a pure query would leave
    /// the caller to remember, and the caller forgetting is exactly how the
    /// original version of this feature never wrote anything at all.
    #[must_use]
    pub fn take_unpersisted(&mut self) -> Option<crate::config::RememberedLatency> {
        let current = self.remembered()?;
        if self.persisted == Some(current) {
            return None;
        }
        self.persisted = Some(current);
        Some(current)
    }

    /// Whether this session already holds a report — measured or remembered.
    ///
    /// The restore is driven off this rather than off a ROM-load hook, so it
    /// self-corrects: `clear_rom_bound_analysis` empties the report at every ROM
    /// transition, and the next frame repopulates it from the NEW ROM's entry.
    /// One condition instead of a fourth per-panel clear plus a fourth restore.
    #[must_use]
    pub const fn has_report(&self) -> bool {
        self.report.is_some()
    }

    /// v2.3.9 — restore a remembered measurement for a freshly-loaded ROM.
    ///
    /// Populates the report so the panel opens showing what was measured last
    /// time, and **nothing else**: no `pending_apply`, so a remembered depth can
    /// never be applied without the user pressing Apply in this session. That is
    /// the same separation the live path keeps, extended across a restart —
    /// persisting a recommendation must not quietly become persisting a setting.
    pub fn restore_remembered(&mut self, r: crate::config::RememberedLatency) {
        self.report = Some(LatencyReport {
            frames: Some(r.frames),
            confidence: if r.unanimous {
                Confidence::Unanimous
            } else {
                Confidence::Majority
            },
            // Zeroed rather than invented. These describe the RUN that produced
            // the measurement — which buttons reacted, which observable answered,
            // how many trials were spent — and no honest value exists for them in
            // a later session. The panel renders them as "0/0 buttons, 0 trials",
            // which reads as "not from this session" and is exactly right.
            reacting_buttons: 0,
            probed_buttons: 0,
            observable: None,
            per_button: Vec::new(),
            trials_used: 0,
        });
        self.frame_ms = f64::from(r.frame_micros) / 1000.0;
        // Already in the config — that is where it came from — so record it as
        // persisted. Without this the next frame would treat it as a fresh
        // measurement and write it straight back, saving the file for nothing.
        self.persisted = Some(r);
        "Remembered from a previous session.".clone_into(&mut self.status);
    }

    /// Take a depth the user pressed **Apply** for, if any.
    ///
    /// Returned rather than written here because the panel has no business
    /// touching the config: the caller owns that, and routing it through a
    /// drained field keeps "measured" and "applied" as two separate, auditable
    /// steps.
    pub const fn take_pending_apply(&mut self) -> Option<u32> {
        self.pending_apply.take()
    }
}

/// Draw the Latency Oracle window. `nes` is `Some` only when a ROM is loaded
/// under the held lock; measuring is disabled otherwise.
pub fn show(
    ctx: &egui::Context,
    detached: &mut std::collections::HashSet<&'static str>,
    open: &mut bool,
    state: &mut LatencyPanel,
    nes: Option<&mut Nes>,
    current_run_ahead: u32,
    render_work: crate::perf::IntervalStats,
) {
    let can_measure = nes.is_some();
    super::detachable_window(
        ctx,
        detached,
        "latency_oracle",
        "Latency Oracle",
        super::WindowCfg {
            default_width: Some(360.0),
            ..Default::default()
        },
        open,
        |ui| body(ui, state, can_measure, current_run_ahead, render_work),
    );
    // Measure AFTER the render — `nes` is free here, not captured by any closure.
    if std::mem::take(&mut state.measure_requested) {
        run_measurement(state, nes);
    }
}

/// The panel body, shared by the docked window and the detached OS viewport.
fn body(
    ui: &mut egui::Ui,
    state: &mut LatencyPanel,
    can_measure: bool,
    current: u32,
    render_work: crate::perf::IntervalStats,
) {
    ui.label("Measures how many frames this game waits before acting on input.");
    ui.weak(
        "Replays the current moment twice — once with a button held, once without — \
         and finds the first frame that differs. Briefly pauses the emulator.",
    );
    ui.separator();

    // `icons::label` with a `glyph::` constant, NOT a literal codepoint. The
    // button read `"\u{23F1} Measure now"` — U+23F1 STOPWATCH, an emoji, which
    // the project style rule forbids in code outright. `glyph::GAUGE` is a
    // private-use-area codepoint from the bundled icon font, and it is the same
    // glyph the Tools menu entry uses, so the button now matches the item that
    // opens it. (PR #385 review.)
    if ui
        .add_enabled(
            can_measure,
            egui::Button::new(ic(glyph::GAUGE, "Measure now")),
        )
        .clicked()
    {
        state.measure_requested = true;
    }
    if !can_measure {
        ui.weak("Load a ROM to measure.");
    }
    ui.weak(format!("Run-ahead is currently {current}."));

    if let Some(report) = &state.report {
        ui.separator();
        report_body(
            ui,
            report,
            current,
            state.frame_ms,
            render_work,
            &mut state.pending_apply,
        );
    }

    if !state.status.is_empty() {
        ui.separator();
        ui.weak(&state.status);
    }
}

/// Render a finished measurement: the verdict, the recommendation, the evidence.
fn report_body(
    ui: &mut egui::Ui,
    report: &LatencyReport,
    current: u32,
    frame_ms: f64,
    render_work: crate::perf::IntervalStats,
    pending_apply: &mut Option<u32>,
) {
    if let Some(frames) = report.frames {
        let plural = if frames == 1 { "frame" } else { "frames" };
        ui.label(format!("Internal lag: {frames} {plural}"));
        // The felt latency, which is what the user actually experiences.
        //
        // Derived from the console's own frame duration, NOT a hardcoded NTSC
        // 16.639. A literal here would overstate PAL and Dendy lag by 20.2% —
        // the identical defect v2.3.5 fixed in the libretro wrapper, where a
        // hardcoded 60.0988 fps had lost all connection to the constant it was
        // copied from and ran every PAL cartridge fast. (PR #385 review.)
        let ms = f64::from(frames) * frame_ms;
        ui.weak(format!("about {ms:.0} ms of the game's own delay"));
        end_to_end(ui, frames, current, frame_ms, render_work);

        let confidence = match report.confidence {
            Confidence::Unanimous => "every reacting button agreed",
            Confidence::Majority => "a majority agreed — treat as approximate",
            Confidence::Inconclusive => "inconclusive",
        };
        ui.weak(format!(
            "{confidence} ({}/{} buttons reacted, {} trials)",
            report.reacting_buttons, report.probed_buttons, report.trials_used
        ));

        if let Some(depth) = report.suggested_run_ahead(MAX_DEPTH) {
            ui.separator();
            ui.horizontal(|ui| {
                if depth == current {
                    ui.label(format!("Run-ahead {depth} already matches."));
                } else {
                    ui.label(format!("Recommended run-ahead: {depth}"));
                    // Explicit, never automatic — see the module docs.
                    if ui.button(format!("Apply {depth}")).clicked() {
                        *pending_apply = Some(depth);
                    }
                }
            });
            if frames > MAX_DEPTH {
                ui.weak(format!(
                    "Measured {frames}, but run-ahead is capped at {MAX_DEPTH}; \
                         each extra frame costs roughly a whole frame of emulation."
                ));
            }
        }
    } else {
        ui.label("Inconclusive — no run-ahead change recommended.");
        ui.weak(match report.reacting_buttons {
            0 => "Nothing reacted to any button inside the probe window. Try \
                  measuring during gameplay rather than on a title screen or \
                  cut-scene."
                .to_owned(),
            n => format!(
                "{n} of {} buttons reacted, but they disagreed on when — so there \
                 is no single lag to report.",
                report.probed_buttons
            ),
        });
    }

    // The evidence, always — including for a confident result. A tool that shows
    // only its conclusion cannot be checked.
    ui.collapsing("Per-button evidence", |ui| {
        const NAMES: [&str; 6] = ["Right", "Left", "Down", "Up", "A", "B"];
        for (name, d) in NAMES.iter().zip(report.per_button.iter()) {
            match d {
                Some(f) => ui.label(format!("{name}: reacted on frame {f}")),
                None => ui.weak(format!("{name}: no reaction")),
            };
        }
        if let Some(obs) = report.observable {
            ui.weak(format!("decided on: {obs:?}"));
        }
    });
}

/// Run the measurement against the live emulator, recording a status line.
fn run_measurement(state: &mut LatencyPanel, nes: Option<&mut Nes>) {
    let Some(nes) = nes else {
        "No ROM loaded.".clone_into(&mut state.status);
        return;
    };
    // Captured BEFORE the measurement, from the console that is about to be
    // measured — see `LatencyPanel::frame_ms`.
    state.frame_ms = nes.frame_duration().as_secs_f64() * 1000.0;
    // `measure_in_place` snapshots, replays, and restores — the live timeline is
    // exactly where it was when this returns.
    let report = latency::measure_in_place(nes, LatencyConfig::default());
    state.status = format!("Measured in {} trials.", report.trials_used);
    state.report = Some(report);
}

/// v2.3.9 item C (interim) — the game's delay plus the renderer's own work.
///
/// # What this deliberately is NOT
///
/// It is **not** the end-to-end wall-clock delay, and the label says so. Two
/// costs are excluded and both are excluded for the same reason: `render_wait`
/// (the blocking present) and `render_lock` (mutex contention) are their own
/// percentile series, and adding two p95s is not the p95 of the sum.
///
/// That is the error `RenderPerf::work` already exists to avoid — it is kept as
/// a real per-redraw series precisely because `total p95 - wait p95` produced a
/// published table whose `work p95` sat below its `work p50`. Summing has the
/// same defect as differencing; only the direction changes.
///
/// What makes THIS sum legitimate is that the lag term is a **constant**, not a
/// distribution: adding a constant shifts every percentile by exactly that
/// constant, so `lag + work_p95` is a genuine p95 of `lag + work`. Extending it
/// to a second series would break that, which is why the honest figure is the
/// narrow one.
///
/// A true wall-clock figure needs a single per-redraw end-to-end series recorded
/// as one sample — an addition to `RenderPerf`, not arithmetic over what exists.
/// See `to-dos/plans/v2.3.9-crucible-plan.md` item C.
fn end_to_end(
    ui: &mut egui::Ui,
    frames: u32,
    current_run_ahead: u32,
    frame_ms: f64,
    render_work: crate::perf::IntervalStats,
) {
    match end_to_end_figure(frames, current_run_ahead, frame_ms, render_work) {
        EndToEnd::Unavailable { samples, need } => {
            ui.weak(format!(
                "end-to-end unavailable: {samples} render samples, need {need}"
            ));
        }
        EndToEnd::Ms { total, effective } => {
            ui.label(format!(
                "Game delay + render work: about {total:.0} ms (p95)"
            ));
            if current_run_ahead > 0 {
                ui.weak(format!(
                    "{effective} of {frames} frames remain after run-ahead {current_run_ahead}"
                ));
            }
            ui.weak("Excludes the vblank wait and lock contention — see the panel docs.");
        }
    }
}

/// A percentile over a handful of redraws is noise wearing a number's clothing.
const MIN_RENDER_SAMPLES: usize = 60;

/// The interim end-to-end figure, or why there isn't one.
///
/// Extracted from the render so the arithmetic is testable rather than trapped
/// in an `egui` closure — the same reason the Divergence Lens lifts its verdict
/// wording out. The two branches are what is worth pinning: a figure and a
/// refusal must stay distinguishable.
#[derive(Debug, Clone, Copy, PartialEq)]
enum EndToEnd {
    /// Too few render samples for a percentile to mean anything.
    Unavailable { samples: usize, need: usize },
    /// Milliseconds, with the frame count left after run-ahead.
    Ms { total: f64, effective: u32 },
}

fn end_to_end_figure(
    frames: u32,
    current_run_ahead: u32,
    frame_ms: f64,
    render_work: crate::perf::IntervalStats,
) -> EndToEnd {
    if render_work.count < MIN_RENDER_SAMPLES {
        return EndToEnd::Unavailable {
            samples: render_work.count,
            need: MIN_RENDER_SAMPLES,
        };
    }
    // Run-ahead removes up to `depth` frames of the game's own lag, so a figure
    // that ignores it overstates what the user experiences — and overstates it
    // worst exactly when they have taken this panel's advice. Saturating, not
    // wrapping: a depth above the measured lag leaves zero, not `u32::MAX`.
    let effective = frames.saturating_sub(current_run_ahead);
    EndToEnd::Ms {
        total: f64::from(effective) * frame_ms + f64::from(render_work.p95_ms),
        effective,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conclusive measurement round-trips, and the frame duration travels
    /// with it so the millisecond figure is later re-derived under the region it
    /// was MEASURED in — not whatever is loaded now.
    #[test]
    fn a_conclusive_measurement_round_trips_with_its_region() {
        let panel = LatencyPanel {
            report: Some(report(Some(3), Confidence::Unanimous)),
            frame_ms: 19.997, // PAL
            ..LatencyPanel::default()
        };

        let remembered = panel.remembered().expect("conclusive results persist");
        assert_eq!(remembered.frames, 3);
        assert!(remembered.unanimous);
        assert_eq!(remembered.frame_micros, 19_997);

        let mut fresh = LatencyPanel::default();
        fresh.restore_remembered(remembered);
        assert!(fresh.has_report());
        assert!(
            (fresh.frame_ms - 19.997).abs() < 1e-9,
            "a PAL measurement must not be restated in NTSC units"
        );
    }

    /// An inconclusive report is NOT remembered. A stored "I could not tell" is
    /// indistinguishable from a stored answer once it has lost its context, and
    /// keeping those apart is this panel's whole discipline.
    ///
    /// **Two cases, because one of them passed for the wrong reason.** With
    /// `frames: None` the `?` returns before the confidence is ever examined, so
    /// that case alone leaves the confidence guard untested — a mutation deleting
    /// it changed nothing. The second case pins the guard itself.
    #[test]
    fn an_inconclusive_measurement_is_not_remembered() {
        let mut panel = LatencyPanel {
            report: Some(report(None, Confidence::Inconclusive)),
            frame_ms: 16.639,
            ..LatencyPanel::default()
        };
        assert!(
            panel.remembered().is_none(),
            "no frame count, nothing to store"
        );

        // Inconclusive DESPITE a frame count. Defensive rather than observed —
        // the measurement path reports `None` frames when it cannot tell — but
        // the guard exists precisely so a future path that sets both cannot
        // quietly persist a shrug, and an untested guard is not a guard.
        panel.report = Some(report(Some(4), Confidence::Inconclusive));
        assert!(
            panel.remembered().is_none(),
            "a frame count does not rescue an inconclusive verdict"
        );
    }

    /// A report with a confidence but no frame count is likewise not
    /// remembered — `frames: None` and `Some(0)` are different answers, and only
    /// one of them is worth carrying to a later session.
    #[test]
    fn a_report_without_a_frame_count_is_not_remembered() {
        let panel = LatencyPanel {
            report: Some(report(None, Confidence::Majority)),
            ..LatencyPanel::default()
        };
        assert!(panel.remembered().is_none());
    }

    /// The write must happen at most once per distinct result.
    ///
    /// The original version wrote on every frame the panel was open and never
    /// called `save()`, so it rewrote an in-memory map continuously and
    /// persisted nothing. Both halves are pinned here: `take_unpersisted` yields
    /// once and then stops, so the caller's `save()` cannot become a per-frame
    /// file write.
    #[test]
    fn a_measurement_is_offered_for_writing_exactly_once() {
        let mut panel = LatencyPanel {
            report: Some(report(Some(3), Confidence::Unanimous)),
            frame_ms: 16.639,
            ..LatencyPanel::default()
        };

        let first = panel.take_unpersisted();
        assert!(first.is_some(), "a fresh measurement must be offered");
        assert!(
            panel.take_unpersisted().is_none(),
            "an unchanged measurement must not be rewritten every frame"
        );

        // A NEW measurement supersedes it and is offered again.
        panel.report = Some(report(Some(1), Confidence::Unanimous));
        let second = panel
            .take_unpersisted()
            .expect("a changed result is offered");
        assert_eq!(second.frames, 1);
    }

    /// A RESTORED measurement came from the config, so it must not be written
    /// back — otherwise opening a game would rewrite the file for nothing.
    #[test]
    fn a_restored_measurement_is_not_written_back() {
        let mut panel = LatencyPanel::default();
        panel.restore_remembered(crate::config::RememberedLatency {
            frames: 2,
            unanimous: true,
            frame_micros: 16_639,
        });
        assert!(
            panel.take_unpersisted().is_none(),
            "a value read from the config must not be offered back to it"
        );
    }

    /// Restoring must NOT queue an Apply. Remembering a recommendation across a
    /// restart must never quietly become applying it — the same separation the
    /// live path keeps between "measured" and "applied".
    #[test]
    fn restoring_never_queues_an_apply() {
        let mut panel = LatencyPanel::default();
        panel.restore_remembered(crate::config::RememberedLatency {
            frames: 2,
            unanimous: true,
            frame_micros: 16_639,
        });
        assert!(
            panel.take_pending_apply().is_none(),
            "a remembered depth is one click from being applied, never zero"
        );
    }

    fn work(count: usize, p95_ms: f32) -> crate::perf::IntervalStats {
        crate::perf::IntervalStats {
            count,
            p95_ms,
            ..crate::perf::IntervalStats::default()
        }
    }

    /// Too few samples must produce a REFUSAL, not a small number. A percentile
    /// over a handful of redraws is noise, and printing it would be this panel
    /// doing the exact thing it exists to refuse.
    #[test]
    fn too_few_render_samples_declines_rather_than_guessing() {
        assert_eq!(
            end_to_end_figure(4, 0, 16.639, work(MIN_RENDER_SAMPLES - 1, 3.0)),
            EndToEnd::Unavailable {
                samples: MIN_RENDER_SAMPLES - 1,
                need: MIN_RENDER_SAMPLES,
            }
        );
    }

    /// The arithmetic: a CONSTANT lag plus ONE percentile series. Valid only
    /// because adding a constant shifts every percentile by exactly that
    /// constant — which is why a second series may never be added here.
    #[test]
    fn the_figure_is_lag_plus_one_series() {
        let EndToEnd::Ms { total, effective } = end_to_end_figure(4, 0, 16.0, work(600, 3.5))
        else {
            panic!("expected a figure");
        };
        assert_eq!(effective, 4);
        assert!(
            (total - (4.0 * 16.0 + 3.5)).abs() < 1e-6,
            "expected 67.5, got {total}"
        );
    }

    /// Run-ahead removes frames of the game's own lag, so the figure must shrink
    /// by exactly one frame per depth — otherwise the panel overstates latency
    /// worst for the users who took its advice.
    #[test]
    fn run_ahead_is_subtracted_frame_for_frame() {
        let (base, with_two) = (
            end_to_end_figure(4, 0, 16.0, work(600, 0.0)),
            end_to_end_figure(4, 2, 16.0, work(600, 0.0)),
        );
        let (
            EndToEnd::Ms { total: a, .. },
            EndToEnd::Ms {
                total: b,
                effective,
            },
        ) = (base, with_two)
        else {
            panic!("expected figures");
        };
        assert_eq!(effective, 2);
        assert!(
            (a - b - 32.0).abs() < 1e-6,
            "two frames at 16 ms = 32 ms; got {a} vs {b}"
        );
    }

    /// A depth ABOVE the measured lag leaves zero, never a wrapped `u32::MAX`.
    #[test]
    fn run_ahead_deeper_than_the_lag_saturates_at_zero() {
        let EndToEnd::Ms { total, effective } = end_to_end_figure(1, 3, 16.0, work(600, 2.0))
        else {
            panic!("expected a figure");
        };
        assert_eq!(effective, 0);
        assert!((total - 2.0).abs() < 1e-6, "only render work remains");
    }

    fn report(frames: Option<u32>, confidence: Confidence) -> LatencyReport {
        LatencyReport {
            frames,
            confidence,
            reacting_buttons: 6,
            probed_buttons: 6,
            observable: None,
            per_button: vec![frames; 6],
            trials_used: 7,
        }
    }

    /// THE property this panel exists to preserve: a measurement never changes
    /// the user's setting by itself. `pending_apply` is only ever set by the
    /// Apply button, so a freshly-stored report leaves it empty.
    #[test]
    fn a_measurement_alone_never_requests_an_apply() {
        let mut panel = LatencyPanel {
            report: Some(report(Some(2), Confidence::Unanimous)),
            ..LatencyPanel::default()
        };
        assert_eq!(
            panel.take_pending_apply(),
            None,
            "storing a report queued a run-ahead change the user never asked for"
        );
    }

    /// An inconclusive report must offer no depth at all — not zero.
    #[test]
    fn an_inconclusive_report_recommends_nothing() {
        let r = report(None, Confidence::Inconclusive);
        assert_eq!(r.suggested_run_ahead(MAX_DEPTH), None);
    }

    /// A measured lag deeper than the cap is still reported, with the
    /// recommendation clamped rather than the measurement thrown away.
    #[test]
    fn a_deep_measurement_is_clamped_not_discarded() {
        let r = report(Some(7), Confidence::Unanimous);
        assert_eq!(r.frames, Some(7));
        assert_eq!(r.suggested_run_ahead(MAX_DEPTH), Some(MAX_DEPTH));
    }

    /// The felt-latency read-out must be a function of the console's frame
    /// duration, not a constant. Hardcoding NTSC's 16.639 ms makes this fail:
    /// PAL and Dendy would report the same milliseconds as NTSC for the same
    /// frame count, understating them by 20.2%.
    #[test]
    fn felt_milliseconds_track_the_region_not_a_constant() {
        let ms_of = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        let ntsc = ms_of(rustynes_core::FRAME_DURATION_NTSC);
        let pal = ms_of(rustynes_core::FRAME_DURATION_PAL);
        assert!(
            (f64::from(3_u32) * pal - f64::from(3_u32) * ntsc).abs() > 1.0,
            "a three-frame lag must read differently on PAL than on NTSC; \
             identical output means the conversion is hardcoded"
        );
    }

    /// A ROM transition must discard the whole measurement — and `pending_apply`
    /// especially. A report left standing describes a cartridge that is no
    /// longer loaded; a `pending_apply` left standing would apply the previous
    /// game's depth to the new one.
    #[test]
    fn clearing_discards_the_report_and_any_queued_apply() {
        let mut panel = LatencyPanel {
            report: Some(report(Some(2), Confidence::Unanimous)),
            pending_apply: Some(2),
            frame_ms: 16.639,
            status: "Measured in 7 trials.".to_owned(),
            measure_requested: true,
            persisted: Some(crate::config::RememberedLatency {
                frames: 2,
                unanimous: true,
                frame_micros: 16_639,
            }),
        };
        panel.clear();
        assert!(
            panel.report.is_none(),
            "a stale report survived a ROM change"
        );
        // v2.3.9 — and the write-tracking, which is keyed to the OLD ROM. Left
        // standing, the next game's first identical measurement would be treated
        // as already written and never reach the config.
        assert!(
            panel.take_unpersisted().is_none(),
            "no report, so nothing to offer"
        );
        assert_eq!(
            panel.take_pending_apply(),
            None,
            "the previous game's run-ahead depth was still queued to apply"
        );
        assert!(panel.status.is_empty());
        assert!(!panel.measure_requested);
    }

    /// `take_pending_apply` drains, so one Apply click cannot be consumed twice
    /// and re-applied on a later frame.
    #[test]
    fn a_pending_apply_is_drained_exactly_once() {
        let mut panel = LatencyPanel {
            pending_apply: Some(2),
            ..LatencyPanel::default()
        };
        assert_eq!(panel.take_pending_apply(), Some(2));
        assert_eq!(panel.take_pending_apply(), None);
    }
}
