//! Application configuration data structures and persistence.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export ScalingMode from viewport module
pub use crate::viewport::ScalingMode;

/// Main application configuration (TOML serializable)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Emulation settings
    #[serde(default)]
    pub emulation: EmulationConfig,

    /// Video/rendering settings
    #[serde(default)]
    pub video: VideoConfig,

    /// Audio settings
    #[serde(default)]
    pub audio: AudioConfig,

    /// Input mappings
    #[serde(default)]
    pub input: InputConfig,

    /// Application settings
    #[serde(default)]
    pub app: ApplicationConfig,
}

/// Emulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulationConfig {
    /// Emulation speed multiplier (1.0 = 60 FPS)
    pub speed: f32,

    /// Region (NTSC/PAL)
    pub region: Region,

    /// Rewind enabled
    pub rewind_enabled: bool,

    /// Rewind buffer size (frames)
    pub rewind_buffer_size: usize,
}

/// Region setting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::upper_case_acronyms)] // NTSC and PAL are standard acronyms
pub enum Region {
    /// NTSC (60.0988 Hz)
    NTSC,
    /// PAL (50.0070 Hz)
    PAL,
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NTSC => write!(f, "NTSC"),
            Self::PAL => write!(f, "PAL"),
        }
    }
}

/// Video configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    /// Scaling mode
    pub scaling_mode: ScalingMode,

    /// VSync enabled
    pub vsync: bool,

    /// CRT shader enabled
    pub crt_shader: bool,

    /// CRT shader preset
    pub crt_preset: CrtPreset,

    /// Overscan cropping (pixels)
    pub overscan: OverscanConfig,
}

/// CRT shader preset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrtPreset {
    /// No CRT effect
    None,
    /// Subtle scanlines
    Subtle,
    /// Moderate CRT effect
    Moderate,
    /// Authentic CRT appearance
    Authentic,
    /// Custom settings
    Custom,
}

impl std::fmt::Display for CrtPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Subtle => write!(f, "Subtle"),
            Self::Moderate => write!(f, "Moderate"),
            Self::Authentic => write!(f, "Authentic"),
            Self::Custom => write!(f, "Custom"),
        }
    }
}

/// Overscan configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverscanConfig {
    /// Top pixels to crop
    pub top: u32,
    /// Bottom pixels to crop
    pub bottom: u32,
    /// Left pixels to crop
    pub left: u32,
    /// Right pixels to crop
    pub right: u32,
}

/// Audio configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Audio enabled
    pub enabled: bool,

    /// Sample rate (Hz)
    pub sample_rate: u32,

    /// Master volume (0.0-1.0)
    pub volume: f32,

    /// Buffer size (samples)
    pub buffer_size: u32,
}

/// Input configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Player 1 keyboard mapping
    pub keyboard_p1: KeyboardMapping,

    /// Player 2 keyboard mapping
    pub keyboard_p2: KeyboardMapping,

    /// Gamepad analog deadzone (0.0-1.0)
    pub gamepad_deadzone: f32,
}

/// Keyboard button mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardMapping {
    /// Up button
    pub up: String,
    /// Down button
    pub down: String,
    /// Left button
    pub left: String,
    /// Right button
    pub right: String,
    /// A button
    pub a: String,
    /// B button
    pub b: String,
    /// Select button
    pub select: String,
    /// Start button
    pub start: String,
}

/// Application-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationConfig {
    /// Recent ROMs list (max 10)
    pub recent_roms: Vec<PathBuf>,

    /// Default ROM directory
    pub rom_directory: Option<PathBuf>,

    /// Window width
    pub window_width: u32,

    /// Window height
    pub window_height: u32,

    /// Window maximized state
    pub window_maximized: bool,

    /// Application theme
    #[serde(default)]
    pub theme: crate::theme::ThemeVariant,
}

// Default implementations
impl Default for EmulationConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            region: Region::NTSC,
            rewind_enabled: false,
            rewind_buffer_size: 600, // 10 seconds at 60 FPS
        }
    }
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            scaling_mode: ScalingMode::PixelPerfect,
            vsync: true,
            crt_shader: false,
            crt_preset: CrtPreset::Subtle,
            overscan: OverscanConfig {
                top: 8,
                bottom: 8,
                left: 0,
                right: 0,
            },
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_rate: 48000,
            volume: 0.7,
            buffer_size: 1024,
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            keyboard_p1: KeyboardMapping {
                up: "Up".to_string(),
                down: "Down".to_string(),
                left: "Left".to_string(),
                right: "Right".to_string(),
                a: "X".to_string(),
                b: "Z".to_string(),
                select: "RShift".to_string(),
                start: "Return".to_string(),
            },
            keyboard_p2: KeyboardMapping {
                up: "W".to_string(),
                down: "S".to_string(),
                left: "A".to_string(),
                right: "D".to_string(),
                a: "G".to_string(),
                b: "F".to_string(),
                select: "Q".to_string(),
                start: "E".to_string(),
            },
            gamepad_deadzone: 0.2,
        }
    }
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            recent_roms: Vec::new(),
            rom_directory: None,
            window_width: 1024,
            window_height: 720,
            window_maximized: false,
            theme: crate::theme::ThemeVariant::default(),
        }
    }
}

