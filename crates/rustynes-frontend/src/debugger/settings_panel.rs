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
    /// v1.2.0 C1 — a Bisqwit-NTSC picture knob (contrast / saturation /
    /// brightness / hue) slider changed; the app pushes the new
    /// `[graphics] ntsc_*` values into the live Bisqwit filter. The defaults
    /// (all 0) are byte-identical to the pre-C1 decode.
    pub ntsc_knobs: bool,
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
    /// v1.1.0 beta.2 — the graphic-EQ enable toggle or a band slider changed;
    /// the app pushes the new `[audio] eq_enabled`/`eq_bands` into the audio
    /// queue. Off (default) is byte-identical to today's output.
    pub audio_eq: bool,
    /// v1.7.0 "Forge" H3 — a stereo-DSP control changed (per-channel pan, reverb
    /// mix/room, or headphone crossfeed); the app pushes the new params into the
    /// audio queue. Center pan / 0 reverb / 0 crossfeed (the default) is a true
    /// bypass → byte-identical output.
    pub audio_stereo: bool,
    /// v1.4.0 Workstream C — a per-APU-channel volume slider changed (or the
    /// "Reset gains" button); the app pushes the new `[audio] channel_gain` into
    /// the core under the emu lock. The default (all 1.0) is byte-identical to
    /// today's mixer output.
    pub apu_channel_gain: bool,
    /// v1.2.0 C2 — the composable shader stack changed (a pass added / removed /
    /// reordered / toggled / re-parameterized, or a preset loaded); the app
    /// rebuilds the live `gfx` shader stack from `[graphics] shader_stack`. An
    /// empty stack falls back to the byte-identical direct-blit path.
    pub shader_stack: bool,
    /// v1.5.0 "Lens" Workstream D1 — the active named palette changed (a custom
    /// palette was selected / edited / cleared / saved); the app re-applies the
    /// resolved base palette to the core (or clears it back to the built-in).
    pub palette_select: bool,
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
            || self.ntsc_knobs
            || self.palette_pick
            || self.palette_clear
            || self.apu_channels
            || self.audio_eq
            || self.audio_stereo
            || self.apu_channel_gain
            || self.shader_stack
            || self.palette_select
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
    /// v1.8.9 — the last FDS BIOS (`disksys.rom`) recognition result shown in the
    /// Settings -> FDS section, set when the user browses for one. Native-only
    /// (the rfd file picker is native; wasm uploads the BIOS via a file input).
    #[cfg(not(target_arch = "wasm32"))]
    fds_bios_status: Option<String>,
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
    /// v1.4.0 Workstream C — the loaded mapper's expansion-audio chip name
    /// (`Some("VRC6")`, …) or `None` when the board has no expansion audio.
    /// Pushed by the app on each ROM load (like [`Self::set_present_mode_warning`]);
    /// the Audio section shows the expansion-channel volume slider only when set.
    expansion_audio_chip: Option<&'static str>,
    /// v1.7.0 "Forge" H3 — the enumerated output device names for the Audio
    /// section's device picker. Populated once by the app at startup (the cpal
    /// host enumeration is native-only); empty on wasm / when no devices.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    audio_output_devices: Vec<String>,
    /// v1.2.0 C2 — index into [`crate::shader_pass::BuiltinPass::all`] for the
    /// "Add pass" picker.
    stack_add_index: usize,
    /// v1.2.0 C2 — the preset-name text input for "Save preset".
    preset_name_input: String,
    /// v1.2.0 C2 — the currently-selected preset name (for Load / Delete).
    selected_preset: String,
    /// v1.6.0 "Studio" I3 — status text after a `RetroArch` `.slangp`/`.cgp`
    /// preset import (e.g. "imported 2 passes, 1 unsupported"). Empty until an
    /// import is attempted. Native-only — the wasm32 panel has no file-dialog
    /// import path.
    #[cfg(not(target_arch = "wasm32"))]
    preset_import_status: String,
    /// v1.5.0 "Lens" Workstream D1 — the palette editor's working state.
    palette_editor: PaletteEditorState,
}

/// v1.5.0 "Lens" Workstream D1 — the palette editor's working state.
///
/// The editor edits a 64-colour working copy (`working`) which the user can
/// save under a name into the [`crate::config::PaletteBank`] or apply directly.
/// `open` toggles the editor's collapsing body; `name_input` backs Save-As.
#[derive(Debug)]
struct PaletteEditorState {
    /// The 64 base RGB colours being edited (the working copy). A NES base
    /// palette is always exactly 64 colours, so this is a fixed array.
    working: [[u8; 3]; 64],
    /// Save-As name input.
    name_input: String,
    /// `true` once `working` has been seeded from the active palette / built-in
    /// (so re-opening the editor does not clobber an in-progress edit).
    seeded: bool,
}

impl Default for PaletteEditorState {
    fn default() -> Self {
        // `working` starts all-black; it is seeded from the active / built-in
        // palette on first show (when `seeded` is still false). A `[[u8; 3]; 64]`
        // does not get a blanket `Default` (arrays only derive it up to N = 32),
        // so the impl is spelled out here.
        Self {
            working: [[0u8; 3]; 64],
            name_input: String::new(),
            seeded: false,
        }
    }
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

    /// v1.4.0 Workstream C — set (or clear) the loaded mapper's expansion-audio
    /// chip name. Pushed by the app on each ROM load; the Audio section shows the
    /// expansion-channel volume slider (labelled with this name) only when `Some`.
    pub fn set_expansion_audio_chip(&mut self, chip: Option<&'static str>) {
        self.expansion_audio_chip = chip;
    }

    /// v1.7.0 "Forge" H3 — populate the Audio section's output-device picker list.
    /// Pushed once by the app at startup (cpal host enumeration is native-only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_audio_output_devices(&mut self, names: Vec<String>) {
        self.audio_output_devices = names;
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
    // v1.3.0 — the shader stack is a dedicated Settings tab; the combined
    // tool-panel view still stacks it after the Graphics section.
    shader_stack_section(ui, state, config);
    ui.add_space(8.0);
    audio_section(ui, state, config);
    ui.add_space(8.0);
    recording_section(ui, config);
    ui.add_space(8.0);
    #[cfg(not(target_arch = "wasm32"))]
    {
        fds_section(ui, state, config);
        ui.add_space(8.0);
    }
    advanced_section(ui, state, config);
}

