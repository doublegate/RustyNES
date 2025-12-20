//! Main Iced application implementing the Elm architecture pattern.
//!
//! The Elm architecture consists of three parts:
//! - Model: Application state (`RustyNes` struct)
//! - Update: State transitions (`update()` function)
//! - View: UI rendering (`view()` function)

use std::path::PathBuf;
use std::sync::Arc;

use iced::{Element, Subscription, Task, Theme};
use tracing::{error, info};

use crate::config::AppConfig;
use crate::input::{gamepad::GamepadManager, keyboard::KeyboardMapper, InputState};
use crate::library::LibraryState;
use crate::message::Message;
use crate::view::View;
use rustynes_core::Console;

/// Main application state (Elm Model)
pub struct RustyNes {
    /// Current view/screen
    current_view: View,

    /// Emulator core (None when no ROM loaded)
    console: Option<Console>,

    /// Currently loaded ROM path
    current_rom: Option<PathBuf>,

    /// Application theme
    theme: Theme,

    /// Application configuration
    config: AppConfig,

    /// Shared framebuffer (updated by emulator, read by renderer)
    framebuffer: Arc<Vec<u8>>,

    /// Input state for both players
    input_state: InputState,

    /// Keyboard mapper
    keyboard_mapper: KeyboardMapper,

    /// Gamepad manager
    gamepad_manager: Option<GamepadManager>,

    /// ROM library state
    library: LibraryState,

    /// Show about dialog
    show_about: bool,
}

impl RustyNes {
    /// Create new application state
    pub fn new() -> (Self, Task<Message>) {
        info!("Initializing RustyNES application");

        // Load configuration from disk
        let config = AppConfig::load().unwrap_or_else(|e| {
            error!("Failed to load config: {}, using defaults", e);
            AppConfig::default()
        });

        // Create default framebuffer (256×240×3 RGB)
        let framebuffer = Arc::new(vec![0u8; 256 * 240 * 3]);

        // Initialize gamepad manager (may fail on platforms without gamepad support)
        let gamepad_manager = match GamepadManager::new() {
            Ok(mgr) => {
                info!("Gamepad support initialized");
                Some(mgr)
            }
            Err(e) => {
                error!("Failed to initialize gamepad support: {}", e);
                None
            }
        };

        let app = Self {
            current_view: View::Library,
            console: None,
            current_rom: None,
            theme: Theme::Dark,
            config,
            framebuffer,
            input_state: InputState::new(),
            keyboard_mapper: KeyboardMapper::new(),
            gamepad_manager,
            library: LibraryState::new(),
            show_about: false,
        };

        (app, Task::none())
    }

    /// Get framebuffer for rendering
    pub fn framebuffer(&self) -> &Arc<Vec<u8>> {
        &self.framebuffer
    }

    /// Get scaling mode
    pub fn scaling_mode(&self) -> crate::config::ScalingMode {
        self.config.video.scaling_mode
    }

    /// Get window title based on current state
    pub fn title(&self) -> String {
        if let Some(rom_path) = &self.current_rom {
            format!(
                "RustyNES - {}",
                rom_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
            )
        } else {
            "RustyNES - NES Emulator".to_string()
        }
    }

    /// Update application state (Elm Update)
    #[allow(clippy::too_many_lines)] // Elm update pattern naturally grows with features
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::None => Task::none(),

            Message::NavigateTo(view) => {
                info!("Navigating to view: {:?}", view);
                self.current_view = view;
                Task::none()
            }

            Message::OpenFileDialog => {
                info!("Opening file dialog");
                Task::future(async {
                    if let Some(path) = Self::show_file_dialog().await {
                        Message::LoadRom(path)
                    } else {
                        Message::None
                    }
                })
            }

            Message::LoadRom(path) => {
                info!("Loading ROM from: {}", path.display());
                self.current_rom = Some(path.clone());
                Task::future(async move { Message::RomLoaded(Self::load_rom_async(path).await) })
            }

