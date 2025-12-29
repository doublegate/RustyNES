//! GUI module for egui-based menus and debug windows.
//!
//! This module provides:
//! - Main menu bar with File, Emulation, Options, Debug menus
//! - Debug windows for CPU, PPU, APU, and memory
//! - Settings dialogs
//! - Modal dialogs (welcome, error, confirm)
//! - Status bar with visual feedback

pub mod debug;
pub mod menu;
pub mod settings;

use crate::audio::AudioOutput;
use crate::config::Config;
use egui::Context;
use rustynes_core::Console;
use std::time::{Duration, Instant};

/// Status message with color and auto-fade.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    /// Message text.
    pub text: String,
    /// Message color.
    pub color: egui::Color32,
    /// When the message was created.
    pub created_at: Instant,
    /// How long to display the message.
    pub duration: Duration,
}

impl StatusMessage {
    /// Create a new status message.
    pub fn new(text: impl Into<String>, color: egui::Color32, duration: Duration) -> Self {
        Self {
            text: text.into(),
            color,
            created_at: Instant::now(),
            duration,
        }
    }

    /// Create an info message (white text, 3 seconds).
    pub fn info(text: impl Into<String>) -> Self {
        Self::new(text, egui::Color32::WHITE, Duration::from_secs(3))
    }

    /// Create a success message (green text, 3 seconds).
    pub fn success(text: impl Into<String>) -> Self {
        Self::new(
            text,
            egui::Color32::from_rgb(100, 200, 100),
            Duration::from_secs(3),
        )
    }

    /// Create an error message (red text, 5 seconds).
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(
            text,
            egui::Color32::from_rgb(255, 100, 100),
            Duration::from_secs(5),
        )
    }

    /// Check if the message has expired.
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    /// Get the alpha value for fade effect (1.0 -> 0.0).
    pub fn alpha(&self) -> f32 {
        let elapsed = self.created_at.elapsed().as_secs_f32();
        let total = self.duration.as_secs_f32();
        // Start fading in the last second
        if elapsed < total - 1.0 {
            1.0
        } else {
            1.0 - ((elapsed - (total - 1.0)) / 1.0).min(1.0)
        }
    }
}

/// Settings tab selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    /// Video settings tab.
    #[default]
    Video,
    /// Audio settings tab.
    Audio,
    /// Input settings tab.
    Input,
    /// Advanced/Debug settings tab.
    Advanced,
}

/// Confirmation action for modal dialogs.
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    /// Reset the console.
    Reset,
    /// Close without saving.
    CloseWithoutSave,
    /// Clear recent ROMs list.
    ClearRecentRoms,
}

/// GUI state for tracking window visibility and menu state.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct GuiState {
    /// Whether the menu bar is visible.
    pub menu_visible: bool,
    /// Whether the settings window is open.
    pub settings_open: bool,
    /// Whether the about window is open.
    pub about_open: bool,
    /// Whether a file dialog is pending.
    pub file_dialog_pending: bool,
    /// Debug window states.
    pub debug: DebugWindows,
    /// Current FPS display.
    pub fps: f32,
    /// Frame counter for FPS calculation.
    frame_count: u32,
    /// Last FPS update time.
    last_fps_update: Instant,

    // -- New M10-S1 fields --
    /// Current settings tab.
    pub settings_tab: SettingsTab,
    /// Status message to display in status bar.
    pub status_message: Option<StatusMessage>,
    /// Error message for modal dialog.
    pub error_message: Option<String>,
    /// Whether to show the welcome modal.
    pub show_welcome: bool,
    /// Whether to show keyboard shortcuts window.
    pub show_shortcuts: bool,
    /// Pending confirmation action.
    pub confirm_action: Option<ConfirmAction>,
    /// Whether the app is currently loading (shows spinner).
    pub loading: bool,
    /// Currently loaded ROM name (for status bar).
    pub rom_name: Option<String>,
}

