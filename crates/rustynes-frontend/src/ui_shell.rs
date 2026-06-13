//! Always-on desktop UX shell (v1.0.0).
//!
//! This module owns the persistent egui chrome that frames the NES image
//! independently of the (separately toggled) debugger overlay:
//!
//! - a top **menu bar** (File / Emulation / Options / Debug / Help),
//! - a bottom **status bar** (ROM name, running/paused, status toasts, FPS),
//! - a tabbed **settings window** (Video / Audio / Input / Advanced),
//! - the first-run **welcome** modal plus **about** and **keyboard-shortcuts**
//!   windows.
//!
//! The shell never touches the emulator core: every menu action that needs
//! `&mut App` is returned as a [`MenuAction`] and dispatched *after* the egui
//! pass (the same "return a request, act later" idiom the debugger panels use
//! for their settings/netplay requests). This keeps the build closure free of
//! the emu lock and avoids the borrow conflict between `&mut self.config` (held
//! by the closure) and the `&mut self` the actions need.
//!
//! The shell runs on both native and the browser (`wasm-winit`) builds; only
//! the filesystem-backed actions (open-ROM dialog, recent-ROM paths, on-disk
//! config save) are native-gated at the dispatch site.

use std::path::PathBuf;
use std::time::Duration;

use web_time::Instant;

use crate::config::{AppTheme, Config};

/// A status message shown in the status bar with a colour and an auto-fade.
///
/// Ported from the parent UX reference, retargeted to `web_time::Instant` so it
/// compiles unchanged on the browser build (where `std::time::Instant` panics).
#[derive(Debug, Clone)]
pub struct StatusMessage {
    /// Message text.
    pub text: String,
    /// Message colour.
    pub color: egui::Color32,
    /// When the message was created.
    pub created_at: Instant,
    /// How long the message stays visible before it fully fades.
    pub duration: Duration,
}

impl StatusMessage {
    /// Create a new status message with an explicit colour and duration.
    #[must_use]
    pub fn new(text: impl Into<String>, color: egui::Color32, duration: Duration) -> Self {
        Self {
            text: text.into(),
            color,
            created_at: Instant::now(),
            duration,
        }
    }

    /// Create an info message (white text, 3 seconds).
    #[must_use]
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(text, egui::Color32::WHITE, Duration::from_secs(3))
    }

    /// Create a success message (green text, 3 seconds).
    #[must_use]
    pub fn success(text: impl Into<String>) -> Self {
        Self::new(
            text,
            egui::Color32::from_rgb(100, 200, 100),
            Duration::from_secs(3),
        )
    }

    /// Returns `true` once the message has outlived its [`Self::duration`].
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    /// The alpha multiplier for the fade-out effect (1.0 down to 0.0 over the
    /// final second of the message's lifetime).
    #[must_use]
    pub fn alpha(&self) -> f32 {
        let elapsed = self.created_at.elapsed().as_secs_f32();
        let total = self.duration.as_secs_f32();
        if elapsed < total - 1.0 {
            1.0
        } else {
            1.0 - ((elapsed - (total - 1.0)) / 1.0).min(1.0)
        }
    }
}

/// Which tab the settings window currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    /// Video / display settings.
    #[default]
    Video,
    /// Audio settings.
    Audio,
    /// Input rebinding.
    Input,
    /// Advanced / debug settings.
    Advanced,
}

/// An action the user triggered in the shell that needs `&mut App`. Returned
/// from [`ShellOutput`] and dispatched after the egui pass so the build closure
/// never borrows the whole `App`.
#[derive(Debug, Clone)]
pub enum MenuAction {
    /// Open the native file dialog to pick a ROM (native only).
    OpenRom,
    /// Load a specific ROM by path (a Recent-ROMs click; native only).
    LoadRom(PathBuf),
    /// Clear the Recent-ROMs list.
    ClearRecent,
    /// Save state to the most-recent slot.
    SaveState,
    /// Load state from the most-recent slot.
    LoadState,
    /// Quit the application.
    Quit,
    /// Toggle pause/resume.
    TogglePause,
    /// Reset (warm boot).
    Reset,
    /// Power-cycle (cold boot).
    PowerCycle,
    /// Toggle the debugger overlay.
    ToggleDebugger,
    /// Toggle borderless fullscreen.
    ToggleFullscreen,
}

/// Per-frame outputs from [`UiShell::build`].
///
/// Carries the chosen menu action (if any) for the `App` to dispatch AFTER the
/// egui pass — the pixel-aspect / theme changes are read directly off `config`
/// by the app, so the only deferred output here is [`MenuAction`].
#[derive(Debug, Default)]
pub struct ShellOutput {
    /// The menu action the user triggered this frame, if any.
    pub action: Option<MenuAction>,
}