            Message::RomLoaded(result) => match result {
                Ok(rom_data) => {
                    info!("ROM loaded successfully, creating console...");
                    // Parse ROM and create mapper
                    match rustynes_core::Rom::load(&rom_data) {
                        Ok(rom) => {
                            match rustynes_core::create_mapper(&rom) {
                                Ok(mapper) => {
                                    let console = rustynes_core::Console::new(mapper);
                                    self.console = Some(console);
                                    self.current_view = View::Playing;
                                    info!("Console created, switching to Playing view");
                                }
                                Err(e) => {
                                    error!("Failed to create mapper: {:?}", e);
                                    // TODO: Show error dialog in later sprint
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse ROM: {:?}", e);
                            // TODO: Show error dialog in later sprint
                        }
                    }
                    Task::none()
                }
                Err(e) => {
                    error!("Failed to load ROM: {}", e);
                    // TODO: Show error dialog in later sprint
                    Task::none()
                }
            },

            Message::SetScalingMode(mode) => {
                info!("Changing scaling mode to: {:?}", mode);
                self.config.video.scaling_mode = mode;
                Task::none()
            }

            // Settings UI
            Message::OpenSettings => {
                info!("Opening settings");
                self.current_view = View::Settings(crate::view::SettingsTab::Emulation);
                Task::none()
            }

            Message::CloseSettings => {
                info!("Closing settings");
                self.current_view = View::Library;
                // Auto-save on close
                let config = self.config.clone();
                Task::future(async move {
                    match config.save() {
                        Ok(()) => Message::ConfigSaved(Ok(())),
                        Err(e) => Message::ConfigSaved(Err(e.to_string())),
                    }
                })
            }

            Message::SelectSettingsTab(tab) => {
                self.current_view = View::Settings(tab);
                Task::none()
            }

            Message::ResetSettingsToDefaults => {
                info!("Resetting settings to defaults");
                self.config = AppConfig::default();
                let config = self.config.clone();
                Task::future(async move {
                    match config.save() {
                        Ok(()) => Message::ConfigSaved(Ok(())),
                        Err(e) => Message::ConfigSaved(Err(e.to_string())),
                    }
                })
            }

            // Emulation settings
            Message::UpdateEmulationSpeed(speed) => {
                self.config.emulation.speed = speed;
                Task::none()
            }

            Message::UpdateRegion(region) => {
                self.config.emulation.region = region;
                Task::none()
            }

            Message::ToggleRewind(enabled) => {
                self.config.emulation.rewind_enabled = enabled;
                Task::none()
            }

            Message::UpdateRewindBufferSize(size) => {
                self.config.emulation.rewind_buffer_size = size;
                Task::none()
            }

            // Video settings
            Message::UpdateScalingMode(mode) => {
                self.config.video.scaling_mode = mode;
                Task::none()
            }

            Message::ToggleVSync(enabled) => {
                self.config.video.vsync = enabled;
                Task::none()
            }

            Message::ToggleCrtShader(enabled) => {
                self.config.video.crt_shader = enabled;
                Task::none()
            }

            Message::UpdateCrtPreset(preset) => {
                self.config.video.crt_preset = preset;
                Task::none()
            }

            Message::UpdateOverscanTop(value) => {
                self.config.video.overscan.top = value;
                Task::none()
            }

            Message::UpdateOverscanBottom(value) => {
                self.config.video.overscan.bottom = value;
                Task::none()
            }

            Message::UpdateOverscanLeft(value) => {
                self.config.video.overscan.left = value;
                Task::none()
            }

            Message::UpdateOverscanRight(value) => {
                self.config.video.overscan.right = value;
                Task::none()
            }

            // Audio settings
            Message::ToggleAudio(enabled) => {
                self.config.audio.enabled = enabled;
                Task::none()
            }

            Message::UpdateSampleRate(rate) => {
                self.config.audio.sample_rate = rate;
                Task::none()
            }

            Message::UpdateVolume(volume) => {
                self.config.audio.volume = volume;
                Task::none()
            }

            Message::UpdateBufferSize(size) => {
                self.config.audio.buffer_size = size;
                Task::none()
            }

            // Input settings
            Message::UpdateGamepadDeadzone(deadzone) => {
                self.config.input.gamepad_deadzone = deadzone;
                Task::none()
            }

            Message::RemapKey {
                player: _,
                button: _,
            } => {
                // TODO: Implement key remapping in future sprint
                Task::none()
            }

            // Persistence
            Message::SaveConfig => {
                let config = self.config.clone();
                Task::future(async move {
                    match config.save() {
                        Ok(()) => Message::ConfigSaved(Ok(())),
                        Err(e) => Message::ConfigSaved(Err(e.to_string())),
                    }
                })
            }

            Message::ConfigSaved(result) => {
                if let Err(e) = result {
                    error!("Failed to save config: {}", e);
                } else {
                    info!("Configuration saved successfully");
                }
                Task::none()
            }

            Message::LoadConfig => Task::future(async {
                match AppConfig::load() {
                    Ok(_) => Message::ConfigLoaded(Ok(())),
                    Err(e) => Message::ConfigLoaded(Err(e.to_string())),
                }
            }),

            Message::ConfigLoaded(result) => {
                match result {
                    Ok(()) => {
                        info!("Configuration loaded successfully");
                    }
                    Err(e) => {
                        error!("Failed to load config: {}", e);
                    }
                }
                Task::none()
            }

            // Recent ROMs
            Message::LoadRecentRom(index) => {
                if let Some(path) = self.config.app.recent_roms.get(index).cloned() {
                    info!("Loading recent ROM: {}", path.display());
                    Task::perform(async move { Message::LoadRom(path) }, |msg| msg)
                } else {
                    Task::none()
                }
            }

            Message::ClearRecentRoms => {
                info!("Clearing recent ROMs list");
                self.config.clear_recent_roms();
                let config = self.config.clone();
                Task::future(async move {
                    match config.save() {
                        Ok(()) => Message::ConfigSaved(Ok(())),
                        Err(e) => Message::ConfigSaved(Err(e.to_string())),
                    }
                })
            }

            // About dialog
            Message::ShowAbout => {
                self.show_about = true;
                Task::none()
            }

            Message::CloseAbout => {
                self.show_about = false;
                Task::none()
            }

            Message::OpenUrl(url) => {
                info!("Opening URL: {}", url);
                // Open URL in browser
                if let Err(e) = opener::open(&url) {
                    error!("Failed to open URL: {}", e);
                }
                Task::none()
            }

            Message::WindowResized(width, height) => {
                // Update window size in config
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    self.config.app.window_width = width as u32;
                    self.config.app.window_height = height as u32;
                }
                Task::none()
            }

            Message::Exit => {
                info!("Exiting application");
                iced::exit()
            }

            // Input messages
            Message::KeyPressed(key) => {
                // Map to Player 1
                if let Some(button) = self.keyboard_mapper.map_player1(&key) {
                    self.input_state.player1.set(button, true);
                }
                // Map to Player 2
                if let Some(button) = self.keyboard_mapper.map_player2(&key) {
                    self.input_state.player2.set(button, true);
                }

                // Apply to console if one is loaded
                if let Some(console) = &mut self.console {
                    self.input_state.apply_to_console(console);
                }

                Task::none()
            }

            Message::KeyReleased(key) => {
                // Map to Player 1
                if let Some(button) = self.keyboard_mapper.map_player1(&key) {
                    self.input_state.player1.set(button, false);
                }
                // Map to Player 2
                if let Some(button) = self.keyboard_mapper.map_player2(&key) {
                    self.input_state.player2.set(button, false);
                }

                // Apply to console if one is loaded
                if let Some(console) = &mut self.console {
                    self.input_state.apply_to_console(console);
                }

                Task::none()
            }

            Message::PollGamepads => {
                if let Some(gamepad_mgr) = &mut self.gamepad_manager {
                    gamepad_mgr.poll(&mut self.input_state.player1, &mut self.input_state.player2);

                    // Apply to console if one is loaded
                    if let Some(console) = &mut self.console {
                        self.input_state.apply_to_console(console);
                    }
                }

                Task::none()
            }

            // Library messages
            Message::LibrarySearch(query) => {
                self.library.set_search_query(query);
                Task::none()
            }

            Message::ToggleLibraryView => {
                self.library.toggle_view_mode();
                Task::none()
            }

            Message::SelectRomDirectory => Task::future(async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Select ROM Directory")
                    .pick_folder()
                    .await;

                Message::RomDirectorySelected(handle.map(|h| h.path().to_path_buf()))
            }),

