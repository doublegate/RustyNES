//! PPU Mask Register ($2001).
//!
//! The PPU mask register controls rendering and color effects:
//!
//! ```text
//! 7  6  5  4  3  2  1  0
//! B  G  R  s  b  M  m  G
//! |  |  |  |  |  |  |  +-- Greyscale (0: normal, 1: greyscale)
//! |  |  |  |  |  |  +----- Show background in leftmost 8 pixels (0: hide, 1: show)
//! |  |  |  |  |  +-------- Show sprites in leftmost 8 pixels (0: hide, 1: show)
//! |  |  |  |  +----------- Show background (0: hide, 1: show)
//! |  |  |  +-------------- Show sprites (0: hide, 1: show)
//! |  |  +----------------- Emphasize red (green on PAL/Dendy)
//! |  +-------------------- Emphasize green (red on PAL/Dendy)
//! +----------------------- Emphasize blue
//! ```

use bitflags::bitflags;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

bitflags! {
    /// PPU Mask Register flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub struct Mask: u8 {
        /// Greyscale mode.
        const GREYSCALE = 1 << 0;
        /// Show background in leftmost 8 pixels of screen.
        const BG_LEFT = 1 << 1;
        /// Show sprites in leftmost 8 pixels of screen.
        const SPRITES_LEFT = 1 << 2;
        /// Show background.
        const BG_ENABLE = 1 << 3;
        /// Show sprites.
        const SPRITES_ENABLE = 1 << 4;
        /// Emphasize red (NTSC) / green (PAL/Dendy).
        const EMPHASIZE_RED = 1 << 5;
        /// Emphasize green (NTSC) / red (PAL/Dendy).
        const EMPHASIZE_GREEN = 1 << 6;
        /// Emphasize blue.
        const EMPHASIZE_BLUE = 1 << 7;
    }
}

impl Mask {
    /// Check if greyscale mode is enabled.
    #[must_use]
    #[inline]
    pub const fn greyscale(self) -> bool {
        self.contains(Self::GREYSCALE)
    }

    /// Check if background is shown in leftmost 8 pixels.
    #[must_use]
    #[inline]
    pub const fn bg_left_enabled(self) -> bool {
        self.contains(Self::BG_LEFT)
    }

    /// Check if sprites are shown in leftmost 8 pixels.
    #[must_use]
    #[inline]
    pub const fn sprites_left_enabled(self) -> bool {
        self.contains(Self::SPRITES_LEFT)
    }

    /// Check if background rendering is enabled.
    #[must_use]
    #[inline]
    pub const fn bg_enabled(self) -> bool {
        self.contains(Self::BG_ENABLE)
    }

    /// Check if sprite rendering is enabled.
    #[must_use]
    #[inline]
    pub const fn sprites_enabled(self) -> bool {
        self.contains(Self::SPRITES_ENABLE)
    }

    /// Check if any rendering is enabled (background or sprites).
    #[must_use]
    #[inline]
    pub const fn rendering_enabled(self) -> bool {
        self.intersects(Self::BG_ENABLE.union(Self::SPRITES_ENABLE))
    }

    /// Get the emphasis bits (bits 5-7).
    #[must_use]
    #[inline]
    pub const fn emphasis(self) -> u8 {
        (self.bits() >> 5) & 0x07
    }

    /// Apply greyscale mask to a color index if enabled.
    #[must_use]
    #[inline]
    pub const fn apply_greyscale(self, color: u8) -> u8 {
        if self.greyscale() {
            color & 0x30
        } else {
            color
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rendering_enabled() {
        assert!(!Mask::empty().rendering_enabled());
        assert!(Mask::BG_ENABLE.rendering_enabled());
        assert!(Mask::SPRITES_ENABLE.rendering_enabled());
        assert!((Mask::BG_ENABLE | Mask::SPRITES_ENABLE).rendering_enabled());
    }

    #[test]
    fn test_emphasis() {
        assert_eq!(Mask::empty().emphasis(), 0);
        assert_eq!(Mask::EMPHASIZE_RED.emphasis(), 1);
        assert_eq!(Mask::EMPHASIZE_GREEN.emphasis(), 2);
        assert_eq!(Mask::EMPHASIZE_BLUE.emphasis(), 4);
        assert_eq!(
            (Mask::EMPHASIZE_RED | Mask::EMPHASIZE_GREEN | Mask::EMPHASIZE_BLUE).emphasis(),
            7
        );
    }

    #[test]
    fn test_greyscale() {
        let mask = Mask::GREYSCALE;
        assert_eq!(mask.apply_greyscale(0x0F), 0x00);
        assert_eq!(mask.apply_greyscale(0x1F), 0x10);
        assert_eq!(mask.apply_greyscale(0x2F), 0x20);
        assert_eq!(mask.apply_greyscale(0x3F), 0x30);

        let no_grey = Mask::empty();
        assert_eq!(no_grey.apply_greyscale(0x1F), 0x1F);
    }

    #[test]
    fn test_left_column() {
        assert!(!Mask::empty().bg_left_enabled());
        assert!(!Mask::empty().sprites_left_enabled());
        assert!(Mask::BG_LEFT.bg_left_enabled());
        assert!(Mask::SPRITES_LEFT.sprites_left_enabled());
    }
}