/// Debug window visibility states.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct DebugWindows {
    /// CPU debug window.
    pub cpu: bool,
    /// PPU debug window.
    pub ppu: bool,
    /// APU debug window.
    pub apu: bool,
    /// Memory viewer window.
    pub memory: bool,
}

impl GuiState {
    /// Create new GUI state from configuration.
    pub fn new(config: &Config) -> Self {
        Self {
            menu_visible: true,
            settings_open: false,
            about_open: false,
            file_dialog_pending: false,
            debug: DebugWindows {
                cpu: config.debug.show_cpu,
                ppu: config.debug.show_ppu,
                apu: config.debug.show_apu,
                memory: config.debug.show_memory,
            },
            fps: 0.0,
            frame_count: 0,
            last_fps_update: Instant::now(),
            // New M10-S1 fields
            settings_tab: SettingsTab::Video,
            status_message: None,
            error_message: None,
            show_welcome: config.first_run,
            show_shortcuts: false,
            confirm_action: None,
            loading: false,
            rom_name: None,
        }
    }

    /// Set a status message.
    pub fn set_status(&mut self, message: StatusMessage) {
        self.status_message = Some(message);
    }

    /// Set an error message to show in a modal.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error_message = Some(message.into());
    }

    /// Clear expired status messages.
    pub fn clear_expired_status(&mut self) {
        if let Some(ref msg) = self.status_message
            && msg.is_expired()
        {
            self.status_message = None;
        }
    }

    /// Toggle menu visibility.
    pub fn toggle_menu(&mut self) {
        self.menu_visible = !self.menu_visible;
    }

    /// Update FPS counter.
    #[allow(clippy::cast_precision_loss)]
    pub fn update_fps(&mut self) {
        self.frame_count += 1;
        let elapsed = self.last_fps_update.elapsed();
        if elapsed.as_secs_f32() >= 1.0 {
            self.fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.frame_count = 0;
            self.last_fps_update = std::time::Instant::now();
        }
    }
}

/// Render the complete GUI.
pub fn render(
    ctx: &Context,
    state: &mut GuiState,
    config: &mut Config,
    console: &mut Option<Console>,
    audio: &Option<AudioOutput>,
    paused: &mut bool,
) {
    // Update FPS and clear expired status
    state.update_fps();
    state.clear_expired_status();

    // Render menu bar
    if state.menu_visible {
        menu::render_menu_bar(ctx, state, config, console, audio, paused);
    }

    // Render status bar at bottom
    render_status_bar(ctx, state, console.as_ref(), *paused);

    // Render debug windows if debug mode is enabled
    if config.debug.enabled {
        if state.debug.cpu {
            debug::cpu::render(ctx, &mut state.debug.cpu, console);
        }
        if state.debug.ppu {
            debug::ppu::render(ctx, &mut state.debug.ppu, console);
        }
        if state.debug.apu {
            debug::apu::render(ctx, &mut state.debug.apu, console);
        }
        if state.debug.memory {
            debug::memory::render(ctx, &mut state.debug.memory, console);
        }
    }

    // Render settings window
    if state.settings_open {
        settings::render_settings(ctx, state, config, audio.as_ref());
    }

    // Render about window
    if state.about_open {
        render_about_window(ctx, &mut state.about_open);
    }

    // Render keyboard shortcuts window
    if state.show_shortcuts {
        render_shortcuts_window(ctx, &mut state.show_shortcuts);
    }

    // Render modal dialogs
    render_welcome_modal(ctx, state, config);
    render_error_modal(ctx, state);
    render_confirm_modal(ctx, state, config, console);

    // Show FPS overlay if enabled
    if config.video.show_fps {
        render_fps_overlay(ctx, state.fps);
    }

    // Show pause indicator
    if *paused {
        render_pause_indicator(ctx);
    }

    // Show loading overlay if loading
    if state.loading {
        render_loading_overlay(ctx);
    }
}

