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
    // Future views (to be implemented in later sprints):
    // Settings(SettingsTab),
}
