//! Performance panel (v2.8.0 Phase 0).
//!
//! Read-only view of the [`crate::perf::PerfView`] snapshot the app pushes
//! each pacer iteration (the `set_fps` push pattern). Three interval tables
//! make the pacing story visible:
//!
//! - **Produced** — the pacer's output cadence. The sleep-then-spin pacer
//!   makes this near-perfect, which is exactly why it alone proves nothing.
//! - **Presented** — what the display actually samples. Judder lives in the
//!   p95/p99/max of THIS row.
//! - **Produce cost** — wall time inside `produce_one_frame`; the budget
//!   run-ahead (Phase 3) and the display-sync pacing mode (Phase 2) spend.
//!
//! plus the audio-queue health (occupancy vs the soft cap, underrun /
//! overrun counters — the 10-minute soak gate watches these stay flat) and
//! pacer anomaly counters.

use crate::perf::{IntervalStats, PerfView};

/// Persistent panel state — the latest pushed snapshot.
#[derive(Debug, Default)]
pub struct PerfPanelState {
    view: PerfView,
    /// v2.8.0 — the "Logging" checkbox (session-only; default OFF). While
    /// set, the app's `PerfLogger` appends interval CSV rows under
    /// `perf-logs/`. Native-only — wasm has no filesystem.
    #[cfg(not(target_arch = "wasm32"))]
    pub logging: bool,
    /// Status line under the checkbox (destination path / error), pushed by
    /// the app from its `PerfLogger`.
    #[cfg(not(target_arch = "wasm32"))]
    log_note: Option<String>,
}

impl PerfPanelState {
    /// Replace the rendered snapshot (called via
    /// `DebuggerOverlay::set_perf_view` from the app's pacer).
    pub fn set_view(&mut self, view: PerfView) {
        self.view = view;
    }

    /// Update the logging status line (destination path / error).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_log_note(&mut self, note: Option<String>) {
        self.log_note = note;
    }
}

/// One interval-stats table row.
fn stats_row(ui: &mut egui::Ui, label: &str, s: &IntervalStats, target_ms: f32) {
    ui.label(label);
    if s.count == 0 {
        ui.label("-");
        ui.label("-");
        ui.label("-");
        ui.label("-");
        ui.label("-");
        ui.end_row();
        return;
    }
    ui.label(format!("{:.2}", s.mean_ms));
    ui.label(format!("{:.2}", s.p50_ms));
    ui.label(format!("{:.2}", s.p95_ms));
    // p99 + max get attention colors when they blow past the frame target
    // (the visible-judder thresholds).
    let warn = egui::Color32::from_rgb(0xF0, 0xC0, 0x40);
    let bad = egui::Color32::from_rgb(0xE0, 0x40, 0x40);
    let color_for = |v: f32| {
        if target_ms > 0.0 && v > target_ms * 1.5 {
            Some(bad)
        } else if target_ms > 0.0 && v > target_ms * 1.1 {
            Some(warn)
        } else {
            None
        }
    };
    let colored = |ui: &mut egui::Ui, v: f32| match color_for(v) {
        Some(c) => {
            ui.colored_label(c, format!("{v:.2}"));
        }
        None => {
            ui.label(format!("{v:.2}"));
        }
    };
    colored(ui, s.p99_ms);
    colored(ui, s.max_ms);
    ui.end_row();
}

/// Render the Performance panel window.
// On wasm the "Logging" checkbox block is compiled out, leaving `state`
// never written — keep the signature uniform across targets.
#[cfg_attr(target_arch = "wasm32", allow(clippy::needless_pass_by_ref_mut))]
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut PerfPanelState) {
    // Cloned so the closure below can also borrow the checkbox mutably.
    let v = state.view.clone();
    egui::Window::new("Performance")
        .open(open)
        .default_pos([480.0, 64.0])
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(format!(
                "target: {:.3} ms/frame   pacing: {}   present mode: {}{}",
                v.target_ms,
                v.pacing,
                v.present_mode,
                if v.present_mode_fell_back {
                    "  (FALLBACK)"
                } else {
                    ""
                }
            ));
            ui.separator();

            egui::Grid::new("perf-intervals")
                .num_columns(6)
                .spacing([12.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("interval (ms)").strong());
                    ui.label(egui::RichText::new("mean").strong());
                    ui.label(egui::RichText::new("p50").strong());
                    ui.label(egui::RichText::new("p95").strong());
                    ui.label(egui::RichText::new("p99").strong());
                    ui.label(egui::RichText::new("max").strong());
                    ui.end_row();
                    stats_row(ui, "produced", &v.produced, v.target_ms);
                    stats_row(ui, "presented", &v.presented, v.target_ms);
                    // The produce cost is a budget, not a cadence — color it
                    // against the full frame budget the same way.
                    stats_row(ui, "produce cost", &v.produce_cost, v.target_ms);
                });

            ui.separator();
            ui.label(format!(
                "pacer: catch-up bursts {}   snap-forwards {}",
                v.catchup_bursts, v.snap_forwards
            ));
            if let Some(gpu) = v.gpu_ms {
                ui.label(format!("gpu pass: {gpu:.3} ms (1-3 frames stale)"));
            }

            ui.separator();
            let a = &v.audio;
            if a.sample_rate == 0 {
                ui.label("audio: (no native stream)");
            } else {
                ui.label(format!(
                    "audio: {:.1} ms queued ({} samples @ {} Hz)",
                    a.queued_ms(),
                    a.queued_samples,
                    a.sample_rate
                ));
                let health = |ui: &mut egui::Ui, label: &str, n: u64| {
                    if n == 0 {
                        ui.label(format!("{label}: 0"));
                    } else {
                        ui.colored_label(
                            egui::Color32::from_rgb(0xE0, 0x40, 0x40),
                            format!("{label}: {n}"),
                        );
                    }
                };
                ui.horizontal(|ui| {
                    health(ui, "underruns", a.underruns);
                    ui.separator();
                    health(ui, "overrun-dropped samples", a.overrun_dropped);
                });
            }

            // v2.8.0 — opt-in interval CSV logging of everything this panel
            // shows (plus the run configuration in the file header), for
            // offline performance analysis. Native-only (file I/O).
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.separator();
                ui.checkbox(&mut state.logging, "Logging").on_hover_text(
                    "Append a CSV row of these stats every second to \
                         perf-logs/ (with the game + configuration in the \
                         header). Session-only; off by default.",
                );
                if let Some(note) = &state.log_note {
                    ui.label(egui::RichText::new(note).weak().small());
                }
            }
        });
}
