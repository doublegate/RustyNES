//! Background rendering implementation
//!
//! The PPU renders backgrounds using an 8-stage pipeline that fetches
//! tile data and feeds shift registers to produce pixel output.
//!
//! # Rendering Pipeline (Every 8 Dots)
//!
//! ```text
//! Dot     Action
//! ---     ------
//! 0       Fetch nametable byte (tile index)
//! 2       Fetch attribute table byte (palette)
//! 4       Fetch pattern table low byte (bitplane 0)
//! 6       Fetch pattern table high byte (bitplane 1)
//! 8       Load shift registers, advance coarse X
//! ```
//!
//! # Shift Registers
//!
//! The PPU uses four 16-bit shift registers:
//! - 2 pattern shift registers (low/high bitplanes)
//! - 2 attribute shift registers (palette bits)
//!
//! Every dot, the shift registers shift left by 1 bit.
//! The fine X scroll selects which bit to output (0-7).

/// Background tile fetcher
///
/// Manages the 8-stage tile fetch pipeline and shift registers.
pub struct Background {
    /// Nametable byte (tile index)
    nametable_byte: u8,
    /// Attribute table byte (palette info)
    attribute_byte: u8,
    /// Pattern table low byte (bitplane 0)
    pattern_low: u8,
    /// Pattern table high byte (bitplane 1)
    pattern_high: u8,

    /// Pattern shift register (low bitplane, 16-bit)
    pattern_shift_low: u16,
    /// Pattern shift register (high bitplane, 16-bit)
    pattern_shift_high: u16,
    /// Attribute shift register low (16-bit, same indexing as pattern registers)
    attribute_shift_low: u16,
    /// Attribute shift register high (16-bit, same indexing as pattern registers)
    attribute_shift_high: u16,
}

impl Background {
    /// Create new background renderer
    pub fn new() -> Self {
        Self {
            nametable_byte: 0,
            attribute_byte: 0,
            pattern_low: 0,
            pattern_high: 0,
            pattern_shift_low: 0,
            pattern_shift_high: 0,
            attribute_shift_low: 0,
            attribute_shift_high: 0,
        }
    }

    /// Fetch nametable byte (tile index)
    #[inline]
    pub fn set_nametable_byte(&mut self, byte: u8) {
        self.nametable_byte = byte;
    }

    /// Get nametable byte (tile index)
    #[inline]
    pub fn nametable_byte(&self) -> u8 {
        self.nametable_byte
    }

    /// Fetch attribute table byte (palette)
    ///
    /// The attribute byte covers a 4x4 tile (32x32 pixel) area:
    /// ```text
    /// 76543210
    /// ||||||||
    /// ||||||++- Palette for top-left 2x2 tiles
    /// ||||++--- Palette for top-right 2x2 tiles
    /// ||++----- Palette for bottom-left 2x2 tiles
    /// ++------- Palette for bottom-right 2x2 tiles
    /// ```
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn set_attribute_byte(&mut self, byte: u8, coarse_x: u8, coarse_y: u8) {
        // Determine which 2-bit palette to use based on tile position
        let shift = ((coarse_y & 0x02) << 1) | (coarse_x & 0x02);
        self.attribute_byte = (byte >> shift) & 0x03;
    }

    /// Fetch pattern table low byte (bitplane 0)
    #[inline]
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn set_pattern_low(&mut self, byte: u8) {
        self.pattern_low = byte;
    }

    /// Fetch pattern table high byte (bitplane 1)
    #[inline]
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn set_pattern_high(&mut self, byte: u8) {
        self.pattern_high = byte;
    }

