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

use crate::config::{AppTheme, Config, UiConfig};

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

    /// Create an error message (red text, 5 seconds). v1.4.0 D1.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(
            text,
            egui::Color32::from_rgb(0xE0, 0x60, 0x60),
            Duration::from_secs(5),
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
    /// Video / display settings (theme, aspect, NTSC/CRT filter, palette).
    #[default]
    Video,
    /// v1.3.0 — the composable post-process shader stack + preset bank
    /// (split out of the Video tab into its own pane).
    Shaders,
    /// Audio settings.
    Audio,
    /// Input rebinding.
    Input,
    /// v1.3.0 — emulation behaviour: run-ahead latency + rewind
    /// (formerly the "Advanced" tab).
    Emulation,
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
    /// v1.3.0 — close the currently-loaded ROM and return to the no-ROM state.
    CloseRom,
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
    /// v1.6.0 B1 — import an external TAS movie (`.fm2` FCEUX / `.bk2`
    /// `BizHawk`) and begin playback against the running ROM.
    MovieImport,
    /// v1.6.0 B1 — export the current recording / loaded movie to an external
    /// TAS movie file (`.fm2` FCEUX / `.bk2` `BizHawk`) via the save dialog.
    MovieExport,
    /// v1.6.0 "Studio" Workstream G — toggle A/V (video + synchronized audio)
    /// recording. Start opens a native save dialog (`.mp4` / `.mkv`) and arms an
    /// `ffmpeg`-piped recorder; a second invocation stops + finalizes. Native +
    /// `av-record`-feature-gated (the dispatch body is
    /// `#[cfg(all(feature = "av-record", not(wasm32)))]`); the variant stays
    /// un-gated so the match remains exhaustive on every target.
    AvRecordToggle,
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
    /// v1.2.0 beta.2 (Workstream C3) — open a dialog to pick an HD-pack folder
    /// or `.zip` for the loaded ROM and enable substitution (native; the
    /// dispatch body is `#[cfg(all(feature = "hd-pack", not(wasm32)))]`, the
    /// variant stays un-gated so the match remains exhaustive on every target).
    LoadHdPack,
    /// v1.2.0 beta.2 (Workstream C3) — disable + unload the active HD-pack for
    /// the loaded ROM.
    UnloadHdPack,
    /// v1.4.0 Workstream D (D1) — open a dialog to pick a symbol/label file
    /// (`.sym` / Mesen `.mlb` / FCEUX `.nl`) and merge its labels into the
    /// debugger's annotation map (native; the dispatch body is
    /// `#[cfg(not(wasm32))]`, the variant stays un-gated so the match remains
    /// exhaustive on every target).
    LoadSymbols,
    /// v1.4.0 Workstream D (D1) — clear all loaded debugger symbols.
    ClearSymbols,
    /// v1.5.0 "Lens" Workstream I10 — open the in-app Documentation browser
    /// (native; the dispatch body is `#[cfg(not(wasm32))]`, the variant stays
    /// un-gated so the match remains exhaustive on every target).
    OpenDocumentation,
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
    /// v1.5.0 "Lens" Workstream I9 — the device whose bindings the Keyboard
    /// Shortcuts window currently shows (Player 1..4 / Power Pad / Family BASIC),
    /// below the emulator-hotkey section.
    pub shortcuts_device: ShortcutsDevice,
}

/// v1.5.0 "Lens" Workstream I9 — selects which device the Keyboard Shortcuts
/// window shows in its controller section.
///
/// The emulator hotkeys are always listed above a separator; this picks the
/// per-device key map shown below them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShortcutsDevice {
    /// Standard controller, player 1 (the default view).
    #[default]
    Player1,
    /// Standard controller, player 2.
    Player2,
    /// Standard controller, player 3 (Four Score).
    Player3,
    /// Standard controller, player 4 (Four Score).
    Player4,
    /// NES Power Pad / Family Trainer mat (fixed default mat keys).
    PowerPad,
    /// Family BASIC / Subor keyboard (host-key matrix, fixed mapping).
    FamilyKeyboard,
}