/// Always-on shell state (window/modal visibility, the active settings tab, the
/// transient status toast, and the mirrored pause/fullscreen flags).
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct UiShell {
    /// Whether the top menu bar is shown. Default `true`.
    pub menu_visible: bool,
    /// Whether the tabbed settings window is open.
    pub show_settings_window: bool,
    /// Whether the About window is open.
    pub show_about: bool,
    /// Whether the keyboard-shortcuts window is open.
    pub show_shortcuts: bool,
    /// Whether the first-run welcome modal is shown. Initialised from
    /// `!config.welcome_shown`.
    pub show_welcome: bool,
    /// The currently selected settings tab.
    pub settings_tab: SettingsTab,
    /// The current transient status message, if any.
    pub status_message: Option<StatusMessage>,
    /// Whether emulation is paused (mirror of the produce gate).
    pub paused: bool,
    /// Whether the window is in borderless fullscreen (mirror of the gfx flag).
    pub fullscreen: bool,
}

impl UiShell {
    /// Build the initial shell state from the loaded config (the welcome modal
    /// shows on a brand-new install where `welcome_shown` is still `false`).
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self {
            menu_visible: true,
            show_settings_window: false,
            show_about: false,
            show_shortcuts: false,
            show_welcome: !config.welcome_shown,
            settings_tab: SettingsTab::default(),
            status_message: None,
            paused: false,
            fullscreen: false,
        }
    }

    /// Set a transient status message (replaces any prior one).
    pub fn set_status(&mut self, message: StatusMessage) {
        self.status_message = Some(message);
    }

    /// Drop the current status message if it has fully faded.
    pub fn clear_expired_status(&mut self) {
        if self
            .status_message
            .as_ref()
            .is_some_and(StatusMessage::is_expired)
        {
            self.status_message = None;
        }
    }
}

/// Context the app threads into [`UiShell::build`] each frame so the shell can
/// render the status bar + welcome shortcuts without locking the emu inside the
/// egui closure.
pub struct ShellFrame<'a> {
    /// The current ROM label (or a placeholder when none is loaded).
    pub rom_label: &'a str,
    /// Whether a ROM is currently loaded (captured under a brief lock BEFORE the
    /// egui pass — never read inside the closure).
    pub rom_loaded: bool,
    /// The latest measured frames-per-second.
    pub fps: f32,
    /// Whether the debugger overlay is currently visible (drives the Debug menu
    /// checkmark).
    pub debugger_visible: bool,
}

impl UiShell {
    /// Build the always-on shell UI for this frame. Returns a [`ShellOutput`]
    /// carrying the menu action (if any) for the app to dispatch afterwards.
    ///
    /// `config` is edited in place (theme combo, 8:7 toggle, recent list);
    /// `settings_body` and `input_body` render the Settings window's Video /
    /// Audio / Advanced and Input tab bodies respectively, reusing the existing
    /// debugger settings + input-rebind widgets so their live-apply plumbing is
    /// untouched.
    #[allow(clippy::too_many_lines)]
    pub fn build(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        frame: &ShellFrame<'_>,
        mut settings_body: impl FnMut(&mut egui::Ui, &mut Config),
        mut input_body: impl FnMut(&mut egui::Ui, &mut Config),
    ) -> ShellOutput {
        self.clear_expired_status();
        let mut out = ShellOutput::default();

        if self.menu_visible {
            self.menu_bar(ctx, config, frame, &mut out);
        }
        self.status_bar(ctx, frame, config);
        self.settings_window(ctx, config, &mut settings_body, &mut input_body);
        self.welcome_modal(ctx, config);
        about_window(ctx, &mut self.show_about);
        shortcuts_window(ctx, &mut self.show_shortcuts);

        out
    }

