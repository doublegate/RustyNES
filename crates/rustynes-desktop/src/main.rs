// RustyNES Desktop Application
#![doc = include_str!("../README.md")]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_precision_loss)] // Color conversion from hex
#![allow(clippy::multiple_crate_versions)] // Dependency version conflicts (transitive deps)
#![allow(clippy::doc_markdown)] // README.md formatting

use std::env;
use std::path::PathBuf;

use iced::{window, Size};

mod app;
mod audio;
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

    // Parse command-line arguments for ROM path
    let rom_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.exists() && p.extension().is_some_and(|ext| ext == "nes"));

    if let Some(ref path) = rom_path {
        tracing::info!("ROM path provided via CLI: {}", path.display());
    }

    // Load configuration to get saved window size
    let config = config::AppConfig::load().unwrap_or_default();
    #[allow(clippy::cast_precision_loss)] // u32 to f32 for window size
    let window_size = Size::new(
        config.app.window_width as f32,
        config.app.window_height as f32,
    );

    // TODO: Load icon from PNG file instead of generating (causes debug spam)
    // For now, use default system icon

    // Run application using Iced 0.13 API
    iced::application(
        app::RustyNes::title,
        app::RustyNes::update,
        app::RustyNes::view,
    )
    .subscription(app::RustyNes::subscription)
    .theme(app::RustyNes::theme)
    .window_size(window_size)
    .window(window::Settings::default())
    .antialiasing(true)
    .run_with(move || app::RustyNes::new(rom_path.clone()))
}

// Icon generation removed - was causing 5MB debug spam in logs
// TODO: Load icon from PNG file in assets/ directory
