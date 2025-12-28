//! Main application implementing `eframe::App`.
//!
//! This module provides the core application structure that:
//! - Implements the `eframe::App` trait for the main loop
//! - Coordinates emulator, audio, and input
//! - Renders the NES framebuffer as an egui texture
//! - Handles keyboard shortcuts

use crate::audio::AudioOutput;
use crate::config::Config;
use crate::gui::{self, StatusMessage};
use crate::input::InputHandler;

use egui::{ColorImage, TextureHandle, TextureOptions};
use log::{error, info};
use rustynes_core::Console;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// NES display width in pixels.
pub const NES_WIDTH: usize = 256;
/// NES display height in pixels.
pub const NES_HEIGHT: usize = 240;

/// Target frame rate (NTSC).
const TARGET_FPS: f64 = 60.0988;
/// Frame duration in nanoseconds.
const FRAME_DURATION: Duration = Duration::from_nanos((1_000_000_000.0 / TARGET_FPS) as u64);

/// Main NES emulator application.
pub struct NesApp {
    /// Configuration.
    config: Config,
    /// NES console.
    console: Option<Console>,
    /// Audio output.
    audio: Option<AudioOutput>,
    /// Input handler.
    input: InputHandler,
    /// GUI state.
    gui_state: gui::GuiState,
    /// Whether the emulator is paused.
    paused: bool,
    /// Last frame time.
    last_frame: Instant,
    /// Accumulated time for frame timing.
    accumulator: Duration,
    /// NES framebuffer texture handle.
    nes_texture: Option<TextureHandle>,
    /// Framebuffer pixel data for the texture.
    framebuffer: Vec<u8>,
    /// Last applied theme (for detecting changes).
    last_theme: crate::config::AppTheme,
}

