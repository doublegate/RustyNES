//! `RustyNES` Desktop Frontend Library
//!
//! This crate provides a desktop frontend for the `RustyNES` emulator using:
//! - **eframe**: egui framework for rendering and window management
//! - **egui**: Immediate mode GUI for menus and debug windows
//! - **cpal**: Low-latency audio output
//! - **gilrs**: Gamepad support
//!
//! # Architecture
//!
//! The frontend is organized into the following modules:
//! - [`app`]: Main application implementing `eframe::App`
//! - [`audio`]: cpal-based audio output
//! - [`input`]: Keyboard and gamepad input handling
//! - [`config`]: Configuration persistence (RON format)
//! - [`gui`]: egui-based menu and debug windows

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::multiple_crate_versions)]

pub mod app;
pub mod audio;
pub mod config;
pub mod gui;
pub mod input;

// Re-export main types
pub use app::NesApp;
pub use config::Config;