    /// Render the top menu bar.
    #[allow(clippy::too_many_lines)]
    fn menu_bar(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        frame: &ShellFrame<'_>,
        out: &mut ShellOutput,
    ) {
        egui::TopBottomPanel::top("shell_menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // File
                ui.menu_button("File", |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Open ROM...").clicked() {
                        out.action = Some(MenuAction::OpenRom);
                        ui.close_menu();
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    ui.menu_button("Recent ROMs", |ui| {
                        if config.recent_roms.paths.is_empty() {
                            ui.label("No recent ROMs");
                        } else {
                            for path in config.recent_roms.paths.clone() {
                                let name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string();
                                if ui.button(name).clicked() {
                                    out.action = Some(MenuAction::LoadRom(path));
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            if ui.button("Clear Recent").clicked() {
                                out.action = Some(MenuAction::ClearRecent);
                                ui.close_menu();
                            }
                        }
                    });

                    ui.separator();

                    if ui
                        .add_enabled(frame.rom_loaded, egui::Button::new("Save State"))
                        .clicked()
                    {
                        out.action = Some(MenuAction::SaveState);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(frame.rom_loaded, egui::Button::new("Load State"))
                        .clicked()
                    {
                        out.action = Some(MenuAction::LoadState);
                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Quit").clicked() {
                        out.action = Some(MenuAction::Quit);
                        ui.close_menu();
                    }
                });

                // Emulation
                ui.menu_button("Emulation", |ui| {
                    let pause_label = if self.paused { "Resume" } else { "Pause" };
                    if ui
                        .add_enabled(frame.rom_loaded, egui::Button::new(pause_label))
                        .clicked()
                    {
                        out.action = Some(MenuAction::TogglePause);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(frame.rom_loaded, egui::Button::new("Reset"))
                        .clicked()
                    {
                        out.action = Some(MenuAction::Reset);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(frame.rom_loaded, egui::Button::new("Power Cycle"))
                        .clicked()
                    {
                        out.action = Some(MenuAction::PowerCycle);
                        ui.close_menu();
                    }
                });

                // Options
                ui.menu_button("Options", |ui| {
                    if ui.button("Settings...").clicked() {
                        self.show_settings_window = true;
                        ui.close_menu();
                    }
                    ui.menu_button("Theme", |ui| {
                        for theme in [AppTheme::Light, AppTheme::Dark, AppTheme::System] {
                            if ui
                                .radio_value(&mut config.ui.theme, theme, theme.display_name())
                                .clicked()
                            {
                                save_config(config);
                                ui.close_menu();
                            }
                        }
                    });
                    if ui
                        .checkbox(
                            &mut config.ui.pixel_aspect_correction,
                            "8:7 Pixel Aspect Ratio",
                        )
                        .changed()
                    {
                        save_config(config);
                    }
                    if ui.checkbox(&mut self.fullscreen, "Fullscreen").changed() {
                        out.action = Some(MenuAction::ToggleFullscreen);
                        ui.close_menu();
                    }
                });

                // Debug
                ui.menu_button("Debug", |ui| {
                    let mut dbg = frame.debugger_visible;
                    if ui.checkbox(&mut dbg, "Show Debugger").changed() {
                        out.action = Some(MenuAction::ToggleDebugger);
                        ui.close_menu();
                    }
                });

                // Help
                ui.menu_button("Help", |ui| {
                    if ui.button("Keyboard Shortcuts").clicked() {
                        self.show_shortcuts = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("About").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    /// Render the bottom status bar.
    fn status_bar(&self, ctx: &egui::Context, frame: &ShellFrame<'_>, config: &Config) {
        egui::TopBottomPanel::bottom("shell_status_bar")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if frame.rom_loaded {
                        ui.label(format!("ROM: {}", frame.rom_label));
                        ui.separator();
                        if self.paused {
                            ui.colored_label(egui::Color32::YELLOW, "Paused");
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(100, 200, 100), "Running");
                        }
                    } else {
                        ui.label("No ROM loaded");
                        ui.separator();
                        ui.colored_label(egui::Color32::GRAY, "Idle");
                    }

                    if let Some(msg) = &self.status_message {
                        ui.separator();
                        let alpha = msg.alpha();
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let color = egui::Color32::from_rgba_unmultiplied(
                            msg.color.r(),
                            msg.color.g(),
                            msg.color.b(),
                            (alpha * 255.0) as u8,
                        );
                        ui.colored_label(color, &msg.text);
                    }

                    if config.ui.show_fps {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("FPS: {:.1}", frame.fps));
                        });
                    }
                });
            });
    }

    /// Render the tabbed settings window.
    fn settings_window(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        settings_body: &mut impl FnMut(&mut egui::Ui, &mut Config),
        input_body: &mut impl FnMut(&mut egui::Ui, &mut Config),
    ) {
        if !self.show_settings_window {
            return;
        }
        let mut open = self.show_settings_window;
        let tab = &mut self.settings_tab;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .default_width(460.0)
            .min_width(400.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(tab, SettingsTab::Video, "Video");
                    ui.selectable_value(tab, SettingsTab::Audio, "Audio");
                    ui.selectable_value(tab, SettingsTab::Input, "Input");
                    ui.selectable_value(tab, SettingsTab::Advanced, "Advanced");
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match *tab {
                        SettingsTab::Video => {
                            ui.heading("Display");
                            ui.horizontal(|ui| {
                                ui.label("Theme:");
                                let before = config.ui.theme;
                                egui::ComboBox::from_id_salt("shell-theme")
                                    .selected_text(config.ui.theme.display_name())
                                    .show_ui(ui, |ui| {
                                        for theme in
                                            [AppTheme::Light, AppTheme::Dark, AppTheme::System]
                                        {
                                            ui.selectable_value(
                                                &mut config.ui.theme,
                                                theme,
                                                theme.display_name(),
                                            );
                                        }
                                    });
                                if before != config.ui.theme {
                                    save_config(config);
                                }
                            });
                            if ui
                                .checkbox(
                                    &mut config.ui.pixel_aspect_correction,
                                    "8:7 Pixel Aspect Ratio (NES native)",
                                )
                                .changed()
                            {
                                save_config(config);
                            }
                            if ui
                                .checkbox(&mut config.ui.show_fps, "Show FPS in status bar")
                                .changed()
                            {
                                save_config(config);
                            }
                            ui.separator();
                            settings_body(ui, config);
                        }
                        SettingsTab::Audio | SettingsTab::Advanced => settings_body(ui, config),
                        SettingsTab::Input => input_body(ui, config),
                    });
            });
        self.show_settings_window = open;
    }

    /// Render the first-run welcome modal.
    fn welcome_modal(&mut self, ctx: &egui::Context, config: &mut Config) {
        if !self.show_welcome {
            return;
        }
        egui::Window::new("Welcome to RustyNES")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.add_space(8.0);
                ui.label("A cycle-accurate NES emulator written in Rust.");
                ui.add_space(16.0);
                ui.label(egui::RichText::new("Quick start:").strong());
                ui.add_space(4.0);
                shortcuts_grid(ui, "welcome_shortcuts");
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("Get Started").clicked() {
                        self.show_welcome = false;
                        config.welcome_shown = true;
                        save_config(config);
                    }
                    if ui.button("Keyboard Shortcuts").clicked() {
                        self.show_shortcuts = true;
                    }
                });
            });
    }
}