impl ShortcutsDevice {
    /// Display label for the selector + section header.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Player1 => "Player 1",
            Self::Player2 => "Player 2",
            Self::Player3 => "Player 3 (Four Score)",
            Self::Player4 => "Player 4 (Four Score)",
            Self::PowerPad => "Power Pad / Family Trainer",
            Self::FamilyKeyboard => "Family BASIC keyboard",
        }
    }
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
            shortcuts_device: ShortcutsDevice::default(),
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
    /// v1.6.0 "Studio" Workstream G — whether an A/V recording is currently
    /// armed (drives the Tools menu "Record A/V" / "Stop A/V Recording" label).
    /// Always present so the struct literal is target-agnostic; only read by
    /// the `av-record`-gated menu item.
    pub av_recording: bool,
    /// v1.5.0 "Lens" Workstream I2 — whether Fast Forward is currently engaged
    /// (the bound key is held). Drives the Emulation-menu Fast Forward item so
    /// it shows a live "ON" state instead of a permanently greyed hint.
    pub fast_forwarding: bool,
    /// v1.5.0 "Lens" Workstream I7 — a compact `RetroAchievements` status string
    /// for the status bar (e.g. `"RA 12/40 (240 pts) HARDCORE"`), relocated
    /// from the retired-overlay HUD readout. `None` when the feature is off, no
    /// user is logged in, or no game is loaded. Shown between the emulator-state
    /// label and the FPS counter.
    pub ra_status: Option<String>,
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
        root_ui: &mut egui::Ui,
        config: &mut Config,
        frame: &ShellFrame<'_>,
        mut settings_body: impl FnMut(&mut egui::Ui, &mut Config, SettingsTab),
        mut input_body: impl FnMut(&mut egui::Ui, &mut Config),
    ) -> ShellOutput {
        self.clear_expired_status();
        let mut out = ShellOutput::default();

        // egui 0.34 — the top/bottom panels are shown inside the root `Ui`
        // (`show_inside`); the floating windows + modals still take the
        // `&Context`, reachable via `ui.ctx()`.
        let ctx = root_ui.ctx().clone();
        if self.menu_visible {
            self.menu_bar(root_ui, config, frame, &mut out);
        }
        self.status_bar(root_ui, frame, config);
        self.settings_window(&ctx, config, &mut settings_body, &mut input_body);
        self.welcome_modal(&ctx, config);
        about_window(&ctx, &mut self.show_about);
        self.shortcuts_window(&ctx, config);
        #[cfg(not(target_arch = "wasm32"))]
        crate::about_fx::render(&ctx);

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
        root_ui: &mut egui::Ui,
        config: &mut Config,
        frame: &ShellFrame<'_>,
        out: &mut ShellOutput,
    ) {
        use crate::debugger::{ChipPanel, ToolPanel};
        use crate::icons::{glyph, label as ic};
        // Clone the system bindings up front so the accelerator hints can read
        // them without holding a `&config` borrow across the `&mut config` edits
        // (theme / aspect / fps / run-ahead) the closure also makes. The clone
        // is a handful of small `String`s, built once per frame — negligible.
        let keys = config.input.system.clone();

        // v1.2.0 Workstream H1 — per-item contextual enable predicates derived
        // from the live frame state, mirroring GeraNES `MenuUI.inl`:
        //
        // - `rom_change_restricted`: while a netplay session is active the
        //   loaded ROM must not change under the rollback session — disables
        //   Open ROM / Open Recent (GeraNES `netplayRomChangeRestricted`).
        // - `replay_locked`: while a TAS movie is recording OR playing back, the
        //   session owns the input/state timeline — disables load-state and the
        //   reset/power-cycle/disk actions that would desync the replay
        //   (GeraNES `replayInteractionLocked` / `replayRecordingActive`). The
        //   per-item Record/Play gating below additionally distinguishes the
        //   recording-vs-playing case (you can't Record over a Playback, etc.).
        //
        // These only gate which items are *clickable*; the dispatched
        // `MenuAction` set is unchanged.
        let rom = frame.rom_loaded;
        let rom_change_restricted = frame.netplay_active;
        let replay_locked = frame.movie_recording || frame.movie_playing;
        // A loaded ROM whose state/timeline the user may freely manipulate
        // (not netplay-locked, not replay-locked). Used to gate the
        // state-mutating items uniformly.
        let rom_interactive = rom && !replay_locked;
        // egui-0.34 menu close model (BUG-3 investigation). The new `MenuBar`
        // builds each top item as a `MenuButton` that inherits the bar's
        // `MenuConfig::close_behavior`, which defaults to
        // `PopupCloseBehavior::CloseOnClick` — a click ANYWHERE (inside or
        // outside the popup) closes the open top menu, *gated* by egui's
        // `is_deepest_open_sub_menu` / `MenuState` tracking: while egui believes
        // a submenu is still open, the top-level close is deferred to that
        // submenu. We deliberately keep every item a DIRECT child of its menu
        // (never wrapped in `add_enabled_ui`, whose nested UI scope perturbs that
        // tracking — the long-standing BUG-1 caveat) and rely on egui's built-in
        // CloseOnClick + Escape handling rather than a bespoke close path, so we
        // don't regress item selection. The "menu lingers until several clicks"
        // report (BUG-3) needs an on-device repro (which menu, click-vs-hover,
        // what finally dismisses it) to pin the exact `MenuState` trigger; we do
        // NOT hack the interaction blind here.
        egui::Panel::top("shell_menu_bar").show_inside(root_ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                // ----- File -----
                ui.menu_button(ic(glyph::FILE, "File"), |ui| {
                    // (H1) Open ROM / Open Recent change the loaded ROM — locked
                    // out while a netplay session is active (a ROM swap would
                    // desync the rollback peers).
                    #[cfg(not(target_arch = "wasm32"))]
                    if accel_enabled(
                        ui,
                        !rom_change_restricted,
                        &ic(glyph::FOLDER_OPEN, "Open ROM..."),
                        &keys.open_rom,
                    )
                    .clicked()
                    {
                        out.action = Some(MenuAction::OpenRom);
                        ui.close();
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    if rom_change_restricted {
                        // Disabled placeholder (a submenu can't carry an
                        // `enabled` flag the way a button does, and an
                        // `add_enabled_ui` wrapper breaks egui's sibling-hover
                        // auto-close — BUG-1), so surface it as a greyed item.
                        ui.add_enabled(
                            false,
                            egui::Button::new(ic(glyph::CLOCK_ROTATE_LEFT, "Open Recent")),
                        );
                    } else {
                        ui.menu_button(ic(glyph::CLOCK_ROTATE_LEFT, "Open Recent"), |ui| {
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
                                    if ui
                                        .add_enabled(
                                            exists,
                                            egui::Button::new(ic(glyph::FOLDER_OPEN, &name)),
                                        )
                                        .clicked()
                                    {
                                        out.action = Some(MenuAction::LoadRom(path));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                if ui.button(ic(glyph::XMARK, "Clear Recent")).clicked() {
                                    out.action = Some(MenuAction::ClearRecent);
                                    ui.close();
                                }
                            }
                        });
                    }

                    // v1.3.0 — close the current ROM (back to the no-ROM state).
                    // A ROM change, so locked during netplay like Open ROM/Recent.
                    if ui
                        .add_enabled(
                            rom && !rom_change_restricted,
                            egui::Button::new(ic(glyph::XMARK, "Close ROM")),
                        )
                        .clicked()
                    {
                        out.action = Some(MenuAction::CloseRom);
                        ui.close();
                    }

                    ui.separator();

                    // v1.3.0 menu reorg — all save-state actions grouped under a
                    // single "Save States" submenu (Save/Load to the active slot,
                    // the per-slot pickers, and the thumbnail-grid manager). The
                    // FDS "Swap Disk Side" item moved to the Emulation menu.
                    ui.menu_button(ic(glyph::FLOPPY_DISK, "Save States"), |ui| {
                        // (H1) Save-state SAVE is allowed during playback (it just
                        // snapshots) but not while RECORDING (the GeraNES rule:
                        // saving mid-record is fine; loading is the dangerous one).
                        // We keep SAVE enabled whenever a ROM is loaded.
                        if accel_enabled(
                            ui,
                            rom,
                            &ic(glyph::FLOPPY_DISK, "Save State"),
                            &keys.save_state,
                        )
                        .clicked()
                        {
                            out.action = Some(MenuAction::SaveState);
                            ui.close();
                        }
                        // (H1) Load-state restores the timeline — forbidden while a
                        // movie is recording (rewrites the recording) OR playing
                        // back (desyncs playback). Mirrors GeraNES
                        // `replayRecordingActive` / `replayInteractionLocked`.
                        if accel_enabled(
                            ui,
                            rom_interactive,
                            &ic(glyph::DOWNLOAD, "Load State"),
                            &keys.load_state,
                        )
                        .clicked()
                        {
                            out.action = Some(MenuAction::LoadState);
                            ui.close();
                        }
                        ui.separator();
                        ui.menu_button(ic(glyph::FLOPPY_DISK, "Active Slot"), |ui| {
                            for slot in 0u8..8 {
                                if ui
                                    .radio(self.active_slot == slot, format!("Slot {}", slot + 1))
                                    .clicked()
                                {
                                    self.active_slot = slot;
                                    out.action = Some(MenuAction::SetSaveSlot(slot));
                                    ui.close();
                                }
                            }
                        });
                        // BUG-1: build the submenus as DIRECT children (not inside
                        // `add_enabled_ui`, whose nested UI scope breaks egui's
                        // sibling-hover auto-close), with disabled placeholders
                        // when no ROM is loaded.
                        // (H1) Save-to-slot needs only a ROM; Load-from-slot is
                        // additionally replay-locked (same rule as Load State).
                        if rom {
                            ui.menu_button(ic(glyph::FLOPPY_DISK, "Save to Slot"), |ui| {
                                for slot in 0u8..8 {
                                    if ui.button(format!("Slot {}", slot + 1)).clicked() {
                                        out.action = Some(MenuAction::SaveStateSlot(slot));
                                        ui.close();
                                    }
                                }
                            });
                        } else {
                            ui.add_enabled(
                                false,
                                egui::Button::new(ic(glyph::FLOPPY_DISK, "Save to Slot")),
                            );
                        }
                        if rom_interactive {
                            ui.menu_button(ic(glyph::DOWNLOAD, "Load from Slot"), |ui| {
                                for slot in 0u8..8 {
                                    if ui.button(format!("Slot {}", slot + 1)).clicked() {
                                        out.action = Some(MenuAction::LoadStateSlot(slot));
                                        ui.close();
                                    }
                                }
                            });
                        } else {
                            ui.add_enabled(
                                false,
                                egui::Button::new(ic(glyph::DOWNLOAD, "Load from Slot")),
                            );
                        }

                        // v1.0.0 — the Save-States manager window (thumbnail grid).
                        // Native slots live on the filesystem; v1.4.0 E2 added the
                        // browser equivalent backed by IndexedDB (`wasm_save_states`),
                        // so the manager is now available on both. (H1) The grid is
                        // keyed on the loaded ROM's hash — needs a ROM.
                        ui.separator();
                        if ui
                            .add_enabled(
                                rom,
                                egui::Button::new(ic(glyph::IMAGE, "Manage States...")),
                            )
                            .clicked()
                        {
                            out.action = Some(MenuAction::OpenSaveStates);
                            ui.close();
                        }
                    });

                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.separator();
                        if ui
                            .add_enabled(
                                frame.rom_loaded,
                                egui::Button::new(ic(glyph::IMAGE, "Take Screenshot")),
                            )
                            .clicked()
                        {
                            out.action = Some(MenuAction::Screenshot);
                            ui.close();
                        }
                        // v1.0.0 — copy the current frame to the system clipboard
                        // (in addition to the save-to-PNG above). Native-only.
                        if ui
                            .add_enabled(
                                frame.rom_loaded,
                                egui::Button::new(ic(
                                    glyph::CLIPBOARD,
                                    "Copy Screenshot to Clipboard",
                                )),
                            )
                            .clicked()
                        {
                            out.action = Some(MenuAction::ScreenshotToClipboard);
                            ui.close();
                        }
                    }

                    ui.separator();

                    if accel_item(ui, &ic(glyph::RIGHT_FROM_BRACKET, "Quit"), &keys.quit).clicked()
                    {
                        out.action = Some(MenuAction::Quit);
                        ui.close();
                    }
                });

                // ----- Emulation -----
                ui.menu_button(ic(glyph::CALCULATOR, "Emulation"), |ui| {
                    let pause_label = if self.paused {
                        ic(glyph::PLAY, "Resume")
                    } else {
                        ic(glyph::PAUSE, "Pause")
                    };
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
                        ui.close();
                    }
                    // (H1) Reset / Power-cycle restart the session — locked
                    // during netplay (desyncs peers) and during a replay
                    // (diverges the timeline). Mirrors GeraNES Reset gating
                    // (`!netplayClientRestricted && !isReplayResetRestricted`).
                    let hw_interactive = rom && !rom_change_restricted && !replay_locked;
                    if accel_enabled(
                        ui,
                        hw_interactive,
                        &ic(glyph::ROTATE_RIGHT, "Reset"),
                        &keys.reset,
                    )
                    .clicked()
                    {
                        out.action = Some(MenuAction::Reset);
                        ui.close();
                    }
                    if accel_enabled(
                        ui,
                        hw_interactive,
                        &ic(glyph::ROTATE_RIGHT, "Power Cycle"),
                        &keys.power_cycle,
                    )
                    .clicked()
                    {
                        out.action = Some(MenuAction::PowerCycle);
                        ui.close();
                    }
                    ui.separator();
                    // v1.5.0 I2 — Frame Advance steps exactly one frame and is
                    // only meaningful WHILE PAUSED (a press while running is a
                    // no-op in `request_frame_advance`). The menu item now mirrors
                    // that: enabled only when a ROM is loaded AND paused AND not
                    // replay/netplay-locked — so it never looks clickable when it
                    // would silently do nothing. (H1) Locked during netplay (the
                    // peers drive frame stepping).
                    if accel_enabled(
                        ui,
                        rom && self.paused && !rom_change_restricted,
                        &ic(glyph::FORWARD_STEP, "Frame Advance"),
                        &keys.frame_advance,
                    )
                    .clicked()
                    {
                        out.action = Some(MenuAction::FrameAdvance);
                        ui.close();
                    }
                    // v1.5.0 I2 — Fast Forward is a held key (no toggle action),
                    // but the item is no longer a permanently-greyed dead hint: it
                    // reads as a live status row showing whether FF is currently
                    // engaged plus the bound key to hold. Enabled-looking while a
                    // ROM is loaded so the "(hold X)" affordance is legible.
                    let ff_label = if frame.fast_forwarding {
                        format!("Fast Forward: ON (hold {})", keys.fast_forward)
                    } else {
                        format!("Fast Forward (hold {})", keys.fast_forward)
                    };
                    ui.add_enabled(
                        rom && !rom_change_restricted,
                        egui::Button::new(ic(glyph::FORWARD_FAST, &ff_label)),
                    )
                    .on_hover_text(
                        "Hold the bound key to run unthrottled (audio muted). \
                         Rebind in Settings -> Input.",
                    );
                    ui.separator();
                    ui.menu_button(
                        ic(
                            glyph::SLIDERS,
                            &format!("Run-Ahead: {}", config.input.run_ahead),
                        ),
                        |ui| {
                            for n in 0u32..=3 {
                                if ui
                                    .radio(config.input.run_ahead == n, format!("{n}"))
                                    .clicked()
                                {
                                    config.input.run_ahead = n;
                                    save_config(config);
                                    ui.close();
                                }
                            }
                        },
                    );
                    // v1.0.0 — emulation-speed presets (transient; not
                    // persisted, so the menu always opens at 100% on launch).
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let speed_pct = (frame.speed * 100.0).round() as u32;
                    // (H1) The Speed submenu opens only with a ROM; during a
                    // netplay session only the 100% preset is selectable (the
                    // peers run lockstep at the console rate). Mirrors GeraNES
                    // `isNetplaySpeedRestricted` (only `Normal` enabled).
                    ui.add_enabled_ui(rom, |ui| {
                        ui.menu_button(ic(glyph::SLIDERS, &format!("Speed: {speed_pct}%")), |ui| {
                            for &preset in &[0.25_f32, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0] {
                                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                                let pct = (preset * 100.0).round() as u32;
                                // Float-equality is fine: the menu sets these
                                // exact preset values, and the keys step the same.
                                #[allow(clippy::float_cmp)]
                                let selected = frame.speed == preset;
                                #[allow(clippy::float_cmp)]
                                let preset_enabled = !rom_change_restricted || preset == 1.0;
                                if ui
                                    .add_enabled(
                                        preset_enabled,
                                        egui::RadioButton::new(selected, format!("{pct}%")),
                                    )
                                    .clicked()
                                {
                                    out.action = Some(MenuAction::SetSpeed(preset));
                                    ui.close();
                                }
                            }
                        });
                    });
                    // Region is read-only (no core setter): display only.
                    ui.add_enabled(
                        false,
                        egui::Button::new(ic(
                            glyph::GLOBE,
                            &format!("Region: {}", frame.region_label),
                        )),
                    );
                    // (H1) Inserting a Vs. coin is a hardware action — locked
                    // during netplay (the host drives arcade hardware).
                    if frame.vs_system
                        && accel_enabled(
                            ui,
                            rom && !rom_change_restricted,
                            &ic(glyph::COINS, "Vs. Insert Coin"),
                            &keys.insert_coin,
                        )
                        .clicked()
                    {
                        out.action = Some(MenuAction::InsertCoin);
                        ui.close();
                    }
                    // v1.3.0 menu reorg — FDS disk controls (only meaningful for
                    // FDS games; moved here from the File menu). (H1) Disk-swap
                    // mutates the running session — locked during a replay (it
                    // would diverge the recorded timeline).
                    if frame.disk_sides > 0 {
                        ui.separator();
                        if accel_enabled(
                            ui,
                            !replay_locked,
                            &ic(glyph::FLOPPY_DISK, "Swap Disk Side"),
                            &keys.disk_swap,
                        )
                        .clicked()
                        {
                            out.action = Some(MenuAction::CycleDiskSide);
                            ui.close();
                        }
                    }
                });

                // ----- View -----
                ui.menu_button(ic(glyph::EYE, "View"), |ui| {
                    if ui.button(ic(glyph::GEAR, "Settings...")).clicked() {
                        self.show_settings_window = true;
                        ui.close();
                    }
                    ui.menu_button(ic(glyph::PALETTE, "Theme"), |ui| {
                        for theme in AppTheme::all() {
                            if ui
                                .radio_value(&mut config.ui.theme, theme, theme.display_name())
                                .clicked()
                            {
                                save_config(config);
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .checkbox(
                            &mut config.ui.pixel_aspect_correction,
                            ic(glyph::TV, "8:7 Pixel Aspect"),
                        )
                        .changed()
                    {
                        save_config(config);
                    }
                    // v1.0.0 — overscan crop (top/bottom 8 scanlines). Applied
                    // live via the menu action so the gfx letterbox updates.
                    if ui
                        .checkbox(
                            &mut config.graphics.hide_overscan,
                            ic(glyph::TV, "Hide Overscan"),
                        )
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
                        if accel_changed(
                            ui,
                            &mut fs,
                            &ic(glyph::EXPAND, "Fullscreen"),
                            &keys.fullscreen,
                        ) {
                            out.action = Some(MenuAction::ToggleFullscreen);
                            ui.close();
                        }
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    ui.menu_button(ic(glyph::TV, "Window Size"), |ui| {
                        for (label, scale) in [
                            ("1x (100%)", 1u32),
                            ("2x (200%)", 2),
                            ("3x (300%)", 3),
                            ("4x (400%)", 4),
                        ] {
                            if ui.button(label).clicked() {
                                out.action = Some(MenuAction::SetWindowScale(scale));
                                ui.close();
                            }
                        }
                    });
                    if ui
                        .checkbox(&mut config.ui.show_fps, ic(glyph::GAUGE, "Show FPS"))
                        .changed()
                    {
                        save_config(config);
                    }
                    if ui
                        .checkbox(
                            &mut config.ui.pause_on_focus_loss,
                            ic(glyph::PAUSE, "Pause When Unfocused"),
                        )
                        .changed()
                    {
                        save_config(config);
                    }
                    let mut menu_bar = self.menu_visible;
                    if accel_changed(
                        ui,
                        &mut menu_bar,
                        &ic(glyph::BARS, "Show Menu Bar"),
                        &keys.toggle_menu_bar,
                    ) {
                        out.action = Some(MenuAction::ToggleMenuBar);
                        ui.close();
                    }
                });

                // ----- Tools -----
                ui.menu_button(ic(glyph::WRENCH, "Tools"), |ui| {
                    if ui
                        .button(ic(glyph::WAND_MAGIC_SPARKLES, "Cheats..."))
                        .clicked()
                    {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Cheats));
                        ui.close();
                    }
                    // BUG-1: direct child (not inside add_enabled_ui — see File).
                    // (H1) The Movies submenu is unavailable during a netplay
                    // session (a rollback session cannot also be a TAS movie).
                    if rom && !rom_change_restricted {
                        ui.menu_button(ic(glyph::VIDEO, "Movies (TAS)"), |ui| {
                            // Record toggles record on/off; it must be locked
                            // while a movie is PLAYING (can't record over a
                            // playback). The toggle-off case (already recording)
                            // stays enabled so the user can stop.
                            let rec_label = if frame.movie_recording {
                                ic(glyph::STOP, "Stop Recording")
                            } else {
                                ic(glyph::VIDEO, "Record")
                            };
                            let rec_enabled = frame.movie_recording || !frame.movie_playing;
                            if accel_enabled(ui, rec_enabled, &rec_label, &keys.movie_record)
                                .clicked()
                            {
                                out.action = Some(MenuAction::MovieRecordToggle);
                                ui.close();
                            }
                            // Play toggles playback; locked while RECORDING. The
                            // toggle-off (already playing) stays enabled to stop.
                            let play_label = if frame.movie_playing {
                                ic(glyph::STOP, "Stop Playback")
                            } else {
                                ic(glyph::PLAY, "Play")
                            };
                            let play_enabled = frame.movie_playing || !frame.movie_recording;
                            if accel_enabled(ui, play_enabled, &play_label, &keys.movie_play)
                                .clicked()
                            {
                                out.action = Some(MenuAction::MoviePlayToggle);
                                ui.close();
                            }
                            // Branch forks the CURRENT playback into a new
                            // recording — only meaningful while playing back.
                            if accel_enabled(
                                ui,
                                frame.movie_playing,
                                &ic(glyph::VIDEO, "Branch"),
                                &keys.movie_branch,
                            )
                            .clicked()
                            {
                                out.action = Some(MenuAction::MovieBranch);
                                ui.close();
                            }
                            ui.separator();
                            // v1.6.0 B1 — external TAS movie interop (FCEUX
                            // `.fm2` / BizHawk `.bk2`). Import begins playback
                            // (locked while recording, like Play); Export writes
                            // the current recording / loaded movie (enabled when
                            // a movie exists to export).
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                let import_enabled = !frame.movie_recording;
                                if ui
                                    .add_enabled(
                                        import_enabled,
                                        egui::Button::new(ic(
                                            glyph::FOLDER_OPEN,
                                            "Import (.fm2 / .bk2)",
                                        )),
                                    )
                                    .clicked()
                                {
                                    out.action = Some(MenuAction::MovieImport);
                                    ui.close();
                                }
                                let export_enabled = frame.movie_recording || frame.movie_playing;
                                if ui
                                    .add_enabled(
                                        export_enabled,
                                        egui::Button::new(ic(
                                            glyph::FLOPPY_DISK,
                                            "Export (.fm2 / .bk2)",
                                        )),
                                    )
                                    .clicked()
                                {
                                    out.action = Some(MenuAction::MovieExport);
                                    ui.close();
                                }
                            }
                        });
                    } else {
                        ui.add_enabled(false, egui::Button::new(ic(glyph::VIDEO, "Movies (TAS)")));
                    }
                    // v1.6.0 "Studio" Workstream G — A/V recording (native +
                    // `av-record`-gated). Start opens a save dialog + arms an
                    // ffmpeg-piped recorder; a second click stops + finalizes.
                    // Needs a loaded ROM to record anything; the stop case stays
                    // enabled while armed so the user can finish.
                    #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
                    {
                        let av_label = if frame.av_recording {
                            ic(glyph::STOP, "Stop A/V Recording")
                        } else {
                            ic(glyph::VIDEO, "Record A/V...")
                        };
                        let av_enabled = frame.av_recording || rom;
                        if ui
                            .add_enabled(av_enabled, egui::Button::new(av_label))
                            .clicked()
                        {
                            out.action = Some(MenuAction::AvRecordToggle);
                            ui.close();
                        }
                    }
                    // (H1) Opening the Netplay panel is locked while a replay
                    // (TAS movie) owns the session. Mirrors GeraNES Netplay
                    // gating (`!replayInteractionLocked`).
                    #[cfg(not(target_arch = "wasm32"))]
                    if ui
                        .add_enabled(
                            !replay_locked,
                            egui::Button::new(ic(glyph::WIFI, "Netplay...")),
                        )
                        .clicked()
                    {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Netplay));
                        ui.close();
                    }
                    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
                    if ui
                        .button(ic(glyph::TROPHY, "RetroAchievements..."))
                        .clicked()
                    {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Cheevos));
                        ui.close();
                    }
                    if ui.button(ic(glyph::GAMEPAD, "Input Display")).clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::InputDisplay));
                        ui.close();
                    }
                    // v1.5.0 "Lens" Workstream A1 — live Input Miniatures overlay
                    // (every connected device, real-time button/axis state).
                    if ui.button(ic(glyph::GAMEPAD, "Input Miniatures")).clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::InputMiniatures));
                        ui.close();
                    }
                    // v1.3.0 menu reorg — NSF/NSFe music player (moved here from
                    // the Debug menu; it is a playback tool, not a chip inspector).
                    if ui.button(ic(glyph::MUSIC, "NSF Player")).clicked() {
                        out.action = Some(MenuAction::OpenChipPanel(ChipPanel::Nsf));
                        ui.close();
                    }
                    // v1.5.0 "Lens" Workstream C2 — Replay / TAS window (device
                    // topology + timebase + branch/seek UX over the .rnm machinery).
                    if ui.button(ic(glyph::VIDEO, "Replay / TAS")).clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Replay));
                        ui.close();
                    }
                    // v1.6.0 "Studio" Workstream A2 — TAStudio piano-roll TAS
                    // editor. Needs a loaded ROM (the editor anchors on the
                    // current emulator state as the project's frame 0).
                    if ui
                        .add_enabled(rom, egui::Button::new(ic(glyph::VIDEO, "TAStudio")))
                        .clicked()
                    {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::TasStudio));
                        ui.close();
                    }
                    // (H1) The ROM Database editor needs a loaded ROM to edit.
                    if ui
                        .add_enabled(rom, egui::Button::new(ic(glyph::DATABASE, "ROM Database")))
                        .clicked()
                    {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::GameDb));
                        ui.close();
                    }
                    // v1.3.0 menu reorg — HD-pack loader (v1.2.0 C3), folded in
                    // from the former standalone "Mod" menu as a Tools submenu;
                    // native + `hd-pack`-feature-gated. (H1) Load/unload needs a
                    // loaded ROM (the pack is keyed on the ROM hash) and is locked
                    // while a netplay/replay session owns presentation.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    ui.menu_button(ic(glyph::PUZZLE_PIECE, "HD Pack"), |ui| {
                        let mod_enabled = rom && !rom_change_restricted && !replay_locked;
                        if ui
                            .add_enabled(
                                mod_enabled,
                                egui::Button::new(ic(glyph::FOLDER_OPEN, "Load HD Pack...")),
                            )
                            .clicked()
                        {
                            out.action = Some(MenuAction::LoadHdPack);
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                mod_enabled,
                                egui::Button::new(ic(glyph::XMARK, "Unload HD Pack")),
                            )
                            .clicked()
                        {
                            out.action = Some(MenuAction::UnloadHdPack);
                            ui.close();
                        }
                        ui.separator();
                        // v1.5.0 "Lens" Workstream A4 — per-pixel composition trace.
                        if ui
                            .button(ic(glyph::MAGNIFYING_GLASS, "Pixel Inspector"))
                            .clicked()
                        {
                            out.action = Some(MenuAction::OpenPanel(ToolPanel::HdPixelInspector));
                            ui.close();
                        }
                    });
                });

                // ----- Debug -----
                ui.menu_button(ic(glyph::BUG, "Debug"), |ui| {
                    let mut dbg = frame.debugger_visible;
                    if accel_changed(
                        ui,
                        &mut dbg,
                        &ic(glyph::BUG, "Show Debugger"),
                        &keys.debug_overlay,
                    ) {
                        out.action = Some(MenuAction::ToggleDebugger);
                        ui.close();
                    }
                    // v1.3.0 menu reorg — the Performance Monitor is a debug
                    // tool, grouped here (moved from Tools).
                    if ui.button(ic(glyph::GAUGE, "Performance Monitor")).clicked() {
                        out.action = Some(MenuAction::OpenPanel(ToolPanel::Perf));
                        ui.close();
                    }
                    ui.separator();
                    // Chip / state inspectors. (NSF Player moved to the Tools menu
                    // in v1.3.0 — it is a playback tool, not a chip inspector.)
                    for (icon, label, panel) in [
                        (glyph::MICROCHIP, "CPU", ChipPanel::Cpu),
                        (glyph::MICROCHIP, "PPU", ChipPanel::Ppu),
                        (glyph::VOLUME_HIGH, "APU", ChipPanel::Apu),
                        (glyph::MEMORY, "Memory", ChipPanel::Memory),
                        (glyph::MEMORY, "Memory Compare", ChipPanel::MemoryCompare),
                        (glyph::MEMORY, "OAM", ChipPanel::Oam),
                        (glyph::PUZZLE_PIECE, "Mapper", ChipPanel::Mapper),
                        (glyph::CLIPBOARD, "Trace Logger", ChipPanel::Trace),
                        (glyph::CLIPBOARD, "Watch / Breakpoints", ChipPanel::Watch),
                        (glyph::CLIPBOARD, "Event Viewer", ChipPanel::Events),
                        (glyph::CODE, "Lua Script", ChipPanel::Script),
                    ] {
                        if ui.button(ic(icon, label)).clicked() {
                            out.action = Some(MenuAction::OpenChipPanel(panel));
                            ui.close();
                        }
                    }
                    // v1.4.0 Workstream D (D1) — symbol/label files annotate the
                    // disassembler + breakpoint + trace views. Native-only (it
                    // reads a picked file).
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.separator();
                        if ui
                            .button(ic(glyph::FILE, "Load Symbols (.sym/.mlb/.nl)..."))
                            .clicked()
                        {
                            out.action = Some(MenuAction::LoadSymbols);
                            ui.close();
                        }
                        if ui.button(ic(glyph::XMARK, "Clear Symbols")).clicked() {
                            out.action = Some(MenuAction::ClearSymbols);
                            ui.close();
                        }
                    }
                });

                // ----- Help -----
                ui.menu_button(ic(glyph::CIRCLE_QUESTION, "Help"), |ui| {
                    // v1.5.0 "Lens" Workstream I10 — the in-app Documentation
                    // browser (reuses the `rustynes help` topic registry so the
                    // CLI + GUI share one source). Native-only content; the menu
                    // item is gated out on wasm (no topic registry there).
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if ui
                            .button(ic(glyph::BOOK_OPEN, "Documentation..."))
                            .on_hover_text("Searchable in-app manual, About, and changelog")
                            .clicked()
                        {
                            out.action = Some(MenuAction::OpenDocumentation);
                            ui.close();
                        }
                        ui.separator();
                    }
                    if ui
                        .button(ic(glyph::KEYBOARD, "Keyboard Shortcuts"))
                        .clicked()
                    {
                        self.show_shortcuts = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(ic(glyph::CIRCLE_INFO, "About")).clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
            });
        });
    }

    /// Render the bottom status bar.
    fn status_bar(&self, root_ui: &mut egui::Ui, frame: &ShellFrame<'_>, config: &Config) {
        let style = root_ui.ctx().global_style();
        egui::Panel::bottom("shell_status_bar")
            .frame(egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::symmetric(8, 4)))
            .show_inside(root_ui, |ui| {
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
                        // v1.5.0 "Lens" Workstream I7 — the RetroAchievements
                        // readout relocated from the retired `` ` `` overlay HUD,
                        // placed between the emulator-state label and the FPS
                        // counter. A gold tint when hardcore is engaged (it ends
                        // with "HARDCORE"); the trophy colour otherwise.
                        if let Some(ra) = frame.ra_status.as_deref() {
                            ui.separator();
                            let color = if ra.ends_with("HARDCORE") {
                                egui::Color32::from_rgb(240, 200, 100)
                            } else {
                                egui::Color32::from_rgb(180, 160, 220)
                            };
                            ui.colored_label(color, ra)
                                .on_hover_text("RetroAchievements (Tools -> RetroAchievements)");
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
    #[allow(clippy::too_many_lines)] // the Video tab inlines the display + accessibility chrome.
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
        // v1.3.0 — auto-save every Settings change: snapshot the config before
        // the window renders and persist if ANY control mutated it this frame.
        // This guarantees persistence for every setting in every tab even where
        // an individual control only flags a live-apply (`state.apply.*`) without
        // its own `save_config` call. (`Config: Clone + PartialEq`.)
        let config_before = config.clone();
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(true)
            .default_width(460.0)
            .min_width(400.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(tab, SettingsTab::Video, "Video");
                    ui.selectable_value(tab, SettingsTab::Shaders, "Shaders");
                    ui.selectable_value(tab, SettingsTab::Audio, "Audio");
                    ui.selectable_value(tab, SettingsTab::Input, "Input");
                    ui.selectable_value(tab, SettingsTab::Emulation, "Emulation");
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
                                        for theme in AppTheme::all() {
                                            ui.selectable_value(
                                                &mut config.ui.theme,
                                                theme,
                                                theme.display_name(),
                                            );
                                        }
                                    })
                                    .response
                                    .on_hover_text(
                                        "High Contrast and Colorblind-Safe are accessibility \
                                         themes (WCAG AA contrast / Okabe-Ito palette).",
                                    );
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

                            // v1.5.0 accessibility — UI zoom. Scales the whole
                            // egui shell (fonts/menus/panels) via the context
                            // zoom factor; the emulated NES image is a raw blit
                            // and is unaffected. Applied live each frame by the
                            // render loop reading `config.ui.zoom_factor`.
                            ui.heading("Accessibility");
                            ui.horizontal(|ui| {
                                ui.label("UI scale:");
                                let mut pct = (config.ui.clamped_zoom_factor() * 100.0).round();
                                let resp = ui.add(
                                    egui::Slider::new(
                                        &mut pct,
                                        (UiConfig::ZOOM_MIN * 100.0)..=(UiConfig::ZOOM_MAX * 100.0),
                                    )
                                    .suffix("%")
                                    .step_by(5.0),
                                );
                                if resp.changed() {
                                    config.ui.zoom_factor =
                                        (pct / 100.0).clamp(UiConfig::ZOOM_MIN, UiConfig::ZOOM_MAX);
                                }
                                // Update the zoom live each frame, but only persist
                                // when the drag stops so a slider drag doesn't thrash
                                // the disk (mirrors the EQ/replay-panel idiom).
                                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                                    save_config(config);
                                }
                                if ui
                                    .button("Reset")
                                    .on_hover_text("Reset UI scale to 100%")
                                    .clicked()
                                {
                                    config.ui.zoom_factor = 1.0;
                                    save_config(config);
                                }
                            });
                            ui.label(
                                egui::RichText::new(
                                    "Scales the menus, Settings, and debugger UI. \
                                     The game image is not affected.",
                                )
                                .small()
                                .weak(),
                            );
                            ui.separator();
                            settings_body(ui, config, SettingsTab::Video);
                        }
                        // v1.0.0 settings split — each tab renders ONLY its own
                        // section (the prior catch-all rendered the whole body on
                        // every tab, duplicating every control).
                        SettingsTab::Shaders => settings_body(ui, config, SettingsTab::Shaders),
                        SettingsTab::Audio => settings_body(ui, config, SettingsTab::Audio),
                        SettingsTab::Emulation => settings_body(ui, config, SettingsTab::Emulation),
                        SettingsTab::Input => input_body(ui, config),
                    });
            });
        // v1.3.0 auto-save — persist once if any tab mutated the config. The
        // per-control `save_config` calls in the panel sections remain (harmless
        // double-save) but this is the backstop that makes EVERY setting sticky.
        // Skip while a pointer button is held so an in-progress slider drag
        // doesn't write the config to disk every frame; the release-edge save
        // (and this backstop on the next idle frame) still persists the change.
        if *config != config_before && !ctx.input(|i| i.pointer.any_down()) {
            save_config(config);
        }
        // v1.5.0 accessibility — Esc dismisses the modal for keyboard-only users.
        esc_closes(ctx, &mut open);
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

