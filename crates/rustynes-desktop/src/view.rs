//! All possible application views (screens).
//!
//! This enum represents the navigation state of the application.
//! Each variant corresponds to a different screen the user can see.

/// All possible application views
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Welcome screen (no ROM loaded)
    #[allow(dead_code)] // Future: welcome screen will be shown on first launch
    Welcome,

    /// ROM library browser
    Library,

    /// Active gameplay screen
    Playing,

    /// Settings panel with selected tab
    Settings(SettingsTab),
}

/// Settings tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// Emulation settings
    Emulation,
    /// Video settings
    Video,
    /// Audio settings
    Audio,
    /// Input settings
    Input,
}

impl std::fmt::Display for SettingsTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Emulation => write!(f, "Emulation"),
            Self::Video => write!(f, "Video"),
            Self::Audio => write!(f, "Audio"),
            Self::Input => write!(f, "Input"),
        }
    }
}
