// RustyNES Desktop Application
#![doc = include_str!("../README.md")]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_precision_loss)] // Color conversion from hex
#![allow(clippy::multiple_crate_versions)] // Dependency version conflicts (transitive deps)
#![allow(clippy::doc_markdown)] // README.md formatting

use iced::{window, Size};

mod app;
mod config;
mod input;
mod library;
mod loading;
mod message;
mod metrics;
mod palette;
mod runahead;
mod theme;
mod view;
mod viewport;
mod views;

fn main() -> iced::Result {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .init();

    tracing::info!("Starting RustyNES Desktop v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration to get saved window size
    let config = config::AppConfig::load().unwrap_or_default();
    #[allow(clippy::cast_precision_loss)] // u32 to f32 for window size
    let window_size = Size::new(
        config.app.window_width as f32,
        config.app.window_height as f32,
    );

    // Create application icon (simple colored square placeholder for MVP)
    let icon = create_icon();

    // Run application using Iced 0.13 API
    iced::application(
        app::RustyNes::title,
        app::RustyNes::update,
        app::RustyNes::view,
    )
    .subscription(app::RustyNes::subscription)
    .theme(app::RustyNes::theme)
    .window_size(window_size)
    .window(window::Settings {
        icon,
        ..Default::default()
    })
    .antialiasing(true)
    .run_with(app::RustyNes::new)
}

/// Create application icon (placeholder for MVP)
///
/// Creates a simple 256x256 icon with the RustyNES color scheme.
/// In production, this should be replaced with a proper PNG icon.
fn create_icon() -> Option<window::Icon> {
    use image::{ImageBuffer, Rgba};

    const SIZE: u32 = 256;

    // Create RGBA buffer (256x256 pixels)
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    // Fill with gradient using RustyNES colors
    // Power Red (#E94560) to NES Blue (#0F3460)
    for y in 0..SIZE {
        for x in 0..SIZE {
            let idx = ((y * SIZE + x) * 4) as usize;

            // Gradient from top (red) to bottom (blue)
            let t = y as f32 / SIZE as f32;

            // Power Red #E94560
            let r1 = 0xE9;
            let g1 = 0x45;
            let b1 = 0x60;

            // NES Blue #0F3460
            let r2 = 0x0F;
            let g2 = 0x34;
            let b2 = 0x60;

            // Linear interpolation
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                rgba[idx] = (r1 as f32 * (1.0 - t) + r2 as f32 * t) as u8; // R
                rgba[idx + 1] = (g1 as f32 * (1.0 - t) + g2 as f32 * t) as u8; // G
                rgba[idx + 2] = (b1 as f32 * (1.0 - t) + b2 as f32 * t) as u8; // B
                rgba[idx + 3] = 255; // A (fully opaque)
            }
        }
    }

    // Convert Vec<u8> to ImageBuffer
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(SIZE, SIZE, rgba)?;

    // Convert to iced icon
    let rgba_data = img.into_raw();
    iced::window::icon::from_rgba(rgba_data, SIZE, SIZE).ok()
}
