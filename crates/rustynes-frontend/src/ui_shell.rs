//! Always-on desktop UX shell (v1.0.0).
//!
//! This module owns the persistent egui chrome that frames the NES image
//! independently of the (separately toggled) debugger overlay:
//!
//! - a top **menu bar** (File / Emulation / Tools / View / Debug / Help),
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
    /// v1.0.0 — toggle the menu bar (from the View menu).
    ToggleMenuBar,
    /// v1.0.0 — resize the window to an integer multiple of the NES resolution
    /// (1x..4x), from the View > Window Size menu.
    SetWindowScale(u32),
    /// v1.0.0 — cycle the inserted FDS disk side.
    CycleDiskSide,
    /// v1.0.0 — capture a screenshot of the current framebuffer (native).
    Screenshot,
    /// v1.0.0 — copy the current framebuffer to the system clipboard (native;
    /// the dispatch body is `#[cfg(not(wasm32))]`, the variant stays un-gated
    /// so the match remains exhaustive on every target).
    ScreenshotToClipboard,
    /// v1.0.0 — set the active save-state slot (0-7).
    SetSaveSlot(u8),
    /// v1.0.0 — save state to a specific slot.
    SaveStateSlot(u8),
    /// v1.0.0 — load state from a specific slot.
    LoadStateSlot(u8),
    /// v1.0.0 — open the Save-States manager window (thumbnail grid; native).
    OpenSaveStates,
    /// v1.0.0 — toggle TAS movie recording.
    MovieRecordToggle,
    /// v1.0.0 — toggle TAS movie playback.
    MoviePlayToggle,
    /// v1.0.0 — branch the current movie playback into a new recording.
    MovieBranch,
    /// v1.0.0 — insert a Vs. System coin (acceptor #1).
    InsertCoin,
    /// Step the emulator exactly one frame (meaningful while paused).
    FrameAdvance,
    /// v1.0.0 — set the emulation-speed factor (25%..300% presets).
    SetSpeed(f32),
    /// v1.0.0 — set the overscan-crop toggle (`[graphics] hide_overscan`).
    SetOverscan(bool),
    /// v1.0.0 — open a tool panel (Cheats / Settings / Netplay / ...).
    OpenPanel(crate::debugger::ToolPanel),
    /// v1.0.0 — open a chip-inspection panel + force the deep overlay visible.
    OpenChipPanel(crate::debugger::ChipPanel),
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
    /// v1.0.0 — the active save-state slot (0-7), mirrored from the app so the
    /// File -> Save Slot radio shows the current selection.
    pub active_slot: u8,
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
            active_slot: 0,
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
// These are independent per-frame status flags captured from the core, not a
// state machine — a bitfield would be less clear than named bools.
#[allow(clippy::struct_excessive_bools)]
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
    /// v1.0.0 (BUG-4) — whether a netplay session is active. The Pause menu item
    /// is disabled while this is set (pausing would stall the rollback session).
    pub netplay_active: bool,
    /// v1.0.0 — number of FDS disk sides (0 = not an FDS game). Enables the
    /// File-menu disk items.
    pub disk_sides: usize,
    /// v1.0.0 — whether the loaded game is a Vs. System title (enables the
    /// "Insert Coin" item).
    pub vs_system: bool,
    /// v1.0.0 — the human-readable mapper name (empty when unavailable).
    pub mapper_label: &'a str,
    /// v1.0.0 — the region label (`"NTSC"` / `"PAL"` / `"Dendy"`).
    pub region_label: &'a str,
    /// v1.0.0 — the configured run-ahead depth (frames).
    pub run_ahead: u32,
    /// v1.0.0 — the emulation-speed factor (1.0 = 100%); drives the Speed
    /// submenu checkmark + the status-bar speed readout (shown when != 1.0).
    pub speed: f32,
    /// v1.0.0 — whether emulation is paused (mirror; drives the paused overlay).
    pub paused: bool,
    /// v1.0.0 — whether a TAS movie is currently recording (drives the Tools
    /// menu Record/Stop label).
    pub movie_recording: bool,
    /// v1.0.0 — whether a TAS movie is currently playing back (drives the Tools
    /// menu Play/Stop label).
    pub movie_playing: bool,
}