/// v1.5.0 accessibility — keyboard-only modal dismissal.
///
/// egui's `Window::open(&mut bool)` gives a mouse-clickable close `X` but does
/// not close on the `Esc` key, so a keyboard-only user could open Settings /
/// About / Shortcuts and have no key to dismiss them (the app's `Esc`/Quit
/// binding is suppressed while a shell window is open, so it can't quit out
/// either). This consumes a pressed `Esc` and clears `open`, giving every modal
/// a consistent keyboard escape hatch. Returns `true` when it closed the window.
fn esc_closes(ctx: &egui::Context, open: &mut bool) -> bool {
    if *open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
        true
    } else {
        false
    }
}

/// Apply the configured [`AppTheme`] to the egui context.
///
/// Called as the first statement of the shell egui closure each frame (guarded
/// by a change check) so the chrome (and the debugger panels) all render in the
/// chosen theme.
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
        AppTheme::HighContrast => ctx.set_visuals(high_contrast_visuals()),
        AppTheme::Colorblind => ctx.set_visuals(colorblind_visuals()),
    }
}

/// A high-contrast dark [`egui::Visuals`] for low-vision accessibility.
///
/// Starts from the stock dark theme and pushes every foreground/background pair
/// to the extremes: a near-black window/panel background, pure-white body text
/// and a bright cyan selection accent, with thicker, fully-opaque widget
/// strokes so focus and boundaries read clearly. The text-vs-background ratios
/// clear WCAG 2.1 AA (4.5:1) — most clear AAA (7:1) — for normal-size text.
fn high_contrast_visuals() -> egui::Visuals {
    use egui::Color32;
    let mut v = egui::Visuals::dark();
    let black = Color32::from_rgb(8, 8, 8);
    let white = Color32::from_rgb(250, 250, 250);
    // Selection / hyperlink accent: bright cyan reads strongly on near-black
    // and is distinguishable across all common color-vision deficiencies.
    let accent = Color32::from_rgb(0, 224, 255);

    v.dark_mode = true;
    v.override_text_color = Some(white);
    v.panel_fill = black;
    v.window_fill = black;
    v.extreme_bg_color = Color32::BLACK;
    v.faint_bg_color = Color32::from_rgb(28, 28, 28);
    v.window_stroke = egui::Stroke::new(1.5, white);
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent.gamma_multiply(0.55);
    v.selection.stroke = egui::Stroke::new(1.5, accent);

    // Widget states: opaque fills + bold white strokes for visible boundaries.
    let stroke = |w: f32| egui::Stroke::new(w, white);
    v.widgets.noninteractive.bg_fill = black;
    v.widgets.noninteractive.weak_bg_fill = black;
    v.widgets.noninteractive.fg_stroke = stroke(1.0);
    v.widgets.inactive.bg_fill = Color32::from_rgb(40, 40, 40);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 40, 40);
    v.widgets.inactive.fg_stroke = stroke(1.5);
    v.widgets.inactive.bg_stroke = stroke(1.0);
    v.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 70);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(70, 70, 70);
    v.widgets.hovered.fg_stroke = stroke(2.0);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(2.0, accent);
    v.widgets.active.bg_fill = Color32::from_rgb(90, 90, 90);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(90, 90, 90);
    v.widgets.active.fg_stroke = stroke(2.0);
    v.widgets.active.bg_stroke = egui::Stroke::new(2.0, accent);
    v
}

