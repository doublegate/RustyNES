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

use crate::input::{gamepad::GamepadManager, keyboard::KeyboardMapper, InputState};
use crate::library::LibraryState;
use crate::message::Message;
use crate::view::View;
use crate::viewport::ScalingMode;
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

    /// Shared framebuffer (updated by emulator, read by renderer)
    framebuffer: Arc<Vec<u8>>,

    /// Viewport scaling mode
    scaling_mode: ScalingMode,

    /// Input state for both players
    input_state: InputState,

    /// Keyboard mapper
    keyboard_mapper: KeyboardMapper,

    /// Gamepad manager
    gamepad_manager: Option<GamepadManager>,

    /// ROM library state
    library: LibraryState,
}

impl RustyNes {
    /// Create new application state
    pub fn new() -> (Self, Task<Message>) {
        info!("Initializing RustyNES application");

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
            framebuffer,
            scaling_mode: ScalingMode::PixelPerfect,
            input_state: InputState::new(),
            keyboard_mapper: KeyboardMapper::new(),
            gamepad_manager,
            library: LibraryState::new(),
        };

        (app, Task::none())
    }

    /// Get framebuffer for rendering
    pub fn framebuffer(&self) -> &Arc<Vec<u8>> {
        &self.framebuffer
    }

    /// Get scaling mode
    pub fn scaling_mode(&self) -> ScalingMode {
        self.scaling_mode
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
                self.scaling_mode = mode;
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
        match &self.current_view {
            View::Welcome => crate::views::welcome::view(self),
            View::Library => crate::views::library::view(&self.library),
            View::Playing => crate::views::playing::view(self),
        }
    }

    /// Get application theme
    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    /// Subscriptions for ongoing events (keyboard, gamepad polling)
    pub fn subscription(&self) -> Subscription<Message> {
        use iced::keyboard;

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

        Subscription::batch(vec![keyboard_sub, gamepad_sub])
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