// Persistence implementation
impl AppConfig {
    /// Get platform-specific config file path
    pub fn config_path() -> PathBuf {
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "rustynes", "RustyNES") {
            proj_dirs.config_dir().join("config.toml")
        } else {
            PathBuf::from("config.toml")
        }
    }

    /// Load configuration from disk
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path();

        if !path.exists() {
            // Create default config
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let contents = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&contents)?;

        // Validate loaded config
        config.validate()?;

        Ok(config)
    }

    /// Save configuration to disk
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let toml = toml::to_string_pretty(self)?;
        std::fs::write(&path, toml)?;

        Ok(())
    }

    /// Validate configuration values
    fn validate(&self) -> Result<(), ConfigError> {
        // Speed must be positive
        if self.emulation.speed <= 0.0 {
            return Err(ConfigError::InvalidValue(
                "Emulation speed must be positive".to_string(),
            ));
        }

        // Volume must be 0.0-1.0
        if !(0.0..=1.0).contains(&self.audio.volume) {
            return Err(ConfigError::InvalidValue(
                "Audio volume must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Gamepad deadzone must be 0.0-1.0
        if !(0.0..=1.0).contains(&self.input.gamepad_deadzone) {
            return Err(ConfigError::InvalidValue(
                "Gamepad deadzone must be between 0.0 and 1.0".to_string(),
            ));
        }

        Ok(())
    }

    /// Add ROM to recent list (deduplicates and limits to 10 entries)
    #[allow(dead_code)] // Will be used for recent ROMs functionality
    pub fn add_recent_rom(&mut self, path: PathBuf) {
        // Remove if already exists (move to front)
        self.app.recent_roms.retain(|p| p != &path);

        // Add to front
        self.app.recent_roms.insert(0, path);

        // Limit to 10 entries
        self.app.recent_roms.truncate(10);
    }

    /// Clear recent ROMs list
    pub fn clear_recent_roms(&mut self) {
        self.app.recent_roms.clear();
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parse error
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),

    /// TOML serialize error
    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Invalid configuration value
    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.emulation.speed, 1.0);
        assert_eq!(config.emulation.region, Region::NTSC);
        assert!(config.audio.enabled);
        assert_eq!(config.audio.volume, 0.7);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_config_serialization() {
        let config = AppConfig::default();
        let toml = toml::to_string(&config).unwrap();
        let deserialized: AppConfig = toml::from_str(&toml).unwrap();

        assert_eq!(config.emulation.speed, deserialized.emulation.speed);
        assert_eq!(
            config.video.scaling_mode as u8,
            deserialized.video.scaling_mode as u8
        );
    }

    #[test]
    fn test_validation() {
        let mut config = AppConfig::default();

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid speed
        config.emulation.speed = -1.0;
        assert!(config.validate().is_err());
        config.emulation.speed = 1.0;

        // Invalid volume
        config.audio.volume = 1.5;
        assert!(config.validate().is_err());
        config.audio.volume = 0.7;

        // Invalid deadzone
        config.input.gamepad_deadzone = 2.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_recent_roms() {
        let mut config = AppConfig::default();

        // Add first ROM
        let rom1 = PathBuf::from("/path/to/rom1.nes");
        config.add_recent_rom(rom1.clone());
        assert_eq!(config.app.recent_roms.len(), 1);
        assert_eq!(config.app.recent_roms[0], rom1);

        // Add second ROM
        let rom2 = PathBuf::from("/path/to/rom2.nes");
        config.add_recent_rom(rom2.clone());
        assert_eq!(config.app.recent_roms.len(), 2);
        assert_eq!(config.app.recent_roms[0], rom2); // Most recent first

        // Add duplicate (should move to front)
        config.add_recent_rom(rom1.clone());
        assert_eq!(config.app.recent_roms.len(), 2);
        assert_eq!(config.app.recent_roms[0], rom1);

        // Clear list
        config.clear_recent_roms();
        assert_eq!(config.app.recent_roms.len(), 0);
    }

    #[test]
    fn test_recent_roms_limit() {
        let mut config = AppConfig::default();

        // Add 12 ROMs
        for i in 0..12 {
            config.add_recent_rom(PathBuf::from(format!("/path/to/rom{i}.nes")));
        }

        // Should be limited to 10
        assert_eq!(config.app.recent_roms.len(), 10);

        // Most recent should be rom11
        assert_eq!(
            config.app.recent_roms[0],
            PathBuf::from("/path/to/rom11.nes")
        );
    }
}
