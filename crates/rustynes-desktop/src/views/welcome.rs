//! Welcome screen view.
//!
//! This is the initial screen shown when the application launches with no ROM loaded.
//! Provides a clean interface for loading ROMs via file dialog.

use iced::widget::{button, column, container, text, Column};
use iced::{Element, Length};

use crate::app::RustyNes;
use crate::message::Message;
use crate::theme::RustyPalette;

/// Render the Welcome view
pub fn view(_app: &RustyNes) -> Element<'_, Message> {
    let palette = RustyPalette::dark();

    // Title with "Power Red" color (primary)
    let title = text("RustyNES").size(48).color(palette.primary);

    // Subtitle
    let subtitle = text("Next-Generation NES Emulator")
        .size(20)
        .color(palette.accent);

    // Version info
    let version = text(format!("v{}", env!("CARGO_PKG_VERSION"))).size(14);

    // Open ROM button
    let open_button = button(text("Open ROM").size(18))
        .padding(15)
        .on_press(Message::OpenFileDialog);

    // Main content column
    let content: Column<Message> = column![title, subtitle, version, open_button,]
        .spacing(20)
        .padding(40)
        .align_x(iced::alignment::Horizontal::Center);

    // Centered container
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
