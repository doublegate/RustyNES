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
}

impl LatencyPanel {
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
        |ui| body(ui, state, can_measure, current_run_ahead),
    );
    // Measure AFTER the render — `nes` is free here, not captured by any closure.
    if std::mem::take(&mut state.measure_requested) {
        run_measurement(state, nes);
    }
}

/// The panel body, shared by the docked window and the detached OS viewport.
fn body(ui: &mut egui::Ui, state: &mut LatencyPanel, can_measure: bool, current: u32) {
    ui.label("Measures how many frames this game waits before acting on input.");
    ui.weak(
        "Replays the current moment twice — once with a button held, once without — \
         and finds the first frame that differs. Briefly pauses the emulator.",
    );
    ui.separator();

    if ui
        .add_enabled(can_measure, egui::Button::new("\u{23F1} Measure now"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
