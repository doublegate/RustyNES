//! Main Iced application implementing the Elm architecture pattern.
//!
//! The Elm architecture consists of three parts:
//! - Model: Application state (`RustyNes` struct)
//! - Update: State transitions (`update()` function)
//! - View: UI rendering (`view()` function)

use std::path::PathBuf;
use std::sync::Arc;

use iced::{Element, Task, Theme};
use tracing::{error, info};

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
}

impl RustyNes {
    /// Create new application state
    pub fn new() -> (Self, Task<Message>) {
        info!("Initializing RustyNES application");

        // Create default framebuffer (256×240×3 RGB)
        let framebuffer = Arc::new(vec![0u8; 256 * 240 * 3]);

        let app = Self {
            current_view: View::Welcome,
            console: None,
            current_rom: None,
            theme: Theme::Dark,
            framebuffer,
            scaling_mode: ScalingMode::PixelPerfect,
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
        }
    }

    /// Render UI (Elm View)
    pub fn view(&self) -> Element<'_, Message> {
        match &self.current_view {
            View::Welcome => crate::views::welcome::view(self),
            View::Playing => crate::views::playing::view(self),
        }
    }

    /// Get application theme
    pub fn theme(&self) -> Theme {
        self.theme.clone()
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
