//! PPU Control Register ($2000).
//!
//! The PPU control register controls various PPU settings:
//!
//! ```text
//! 7  6  5  4  3  2  1  0
//! V  P  H  B  S  I  N  N
//! |  |  |  |  |  |  +--+-- Base nametable address (0=$2000, 1=$2400, 2=$2800, 3=$2C00)
//! |  |  |  |  |  +------- VRAM address increment (0: add 1, going across; 1: add 32, going down)
//! |  |  |  |  +---------- Sprite pattern table address for 8x8 sprites (0: $0000; 1: $1000)
//! |  |  |  +------------- Background pattern table address (0: $0000; 1: $1000)
//! |  |  +---------------- Sprite size (0: 8x8; 1: 8x16)
//! |  +------------------- PPU master/slave select (0: read backdrop from EXT pins; 1: output color on EXT pins)
//! +---------------------- Generate an NMI at the start of vblank (0: off; 1: on)
//! ```

use bitflags::bitflags;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

bitflags! {
    /// PPU Control Register flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub struct Ctrl: u8 {
        /// Base nametable address bit 0.
        const NAMETABLE_LO = 1 << 0;
        /// Base nametable address bit 1.
        const NAMETABLE_HI = 1 << 1;
        /// VRAM address increment mode (0: add 1, 1: add 32).
        const VRAM_INCREMENT = 1 << 2;
        /// Sprite pattern table address for 8x8 sprites.
        const SPRITE_PATTERN = 1 << 3;
        /// Background pattern table address.
        const BG_PATTERN = 1 << 4;
        /// Sprite size (0: 8x8, 1: 8x16).
        const SPRITE_SIZE = 1 << 5;
        /// PPU master/slave select.
        const MASTER_SLAVE = 1 << 6;
        /// NMI enable at start of VBlank.
        const NMI_ENABLE = 1 << 7;
    }
}

impl Ctrl {
    /// Get the base nametable address.
    #[must_use]
    #[inline]
    pub const fn nametable_addr(self) -> u16 {
        0x2000 + ((self.bits() as u16) & 0x03) * 0x0400
    }

    /// Get the nametable select bits (0-3).
    #[must_use]
    #[inline]
    pub const fn nametable_select(self) -> u8 {
        self.bits() & 0x03
    }

    /// Get the VRAM address increment value.
    #[must_use]
    #[inline]
    pub const fn vram_increment(self) -> u16 {
        if self.contains(Self::VRAM_INCREMENT) {
            32
        } else {
            1
        }
    }

    /// Get the sprite pattern table address for 8x8 sprites.
    #[must_use]
    #[inline]
    pub const fn sprite_pattern_addr(self) -> u16 {
        if self.contains(Self::SPRITE_PATTERN) {
            0x1000
        } else {
            0x0000
        }
    }

    /// Get the background pattern table address.
    #[must_use]
    #[inline]
    pub const fn bg_pattern_addr(self) -> u16 {
        if self.contains(Self::BG_PATTERN) {
            0x1000
        } else {
            0x0000
        }
    }

    /// Check if sprites are 8x16 (true) or 8x8 (false).
    #[must_use]
    #[inline]
    pub const fn sprite_size_16(self) -> bool {
        self.contains(Self::SPRITE_SIZE)
    }

    /// Get sprite height (8 or 16).
    #[must_use]
    #[inline]
    pub const fn sprite_height(self) -> u8 {
        if self.sprite_size_16() { 16 } else { 8 }
    }

    /// Check if NMI is enabled.
    #[must_use]
    #[inline]
    pub const fn nmi_enabled(self) -> bool {
        self.contains(Self::NMI_ENABLE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nametable_addr() {
        assert_eq!(Ctrl::empty().nametable_addr(), 0x2000);
        assert_eq!(Ctrl::NAMETABLE_LO.nametable_addr(), 0x2400);
        assert_eq!(Ctrl::NAMETABLE_HI.nametable_addr(), 0x2800);
        assert_eq!(
            (Ctrl::NAMETABLE_LO | Ctrl::NAMETABLE_HI).nametable_addr(),
            0x2C00
        );
    }

    #[test]
    fn test_vram_increment() {
        assert_eq!(Ctrl::empty().vram_increment(), 1);
        assert_eq!(Ctrl::VRAM_INCREMENT.vram_increment(), 32);
    }

    #[test]
    fn test_pattern_addresses() {
        assert_eq!(Ctrl::empty().sprite_pattern_addr(), 0x0000);
        assert_eq!(Ctrl::SPRITE_PATTERN.sprite_pattern_addr(), 0x1000);
        assert_eq!(Ctrl::empty().bg_pattern_addr(), 0x0000);
        assert_eq!(Ctrl::BG_PATTERN.bg_pattern_addr(), 0x1000);
    }

    #[test]
    fn test_sprite_size() {
        assert!(!Ctrl::empty().sprite_size_16());
        assert!(Ctrl::SPRITE_SIZE.sprite_size_16());
        assert_eq!(Ctrl::empty().sprite_height(), 8);
        assert_eq!(Ctrl::SPRITE_SIZE.sprite_height(), 16);
    }
}
