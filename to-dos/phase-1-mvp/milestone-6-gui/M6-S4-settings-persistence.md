# Sprint M6-S4: Settings & Persistence

**Status:** ⏳ PENDING
**Sprint:** 6.4 (Settings & Persistence)
**Milestone:** M6 (Desktop GUI - Iced Hybrid)
**Estimated Duration:** ~1 week (40 hours)
**Target Files:** `crates/rustynes-desktop/src/config.rs`, `ui/settings.rs`, `persistence.rs`

---

## Overview

Sprint M6-S4 implements the **Settings UI and persistence system** for RustyNES. This sprint delivers a professional, tabbed Settings window built with Iced's Elm architecture, TOML-based configuration storage with automatic persistence, and comprehensive error handling. All user preferences persist across sessions, from emulation parameters to window geometry.

**Goals:**

1. Implement TOML-based configuration file with automatic save/load
2. Create tabbed Settings window using Iced widgets
3. Build Recent ROMs list with quick access (Ctrl+1-9)
4. Implement About dialog and error handling system
5. Add configuration validation and fallback defaults
6. Set up cross-platform configuration directories

**Dependencies:**

- M6-S1 (Iced application structure)
- M6-S2 (wgpu rendering backend - applies video settings)
- M6-S3 (Input + Library - applies input mappings)

---

## Architecture Context

### Iced Integration Pattern

The Settings system integrates with Iced's Elm architecture using:

1. **Model:** `AppConfig` struct (serializable to TOML)
2. **Update:** `Message::ApplySetting(SettingChange)` messages
3. **View:** `settings_window()` pure function rendering UI
4. **Command:** `Command::perform()` for async save/load

**Message Flow:**

```rust
User clicks setting
    ↓
SettingChange message dispatched
    ↓
Update function modifies AppConfig
    ↓
Command schedules async save to disk
    ↓
View re-renders with new config
```

---

## Tasks

### Task 1: Configuration Data Structures

**Duration:** ~4 hours

Implement comprehensive configuration data model with serde serialization.

#### 1.1 Core Config Structure