impl NesApp {
    /// Create a new NES application.
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        rom_path: Option<PathBuf>,
    ) -> Self {
        // Apply theme based on config
        let ctx = &cc.egui_ctx;
        gui::apply_theme(ctx, &config);

        // Create audio output
        let audio = match AudioOutput::new(
            config.audio.sample_rate,
            config.audio.volume,
            config.audio.muted,
        ) {
            Ok(audio) => Some(audio),
            Err(e) => {
                error!("Failed to initialize audio: {e}");
                None
            }
        };

        // Create input handler
        let input = InputHandler::new(
            &config.input.player1_keyboard,
            &config.input.player2_keyboard,
        );

        // Create GUI state
        let mut gui_state = gui::GuiState::new(&config);

        // Load console if ROM was provided
        let console = if let Some(rom_path) = &rom_path {
            match Self::load_rom(rom_path) {
                Ok(console) => {
                    info!("Loaded ROM: {}", rom_path.display());
                    // Set ROM name in GUI state
                    gui_state.rom_name = rom_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(String::from);
                    gui_state.set_status(StatusMessage::success("ROM loaded"));
                    Some(console)
                }
                Err(e) => {
                    error!("Failed to load ROM: {e}");
                    gui_state.set_error(format!("Failed to load ROM: {e}"));
                    None
                }
            }
        } else {
            None
        };

        // Initialize framebuffer (RGBA, 256x240)
        let framebuffer = vec![0u8; NES_WIDTH * NES_HEIGHT * 4];

        // Store initial theme
        let last_theme = config.video.theme;

        Self {
            config,
            console,
            audio,
            input,
            gui_state,
            paused: false,
            last_frame: Instant::now(),
            accumulator: Duration::ZERO,
            nes_texture: None,
            framebuffer,
            last_theme,
        }
    }

    /// Load a ROM file into a new console instance.
    fn load_rom(path: &PathBuf) -> anyhow::Result<Console> {
        let rom_data = std::fs::read(path)?;
        Console::from_rom_bytes(&rom_data).map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Update the NES texture from the console framebuffer.
    fn update_texture(&mut self, ctx: &egui::Context) {
        // Get pixel data from console or use placeholder
        if let Some(console) = &self.console {
            let fb = console.framebuffer();
            let len = self.framebuffer.len().min(fb.len());
            self.framebuffer[..len].copy_from_slice(&fb[..len]);
        } else {
            // Dark blue placeholder when no ROM is loaded
            for pixel in self.framebuffer.chunks_exact_mut(4) {
                pixel[0] = 32;
                pixel[1] = 32;
                pixel[2] = 64;
                pixel[3] = 255;
            }
        }

        // Create or update the texture
        let image = ColorImage::from_rgba_unmultiplied([NES_WIDTH, NES_HEIGHT], &self.framebuffer);

        if let Some(texture) = &mut self.nes_texture {
            texture.set(image, TextureOptions::NEAREST);
        } else {
            self.nes_texture =
                Some(ctx.load_texture("nes_framebuffer", image, TextureOptions::NEAREST));
        }
    }

    /// Run emulation for one frame.
    fn run_frame(&mut self) {
        if let Some(console) = &mut self.console {
            // Update controller input
            console.set_controller_1(self.input.player1_buttons());
            console.set_controller_2(self.input.player2_buttons());

            // Run one frame
            console.step_frame();

            // Get audio samples and queue them
            if let Some(audio) = &mut self.audio {
                let samples = console.audio_samples();
                if !samples.is_empty() {
                    audio.queue_samples(samples);
                }
                console.clear_audio_samples();
            }
        }
    }

    /// Handle keyboard input for special keys and shortcuts.
    fn handle_special_keys(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            let ctrl = i.modifiers.ctrl;

            // Ctrl+O: Open ROM
            if ctrl && i.key_pressed(egui::Key::O) {
                self.open_file_dialog();
            }

            // Ctrl+P: Toggle Pause
            if ctrl && i.key_pressed(egui::Key::P) && self.console.is_some() {
                self.paused = !self.paused;
                self.gui_state
                    .set_status(StatusMessage::info(if self.paused {
                        "Emulation paused"
                    } else {
                        "Emulation resumed"
                    }));
                info!("Console {}", if self.paused { "paused" } else { "resumed" });
            }

            // Ctrl+R: Reset
            if ctrl
                && i.key_pressed(egui::Key::R)
                && let Some(console) = &mut self.console
            {
                console.reset();
                self.gui_state
                    .set_status(StatusMessage::info("Console reset"));
                info!("Console reset");
            }

            // Ctrl+Q: Quit
            if ctrl && i.key_pressed(egui::Key::Q) {
                std::process::exit(0);
            }

            // Ctrl+,: Settings (comma key)
            if ctrl && i.key_pressed(egui::Key::Comma) {
                self.gui_state.settings_open = true;
            }

            // F3: Toggle pause (legacy)
            if i.key_pressed(egui::Key::F3) && self.console.is_some() {
                self.paused = !self.paused;
                self.gui_state
                    .set_status(StatusMessage::info(if self.paused {
                        "Emulation paused"
                    } else {
                        "Emulation resumed"
                    }));
                info!("Console {}", if self.paused { "paused" } else { "resumed" });
            }

            // F2: Reset (legacy)
            if i.key_pressed(egui::Key::F2)
                && let Some(console) = &mut self.console
            {
                console.reset();
                self.gui_state
                    .set_status(StatusMessage::info("Console reset"));
                info!("Console reset");
            }

            // F1: Toggle debug mode
            if i.key_pressed(egui::Key::F1) {
                self.config.debug.enabled = !self.config.debug.enabled;
                self.gui_state
                    .set_status(StatusMessage::info(if self.config.debug.enabled {
                        "Debug mode enabled"
                    } else {
                        "Debug mode disabled"
                    }));
            }

            // Escape: Toggle menu or close dialogs
            if i.key_pressed(egui::Key::Escape) {
                if self.gui_state.show_welcome {
                    self.gui_state.show_welcome = false;
                    self.config.first_run = false;
                } else if self.gui_state.error_message.is_some() {
                    self.gui_state.error_message = None;
                } else if self.gui_state.confirm_action.is_some() {
                    self.gui_state.confirm_action = None;
                } else if self.gui_state.settings_open {
                    self.gui_state.settings_open = false;
                } else if self.gui_state.show_shortcuts {
                    self.gui_state.show_shortcuts = false;
                } else {
                    self.gui_state.toggle_menu();
                }
            }

            // M: Toggle mute
            if i.key_pressed(egui::Key::M)
                && !i.modifiers.ctrl
                && let Some(audio) = &self.audio
            {
                audio.toggle_mute();
                let muted = audio.is_muted();
                self.gui_state.set_status(StatusMessage::info(if muted {
                    "Audio muted"
                } else {
                    "Audio unmuted"
                }));
            }
        });
    }

    /// Open file dialog to select a ROM.
    fn open_file_dialog(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter("NES ROMs", &["nes", "NES"])
            .add_filter("All Files", &["*"])
            .pick_file();

        if let Some(path) = file {
            let rom_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();

            match Self::load_rom(&path) {
                Ok(console) => {
                    self.console = Some(console);
                    self.paused = false;
                    self.config.recent_roms.add(path);
                    self.gui_state.rom_name = Some(rom_name.clone());
                    self.gui_state
                        .set_status(StatusMessage::success(format!("Loaded: {rom_name}")));
                    info!("Loaded ROM: {rom_name}");
                }
                Err(e) => {
                    self.gui_state.set_error(format!("Failed to load ROM: {e}"));
                    error!("Failed to load ROM: {e}");
                }
            }
        }
    }

    /// Handle keyboard input for NES controller.
    fn handle_controller_keys(&mut self, ctx: &egui::Context) {
        // Only process input if egui doesn't want it
        if ctx.wants_keyboard_input() {
            return;
        }

        ctx.input(|i| {
            use crate::input::NesButton;
            use egui::Key;

            // Map egui keys to controller buttons
            let key_mappings = [
                (Key::Z, NesButton::A),
                (Key::X, NesButton::B),
                (Key::Backspace, NesButton::Select),
                (Key::Enter, NesButton::Start),
                (Key::ArrowUp, NesButton::Up),
                (Key::ArrowDown, NesButton::Down),
                (Key::ArrowLeft, NesButton::Left),
                (Key::ArrowRight, NesButton::Right),
            ];

            for (key, button) in key_mappings {
                if i.key_pressed(key) {
                    self.input.set_button(1, button, true);
                }
                if i.key_released(key) {
                    self.input.set_button(1, button, false);
                }
            }
        });
    }

    /// Handle file drops.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    let rom_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();

                    info!("File dropped: {}", path.display());
                    match Self::load_rom(path) {
                        Ok(console) => {
                            self.console = Some(console);
                            self.paused = false;
                            self.config.recent_roms.add(path.clone());
                            self.gui_state.rom_name = Some(rom_name.clone());
                            self.gui_state
                                .set_status(StatusMessage::success(format!("Loaded: {rom_name}")));
                        }
                        Err(e) => {
                            self.gui_state.set_error(format!("Failed to load ROM: {e}"));
                            error!("Failed to load dropped file: {e}");
                        }
                    }
                }
            }
        });
    }
}

