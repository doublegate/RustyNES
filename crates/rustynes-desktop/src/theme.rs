//! Theme system for RustyNES with multiple color schemes.
//!
//! Provides multiple theme variants (Dark, Light, Nord, Gruvbox) with
//! the default "Nostalgic Futurism" design.
//!
//! Colors for the default dark theme follow `RustyNES-UI_UX-Design-v2.md`:
//! - Console Black (#1A1A2E) - Primary background
//! - Deep Navy (#16213E) - Secondary background
//! - NES Blue (#0F3460) - Accent color
//! - Power Red (#E94560) - Primary action color
//! - Coral Accent (#FF6B6B) - Secondary action color

use iced::{Color, Theme as IcedTheme};
use serde::{Deserialize, Serialize};

/// Available theme variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeVariant {
    /// Dark theme (default "Nostalgic Futurism")
    #[default]
    Dark,
    /// Light theme
    Light,
    /// Nord color scheme
    Nord,
    /// Gruvbox Dark
    GruvboxDark,
}

impl ThemeVariant {
    /// Get all available themes
    pub fn all() -> &'static [ThemeVariant] {
        &[
            ThemeVariant::Dark,
            ThemeVariant::Light,
            ThemeVariant::Nord,
            ThemeVariant::GruvboxDark,
        ]
    }

    /// Convert to Iced theme
    pub fn to_iced_theme(self) -> IcedTheme {
        match self {
            Self::Dark => IcedTheme::Dark,
            Self::Light => IcedTheme::Light,
            Self::Nord => IcedTheme::Nord,
            Self::GruvboxDark => IcedTheme::GruvboxDark,
        }
    }

    /// Get custom palette for this theme
    #[allow(dead_code)] // Available for custom widget styling
    pub fn palette(self) -> RustyPalette {
        match self {
            Self::Dark => RustyPalette::dark(),
            Self::Light => RustyPalette::light(),
            Self::Nord => RustyPalette::nord(),
            Self::GruvboxDark => RustyPalette::gruvbox_dark(),
        }
    }
}

impl std::fmt::Display for ThemeVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dark => write!(f, "Dark"),
            Self::Light => write!(f, "Light"),
            Self::Nord => write!(f, "Nord"),
            Self::GruvboxDark => write!(f, "Gruvbox Dark"),
        }
    }
}

/// Custom color palette for RustyNES themes
#[derive(Debug, Clone)]
#[allow(dead_code)] // Some fields used, infrastructure for custom widget styling
pub struct RustyPalette {
    /// Primary background color
    pub background: Color,
    /// Secondary surface color
    pub surface: Color,
    /// Accent color
    pub accent: Color,
    /// Primary action color
    pub primary: Color,
    /// Success color
    pub success: Color,
    /// Danger/error color
    pub danger: Color,
    /// Primary text color
    pub text: Color,
    /// Dimmed text color
    pub text_dim: Color,
}

impl RustyPalette {
    /// Dark theme palette (Nostalgic Futurism)
    pub fn dark() -> Self {
        Self {
            // #1C1E21 - Console Black
            background: Color::from_rgb(
                0x1C as f32 / 255.0,
                0x1E as f32 / 255.0,
                0x21 as f32 / 255.0,
            ),
            // #262A2E - Deep Navy surface
            surface: Color::from_rgb(
                0x26 as f32 / 255.0,
                0x2A as f32 / 255.0,
                0x2E as f32 / 255.0,
            ),
            // #0F3460 - NES Blue
            accent: Color::from_rgb(
                0x0F as f32 / 255.0,
                0x34 as f32 / 255.0,
                0x60 as f32 / 255.0,
            ),
            // #E94560 - Power Red
            primary: Color::from_rgb(
                0xE9 as f32 / 255.0,
                0x45 as f32 / 255.0,
                0x60 as f32 / 255.0,
            ),
            // #4AB04F - Success Green
            success: Color::from_rgb(
                0x4A as f32 / 255.0,
                0xB0 as f32 / 255.0,
                0x4F as f32 / 255.0,
            ),
            // #E85454 - Danger Red
            danger: Color::from_rgb(
                0xE8 as f32 / 255.0,
                0x54 as f32 / 255.0,
                0x54 as f32 / 255.0,
            ),
            // #DEDEDE - Text
            text: Color::from_rgb(
                0xDE as f32 / 255.0,
                0xDE as f32 / 255.0,
                0xDE as f32 / 255.0,
            ),
            // #999999 - Dimmed text
            text_dim: Color::from_rgb(
                0x99 as f32 / 255.0,
                0x99 as f32 / 255.0,
                0x99 as f32 / 255.0,
            ),
        }
    }

