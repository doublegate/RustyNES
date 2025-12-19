//! Playing view - active gameplay screen with NES viewport.
//!
//! This view displays the running NES emulator with the game viewport
//! and minimal UI overlay.

use iced::widget::column;
use iced::{Alignment, Element, Length};

use crate::app::RustyNes;
use crate::message::Message;

/// Render the Playing view
pub fn view(model: &RustyNes) -> Element<'_, Message> {
    // Create viewport from framebuffer
    let viewport = crate::viewport::GameViewport::new(model.framebuffer().clone())
        .scaling(model.scaling_mode())
        .into_element();

    // Main layout
    column![viewport]
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}
