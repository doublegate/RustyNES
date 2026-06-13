#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::needless_pass_by_ref_mut
)]
//! Graphics / audio / rewind settings panel (v1.7.0 Sprint 3).
//!
//! Edits the `[graphics]`, `[audio]`, and `[rewind]` sections of the live
//! [`Config`] in place and (native only) persists the whole config to
//! `config.toml` via [`Config::save`], mirroring the input rebind modal's
//! "Save to disk" flow.
//!
//! Some settings apply live; some need a relaunch:
//!
//! - **NTSC filter** (`[graphics] ntsc_filter`): applied live. The gfx
//!   post-pass is binary (on/off) — the `"composite"` / `"rgb"` distinction
//!   is persisted but the simplified wgsl pass treats any non-`"off"` value
//!   the same, so toggling between them looks identical at runtime.
//! - **Rewind enabled** (`[rewind] enabled`): applied live — the running
//!   `Nes` arms / frees the rewind ring immediately.
//! - **Present mode** (`[graphics] present_mode`) and **sample rate**
//!   (`[audio] sample_rate`): persisted only, labelled "(restart to apply)"
//!   — both need a surface / audio-stream rebuild that the produce path
//!   doesn't do live.
//! - **Rewind window / keyframe period** (`[rewind] max_seconds`,
//!   `keyframe_period`): persisted only, labelled "(restart to apply)" —
//!   they size the ring at the point it is armed, so a live edit while
//!   rewind is already enabled does not resize the existing buffer.
//!
//! The on-disk schema and defaults are unchanged (the fields already
//! exist in [`Config`]); this panel only reads / edits / saves them.

use crate::config::Config;

/// Live-apply request drained by the app's produce path each frame.
///
/// Produced by edits in the settings panel and polled like the input rebind
/// panel's `take_bindings_dirty` flag. Each field flags one live-applicable
/// change since the last poll; persisted-only settings (present mode, sample
/// rate, rewind capacity) are not represented here because the app cannot
/// apply them without a surface / stream / ring rebuild.
#[derive(Debug, Default, Clone, Copy)]
pub struct SettingsApply {
    /// The NTSC filter toggle changed — the app should enable / disable the
    /// gfx post-pass to match `[graphics] ntsc_filter`.
    pub ntsc_filter: bool,
    /// The rewind `enabled` toggle changed — the app should arm / free the
    /// running `Nes` rewind ring to match `[rewind] enabled`.
    pub rewind_enabled: bool,
    /// v2.8.0 Phase 2 — the pacing mode changed; the app re-resolves the
    /// pacing regime (and the surface present mode) live.
    pub pacing_mode: bool,
}

impl SettingsApply {
    /// `true` if any live-applicable change is pending.
    #[must_use]
    pub const fn any(self) -> bool {
        self.ntsc_filter || self.rewind_enabled || self.pacing_mode
    }
}

/// Persistent state of the settings panel.
#[derive(Debug, Default)]
pub struct SettingsPanelState {
    /// Pending live-apply request, accumulated across edits and drained by
    /// the app via [`Self::take_apply`].
    apply: SettingsApply,
    /// Status text after a save / failure. Native-only — the wasm32 panel
    /// has no `config.toml` save path, so there is nothing to report.
    #[cfg(not(target_arch = "wasm32"))]
    status: String,
    /// Warning pushed by the app when the configured present mode was not
    /// supported by the surface and the swapchain fell back to `Fifo`
    /// (v2.8.0 Phase 0 — the fallback used to be silent, leaving the
    /// resulting pacer-vs-vsync beat judder unattributable).
    present_mode_warning: Option<String>,
}

impl SettingsPanelState {
    /// Return (and clear) the pending live-apply request.
    pub fn take_apply(&mut self) -> SettingsApply {
        core::mem::take(&mut self.apply)
    }

