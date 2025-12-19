//! PPU register definitions (PPUCTRL, PPUMASK, PPUSTATUS)
//!
//! The NES PPU exposes 8 memory-mapped registers at CPU addresses $2000-$2007.
//! This module defines the bit flags and behavior for the control/status registers.

use bitflags::bitflags;

bitflags! {
    /// PPUCTRL ($2000) - Write Only
    ///
    /// Controls PPU operation and rendering behavior.
    ///
    /// # Bit Layout
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// VPHB SINN
    /// |||| ||||
    /// |||| ||++- Base nametable address
    /// |||| ||    (0 = $2000; 1 = $2400; 2 = $2800; 3 = $2C00)
    /// |||| |+--- VRAM address increment per CPU read/write of PPUDATA
    /// |||| |     (0: add 1, going across; 1: add 32, going down)
    /// |||| +---- Sprite pattern table address for 8×8 sprites
    /// ||||       (0: $0000; 1: $1000; ignored in 8×16 mode)
    /// |||+------ Background pattern table address (0: $0000; 1: $1000)
    /// ||+------- Sprite size (0: 8×8 pixels; 1: 8×16 pixels)
    /// |+-------- PPU master/slave select (not used on NES)
    /// +--------- Generate an NMI at the start of vblank (0: off; 1: on)
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PpuCtrl: u8 {
        /// Nametable X bit (bit 0)
        const NAMETABLE_X = 0b0000_0001;
        /// Nametable Y bit (bit 1)
        const NAMETABLE_Y = 0b0000_0010;
        /// VRAM address increment (0: +1 across, 1: +32 down)
        const VRAM_INCREMENT = 0b0000_0100;
        /// Sprite pattern table address (8×8 mode only)
        const SPRITE_TABLE = 0b0000_1000;
        /// Background pattern table address
        const BG_TABLE = 0b0001_0000;
        /// Sprite size (0: 8×8, 1: 8×16)
        const SPRITE_SIZE = 0b0010_0000;
        /// Master/slave select (unused on NES)
        const MASTER_SLAVE = 0b0100_0000;
        /// Generate NMI at VBlank
        const NMI_ENABLE = 0b1000_0000;
    }
}

impl PpuCtrl {
    /// Get nametable base address ($2000, $2400, $2800, $2C00)
    #[inline]
    pub fn nametable_addr(self) -> u16 {
        0x2000 | ((self.bits() & 0x03) as u16) << 10
    }

    /// Get VRAM address increment (1 or 32)
    #[inline]
    pub fn vram_increment(self) -> u16 {
        if self.contains(Self::VRAM_INCREMENT) {
            32
        } else {
            1
        }
    }

    /// Get sprite pattern table base address ($0000 or $1000)
    #[inline]
    pub fn sprite_table_addr(self) -> u16 {
        if self.contains(Self::SPRITE_TABLE) {
            0x1000
        } else {
            0x0000
        }
    }

    /// Get background pattern table base address ($0000 or $1000)
    #[inline]
    pub fn bg_table_addr(self) -> u16 {
        if self.contains(Self::BG_TABLE) {
            0x1000
        } else {
            0x0000
        }
    }

    /// Get sprite height (8 or 16)
    #[inline]
    pub fn sprite_height(self) -> u8 {
        if self.contains(Self::SPRITE_SIZE) {
            16
        } else {
            8
        }
    }

    /// Check if NMI should be generated at VBlank
    #[inline]
    pub fn nmi_enabled(self) -> bool {
        self.contains(Self::NMI_ENABLE)
    }
}

bitflags! {
    /// PPUMASK ($2001) - Write Only
    ///
    /// Controls rendering enable/disable and color effects.
    ///
    /// # Bit Layout
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// BGRs bMmG
    /// |||| ||||
    /// |||| |||+- Greyscale (0: normal color, 1: greyscale)
    /// |||| ||+-- Show background in leftmost 8 pixels (0: hide, 1: show)
    /// |||| |+--- Show sprites in leftmost 8 pixels (0: hide, 1: show)
    /// |||| +---- Show background
    /// |||+------ Show sprites
    /// ||+------- Emphasize red (green on PAL)
    /// |+-------- Emphasize green (red on PAL)
    /// +--------- Emphasize blue
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PpuMask: u8 {
        /// Greyscale mode
        const GREYSCALE = 0b0000_0001;
        /// Show background in leftmost 8 pixels
        const SHOW_BG_LEFT = 0b0000_0010;
        /// Show sprites in leftmost 8 pixels
        const SHOW_SPRITES_LEFT = 0b0000_0100;
        /// Show background
        const SHOW_BG = 0b0000_1000;
        /// Show sprites
        const SHOW_SPRITES = 0b0001_0000;
        /// Emphasize red (NTSC) / green (PAL)
        const EMPHASIZE_RED = 0b0010_0000;
        /// Emphasize green (NTSC) / red (PAL)
        const EMPHASIZE_GREEN = 0b0100_0000;
        /// Emphasize blue
        const EMPHASIZE_BLUE = 0b1000_0000;
    }
}

impl PpuMask {
    /// Check if rendering is enabled (background or sprites)
    #[inline]
    pub fn rendering_enabled(self) -> bool {
        self.intersects(Self::SHOW_BG | Self::SHOW_SPRITES)
    }