/// Apply the configured [`AppTheme`] to the egui context. Called as the first
/// statement of the shell egui closure each frame so the chrome (and the
/// debugger panels) all render in the chosen theme.
pub fn apply_theme(ctx: &egui::Context, theme: AppTheme) {
    match theme {
        AppTheme::Light => ctx.set_visuals(egui::Visuals::light()),
        AppTheme::Dark => ctx.set_visuals(egui::Visuals::dark()),
        AppTheme::System => {
            // Use the windowing system's reported preference when available;
            // fall back to dark otherwise.
            match ctx.system_theme() {
                Some(egui::Theme::Light) => ctx.set_visuals(egui::Visuals::light()),
                _ => ctx.set_visuals(egui::Visuals::dark()),
            }
        }
    }
}

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

/// Render the About window.
fn about_window(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    egui::Window::new("About RustyNES")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("RustyNES");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.label("A cycle-accurate NES emulator written in Rust");
                ui.add_space(8.0);
                ui.hyperlink_to("GitHub", "https://github.com/doublegate/RustyNES");
                ui.add_space(8.0);
                ui.label("MIT OR Apache-2.0");
            });
        });
}

/// Render the keyboard-shortcuts window.
fn shortcuts_window(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    egui::Window::new("Keyboard Shortcuts")
        .open(open)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            shortcuts_grid(ui, "shortcuts_full");
        });
}

/// The shared shortcuts grid (the REAL default binds from `input.rs`).
fn shortcuts_grid(ui: &mut egui::Ui, id: &str) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([32.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Action").strong());
            ui.label(egui::RichText::new("Key").strong());
            ui.end_row();

            for (action, key) in [
                ("Open ROM", "F12 / drag & drop"),
                ("Save state", "F1"),
                ("Load state", "F4"),
                ("Rewind (hold)", "F5"),
                ("Reset", "F2"),
                ("Power cycle", "F3"),
                ("Toggle debugger", "`"),
                ("Quit", "Esc"),
                ("D-pad", "Arrow keys"),
                ("A button", "Z"),
                ("B button", "X"),
                ("Start", "Enter"),
                ("Select", "Right Shift"),
            ] {
                ui.label(action);
                ui.label(key);
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_shows_when_not_yet_shown() {
        let config = Config {
            welcome_shown: false,
            ..Default::default()
        };
        let shell = UiShell::new(&config);
        assert!(shell.show_welcome);
        assert!(shell.menu_visible);
    }

    #[test]
    fn welcome_hidden_once_shown() {
        let config = Config {
            welcome_shown: true,
            ..Default::default()
        };
        let shell = UiShell::new(&config);
        assert!(!shell.show_welcome);
    }

    #[test]
    fn status_message_expires() {
        let msg = StatusMessage::new("x", egui::Color32::WHITE, Duration::from_millis(0));
        assert!(msg.is_expired());
        let msg = StatusMessage::info("y");
        assert!(!msg.is_expired());
        assert!((msg.alpha() - 1.0).abs() < 1e-6);
    }
}