/// v1.8.9 — the Famicom Disk System firmware manager: point at a `disksys.rom`,
/// validate it (8 KiB + SHA-256 recognition), and persist the path to config so
/// `.fds` images can boot. Native-only (rfd file picker; wasm uploads the BIOS via
/// a file input).
#[cfg(not(target_arch = "wasm32"))]
fn fds_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    use crate::fds_firmware::{BiosStatus, classify};
    egui::CollapsingHeader::new("Famicom Disk System (FDS BIOS)").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("disksys.rom:");
            let cur = config
                .fds
                .bios_path
                .as_ref()
                .map_or_else(|| "(not set)".to_owned(), |p| p.display().to_string());
            ui.monospace(cur);
        });
        if ui.button("Browse for disksys.rom\u{2026}").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("FDS BIOS", &["rom", "bin"])
                .pick_file()
        {
            // Read at most one byte past the 8 KiB BIOS size: an accidentally-huge
            // file then can't OOM / freeze the UI thread, and `classify` rejects any
            // length != 8192 (an oversize file yields 8193 bytes here -> WrongSize).
            use std::io::Read as _;
            let read = std::fs::File::open(&path).and_then(|f| {
                let mut buf = Vec::new();
                f.take(crate::fds_firmware::BIOS_SIZE as u64 + 1)
                    .read_to_end(&mut buf)?;
                Ok(buf)
            });
            state.fds_bios_status = Some(match read {
                Ok(bytes) => match classify(&bytes) {
                    BiosStatus::WrongSize(n) => {
                        format!("Not an FDS BIOS: {n} bytes (need 8192) - path NOT changed.")
                    }
                    BiosStatus::Recognized(label) => {
                        config.fds.bios_path = Some(path);
                        format!("Recognized: {label} - path set.")
                    }
                    BiosStatus::Unverified(hex) => {
                        config.fds.bios_path = Some(path);
                        format!(
                            "8 KiB, unverified dump (sha256 {}\u{2026}) - path set.",
                            &hex[..16]
                        )
                    }
                },
                Err(e) => format!("read error: {e}"),
            });
        }
        if let Some(s) = &state.fds_bios_status {
            ui.label(s);
        }
        ui.weak("Required to boot .fds disk images. Takes effect on the next FDS load.");
    });
}