            Message::RomDirectorySelected(maybe_dir) => {
                if let Some(dir) = &maybe_dir {
                    self.library.scan_directory(dir);
                }
                Task::none()
            }
        }
    }

    /// Render UI (Elm View)
    pub fn view(&self) -> Element<'_, Message> {
        let main_view = match &self.current_view {
            View::Welcome => crate::views::welcome::view(self),
            View::Library => crate::views::library::view(&self.library),
            View::Playing => crate::views::playing::view(self),
            View::Settings(tab) => crate::views::settings::view(&self.config, *tab),
        };

        // Overlay about dialog if shown
        if self.show_about {
            iced::widget::stack![main_view, about_dialog(),].into()
        } else {
            main_view
        }
    }

    /// Get application theme
    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    /// Subscriptions for ongoing events (keyboard, gamepad polling)
    pub fn subscription(&self) -> Subscription<Message> {
        use iced::keyboard;
        use iced::window;

        // Keyboard events
        let keyboard_sub = Subscription::batch(vec![
            keyboard::on_key_press(|key, _modifiers| Some(Message::KeyPressed(key))),
            keyboard::on_key_release(|key, _modifiers| Some(Message::KeyReleased(key))),
        ]);

        // Gamepad polling (every 16ms ≈ 60Hz)
        let gamepad_sub = if self.gamepad_manager.is_some() {
            iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::PollGamepads)
        } else {
            Subscription::none()
        };

        // Window resize events
        let window_sub = window::resize_events()
            .map(|(_, size)| Message::WindowResized(size.width, size.height));

        Subscription::batch(vec![keyboard_sub, gamepad_sub, window_sub])
    }
    /// Asynchronously load ROM file data
    async fn load_rom_async(path: PathBuf) -> Result<Vec<u8>, String> {
        // Read ROM file
        let rom_data = tokio::fs::read(&path)
            .await
            .map_err(|e| format!("Failed to read ROM file: {e}"))?;

        Ok(rom_data)
    }

    /// Show native file dialog for ROM selection
    async fn show_file_dialog() -> Option<PathBuf> {
        tokio::task::spawn_blocking(|| {
            rfd::FileDialog::new()
                .add_filter("NES ROM", &["nes"])
                .set_title("Open NES ROM")
                .pick_file()
        })
        .await
        .ok()
        .flatten()
    }
}

