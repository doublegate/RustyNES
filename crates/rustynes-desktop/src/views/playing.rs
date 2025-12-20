//! Playing view - active gameplay screen with NES viewport.
//!
//! This view displays the running NES emulator with the game viewport
//! and minimal UI overlay (including optional metrics overlay).

use iced::widget::{column, container, stack};
use iced::{Alignment, Element, Length};

use crate::app::RustyNes;
use crate::message::Message;

/// Render the Playing view
pub fn view(model: &RustyNes) -> Element<'_, Message> {
    // Create viewport from framebuffer
    let viewport = crate::viewport::GameViewport::new(model.framebuffer().clone())
        .scaling(model.scaling_mode())
        .into_element();

    // Create base layout with viewport
    let base = column![viewport]
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .into();

    // Overlay metrics if enabled
    if model.show_metrics() {
        // Position metrics in top-left corner
        let metrics_overlay = container(model.metrics().view(true)).padding(10);

        stack![base, metrics_overlay].into()
    } else {
        base
    }
}