/// v1.8.9 — the A/V recording codec-depth picker (encoder / CRF / preset / audio
/// bitrate).
///
/// Writes plain `config.recording` strings + numbers (persisted to `config.toml`);
/// the recorder's arm path (gated by `av-record`) parses them via
/// `AvRecordOptions::from_parts`. Deliberately un-gated so the picker shows even in
/// a build without the feature — the values just have no effect there.
fn recording_section(ui: &mut egui::Ui, config: &mut Config) {
    egui::CollapsingHeader::new("Recording (A/V codec depth)").show(ui, |ui| {
        let rec = &mut config.recording;
        const CODECS: [(&str, &str); 3] = [
            ("H.264 (universal)", "h264"),
            ("H.265 / HEVC (smaller)", "h265"),
            ("VP9 (royalty-free)", "vp9"),
        ];
        const PRESETS: [(&str, &str); 6] = [
            ("Ultrafast", "ultrafast"),
            ("Superfast", "superfast"),
            ("Veryfast", "veryfast"),
            ("Faster", "faster"),
            ("Medium", "medium"),
            ("Slow", "slow"),
        ];
        ui.horizontal(|ui| {
            ui.label("Video codec");
            let cur = CODECS
                .iter()
                .find(|(_, id)| *id == rec.video_codec)
                .map_or("H.264 (universal)", |(label, _)| *label);
            egui::ComboBox::from_id_salt("rec-codec")
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    for (label, id) in CODECS {
                        if ui.selectable_label(rec.video_codec == id, label).clicked() {
                            id.clone_into(&mut rec.video_codec);
                        }
                    }
                });
        });
        // VP9's CRF ceiling is 63; x264/x265 cap at 51.
        let max_crf = if rec.video_codec == "vp9" { 63 } else { 51 };
        ui.add(egui::Slider::new(&mut rec.crf, 0..=max_crf).text("CRF (lower = better)"));
        ui.horizontal(|ui| {
            ui.label("Preset");
            let cur = PRESETS
                .iter()
                .find(|(_, id)| *id == rec.preset)
                .map_or("Veryfast", |(label, _)| *label);
            egui::ComboBox::from_id_salt("rec-preset")
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    for (label, id) in PRESETS {
                        if ui.selectable_label(rec.preset == id, label).clicked() {
                            id.clone_into(&mut rec.preset);
                        }
                    }
                });
            ui.weak("(x264/x265 only)");
        });
        ui.add(egui::Slider::new(&mut rec.audio_bitrate_k, 32..=512).text("Audio kbit/s"));
    });
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
                // "composite-rt" = true composite NES_NTSC (Bisqwit, T-110-A1);
                // "composite"/"rgb" = the simplified blur.
                for mode in ["off", "composite", "rgb", "composite-rt"] {
                    ui.selectable_value(&mut config.graphics.ntsc_filter, mode.to_string(), mode);
                }
            });
        // Flag a live apply on any change — switching between the simplified and
        // true-composite filters (or on/off) all re-select the gfx post-pass.
        if before != config.graphics.ntsc_filter {
            state.apply.ntsc_filter = true;
        }
    });

    // v1.2.0 C1 — live picture knobs for the true-composite ("composite-rt",
    // Bisqwit) filter. Only shown when that filter is selected; the defaults
    // (all 0) are byte-identical to the pre-C1 decode.
    if config.graphics.ntsc_filter == "composite-rt" {
        let mut knob_changed = false;
        ui.indent("ntsc-knobs", |ui| {
            knob_changed |= ui
                .add(
                    egui::Slider::new(&mut config.graphics.ntsc_contrast, -1.0..=1.0)
                        .text("Contrast"),
                )
                .changed();
            knob_changed |= ui
                .add(
                    egui::Slider::new(&mut config.graphics.ntsc_saturation, -1.0..=1.0)
                        .text("Saturation"),
                )
                .changed();
            knob_changed |= ui
                .add(
                    egui::Slider::new(&mut config.graphics.ntsc_brightness, -100.0..=100.0)
                        .text("Brightness"),
                )
                .changed();
            knob_changed |= ui
                .add(egui::Slider::new(&mut config.graphics.ntsc_hue, -180.0..=180.0).text("Hue"))
                .changed();
        });
        if knob_changed {
            state.apply.ntsc_knobs = true;
            save_config(config);
        }
    }

    // v1.0.0 / v1.5.0 D2 — overscan crop. Applied live (the gfx letterbox
    // samples the inner source rect); default off / all-zero = the full
    // 256x240 framebuffer (today's presentation, byte-identical).
    overscan_section(ui, state, config);

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

    // v1.1.0 beta.1 / v1.5.0 D1 — custom palette. The legacy `.pal` file path
    // loads on the next ROM load; the v1.5.0 named-palette bank + editor below
    // supersede it. The actual file dialog runs in the app after the egui pass
    // (it must not block the render / hold the emu lock here). Presentation-only.
    palette_section(ui, state, config);

    ui.add_space(4.0);
    // v1.0.0 — reset the Graphics section to its defaults (guarded by a
    // two-click confirm so it isn't a foot-gun), then re-apply live.
    if reset_to_defaults_button(ui, &mut state.reset_video_armed, "graphics") {
        let def = crate::config::GraphicsConfig::default();
        // Cross any off<->on filter / overscan boundary so the app re-applies.
        let ntsc_changed = (config.graphics.ntsc_filter == "off") != (def.ntsc_filter == "off");
        // v1.5.0 D2 — re-apply if EITHER the legacy toggle or the per-side crop
        // moves off default.
        let overscan_changed = config.graphics.hide_overscan != def.hide_overscan
            || config.graphics.overscan != def.overscan;
        let pacing_changed = config.graphics.pacing_mode != def.pacing_mode;
        let crt_changed = config.graphics.crt_filter != def.crt_filter;
        let palette_changed = config.graphics.palette_file != def.palette_file;
        // v1.5.0 D1 — re-apply the base palette if a named custom palette was
        // active (reset clears it back to the built-in).
        let active_palette_changed = config.graphics.active_palette != def.active_palette;
        // v1.2.0 C1 — re-push the Bisqwit NTSC knobs if any moved off default.
        // Exact equality is intentional: the goal is "differs from the default at
        // all", and the defaults are exact literals.
        #[allow(clippy::float_cmp)]
        let knobs_changed = config.graphics.ntsc_contrast != def.ntsc_contrast
            || config.graphics.ntsc_saturation != def.ntsc_saturation
            || config.graphics.ntsc_brightness != def.ntsc_brightness
            || config.graphics.ntsc_hue != def.ntsc_hue;
        // v1.2.0 C2 — resetting the Graphics section clears the active shader
        // stack (back to the byte-identical direct blit), but PRESERVES the
        // saved preset bank (a reset of live settings should not throw away the
        // user's named presets).
        let stack_changed = !config.graphics.shader_stack.passes.is_empty();
        let saved_presets = std::mem::take(&mut config.graphics.shader_presets);
        // v1.5.0 D1 — preserve the user's named palette bank across a reset
        // (like the shader presets); only the *active* selection clears.
        let saved_palettes = std::mem::take(&mut config.graphics.palettes);
        config.graphics = def;
        config.graphics.shader_presets = saved_presets;
        config.graphics.palettes = saved_palettes;
        state.apply.ntsc_filter |= ntsc_changed;
        state.apply.overscan |= overscan_changed;
        state.apply.pacing_mode |= pacing_changed;
        state.apply.crt_filter |= crt_changed;
        state.apply.ntsc_knobs |= knobs_changed;
        state.apply.palette_clear |= palette_changed;
        state.apply.palette_select |= active_palette_changed;
        state.apply.shader_stack |= stack_changed;
        if active_palette_changed {
            // The active selection cleared back to the built-in; re-seed the
            // palette editor's swatches from it next frame.
            state.palette_editor.seeded = false;
        }
        save_config(config);
    }
}

/// v1.5.0 "Lens" Workstream D2 — per-side overscan WYSIWYG editor.
///
/// Replaces the binary "hide overscan" toggle's discoverability gap: the
/// legacy toggle is kept (8 px top + bottom, the common preset) and four
/// per-side sliders (Top / Right / Bottom / Left, in NES pixels) live-preview
/// the crop. Every change flags `state.apply.overscan` so the app pushes the
/// new crop into the gfx letterbox. All zero + toggle off = byte-identical.
fn overscan_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    // The legacy preset toggle (8 px top + bottom). Kept for one-click parity.
    if ui
        .checkbox(
            &mut config.graphics.hide_overscan,
            "Hide overscan (crop top + bottom 8 scanlines)",
        )
        .changed()
    {
        state.apply.overscan = true;
        save_config(config);
    }

    egui::CollapsingHeader::new("Overscan (per-side, live)")
        .id_salt("settings-overscan")
        .default_open(false)
        .show(ui, |ui| {
            ui.weak(
                "Trim each edge independently (NES pixels). Combined with the \
                 toggle above; preview updates live.",
            );
            let mut os = config.graphics.overscan;
            let mut changed = false;
            // Top/Bottom range to 112 px (keeps >= 16 visible rows), Left/Right
            // to 120 px (keeps >= 16 visible columns).
            changed |= ui
                .add(egui::Slider::new(&mut os.top, 0..=112).text("Top"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut os.bottom, 0..=112).text("Bottom"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut os.left, 0..=120).text("Left"))
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut os.right, 0..=120).text("Right"))
                .changed();
            if changed {
                config.graphics.overscan = os.clamped();
                state.apply.overscan = true;
                save_config(config);
            }
            if ui.button("Reset overscan (0,0,0,0)").clicked()
                && !config.graphics.overscan.is_zero()
            {
                config.graphics.overscan = crate::config::Overscan::default();
                state.apply.overscan = true;
                save_config(config);
            }
        });
}

