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

/// Persist `config` to disk (native) or no-op (wasm: no filesystem).
#[cfg(not(target_arch = "wasm32"))]
fn save_config(config: &Config) {
    if let Err(e) = config.save() {
        eprintln!("rustynes: failed to save config: {e}");
    }
}

/// wasm: config lives in memory only (no filesystem on the web).
#[cfg(target_arch = "wasm32")]
#[allow(clippy::missing_const_for_fn)]
fn save_config(_config: &Config) {}

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
    /// v1.0.0 — the master volume / mute changed; the app applies the new
    /// `[audio] volume`/`muted` to the output gain (the cpal consume point).
    pub audio_gain: bool,
    /// v1.0.0 — the overscan-crop toggle changed; the app pushes the new
    /// `[graphics] hide_overscan` into the gfx letterbox UV rect.
    pub overscan: bool,
    /// v1.1.0 beta.1 — the CRT-filter on/off toggle changed; the app enables /
    /// disables the gfx CRT post-pass to match `[graphics] crt_filter`.
    pub crt_filter: bool,
    /// v1.1.0 beta.1 — the CRT scanline-intensity slider changed; the app pushes
    /// the new `[graphics] crt_scanline` into the live CRT filter.
    pub crt_scanline: bool,
    /// v1.1.0 beta.1 — the user clicked "Load .pal…"; the app opens a file dialog,
    /// parses the palette, applies it to the core, and persists the path.
    pub palette_pick: bool,
    /// v1.1.0 beta.1 — the user reset the palette to built-in; the app clears the
    /// custom palette + `[graphics] palette_file`.
    pub palette_clear: bool,
    /// v1.0.0 — a per-APU-channel mute checkbox changed; the app pushes the new
    /// `[audio] channel_mask` into the core under the emu lock. The default
    /// mask (all on) is byte-identical to today's mixer output.
    pub apu_channels: bool,
}

impl SettingsApply {
    /// `true` if any live-applicable change is pending.
    #[must_use]
    pub const fn any(self) -> bool {
        self.ntsc_filter
            || self.rewind_enabled
            || self.pacing_mode
            || self.audio_gain
            || self.overscan
            || self.crt_filter
            || self.crt_scanline
            || self.palette_pick
            || self.palette_clear
            || self.apu_channels
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
    /// v1.0.0 — "Reset to Defaults" two-click confirm armed-state, per
    /// section (the first click arms + relabels "Confirm?"; the second
    /// click within the same open window performs the reset).
    reset_video_armed: bool,
    reset_audio_armed: bool,
    reset_advanced_armed: bool,
}

/// v1.0.0 — a two-click "Reset to Defaults" button: the first click arms it
/// (relabelling to a red "Confirm reset?"); the second click returns `true`
/// (and disarms). `armed` persists in the panel state across frames.
fn reset_to_defaults_button(ui: &mut egui::Ui, armed: &mut bool, section: &str) -> bool {
    if *armed {
        let confirm = ui
            .button(
                egui::RichText::new(format!("Confirm reset {section}?"))
                    .color(egui::Color32::from_rgb(240, 120, 120)),
            )
            .clicked();
        // Offer an inline Cancel so the user can back out of an arm.
        if ui.button("Cancel").clicked() {
            *armed = false;
        }
        if confirm {
            *armed = false;
            return true;
        }
    } else if ui.button("Reset to Defaults").clicked() {
        *armed = true;
    }
    false
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

/// The full settings body: graphics / audio / rewind sections, rendered one
/// after another. Used by the debugger's standalone Settings window (`show`),
/// which is a single scrolling panel.
///
/// The always-on UX shell instead calls the three `*_section` functions
/// directly, one per Settings-window tab, so each tab shows only its own
/// controls (v1.0.0 settings split — the prior `body`-for-every-tab wiring
/// duplicated every control on every tab).
pub fn body(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    video_section(ui, state, config);
    ui.add_space(8.0);
    audio_section(ui, state, config);
    ui.add_space(8.0);
    advanced_section(ui, state, config);
}

/// The Graphics section: present mode, pacing, swapchain depth, NTSC filter.
/// Mutates `config` directly and accumulates live-apply flags on `state.apply`.
pub fn video_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
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

    // v1.0.0 — overscan crop. Applied live (the gfx letterbox samples the
    // inner source rect); default off = the full 256x240 framebuffer (today's
    // presentation, byte-identical).
    if ui
        .checkbox(
            &mut config.graphics.hide_overscan,
            "Hide overscan (crop top/bottom 8 scanlines)",
        )
        .changed()
    {
        state.apply.overscan = true;
        save_config(config);
    }

    // v1.1.0 beta.1 — CRT / scanline post-pass. Applied live; mutually exclusive
    // with the NTSC filter (CRT wins). Default off = byte-identical presentation.
    if ui
        .checkbox(&mut config.graphics.crt_filter, "CRT / scanlines")
        .changed()
    {
        state.apply.crt_filter = true;
        save_config(config);
    }
    if config.graphics.crt_filter
        && ui
            .add(
                egui::Slider::new(&mut config.graphics.crt_scanline, 0.0..=1.0)
                    .text("Scanline intensity"),
            )
            .changed()
    {
        state.apply.crt_scanline = true;
        save_config(config);
    }

    // v1.1.0 beta.1 — custom .pal palette. The actual file dialog runs in the app
    // after the egui pass (it must not block the render / hold the emu lock here);
    // these buttons just request it. Default = the built-in palette.
    ui.horizontal(|ui| {
        ui.label("Palette");
        let current = config.graphics.palette_file.as_ref().map_or_else(
            || "built-in".to_string(),
            |p| {
                p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            },
        );
        ui.weak(current);
        if ui.button("Load .pal…").clicked() {
            state.apply.palette_pick = true;
        }
        if config.graphics.palette_file.is_some() && ui.button("Built-in").clicked() {
            state.apply.palette_clear = true;
        }
    });

    ui.add_space(4.0);
    // v1.0.0 — reset the Graphics section to its defaults (guarded by a
    // two-click confirm so it isn't a foot-gun), then re-apply live.
    if reset_to_defaults_button(ui, &mut state.reset_video_armed, "graphics") {
        let def = crate::config::GraphicsConfig::default();
        // Cross any off<->on filter / overscan boundary so the app re-applies.
        let ntsc_changed = (config.graphics.ntsc_filter == "off") != (def.ntsc_filter == "off");
        let overscan_changed = config.graphics.hide_overscan != def.hide_overscan;
        let pacing_changed = config.graphics.pacing_mode != def.pacing_mode;
        let crt_changed = config.graphics.crt_filter != def.crt_filter;
        let palette_changed = config.graphics.palette_file != def.palette_file;
        config.graphics = def;
        state.apply.ntsc_filter |= ntsc_changed;
        state.apply.overscan |= overscan_changed;
        state.apply.pacing_mode |= pacing_changed;
        state.apply.crt_filter |= crt_changed;
        state.apply.palette_clear |= palette_changed;
        save_config(config);
    }
}

/// The Audio section: master volume, sample rate, DRC latency target,
/// dynamic rate control.
pub fn audio_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    ui.heading("Audio");