    /// Load shift registers with fetched tile data
    ///
    /// Called every 8 dots after all 4 bytes are fetched.
    /// Pattern and attribute data loads into the low 8 bits while
    /// the high 8 bits continue to be shifted out for rendering.
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn load_shift_registers(&mut self) {
        // Load pattern data into low 8 bits of shift registers
        self.pattern_shift_low = (self.pattern_shift_low & 0xFF00) | (self.pattern_low as u16);
        self.pattern_shift_high = (self.pattern_shift_high & 0xFF00) | (self.pattern_high as u16);

        // Load attribute data into low 8 bits of shift registers
        // Each bit is extended to fill all 8 bits (0x00 or 0xFF)
        // This allows fine_x-based bit selection to work correctly
        self.attribute_shift_low = (self.attribute_shift_low & 0xFF00)
            | if self.attribute_byte & 0x01 != 0 {
                0x00FF
            } else {
                0x0000
            };
        self.attribute_shift_high = (self.attribute_shift_high & 0xFF00)
            | if self.attribute_byte & 0x02 != 0 {
                0x00FF
            } else {
                0x0000
            };
    }

    /// Shift all registers by 1 bit
    ///
    /// Called every dot during rendering.
    /// All four 16-bit shift registers shift left together.
    #[inline]
    pub fn shift_registers(&mut self) {
        self.pattern_shift_low <<= 1;
        self.pattern_shift_high <<= 1;
        self.attribute_shift_low <<= 1;
        self.attribute_shift_high <<= 1;
    }

    /// Get background pixel and palette
    ///
    /// Returns (pixel, palette) where:
    /// - pixel: 2-bit pattern value (0-3)
    /// - palette: 2-bit palette select (0-3)
    ///
    /// If pixel is 0, the background is transparent.
    #[inline]
    pub fn get_pixel(&self, fine_x: u8) -> (u8, u8) {
        // Select bit based on fine X scroll (0-7)
        // All four 16-bit shift registers use the same bit position
        let bit_select = 0x8000 >> fine_x;

        // Get pattern bits (2-bit pixel value)
        let pattern_low_bit = u8::from(self.pattern_shift_low & bit_select != 0);
        let pattern_high_bit = u8::from(self.pattern_shift_high & bit_select != 0);
        let pixel = pattern_low_bit | (pattern_high_bit << 1);

        // Get attribute bits (2-bit palette select)
        // Uses same bit_select as pattern - this is the critical fix!
        let attr_low_bit = u8::from(self.attribute_shift_low & bit_select != 0);
        let attr_high_bit = u8::from(self.attribute_shift_high & bit_select != 0);
        let palette = attr_low_bit | (attr_high_bit << 1);

        (pixel, palette)
    }

    /// Reset to power-up state
    pub fn reset(&mut self) {
        self.nametable_byte = 0;
        self.attribute_byte = 0;
        self.pattern_low = 0;
        self.pattern_high = 0;
        self.pattern_shift_low = 0;
        self.pattern_shift_high = 0;
        self.attribute_shift_low = 0;
        self.attribute_shift_high = 0;
    }
}

impl Default for Background {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_byte_extraction() {
        let mut bg = Background::new();

        // Attribute byte: 0b11_10_01_00
        let attr_byte = 0b11_10_01_00;

        // Top-left (coarse X & Y both even)
        bg.set_attribute_byte(attr_byte, 0, 0);
        assert_eq!(bg.attribute_byte, 0b00);

        // Top-right (coarse X odd, Y even)
        bg.set_attribute_byte(attr_byte, 2, 0);
        assert_eq!(bg.attribute_byte, 0b01);

        // Bottom-left (coarse X even, Y odd)
        bg.set_attribute_byte(attr_byte, 0, 2);
        assert_eq!(bg.attribute_byte, 0b10);

        // Bottom-right (coarse X & Y both odd)
        bg.set_attribute_byte(attr_byte, 2, 2);
        assert_eq!(bg.attribute_byte, 0b11);
    }

    #[test]
    fn test_load_shift_registers() {
        let mut bg = Background::new();

        bg.set_pattern_low(0b1010_1010);
        bg.set_pattern_high(0b1100_1100);
        bg.set_attribute_byte(0b11, 0, 0); // Palette 3

        bg.load_shift_registers();

        // Pattern should be in low 8 bits
        assert_eq!(bg.pattern_shift_low & 0xFF, 0b1010_1010);
        assert_eq!(bg.pattern_shift_high & 0xFF, 0b1100_1100);

        // Attribute shift registers should be extended (0xFF in low byte)
        assert_eq!(bg.attribute_shift_low & 0xFF, 0xFF);
        assert_eq!(bg.attribute_shift_high & 0xFF, 0xFF);
    }