/// v1.5.0 "Lens" Workstream D1 — the full palette editor: select / load / edit
/// / save *named* custom palettes with a per-index colour picker.
///
/// Extends the v1.1.0 `.pal` loader + palette viewer: the named-palette bank
/// (`config.graphics.palettes`) holds saved 64-colour base palettes; the active
/// selection (`config.graphics.active_palette`) drives the live presentation.
/// Every change flags `state.apply.palette_select` so the app re-applies the
/// resolved base palette to the core. Presentation-only (no core/accuracy
/// impact); built-in / unselected is byte-identical.
fn palette_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    use crate::config::CustomPalette;

    // --- Active-palette selector ----------------------------------------
    ui.horizontal(|ui| {
        ui.label("Palette");
        let selected_text = config
            .graphics
            .active_palette
            .clone()
            .unwrap_or_else(|| "Built-in".to_string());
        let names: Vec<String> = config.graphics.palettes.palettes.keys().cloned().collect();
        let mut new_active = config.graphics.active_palette.clone();
        egui::ComboBox::from_id_salt("palette-active")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut new_active, None, "Built-in");
                for name in &names {
                    ui.selectable_value(&mut new_active, Some(name.clone()), name);
                }
            });
        if new_active != config.graphics.active_palette {
            config.graphics.active_palette = new_active;
            state.apply.palette_select = true;
            // Re-seed the editor from the newly selected palette next frame so
            // its swatches do not show the previously selected palette's colours.
            state.palette_editor.seeded = false;
            save_config(config);
        }
    });

    // Legacy `.pal` file loader (imports straight into the live palette).
    // Native-only (rfd / filesystem). The named bank below is the v1.5.0 path.
    ui.horizontal(|ui| {
        ui.weak("Legacy .pal:");
        let current = config.graphics.palette_file.as_ref().map_or_else(
            || "none".to_string(),
            |p| {
                p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
            },
        );
        ui.weak(current);
        #[cfg(not(target_arch = "wasm32"))]
        {
            if ui.button("Load .pal…").clicked() {
                state.apply.palette_pick = true;
            }
            if config.graphics.palette_file.is_some() && ui.button("Clear .pal").clicked() {
                state.apply.palette_clear = true;
            }
        }
        #[cfg(target_arch = "wasm32")]
        ui.weak("(native only)");
    });

    // --- Generated NTSC palette (F1.4, collapsing) ----------------------
    // Synthesizes the 64-colour base from the composite-video model instead of
    // the hand-authored built-in. Off by default (byte-identical presentation);
    // when on it takes precedence over the named bank + legacy `.pal`.
    egui::CollapsingHeader::new("Generated NTSC palette")
        .id_salt("palette-generated-ntsc")
        .default_open(false)
        .show(ui, |ui| {
            let g = &mut config.graphics;
            let mut changed = ui
                .checkbox(
                    &mut g.ntsc_palette_enabled,
                    "Use generated palette (overrides built-in / .pal)",
                )
                .changed();
            ui.add_enabled_ui(g.ntsc_palette_enabled, |ui| {
                let p = &mut g.ntsc_palette;
                changed |= ui
                    .add(egui::Slider::new(&mut p.saturation, 0.0..=2.0).text("Saturation"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.hue, -3.0..=3.0).text("Hue (phase units)"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.contrast, 0.5..=2.0).text("Contrast"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.brightness, 0.5..=1.5).text("Brightness"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.gamma, 1.0..=2.6).text("Gamma"))
                    .changed();
                if ui.button("Reset to defaults").clicked() {
                    *p = crate::config::NtscPaletteConfig::default();
                    changed = true;
                }
            });
            if changed {
                // Re-apply the resolved palette (the generated-precedence branch
                // in `App::apply_active_palette` handles the enabled case) and
                // re-seed the editor so its swatches reflect the new base.
                state.apply.palette_select = true;
                state.palette_editor.seeded = false;
                save_config(config);
            }
        });

    // --- Editor (collapsing) --------------------------------------------
    egui::CollapsingHeader::new("Palette editor")
        .id_salt("palette-editor")
        .default_open(false)
        .show(ui, |ui| {
            // Seed the working copy once from the active palette (or built-in).
            if !state.palette_editor.seeded {
                state.palette_editor.working = resolve_active_base_palette(config);
                state.palette_editor.seeded = true;
            }
            ui.weak(
                "Click a swatch to edit its colour. 8 columns x 8 rows = the 64 \
                 NES base colours; emphasis is applied by the renderer.",
            );

            // 8x8 colour-picker grid.
            let mut edited = false;
            // Only persist (synchronous file I/O) once the drag finishes, so
            // dragging a colour picker does not write `config.toml` every frame.
            let mut committed = false;
            egui::Grid::new("palette-editor-grid")
                .spacing([4.0, 4.0])
                .show(ui, |ui| {
                    for row in 0..8 {
                        for col in 0..8 {
                            let idx = row * 8 + col;
                            let c = &mut state.palette_editor.working[idx];
                            let mut rgb = [
                                f32::from(c[0]) / 255.0,
                                f32::from(c[1]) / 255.0,
                                f32::from(c[2]) / 255.0,
                            ];
                            let resp = ui.color_edit_button_rgb(&mut rgb);
                            if resp.changed() {
                                c[0] = (rgb[0] * 255.0).round().clamp(0.0, 255.0) as u8;
                                c[1] = (rgb[1] * 255.0).round().clamp(0.0, 255.0) as u8;
                                c[2] = (rgb[2] * 255.0).round().clamp(0.0, 255.0) as u8;
                                edited = true;
                            }
                            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                                committed = true;
                            }
                        }
                        ui.end_row();
                    }
                });

            // Live-apply the working edits to the active palette (so the editor
            // is WYSIWYG when a named palette is selected). The live core-apply
            // runs on every change during a drag; `save_config` is deferred to
            // when the drag stops (`committed`) to avoid per-frame disk I/O.
            if edited && let Some(name) = config.graphics.active_palette.clone() {
                config
                    .graphics
                    .palettes
                    .palettes
                    .insert(name, CustomPalette::from_base(state.palette_editor.working));
                state.apply.palette_select = true;
                if committed {
                    save_config(config);
                }
            }

            ui.separator();
            // Save-As: store the working copy under a name + select it.
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.palette_editor.name_input)
                        .hint_text("Palette name")
                        .desired_width(160.0),
                );
                if ui.button("Save as").clicked() {
                    let name = state.palette_editor.name_input.trim().to_string();
                    if !name.is_empty() {
                        config.graphics.palettes.palettes.insert(
                            name.clone(),
                            CustomPalette::from_base(state.palette_editor.working),
                        );
                        config.graphics.active_palette = Some(name);
                        state.apply.palette_select = true;
                        save_config(config);
                    }
                }
                // Reseed the editor from the built-in palette (start fresh).
                if ui.button("Reset to built-in").clicked() {
                    state.palette_editor.working = rustynes_core::rustynes_ppu::NES_PALETTE;
                }
            });

            // Import a `.pal` straight into a named bank entry (native only).
            #[cfg(not(target_arch = "wasm32"))]
            if ui.button("Import .pal into bank…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("NES palette", &["pal"])
                    .pick_file()
                && let Some(base) = std::fs::read(&path)
                    .ok()
                    .and_then(|b| crate::config::parse_pal(&b))
            {
                let name = path.file_stem().map_or_else(
                    || "imported".to_string(),
                    |s| s.to_string_lossy().into_owned(),
                );
                config
                    .graphics
                    .palettes
                    .palettes
                    .insert(name.clone(), CustomPalette::from_base(base));
                config.graphics.active_palette = Some(name);
                state.palette_editor.working = base;
                state.apply.palette_select = true;
                save_config(config);
            }

            // Delete the active named palette.
            if let Some(name) = config.graphics.active_palette.clone()
                && ui.button(format!("Delete \"{name}\"")).clicked()
            {
                config.graphics.palettes.palettes.remove(&name);
                config.graphics.active_palette = None;
                state.palette_editor.seeded = false;
                state.apply.palette_select = true;
                save_config(config);
            }
        });
}