/// Render About dialog as modal overlay
fn about_dialog() -> Element<'static, Message> {
    use iced::widget::{button, column, container, row, text};
    use iced::{Alignment, Length};

    let version = env!("CARGO_PKG_VERSION");

    container(
        container(
            column![
                text("RustyNES").size(28),
                text(format!("Version {version}")).size(14),
                iced::widget::vertical_space().height(10),
                text("A next-generation NES emulator written in Rust"),
                iced::widget::vertical_space().height(20),
                text("Features:").size(16),
                text("• Cycle-accurate CPU emulation"),
                text("• Dot-accurate PPU rendering"),
                text("• Save states and rewind"),
                text("• Game library management"),
                iced::widget::vertical_space().height(20),
                row![
                    button("GitHub").on_press(Message::OpenUrl(
                        "https://github.com/doublegate/RustyNES".to_string()
                    )),
                    button("Documentation").on_press(Message::OpenUrl(
                        "https://github.com/doublegate/RustyNES/blob/main/README.md".to_string()
                    )),
                ]
                .spacing(10),
                iced::widget::vertical_space().height(20),
                button("Close")
                    .on_press(Message::CloseAbout)
                    .width(Length::Fill),
            ]
            .spacing(10)
            .padding(30)
            .width(Length::Fixed(400.0))
            .align_x(Alignment::Center),
        )
        .style(container::bordered_box),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme: &Theme| {
        // Semi-transparent dark overlay
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgba(
                0.0, 0.0, 0.0, 0.7,
            ))),
            ..Default::default()
        }
    })
    .into()
}

/// Auto-save configuration on application exit
impl Drop for RustyNes {
    fn drop(&mut self) {
        info!("Saving configuration on exit");
        if let Err(e) = self.config.save() {
            error!("Failed to save configuration: {e}");
        }
    }
}
