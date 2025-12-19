//! Custom `RustyNES` theme system with "Nostalgic Futurism" design.
//!
//! Colors follow the design specification from `RustyNES-UI_UX-Design-v2.md`:
//! - Console Black (#1A1A2E) - Primary background
//! - Deep Navy (#16213E) - Secondary background
//! - NES Blue (#0F3460) - Accent color
//! - Power Red (#E94560) - Primary action color
//! - Coral Accent (#FF6B6B) - Secondary action color

use iced::Color;

/// Custom `RustyNES` theme palette
#[derive(Debug, Clone)]
#[allow(dead_code)] // Theme colors will be used in future UI components
pub struct RustyTheme {
    pub console_black: Color,
    pub deep_navy: Color,
    pub nes_blue: Color,
    pub power_red: Color,
    pub coral_accent: Color,
}

impl RustyTheme {
    /// Dark theme (default for "Nostalgic Futurism")
    pub fn dark() -> Self {
        Self {
            // #1A1A2E - Console Black
            console_black: Color::from_rgb(
                0x1A as f32 / 255.0,
                0x1A as f32 / 255.0,
                0x2E as f32 / 255.0,
            ),
            // #16213E - Deep Navy
            deep_navy: Color::from_rgb(
                0x16 as f32 / 255.0,
                0x21 as f32 / 255.0,
                0x3E as f32 / 255.0,
            ),
            // #0F3460 - NES Blue
            nes_blue: Color::from_rgb(
                0x0F as f32 / 255.0,
                0x34 as f32 / 255.0,
                0x60 as f32 / 255.0,
            ),
            // #E94560 - Power Red
            power_red: Color::from_rgb(
                0xE9 as f32 / 255.0,
                0x45 as f32 / 255.0,
                0x60 as f32 / 255.0,
            ),
            // #FF6B6B - Coral Accent
            coral_accent: Color::from_rgb(
                0xFF as f32 / 255.0,
                0x6B as f32 / 255.0,
                0x6B as f32 / 255.0,
            ),
        }
    }

    /// Glass morphism background color
    /// rgba(26, 26, 46, 0.7) with blur(20px) saturate(180%)
    #[allow(dead_code)] // Will be used for overlays and modals
    pub fn glass_background() -> Color {
        Color::from_rgba(
            0x1A as f32 / 255.0,
            0x1A as f32 / 255.0,
            0x2E as f32 / 255.0,
            0.7,
        )
    }
}

impl Default for RustyTheme {
    fn default() -> Self {
        Self::dark()
    }
}