**File:** `crates/rustynes-desktop/src/config.rs`

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration (TOML serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Region {
    NTSC, // 60.0988 Hz
    PAL,  // 50.0070 Hz
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::NTSC => write!(f, "NTSC"),
            Region::PAL => write!(f, "PAL"),
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ScalingMode {
    AspectRatio4x3,
    PixelPerfect,
    IntegerScaling,
    Stretch,
}

impl std::fmt::Display for ScalingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScalingMode::AspectRatio4x3 => write!(f, "4:3 Aspect Ratio"),
            ScalingMode::PixelPerfect => write!(f, "Pixel Perfect"),
            ScalingMode::IntegerScaling => write!(f, "Integer Scaling"),
            ScalingMode::Stretch => write!(f, "Stretch to Fill"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CrtPreset {
    None,
    Subtle,
    Moderate,
    Authentic,
    Custom,
}

impl std::fmt::Display for CrtPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrtPreset::None => write!(f, "None"),
            CrtPreset::Subtle => write!(f, "Subtle"),
            CrtPreset::Moderate => write!(f, "Moderate"),
            CrtPreset::Authentic => write!(f, "Authentic"),
            CrtPreset::Custom => write!(f, "Custom"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverscanConfig {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Player 1 keyboard mapping
    pub keyboard_p1: KeyboardMapping,

    /// Player 2 keyboard mapping
    pub keyboard_p2: KeyboardMapping,

    /// Gamepad analog deadzone (0.0-1.0)
    pub gamepad_deadzone: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardMapping {
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub a: String,
    pub b: String,
    pub select: String,
    pub start: String,
}

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

    /// Window maximized
    pub window_maximized: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            emulation: EmulationConfig {
                speed: 1.0,
                region: Region::NTSC,
                rewind_enabled: false,
                rewind_buffer_size: 600, // 10 seconds at 60 FPS
            },
            video: VideoConfig {
                scaling_mode: ScalingMode::AspectRatio4x3,
                vsync: true,
                crt_shader: false,
                crt_preset: CrtPreset::Subtle,
                overscan: OverscanConfig {
                    top: 8,
                    bottom: 8,
                    left: 0,
                    right: 0,
                },
            },
            audio: AudioConfig {
                enabled: true,
                sample_rate: 48000,
                volume: 0.7,
                buffer_size: 1024,
            },
            input: InputConfig {
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
            },
            app: ApplicationConfig {
                recent_roms: Vec::new(),
                rom_directory: None,
                window_width: 1024,
                window_height: 720,
                window_maximized: false,
            },
        }
    }
}
```

#### 1.2 Persistence Functions

```rust
use std::path::Path;

impl AppConfig {
    /// Get platform-specific config file path
    pub fn config_path() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            config_dir.join("rustynes").join("config.toml")
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
        let config: AppConfig = toml::from_str(&contents)?;

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
                "Emulation speed must be positive".to_string()
            ));
        }

        // Volume must be 0.0-1.0
        if !(0.0..=1.0).contains(&self.audio.volume) {
            return Err(ConfigError::InvalidValue(
                "Audio volume must be between 0.0 and 1.0".to_string()
            ));
        }

        // Gamepad deadzone must be 0.0-1.0
        if !(0.0..=1.0).contains(&self.input.gamepad_deadzone) {
            return Err(ConfigError::InvalidValue(
                "Gamepad deadzone must be between 0.0 and 1.0".to_string()
            ));
        }

        Ok(())
    }

    /// Add ROM to recent list
    pub fn add_recent_rom(&mut self, path: PathBuf) {
        // Remove if already exists
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

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),
}
```

---

### Task 2: Settings Window UI (Iced)

**Duration:** ~12 hours

Create comprehensive Settings window using Iced's widget system.

#### 2.1 Settings Window State

**File:** `crates/rustynes-desktop/src/ui/settings.rs`

```rust
use iced::{
    widget::{button, checkbox, column, container, pick_list, row, slider, text, Column, Row},
    Element, Length,
};
use crate::config::*;
use crate::Message;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    Emulation,
    Video,
    Audio,
    Input,
}

impl std::fmt::Display for SettingsTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsTab::Emulation => write!(f, "Emulation"),
            SettingsTab::Video => write!(f, "Video"),
            SettingsTab::Audio => write!(f, "Audio"),
            SettingsTab::Input => write!(f, "Input"),
        }
    }
}

pub struct SettingsState {
    pub selected_tab: SettingsTab,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            selected_tab: SettingsTab::Emulation,
        }
    }
}
```

#### 2.2 Settings Window View

```rust
pub fn settings_window<'a>(
    config: &'a AppConfig,
    state: &'a SettingsState,
) -> Element<'a, Message> {
    let tabs = row![
        tab_button(&state.selected_tab, SettingsTab::Emulation),
        tab_button(&state.selected_tab, SettingsTab::Video),
        tab_button(&state.selected_tab, SettingsTab::Audio),
        tab_button(&state.selected_tab, SettingsTab::Input),
    ]
    .spacing(10);

    let content = match state.selected_tab {
        SettingsTab::Emulation => emulation_settings(config),
        SettingsTab::Video => video_settings(config),
        SettingsTab::Audio => audio_settings(config),
        SettingsTab::Input => input_settings(config),
    };

    let buttons = row![
        button("Reset to Defaults")
            .on_press(Message::ResetSettingsToDefaults),
        iced::widget::Space::with_width(Length::Fill),
        button("Close")
            .on_press(Message::CloseSettings),
    ]
    .spacing(10)
    .padding(10);

    column![
        tabs,
        iced::widget::horizontal_rule(1),
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20),
        iced::widget::horizontal_rule(1),
        buttons,
    ]
    .width(Length::Fixed(600.0))
    .height(Length::Fixed(500.0))
    .into()
}

