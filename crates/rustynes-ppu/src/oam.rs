//! OAM (Object Attribute Memory) implementation
//!
//! OAM stores sprite data for the PPU. It contains 64 sprite entries,
//! each 4 bytes, for a total of 256 bytes.
//!
//! # Sprite Format (4 bytes per sprite)
//!
//! ```text
//! Byte 0: Y position (top of sprite, minus 1)
//! Byte 1: Tile index
//! Byte 2: Attributes
//!   76543210
//!   |||   ||
//!   |||   ++- Palette (4 to 7) of sprite
//!   |||
//!   ||+------ Priority (0: in front of background; 1: behind background)
//!   |+------- Flip horizontally
//!   +-------- Flip vertically
//! Byte 3: X position (left edge of sprite)
//! ```
//!
//! # OAM DMA
//!
//! The CPU can write to OAM one byte at a time via $2004 (OAMDATA),
//! or copy 256 bytes at once via $4014 (OAMDMA). DMA is much faster
//! and is the standard method for updating sprites.

use bitflags::bitflags;

bitflags! {
    /// Sprite attributes (byte 2 of sprite data)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SpriteAttributes: u8 {
        /// Palette bit 0 (4-7) for sprite
        const PALETTE_0 = 0b0000_0001;
        /// Palette bit 1 (4-7) for sprite
        const PALETTE_1 = 0b0000_0010;
        /// Priority (0: front of bg, 1: behind bg)
        const PRIORITY = 0b0010_0000;
        /// Flip sprite horizontally
        const FLIP_HORIZONTAL = 0b0100_0000;
        /// Flip sprite vertically
        const FLIP_VERTICAL = 0b1000_0000;
    }
}

impl SpriteAttributes {
    /// Get palette index (0-3, maps to palettes 4-7)
    #[inline]
    pub fn palette(self) -> u8 {
        (self.bits() & 0x03) + 4
    }

    /// Check if sprite is behind background
    #[inline]
    pub fn behind_background(self) -> bool {
        self.contains(Self::PRIORITY)
    }

    /// Check if horizontally flipped
    #[inline]
    pub fn flip_horizontal(self) -> bool {
        self.contains(Self::FLIP_HORIZONTAL)
    }

    /// Check if vertically flipped
    #[inline]
    pub fn flip_vertical(self) -> bool {
        self.contains(Self::FLIP_VERTICAL)
    }
}

/// Single sprite entry (4 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sprite {
    /// Y position (top of sprite, minus 1)
    pub y: u8,
    /// Tile index
    pub tile_index: u8,
    /// Sprite attributes
    pub attributes: SpriteAttributes,
    /// X position (left edge)
    pub x: u8,
}

impl Sprite {
    /// Create a new sprite from raw bytes
    #[inline]
    pub fn from_bytes(bytes: &[u8; 4]) -> Self {
        Self {
            y: bytes[0],
            tile_index: bytes[1],
            attributes: SpriteAttributes::from_bits_truncate(bytes[2]),
            x: bytes[3],
        }
    }

    /// Convert sprite to raw bytes
    #[inline]
    pub fn to_bytes(&self) -> [u8; 4] {
        [self.y, self.tile_index, self.attributes.bits(), self.x]
    }

    /// Check if sprite is on given scanline
    ///
    /// A sprite is on a scanline if:
    /// scanline >= sprite.y && scanline < sprite.y + height
    ///
    /// Note: Y position is top of sprite minus 1.
    #[inline]
    pub fn is_on_scanline(&self, scanline: u16, sprite_height: u8) -> bool {
        let y = self.y as u16;
        let height = sprite_height as u16;
        scanline >= y && scanline < y.wrapping_add(height)
    }
}

/// OAM (Object Attribute Memory)
///
/// Stores 64 sprites (256 bytes total).
pub struct Oam {
    /// Primary OAM (256 bytes, 64 sprites)
    data: Vec<u8>,
    /// OAM address register (OAMADDR)
    addr: u8,
}

impl Oam {
    /// Create new OAM
    pub fn new() -> Self {
        Self {
            data: vec![0; 256],
            addr: 0,
        }
    }

    /// Read from OAM at current address (OAMDATA read)
    ///
    /// Note: Reads during rendering return garbage on real hardware.
    pub fn read(&self) -> u8 {
        let value = self.data[self.addr as usize];

        // Mask unused bits (2-4) in byte 2 (attributes)
        // These bits physically do not exist in the PPU OAM
        if self.addr % 4 == 2 {
            value & 0xE3
        } else {
            value
        }
    }

    /// Write to OAM at current address (OAMDATA write)
    ///
    /// Increments address after write.
    pub fn write(&mut self, value: u8) {
        self.data[self.addr as usize] = value;
        self.addr = self.addr.wrapping_add(1);
    }