/// Apply theme to egui context.
pub fn apply_theme(ctx: &Context, config: &Config) {
    use crate::config::AppTheme;
    match config.video.theme {
        AppTheme::Light => ctx.set_visuals(egui::Visuals::light()),
        AppTheme::Dark => ctx.set_visuals(egui::Visuals::dark()),
        AppTheme::System => {
            // egui/eframe automatically detects system theme when using default
            // For now, we'll default to dark
            ctx.set_visuals(egui::Visuals::dark());
        }
    }
}

/// Render the status bar at the bottom of the window.
fn render_status_bar(ctx: &Context, state: &GuiState, console: Option<&Console>, paused: bool) {
    egui::TopBottomPanel::bottom("status_bar")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 4)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Left side: ROM name and status
                if let Some(ref rom_name) = state.rom_name {
                    ui.label(format!("ROM: {rom_name}"));
                    ui.separator();
                } else {
                    ui.label("No ROM loaded");
                    ui.separator();
                }

                // Emulation status
                if console.is_some() {
                    if paused {
                        ui.colored_label(egui::Color32::YELLOW, "Paused");
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(100, 200, 100), "Running");
                    }
                } else {
                    ui.colored_label(egui::Color32::GRAY, "Idle");
                }

                // Status message (with fade)
                if let Some(ref msg) = state.status_message {
                    ui.separator();
                    let alpha = msg.alpha();
                    let color = egui::Color32::from_rgba_unmultiplied(
                        msg.color.r(),
                        msg.color.g(),
                        msg.color.b(),
                        (alpha * 255.0) as u8,
                    );
                    ui.colored_label(color, &msg.text);
                }

                // Right side: FPS
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("FPS: {:.1}", state.fps));
                });
            });
        });
}

/// Render the welcome modal for first-run experience.
fn render_welcome_modal(ctx: &Context, state: &mut GuiState, config: &mut Config) {
    if !state.show_welcome {
        return;
    }

    egui::Window::new("Welcome to RustyNES!")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.add_space(8.0);
            ui.label("A high-accuracy NES emulator written in Rust.");
            ui.add_space(16.0);

            ui.label(egui::RichText::new("Quick Start:").strong());
            ui.add_space(4.0);
            egui::Grid::new("shortcuts_grid")
                .num_columns(2)
                .spacing([20.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Open ROM:");
                    ui.label("Ctrl+O or drag & drop");
                    ui.end_row();
                    ui.label("Pause/Resume:");
                    ui.label("Ctrl+P or F3");
                    ui.end_row();
                    ui.label("Reset:");
                    ui.label("Ctrl+R or F2");
                    ui.end_row();
                    ui.label("Controls:");
                    ui.label("Arrow keys + Z/X");
                    ui.end_row();
                });

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button("Get Started").clicked() {
                    state.show_welcome = false;
                    config.first_run = false;
                    if let Err(e) = config.save() {
                        log::error!("Failed to save config: {e}");
                    }
                }
                if ui.button("Show Keyboard Shortcuts").clicked() {
                    state.show_shortcuts = true;
                }
            });
        });
}

/// Render error modal dialog.
fn render_error_modal(ctx: &Context, state: &mut GuiState) {
    if state.error_message.is_none() {
        return;
    }

    let error_msg = state.error_message.clone().unwrap_or_default();

    egui::Window::new("Error")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(350.0);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("!").color(egui::Color32::RED).heading());
                ui.add_space(8.0);
                ui.label(&error_msg);
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        state.error_message = None;
                    }
                });
            });
        });
}

/// Render confirmation modal dialog.
fn render_confirm_modal(
    ctx: &Context,
    state: &mut GuiState,
    config: &mut Config,
    console: &mut Option<Console>,
) {
    if state.confirm_action.is_none() {
        return;
    }

    let action_text = match state.confirm_action {
        Some(ConfirmAction::Reset) => "reset the console",
        Some(ConfirmAction::CloseWithoutSave) => "close without saving",
        Some(ConfirmAction::ClearRecentRoms) => "clear the recent ROMs list",
        None => return,
    };

    egui::Window::new("Confirm Action")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(300.0);
            ui.add_space(8.0);
            ui.label(format!("Are you sure you want to {action_text}?"));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    // Execute the action
                    match state.confirm_action.take() {
                        Some(ConfirmAction::Reset) => {
                            if let Some(cons) = console {
                                cons.reset();
                                state.set_status(StatusMessage::info("Console reset"));
                            }
                        }
                        Some(ConfirmAction::ClearRecentRoms) => {
                            config.recent_roms.paths.clear();
                            state.set_status(StatusMessage::info("Recent ROMs cleared"));
                        }
                        Some(ConfirmAction::CloseWithoutSave) | None => {}
                    }
                }
                if ui.button("Cancel").clicked() {
                    state.confirm_action = None;
                }
            });
        });
}