fn tab_button<'a>(
    selected: &SettingsTab,
    tab: SettingsTab,
) -> iced::widget::Button<'a, Message> {
    let style = if *selected == tab {
        iced::widget::button::primary
    } else {
        iced::widget::button::secondary
    };

    button(text(tab.to_string()))
        .style(style)
        .on_press(Message::SelectSettingsTab(tab))
}
```

#### 2.3 Emulation Settings Tab

```rust
fn emulation_settings<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    column![
        text("Emulation Settings").size(20),
        iced::widget::vertical_space(20),

        // Speed
        row![
            text("Speed:").width(Length::Fixed(150.0)),
            slider(0.25..=3.0, config.emulation.speed, |v| {
                Message::UpdateEmulationSpeed(v)
            })
            .step(0.25),
            text(format!("{:.2}x", config.emulation.speed))
                .width(Length::Fixed(60.0)),
        ]
        .spacing(10),

        // Region
        row![
            text("Region:").width(Length::Fixed(150.0)),
            pick_list(
                &[Region::NTSC, Region::PAL][..],
                Some(config.emulation.region),
                Message::UpdateRegion
            ),
        ]
        .spacing(10),

        iced::widget::vertical_space(20),

        // Rewind
        checkbox(
            "Enable Rewind",
            config.emulation.rewind_enabled,
            Message::ToggleRewind
        ),

        // Rewind buffer size (only if enabled)
        if config.emulation.rewind_enabled {
            column![
                row![
                    text("Buffer Size:").width(Length::Fixed(150.0)),
                    slider(60..=3600, config.emulation.rewind_buffer_size as f32, |v| {
                        Message::UpdateRewindBufferSize(v as usize)
                    })
                    .step(60.0),
                    text(format!("{:.1}s", config.emulation.rewind_buffer_size as f32 / 60.0))
                        .width(Length::Fixed(60.0)),
                ]
                .spacing(10),
            ]
            .into()
        } else {
            column![].into()
        },
    ]
    .spacing(15)
    .into()
}
```

#### 2.4 Video Settings Tab

```rust
fn video_settings<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    column![
        text("Video Settings").size(20),
        iced::widget::vertical_space(20),

        // Scaling mode
        row![
            text("Scaling Mode:").width(Length::Fixed(150.0)),
            pick_list(
                &[
                    ScalingMode::AspectRatio4x3,
                    ScalingMode::PixelPerfect,
                    ScalingMode::IntegerScaling,
                    ScalingMode::Stretch,
                ][..],
                Some(config.video.scaling_mode),
                Message::UpdateScalingMode
            ),
        ]
        .spacing(10),

        // VSync
        checkbox(
            "VSync",
            config.video.vsync,
            Message::ToggleVSync
        ),

        iced::widget::vertical_space(20),

        // CRT Shader
        checkbox(
            "CRT Shader",
            config.video.crt_shader,
            Message::ToggleCrtShader
        ),

        // CRT preset (only if shader enabled)
        if config.video.crt_shader {
            row![
                text("CRT Preset:").width(Length::Fixed(150.0)),
                pick_list(
                    &[
                        CrtPreset::Subtle,
                        CrtPreset::Moderate,
                        CrtPreset::Authentic,
                    ][..],
                    Some(config.video.crt_preset),
                    Message::UpdateCrtPreset
                ),
            ]
            .spacing(10)
            .into()
        } else {
            row![].into()
        },

        iced::widget::vertical_space(20),
        text("Overscan Cropping").size(16),

        // Overscan sliders
        row![
            text("Top:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.top as f32, |v| {
                Message::UpdateOverscanTop(v as u32)
            })
            .step(1.0),
            text(format!("{}px", config.video.overscan.top))
                .width(Length::Fixed(50.0)),
        ]
        .spacing(10),

        row![
            text("Bottom:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.bottom as f32, |v| {
                Message::UpdateOverscanBottom(v as u32)
            })
            .step(1.0),
            text(format!("{}px", config.video.overscan.bottom))
                .width(Length::Fixed(50.0)),
        ]
        .spacing(10),

        row![
            text("Left:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.left as f32, |v| {
                Message::UpdateOverscanLeft(v as u32)
            })
            .step(1.0),
            text(format!("{}px", config.video.overscan.left))
                .width(Length::Fixed(50.0)),
        ]
        .spacing(10),

        row![
            text("Right:").width(Length::Fixed(80.0)),
            slider(0..=16, config.video.overscan.right as f32, |v| {
                Message::UpdateOverscanRight(v as u32)
            })
            .step(1.0),
            text(format!("{}px", config.video.overscan.right))
                .width(Length::Fixed(50.0)),
        ]
        .spacing(10),
    ]
    .spacing(15)
    .into()
}
```

#### 2.5 Audio Settings Tab

```rust
fn audio_settings<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    column![
        text("Audio Settings").size(20),
        iced::widget::vertical_space(20),

        // Audio enabled
        checkbox(
            "Audio Output",
            config.audio.enabled,
            Message::ToggleAudio
        ),

        // Sample rate
        row![
            text("Sample Rate:").width(Length::Fixed(150.0)),
            pick_list(
                &[44100, 48000, 96000][..],
                Some(config.audio.sample_rate),
                Message::UpdateSampleRate
            ),
            text("Hz"),
        ]
        .spacing(10),

        // Volume
        row![
            text("Master Volume:").width(Length::Fixed(150.0)),
            slider(0.0..=1.0, config.audio.volume, Message::UpdateVolume)
                .step(0.01),
            text(format!("{:.0}%", config.audio.volume * 100.0))
                .width(Length::Fixed(60.0)),
        ]
        .spacing(10),

        // Buffer size
        row![
            text("Buffer Size:").width(Length::Fixed(150.0)),
            pick_list(
                &[512, 1024, 2048, 4096][..],
                Some(config.audio.buffer_size),
                Message::UpdateBufferSize
            ),
            text("samples"),
        ]
        .spacing(10),

        iced::widget::vertical_space(20),
        text("Lower buffer size = lower latency, higher CPU usage")
            .size(12)
            .style(iced::theme::Text::Color(iced::Color::from_rgb(0.6, 0.6, 0.6))),
    ]
    .spacing(15)
    .into()
}
```

#### 2.6 Input Settings Tab

```rust
fn input_settings<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    column![
        text("Input Settings").size(20),
        iced::widget::vertical_space(20),

        text("Player 1 Keyboard").size(16),
        key_mapping_grid(&config.input.keyboard_p1, 1),

        iced::widget::vertical_space(20),
        text("Player 2 Keyboard").size(16),
        key_mapping_grid(&config.input.keyboard_p2, 2),

        iced::widget::vertical_space(20),
        text("Gamepad").size(16),

        row![
            text("Analog Deadzone:").width(Length::Fixed(150.0)),
            slider(0.0..=0.5, config.input.gamepad_deadzone, |v| {
                Message::UpdateGamepadDeadzone(v)
            })
            .step(0.05),
            text(format!("{:.2}", config.input.gamepad_deadzone))
                .width(Length::Fixed(60.0)),
        ]
        .spacing(10),
    ]
    .spacing(15)
    .into()
}