    /// Get current OAM address (OAMADDR)
    #[inline]
    pub fn get_addr(&self) -> u8 {
        self.addr
    }

    /// Set OAM address (OAMADDR write)
    #[inline]
    pub fn set_addr(&mut self, addr: u8) {
        self.addr = addr;
    }

    /// Perform OAM DMA (copy 256 bytes)
    ///
    /// Copies 256 bytes from CPU memory to OAM, starting at current OAMADDR.
    pub fn dma_write(&mut self, data: &[u8; 256]) {
        let start = self.addr as usize;

        // Copy with wrapping
        if start == 0 {
            // Fast path - no wrapping
            self.data.copy_from_slice(data);
        } else {
            // Copy in two parts due to wrapping
            let first_len = 256 - start;
            self.data[start..].copy_from_slice(&data[..first_len]);
            self.data[..start].copy_from_slice(&data[first_len..]);
        }
    }

    /// Get sprite at index (0-63)
    #[inline]
    pub fn get_sprite(&self, index: u8) -> Sprite {
        let offset = (index as usize) * 4;
        let bytes = [
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ];
        Sprite::from_bytes(&bytes)
    }

    /// Set sprite at index (0-63)
    #[inline]
    pub fn set_sprite(&mut self, index: u8, sprite: &Sprite) {
        let offset = (index as usize) * 4;
        let bytes = sprite.to_bytes();
        self.data[offset..offset + 4].copy_from_slice(&bytes);
    }

    /// Get raw OAM data (for sprite evaluation)
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Clear OAM to power-up state
    pub fn reset(&mut self) {
        self.data.fill(0xFF); // OAM initializes to $FF on power-up
        self.addr = 0;
    }

    /// Clear sprite data (set to $FF)
    pub fn clear(&mut self) {
        self.data.fill(0xFF);
    }
}

impl Default for Oam {
    fn default() -> Self {
        Self::new()
    }
}

/// Secondary OAM
///
/// Used during sprite evaluation to store up to 8 sprites for the next scanline.
pub struct SecondaryOam {
    /// Secondary OAM data (32 bytes, 8 sprites)
    data: Vec<u8>,
    /// Number of sprites in secondary OAM (0-8)
    count: u8,
}

impl SecondaryOam {
    /// Create new secondary OAM
    pub fn new() -> Self {
        Self {
            data: vec![0xFF; 32],
            count: 0,
        }
    }

    /// Clear secondary OAM for new scanline
    pub fn clear(&mut self) {
        self.data.fill(0xFF);
        self.count = 0;
    }

    /// Add sprite to secondary OAM
    ///
    /// Returns true if added, false if full (8 sprites).
    pub fn add_sprite(&mut self, sprite_data: &[u8; 4]) -> bool {
        if self.count >= 8 {
            return false;
        }

        let offset = (self.count as usize) * 4;
        self.data[offset..offset + 4].copy_from_slice(sprite_data);
        self.count += 1;
        true
    }

    /// Get sprite from secondary OAM
    #[inline]
    pub fn get_sprite(&self, index: u8) -> Option<Sprite> {
        if index >= self.count {
            return None;
        }

        let offset = (index as usize) * 4;
        let bytes = [
            self.data[offset],
            self.data[offset + 1],
            self.data[offset + 2],
            self.data[offset + 3],
        ];
        Some(Sprite::from_bytes(&bytes))
    }

    /// Get number of sprites in secondary OAM
    #[inline]
    pub fn count(&self) -> u8 {
        self.count
    }

    /// Check if secondary OAM is full
    #[inline]
    pub fn is_full(&self) -> bool {
        self.count >= 8
    }
}

impl Default for SecondaryOam {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_attributes() {
        let attrs = SpriteAttributes::from_bits_truncate(0b1110_0011);

        assert_eq!(attrs.palette(), 7); // Palette 3 -> 7
        assert!(attrs.behind_background());
        assert!(attrs.flip_horizontal());
        assert!(attrs.flip_vertical());
    }

    #[test]
    fn test_sprite_from_bytes() {
        let bytes = [50, 0x42, 0b0100_0001, 100];
        let sprite = Sprite::from_bytes(&bytes);

        assert_eq!(sprite.y, 50);
        assert_eq!(sprite.tile_index, 0x42);
        assert_eq!(sprite.x, 100);
        assert!(sprite.attributes.flip_horizontal());
        assert!(!sprite.attributes.flip_vertical());
    }

    #[test]
    fn test_sprite_to_bytes() {
        let sprite = Sprite {
            y: 50,
            tile_index: 0x42,
            attributes: SpriteAttributes::FLIP_HORIZONTAL,
            x: 100,
        };

        let bytes = sprite.to_bytes();
        assert_eq!(bytes, [50, 0x42, 0x40, 100]);
    }