/// A colorblind-safe dark [`egui::Visuals`] (deuteranopia/protanopia-friendly).
///
/// Built on the stock dark theme but with interactive accents drawn from the
/// Okabe-Ito palette — a set chosen to stay mutually distinguishable for the
/// most common (red-green) forms of color vision deficiency. Selection/active
/// uses Okabe-Ito blue, hover uses Okabe-Ito orange, and hyperlinks use sky
/// blue, so the "where is focus / what is selected" cues never collapse to an
/// ambiguous red-green pair.
fn colorblind_visuals() -> egui::Visuals {
    use egui::Color32;
    let mut v = egui::Visuals::dark();
    // Okabe-Ito palette (https://jfly.uni-koeln.de/color/).
    let blue = Color32::from_rgb(0, 114, 178); // selection / active
    let sky_blue = Color32::from_rgb(86, 180, 233); // hyperlinks / active stroke
    let orange = Color32::from_rgb(230, 159, 0); // hover

    v.hyperlink_color = sky_blue;
    v.selection.bg_fill = blue.gamma_multiply(0.55);
    v.selection.stroke = egui::Stroke::new(1.0, sky_blue);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, orange);
    v.widgets.active.bg_fill = blue.gamma_multiply(0.7);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.5, sky_blue);
    v
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
/// The About-dialog app icon as a cached egui texture (native only).
///
/// Loaded once on first display and kept alive in a `thread_local` so it isn't
/// re-uploaded each frame. `None` on wasm or decode failure.
#[cfg(not(target_arch = "wasm32"))]
fn about_icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    thread_local! {
        static ICON: std::cell::RefCell<Option<egui::TextureHandle>> =
            const { std::cell::RefCell::new(None) };
    }
    ICON.with(|cell| {
        let mut held = cell.borrow_mut();
        if held.is_none() {
            let img = crate::icon::about_color_image()?;
            *held =
                Some(ctx.load_texture("rustynes_about_icon", img, egui::TextureOptions::LINEAR));
        }
        held.clone()
    })
}

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
                ui.add_space(6.0);
                // The app icon (native; the rounded corners stay transparent).
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(tex) = about_icon_texture(ctx) {
                    let resp = ui.add(
                        egui::Image::new((tex.id(), egui::vec2(96.0, 96.0)))
                            .sense(egui::Sense::click()),
                    );
                    // Lower-right region of the emblem is an interaction target.
                    if resp.clicked()
                        && let Some(p) = resp.interact_pointer_pos()
                    {
                        let r = resp.rect;
                        if p.x >= r.center().x && p.y >= r.center().y {
                            crate::about_fx::tap();
                        }
                    }
                    ui.add_space(8.0);
                }
                ui.heading("RustyNES");
                ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
                ui.add_space(2.0);
                ui.label(egui::RichText::new("Precise. Pure. Powerful.").italics());
                ui.add_space(10.0);
                ui.label("A cycle-accurate NES emulator written in pure Rust.");
                ui.add_space(10.0);
                ui.label("Created by DoubleGate");
                ui.hyperlink_to(
                    "github.com/doublegate/RustyNES",
                    "https://github.com/doublegate/RustyNES",
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new("MIT OR Apache-2.0").weak());
                ui.add_space(4.0);
            });
        });
    // v1.5.0 accessibility — Esc dismisses the modal for keyboard-only users.
    esc_closes(ctx, open);
    #[cfg(not(target_arch = "wasm32"))]
    crate::about_fx::pump(ctx);
}