    /// Set (or clear) the present-mode fallback warning shown beside the
    /// present-mode selector. Pushed by the app after gfx init.
    pub fn set_present_mode_warning(&mut self, warning: Option<String>) {
        self.present_mode_warning = warning;
    }
}

/// Render the settings panel. Edits `config` in place; native builds persist
/// the whole config to `config.toml` on the "Save to config.toml" button.
#[cfg(not(target_arch = "wasm32"))]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut SettingsPanelState,
    config: &mut Config,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([420.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, state, config);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save to config.toml").clicked() {
                    match config.save() {
                        Ok(()) => state.status = "Saved.".into(),
                        Err(e) => state.status = format!("save error: {e}"),
                    }
                }
            });
            if !state.status.is_empty() {
                ui.label(state.status.clone());
            }
        });
}

/// wasm32 variant: identical UI but no filesystem persistence (the panel
/// still edits the in-memory config, and the live-apply flags still fire).
#[cfg(target_arch = "wasm32")]
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut SettingsPanelState,
    config: &mut Config,
) {
    egui::Window::new("Settings")
        .open(open)
        .default_pos([560.0, 64.0])
        .default_size([420.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            body(ui, state, config);
            ui.separator();
            ui.label("(config save unavailable on web — changes are in-memory only)");
        });
}

