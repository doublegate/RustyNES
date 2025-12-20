//! Configuration system for RustyNES desktop application.
//!
//! This module provides persistent configuration storage using TOML format
//! with platform-specific directory paths (XDG on Linux, AppData on Windows, etc.).

mod settings;

// Re-export commonly used types (others available via settings module)
pub use settings::{AppConfig, CrtPreset, KeyboardMapping, Region, ScalingMode};
