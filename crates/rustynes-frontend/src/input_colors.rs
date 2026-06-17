//! v1.5.0 "Lens" Workstream I5 — the shared "lit button" colour palette for the
//! input visualizers.
//!
//! Both the **Input Display** panel (`debugger/input_display_panel.rs`) and the
//! A1 **Input Miniatures** overlay (`debugger/input_miniatures_panel.rs`) draw a
//! held NES button in a colour keyed by which button group it belongs to. Having
//! the constants in one module is what keeps the two overlays from drifting
//! apart (the maintainer asked for the same palette in both):
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