/// v1.5.0 "Lens" Workstream D1 — resolve the active base palette: the selected
/// named palette, else the built-in NES palette. (The legacy `.pal` file is
/// applied separately by the app on ROM load; the named bank takes precedence
/// when an entry is selected.)
fn resolve_active_base_palette(config: &Config) -> [[u8; 3]; 64] {
    config
        .graphics
        .active_palette
        .as_ref()
        .and_then(|name| config.graphics.palettes.palettes.get(name))
        .map_or_else(
            || rustynes_core::rustynes_ppu::NES_PALETTE,
            crate::config::CustomPalette::to_base,
        )
}

/// v1.2.0 C2 — the composable shader-stack editor + preset bank UI (mirrors
/// `GeraNES`' `ShaderWindowUI.inl`).
///
/// Lives inside a collapsing header so it does not clutter the Graphics tab.
/// Every mutation flags `state.apply.shader_stack` so the app rebuilds the live
/// `gfx` stack; an empty stack rebuilds to the byte-identical direct-blit path.
///
/// v1.3.0 — `pub` so it can back its own dedicated "Shaders" Settings tab.
pub fn shader_stack_section(
    ui: &mut egui::Ui,
    state: &mut SettingsPanelState,
    config: &mut Config,
) {
    use crate::shader_pass::{BuiltinPass, ShaderPassDesc};

    egui::CollapsingHeader::new("Shader stack (composable)")
        .id_salt("settings-shader-stack")
        // v1.5.0 "Lens" Workstream I3 — default-OPEN: the composable stack is the
        // primary control on the Shaders tab, so it should not require a manual
        // click to reveal.
        .default_open(true)
        .show(ui, |ui| {
            ui.weak(
                "Passes run top to bottom. An empty / all-disabled stack uses the \
                 default direct blit (no change to the image).",
            );

            let all = BuiltinPass::all();
            state.stack_add_index = state.stack_add_index.min(all.len() - 1);

            // --- Add-pass picker -------------------------------------------------
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("shader-stack-add")
                    .selected_text(all[state.stack_add_index].label())
                    .show_ui(ui, |ui| {
                        for (i, p) in all.iter().enumerate() {
                            ui.selectable_value(&mut state.stack_add_index, i, p.label());
                        }
                    });
                if ui.button("Add pass").clicked() {
                    config
                        .graphics
                        .shader_stack
                        .passes
                        .push(ShaderPassDesc::new(all[state.stack_add_index].id()));
                    state.apply.shader_stack = true;
                    save_config(config);
                }
                if ui.button("Clear stack").clicked()
                    && !config.graphics.shader_stack.passes.is_empty()
                {
                    config.graphics.shader_stack.passes.clear();
                    state.apply.shader_stack = true;
                    save_config(config);
                }
            });

            // --- Pass list (toggle / reorder / remove / parameters) --------------
            let mut mutated = false;
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let count = config.graphics.shader_stack.passes.len();
            if count == 0 {
                ui.weak("(no passes — using the default blit)");
            }
            for i in 0..count {
                let pass = &mut config.graphics.shader_stack.passes[i];
                let label = BuiltinPass::from_id(&pass.id).map_or_else(
                    || format!("{} (unknown)", pass.id),
                    |b| b.label().to_string(),
                );
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut pass.enabled, "").changed() {
                            mutated = true;
                        }
                        ui.label(format!("{}. {label}", i + 1));
                        if ui.small_button("^").clicked() && i > 0 {
                            move_up = Some(i);
                        }
                        if ui.small_button("v").clicked() && i + 1 < count {
                            move_down = Some(i);
                        }
                        if ui.small_button("x").clicked() {
                            remove = Some(i);
                        }
                    });
                    // Per-pass parameter sliders, parsed from the shader's
                    // `#pragma parameter` headers.
                    if let Some(builtin) = BuiltinPass::from_id(&pass.id) {
                        for p in builtin.params() {
                            let mut val = pass.param_value(&p);
                            if ui
                                .add(
                                    egui::Slider::new(&mut val, p.min..=p.max)
                                        .text(&p.label)
                                        .step_by(f64::from(p.step.max(0.0))),
                                )
                                .changed()
                            {
                                pass.params.insert(p.name.clone(), val);
                                mutated = true;
                            }
                        }
                    }
                });
            }
            // Apply the deferred structural edits (after the borrow ends).
            let passes = &mut config.graphics.shader_stack.passes;
            if let Some(i) = move_up {
                passes.swap(i, i - 1);
                mutated = true;
            }
            if let Some(i) = move_down {
                passes.swap(i, i + 1);
                mutated = true;
            }
            if let Some(i) = remove {
                passes.remove(i);
                mutated = true;
            }
            if mutated {
                state.apply.shader_stack = true;
                save_config(config);
            }

            // --- Preset bank -----------------------------------------------------
            ui.separator();
            ui.label("Presets");
            // Seed the built-in CRT presets (only adds the ones the user doesn't
            // already have a name collision with — never clobbers a user preset).
            if config.graphics.shader_presets.presets.is_empty()
                && ui.button("Add built-in CRT presets").clicked()
            {
                for (name, stack) in crate::shader_pass::ShaderPresetBank::builtins() {
                    config
                        .graphics
                        .shader_presets
                        .presets
                        .entry(name)
                        .or_insert(stack);
                }
                save_config(config);
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.preset_name_input)
                        .hint_text("Preset name")
                        .desired_width(160.0),
                );
                if ui.button("Save preset").clicked() {
                    let name = state.preset_name_input.trim().to_string();
                    if !name.is_empty() {
                        config
                            .graphics
                            .shader_presets
                            .presets
                            .insert(name.clone(), config.graphics.shader_stack.clone());
                        state.selected_preset = name;
                        save_config(config);
                    }
                }
            });
            ui.horizontal(|ui| {
                let preview = if state.selected_preset.is_empty() {
                    "Load preset…".to_string()
                } else {
                    state.selected_preset.clone()
                };
                // Snapshot the names so the combo closure does not borrow
                // `config` while we also mutate `state.selected_preset`.
                let names: Vec<String> = config
                    .graphics
                    .shader_presets
                    .presets
                    .keys()
                    .cloned()
                    .collect();
                egui::ComboBox::from_id_salt("shader-stack-preset-list")
                    .selected_text(preview)
                    .show_ui(ui, |ui| {
                        for name in names {
                            ui.selectable_value(&mut state.selected_preset, name.clone(), name);
                        }
                    });
                let has_sel = config
                    .graphics
                    .shader_presets
                    .presets
                    .contains_key(&state.selected_preset);
                if ui.add_enabled(has_sel, egui::Button::new("Load")).clicked()
                    && let Some(stack) = config
                        .graphics
                        .shader_presets
                        .presets
                        .get(&state.selected_preset)
                        .cloned()
                {
                    config.graphics.shader_stack = stack;
                    state.preset_name_input.clone_from(&state.selected_preset);
                    state.apply.shader_stack = true;
                    save_config(config);
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("Delete"))
                    .clicked()
                {
                    config
                        .graphics
                        .shader_presets
                        .presets
                        .remove(&state.selected_preset);
                    state.selected_preset.clear();
                    save_config(config);
                }
            });

            // --- RetroArch `.slangp` / `.cgp` import (v1.6.0 "Studio" I3) --------
            // A CONSTRAINED translator: it recognizes common community-preset
            // shader filenames and re-expresses them with RustyNES's built-in
            // passes; it does NOT translate GLSL/Slang source (out of scope per
            // ADR 0013). Unsupported passes are reported, not silently dropped.
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.separator();
                ui.label("Import RetroArch preset (constrained)");
                ui.weak(
                    "Recognizes common crt / ntsc / hqx / xbr preset names and maps \
                     them onto the built-in passes. Source shaders are not translated; \
                     unrecognized passes are reported.",
                );
                if ui.button("Import .slangp / .cgp…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("RetroArch preset", &["slangp", "cgp"])
                        .pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(text) => match crate::slang_preset::import_preset(&text) {
                            Ok(result) => {
                                let mapped = result.stack.passes.len();
                                let unsupported = result.unsupported_count();
                                if result.any_mapped() {
                                    config.graphics.shader_stack = result.stack;
                                    state.apply.shader_stack = true;
                                    save_config(config);
                                }
                                state.preset_import_status = format!(
                                    "Imported {mapped} pass(es); {unsupported} unsupported."
                                );
                            }
                            Err(e) => {
                                state.preset_import_status = format!("Import failed: {e}");
                            }
                        },
                        Err(e) => {
                            state.preset_import_status = format!("Could not read file: {e}");
                        }
                    }
                }
                if !state.preset_import_status.is_empty() {
                    ui.weak(state.preset_import_status.clone());
                }
            }
        });
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

    // v1.4.0 Workstream C — per-channel volume (gain) sliders. A studio mixing
    // overlay generalizing the mute mask above (a slider at 1.0 = full, at 0.0 =
    // muted). The 5 internal APU channels are always shown; the expansion-audio
    // channel (index 5) is shown ONLY when the loaded mapper has on-cart audio
    // (VRC6 / VRC7 / MMC5 / Namco 163 / Sunsoft 5B / FDS), labelled with the chip
    // name. Live-applied via `apu_channel_gain`; the default (all 1.0) is
    // byte-identical to today's mixer output (the determinism contract).
    ui.add_space(4.0);
    ui.label("Channel volume");
    {
        // (gain index, label) for the five always-present internal channels.
        const APU_GAINS: [(usize, &str); 5] = [
            (0, "Pulse 1"),
            (1, "Pulse 2"),
            (2, "Triangle"),
            (3, "Noise"),
            (4, "DMC"),
        ];
        let gain_slider = |ui: &mut egui::Ui, value: &mut f32, label: &str| -> bool {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.add_sized([72.0, 0.0], egui::Label::new(label));
                if ui
                    .add(egui::Slider::new(value, 0.0..=2.0).fixed_decimals(2))
                    .changed()
                {
                    changed = true;
                }
            });
            changed
        };
        for (idx, label) in APU_GAINS {
            if gain_slider(ui, &mut config.audio.channel_gain[idx], label) {
                config.audio.channel_gain[idx] = config.audio.channel_gain[idx].clamp(0.0, 2.0);
                state.apply.apu_channel_gain = true;
                save_config(config);
            }
        }
        // Expansion-audio channel (index 5): only when the loaded mapper has it.
        if let Some(chip) = state.expansion_audio_chip
            && gain_slider(ui, &mut config.audio.channel_gain[5], chip)
        {
            config.audio.channel_gain[5] = config.audio.channel_gain[5].clamp(0.0, 2.0);
            state.apply.apu_channel_gain = true;
            save_config(config);
        }
        if ui.button("Reset volumes (1.0)").clicked() {
            config.audio.channel_gain = [1.0; 6];
            state.apply.apu_channel_gain = true;
            save_config(config);
        }
    }

    // v1.1.0 beta.2 (T-110-D2) — optional graphic EQ output stage. Off by
    // default → byte-identical output; this is a frontend stage (post-resampler),
    // never the deterministic core synthesis. Native-only: the EQ runs through
    // the cpal audio handle, which is absent on wasm (the sliders would be inert).
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui.add_space(6.0);
        ui.separator();
        if ui
            .checkbox(&mut config.audio.eq_enabled, "Graphic EQ")
            .changed()
        {
            state.apply.audio_eq = true;
            save_config(config);
        }
        // v1.7.0 H3 — choose the 5-band voicing or the 20-band graphic EQ.
        if ui
            .checkbox(&mut config.audio.eq_20_band, "20-band graphic EQ")
            .on_hover_text("ISO third-octave bands (25 Hz–20 kHz); off uses the classic 5 bands")
            .changed()
        {
            state.apply.audio_eq = true;
            save_config(config);
        }
        ui.add_enabled_ui(config.audio.eq_enabled, |ui| {
            const BANDS_5: [&str; 5] = ["60", "240", "1k", "3.8k", "12k"];
            // Compact labels for the 20 ISO bands (Hz / k).
            const BANDS_20: [&str; 20] = [
                "25", "40", "63", "100", "160", "250", "400", "630", "1k", "1.6k", "2.5k", "4k",
                "6.3k", "8k", "10k", "12.5k", "14k", "16k", "18k", "20k",
            ];
            let band_slider = |ui: &mut egui::Ui, value: &mut f32, label: &str| {
                ui.vertical(|ui| {
                    let resp = ui.add(
                        egui::Slider::new(value, -12.0..=12.0)
                            .vertical()
                            .suffix(" dB"),
                    );
                    ui.label(label);
                    resp
                })
                .inner
            };
            ui.horizontal_wrapped(|ui| {
                if config.audio.eq_20_band {
                    for (i, label) in BANDS_20.iter().enumerate() {
                        let resp = band_slider(ui, &mut config.audio.eq_bands_20[i], label);
                        if resp.changed() {
                            state.apply.audio_eq = true;
                        }
                        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                            save_config(config);
                        }
                    }
                } else {
                    for (i, label) in BANDS_5.iter().enumerate() {
                        let resp = band_slider(ui, &mut config.audio.eq_bands[i], label);
                        if resp.changed() {
                            state.apply.audio_eq = true;
                        }
                        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                            save_config(config);
                        }
                    }
                }
            });
            if ui.button("Reset EQ (flat)").clicked() {
                if config.audio.eq_20_band {
                    config.audio.eq_bands_20 = [0.0; 20];
                } else {
                    config.audio.eq_bands = [0.0; 5];
                }
                state.apply.audio_eq = true;
                save_config(config);
            }
        });

        // v1.7.0 H3 — stereo output DSP: per-channel pan, reverb, headphone
        // crossfeed. Native-only (the cpal output stage); bypass-by-default
        // (center pan / 0 reverb / 0 crossfeed) is byte-identical to the prior
        // mono-duplicated output.
        ui.add_space(6.0);
        ui.separator();
        ui.label("Stereo");
        {
            const PANS: [(usize, &str); 5] = [
                (0, "Pulse 1"),
                (1, "Pulse 2"),
                (2, "Triangle"),
                (3, "Noise"),
                (4, "DMC"),
            ];
            let pan_slider = |ui: &mut egui::Ui, value: &mut f32, label: &str| -> bool {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.add_sized([72.0, 0.0], egui::Label::new(label));
                    let resp = ui.add(
                        egui::Slider::new(value, -1.0..=1.0)
                            .fixed_decimals(2)
                            .custom_formatter(|v, _| {
                                if v.abs() < 0.005 {
                                    "C".to_owned()
                                } else if v < 0.0 {
                                    format!("L{:.0}", v.abs() * 100.0)
                                } else {
                                    format!("R{:.0}", v * 100.0)
                                }
                            }),
                    );
                    changed = resp.changed();
                });
                changed
            };
            for (idx, label) in PANS {
                if pan_slider(ui, &mut config.audio.pan[idx], label) {
                    config.audio.pan[idx] = config.audio.pan[idx].clamp(-1.0, 1.0);
                    state.apply.audio_stereo = true;
                    save_config(config);
                }
            }
            if let Some(chip) = state.expansion_audio_chip
                && pan_slider(ui, &mut config.audio.pan[5], chip)
            {
                config.audio.pan[5] = config.audio.pan[5].clamp(-1.0, 1.0);
                state.apply.audio_stereo = true;
                save_config(config);
            }
            ui.horizontal(|ui| {
                ui.label("Reverb");
                if ui
                    .add(
                        egui::Slider::new(&mut config.audio.reverb_mix, 0.0..=1.0)
                            .fixed_decimals(2),
                    )
                    .changed()
                {
                    state.apply.audio_stereo = true;
                    save_config(config);
                }
                if ui
                    .add(
                        egui::Slider::new(&mut config.audio.reverb_room, 0.0..=1.0)
                            .fixed_decimals(2)
                            .text("room"),
                    )
                    .changed()
                {
                    state.apply.audio_stereo = true;
                    save_config(config);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Crossfeed");
                if ui
                    .add(
                        egui::Slider::new(&mut config.audio.crossfeed, 0.0..=1.0).fixed_decimals(2),
                    )
                    .on_hover_text("Headphone L/R blend; 0 = off")
                    .changed()
                {
                    state.apply.audio_stereo = true;
                    save_config(config);
                }
            });
            if ui.button("Reset stereo (center / dry)").clicked() {
                config.audio.pan = [0.0; 6];
                config.audio.reverb_mix = 0.0;
                config.audio.reverb_room = 0.5;
                config.audio.crossfeed = 0.0;
                state.apply.audio_stereo = true;
                save_config(config);
            }
        }

        // v1.7.0 H3 — per-context master volumes (master / game / menu). All
        // default to 1.0 (no-op → byte-identical).
        ui.add_space(6.0);
        ui.label("Context volume");
        {
            let vol_slider = |ui: &mut egui::Ui, value: &mut f32, label: &str| -> bool {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.add_sized([56.0, 0.0], egui::Label::new(label));
                    if ui
                        .add(egui::Slider::new(value, 0.0..=1.0).fixed_decimals(2))
                        .changed()
                    {
                        *value = value.clamp(0.0, 1.0);
                        changed = true;
                    }
                });
                changed
            };
            if vol_slider(ui, &mut config.audio.master_volume, "Master")
                || vol_slider(ui, &mut config.audio.volume_game, "Game")
                || vol_slider(ui, &mut config.audio.volume_menu, "Menu")
            {
                state.apply.audio_gain = true;
                save_config(config);
            }
        }

        // v1.7.0 H3 — output device picker. "System default" = None (today's
        // behaviour); a named device takes effect on the next stream open
        // (restart), and an absent device falls back to the default gracefully.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Output device");
            let selected = config
                .audio
                .output_device
                .clone()
                .unwrap_or_else(|| "System default".to_owned());
            egui::ComboBox::from_id_salt("settings-audio-device")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(config.audio.output_device.is_none(), "System default")
                        .clicked()
                    {
                        config.audio.output_device = None;
                        save_config(config);
                    }
                    for name in &state.audio_output_devices {
                        let on = config.audio.output_device.as_deref() == Some(name.as_str());
                        if ui.selectable_label(on, name).clicked() {
                            config.audio.output_device = Some(name.clone());
                            save_config(config);
                        }
                    }
                });
            ui.weak("(restart to apply)");
        });

        // Manual persist for anything still in-flight; the live gain is already
        // applied above.
        if ui.button("Save audio settings").clicked() {
            save_config(config);
        }
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
        state.apply.apu_channel_gain = true;
        // v1.1.0 / v1.7.0 H3 — re-apply the EQ + stereo DSP (reset to flat /
        // bypass) so the live output stage matches the reset config.
        state.apply.audio_eq = true;
        state.apply.audio_stereo = true;
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

    ui.add_space(8.0);
    enhancements_section(ui, state, config);

    ui.add_space(4.0);
    // v1.0.0 — reset run-ahead + rewind to defaults; re-arm the rewind ring.
    if reset_to_defaults_button(ui, &mut state.reset_advanced_armed, "latency/rewind") {
        config.input.run_ahead = crate::config::InputConfig::default().run_ahead;
        config.rewind = crate::config::RewindConfig::default();
        config.enhancements = crate::config::EnhancementsConfig::default();
        state.apply.rewind_enabled = true;
        save_config(config);
    }
}

