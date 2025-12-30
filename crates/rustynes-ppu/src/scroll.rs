//! PPU Scroll and Address Registers.
//!
//! The PPU has an internal 15-bit VRAM address register (v) and a temporary
//! address register (t). The address latch (w) toggles between high/low bytes.
//!
//! ```text
//! v/t register layout:
//! yyy NN YYYYY XXXXX
//! ||| || ||||| +++++-- coarse X scroll (5 bits)
//! ||| || +++++-------- coarse Y scroll (5 bits)
//! ||| ++-------------- nametable select (2 bits)
//! +++----------------- fine Y scroll (3 bits)
//! ```
//!
//! The fine X scroll is stored separately in a 3-bit register (x).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// PPU scroll/address registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Scroll {
    /// Current VRAM address (15 bits, but stored as u16).
    v: u16,
    /// Temporary VRAM address (15 bits).
    t: u16,
    /// Fine X scroll (3 bits).
    x: u8,
    /// Address latch (write toggle): false = first write, true = second write.
    w: bool,
}

impl Scroll {
    /// Mask for the 15-bit VRAM address.
    const VRAM_MASK: u16 = 0x7FFF;
    /// Mask for coarse X (bits 0-4).
    const COARSE_X_MASK: u16 = 0x001F;
    /// Mask for coarse Y (bits 5-9).
    const COARSE_Y_MASK: u16 = 0x03E0;
    /// Mask for nametable select (bits 10-11).
    const NAMETABLE_MASK: u16 = 0x0C00;
    /// Mask for fine Y (bits 12-14).
    const FINE_Y_MASK: u16 = 0x7000;

