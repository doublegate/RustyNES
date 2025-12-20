//! Loading states and loading screen UI.
//!
//! Provides state management and UI components for displaying
//! loading progress during ROM loading and emulator initialization.

use std::path::PathBuf;

use iced::widget::{column, container, progress_bar, text};
use iced::{Alignment, Element, Length};

use crate::message::Message;

/// Loading state for the application
#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // Infrastructure for future loading UI
pub enum LoadingState {
    /// No loading in progress (normal operation)
    #[default]
    None,

    /// Loading a ROM file
    LoadingRom {
        /// Path to the ROM being loaded
        path: PathBuf,
        /// Progress from 0.0 to 1.0
        progress: f32,
    },

    /// Initializing emulator core
    InitializingEmulator {
        /// Progress from 0.0 to 1.0
        progress: f32,
    },
}

impl LoadingState {
    /// Check if currently loading
    #[allow(dead_code)] // Will be used when loading UI is active
    pub fn is_loading(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Render loading screen UI
    #[allow(dead_code)] // Will be used when loading UI is active
    pub fn view(&self) -> Option<Element<'static, Message>> {
        match self {
            Self::None => None,
            Self::LoadingRom { path, progress } => {
                let filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string();

                Some(loading_screen(filename, *progress))
            }
            Self::InitializingEmulator { progress } => Some(loading_screen(
                "Initializing emulator".to_string(),
                *progress,
            )),
        }
    }
}

/// Render loading screen with progress bar
#[allow(dead_code)] // Will be used when loading UI is active
fn loading_screen(message: String, progress: f32) -> Element<'static, Message> {
    container(
        column![
            text("RustyNES").size(48),
            iced::widget::vertical_space().height(40),
            text(message).size(18),
            iced::widget::vertical_space().height(20),
            progress_bar(0.0..=1.0, progress).width(Length::Fixed(400.0)),
            iced::widget::vertical_space().height(10),
            text(format!("{:.0}%", progress * 100.0)).size(14),
        ]
        .align_x(Alignment::Center)
        .spacing(5),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