    /// Light theme palette
    pub fn light() -> Self {
        Self {
            background: Color::from_rgb(0.98, 0.98, 0.98),
            surface: Color::from_rgb(1.0, 1.0, 1.0),
            accent: Color::from_rgb(0.2, 0.4, 0.8),
            primary: Color::from_rgb(0.0, 0.5, 1.0),
            success: Color::from_rgb(0.0, 0.7, 0.3),
            danger: Color::from_rgb(0.9, 0.2, 0.2),
            text: Color::from_rgb(0.1, 0.1, 0.1),
            text_dim: Color::from_rgb(0.4, 0.4, 0.4),
        }
    }

    /// Nord theme palette
    pub fn nord() -> Self {
        Self {
            // Nord Polar Night
            background: Color::from_rgb(
                0x2E as f32 / 255.0,
                0x34 as f32 / 255.0,
                0x40 as f32 / 255.0,
            ),
            surface: Color::from_rgb(
                0x3B as f32 / 255.0,
                0x42 as f32 / 255.0,
                0x52 as f32 / 255.0,
            ),
            // Nord Frost
            accent: Color::from_rgb(
                0x88 as f32 / 255.0,
                0xC0 as f32 / 255.0,
                0xD0 as f32 / 255.0,
            ),
            primary: Color::from_rgb(
                0x5E as f32 / 255.0,
                0x81 as f32 / 255.0,
                0xAC as f32 / 255.0,
            ),
            success: Color::from_rgb(
                0xA3 as f32 / 255.0,
                0xBE as f32 / 255.0,
                0x8C as f32 / 255.0,
            ),
            danger: Color::from_rgb(
                0xBF as f32 / 255.0,
                0x61 as f32 / 255.0,
                0x6A as f32 / 255.0,
            ),
            // Nord Snow Storm
            text: Color::from_rgb(
                0xEC as f32 / 255.0,
                0xEF as f32 / 255.0,
                0xF4 as f32 / 255.0,
            ),
            text_dim: Color::from_rgb(
                0xD8 as f32 / 255.0,
                0xDE as f32 / 255.0,
                0xE9 as f32 / 255.0,
            ),
        }
    }

    /// Gruvbox Dark palette
    pub fn gruvbox_dark() -> Self {
        Self {
            background: Color::from_rgb(
                0x28 as f32 / 255.0,
                0x28 as f32 / 255.0,
                0x28 as f32 / 255.0,
            ),
            surface: Color::from_rgb(
                0x3C as f32 / 255.0,
                0x38 as f32 / 255.0,
                0x36 as f32 / 255.0,
            ),
            accent: Color::from_rgb(
                0x45 as f32 / 255.0,
                0x85 as f32 / 255.0,
                0x88 as f32 / 255.0,
            ),
            primary: Color::from_rgb(
                0xFE as f32 / 255.0,
                0x80 as f32 / 255.0,
                0x19 as f32 / 255.0,
            ),
            success: Color::from_rgb(
                0x98 as f32 / 255.0,
                0x97 as f32 / 255.0,
                0x1A as f32 / 255.0,
            ),
            danger: Color::from_rgb(
                0xFB as f32 / 255.0,
                0x49 as f32 / 255.0,
                0x34 as f32 / 255.0,
            ),
            text: Color::from_rgb(
                0xEB as f32 / 255.0,
                0xDB as f32 / 255.0,
                0xB2 as f32 / 255.0,
            ),
            text_dim: Color::from_rgb(
                0xA8 as f32 / 255.0,
                0x99 as f32 / 255.0,
                0x84 as f32 / 255.0,
            ),
        }
    }

    /// Glass morphism background color (semi-transparent)
    #[allow(dead_code)] // Will be used for overlays and modals
    pub fn glass_background(&self) -> Color {
        Color::from_rgba(self.background.r, self.background.g, self.background.b, 0.7)
    }
}

impl Default for RustyPalette {
    fn default() -> Self {
        Self::dark()
    }
}