fn key_mapping_grid<'a>(mapping: &'a KeyboardMapping, player: u8) -> Element<'a, Message> {
    column![
        key_mapping_row("Up", &mapping.up, player, "up"),
        key_mapping_row("Down", &mapping.down, player, "down"),
        key_mapping_row("Left", &mapping.left, player, "left"),
        key_mapping_row("Right", &mapping.right, player, "right"),
        key_mapping_row("A", &mapping.a, player, "a"),
        key_mapping_row("B", &mapping.b, player, "b"),
        key_mapping_row("Select", &mapping.select, player, "select"),
        key_mapping_row("Start", &mapping.start, player, "start"),
    ]
    .spacing(8)
    .into()
}

fn key_mapping_row<'a>(
    label: &'a str,
    current_key: &'a str,
    player: u8,
    button: &'static str,
) -> Element<'a, Message> {
    row![
        text(format!("{}:", label)).width(Length::Fixed(80.0)),
        button(text(current_key))
            .on_press(Message::RemapKey { player, button: button.to_string() })
            .width(Length::Fixed(120.0)),
    ]
    .spacing(10)
    .into()
}
```

---

### Task 3: Message Handling & Integration

**Duration:** ~8 hours

Implement message handlers for settings updates and persistence.

#### 3.1 Settings Messages

**File:** `crates/rustynes-desktop/src/lib.rs`

```rust
#[derive(Debug, Clone)]
pub enum Message {
    // ... existing messages ...

