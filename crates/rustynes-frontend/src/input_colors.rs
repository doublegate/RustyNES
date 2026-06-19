//! v1.5.0 "Lens" Workstream I5 — the shared "lit button" colour palette for the
//! input visualizer.
//!
//! The consolidated **Input Display** panel
//! (`debugger/input_miniatures_panel.rs`; v1.7.0 "Forge" beta.5 #51 merged the
//! former standalone Input Display + Input Miniatures panels into it) draws a
//! held NES button in a colour keyed by which button group it belongs to. Having
//! the constants in one module keeps the palette consistent across the standard
//! pads + any expansion devices (the maintainer asked for the same palette):
//!
//! - **D-pad** → green (the original lit colour).
//! - **Select / Start** → yellow.
//! - **B / A** → the classic Nintendo red, `#E60012`.
//!
//! Frontend-only, output-only: these are pure display tints with no core or
//! determinism surface.

use egui::Color32;

/// D-pad (Up/Down/Left/Right) lit fill — green.
pub const LIT_DPAD: Color32 = Color32::from_rgb(0x46, 0xC0, 0x50);

/// Select / Start lit fill — yellow.
pub const LIT_STARTSEL: Color32 = Color32::from_rgb(0xF2, 0xC8, 0x3C);

/// B / A lit fill — Nintendo red (`#E60012`).
pub const LIT_AB: Color32 = Color32::from_rgb(0xE6, 0x00, 0x12);