/// The window body: graphics / audio / rewind sections. Mutates `config`
/// directly and accumulates live-apply flags on `state.apply`.
fn body(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    ui.heading("Graphics");

    // Present mode: persisted only — a live change needs a surface rebuild.
    ui.horizontal(|ui| {
        ui.label("Present mode");
        egui::ComboBox::from_id_salt("settings-present-mode")
            .selected_text(config.graphics.present_mode.clone())
            .show_ui(ui, |ui| {
                for mode in ["Mailbox", "Fifo"] {
                    ui.selectable_value(&mut config.graphics.present_mode, mode.to_string(), mode);
                }
            });
        ui.weak("(restart to apply)");
    });
    if let Some(w) = &state.present_mode_warning {
        ui.colored_label(egui::Color32::from_rgb(255, 160, 0), w.clone());
    }

    // v2.8.0 Phase 2 — pacing regime. Applied live (the app re-resolves
    // against the monitor refresh and reconfigures the surface).
    ui.horizontal(|ui| {
        ui.label("Pacing");
        let before = config.graphics.pacing_mode.clone();
        egui::ComboBox::from_id_salt("settings-pacing-mode")
            .selected_text(config.graphics.pacing_mode.clone())
            .show_ui(ui, |ui| {
                for (mode, label) in [
                    ("auto", "auto (display-sync when refresh matches)"),
                    ("display", "display (sync to vsync)"),
                    ("vrr", "vrr (G-Sync/FreeSync)"),
                    ("wallclock", "wallclock (classic)"),
                ] {
                    ui.selectable_value(&mut config.graphics.pacing_mode, mode.to_string(), label);
                }
            });
        if before != config.graphics.pacing_mode {
            state.apply.pacing_mode = true;
        }
    });

    // v2.8.0 Phase 2 — swapchain depth. Persisted only (surface created
    // once at startup).
    ui.horizontal(|ui| {
        ui.label("Max frame latency");
        ui.add(
            egui::DragValue::new(&mut config.graphics.max_frame_latency)
                .speed(0.05)
                .range(1..=2),
        );
        ui.weak("(1 = lowest latency; restart to apply)");
    });

    // NTSC filter: applied live (binary on/off in the gfx post-pass).
    ui.horizontal(|ui| {
        ui.label("NTSC filter");
        let before = config.graphics.ntsc_filter.clone();
        egui::ComboBox::from_id_salt("settings-ntsc-filter")
            .selected_text(config.graphics.ntsc_filter.clone())
            .show_ui(ui, |ui| {
                for mode in ["off", "composite", "rgb"] {
                    ui.selectable_value(&mut config.graphics.ntsc_filter, mode.to_string(), mode);
                }
            });
        // The gfx post-pass only distinguishes off vs on, so flag a live
        // apply only when crossing that boundary (off <-> non-off).
        if (before == "off") != (config.graphics.ntsc_filter == "off") {
            state.apply.ntsc_filter = true;
        }
    });

    ui.add_space(8.0);
    ui.heading("Audio");

    // Sample rate: persisted only — a live change needs an audio-stream
    // (and APU) rebuild. Offer the two common presets plus a numeric edit.
    ui.horizontal(|ui| {
        ui.label("Sample rate");
        egui::ComboBox::from_id_salt("settings-sample-rate")
            .selected_text(format!("{}", config.audio.sample_rate))
            .show_ui(ui, |ui| {
                for rate in [44_100_u32, 48_000] {
                    ui.selectable_value(&mut config.audio.sample_rate, rate, format!("{rate}"));
                }
            });
        ui.add(
            egui::DragValue::new(&mut config.audio.sample_rate)
                .speed(100.0)
                .range(8_000..=192_000),
        );
        ui.weak("(restart to apply)");
    });

    // v2.8.0 Phase 1 — DRC latency target. Persisted only (the queue +
    // start-gate are sized at stream open).
    ui.horizontal(|ui| {
        ui.label("Audio latency");
        ui.add(
            egui::DragValue::new(&mut config.audio.latency_ms)
                .speed(1.0)
                .range(20..=250)
                .suffix(" ms"),
        );
        ui.weak("(restart to apply)");
    });

    // v2.8.0 Phase 1 — dynamic rate control toggle. Persisted only.
    ui.horizontal(|ui| {
        ui.checkbox(&mut config.audio.drc, "Dynamic rate control");
        ui.weak("(±0.5% drift compensation; restart to apply)");
    });

    ui.add_space(8.0);
    ui.heading("Latency");

    // v2.8.0 Phase 3 — run-ahead depth. Applied live (the produce path
    // reads the config each frame). Native-only feature; the wasm panel
    // shows the control but the wasm produce path ignores it.
    ui.horizontal(|ui| {
        ui.label("Run-ahead (frames)");
        ui.add(
            egui::DragValue::new(&mut config.input.run_ahead)
                .speed(0.05)
                .range(0..=3),
        );
        ui.weak("(removes the game's internal input lag; 1 fits most games)");
    });

    ui.add_space(8.0);
    ui.heading("Rewind");

    // Enabled: applied live — the running Nes arms / frees the ring.
    if ui.checkbox(&mut config.rewind.enabled, "Enabled").changed() {
        state.apply.rewind_enabled = true;
    }

    // Window + keyframe period: persisted only — they size the ring when it
    // is armed, so editing them while rewind is already on does not resize
    // the live buffer.
    ui.horizontal(|ui| {
        ui.label("Window (seconds)");
        ui.add(
            egui::DragValue::new(&mut config.rewind.max_seconds)
                .speed(1.0)
                .range(1..=600),
        );
        ui.weak("(restart to apply)");
    });
    ui.horizontal(|ui| {
        ui.label("Keyframe period (frames)");
        ui.add(
            egui::DragValue::new(&mut config.rewind.keyframe_period)
                .speed(1.0)
                .range(1..=600),
        );
        ui.weak("(restart to apply)");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_apply_drains_and_resets() {
        let mut state = SettingsPanelState {
            apply: SettingsApply {
                ntsc_filter: true,
                rewind_enabled: true,
                pacing_mode: false,
            },
            status: String::new(),
            present_mode_warning: None,
        };
        let first = state.take_apply();
        assert!(first.ntsc_filter);
        assert!(first.rewind_enabled);
        assert!(first.any());
        // A second poll comes back empty (the flag is taken, like
        // `take_bindings_dirty`).
        let second = state.take_apply();
        assert!(!second.any());
    }

    #[test]
    fn default_apply_is_inert() {
        assert!(!SettingsApply::default().any());
    }
}