/// Render the keyboard-shortcuts window.
impl UiShell {
    /// v1.5.0 "Lens" Workstream I9 — render the Keyboard Shortcuts window from
    /// the LIVE `[input]` / `[input.system]` config (not the hardcoded defaults),
    /// with the emulator hotkeys above a separator and a per-device controller
    /// section below selected by [`Self::shortcuts_device`].
    fn shortcuts_window(&mut self, ctx: &egui::Context, config: &Config) {
        if !self.show_shortcuts {
            return;
        }
        let mut open = self.show_shortcuts;
        let device = &mut self.shortcuts_device;
        egui::Window::new("Keyboard Shortcuts")
            .open(&mut open)
            .resizable(true)
            .collapsible(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                // --- Emulator / system hotkeys (live bindings) ---
                ui.label(egui::RichText::new("Emulator hotkeys").strong());
                system_hotkeys_grid(ui, config);

                // --- Separator between emulator hotkeys and controller mapping ---
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                // --- Controller / device mapping (per-device selector) ---
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Device:").strong());
                    egui::ComboBox::from_id_salt("shortcuts-device")
                        .selected_text(device.label())
                        .show_ui(ui, |ui| {
                            for d in [
                                ShortcutsDevice::Player1,
                                ShortcutsDevice::Player2,
                                ShortcutsDevice::Player3,
                                ShortcutsDevice::Player4,
                                ShortcutsDevice::PowerPad,
                                ShortcutsDevice::FamilyKeyboard,
                            ] {
                                ui.selectable_value(device, d, d.label());
                            }
                        });
                });
                ui.add_space(4.0);
                device_bindings_grid(ui, config, *device);
            });
        // v1.5.0 accessibility — Esc dismisses the modal for keyboard-only users.
        esc_closes(ctx, &mut open);
        self.show_shortcuts = open;
    }
}

