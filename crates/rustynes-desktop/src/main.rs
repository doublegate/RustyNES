// RustyNES Desktop Application
#![doc = include_str!("../README.md")]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_precision_loss)] // Color conversion from hex
#![allow(clippy::multiple_crate_versions)] // Dependency version conflicts (transitive deps)
#![allow(clippy::doc_markdown)] // README.md formatting

use iced::Size;

mod app;
mod input;
mod library;
mod message;
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

    // Run application using Iced 0.13 API
    iced::application(
        app::RustyNes::title,
        app::RustyNes::update,
        app::RustyNes::view,
    )
    .subscription(app::RustyNes::subscription)
    .theme(app::RustyNes::theme)
    .window_size(Size::new(768.0, 720.0))
    .antialiasing(true)
    .run_with(app::RustyNes::new)
}
