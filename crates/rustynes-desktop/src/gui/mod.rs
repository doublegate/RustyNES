//! GUI module for egui-based menus and debug windows.
//!
//! This module provides:
//! - Main menu bar with File, Emulation, Options, Debug menus
//! - Debug windows for CPU, PPU, APU, and memory
//! - Settings dialogs

pub mod debug;
pub mod menu;
pub mod settings;

use crate::audio::AudioOutput;
use crate::config::Config;
use egui::Context;
use rustynes_core::Console;

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
    last_fps_update: std::time::Instant,
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
            last_fps_update: std::time::Instant::now(),
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
    // Update FPS
    state.update_fps();

    // Render menu bar
    if state.menu_visible {
        menu::render_menu_bar(ctx, state, config, console, audio, paused);
    }

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
        settings::render_settings(ctx, &mut state.settings_open, config, audio);
    }

    // Render about window
    if state.about_open {
        render_about_window(ctx, &mut state.about_open);
    }

    // Show FPS overlay if enabled
    if config.video.show_fps {
        render_fps_overlay(ctx, state.fps);
    }

    // Show pause indicator
    if *paused {
        render_pause_indicator(ctx);
    }
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
                ui.label("Version 0.7.0");
                ui.add_space(10.0);
                ui.label("A cycle-accurate NES emulator written in Rust");
                ui.add_space(10.0);
                ui.hyperlink_to("GitHub", "https://github.com/doublegate/RustyNES");
                ui.add_space(10.0);
                ui.label("MIT / Apache-2.0 License");
            });
        });
}
