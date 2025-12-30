//! PPU Sprite (OAM) handling.
//!
//! The NES PPU supports up to 64 sprites, with data stored in Object Attribute Memory (OAM).
//! Each sprite entry is 4 bytes:
//!
//! ```text
//! Byte 0: Y position (top of sprite, actual display Y + 1)
//! Byte 1: Tile index
//! Byte 2: Attributes
//!         76543210
//!         ||||||||
//!         ||||||++-- Palette (4-7)
//!         |||+++---- Unimplemented (reads back 0)
//!         ||+------- Priority (0: in front of background, 1: behind)
//!         |+-------- Flip horizontally
//!         +--------- Flip vertically
//! Byte 3: X position (left side of sprite)
//! ```

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Size of primary OAM in bytes (64 sprites * 4 bytes each).
pub const OAM_SIZE: usize = 256;

/// Size of secondary OAM in bytes (8 sprites * 4 bytes each).
pub const SECONDARY_OAM_SIZE: usize = 32;

/// Maximum sprites per scanline.
pub const MAX_SPRITES_PER_LINE: usize = 8;

/// Sprite attribute flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SpriteAttr(u8);

impl SpriteAttr {
    /// Create from raw byte value.
    #[must_use]
    #[inline]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Get the palette number (0-3, actual palette is 4-7).
    #[must_use]
    #[inline]
    pub const fn palette(self) -> u8 {
        self.0 & 0x03
    }

    /// Get the full palette address offset (4-7).
    #[must_use]
    #[inline]
    pub const fn palette_addr(self) -> u8 {
        4 + (self.0 & 0x03)
    }

    /// Check if sprite is behind background.
    #[must_use]
    #[inline]
    pub const fn behind_background(self) -> bool {
        self.0 & 0x20 != 0
    }

    /// Check if sprite is flipped horizontally.
    #[must_use]
    #[inline]
    pub const fn flip_horizontal(self) -> bool {
        self.0 & 0x40 != 0
    }

    /// Check if sprite is flipped vertically.
    #[must_use]
    #[inline]
    pub const fn flip_vertical(self) -> bool {
        self.0 & 0x80 != 0
    }

    /// Get the raw attribute byte.
    #[must_use]
    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// A single sprite entry from OAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Sprite {
    /// Y position (top of sprite).
    pub y: u8,
    /// Tile index.
    pub tile: u8,
    /// Attributes.
    pub attr: SpriteAttr,
    /// X position (left of sprite).
    pub x: u8,
}

impl Sprite {
    /// Create a sprite from OAM bytes.
    #[must_use]
    #[inline]
    pub const fn from_bytes(bytes: [u8; 4]) -> Self {
        Self {
            y: bytes[0],
            tile: bytes[1],
            attr: SpriteAttr::new(bytes[2]),
            x: bytes[3],
        }
    }

    /// Check if this sprite is on the given scanline (0-239).
    #[must_use]
    #[inline]
    pub const fn is_on_scanline(self, scanline: u16, sprite_height: u8) -> bool {
        let y = self.y as u16;
        // Y in OAM is the scanline before the sprite starts
        scanline >= y && scanline < y + sprite_height as u16
    }

    /// Get the row within the sprite for a given scanline.
    ///
    /// # Safety
    /// Caller should ensure `is_on_scanline()` returns true before calling.
    /// This function uses saturating arithmetic to prevent panics on edge cases.
    #[must_use]
    #[inline]
    pub const fn sprite_row(self, scanline: u16, sprite_height: u8) -> u8 {
        let y = self.y as u16;
        // Saturating subtraction to handle edge cases
        let diff = scanline.saturating_sub(y);
        let row = if diff > 255 { 255 } else { diff as u8 };
        if self.attr.flip_vertical() {
            // Protect against underflow if row >= sprite_height
            if row >= sprite_height {
                0
            } else {
                sprite_height - 1 - row
            }
        } else {
            row
        }
    }
}

/// Sprite evaluation state for scanline rendering.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SpriteEval {
    /// Secondary OAM (up to 8 sprites for current scanline).
    secondary_oam: [u8; SECONDARY_OAM_SIZE],
    /// Number of sprites found for this scanline.
    sprite_count: u8,
    /// Current sprite index being evaluated in primary OAM.
    oam_index: u8,
    /// Current byte within sprite being copied.
    oam_byte: u8,
    /// Whether sprite 0 is in secondary OAM for this scanline.
    sprite_zero_on_line: bool,
    /// Overflow flag (more than 8 sprites on scanline).
    overflow: bool,
    /// Evaluation phase: true = reading, false = writing.
    reading_oam: bool,
}

