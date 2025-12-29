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

/// NES System Palette - 64 RGB colors used by the PPU.
/// Each entry is (R, G, B). This is the standard 2C02 palette.
/// Source: `NESdev` Wiki standard palette
#[rustfmt::skip]
const NES_PALETTE: [(u8, u8, u8); 64] = [
    // Row 0: $00-$0F
    (0x54, 0x54, 0x54), (0x00, 0x1E, 0x74), (0x08, 0x10, 0x90), (0x30, 0x00, 0x88),
    (0x44, 0x00, 0x64), (0x5C, 0x00, 0x30), (0x54, 0x04, 0x00), (0x3C, 0x18, 0x00),
    (0x20, 0x2A, 0x00), (0x08, 0x3A, 0x00), (0x00, 0x40, 0x00), (0x00, 0x3C, 0x00),
    (0x00, 0x32, 0x3C), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
    // Row 1: $10-$1F
    (0x98, 0x96, 0x98), (0x08, 0x4C, 0xC4), (0x30, 0x32, 0xEC), (0x5C, 0x1E, 0xE4),
    (0x88, 0x14, 0xB0), (0xA0, 0x14, 0x64), (0x98, 0x22, 0x20), (0x78, 0x3C, 0x00),
    (0x54, 0x5A, 0x00), (0x28, 0x72, 0x00), (0x08, 0x7C, 0x00), (0x00, 0x76, 0x28),
    (0x00, 0x66, 0x78), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
    // Row 2: $20-$2F
    (0xEC, 0xEE, 0xEC), (0x4C, 0x9A, 0xEC), (0x78, 0x7C, 0xEC), (0xB0, 0x62, 0xEC),
    (0xE4, 0x54, 0xEC), (0xEC, 0x58, 0xB4), (0xEC, 0x6A, 0x64), (0xD4, 0x88, 0x20),
    (0xA0, 0xAA, 0x00), (0x74, 0xC4, 0x00), (0x4C, 0xD0, 0x20), (0x38, 0xCC, 0x6C),
    (0x38, 0xB4, 0xCC), (0x3C, 0x3C, 0x3C), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
    // Row 3: $30-$3F
    (0xEC, 0xEE, 0xEC), (0xA8, 0xCC, 0xEC), (0xBC, 0xBC, 0xEC), (0xD4, 0xB2, 0xEC),
    (0xEC, 0xAE, 0xEC), (0xEC, 0xAE, 0xD4), (0xEC, 0xB4, 0xB0), (0xE4, 0xC4, 0x90),
    (0xCC, 0xD2, 0x78), (0xB4, 0xDE, 0x78), (0xA8, 0xE2, 0x90), (0x98, 0xE2, 0xB4),
    (0xA0, 0xD6, 0xE4), (0xA0, 0xA2, 0xA0), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
];

// === NES Timing Constants (NTSC) ===
// Master clock: 21.477272 MHz
// CPU: 1.789773 MHz (master / 12)
// PPU: 5.369318 MHz (master / 4) = 3x CPU clock

/// CPU frequency in Hz (NTSC).
const CPU_FREQUENCY_NTSC: f64 = 1_789_773.0;

/// Average CPU cycles per frame (odd frames: 29781, even frames: 29780).
const CPU_CYCLES_PER_FRAME: f64 = 29780.5;

/// CPU cycles for even frames.
#[allow(dead_code)]
const CPU_CYCLES_EVEN_FRAME: u32 = 29780;

/// CPU cycles for odd frames.
#[allow(dead_code)]
const CPU_CYCLES_ODD_FRAME: u32 = 29781;

/// Target frame rate (NTSC): `CPU_FREQUENCY_NTSC` / `CPU_CYCLES_PER_FRAME` = ~60.0988 Hz.
const TARGET_FPS: f64 = CPU_FREQUENCY_NTSC / CPU_CYCLES_PER_FRAME;