    #[test]
    fn test_sprite_on_scanline() {
        let sprite = Sprite {
            y: 50,
            tile_index: 0,
            attributes: SpriteAttributes::empty(),
            x: 0,
        };

        // 8x8 sprite
        assert!(!sprite.is_on_scanline(49, 8));
        assert!(sprite.is_on_scanline(50, 8));
        assert!(sprite.is_on_scanline(57, 8));
        assert!(!sprite.is_on_scanline(58, 8));

        // 8x16 sprite
        assert!(sprite.is_on_scanline(50, 16));
        assert!(sprite.is_on_scanline(65, 16));
        assert!(!sprite.is_on_scanline(66, 16));
    }

    #[test]
    fn test_oam_read_write() {
        let mut oam = Oam::new();

        oam.set_addr(0);
        oam.write(0x50);
        oam.write(0x42);

        // Address should auto-increment
        assert_eq!(oam.get_addr(), 2);

        oam.set_addr(0);
        assert_eq!(oam.read(), 0x50);
        oam.set_addr(1);
        assert_eq!(oam.read(), 0x42);
    }

    #[test]
    fn test_oam_address_wrapping() {
        let mut oam = Oam::new();

        oam.set_addr(255);
        oam.write(0xAA);

        // Should wrap to 0
        assert_eq!(oam.get_addr(), 0);
        // Read what we just wrote (still at address 255)
        oam.set_addr(255);
        assert_eq!(oam.read(), 0xAA);
    }

    #[test]
    fn test_oam_get_set_sprite() {
        let mut oam = Oam::new();

        let sprite = Sprite {
            y: 50,
            tile_index: 0x42,
            attributes: SpriteAttributes::FLIP_HORIZONTAL,
            x: 100,
        };

        oam.set_sprite(5, &sprite);
        let read_sprite = oam.get_sprite(5);

        assert_eq!(read_sprite, sprite);
    }

    #[test]
    fn test_oam_dma_no_wrapping() {
        let mut oam = Oam::new();
        let mut data = [0u8; 256];

        // Fill test data
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = i as u8;
        }

        oam.set_addr(0);
        oam.dma_write(&data);

        // Verify
        for (i, byte) in oam.data.iter().enumerate() {
            assert_eq!(*byte, i as u8);
        }
    }

    #[test]
    fn test_oam_dma_with_wrapping() {
        let mut oam = Oam::new();
        let mut data = [0u8; 256];

        // Fill test data
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = i as u8;
        }

        // Start at offset 128
        oam.set_addr(128);
        oam.dma_write(&data);

        // First 128 bytes should be at offset 128
        for i in 0..128 {
            assert_eq!(oam.data[128 + i], i as u8);
        }

        // Last 128 bytes should wrap to beginning
        for i in 128..256 {
            assert_eq!(oam.data[i - 128], i as u8);
        }
    }

    #[test]
    fn test_oam_reset() {
        let mut oam = Oam::new();

        oam.write(0x42);
        oam.reset();

        assert_eq!(oam.get_addr(), 0);
        assert_eq!(oam.read(), 0xFF); // OAM initializes to $FF
    }

    #[test]
    fn test_secondary_oam_add_sprite() {
        let mut secondary = SecondaryOam::new();

        // Add sprite
        let sprite_data = [50, 0x42, 0x00, 100];
        assert!(secondary.add_sprite(&sprite_data));
        assert_eq!(secondary.count(), 1);

        // Add 7 more sprites
        for _ in 0..7 {
            assert!(secondary.add_sprite(&sprite_data));
        }
        assert_eq!(secondary.count(), 8);

        // Should be full now
        assert!(secondary.is_full());
        assert!(!secondary.add_sprite(&sprite_data));
    }

    #[test]
    fn test_secondary_oam_get_sprite() {
        let mut secondary = SecondaryOam::new();

        let sprite_data = [50, 0x42, 0x40, 100];
        secondary.add_sprite(&sprite_data);

        let sprite = secondary.get_sprite(0).unwrap();
        assert_eq!(sprite.y, 50);
        assert_eq!(sprite.tile_index, 0x42);
        assert_eq!(sprite.x, 100);

        // Out of bounds
        assert!(secondary.get_sprite(1).is_none());
    }

    #[test]
    fn test_secondary_oam_clear() {
        let mut secondary = SecondaryOam::new();

        let sprite_data = [50, 0x42, 0x00, 100];
        secondary.add_sprite(&sprite_data);

        secondary.clear();
        assert_eq!(secondary.count(), 0);
        assert!(secondary.get_sprite(0).is_none());
    }
}