impl SpriteEval {
    /// Create a new sprite evaluation state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            secondary_oam: [0xFF; SECONDARY_OAM_SIZE],
            sprite_count: 0,
            oam_index: 0,
            oam_byte: 0,
            sprite_zero_on_line: false,
            overflow: false,
            reading_oam: true,
        }
    }

    /// Reset for a new scanline.
    pub fn reset(&mut self) {
        self.secondary_oam.fill(0xFF);
        self.sprite_count = 0;
        self.oam_index = 0;
        self.oam_byte = 0;
        self.sprite_zero_on_line = false;
        self.overflow = false;
        self.reading_oam = true;
    }

    /// Get the sprite count for this scanline.
    #[must_use]
    #[inline]
    pub const fn sprite_count(&self) -> u8 {
        self.sprite_count
    }

    /// Check if sprite 0 is on this scanline.
    #[must_use]
    #[inline]
    pub const fn sprite_zero_on_line(&self) -> bool {
        self.sprite_zero_on_line
    }

    /// Check if sprite overflow occurred.
    #[must_use]
    #[inline]
    pub const fn overflow(&self) -> bool {
        self.overflow
    }

    /// Get a sprite from secondary OAM by index.
    #[must_use]
    #[inline]
    pub fn get_sprite(&self, index: usize) -> Sprite {
        let base = index * 4;
        Sprite::from_bytes([
            self.secondary_oam[base],
            self.secondary_oam[base + 1],
            self.secondary_oam[base + 2],
            self.secondary_oam[base + 3],
        ])
    }

    /// Evaluate sprites for the current scanline.
    /// This should be called during cycles 65-256 of visible scanlines.
    pub fn evaluate(&mut self, oam: &[u8; OAM_SIZE], scanline: u16, sprite_height: u8) {
        self.reset();

        for i in 0..64 {
            let y = oam[i * 4] as u16;

            // Check if sprite is on this scanline
            // Note: Y in OAM is the scanline before the sprite starts
            if scanline >= y && scanline < y + sprite_height as u16 {
                if self.sprite_count < 8 {
                    // Copy sprite to secondary OAM
                    let src = i * 4;
                    let dst = self.sprite_count as usize * 4;
                    self.secondary_oam[dst..dst + 4].copy_from_slice(&oam[src..src + 4]);

                    if i == 0 {
                        self.sprite_zero_on_line = true;
                    }
                    self.sprite_count += 1;
                } else {
                    // More than 8 sprites - set overflow flag
                    // Note: Hardware has a bug here that causes false positives/negatives
                    self.overflow = true;
                    break;
                }
            }
        }
    }

    /// Tick the sprite evaluation state machine (cycle-accurate version).
    /// Returns true if a secondary OAM write occurred.
    #[allow(clippy::too_many_lines)]
    pub fn tick(
        &mut self,
        cycle: u16,
        oam: &[u8; OAM_SIZE],
        scanline: u16,
        sprite_height: u8,
    ) -> bool {
        // Sprite evaluation happens during cycles 65-256
        if !(65..=256).contains(&cycle) {
            return false;
        }

        // Cycles 65-256: alternate between reading OAM and writing secondary OAM
        if (cycle - 65).is_multiple_of(2) {
            // Read cycle
            if self.sprite_count < 8 && self.oam_index < 64 {
                self.reading_oam = true;
            }
            false
        } else {
            // Write cycle
            if self.sprite_count >= 8 {
                // Check for overflow with buggy behavior
                if self.oam_index < 64 && !self.overflow {
                    let idx = self.oam_index as usize * 4 + self.oam_byte as usize;
                    if idx < OAM_SIZE {
                        let y = oam[idx] as u16;
                        if scanline >= y && scanline < y + sprite_height as u16 {
                            self.overflow = true;
                        } else {
                            // Hardware bug: increment both n and m
                            self.oam_byte = (self.oam_byte + 1) & 0x03;
                            self.oam_index += 1;
                        }
                    }
                }
                false
            } else if self.oam_index < 64 {
                let src = self.oam_index as usize * 4;
                let y = oam[src] as u16;

                if self.oam_byte == 0 {
                    // First byte: check if sprite is on scanline
                    if scanline >= y && scanline < y + sprite_height as u16 {
                        // Sprite is on scanline, start copying
                        let dst = self.sprite_count as usize * 4;
                        self.secondary_oam[dst] = oam[src];
                        self.oam_byte = 1;

                        if self.oam_index == 0 {
                            self.sprite_zero_on_line = true;
                        }
                        return true;
                    }
                    // Sprite not on scanline, move to next
                    self.oam_index += 1;
                } else {
                    // Bytes 1-3: copy remaining sprite data
                    let dst = self.sprite_count as usize * 4 + self.oam_byte as usize;
                    self.secondary_oam[dst] = oam[src + self.oam_byte as usize];
                    self.oam_byte += 1;

                    if self.oam_byte >= 4 {
                        self.oam_byte = 0;
                        self.oam_index += 1;
                        self.sprite_count += 1;
                    }
                    return true;
                }
                false
            } else {
                false
            }
        }
    }
}

/// Sprite rendering data for a single sprite on the current scanline.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SpriteRender {
    /// Sprite X position.
    pub x: u8,
    /// Low byte of pattern data.
    pub pattern_lo: u8,
    /// High byte of pattern data.
    pub pattern_hi: u8,
    /// Sprite attributes.
    pub attr: SpriteAttr,
    /// Whether this is sprite 0.
    pub is_sprite_zero: bool,
}