/// v1.5.0 "Lens" Workstream D3 — the grouped "Enhancements" settings (à la
/// `GeraNES`' Improvements window / Mesen2's emulation enhancements). These are
/// non-accuracy *enhancement* modes; each is OFF / neutral by default, clearly
/// labelled, and **never part of the determinism oracle / `AccuracyCoin` / TAS /
/// netplay paths**.
///
/// The max-rewind window (above, in the Rewind group) is the third
/// enhancement-adjacent knob; the sprite-limit / overclock toggles below are
/// staged: the cycle-accurate core has no hook to disable the sprite limit or
/// to overclock yet (both need the v2.0 fractional-master-clock core pass,
/// ADR 0002), so they persist the user's intent + are surfaced as experimental
/// but do not affect the deterministic core output today.
fn enhancements_section(ui: &mut egui::Ui, state: &mut SettingsPanelState, config: &mut Config) {
    egui::CollapsingHeader::new("Enhancements (non-accuracy)")
        .id_salt("settings-enhancements")
        .default_open(false)
        .show(ui, |ui| {
            ui.weak(
                "Off-by-default enhancement modes. These are NEVER applied while \
                 accuracy tests / TAS replay / netplay run.",
            );

            let mut changed = false;
            changed |= ui
                .checkbox(
                    &mut config.enhancements.disable_sprite_limit,
                    "Disable 8-sprite-per-scanline limit (reduces flicker)",
                )
                .changed();
            ui.indent("enh-sprite-note", |ui| {
                ui.weak("Experimental: staged for the v2.0 core pass (currently inert).");
            });

            ui.horizontal(|ui| {
                ui.label("Overclock (extra scanlines)");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut config.enhancements.overclock_scanlines)
                            .speed(1.0)
                            .range(0..=80),
                    )
                    .changed();
            });
            ui.indent("enh-overclock-note", |ui| {
                ui.weak("Experimental: staged for the v2.0 core pass (currently inert).");
            });

            // The max-rewind window cross-links the Rewind group above (the
            // enhancement-adjacent third knob), surfaced here for grouping.
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Max rewind (seconds)");
                if ui
                    .add(
                        egui::DragValue::new(&mut config.rewind.max_seconds)
                            .speed(1.0)
                            .range(1..=600),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.weak("(also in Rewind; restart to resize the buffer)");
            });

            if changed {
                save_config(config);
            }
        });
    // The enhancement toggles do not yet drive a live core apply (no core hook);
    // they are persisted by the auto-save backstop + the explicit save above.
    let _ = state;
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
                ..Default::default()
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
