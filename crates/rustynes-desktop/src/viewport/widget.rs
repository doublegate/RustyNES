//! Custom Iced widget for NES game viewport.
//!
//! This module provides a widget that renders the NES framebuffer
//! using Iced's image widget with dynamic updates.

use std::sync::Arc;

use iced::widget::image;
use iced::{ContentFit, Element, Length};

use super::ScalingMode;

/// Game viewport widget for rendering NES frames
pub struct GameViewport {
    /// Shared framebuffer (updated by emulator thread)
    framebuffer: Arc<Vec<u8>>,
    /// Scaling mode
    scaling: ScalingMode,
}

impl GameViewport {
    /// Create a new game viewport
    pub fn new(framebuffer: Arc<Vec<u8>>) -> Self {
        Self {
            framebuffer,
            scaling: ScalingMode::PixelPerfect,
        }
    }

    /// Set scaling mode
    pub fn scaling(mut self, mode: ScalingMode) -> Self {
        self.scaling = mode;
        self
    }

    /// Convert to Iced Element
    ///
    /// For now, we use a placeholder implementation.
    /// Full wgpu integration will be completed in the widget implementation.
    pub fn into_element<'a, Message: 'a>(self) -> Element<'a, Message> {
        // Create a simple container as placeholder
        // The full wgpu rendering pipeline will be integrated here
        let content_fit = match self.scaling {
            ScalingMode::Stretch => ContentFit::Fill,
            ScalingMode::AspectRatio4x3 | ScalingMode::PixelPerfect => ContentFit::Contain,
            ScalingMode::IntegerScaling => ContentFit::ScaleDown,
        };

        // For now, create an image widget from the framebuffer
        // This will be replaced with custom rendering in the pipeline integration
        let handle = create_image_handle(&self.framebuffer);

        iced::widget::container(
            image::Image::new(handle)
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(content_fit),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }
}

/// Create an image handle from RGB framebuffer
fn create_image_handle(framebuffer: &[u8]) -> image::Handle {
    // Convert RGB to RGBA
    let mut rgba = Vec::with_capacity(256 * 240 * 4);
    for chunk in framebuffer.chunks_exact(3) {
        rgba.push(chunk[0]); // R
        rgba.push(chunk[1]); // G
        rgba.push(chunk[2]); // B
        rgba.push(255); // A
    }

    image::Handle::from_rgba(256, 240, rgba)
}