    // Settings UI
    OpenSettings,
    CloseSettings,
    SelectSettingsTab(SettingsTab),
    ResetSettingsToDefaults,

    // Emulation settings
    UpdateEmulationSpeed(f32),
    UpdateRegion(Region),
    ToggleRewind(bool),
    UpdateRewindBufferSize(usize),

    // Video settings
    UpdateScalingMode(ScalingMode),
    ToggleVSync(bool),
    ToggleCrtShader(bool),
    UpdateCrtPreset(CrtPreset),
    UpdateOverscanTop(u32),
    UpdateOverscanBottom(u32),
    UpdateOverscanLeft(u32),
    UpdateOverscanRight(u32),

    // Audio settings
    ToggleAudio(bool),
    UpdateSampleRate(u32),
    UpdateVolume(f32),
    UpdateBufferSize(u32),

    // Input settings
    UpdateGamepadDeadzone(f32),
    RemapKey { player: u8, button: String },

    // Persistence
    SaveConfig,
    ConfigSaved(Result<(), String>),
    LoadConfig,
    ConfigLoaded(Result<AppConfig, String>),
}
```

#### 3.2 Update Function

```rust
impl RustyNes {
    pub fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            // Settings window
            Message::OpenSettings => {
                self.show_settings = true;
                Command::none()
            }
            Message::CloseSettings => {
                self.show_settings = false;
                // Auto-save on close
                Command::perform(
                    save_config(self.config.clone()),
                    Message::ConfigSaved
                )
            }
            Message::SelectSettingsTab(tab) => {
                self.settings_state.selected_tab = tab;
                Command::none()
            }
            Message::ResetSettingsToDefaults => {
                self.config = AppConfig::default();
                Command::perform(
                    save_config(self.config.clone()),
                    Message::ConfigSaved
                )
            }

            // Emulation settings
            Message::UpdateEmulationSpeed(speed) => {
                self.config.emulation.speed = speed;
                Command::none()
            }
            Message::UpdateRegion(region) => {
                self.config.emulation.region = region;
                Command::none()
            }
            Message::ToggleRewind(enabled) => {
                self.config.emulation.rewind_enabled = enabled;
                Command::none()
            }
            Message::UpdateRewindBufferSize(size) => {
                self.config.emulation.rewind_buffer_size = size;
                Command::none()
            }

            // Video settings
            Message::UpdateScalingMode(mode) => {
                self.config.video.scaling_mode = mode;
                Command::none()
            }
            Message::ToggleVSync(enabled) => {
                self.config.video.vsync = enabled;
                Command::none()
            }
            Message::ToggleCrtShader(enabled) => {
                self.config.video.crt_shader = enabled;
                Command::none()
            }
            Message::UpdateCrtPreset(preset) => {
                self.config.video.crt_preset = preset;
                Command::none()
            }
            Message::UpdateOverscanTop(value) => {
                self.config.video.overscan.top = value;
                Command::none()
            }
            Message::UpdateOverscanBottom(value) => {
                self.config.video.overscan.bottom = value;
                Command::none()
            }
            Message::UpdateOverscanLeft(value) => {
                self.config.video.overscan.left = value;
                Command::none()
            }
            Message::UpdateOverscanRight(value) => {
                self.config.video.overscan.right = value;
                Command::none()
            }

            // Audio settings
            Message::ToggleAudio(enabled) => {
                self.config.audio.enabled = enabled;
                Command::none()
            }
            Message::UpdateSampleRate(rate) => {
                self.config.audio.sample_rate = rate;
                Command::none()
            }
            Message::UpdateVolume(volume) => {
                self.config.audio.volume = volume;
                Command::none()
            }
            Message::UpdateBufferSize(size) => {
                self.config.audio.buffer_size = size;
                Command::none()
            }

