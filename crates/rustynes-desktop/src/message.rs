//! Application messages following the Elm architecture pattern.
//!
//! All user interactions and asynchronous results are represented as messages
//! that flow through the `update()` function to modify application state.

use std::path::PathBuf;

use crate::view::View;
use crate::viewport::ScalingMode;

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
}