impl UiShell {
    /// Build the always-on shell UI for this frame. Returns a [`ShellOutput`]
    /// carrying the menu action (if any) for the app to dispatch afterwards.
    ///
    /// `config` is edited in place (theme combo, 8:7 toggle, recent list);
    /// `settings_body` renders the Settings window's Video / Audio / Advanced
    /// tab body for the [`SettingsTab`] passed to it (so each tab shows only its
    /// own section), and `input_body` renders the Input tab — both reusing the
    /// existing debugger settings + input-rebind widgets so their live-apply
    /// plumbing is untouched.
    #[allow(clippy::too_many_lines)]
    pub fn build(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        frame: &ShellFrame<'_>,
        mut settings_body: impl FnMut(&mut egui::Ui, &mut Config, SettingsTab),
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

    /// Render the top menu bar (v1.0.0 IA — surfaces Netplay / RA / Cheats /
    /// Movies / Perf / save-slots / disk / screenshot directly instead of
    /// burying them in the debugger overlay). Every ROM-dependent item is
    /// disabled when no ROM is loaded; platform-only items (Open ROM, Netplay,
    /// Screenshot, RA) are `#[cfg]`-gated out on wasm.
    #[allow(clippy::too_many_lines)]
    fn menu_bar(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        frame: &ShellFrame<'_>,
        out: &mut ShellOutput,
    ) {
        use crate::debugger::{ChipPanel, ToolPanel};
        // Clone the system bindings up front so the accelerator hints can read
        // them without holding a `&config` borrow across the `&mut config` edits
        // (theme / aspect / fps / run-ahead) the closure also makes. The clone
        // is a handful of small `String`s, built once per frame — negligible.
        let keys = config.input.system.clone();
        egui::TopBottomPanel::top("shell_menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // ----- File -----
                ui.menu_button("File", |ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    if accel_item(ui, "Open ROM...", &keys.open_rom).clicked() {
                        out.action = Some(MenuAction::OpenRom);
                        ui.close_menu();
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    ui.menu_button("Open Recent", |ui| {
                        if config.recent_roms.paths.is_empty() {
                            ui.label("No recent ROMs");
                        } else {
                            for path in config.recent_roms.paths.clone() {
                                let name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string();
                                // (audit m3) gray out entries whose file is gone.
                                let exists = path.exists();
                                if ui.add_enabled(exists, egui::Button::new(name)).clicked() {
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

                    // FDS disk controls (only meaningful for FDS games).
                    if frame.disk_sides > 0 {
                        ui.separator();
                        if accel_item(ui, "Swap Disk Side", &keys.disk_swap).clicked() {
                            out.action = Some(MenuAction::CycleDiskSide);
                            ui.close_menu();
                        }
                    }

                    ui.separator();

                    if accel_enabled(ui, frame.rom_loaded, "Save State", &keys.save_state).clicked()
                    {
                        out.action = Some(MenuAction::SaveState);
                        ui.close_menu();
                    }
                    if accel_enabled(ui, frame.rom_loaded, "Load State", &keys.load_state).clicked()
                    {
                        out.action = Some(MenuAction::LoadState);
                        ui.close_menu();
                    }
                    ui.menu_button("Save Slot", |ui| {
                        for slot in 0u8..8 {
                            if ui
                                .radio(self.active_slot == slot, format!("Slot {}", slot + 1))
                                .clicked()
                            {
                                self.active_slot = slot;
                                out.action = Some(MenuAction::SetSaveSlot(slot));
                                ui.close_menu();
                            }
                        }
                    });
                    // BUG-1: build the submenus as DIRECT children of the File
                    // menu (not inside `add_enabled_ui`, whose nested UI scope
                    // breaks egui's sibling-hover auto-close), with disabled
                    // placeholders when no ROM is loaded.
                    if frame.rom_loaded {
                        ui.menu_button("Save to Slot", |ui| {
                            for slot in 0u8..8 {
                                if ui.button(format!("Slot {}", slot + 1)).clicked() {
                                    out.action = Some(MenuAction::SaveStateSlot(slot));
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.menu_button("Load from Slot", |ui| {
                            for slot in 0u8..8 {
                                if ui.button(format!("Slot {}", slot + 1)).clicked() {
                                    out.action = Some(MenuAction::LoadStateSlot(slot));
                                    ui.close_menu();
                                }
                            }
                        });
                    } else {
                        ui.add_enabled(false, egui::Button::new("Save to Slot"));
                        ui.add_enabled(false, egui::Button::new("Load from Slot"));
                    }

                    // v1.0.0 — the Save-States manager window (thumbnail grid).
                    // Native-only (the slots live on the filesystem).
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Save States...").clicked() {
                        out.action = Some(MenuAction::OpenSaveStates);
                        ui.close_menu();
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.separator();
                        if ui
                            .add_enabled(frame.rom_loaded, egui::Button::new("Take Screenshot"))
                            .clicked()
                        {
                            out.action = Some(MenuAction::Screenshot);
                            ui.close_menu();
                        }
                        // v1.0.0 — copy the current frame to the system clipboard
                        // (in addition to the save-to-PNG above). Native-only.
                        if ui
                            .add_enabled(
                                frame.rom_loaded,
                                egui::Button::new("Copy Screenshot to Clipboard"),
                            )
                            .clicked()
                        {
                            out.action = Some(MenuAction::ScreenshotToClipboard);
                            ui.close_menu();
                        }
                    }

                    ui.separator();

                    if accel_item(ui, "Quit", &keys.quit).clicked() {
                        out.action = Some(MenuAction::Quit);
                        ui.close_menu();
                    }
                });

                // ----- Emulation -----
                ui.menu_button("Emulation", |ui| {
                    let pause_label = if self.paused { "Resume" } else { "Pause" };
                    // (BUG-4) disabled during netplay. (UX3 BUG-1) show the
                    // pause/resume accelerator key alongside the label.
                    if ui
                        .add_enabled(
                            frame.rom_loaded && !frame.netplay_active,
                            egui::Button::new(pause_label).shortcut_text(accel_hint(&keys.pause)),
                        )
                        .clicked()
                    {
                        out.action = Some(MenuAction::TogglePause);
                        ui.close_menu();
                    }
                    if accel_enabled(ui, frame.rom_loaded, "Reset", &keys.reset).clicked() {
                        out.action = Some(MenuAction::Reset);
                        ui.close_menu();
                    }
                    if accel_enabled(ui, frame.rom_loaded, "Power Cycle", &keys.power_cycle)
                        .clicked()
                    {
                        out.action = Some(MenuAction::PowerCycle);
                        ui.close_menu();
                    }
                    ui.separator();
                    // Frame advance is meaningful while paused; enabled with a
                    // ROM loaded (a press while running is a no-op).
                    if accel_enabled(ui, frame.rom_loaded, "Frame Advance", &keys.frame_advance)
                        .clicked()
                    {
                        out.action = Some(MenuAction::FrameAdvance);
                        ui.close_menu();
                    }
                    // Fast-forward is a held key — surface it as a disabled hint
                    // (there is no toggle action; hold the key to engage it).
                    ui.add_enabled(
                        false,
                        egui::Button::new(format!("Fast Forward (hold {})", keys.fast_forward)),
                    );
                    ui.separator();
                    ui.menu_button(format!("Run-Ahead: {}", config.input.run_ahead), |ui| {
                        for n in 0u32..=3 {
                            if ui
                                .radio(config.input.run_ahead == n, format!("{n}"))
                                .clicked()
                            {
                                config.input.run_ahead = n;
                                save_config(config);
                                ui.close_menu();
                            }
                        }
                    });
                    // v1.0.0 — emulation-speed presets (transient; not
                    // persisted, so the menu always opens at 100% on launch).
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let speed_pct = (frame.speed * 100.0).round() as u32;
                    ui.menu_button(format!("Speed: {speed_pct}%"), |ui| {
                        for &preset in &[0.25_f32, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0] {
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let pct = (preset * 100.0).round() as u32;
                            // Float-equality is fine: the menu sets these exact
                            // preset values, and the keys step through the same.
                            #[allow(clippy::float_cmp)]
                            let selected = frame.speed == preset;
                            if ui.radio(selected, format!("{pct}%")).clicked() {
                                out.action = Some(MenuAction::SetSpeed(preset));
                                ui.close_menu();
                            }
                        }
                    });
                    // Region is read-only (no core setter): display only.
                    ui.add_enabled(
                        false,
                        egui::Button::new(format!("Region: {}", frame.region_label)),
                    );
                    if frame.vs_system
                        && accel_enabled(ui, frame.rom_loaded, "Vs. Insert Coin", &keys.insert_coin)
                            .clicked()
                    {
                        out.action = Some(MenuAction::InsertCoin);
                        ui.close_menu();
                    }
                });

                // ----- Tools -----
                ui.menu_button("Tools", |ui| {
                    if ui.button("Cheats...").clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Cheats));
                        ui.close_menu();
                    }
                    // BUG-1: direct child (not inside add_enabled_ui — see File).
                    if frame.rom_loaded {
                        ui.menu_button("Movies (TAS)", |ui| {
                            let rec_label = if frame.movie_recording {
                                "Stop Recording"
                            } else {
                                "Record"
                            };
                            if accel_item(ui, rec_label, &keys.movie_record).clicked() {
                                out.action = Some(MenuAction::MovieRecordToggle);
                                ui.close_menu();
                            }
                            let play_label = if frame.movie_playing {
                                "Stop Playback"
                            } else {
                                "Play"
                            };
                            if accel_item(ui, play_label, &keys.movie_play).clicked() {
                                out.action = Some(MenuAction::MoviePlayToggle);
                                ui.close_menu();
                            }
                            if accel_item(ui, "Branch", &keys.movie_branch).clicked() {
                                out.action = Some(MenuAction::MovieBranch);
                                ui.close_menu();
                            }
                        });
                    } else {
                        ui.add_enabled(false, egui::Button::new("Movies (TAS)"));
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui.button("Netplay...").clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Netplay));
                        ui.close_menu();
                    }
                    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
                    if ui.button("RetroAchievements...").clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Cheevos));
                        ui.close_menu();
                    }
                    if ui.button("Performance Monitor").clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Perf));
                        ui.close_menu();
                    }
                    if ui.button("Input Display").clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::InputDisplay));
                        ui.close_menu();
                    }
                });

                // ----- View -----
                ui.menu_button("View", |ui| {
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
                        .checkbox(&mut config.ui.pixel_aspect_correction, "8:7 Pixel Aspect")
                        .changed()
                    {
                        save_config(config);
                    }
                    // v1.0.0 — overscan crop (top/bottom 8 scanlines). Applied
                    // live via the menu action so the gfx letterbox updates.
                    if ui
                        .checkbox(&mut config.graphics.hide_overscan, "Hide Overscan")
                        .changed()
                    {
                        save_config(config);
                        out.action = Some(MenuAction::SetOverscan(config.graphics.hide_overscan));
                    }
                    // (audit m4) Fullscreen is a native-only window mode.
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        // (BUG-2) read-only mirror — never flip here.
                        let mut fs = self.fullscreen;
                        if accel_changed(ui, &mut fs, "Fullscreen", &keys.fullscreen) {
                            out.action = Some(MenuAction::ToggleFullscreen);
                            ui.close_menu();
                        }
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    ui.menu_button("Window Size", |ui| {
                        for (label, scale) in [
                            ("1x (100%)", 1u32),
                            ("2x (200%)", 2),
                            ("3x (300%)", 3),
                            ("4x (400%)", 4),
                        ] {
                            if ui.button(label).clicked() {
                                out.action = Some(MenuAction::SetWindowScale(scale));
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.checkbox(&mut config.ui.show_fps, "Show FPS").changed() {
                        save_config(config);
                    }
                    if ui
                        .checkbox(&mut config.ui.pause_on_focus_loss, "Pause When Unfocused")
                        .changed()
                    {
                        save_config(config);
                    }
                    let mut menu_bar = self.menu_visible;
                    if accel_changed(ui, &mut menu_bar, "Show Menu Bar", &keys.toggle_menu_bar) {
                        out.action = Some(MenuAction::ToggleMenuBar);
                        ui.close_menu();
                    }
                });

                // ----- Debug -----
                ui.menu_button("Debug", |ui| {
                    let mut dbg = frame.debugger_visible;
                    if accel_changed(ui, &mut dbg, "Show Debugger", &keys.debug_overlay) {
                        out.action = Some(MenuAction::ToggleDebugger);
                        ui.close_menu();
                    }
                    ui.separator();
                    for (label, panel) in [
                        ("CPU", ChipPanel::Cpu),
                        ("PPU", ChipPanel::Ppu),
                        ("APU", ChipPanel::Apu),
                        ("Memory", ChipPanel::Memory),
                        ("OAM", ChipPanel::Oam),
                        ("Mapper", ChipPanel::Mapper),
                        ("Trace Logger", ChipPanel::Trace),
                    ] {
                        if ui.button(label).clicked() {
                            out.action = Some(MenuAction::OpenChipPanel(panel));
                            ui.close_menu();
                        }
                    }
                });

                // ----- Help -----
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
                        // v1.0.0 polish — richer status: region, mapper, run-ahead.
                        if !frame.region_label.is_empty() {
                            ui.separator();
                            ui.label(frame.region_label);
                        }
                        if !frame.mapper_label.is_empty() {
                            ui.separator();
                            ui.label(frame.mapper_label);
                        }
                        if frame.run_ahead > 0 {
                            ui.separator();
                            ui.label(format!("Run-Ahead {}", frame.run_ahead));
                        }
                        // v1.0.0 — show the emulation speed only when off 100%.
                        #[allow(clippy::float_cmp)] // 1.0 is the exact preset.
                        if frame.speed != 1.0 {
                            ui.separator();
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let pct = (frame.speed * 100.0).round() as u32;
                            ui.colored_label(
                                egui::Color32::from_rgb(240, 200, 100),
                                format!("{pct}%"),
                            );
                        }
                        ui.separator();
                        if self.paused {
                            ui.colored_label(egui::Color32::YELLOW, "Paused");
                        } else if frame.netplay_active {
                            ui.colored_label(egui::Color32::from_rgb(80, 180, 240), "Netplay");
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
                        // v1.0.0 (BUG-8) — `current_fps()` returns the last
                        // rolling mean (e.g. 60.0) even while paused; show 0.0
                        // so the readout reflects that emulation has stopped.
                        let fps = if self.paused { 0.0 } else { frame.fps };
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("FPS: {fps:.1}"));
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
        settings_body: &mut impl FnMut(&mut egui::Ui, &mut Config, SettingsTab),
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
                            // Display chrome (theme / aspect / fps) is shell-only,
                            // so it lives here; the rest of the Video tab is the
                            // settings panel's `video_section`.
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
                            settings_body(ui, config, SettingsTab::Video);
                        }
                        // v1.0.0 settings split — each tab renders ONLY its own
                        // section (the prior catch-all rendered the whole body on
                        // every tab, duplicating every control).
                        SettingsTab::Audio => settings_body(ui, config, SettingsTab::Audio),
                        SettingsTab::Advanced => settings_body(ui, config, SettingsTab::Advanced),
                        SettingsTab::Input => input_body(ui, config),
                    });
            });
        self.show_settings_window = open;
    }

    /// Render the first-run welcome modal.
    ///
    /// (audit m2) — the modal is dismissible (the window's close `X`, the "Get
    /// Started" button, or clicking away), AND `welcome_shown` is persisted the
    /// FIRST time it is shown (not only on "Get Started") so it never re-nags
    /// even if the user closes it some other way.
    fn welcome_modal(&mut self, ctx: &egui::Context, config: &mut Config) {
        if !self.show_welcome {
            return;
        }
        // Persist on first display so a quit-without-clicking doesn't re-show it.
        if !config.welcome_shown {
            config.welcome_shown = true;
            save_config(config);
        }
        let mut open = true;
        egui::Window::new("Welcome to RustyNES")
            .open(&mut open)
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
                    }
                    if ui.button("Keyboard Shortcuts").clicked() {
                        self.show_shortcuts = true;
                    }
                });
            });
        if !open {
            self.show_welcome = false;
        }
    }
}

