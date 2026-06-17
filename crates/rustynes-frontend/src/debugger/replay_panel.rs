//! Replay / TAS window (v1.5.0 "Lens" Workstream C2).
//!
//! A dedicated Tools window for the `.rnm` TAS-movie machinery, modelled on
//! `GeraNES`'s `ReplayWindowUI`. It surfaces what the status-bar HUD cannot fit:
//!
//! - the current mode (Idle / Recording / Playing) + frame cursor / total,
//! - a **device topology** read-out (the controller / peripheral on each port,
//!   and whether the Four Score adapter multiplexes P1..P4),
//! - a **timebase** read-out (region + whole-Hz estimate + elapsed / total
//!   wall-clock time derived from the frame cursor),
//! - **branch / seek UX**: Record / Play / Branch / Stop buttons (mirroring the
//!   F6/F7/F8 shortcuts) plus a seek-to-frame slider and a single-step button
//!   for playback.
//!
//! The window is purely a control + read-out surface. It mutates no emulator
//! state directly: every action is recorded as a [`ReplayRequest`] that the app
//! drains after the egui pass (the same `take_*_request` pattern the netplay /
//! cheevos panels use) and dispatches under the emu lock. The seek itself
//! re-derives state by replaying the recorded inputs (`MovieUi::seek_playback`),
//! so replay stays bit-identical — no new determinism surface.

use crate::movie_ui::{MovieMode, MovieStatus, ReplayInfo};

/// A user action requested from the Replay window, drained by the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayRequest {
    /// Toggle recording (mirrors F6 / `MovieRecordToggle`).
    RecordToggle,
    /// Toggle playback (mirrors F7 / `MoviePlayToggle`).
    PlayToggle,
    /// Branch the current state into a new recording (mirrors F8).
    Branch,
    /// Stop whatever movie activity is active (record discard / playback stop).
    Stop,
    /// Seek the active playback to this absolute frame index (deterministic
    /// re-derive; clamped to the movie length by the app).
    Seek(usize),
}

/// Persistent Replay-window state (the pending request + the seek-slider value).
#[derive(Default)]
pub struct ReplayPanelState {
    status: MovieStatus,
    info: ReplayInfo,
    /// Scratch value bound to the seek slider while the user drags it.
    seek_target: usize,
    /// The pending action, taken by the app each frame.
    request: Option<ReplayRequest>,
}

impl ReplayPanelState {
    /// Push the latest movie status + topology/timebase snapshot (called from
    /// the app's pacer alongside the other panel feeds).
    pub fn set(&mut self, status: MovieStatus, info: ReplayInfo) {
        self.status = status;
        self.info = info;
    }

    /// Return (and clear) the pending user request.
    pub fn take_request(&mut self) -> Option<ReplayRequest> {
        self.request.take()
    }
}

/// Format `frames` at `hz` as `MM:SS.mmm` wall-clock (a display estimate).
fn fmt_time(frames: usize, hz: u32) -> String {
    if hz == 0 {
        return "--:--".to_owned();
    }
    let total_ms = (frames as u64 * 1000) / u64::from(hz);
    let ms = total_ms % 1000;
    let secs = (total_ms / 1000) % 60;
    let mins = total_ms / 60_000;
    format!("{mins:02}:{secs:02}.{ms:03}")
}