/// Render the keyboard shortcuts window.
fn render_shortcuts_window(ctx: &Context, open: &mut bool) {
    egui::Window::new("Keyboard Shortcuts")
        .open(open)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            egui::Grid::new("shortcuts_grid_full")
                .num_columns(2)
                .spacing([40.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Action").strong());
                    ui.label(egui::RichText::new("Shortcut").strong());
                    ui.end_row();

                    ui.label("Open ROM");
                    ui.label("Ctrl+O");
                    ui.end_row();

                    ui.label("Pause/Resume");
                    ui.label("Ctrl+P / F3");
                    ui.end_row();

                    ui.label("Reset");
                    ui.label("Ctrl+R / F2");
                    ui.end_row();

                    ui.label("Toggle Mute");
                    ui.label("M");
                    ui.end_row();

                    ui.label("Toggle Debug Mode");
                    ui.label("F1");
                    ui.end_row();

                    ui.label("Settings");
                    ui.label("Ctrl+,");
                    ui.end_row();

                    ui.label("Quit");
                    ui.label("Ctrl+Q");
                    ui.end_row();

                    ui.end_row();
                    ui.label(egui::RichText::new("NES Controls").strong());
                    ui.label("");
                    ui.end_row();

                    ui.label("D-Pad");
                    ui.label("Arrow Keys");
                    ui.end_row();

                    ui.label("A Button");
                    ui.label("X");
                    ui.end_row();

                    ui.label("B Button");
                    ui.label("Z");
                    ui.end_row();

                    ui.label("Start");
                    ui.label("Enter");
                    ui.end_row();

                    ui.label("Select");
                    ui.label("Backspace");
                    ui.end_row();
                });
        });
}

/// Render loading overlay with spinner.
fn render_loading_overlay(ctx: &Context) {
    // Semi-transparent overlay
    let screen_rect = ctx.available_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("loading_overlay"),
    ));
    painter.rect_filled(
        screen_rect,
        0.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
    );

    // Centered loading content
    egui::Area::new(egui::Id::new("loading_content"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(50.0));
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("Loading ROM...")
                        .size(18.0)
                        .color(egui::Color32::WHITE),
                );
            });
        });
}

/// Render the FPS overlay.
fn render_fps_overlay(ctx: &Context, fps: f32) {
    egui::Area::new(egui::Id::new("fps_overlay"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-10.0, 10.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("FPS: {fps:.1}"))
                    .color(egui::Color32::YELLOW)
                    .background_color(egui::Color32::from_black_alpha(128)),
            );
        });
}

/// Render the pause indicator.
fn render_pause_indicator(ctx: &Context) {
    egui::Area::new(egui::Id::new("pause_indicator"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("PAUSED")
                    .heading()
                    .color(egui::Color32::WHITE)
                    .background_color(egui::Color32::from_black_alpha(192)),
            );
        });
}

/// Render the about window.
fn render_about_window(ctx: &Context, open: &mut bool) {
    egui::Window::new("About RustyNES")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("RustyNES");
                ui.label("Version 0.8.6");
                ui.add_space(10.0);
                ui.label("A cycle-accurate NES emulator written in Rust");
                ui.add_space(10.0);
                ui.hyperlink_to("GitHub", "https://github.com/doublegate/RustyNES");
                ui.add_space(10.0);
                ui.label("MIT / Apache-2.0 License");
            });
        });
}