    #[test]
    fn test_shift_registers() {
        let mut bg = Background::new();

        bg.pattern_shift_low = 0b0000_0000_1010_1010;
        bg.pattern_shift_high = 0b0000_0000_1100_1100;

        bg.shift_registers();

        assert_eq!(bg.pattern_shift_low, 0b0000_0001_0101_0100);
        assert_eq!(bg.pattern_shift_high, 0b0000_0001_1001_1000);
    }

    #[test]
    fn test_get_pixel() {
        let mut bg = Background::new();

        // Set up pattern: 0b10101010, 0b11001100 in high byte (for fine_x=0..7 selection)
        bg.pattern_shift_low = 0b1010_1010_0000_0000;
        bg.pattern_shift_high = 0b1100_1100_0000_0000;
        // Attribute shift registers: all 1s in high byte = palette bit set
        bg.attribute_shift_low = 0xFF00; // Palette bit 0 = 1 for all positions
        bg.attribute_shift_high = 0x0000; // Palette bit 1 = 0 for all positions

        // Fine X = 0 (leftmost pixel, bit 15)
        let (pixel, palette) = bg.get_pixel(0);
        // Pattern bits: low=1, high=1 -> pixel=3
        assert_eq!(pixel, 0b11);
        // Palette bits: low=1, high=0 -> palette=1
        assert_eq!(palette, 0b01);

        // Fine X = 1 (bit 14)
        let (pixel, palette) = bg.get_pixel(1);
        // Pattern bits: low=0, high=1 -> pixel=2
        assert_eq!(pixel, 0b10);
        assert_eq!(palette, 0b01);

        // Fine X = 2 (bit 13)
        let (pixel, palette) = bg.get_pixel(2);
        // Pattern bits: low=1, high=0 -> pixel=1
        assert_eq!(pixel, 0b01);
        assert_eq!(palette, 0b01);
    }

    #[test]
    fn test_transparent_pixel() {
        let mut bg = Background::new();

        bg.pattern_shift_low = 0b0000_0000_0000_0000;
        bg.pattern_shift_high = 0b0000_0000_0000_0000;

        let (pixel, _) = bg.get_pixel(0);
        assert_eq!(pixel, 0); // Transparent
    }

    #[test]
    fn test_reset() {
        let mut bg = Background::new();

        bg.nametable_byte = 0xFF;
        bg.pattern_shift_low = 0xFFFF;
        bg.pattern_shift_high = 0xFFFF;

        bg.reset();

        assert_eq!(bg.nametable_byte, 0);
        assert_eq!(bg.pattern_shift_low, 0);
        assert_eq!(bg.pattern_shift_high, 0);
    }

    #[test]
    fn test_full_tile_fetch_cycle() {
        let mut bg = Background::new();

        // Simulate 8-dot fetch cycle
        bg.set_nametable_byte(0x42); // Tile index
        bg.set_attribute_byte(0b11_10_01_00, 0, 0); // Palette 0
        bg.set_pattern_low(0b1010_1010);
        bg.set_pattern_high(0b1100_1100);

        // Load into shift registers
        bg.load_shift_registers();

        // Verify loaded correctly
        assert_eq!(bg.pattern_shift_low & 0xFF, 0b1010_1010);
        assert_eq!(bg.pattern_shift_high & 0xFF, 0b1100_1100);

        // Shift and get pixel
        for i in 0..8 {
            bg.shift_registers();
            let (pixel, _) = bg.get_pixel(7);
            // Verify pixel extracted correctly (from shifted position)
            let expected_low = (0b1010_1010 >> (7 - i)) & 1;
            let expected_high = (0b1100_1100 >> (7 - i)) & 1;
            let expected_pixel = expected_low | (expected_high << 1);
            assert_eq!(pixel, expected_pixel, "Mismatch at shift {i}");
        }
    }
}