/// v1.5.0 I9 — the live emulator/system hotkey grid (from `[input.system]`).
fn system_hotkeys_grid(ui: &mut egui::Ui, config: &Config) {
    let s = &config.input.system;
    egui::Grid::new("shortcuts_system")
        .num_columns(2)
        .spacing([32.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Action").strong());
            ui.label(egui::RichText::new("Key").strong());
            ui.end_row();
            for (action, key) in [
                ("Open ROM", s.open_rom.as_str()),
                ("Save state", s.save_state.as_str()),
                ("Load state", s.load_state.as_str()),
                ("Rewind (hold)", s.rewind.as_str()),
                ("Reset", s.reset.as_str()),
                ("Power cycle", s.power_cycle.as_str()),
                ("Pause / resume", s.pause.as_str()),
                ("Frame advance", s.frame_advance.as_str()),
                ("Fast forward (hold)", s.fast_forward.as_str()),
                ("Movie record", s.movie_record.as_str()),
                ("Movie play", s.movie_play.as_str()),
                ("Movie branch", s.movie_branch.as_str()),
                ("Swap disk side (FDS)", s.disk_swap.as_str()),
                ("Insert coin (Vs.)", s.insert_coin.as_str()),
                ("Fullscreen", s.fullscreen.as_str()),
                ("Toggle menu bar", s.toggle_menu_bar.as_str()),
                ("Toggle debugger", s.debug_overlay.as_str()),
                ("Quit / exit fullscreen", s.quit.as_str()),
            ] {
                ui.label(action);
                ui.label(pretty_key(key));
                ui.end_row();
            }
        });
}