#[allow(clippy::too_many_lines)]
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut ReplayPanelState) {
    let status = state.status;
    let info = state.info.clone();
    egui::Window::new("Replay / TAS")
        .open(open)
        .default_size([340.0, 300.0])
        .resizable(true)
        .show(ctx, |ui| {
            // --- Mode + progress ---
            let (mode_txt, mode_col) = match status.mode {
                MovieMode::Idle => ("Idle", egui::Color32::GRAY),
                MovieMode::Recording => ("Recording", egui::Color32::from_rgb(0xE0, 0x40, 0x40)),
                MovieMode::Playing => ("Playing", egui::Color32::from_rgb(0x40, 0xC0, 0x40)),
            };
            ui.horizontal(|ui| {
                ui.strong("Mode:");
                ui.colored_label(mode_col, mode_txt);
            });

            match status.mode {
                MovieMode::Recording => {
                    ui.label(format!("Recorded: {} frames", status.cursor));
                }
                MovieMode::Playing => {
                    let pct = if status.total == 0 {
                        0.0
                    } else {
                        status.cursor as f32 / status.total as f32
                    };
                    ui.add(
                        egui::ProgressBar::new(pct)
                            .text(format!("{} / {}", status.cursor, status.total)),
                    );
                }
                MovieMode::Idle => {
                    ui.weak("No movie loaded. Record (F6) or play (F7) a .rnm movie.");
                }
            }

            ui.separator();

            // --- Timebase ---
            egui::Grid::new("replay_timebase")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.strong("Region");
                    ui.label(format!("{} (~{} Hz)", info.region, info.region_hz));
                    ui.end_row();

                    match status.mode {
                        MovieMode::Recording => {
                            ui.strong("Elapsed");
                            ui.label(fmt_time(status.cursor, info.region_hz));
                            ui.end_row();
                        }
                        MovieMode::Playing => {
                            ui.strong("Time");
                            ui.label(format!(
                                "{} / {}",
                                fmt_time(status.cursor, info.region_hz),
                                fmt_time(status.total, info.region_hz)
                            ));
                            ui.end_row();
                        }
                        MovieMode::Idle => {}
                    }
                });

            ui.separator();

            // --- Device topology ---
            ui.strong("Port topology");
            egui::Grid::new("replay_topology")
                .num_columns(2)
                .show(ui, |ui| {
                    if info.four_score {
                        ui.label("Adapter");
                        ui.label("Four Score (P1..P4)");
                        ui.end_row();
                    }
                    ui.label("Port 1");
                    ui.label(info.port1);
                    ui.end_row();
                    ui.label("Port 2");
                    ui.label(info.port2);
                    ui.end_row();
                });

            ui.separator();

            // --- Controls ---
            ui.horizontal(|ui| {
                let rec = status.mode == MovieMode::Recording;
                if ui
                    .button(if rec { "⏹ Stop Rec" } else { "⏺ Record" })
                    .on_hover_text("Toggle TAS recording (F6)")
                    .clicked()
                {
                    state.request = Some(ReplayRequest::RecordToggle);
                }
                let playing = status.mode == MovieMode::Playing;
                if ui
                    .button(if playing { "⏹ Stop Play" } else { "▶ Play" })
                    .on_hover_text("Toggle TAS playback (F7)")
                    .clicked()
                {
                    state.request = Some(ReplayRequest::PlayToggle);
                }
                if ui
                    .add_enabled(
                        status.mode != MovieMode::Idle,
                        egui::Button::new("⑂ Branch"),
                    )
                    .on_hover_text("Branch the current state into a new recording (F8)")
                    .clicked()
                {
                    state.request = Some(ReplayRequest::Branch);
                }
            });

            // --- Seek (playback only) ---
            if status.mode == MovieMode::Playing && status.total > 0 {
                ui.add_space(4.0);
                ui.label("Seek");
                // Keep the slider tracking the live cursor unless the user is
                // dragging it.
                let last = status.total.saturating_sub(1);
                if state.seek_target > last {
                    state.seek_target = status.cursor.min(last);
                }
                let resp =
                    ui.add(egui::Slider::new(&mut state.seek_target, 0..=last).text("frame"));
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    state.request = Some(ReplayRequest::Seek(state.seek_target));
                }
                ui.horizontal(|ui| {
                    if ui.button("⏮ Start").clicked() {
                        state.seek_target = 0;
                        state.request = Some(ReplayRequest::Seek(0));
                    }
                    if ui.button("◀ -10").clicked() {
                        let t = status.cursor.saturating_sub(10);
                        state.seek_target = t;
                        state.request = Some(ReplayRequest::Seek(t));
                    }
                    if ui.button("+1 ▶").clicked() {
                        let t = (status.cursor + 1).min(status.total);
                        state.seek_target = t.min(last);
                        state.request = Some(ReplayRequest::Seek(t));
                    }
                    if ui.button("+10 ▶▶").clicked() {
                        let t = (status.cursor + 10).min(status.total);
                        state.seek_target = t.min(last);
                        state.request = Some(ReplayRequest::Seek(t));
                    }
                });
                ui.weak("Seeking re-derives state by replaying inputs — bit-identical.");
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_time_basics() {
        assert_eq!(fmt_time(0, 60), "00:00.000");
        assert_eq!(fmt_time(60, 60), "00:01.000");
        assert_eq!(fmt_time(90, 60), "00:01.500");
        assert_eq!(fmt_time(3600, 60), "01:00.000");
        assert_eq!(fmt_time(100, 0), "--:--");
    }

    #[test]
    fn request_round_trips() {
        let mut s = ReplayPanelState::default();
        assert_eq!(s.take_request(), None);
        s.request = Some(ReplayRequest::Seek(42));
        assert_eq!(s.take_request(), Some(ReplayRequest::Seek(42)));
        assert_eq!(s.take_request(), None);
    }
}