impl SpriteRender {
    /// Get the color index (0-3) for a given pixel offset (0-7).
    #[must_use]
    #[inline]
    pub const fn pixel(&self, offset: u8) -> u8 {
        let bit = if self.attr.flip_horizontal() {
            offset
        } else {
            7 - offset
        };
        let lo = (self.pattern_lo >> bit) & 1;
        let hi = (self.pattern_hi >> bit) & 1;
        lo | (hi << 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_attr() {
        let attr = SpriteAttr::new(0b1110_0011);
        assert_eq!(attr.palette(), 3);
        assert_eq!(attr.palette_addr(), 7);
        assert!(attr.behind_background());
        assert!(attr.flip_horizontal());
        assert!(attr.flip_vertical());

        let attr = SpriteAttr::new(0b0000_0000);
        assert!(!attr.behind_background());
        assert!(!attr.flip_horizontal());
        assert!(!attr.flip_vertical());
    }

    #[test]
    fn test_sprite_from_bytes() {
        let sprite = Sprite::from_bytes([10, 42, 0x63, 100]);
        assert_eq!(sprite.y, 10);
        assert_eq!(sprite.tile, 42);
        assert_eq!(sprite.attr.palette(), 3);
        assert!(sprite.attr.behind_background());
        assert!(sprite.attr.flip_horizontal());
        assert!(!sprite.attr.flip_vertical());
        assert_eq!(sprite.x, 100);
    }

    #[test]
    fn test_sprite_on_scanline() {
        let sprite = Sprite::from_bytes([10, 0, 0, 0]);

        // 8x8 sprite
        assert!(!sprite.is_on_scanline(9, 8));
        assert!(sprite.is_on_scanline(10, 8));
        assert!(sprite.is_on_scanline(17, 8));
        assert!(!sprite.is_on_scanline(18, 8));

        // 8x16 sprite
        assert!(sprite.is_on_scanline(10, 16));
        assert!(sprite.is_on_scanline(25, 16));
        assert!(!sprite.is_on_scanline(26, 16));
    }

    #[test]
    fn test_sprite_row() {
        let sprite = Sprite::from_bytes([10, 0, 0, 0]);
        assert_eq!(sprite.sprite_row(10, 8), 0);
        assert_eq!(sprite.sprite_row(17, 8), 7);

        // With vertical flip
        let flipped = Sprite::from_bytes([10, 0, 0x80, 0]);
        assert_eq!(flipped.sprite_row(10, 8), 7);
        assert_eq!(flipped.sprite_row(17, 8), 0);
    }

    #[test]
    fn test_sprite_eval_simple() {
        let mut oam = [0xFF_u8; OAM_SIZE];
        // Sprite 0 at Y=10
        oam[0] = 10;
        oam[1] = 0;
        oam[2] = 0;
        oam[3] = 50;
        // Sprite 1 at Y=100
        oam[4] = 100;
        oam[5] = 1;
        oam[6] = 0;
        oam[7] = 60;

        let mut eval = SpriteEval::new();
        eval.evaluate(&oam, 10, 8);

        assert_eq!(eval.sprite_count(), 1);
        assert!(eval.sprite_zero_on_line());
        assert!(!eval.overflow());
    }

    #[test]
    fn test_sprite_eval_overflow() {
        let mut oam = [0xFF_u8; OAM_SIZE];
        // Put 9 sprites all at Y=10
        for i in 0..9 {
            oam[i * 4] = 10;
            oam[i * 4 + 1] = i as u8;
            oam[i * 4 + 2] = 0;
            oam[i * 4 + 3] = (i * 10) as u8;
        }

        let mut eval = SpriteEval::new();
        eval.evaluate(&oam, 10, 8);

        assert_eq!(eval.sprite_count(), 8);
        assert!(eval.overflow());
    }

    #[test]
    fn test_sprite_render_pixel() {
        let render = SpriteRender {
            x: 0,
            pattern_lo: 0b1010_1010,
            pattern_hi: 0b1100_1100,
            attr: SpriteAttr::new(0),
            is_sprite_zero: false,
        };

        // Without flip: read from bit 7 down
        assert_eq!(render.pixel(0), 0b11); // bits 7: lo=1, hi=1
        assert_eq!(render.pixel(1), 0b10); // bits 6: lo=0, hi=1
        assert_eq!(render.pixel(2), 0b01); // bits 5: lo=1, hi=0
        assert_eq!(render.pixel(3), 0b00); // bits 4: lo=0, hi=0

        // With horizontal flip
        let flipped = SpriteRender {
            x: 0,
            pattern_lo: 0b1010_1010,
            pattern_hi: 0b1100_1100,
            attr: SpriteAttr::new(0x40),
            is_sprite_zero: false,
        };

        assert_eq!(flipped.pixel(0), 0b00); // bits 0: lo=0, hi=0
        assert_eq!(flipped.pixel(1), 0b01); // bits 1: lo=1, hi=0
    }
}