impl eframe::App for NesApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaint for smooth emulation
        ctx.request_repaint();

        // Apply theme changes if config changed
        if self.config.video.theme != self.last_theme {
            gui::apply_theme(ctx, &self.config);
            self.last_theme = self.config.video.theme;
        }

        // Handle input
        self.handle_special_keys(ctx);
        self.handle_controller_keys(ctx);
        self.handle_dropped_files(ctx);

        // Poll gamepads
        self.input.poll_gamepads();

        // Frame timing and emulation
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;

        if !self.paused {
            self.accumulator += delta;

            // Run emulation to catch up
            while self.accumulator >= FRAME_DURATION {
                self.accumulator -= FRAME_DURATION;
                self.run_frame();
            }
        }

        // Update the framebuffer texture
        self.update_texture(ctx);

        // Render GUI (menu bar and overlays)
        gui::render(
            ctx,
            &mut self.gui_state,
            &mut self.config,
            &mut self.console,
            &self.audio,
            &mut self.paused,
        );

        // Render the NES display in the central panel
        egui::CentralPanel::default().show(ctx, |ui| {
            // Calculate the best fit size while maintaining aspect ratio
            let available_size = ui.available_size();

            // NES aspect ratio is 256:240 (1.067), but with 8:7 pixel aspect correction it's ~1.14
            let nes_aspect = if self.config.video.pixel_aspect_correction {
                256.0 * (8.0 / 7.0) / 240.0
            } else {
                256.0 / 240.0
            };

            let (display_width, display_height) = {
                let width_from_height = available_size.y * nes_aspect;
                let height_from_width = available_size.x / nes_aspect;

                if width_from_height <= available_size.x {
                    (width_from_height, available_size.y)
                } else {
                    (available_size.x, height_from_width)
                }
            };

            // Center the display using vertical and horizontal centering
            ui.vertical_centered(|ui| {
                ui.add_space((available_size.y - display_height) / 2.0);
                if let Some(texture) = &self.nes_texture {
                    ui.image(egui::load::SizedTexture::new(
                        texture.id(),
                        egui::vec2(display_width, display_height),
                    ));
                }
            });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Save configuration on exit
        if let Err(e) = self.config.save() {
            error!("Failed to save config: {e}");
        }
        info!("RustyNES exiting");
    }
}
