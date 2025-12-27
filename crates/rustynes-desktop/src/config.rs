//! Configuration management for `RustyNES` desktop frontend.
//!
//! Configuration is stored in RON format in the platform-specific config directory:
//! - Linux: `~/.config/rustynes/config.ron`
//! - macOS: `~/Library/Application Support/rustynes/config.ron`
//! - Windows: `%APPDATA%\rustynes\config.ron`

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Video configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct VideoConfig {
    /// Window scale factor (1-8).
    pub scale: u32,
    /// Start in fullscreen mode.
    pub fullscreen: bool,
    /// Enable `VSync`.
    pub vsync: bool,
    /// Maintain 8:7 pixel aspect ratio (NES native).
    pub pixel_aspect_correction: bool,
    /// Show FPS counter.
    pub show_fps: bool,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            scale: 3,
            fullscreen: false,
            vsync: true,
            pixel_aspect_correction: true,
            show_fps: false,
        }
    }
}

/// Audio configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Master volume (0.0 - 1.0).
    pub volume: f32,
    /// Mute audio.
    pub muted: bool,
    /// Audio sample rate.
    pub sample_rate: u32,
    /// Audio buffer size in samples.
    pub buffer_size: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            volume: 0.8,
            muted: false,
            sample_rate: 44100,
            buffer_size: 2048,
        }
    }
}

/// Input configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Player 1 keyboard bindings.
    pub player1_keyboard: KeyboardBindings,
    /// Player 2 keyboard bindings.
    pub player2_keyboard: KeyboardBindings,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            player1_keyboard: KeyboardBindings::player1_defaults(),
            player2_keyboard: KeyboardBindings::player2_defaults(),
        }
    }
}

/// Keyboard key bindings for a single player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardBindings {
    /// A button.
    pub a: String,
    /// B button.
    pub b: String,
    /// Select button.
    pub select: String,
    /// Start button.
    pub start: String,
    /// D-pad Up.
    pub up: String,
    /// D-pad Down.
    pub down: String,
    /// D-pad Left.
    pub left: String,
    /// D-pad Right.
    pub right: String,
}

impl KeyboardBindings {
    /// Default keyboard bindings for player 1.
    #[must_use]
    pub fn player1_defaults() -> Self {
        Self {
            a: "KeyX".to_string(),
            b: "KeyZ".to_string(),
            select: "ShiftRight".to_string(),
            start: "Enter".to_string(),
            up: "ArrowUp".to_string(),
            down: "ArrowDown".to_string(),
            left: "ArrowLeft".to_string(),
            right: "ArrowRight".to_string(),
        }
    }

    /// Default keyboard bindings for player 2.
    #[must_use]
    pub fn player2_defaults() -> Self {
        Self {
            a: "KeyG".to_string(),
            b: "KeyF".to_string(),
            select: "KeyT".to_string(),
            start: "KeyY".to_string(),
            up: "KeyW".to_string(),
            down: "KeyS".to_string(),
            left: "KeyA".to_string(),
            right: "KeyD".to_string(),
        }
    }
}

/// Debug configuration options.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct DebugConfig {
    /// Enable debug mode.
    pub enabled: bool,
    /// Show CPU debug window.
    pub show_cpu: bool,
    /// Show PPU debug window.
    pub show_ppu: bool,
    /// Show APU debug window.
    pub show_apu: bool,
    /// Show memory viewer.
    pub show_memory: bool,
}

/// Recent ROM paths for quick access.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentRoms {
    /// List of recently opened ROM paths (most recent first).
    pub paths: Vec<PathBuf>,
    /// Maximum number of recent ROMs to remember.
    pub max_entries: usize,
}

impl RecentRoms {
    /// Add a ROM path to the recent list.
    pub fn add(&mut self, path: PathBuf) {
        // Remove if already exists
        self.paths.retain(|p| p != &path);
        // Add to front
        self.paths.insert(0, path);
        // Trim to max entries
        if self.max_entries == 0 {
            self.max_entries = 10;
        }
        self.paths.truncate(self.max_entries);
    }
}

/// Complete application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Video settings.
    pub video: VideoConfig,
    /// Audio settings.
    pub audio: AudioConfig,
    /// Input settings.
    pub input: InputConfig,
    /// Debug settings.
    pub debug: DebugConfig,
    /// Recent ROM paths.
    pub recent_roms: RecentRoms,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            video: VideoConfig::default(),
            audio: AudioConfig::default(),
            input: InputConfig::default(),
            debug: DebugConfig::default(),
            recent_roms: RecentRoms {
                paths: Vec::new(),
                max_entries: 10,
            },
        }
    }
}

impl Config {
    /// Get the configuration directory path.
    fn config_dir() -> Result<PathBuf> {
        ProjectDirs::from("com", "doublegate", "rustynes")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .context("Failed to determine config directory")
    }

    /// Get the configuration file path.
    fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.ron"))
    }

    /// Load configuration from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be read or parsed.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config from {}", path.display()))?;
            ron::from_str(&content)
                .with_context(|| format!("Failed to parse config from {}", path.display()))
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file cannot be written.
    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

        let path = Self::config_path()?;
        let content = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .context("Failed to serialize config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }
}