    /// Check if background rendering is enabled
    #[inline]
    pub fn show_background(self) -> bool {
        self.contains(Self::SHOW_BG)
    }

    /// Check if sprite rendering is enabled
    #[inline]
    pub fn show_sprites(self) -> bool {
        self.contains(Self::SHOW_SPRITES)
    }

    /// Check if leftmost 8 pixels should show background
    #[inline]
    pub fn show_bg_left(self) -> bool {
        self.contains(Self::SHOW_BG_LEFT)
    }

    /// Check if leftmost 8 pixels should show sprites
    #[inline]
    pub fn show_sprites_left(self) -> bool {
        self.contains(Self::SHOW_SPRITES_LEFT)
    }
}

bitflags! {
    /// PPUSTATUS ($2002) - Read Only
    ///
    /// PPU status flags and VBlank detection.
    ///
    /// # Bit Layout
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// VSO- ----
    /// |||| ||||
    /// |||+-++++- Open bus (returns last value written to any PPU register)
    /// ||+------- Sprite overflow (more than 8 sprites on a scanline)
    /// |+-------- Sprite 0 hit (sprite 0 overlaps background)
    /// +--------- Vertical blank flag
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PpuStatus: u8 {
        /// Sprite overflow flag (hardware bug)
        const SPRITE_OVERFLOW = 0b0010_0000;
        /// Sprite 0 hit flag
        const SPRITE_ZERO_HIT = 0b0100_0000;
        /// Vertical blank flag
        const VBLANK = 0b1000_0000;
    }
}

impl PpuStatus {
    /// Check if in VBlank period
    #[inline]
    pub fn in_vblank(self) -> bool {
        self.contains(Self::VBLANK)
    }

    /// Check if sprite 0 hit occurred
    #[inline]
    pub fn sprite_zero_hit(self) -> bool {
        self.contains(Self::SPRITE_ZERO_HIT)
    }

    /// Check if sprite overflow occurred
    #[inline]
    pub fn sprite_overflow(self) -> bool {
        self.contains(Self::SPRITE_OVERFLOW)
    }

    /// Set VBlank flag
    #[inline]
    pub fn set_vblank(&mut self) {
        self.insert(Self::VBLANK);
    }

    /// Clear VBlank flag
    #[inline]
    pub fn clear_vblank(&mut self) {
        self.remove(Self::VBLANK);
    }

    /// Set sprite 0 hit flag
    #[inline]
    pub fn set_sprite_zero_hit(&mut self) {
        self.insert(Self::SPRITE_ZERO_HIT);
    }

    /// Set sprite overflow flag
    #[inline]
    pub fn set_sprite_overflow(&mut self) {
        self.insert(Self::SPRITE_OVERFLOW);
    }

    /// Clear sprite flags (sprite 0 hit and overflow)
    #[inline]
    pub fn clear_sprite_flags(&mut self) {
        self.remove(Self::SPRITE_ZERO_HIT | Self::SPRITE_OVERFLOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppuctrl_nametable_addr() {
        assert_eq!(PpuCtrl::empty().nametable_addr(), 0x2000);
        assert_eq!(PpuCtrl::NAMETABLE_X.nametable_addr(), 0x2400);
        assert_eq!(PpuCtrl::NAMETABLE_Y.nametable_addr(), 0x2800);
        assert_eq!(
            (PpuCtrl::NAMETABLE_X | PpuCtrl::NAMETABLE_Y).nametable_addr(),
            0x2C00
        );
    }

    #[test]
    fn test_ppuctrl_vram_increment() {
        assert_eq!(PpuCtrl::empty().vram_increment(), 1);
        assert_eq!(PpuCtrl::VRAM_INCREMENT.vram_increment(), 32);
    }

    #[test]
    fn test_ppuctrl_pattern_table_addrs() {
        assert_eq!(PpuCtrl::empty().sprite_table_addr(), 0x0000);
        assert_eq!(PpuCtrl::SPRITE_TABLE.sprite_table_addr(), 0x1000);
        assert_eq!(PpuCtrl::empty().bg_table_addr(), 0x0000);
        assert_eq!(PpuCtrl::BG_TABLE.bg_table_addr(), 0x1000);
    }

    #[test]
    fn test_ppuctrl_sprite_height() {
        assert_eq!(PpuCtrl::empty().sprite_height(), 8);
        assert_eq!(PpuCtrl::SPRITE_SIZE.sprite_height(), 16);
    }

    #[test]
    fn test_ppumask_rendering_enabled() {
        assert!(!PpuMask::empty().rendering_enabled());
        assert!(PpuMask::SHOW_BG.rendering_enabled());
        assert!(PpuMask::SHOW_SPRITES.rendering_enabled());
        assert!((PpuMask::SHOW_BG | PpuMask::SHOW_SPRITES).rendering_enabled());
    }

    #[test]
    fn test_ppustatus_flags() {
        let mut status = PpuStatus::empty();

        assert!(!status.in_vblank());
        status.set_vblank();
        assert!(status.in_vblank());
        status.clear_vblank();
        assert!(!status.in_vblank());

        status.set_sprite_zero_hit();
        status.set_sprite_overflow();
        assert!(status.sprite_zero_hit());
        assert!(status.sprite_overflow());

        status.clear_sprite_flags();
        assert!(!status.sprite_zero_hit());
        assert!(!status.sprite_overflow());
    }
}
