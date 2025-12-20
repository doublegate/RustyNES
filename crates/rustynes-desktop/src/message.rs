//! Application messages following the Elm architecture pattern.
//!
//! All user interactions and asynchronous results are represented as messages
//! that flow through the `update()` function to modify application state.

use std::path::PathBuf;

use crate::config::{CrtPreset, Region, ScalingMode};
use crate::view::{SettingsTab, View};

/// All application messages (events)
#[derive(Debug, Clone)]
#[allow(dead_code)] // Message variants will be used in future sprints
pub enum Message {
    /// No-op message (used when no action is needed)
    None,

    /// Navigate to a different view
    NavigateTo(View),

    /// Open file dialog for ROM selection
    OpenFileDialog,

    /// Load ROM from path (triggered after file dialog)
    LoadRom(PathBuf),

    /// ROM loading completed (success with ROM data, or error message)
    /// Note: ROM data is Vec<u8> to maintain Send trait for Message
    RomLoaded(Result<Vec<u8>, String>),

    /// Change viewport scaling mode
    SetScalingMode(ScalingMode),

    /// Exit application
    Exit,

    // Input messages
    /// Keyboard key pressed
    KeyPressed(iced::keyboard::Key),

    /// Keyboard key released
    KeyReleased(iced::keyboard::Key),

    /// Poll gamepads for input
    PollGamepads,

    // Library messages
    /// Search ROMs in library
    LibrarySearch(String),

    /// Toggle between grid and list view
    ToggleLibraryView,

    /// Open directory picker for ROM library
    SelectRomDirectory,

    /// ROM directory selected from picker
    RomDirectorySelected(Option<PathBuf>),

    // Settings UI
    /// Open settings window
    OpenSettings,

    /// Close settings window
    CloseSettings,

    /// Select settings tab
    SelectSettingsTab(SettingsTab),

    /// Reset all settings to defaults
    ResetSettingsToDefaults,

    // Emulation settings
    /// Update emulation speed
    UpdateEmulationSpeed(f32),

    /// Update region
    UpdateRegion(Region),

    /// Toggle rewind
    ToggleRewind(bool),

    /// Update rewind buffer size
    UpdateRewindBufferSize(usize),

    // Video settings
    /// Update scaling mode
    UpdateScalingMode(ScalingMode),

    /// Toggle VSync
    ToggleVSync(bool),

    /// Toggle CRT shader
    ToggleCrtShader(bool),

    /// Update CRT preset
    UpdateCrtPreset(CrtPreset),

    /// Update overscan top
    UpdateOverscanTop(u32),

    /// Update overscan bottom
    UpdateOverscanBottom(u32),

    /// Update overscan left
    UpdateOverscanLeft(u32),

    /// Update overscan right
    UpdateOverscanRight(u32),

    // Audio settings
    /// Toggle audio
    ToggleAudio(bool),

    /// Update sample rate
    UpdateSampleRate(u32),

    /// Update volume
    UpdateVolume(f32),

    /// Update buffer size
    UpdateBufferSize(u32),

    // Input settings
    /// Update gamepad deadzone
    UpdateGamepadDeadzone(f32),

    /// Remap key (player, button name)
    RemapKey { player: u8, button: String },

    // Persistence
    /// Save configuration to disk
    SaveConfig,

    /// Configuration saved (result)
    ConfigSaved(Result<(), String>),

    /// Load configuration from disk
    LoadConfig,

    /// Configuration loaded (result)
    ConfigLoaded(Result<(), String>),

    // Recent ROMs
    /// Load recent ROM by index (0-9)
    LoadRecentRom(usize),

    /// Clear recent ROMs list
    ClearRecentRoms,

    // About dialog
    /// Show about dialog
    ShowAbout,

    /// Close about dialog
    CloseAbout,

    /// Open URL in browser
    OpenUrl(String),

    // Window events
    /// Window resized (width, height)
    WindowResized(f32, f32),

    // Theme
    /// Update theme
    UpdateTheme(crate::theme::ThemeVariant),

    // Metrics
    /// Toggle performance metrics overlay (F3)
    ToggleMetrics,

    // Emulation
    /// Tick emulation (run one frame at ~60Hz)
    Tick,
}
