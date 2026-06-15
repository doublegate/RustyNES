//! Lua script console panel (v1.1.0 beta.3, Workstream E, T-110-E5).
//!
//! Load / reload / stop a `.lua` script and watch its `print` / `emu.log`
//! output. The panel is UI-only — it holds no `rustynes_script` types, so it
//! compiles whether or not the `scripting` feature is on; the `App` owns the
//! engine, feeds this panel its log/status, and acts on [`ScriptAction`].
//! When the build has no scripting support, the panel shows a notice.

use rustynes_core::Nes;

/// Max log lines retained in the console (oldest dropped).
const LOG_CAP: usize = 500;

/// An action the user requested from the console; polled by the `App`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptAction {
    /// Open a file dialog and load the chosen `.lua` script.
    Load,
    /// Re-load the currently-loaded script from disk.
    Reload,
    /// Unload the running script.
    Stop,
}

/// Script console state. The `App` pushes log lines + status here each pump.
pub struct ScriptPanelState {
    /// `true` when the binary was built with the `scripting` feature.
    available: bool,
    /// Path label of the loaded script (empty when none).
    loaded: String,
    /// Number of registered `onFrame` callbacks (a quick "is it live" signal).
    callbacks: usize,
    /// Last error (load/runtime), shown in red until cleared.
    error: Option<String>,
    /// Rolling log / `print` output.
    log: Vec<String>,
    /// Pending user action, drained by the `App`.
    action: Option<ScriptAction>,
}

impl Default for ScriptPanelState {
    // `available` keys off a cfg flag, so this is not a trivial derive.
    #[allow(clippy::derivable_impls)]
    fn default() -> Self {
        Self {
            available: cfg!(all(feature = "scripting", not(target_arch = "wasm32"))),
            loaded: String::new(),
            callbacks: 0,
            error: None,
            log: Vec::new(),
            action: None,
        }
    }
}

impl ScriptPanelState {
    /// Append log lines (capped at [`LOG_CAP`]).
    pub fn push_log<I: IntoIterator<Item = String>>(&mut self, lines: I) {
        self.log.extend(lines);
        if self.log.len() > LOG_CAP {
            let drop = self.log.len() - LOG_CAP;
            self.log.drain(0..drop);
        }
    }

    /// Set the loaded-script label + callback count (after a successful load).
    pub fn set_loaded(&mut self, label: String, callbacks: usize) {
        self.loaded = label;
        self.callbacks = callbacks;
        self.error = None;
    }

    /// Mark no script loaded.
    pub fn set_unloaded(&mut self) {
        self.loaded.clear();
        self.callbacks = 0;
    }

    /// Record an error (clears on the next successful load).
    pub fn set_error(&mut self, err: String) {
        self.error = Some(err);
    }

    /// Drain the pending user action.
    pub fn take_action(&mut self) -> Option<ScriptAction> {
        self.action.take()
    }

    /// The loaded script's path label (empty string when none) — used by the
    /// `App` to re-read the file on Reload.
    #[must_use]
    pub fn loaded_label(&self) -> &str {
        &self.loaded
    }
}

/// Render the script console.
#[allow(clippy::needless_pass_by_ref_mut)] // uniform chip-panel signature.
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut ScriptPanelState, _nes: &mut Nes) {
    egui::Window::new("Lua Script")
        .open(open)
        .default_size([420.0, 320.0])
        .resizable(true)
        .show(ctx, |ui| {
            if !state.available {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 180, 80),
                    "This build has no scripting support.",
                );
                ui.label("Rebuild with `--features scripting` to enable Lua.");
                return;
            }

            ui.horizontal(|ui| {
                if ui.button("Load .lua…").clicked() {
                    state.action = Some(ScriptAction::Load);
                }
                let has = !state.loaded.is_empty();
                if ui.add_enabled(has, egui::Button::new("Reload")).clicked() {
                    state.action = Some(ScriptAction::Reload);
                }
                if ui.add_enabled(has, egui::Button::new("Stop")).clicked() {
                    state.action = Some(ScriptAction::Stop);
                }
                if ui.button("Clear log").clicked() {
                    state.log.clear();
                }
            });

            if state.loaded.is_empty() {
                ui.weak("No script loaded.");
            } else {
                ui.label(format!(
                    "Loaded: {}  ({} onFrame callback{})",
                    state.loaded,
                    state.callbacks,
                    if state.callbacks == 1 { "" } else { "s" }
                ));
            }
            if let Some(err) = &state.error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), err);
            }
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &state.log {
                        ui.monospace(line);
                    }
                });
        });
}