/// v1.0.0 — render a friendly accelerator hint for a keycode-name binding
/// (`config.input.system.*`), e.g. `"Backquote"` -> `` "`" ``, `"ShiftRight"`
/// -> `"R-Shift"`. Best-effort; unknown names pass through unchanged so a rebind
/// still shows something sensible.
fn accel_hint(key: &str) -> String {
    match key {
        "Backquote" => "`".to_string(),
        "Escape" => "Esc".to_string(),
        "Enter" => "Enter".to_string(),
        "ShiftRight" => "R-Shift".to_string(),
        "ShiftLeft" => "L-Shift".to_string(),
        k if k.starts_with("Key") => k.trim_start_matches("Key").to_string(),
        k if k.starts_with("Digit") => k.trim_start_matches("Digit").to_string(),
        k if k.starts_with("Arrow") => k.trim_start_matches("Arrow").to_string(),
        k => k.to_string(),
    }
}

/// v1.0.0 — a menu button with the accelerator hint right-aligned (tracks the
/// live rebind via `config.input.system.*`). Returns the click response.
fn accel_item(ui: &mut egui::Ui, label: &str, key: &str) -> egui::Response {
    ui.add(egui::Button::new(label).shortcut_text(accel_hint(key)))
}

/// v1.0.0 — [`accel_item`] gated by an `enabled` flag (ROM-dependent items).
fn accel_enabled(ui: &mut egui::Ui, enabled: bool, label: &str, key: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(label).shortcut_text(accel_hint(key)),
    )
}

/// v1.0.0 — a checkbox-style menu item with an accelerator hint. Returns `true`
/// when toggled this frame. The `value` mirror is shown but the caller drives
/// the real toggle through a dispatched action (read-only-mirror pattern).
fn accel_changed(ui: &mut egui::Ui, value: &mut bool, label: &str, key: &str) -> bool {
    let hint = accel_hint(key);
    let resp = ui.add(
        egui::Button::new(label)
            .shortcut_text(hint)
            .selected(*value),
    );
    if resp.clicked() {
        *value = !*value;
        true
    } else {
        false
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
                ui.label("Created by DoubleGate");
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
                ("Movie record", "F6"),
                ("Movie play", "F7"),
                ("Movie branch", "F8"),
                ("Swap disk side (FDS)", "F9"),
                ("Insert coin (Vs.)", "F10"),
                ("Fullscreen", "F11"),
                ("Toggle menu bar", "M"),
                ("Toggle debugger", "`"),
                ("Quit / exit fullscreen", "Esc"),
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
