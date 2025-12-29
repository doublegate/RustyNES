//! `RustyNES` Desktop - NES Emulator Frontend
//!
//! A desktop frontend for the `RustyNES` emulator using egui + eframe.
//!
//! # Usage
//!
//! ```bash
//! rustynes [OPTIONS] [ROM_PATH]
//! ```
//!
//! # Examples
//!
//! ```bash
//! # Launch without a ROM (opens file dialog)
//! rustynes
//!
//! # Launch with a specific ROM
//! rustynes path/to/game.nes
//!
//! # Launch with debug mode enabled
//! rustynes --debug path/to/game.nes
//! ```

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info};
use std::path::PathBuf;

use rustynes_desktop::{Config, NesApp};

/// Command-line arguments for `RustyNES`
#[derive(Parser, Debug)]
#[command(name = "rustynes")]
#[command(author = "doublegate")]
#[command(version = "0.8.2")]
#[command(about = "A cycle-accurate NES emulator written in Rust")]
#[command(long_about = None)]
struct Args {
    /// Path to a NES ROM file (.nes)
    #[arg(value_name = "ROM")]
    rom_path: Option<PathBuf>,

    /// Start in fullscreen mode
    #[arg(short, long)]
    fullscreen: bool,

    /// Window scale factor (1-8)
    #[arg(short, long, default_value = "3")]
    scale: u32,

    /// Enable debug mode (shows debug windows)
    #[arg(short, long)]
    debug: bool,

    /// Mute audio on startup
    #[arg(short, long)]
    mute: bool,
}

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    info!("RustyNES v0.8.2 starting...");

    // Parse command-line arguments
    let args = Args::parse();

    // Load or create configuration
    let mut config = Config::load().unwrap_or_else(|e| {
        error!("Failed to load config, using defaults: {e}");
        Config::default()
    });

    // Apply command-line overrides
    if args.fullscreen {
        config.video.fullscreen = true;
    }
    if args.scale >= 1 && args.scale <= 8 {
        config.video.scale = args.scale;
    }
    if args.debug {
        config.debug.enabled = true;
    }
    if args.mute {
        config.audio.muted = true;
    }

    // Calculate initial window size (scale is bounded 1-8, safe for f32)
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = {
        let scale = f32::from(config.video.scale as u16);
        (256.0 * scale, 240.0 * scale + 25.0) // Extra for menu bar
    };

    // Set up native options for eframe
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width, height])
            .with_min_inner_size([256.0, 265.0])
            .with_title("RustyNES")
            .with_fullscreen(config.video.fullscreen),
        vsync: config.video.vsync,
        ..Default::default()
    };

    // Create and run the application
    eframe::run_native(
        "RustyNES",
        native_options,
        Box::new(move |cc| Ok(Box::new(NesApp::new(cc, config, args.rom_path)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
    .context("Failed to run application")?;

    Ok(())
}