/// v1.5.0 I9 — the live per-device controller binding grid. Standard pads read
/// the `[input]` `PadBindings`; the Power Pad + Family BASIC keyboard use the
/// fixed default mapping documented in `input.rs` (they are not rebindable yet,
/// so this reflects their actual host keys).
fn device_bindings_grid(ui: &mut egui::Ui, config: &Config, device: ShortcutsDevice) {
    ui.label(egui::RichText::new(device.label()).strong());
    let pad = match device {
        ShortcutsDevice::Player1 => Some(&config.input.player1),
        ShortcutsDevice::Player2 => Some(&config.input.player2),
        ShortcutsDevice::Player3 => Some(&config.input.player3),
        ShortcutsDevice::Player4 => Some(&config.input.player4),
        ShortcutsDevice::PowerPad | ShortcutsDevice::FamilyKeyboard => None,
    };
    if let Some(p) = pad {
        egui::Grid::new("shortcuts_pad")
            .num_columns(2)
            .spacing([32.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Button").strong());
                ui.label(egui::RichText::new("Key").strong());
                ui.end_row();
                for (b, key) in [
                    ("Up", p.up.as_str()),
                    ("Down", p.down.as_str()),
                    ("Left", p.left.as_str()),
                    ("Right", p.right.as_str()),
                    ("A", p.a.as_str()),
                    ("B", p.b.as_str()),
                    ("Select", p.select.as_str()),
                    ("Start", p.start.as_str()),
                ] {
                    ui.label(b);
                    ui.label(pretty_key(key));
                    ui.end_row();
                }
            });
        return;
    }
    // Non-rebindable devices: show the fixed default host-key layout.
    let rows: &[(&str, &str)] = match device {
        ShortcutsDevice::PowerPad => &[
            ("Top row (1-4)", "1  2  3  4"),
            ("Middle row (5-8)", "Q  W  E  R"),
            ("Bottom row (9-12)", "A  S  D  F"),
            ("Note", "12-button mat; fixed default keys"),
        ],
        ShortcutsDevice::FamilyKeyboard => &[
            ("Layout", "Host keyboard maps to the matrix"),
            ("Letters / digits", "as labelled on your keyboard"),
            (
                "Note",
                "Active only with the Family BASIC / Subor keyboard device",
            ),
        ],
        _ => &[],
    };
    egui::Grid::new("shortcuts_fixed")
        .num_columns(2)
        .spacing([32.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            for (a, k) in rows {
                ui.label(*a);
                ui.label(*k);
                ui.end_row();
            }
        });
}

/// v1.5.0 I9 — humanize a winit `KeyCode` config token for display (e.g.
/// `ArrowUp` -> `Up`, `ShiftRight` -> `Right Shift`, `KeyZ` -> `Z`,
/// `Backslash` -> `\\`). Falls back to the raw token for anything unmapped.
fn pretty_key(code: &str) -> String {
    match code {
        "ArrowUp" => "Up".into(),
        "ArrowDown" => "Down".into(),
        "ArrowLeft" => "Left".into(),
        "ArrowRight" => "Right".into(),
        "ShiftRight" => "Right Shift".into(),
        "ShiftLeft" => "Left Shift".into(),
        "Backslash" => "\\".into(),
        "Backquote" => "`".into(),
        "Equal" => "=".into(),
        "Minus" => "-".into(),
        "Space" => "Space".into(),
        "Enter" => "Enter".into(),
        "Tab" => "Tab".into(),
        "Escape" => "Esc".into(),
        other => other
            .strip_prefix("Key")
            .or_else(|| other.strip_prefix("Digit"))
            .map_or_else(|| other.to_string(), ToString::to_string),
    }
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
