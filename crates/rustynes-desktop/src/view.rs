//! All possible application views (screens).
//!
//! This enum represents the navigation state of the application.
//! Each variant corresponds to a different screen the user can see.

/// All possible application views
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// Welcome screen (no ROM loaded)
    Welcome,

    /// Active gameplay screen
    Playing,
    // Future views (to be implemented in later sprints):
    // Library,
    // Settings(SettingsTab),
}