            // Input settings
            Message::UpdateGamepadDeadzone(deadzone) => {
                self.config.input.gamepad_deadzone = deadzone;
                Command::none()
            }
            Message::RemapKey { player, button } => {
                // TODO: Enter key remapping mode
                Command::none()
            }

            // Persistence
            Message::SaveConfig => {
                Command::perform(
                    save_config(self.config.clone()),
                    Message::ConfigSaved
                )
            }
            Message::ConfigSaved(result) => {
                if let Err(e) = result {
                    log::error!("Failed to save config: {}", e);
                }
                Command::none()
            }
            Message::LoadConfig => {
                Command::perform(load_config(), Message::ConfigLoaded)
            }
            Message::ConfigLoaded(result) => {
                match result {
                    Ok(config) => {
                        self.config = config;
                    }
                    Err(e) => {
                        log::error!("Failed to load config: {}", e);
                        self.config = AppConfig::default();
                    }
                }
                Command::none()
            }

            _ => Command::none(),
        }
    }
}
```

#### 3.3 Async Persistence Functions

```rust
async fn save_config(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}

async fn load_config() -> Result<AppConfig, String> {
    AppConfig::load().map_err(|e| e.to_string())
}
```

---

### Task 4: Recent ROMs List

**Duration:** ~4 hours

Implement Recent ROMs menu with keyboard shortcuts.

#### 4.1 Recent ROMs UI

**File:** `crates/rustynes-desktop/src/ui/menu_bar.rs`

```rust
pub fn menu_bar<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    row![
        menu_button("File", file_menu(config)),
        menu_button("Emulation", emulation_menu()),
        menu_button("Help", help_menu()),
    ]
    .spacing(10)
    .padding(5)
    .into()
}

fn file_menu<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    column![
        menu_item("Open ROM...", Message::OpenRomDialog),
        recent_roms_submenu(config),
        iced::widget::horizontal_rule(1),
        menu_item("Exit", Message::Exit),
    ]
    .spacing(5)
    .into()
}

fn recent_roms_submenu<'a>(config: &'a AppConfig) -> Element<'a, Message> {
    if config.app.recent_roms.is_empty() {
        text("Recent ROMs (empty)")
            .size(14)
            .style(iced::theme::Text::Color(iced::Color::from_rgb(0.5, 0.5, 0.5)))
            .into()
    } else {
        let mut items = column![].spacing(5);

        for (i, rom_path) in config.app.recent_roms.iter().enumerate() {
            let label = rom_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");

            let shortcut = if i < 9 {
                format!("  Ctrl+{}", i + 1)
            } else {
                String::new()
            };

            items = items.push(
                button(text(format!("{}{}", label, shortcut)))
                    .on_press(Message::LoadRom(rom_path.clone()))
            );
        }

        items.push(iced::widget::horizontal_rule(1));
        items = items.push(
            button(text("Clear Recent List"))
                .on_press(Message::ClearRecentRoms)
        );

        items.into()
    }
}
```

#### 4.2 Keyboard Shortcuts

```rust
impl RustyNes {
    pub fn subscription(&self) -> Subscription<Message> {
        iced::keyboard::on_key_press(|key, modifiers| {
            use iced::keyboard::{Key, Modifiers};

            // Recent ROMs shortcuts (Ctrl+1 through Ctrl+9)
            if modifiers.control() {
                match key {
                    Key::Character(ref c) if c.as_str() == "1" => Some(Message::LoadRecentRom(0)),
                    Key::Character(ref c) if c.as_str() == "2" => Some(Message::LoadRecentRom(1)),
                    Key::Character(ref c) if c.as_str() == "3" => Some(Message::LoadRecentRom(2)),
                    Key::Character(ref c) if c.as_str() == "4" => Some(Message::LoadRecentRom(3)),
                    Key::Character(ref c) if c.as_str() == "5" => Some(Message::LoadRecentRom(4)),
                    Key::Character(ref c) if c.as_str() == "6" => Some(Message::LoadRecentRom(5)),
                    Key::Character(ref c) if c.as_str() == "7" => Some(Message::LoadRecentRom(6)),
                    Key::Character(ref c) if c.as_str() == "8" => Some(Message::LoadRecentRom(7)),
                    Key::Character(ref c) if c.as_str() == "9" => Some(Message::LoadRecentRom(8)),
                    _ => None,
                }
            } else {
                None
            }
        })
    }