/// Frame duration in nanoseconds (~16,639,265 ns).
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
    /// Audio/video sync speed adjustment (0.99x - 1.01x).
    speed_adjustment: f32,
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

        // Get audio sample rate for APU configuration
        let sample_rate = audio.as_ref().map_or(48000, AudioOutput::sample_rate);

        // Load console if ROM was provided
        let console = if let Some(rom_path) = &rom_path {
            match Self::load_rom_with_prewarm(rom_path, sample_rate) {
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
            speed_adjustment: 1.0,
        }
    }

    /// Number of frames to pre-warm the emulator after ROM load.
    /// The PPU needs ~40 frames before rendering visible content.
    /// Pre-warming eliminates the grey screen on startup.
    const PREWARM_FRAMES: u32 = 50;

    /// Load a ROM file into a new console instance with pre-warming.
    ///
    /// Pre-warming runs the emulator for `PREWARM_FRAMES` before returning,
    /// eliminating the grey screen that occurs during PPU initialization.
    /// Audio samples generated during pre-warming are cleared.
    fn load_rom_with_prewarm(path: &PathBuf, sample_rate: u32) -> anyhow::Result<Console> {
        let rom_data = std::fs::read(path)?;
        let mut console = Console::from_rom_bytes_with_sample_rate(&rom_data, sample_rate)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Pre-warm the emulator to eliminate grey screen on startup.
        // The first ~40 frames have garbage PPU data during initialization.
        info!(
            "Pre-warming emulator for {} frames to eliminate grey screen...",
            Self::PREWARM_FRAMES
        );
        for _ in 0..Self::PREWARM_FRAMES {
            console.step_frame();
        }

        // Clear audio samples generated during pre-warm to avoid audio burst
        console.clear_audio_samples();

        Ok(console)
    }

    /// Update the NES texture from the console framebuffer.
    ///
    /// The PPU outputs palette indices (0-63). This method converts them
    /// to RGBA using the NES system palette for display.
    fn update_texture(&mut self, ctx: &egui::Context) {
        // Get pixel data from console or use placeholder
        if let Some(console) = &self.console {
            let palette_indices = console.framebuffer();

            // Convert palette indices to RGBA
            // PPU outputs 256x240 palette indices (0-63), we convert to RGBA
            for (i, &palette_idx) in palette_indices.iter().enumerate() {
                // Clamp palette index to valid range (0-63)
                let idx = (palette_idx & 0x3F) as usize;
                let (r, g, b) = NES_PALETTE[idx];

                // Write RGBA to framebuffer (4 bytes per pixel)
                let offset = i * 4;
                if offset + 3 < self.framebuffer.len() {
                    self.framebuffer[offset] = r;
                    self.framebuffer[offset + 1] = g;
                    self.framebuffer[offset + 2] = b;
                    self.framebuffer[offset + 3] = 255; // Full alpha
                }
            }
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
    ///
    /// Returns a speed adjustment factor for A/V synchronization.
    /// - 0.99: Buffer low, slow down slightly
    /// - 1.00: Buffer healthy, no adjustment
    /// - 1.01: Buffer high, speed up slightly
    fn run_frame(&mut self) -> f32 {
        let mut speed_adjustment = 1.0;

        if let Some(console) = &mut self.console {
            // Update controller input BEFORE emulation (minimizes input latency)
            console.set_controller_1(self.input.player1_buttons());
            console.set_controller_2(self.input.player2_buttons());

            // Run one frame
            console.step_frame();

            // Get audio samples and queue them with adaptive sync
            if let Some(audio) = &mut self.audio {
                let samples = console.audio_samples();
                if !samples.is_empty() {
                    // Use adaptive sync to maintain audio/video synchronization
                    let (_, adjustment) = audio.queue_samples_with_sync(samples);
                    speed_adjustment = adjustment;
                }
                console.clear_audio_samples();
            }
        }

        speed_adjustment
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

    /// Get the audio sample rate for APU configuration.
    fn audio_sample_rate(&self) -> u32 {
        self.audio.as_ref().map_or(48000, AudioOutput::sample_rate)
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

            let sample_rate = self.audio_sample_rate();
            match Self::load_rom_with_prewarm(&path, sample_rate) {
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

    /// Handle keyboard input for NES controller using configured bindings.
    ///
    /// Uses direct key state polling to ensure game controller input always works,
    /// even when egui widgets have keyboard focus (e.g., modal dialogs).
    fn handle_controller_keys(&mut self, ctx: &egui::Context) {
        use crate::input::{KeyCode, egui_key_to_keycode};

        ctx.input(|i| {
            // Poll all controller-relevant keys directly using key_down()
            // This ensures input works regardless of egui focus state
            let controller_keys = [
                // D-pad
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                // Action buttons (common bindings)
                egui::Key::X, // A
                egui::Key::Z, // B
                // Start/Select
                egui::Key::Enter, // Start
                // WASD alternative for player 2
                egui::Key::W,
                egui::Key::A,
                egui::Key::S,
                egui::Key::D,
                // Number keys for player 2 buttons
                egui::Key::Num1,
                egui::Key::Num2,
            ];

            for &key in &controller_keys {
                let pressed = i.key_down(key);
                if let Some(keycode) = egui_key_to_keycode(key) {
                    self.input.handle_key_1(keycode, pressed);
                    self.input.handle_key_2(keycode, pressed);
                }
            }

            // Handle shift keys for Select button (not exposed as regular Key events)
            // Check modifiers for shift state changes
            let shift_pressed = i.modifiers.shift;

            // Track shift state for player 1 (ShiftRight is default Select)
            self.input.handle_key_1(KeyCode::ShiftRight, shift_pressed);
            self.input.handle_key_1(KeyCode::ShiftLeft, shift_pressed);

            // Track shift state for player 2
            self.input.handle_key_2(KeyCode::ShiftRight, shift_pressed);
            self.input.handle_key_2(KeyCode::ShiftLeft, shift_pressed);
        });
    }

    /// Handle file drops.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        // Get sample rate before the closure to avoid borrowing issues
        let sample_rate = self.audio_sample_rate();

        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    let rom_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();

                    info!("File dropped: {}", path.display());
                    match Self::load_rom_with_prewarm(path, sample_rate) {
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

        // Frame timing and emulation with A/V sync
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;

        if !self.paused {
            self.accumulator += delta;

            // Apply speed adjustment for A/V sync (0.99x - 1.01x)
            // This slightly adjusts the effective frame duration to maintain audio sync
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss
            )]
            let adjusted_duration = Duration::from_nanos(
                (FRAME_DURATION.as_nanos() as f64 / f64::from(self.speed_adjustment)) as u64,
            );

            // Run emulation to catch up
            while self.accumulator >= adjusted_duration {
                self.accumulator -= adjusted_duration;
                self.speed_adjustment = self.run_frame();
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
