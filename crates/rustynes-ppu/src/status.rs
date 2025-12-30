//! PPU Status Register ($2002).
//!
//! The PPU status register is read-only and contains:
//!
//! ```text
//! 7  6  5  4  3  2  1  0
//! V  S  O  .  .  .  .  .
//! |  |  |  +--+--+--+--+-- Open bus (last value written to PPU registers)
//! |  |  +----------------- Sprite overflow flag
//! |  +-------------------- Sprite 0 hit flag
//! +----------------------- VBlank flag
//! ```
//!
//! Reading $2002 has side effects:
//! - Clears the VBlank flag
//! - Resets the address latch (w register)

use bitflags::bitflags;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

bitflags! {
    /// PPU Status Register flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub struct Status: u8 {
        /// Sprite overflow - Set when more than 8 sprites on a scanline.
        /// Note: Hardware has a bug causing false positives/negatives.
        const SPRITE_OVERFLOW = 1 << 5;
        /// Sprite 0 hit - Set when a non-transparent pixel of sprite 0
        /// overlaps a non-transparent background pixel.
        const SPRITE_ZERO_HIT = 1 << 6;
        /// VBlank flag - Set at dot 1 of line 241 (post-render line).
        /// Cleared after reading $2002 and at dot 1 of pre-render line.
        const VBLANK = 1 << 7;
    }
}

impl Status {
    /// Check if VBlank flag is set.
    #[must_use]
    #[inline]
    pub const fn in_vblank(self) -> bool {
        self.contains(Self::VBLANK)
    }

    /// Check if sprite 0 hit occurred.
    #[must_use]
    #[inline]
    pub const fn sprite_zero_hit(self) -> bool {
        self.contains(Self::SPRITE_ZERO_HIT)
    }

    /// Check if sprite overflow occurred.
    #[must_use]
    #[inline]
    pub const fn sprite_overflow(self) -> bool {
        self.contains(Self::SPRITE_OVERFLOW)
    }

    /// Set or clear the VBlank flag.
    #[inline]
    pub fn set_vblank(&mut self, value: bool) {
        if value {
            *self |= Self::VBLANK;
        } else {
            *self &= !Self::VBLANK;
        }
    }

    /// Set or clear the sprite 0 hit flag.
    #[inline]
    pub fn set_sprite_zero_hit(&mut self, value: bool) {
        if value {
            *self |= Self::SPRITE_ZERO_HIT;
        } else {
            *self &= !Self::SPRITE_ZERO_HIT;
        }
    }

    /// Set or clear the sprite overflow flag.
    #[inline]
    pub fn set_sprite_overflow(&mut self, value: bool) {
        if value {
            *self |= Self::SPRITE_OVERFLOW;
        } else {
            *self &= !Self::SPRITE_OVERFLOW;
        }
    }

    /// Read the status register with open bus bits.
    /// The lower 5 bits return the last value written to any PPU register.
    #[must_use]
    #[inline]
    pub const fn read_with_open_bus(self, open_bus: u8) -> u8 {
        (self.bits() & 0xE0) | (open_bus & 0x1F)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vblank() {
        let mut status = Status::empty();
        assert!(!status.in_vblank());

        status.set_vblank(true);
        assert!(status.in_vblank());

        status.set_vblank(false);
        assert!(!status.in_vblank());
    }

    #[test]
    fn test_sprite_zero_hit() {
        let mut status = Status::empty();
        assert!(!status.sprite_zero_hit());

        status.set_sprite_zero_hit(true);
        assert!(status.sprite_zero_hit());

        status.set_sprite_zero_hit(false);
        assert!(!status.sprite_zero_hit());
    }

    #[test]
    fn test_sprite_overflow() {
        let mut status = Status::empty();
        assert!(!status.sprite_overflow());

        status.set_sprite_overflow(true);
        assert!(status.sprite_overflow());

        status.set_sprite_overflow(false);
        assert!(!status.sprite_overflow());
    }

    #[test]
    fn test_open_bus() {
        let status = Status::VBLANK | Status::SPRITE_ZERO_HIT;
        // Open bus has 0x15 in lower 5 bits
        let result = status.read_with_open_bus(0x15);
        assert_eq!(result, 0xC0 | 0x15);

        let status = Status::empty();
        let result = status.read_with_open_bus(0x1F);
        assert_eq!(result, 0x1F);
    }
}