    // v1.0.0 — master volume + mute. Applied LIVE at the cpal consume point
    // (the gain is lock-free); flag `audio_gain` so the app pushes the new
    // value into the queue this frame. Default 1.0 / un-muted = today's sound.
    ui.horizontal(|ui| {
        ui.label("Volume");
        let mut pct = (config.audio.volume.clamp(0.0, 1.0) * 100.0).round();
        if ui
            .add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%"))
            .changed()
        {
            config.audio.volume = (pct / 100.0).clamp(0.0, 1.0);
            state.apply.audio_gain = true;
        }
        if ui.checkbox(&mut config.audio.muted, "Mute").changed() {
            state.apply.audio_gain = true;
        }
    });
    // v1.0.0 — per-APU-channel mute toggles (a studio/debug audio overlay).
    // Each checkbox flips one bit of `[audio] channel_mask`; the app pushes the
    // mask into the core under the emu lock on `apu_channels`. The default mask
    // (all six on) is byte-identical to today's mixer output (the determinism
    // contract — the oracle / test ROMs never set a mask).
    ui.add_space(4.0);
    ui.label("Channels");
    {
        // Bit layout matches `rustynes_apu::Apu::channel_mask`:
        // 0 = pulse 1, 1 = pulse 2, 2 = triangle, 3 = noise, 4 = DMC,
        // 5 = external/mapper audio.
        const CHANNELS: [(u8, &str); 6] = [
            (0, "Pulse 1"),
            (1, "Pulse 2"),
            (2, "Triangle"),
            (3, "Noise"),
            (4, "DMC"),
            (5, "Mapper Audio"),
        ];
        ui.horizontal_wrapped(|ui| {
            for (bit, label) in CHANNELS {
                let mut on = config.audio.channel_mask & (1 << bit) != 0;
                if ui.checkbox(&mut on, label).changed() {
                    if on {
                        config.audio.channel_mask |= 1 << bit;
                    } else {
                        config.audio.channel_mask &= !(1 << bit);
                    }
                    state.apply.apu_channels = true;
                    save_config(config);
                }
            }
        });
    }

    // Persist on release so a drag doesn't thrash the disk; the live gain is
    // already applied above.
    if ui.button("Save audio settings").clicked() {
        save_config(config);
    }

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

    ui.add_space(4.0);
    // v1.0.0 — reset the Audio section to defaults; re-apply the live gain.
    if reset_to_defaults_button(ui, &mut state.reset_audio_armed, "audio") {
        config.audio = crate::config::AudioConfig::default();
        state.apply.audio_gain = true;
        state.apply.apu_channels = true;
        save_config(config);
    }
}

/// The Advanced section: run-ahead depth + rewind enable / window / keyframe.
pub fn advanced_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
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

    ui.add_space(4.0);
    // v1.0.0 — reset run-ahead + rewind to defaults; re-arm the rewind ring.
    if reset_to_defaults_button(ui, &mut state.reset_advanced_armed, "latency/rewind") {
        config.input.run_ahead = crate::config::InputConfig::default().run_ahead;
        config.rewind = crate::config::RewindConfig::default();
        state.apply.rewind_enabled = true;
        save_config(config);
    }
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
                audio_gain: false,
                overscan: false,
                apu_channels: false,
                crt_filter: false,
                crt_scanline: false,
                palette_pick: false,
                palette_clear: false,
            },
            ..Default::default()
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