    pub fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::LoadRecentRom(index) => {
                if let Some(path) = self.config.app.recent_roms.get(index).cloned() {
                    Command::perform(
                        load_rom(path.clone()),
                        move |result| Message::RomLoaded(result, path)
                    )
                } else {
                    Command::none()
                }
            }
            Message::ClearRecentRoms => {
                self.config.clear_recent_roms();
                Command::perform(
                    save_config(self.config.clone()),
                    Message::ConfigSaved
                )
            }
            _ => Command::none(),
        }
    }
}
```

---

### Task 5: About Dialog & Error Handling

**Duration:** ~4 hours

Create About dialog and comprehensive error handling.

#### 5.1 About Dialog

**File:** `crates/rustynes-desktop/src/ui/dialogs.rs`

```rust
pub fn about_dialog<'a>() -> Element<'a, Message> {
    container(
        column![
            text("RustyNES").size(32),
            text(format!("Version {}", env!("CARGO_PKG_VERSION"))).size(14),
            iced::widget::vertical_space(20),
            text("A high-accuracy NES emulator written in Rust"),
            iced::widget::vertical_space(20),
            button(text("GitHub"))
                .on_press(Message::OpenUrl("https://github.com/doublegate/RustyNES".to_string())),
            iced::widget::vertical_space(20),
            text("Copyright © 2025").size(12),
            text("Licensed under MIT / Apache-2.0").size(12),
            iced::widget::vertical_space(20),
            button("Close")
                .on_press(Message::CloseAbout),
        ]
        .align_items(iced::Alignment::Center)
        .spacing(10)
        .padding(20)
    )
    .width(Length::Fixed(400.0))
    .height(Length::Fixed(350.0))
    .center_x()
    .center_y()
    .into()
}
```

#### 5.2 Error Dialog

```rust
pub fn error_dialog<'a>(message: &'a str, details: Option<&'a str>) -> Element<'a, Message> {
    let mut content = column![
        row![
            text("⚠").size(32).style(iced::theme::Text::Color(iced::Color::from_rgb(1.0, 0.0, 0.0))),
            iced::widget::Space::with_width(Length::Fixed(20.0)),
            text(message).size(16),
        ]
        .spacing(10),
    ]
    .spacing(15);

    if let Some(details_text) = details {
        content = content.push(
            container(
                text(details_text)
                    .size(12)
                    .style(iced::theme::Text::Color(iced::Color::from_rgb(0.5, 0.5, 0.5)))
            )
            .padding(10)
            .style(iced::theme::Container::Box)
        );
    }

    content = content.push(
        row![
            iced::widget::Space::with_width(Length::Fill),
            button("OK")
                .on_press(Message::CloseError),
        ]
        .spacing(10)
    );

    container(content)
        .width(Length::Fixed(500.0))
        .padding(20)
        .center_x()
        .center_y()
        .into()
}
```

---

### Task 6: Auto-Save & Window Geometry

**Duration:** ~4 hours

Implement auto-save on exit and window geometry persistence.

#### 6.1 Auto-Save on Exit

**File:** `crates/rustynes-desktop/src/lib.rs`

```rust
impl Drop for RustyNes {
    fn drop(&mut self) {
        // Save window geometry
        self.config.app.window_width = self.window_width;
        self.config.app.window_height = self.window_height;
        self.config.app.window_maximized = self.window_maximized;

        // Save configuration on exit
        if let Err(e) = self.config.save() {
            log::error!("Failed to save config on exit: {}", e);
        }
    }
}
```

#### 6.2 Window Geometry Tracking

```rust
impl RustyNes {
    pub fn new() -> (Self, Command<Message>) {
        // Load config
        let config = AppConfig::load().unwrap_or_default();

        let app = Self {
            config,
            window_width: config.app.window_width,
            window_height: config.app.window_height,
            window_maximized: config.app.window_maximized,
            // ... other fields ...
        };

        (app, Command::none())
    }