    /// Create a new scroll register set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            v: 0,
            t: 0,
            x: 0,
            w: false,
        }
    }

    /// Get the current VRAM address.
    #[must_use]
    #[inline]
    pub const fn vram_addr(self) -> u16 {
        self.v & Self::VRAM_MASK
    }

    /// Get the coarse X scroll (0-31).
    #[must_use]
    #[inline]
    pub const fn coarse_x(self) -> u8 {
        (self.v & Self::COARSE_X_MASK) as u8
    }

    /// Get the coarse Y scroll (0-31, but 30-31 are attribute table).
    #[must_use]
    #[inline]
    pub const fn coarse_y(self) -> u8 {
        ((self.v & Self::COARSE_Y_MASK) >> 5) as u8
    }

    /// Get the fine X scroll (0-7).
    #[must_use]
    #[inline]
    pub const fn fine_x(self) -> u8 {
        self.x & 0x07
    }

    /// Get the fine Y scroll (0-7).
    #[must_use]
    #[inline]
    pub const fn fine_y(self) -> u8 {
        ((self.v & Self::FINE_Y_MASK) >> 12) as u8
    }

    /// Get the nametable select bits (0-3).
    #[must_use]
    #[inline]
    pub const fn nametable(self) -> u8 {
        ((self.v & Self::NAMETABLE_MASK) >> 10) as u8
    }

    /// Get the address latch state.
    #[must_use]
    #[inline]
    pub const fn write_latch(self) -> bool {
        self.w
    }

    /// Reset the address latch (called when reading $2002).
    #[inline]
    pub fn reset_latch(&mut self) {
        self.w = false;
    }

    /// Write to PPUCTRL ($2000) - updates nametable select in t.
    #[inline]
    pub fn write_ctrl(&mut self, value: u8) {
        // t: ...GH.. ........ <- d: ......GH
        // Bits 0-1 of value go to bits 10-11 of t
        self.t = (self.t & !Self::NAMETABLE_MASK) | ((u16::from(value) & 0x03) << 10);
    }

    /// Write to PPUSCROLL ($2005).
    #[inline]
    pub fn write_scroll(&mut self, value: u8) {
        if self.w {
            // Second write: Y scroll
            // t: FGH..AB CDE..... <- d: ABCDEFGH
            self.t = (self.t & !(Self::COARSE_Y_MASK | Self::FINE_Y_MASK))
                | ((u16::from(value) & 0xF8) << 2)
                | ((u16::from(value) & 0x07) << 12);
        } else {
            // First write: X scroll
            // t: ....... ...ABCDE <- d: ABCDE...
            // x:              FGH <- d: .....FGH
            self.t = (self.t & !Self::COARSE_X_MASK) | (u16::from(value) >> 3);
            self.x = value & 0x07;
        }
        self.w = !self.w;
    }

    /// Write to PPUADDR ($2006).
    #[inline]
    pub fn write_addr(&mut self, value: u8) {
        if self.w {
            // Second write: low byte
            // t: ....... ABCDEFGH <- d: ABCDEFGH
            // v: <...all bits...> <- t
            self.t = (self.t & 0xFF00) | u16::from(value);
            self.v = self.t;
        } else {
            // First write: high byte
            // t: .CDEFGH ........ <- d: ..CDEFGH
            // t: Z...... ........ <- 0 (bit 15 is always cleared)
            self.t = (self.t & 0x00FF) | ((u16::from(value) & 0x3F) << 8);
        }
        self.w = !self.w;
    }

    /// Increment the VRAM address by the specified amount.
    #[inline]
    pub fn increment_vram(&mut self, amount: u16) {
        self.v = self.v.wrapping_add(amount) & Self::VRAM_MASK;
    }

    /// Copy horizontal position from t to v.
    /// Called at dot 257 of each visible scanline.
    #[inline]
    pub fn copy_horizontal(&mut self) {
        // v: ....A.. ...BCDEF <- t: ....A.. ...BCDEF
        self.v = (self.v & !0x041F) | (self.t & 0x041F);
    }

    /// Copy vertical position from t to v.
    /// Called at dots 280-304 of the pre-render scanline.
    #[inline]
    pub fn copy_vertical(&mut self) {
        // v: GHIA.BC DEF..... <- t: GHIA.BC DEF.....
        self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
    }

    /// Increment coarse X (horizontal scroll).
    /// Called at the end of each tile fetch.
    #[inline]
    pub fn increment_x(&mut self) {
        if (self.v & Self::COARSE_X_MASK) == 31 {
            // Wrap around and switch horizontal nametable
            self.v = (self.v & !Self::COARSE_X_MASK) ^ 0x0400;
        } else {
            self.v += 1;
        }
    }

    /// Increment fine Y (vertical scroll).
    /// Called at dot 256 of each visible scanline.
    #[inline]
    pub fn increment_y(&mut self) {
        if (self.v & Self::FINE_Y_MASK) == Self::FINE_Y_MASK {
            // Fine Y = 7, reset to 0 and increment coarse Y
            self.v &= !Self::FINE_Y_MASK;
            let mut coarse_y = self.coarse_y();
            if coarse_y == 29 {
                // Row 29 is the last row of tiles, wrap to 0 and switch nametable
                coarse_y = 0;
                self.v ^= 0x0800;
            } else if coarse_y == 31 {
                // Row 31 wraps to 0 without switching nametable (attribute area)
                coarse_y = 0;
            } else {
                coarse_y += 1;
            }
            self.v = (self.v & !Self::COARSE_Y_MASK) | (u16::from(coarse_y) << 5);
        } else {
            // Fine Y < 7, increment it
            self.v += 0x1000;
        }
    }

    /// Get the nametable byte address for the current tile.
    #[must_use]
    #[inline]
    pub const fn nametable_addr(self) -> u16 {
        0x2000 | (self.v & 0x0FFF)
    }

    /// Get the attribute table address for the current tile.
    #[must_use]
    #[inline]
    pub const fn attribute_addr(self) -> u16 {
        0x23C0 | (self.v & Self::NAMETABLE_MASK) | ((self.v >> 4) & 0x38) | ((self.v >> 2) & 0x07)
    }

    /// Get the pattern table address for a tile.
    #[must_use]
    #[inline]
    pub fn pattern_addr(self, tile: u8, base: u16) -> u16 {
        base + (u16::from(tile) << 4) + u16::from(self.fine_y())
    }

    /// Get the temporary register value.
    #[must_use]
    #[inline]
    pub const fn temp_addr(self) -> u16 {
        self.t
    }

    /// Set the VRAM address directly (for debugging/testing).
    #[inline]
    pub fn set_vram_addr(&mut self, addr: u16) {
        self.v = addr & Self::VRAM_MASK;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let scroll = Scroll::new();
        assert_eq!(scroll.vram_addr(), 0);
        assert_eq!(scroll.fine_x(), 0);
        assert!(!scroll.write_latch());
    }

    #[test]
    fn test_write_scroll() {
        let mut scroll = Scroll::new();

        // First write: X scroll = 0x7D (coarse 15, fine 5)
        scroll.write_scroll(0x7D);
        assert!(scroll.write_latch());
        assert_eq!(scroll.temp_addr() & 0x001F, 15); // coarse X
        assert_eq!(scroll.fine_x(), 5);

        // Second write: Y scroll = 0x5E (coarse 11, fine 6)
        scroll.write_scroll(0x5E);
        assert!(!scroll.write_latch());
        assert_eq!((scroll.temp_addr() >> 5) & 0x1F, 11); // coarse Y
        assert_eq!((scroll.temp_addr() >> 12) & 0x07, 6); // fine Y
    }

    #[test]
    fn test_write_addr() {
        let mut scroll = Scroll::new();

        // Write address $2108
        scroll.write_addr(0x21);
        assert!(scroll.write_latch());
        assert_eq!(scroll.vram_addr(), 0); // v not updated yet

        scroll.write_addr(0x08);
        assert!(!scroll.write_latch());
        assert_eq!(scroll.vram_addr(), 0x2108);
    }

    #[test]
    fn test_increment_x() {
        let mut scroll = Scroll::new();
        scroll.set_vram_addr(0x001F); // coarse X = 31

        scroll.increment_x();
        assert_eq!(scroll.coarse_x(), 0);
        // Nametable bit should toggle
        assert_eq!(scroll.vram_addr() & 0x0400, 0x0400);
    }

    #[test]
    fn test_increment_y() {
        let mut scroll = Scroll::new();

        // Test fine Y increment
        scroll.set_vram_addr(0x0000);
        scroll.increment_y();
        assert_eq!(scroll.fine_y(), 1);

        // Test fine Y overflow to coarse Y
        scroll.set_vram_addr(0x7000); // fine Y = 7
        scroll.increment_y();
        assert_eq!(scroll.fine_y(), 0);
        assert_eq!(scroll.coarse_y(), 1);
    }

    #[test]
    fn test_nametable_addr() {
        let mut scroll = Scroll::new();
        scroll.set_vram_addr(0x0000);
        assert_eq!(scroll.nametable_addr(), 0x2000);

        scroll.set_vram_addr(0x0400);
        assert_eq!(scroll.nametable_addr(), 0x2400);
    }

    #[test]
    fn test_attribute_addr() {
        let mut scroll = Scroll::new();
        scroll.set_vram_addr(0x0000);
        assert_eq!(scroll.attribute_addr(), 0x23C0);
    }

    #[test]
    fn test_copy_horizontal() {
        let mut scroll = Scroll::new();
        scroll.set_vram_addr(0x7FFF);
        scroll.write_addr(0x00);
        scroll.write_addr(0x00);
        scroll.t = 0x041F;

        scroll.copy_horizontal();
        assert_eq!(scroll.vram_addr() & 0x041F, 0x041F);
    }

    #[test]
    fn test_copy_vertical() {
        let mut scroll = Scroll::new();
        scroll.set_vram_addr(0x0000);
        scroll.t = 0x7BE0;

        scroll.copy_vertical();
        assert_eq!(scroll.vram_addr() & 0x7BE0, 0x7BE0);
    }
}
