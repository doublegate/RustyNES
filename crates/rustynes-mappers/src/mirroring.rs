//! Nametable mirroring modes for NES PPU.
//!
//! The NES PPU has 2KB of VRAM for nametables but needs 4KB for four logical nametables.
//! Mirroring determines how the 2KB is mapped to the 4 nametable addresses.

/// Nametable mirroring mode.
///
/// Determines how the PPU's 2KB of nametable VRAM is mirrored across the
/// four logical nametable addresses ($2000-$2FFF).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    /// Horizontal mirroring (vertical arrangement).
    ///
    /// ```text
    /// [ A ] [ A ]
    /// [ B ] [ B ]
    /// ```
    ///
    /// Used by games with horizontal scrolling (e.g., Super Mario Bros.).
    Horizontal,

    /// Vertical mirroring (horizontal arrangement).
    ///
    /// ```text
    /// [ A ] [ B ]
    /// [ A ] [ B ]
    /// ```
    ///
    /// Used by games with vertical scrolling (e.g., Balloon Fight).
    Vertical,

    /// Single-screen mirroring (lower bank).
    ///
    /// ```text
    /// [ A ] [ A ]
    /// [ A ] [ A ]
    /// ```
    ///
    /// All nametable addresses map to the first 1KB of VRAM.
    SingleScreenLower,

    /// Single-screen mirroring (upper bank).
    ///
    /// ```text
    /// [ B ] [ B ]
    /// [ B ] [ B ]
    /// ```
    ///
    /// All nametable addresses map to the second 1KB of VRAM.
    SingleScreenUpper,

    /// Four-screen mirroring.
    ///
    /// ```text
    /// [ A ] [ B ]
    /// [ C ] [ D ]
    /// ```
    ///
    /// Requires 4KB of VRAM on the cartridge (e.g., Gauntlet).
    FourScreen,
}

impl Mirroring {
    /// Convert nametable address to physical VRAM address.
    ///
    /// # Arguments
    ///
    /// * `addr` - Nametable address ($2000-$2FFF, will be masked to $0000-$0FFF)
    ///
    /// # Returns
    ///
    /// Physical VRAM address (0-0xFFF for 2KB VRAM, 0-0xFFF for 4KB VRAM in `FourScreen` mode)
    ///
    /// # Examples
    ///
    /// ```
    /// use rustynes_mappers::Mirroring;
    ///
    /// let mirror = Mirroring::Horizontal;
    /// assert_eq!(mirror.map_address(0x2000), 0x0000); // Nametable 0 -> A
    /// assert_eq!(mirror.map_address(0x2400), 0x0000); // Nametable 1 -> A
    /// assert_eq!(mirror.map_address(0x2800), 0x0400); // Nametable 2 -> B
    /// assert_eq!(mirror.map_address(0x2C00), 0x0400); // Nametable 3 -> B
    /// ```
    #[must_use]
    pub fn map_address(self, addr: u16) -> u16 {
        let addr = addr & 0x0FFF; // Mask to $0000-$0FFF range
        let nametable = (addr >> 10) & 0x03; // Extract nametable index (0-3)
        let offset = addr & 0x03FF; // Offset within nametable (0-0x3FF)

        let bank = match self {
            Mirroring::Horizontal => match nametable {
                0 | 1 => 0, // Top nametables -> A
                2 | 3 => 1, // Bottom nametables -> B
                _ => unreachable!(),
            },
            Mirroring::Vertical => match nametable {
                0 | 2 => 0, // Left nametables -> A
                1 | 3 => 1, // Right nametables -> B
                _ => unreachable!(),
            },
            Mirroring::SingleScreenLower => 0,  // All -> A
            Mirroring::SingleScreenUpper => 1,  // All -> B
            Mirroring::FourScreen => nametable, // No mirroring
        };

        (bank << 10) | offset
    }

    /// Returns true if this mirroring mode requires 4KB of VRAM.
    ///
    /// # Examples
    ///
    /// ```
    /// use rustynes_mappers::Mirroring;
    ///
    /// assert!(!Mirroring::Horizontal.is_four_screen());
    /// assert!(!Mirroring::Vertical.is_four_screen());
    /// assert!(Mirroring::FourScreen.is_four_screen());
    /// ```
    #[must_use]
    pub const fn is_four_screen(self) -> bool {
        matches!(self, Mirroring::FourScreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_horizontal_mirroring() {
        let mirror = Mirroring::Horizontal;
        // Top row (nametables 0 and 1) -> Bank A
        assert_eq!(mirror.map_address(0x2000), 0x0000);
        assert_eq!(mirror.map_address(0x2400), 0x0000);
        // Bottom row (nametables 2 and 3) -> Bank B
        assert_eq!(mirror.map_address(0x2800), 0x0400);
        assert_eq!(mirror.map_address(0x2C00), 0x0400);
    }

    #[test]
    fn test_vertical_mirroring() {
        let mirror = Mirroring::Vertical;
        // Left column (nametables 0 and 2) -> Bank A
        assert_eq!(mirror.map_address(0x2000), 0x0000);
        assert_eq!(mirror.map_address(0x2800), 0x0000);
        // Right column (nametables 1 and 3) -> Bank B
        assert_eq!(mirror.map_address(0x2400), 0x0400);
        assert_eq!(mirror.map_address(0x2C00), 0x0400);
    }

    #[test]
    fn test_single_screen_lower() {
        let mirror = Mirroring::SingleScreenLower;
        // All nametables -> Bank A
        assert_eq!(mirror.map_address(0x2000), 0x0000);
        assert_eq!(mirror.map_address(0x2400), 0x0000);
        assert_eq!(mirror.map_address(0x2800), 0x0000);
        assert_eq!(mirror.map_address(0x2C00), 0x0000);
    }

    #[test]
    fn test_single_screen_upper() {
        let mirror = Mirroring::SingleScreenUpper;
        // All nametables -> Bank B
        assert_eq!(mirror.map_address(0x2000), 0x0400);
        assert_eq!(mirror.map_address(0x2400), 0x0400);
        assert_eq!(mirror.map_address(0x2800), 0x0400);
        assert_eq!(mirror.map_address(0x2C00), 0x0400);
    }

    #[test]
    fn test_four_screen() {
        let mirror = Mirroring::FourScreen;
        // Each nametable -> Separate bank
        assert_eq!(mirror.map_address(0x2000), 0x0000);
        assert_eq!(mirror.map_address(0x2400), 0x0400);
        assert_eq!(mirror.map_address(0x2800), 0x0800);
        assert_eq!(mirror.map_address(0x2C00), 0x0C00);
    }

    #[test]
    fn test_address_masking() {
        let mirror = Mirroring::Horizontal;
        // Addresses above $2FFF should be masked
        assert_eq!(mirror.map_address(0x3000), mirror.map_address(0x2000));
        assert_eq!(mirror.map_address(0x3400), mirror.map_address(0x2400));
    }

    #[test]
    fn test_is_four_screen() {
        assert!(!Mirroring::Horizontal.is_four_screen());
        assert!(!Mirroring::Vertical.is_four_screen());
        assert!(!Mirroring::SingleScreenLower.is_four_screen());
        assert!(!Mirroring::SingleScreenUpper.is_four_screen());
        assert!(Mirroring::FourScreen.is_four_screen());
    }
}