    pub fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::WindowResized(width, height) => {
                self.window_width = width;
                self.window_height = height;
                Command::none()
            }
            Message::WindowMaximized(maximized) => {
                self.window_maximized = maximized;
                Command::none()
            }
            _ => Command::none(),
        }
    }
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Configuration file loads/saves at `~/.config/rustynes/config.toml` (Linux) or equivalent
- [ ] Settings window displays all configuration options
- [ ] All settings persist across application restarts
- [ ] Recent ROMs list shows last 10 opened files
- [ ] Recent ROMs accessible via Ctrl+1 through Ctrl+9
- [ ] About dialog displays version, license, and credits
- [ ] Error dialogs show user-friendly messages with technical details
- [ ] Window geometry persists (size and maximized state)
- [ ] Settings changes apply immediately (no restart required)

### User Experience

- [ ] Settings window has intuitive tabbed layout
- [ ] All controls have clear labels
- [ ] Reset to Defaults button works for all settings
- [ ] Recent ROMs show filename only (not full path)
- [ ] Error dialogs are modal and non-intrusive
- [ ] Configuration file is human-readable TOML

### Quality

- [ ] No crashes on invalid configuration files (fallback to defaults)
- [ ] Configuration validation prevents invalid values
- [ ] Settings window closes when pressing Escape
- [ ] All keyboard shortcuts documented in UI
- [ ] Platform-specific paths used correctly (XDG on Linux, AppData on Windows, etc.)
- [ ] Zero unsafe code in configuration/UI modules
- [ ] Zero clippy warnings (`clippy::pedantic`)

---

## Dependencies

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies]
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
dirs = "5.0"       # Cross-platform config directories
thiserror = "1.0"
```

---

## Related Documentation

- [M6-S1-iced-application.md](M6-S1-iced-application.md) - Iced application structure
- [M6-S2-wgpu-rendering.md](M6-S2-wgpu-rendering.md) - Video settings integration
- [M6-S3-input-library.md](M6-S3-input-library.md) - Input settings integration

---

## Technical Notes

### Configuration File Locations

**Platform-specific paths** (via `dirs` crate):

- **Linux**: `~/.config/rustynes/config.toml`
- **macOS**: `~/Library/Application Support/rustynes/config.toml`
- **Windows**: `%APPDATA%\rustynes\config.toml`

**Fallback**: If `dirs::config_dir()` fails, use `./config.toml` in current directory.

### Settings Persistence Strategy

1. **Auto-save on exit**: `Drop` trait implementation
2. **Auto-save on settings close**: Explicit save command
3. **Immediate save**: After opening ROM (updates recent list)

### TOML Validation

Use `serde` default values and `#[serde(default)]` to handle missing keys gracefully. If parsing fails entirely, log error and use `AppConfig::default()`.

### Recent ROMs List

- **Max size**: 10 entries
- **Deduplication**: Remove existing entry before adding to front
- **Display**: Show filename only (use `Path::file_name()`)
- **Keyboard shortcuts**: Ctrl+1 through Ctrl+9 (limit to 9 entries)

---

## Performance Targets

- **Config load time**: <10ms
- **Config save time**: <50ms
- **Settings window render**: 60 FPS
- **Recent ROMs menu**: <1ms to populate

---

## Success Criteria

1. Configuration system tested on Linux, Windows, and macOS
2. All settings persist correctly across restarts
3. Recent ROMs list functions with keyboard shortcuts
4. Settings window has no UI glitches or rendering issues
5. Error dialogs tested with various error scenarios
6. Window geometry persists correctly
7. Zero clippy warnings (`clippy::pedantic`)
8. All acceptance criteria met
9. M6-S4 sprint marked as ✅ COMPLETE

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1, M6-S2, M6-S3
**Next Sprint:** [M6-S5 Polish & Basic Run-Ahead](M6-S5-polish-runahead.md)
