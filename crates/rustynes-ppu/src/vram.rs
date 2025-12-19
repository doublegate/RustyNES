//! PPU VRAM (Video RAM) and palette RAM implementation
//!
//! The PPU has access to:
//! - 2KB internal VRAM for nametables (mirrored to 4KB address space)
//! - 32 bytes of palette RAM
//! - External CHR ROM/RAM (accessed via mapper)
//!
//! # Address Space ($0000-$3FFF)
//!
//! ```text
//! $0000-$0FFF: Pattern Table 0 (CHR ROM/RAM)
//! $1000-$1FFF: Pattern Table 1 (CHR ROM/RAM)
//! $2000-$23FF: Nametable 0
//! $2400-$27FF: Nametable 1
//! $2800-$2BFF: Nametable 2
//! $2C00-$2FFF: Nametable 3
//! $3000-$3EFF: Mirror of $2000-$2EFF
//! $3F00-$3F1F: Palette RAM (32 bytes)
//! $3F20-$3FFF: Mirror of $3F00-$3F1F
//! ```
//!
//! # Nametable Mirroring
//!
//! The NES only has 2KB internal VRAM, so nametables are mirrored:
//! - **Horizontal**: A A B B (vertical scrolling games)
//! - **Vertical**: A B A B (horizontal scrolling games)
//! - **Single-Screen**: A A A A or B B B B
//! - **Four-Screen**: A B C D (mapper provides extra RAM)

/// Nametable mirroring mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    /// Horizontal mirroring (vertical arrangement)
    Horizontal,
    /// Vertical mirroring (horizontal arrangement)
    Vertical,
    /// Single-screen mirroring (lower bank)
    SingleScreenLower,
    /// Single-screen mirroring (upper bank)
    SingleScreenUpper,
    /// Four-screen mirroring (mapper-provided RAM)
    FourScreen,
}

/// PPU VRAM manager
///
/// Handles internal 2KB nametable RAM and 32-byte palette RAM.
/// Pattern table access is delegated to the mapper.
pub struct Vram {
    /// Internal nametable RAM (2KB)
    nametables: Vec<u8>,
    /// Palette RAM (32 bytes)
    palette: Vec<u8>,
    /// Current mirroring mode
    mirroring: Mirroring,
    /// Four-screen mode extra RAM (optional, 4KB)
    four_screen_ram: Option<Vec<u8>>,
}

impl Vram {
    /// Create new VRAM with specified mirroring
    pub fn new(mirroring: Mirroring) -> Self {
        Self {
            nametables: vec![0; 2048], // 2KB
            palette: vec![0; 32],
            mirroring,
            four_screen_ram: if matches!(mirroring, Mirroring::FourScreen) {
                Some(vec![0; 4096]) // 4KB for four-screen
            } else {
                None
            },
        }
    }

    /// Set mirroring mode
    pub fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.mirroring = mirroring;
        if matches!(mirroring, Mirroring::FourScreen) && self.four_screen_ram.is_none() {
            self.four_screen_ram = Some(vec![0; 4096]);
        }
    }

    /// Get current mirroring mode
    pub fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    /// Read from VRAM address space
    ///
    /// Pattern table reads ($0000-$1FFF) should be handled by the mapper.
    /// This handles nametable ($2000-$2FFF) and palette ($3F00-$3FFF) reads.
    pub fn read(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF; // Mirror $4000-$FFFF to $0000-$3FFF

        match addr {
            // Pattern tables - should be handled by mapper
            0x0000..=0x1FFF => {
                log::warn!("VRAM read from pattern table ${addr:04X} - should use mapper");
                0
            }

            // Nametables ($2000-$2FFF) with mirroring
            0x2000..=0x2FFF => {
                let mirrored_addr = self.mirror_nametable_addr(addr);
                self.nametables[mirrored_addr]
            }

            // Nametable mirror ($3000-$3EFF)
            0x3000..=0x3EFF => {
                let mirrored_addr = self.mirror_nametable_addr(addr - 0x1000);
                self.nametables[mirrored_addr]
            }

            // Palette RAM ($3F00-$3FFF)
            0x3F00..=0x3FFF => {
                let palette_addr = self.mirror_palette_addr(addr);
                self.palette[palette_addr]
            }

            _ => unreachable!(),
        }
    }

    /// Write to VRAM address space
    pub fn write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;

        match addr {
            // Pattern tables - should be handled by mapper
            0x0000..=0x1FFF => {
                log::warn!("VRAM write to pattern table ${addr:04X} - should use mapper");
            }

            // Nametables ($2000-$2FFF) with mirroring
            0x2000..=0x2FFF => {
                let mirrored_addr = self.mirror_nametable_addr(addr);
                self.nametables[mirrored_addr] = value;
            }

            // Nametable mirror ($3000-$3EFF)
            0x3000..=0x3EFF => {
                let mirrored_addr = self.mirror_nametable_addr(addr - 0x1000);
                self.nametables[mirrored_addr] = value;
            }

            // Palette RAM ($3F00-$3FFF)
            0x3F00..=0x3FFF => {
                let palette_addr = self.mirror_palette_addr(addr);
                self.palette[palette_addr] = value;
            }

            _ => unreachable!(),
        }
    }

    /// Mirror nametable address according to current mirroring mode
    fn mirror_nametable_addr(&self, addr: u16) -> usize {
        let addr = addr & 0x0FFF; // Get offset within nametable space
        let nametable = (addr / 0x0400) as usize; // Which nametable (0-3)
        let offset = (addr % 0x0400) as usize; // Offset within nametable

        match self.mirroring {
            Mirroring::Horizontal => {
                // A A B B (0=A, 1=A, 2=B, 3=B)
                let bank = usize::from(nametable >= 2);
                bank * 0x0400 + offset
            }
            Mirroring::Vertical => {
                // A B A B (0=A, 1=B, 2=A, 3=B)
                let bank = nametable % 2;
                bank * 0x0400 + offset
            }
            Mirroring::SingleScreenLower => {
                // A A A A (all map to first 1KB)
                offset
            }
            Mirroring::SingleScreenUpper => {
                // B B B B (all map to second 1KB)
                0x0400 + offset
            }
            Mirroring::FourScreen => {
                // A B C D (use external RAM)
                // This shouldn't be called if four_screen_ram is used
                nametable * 0x0400 + offset
            }
        }
    }

    /// Mirror palette address according to palette mirroring rules
    ///
    /// Palette RAM has special mirroring:
    /// - $3F10, $3F14, $3F18, $3F1C mirror $3F00, $3F04, $3F08, $3F0C
    /// - All addresses mirror every 32 bytes
    fn mirror_palette_addr(&self, addr: u16) -> usize {
        let mut addr = (addr & 0x1F) as usize; // Mirror to 32 bytes

        // Mirror sprite palette background colors to background palette
        if addr >= 0x10 && addr % 4 == 0 {
            addr -= 0x10;
        }

        addr
    }

    /// Read palette entry directly (for rendering)
    #[inline]
    pub fn read_palette(&self, addr: u8) -> u8 {
        let addr = self.mirror_palette_addr(0x3F00 | (addr as u16));
        self.palette[addr]
    }

    /// Reset VRAM to power-up state
    pub fn reset(&mut self) {
        // Clear nametables
        self.nametables.fill(0);
        // Clear palette RAM
        self.palette.fill(0);
        // Clear four-screen RAM if present
        if let Some(ref mut ram) = self.four_screen_ram {
            ram.fill(0);
        }
    }
}

impl Default for Vram {
    fn default() -> Self {
        Self::new(Mirroring::Horizontal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_horizontal_mirroring() {
        let vram = Vram::new(Mirroring::Horizontal);

        // Nametables 0 and 1 should map to the same memory
        assert_eq!(vram.mirror_nametable_addr(0x2000), 0x0000);
        assert_eq!(vram.mirror_nametable_addr(0x2400), 0x0000);

        // Nametables 2 and 3 should map to the same memory
        assert_eq!(vram.mirror_nametable_addr(0x2800), 0x0400);
        assert_eq!(vram.mirror_nametable_addr(0x2C00), 0x0400);
    }

    #[test]
    fn test_vertical_mirroring() {
        let vram = Vram::new(Mirroring::Vertical);

        // Nametables 0 and 2 should map to the same memory
        assert_eq!(vram.mirror_nametable_addr(0x2000), 0x0000);
        assert_eq!(vram.mirror_nametable_addr(0x2800), 0x0000);

        // Nametables 1 and 3 should map to the same memory
        assert_eq!(vram.mirror_nametable_addr(0x2400), 0x0400);
        assert_eq!(vram.mirror_nametable_addr(0x2C00), 0x0400);
    }

    #[test]
    fn test_single_screen_lower() {
        let vram = Vram::new(Mirroring::SingleScreenLower);

        // All nametables map to the same memory (first 1KB)
        assert_eq!(vram.mirror_nametable_addr(0x2000), 0x0000);
        assert_eq!(vram.mirror_nametable_addr(0x2400), 0x0000);
        assert_eq!(vram.mirror_nametable_addr(0x2800), 0x0000);
        assert_eq!(vram.mirror_nametable_addr(0x2C00), 0x0000);
    }

    #[test]
    fn test_palette_mirroring() {
        let vram = Vram::new(Mirroring::Horizontal);

        // Normal palette addresses
        assert_eq!(vram.mirror_palette_addr(0x3F00), 0x00);
        assert_eq!(vram.mirror_palette_addr(0x3F0F), 0x0F);

        // Sprite palette background colors mirror to background palette
        assert_eq!(vram.mirror_palette_addr(0x3F10), 0x00); // Mirrors $3F00
        assert_eq!(vram.mirror_palette_addr(0x3F14), 0x04); // Mirrors $3F04
        assert_eq!(vram.mirror_palette_addr(0x3F18), 0x08); // Mirrors $3F08
        assert_eq!(vram.mirror_palette_addr(0x3F1C), 0x0C); // Mirrors $3F0C

        // Non-background sprite colors don't mirror
        assert_eq!(vram.mirror_palette_addr(0x3F11), 0x11);
        assert_eq!(vram.mirror_palette_addr(0x3F1F), 0x1F);
    }

    #[test]
    fn test_palette_read_write() {
        let mut vram = Vram::new(Mirroring::Horizontal);

        // Write to palette
        vram.write(0x3F00, 0x0F); // Universal background
        vram.write(0x3F01, 0x30); // Background palette 0, color 1

        assert_eq!(vram.read(0x3F00), 0x0F);
        assert_eq!(vram.read(0x3F01), 0x30);

        // Test mirroring - sprite palette background should read from bg palette
        vram.write(0x3F10, 0x20);
        assert_eq!(vram.read(0x3F00), 0x20); // Should read the mirrored value
    }

    #[test]
    fn test_nametable_read_write() {
        let mut vram = Vram::new(Mirroring::Horizontal);

        vram.write(0x2000, 0x42);
        assert_eq!(vram.read(0x2000), 0x42);

        // Test horizontal mirroring - NT1 should mirror NT0
        assert_eq!(vram.read(0x2400), 0x42);
    }

    #[test]
    fn test_nametable_mirror_region() {
        let mut vram = Vram::new(Mirroring::Horizontal);

        // Write to $2000
        vram.write(0x2000, 0x55);

        // Should be readable from $3000 (mirror)
        assert_eq!(vram.read(0x3000), 0x55);

        // Write to $3100 should affect $2100
        vram.write(0x3100, 0xAA);
        assert_eq!(vram.read(0x2100), 0xAA);
    }

    #[test]
    fn test_palette_32_byte_mirror() {
        let mut vram = Vram::new(Mirroring::Horizontal);

        vram.write(0x3F00, 0x11);

        // Should mirror every 32 bytes
        assert_eq!(vram.read(0x3F20), 0x11);
        assert_eq!(vram.read(0x3F40), 0x11);
        assert_eq!(vram.read(0x3FE0), 0x11);
    }

    #[test]
    fn test_change_mirroring() {
        let mut vram = Vram::new(Mirroring::Horizontal);

        vram.write(0x2000, 0x42);
        vram.write(0x2400, 0x55);

        // With horizontal mirroring, both should be 0x55
        assert_eq!(vram.read(0x2000), 0x55);

        // Change to vertical mirroring
        vram.set_mirroring(Mirroring::Vertical);

        // Now they should be independent
        vram.write(0x2000, 0xAA);
        assert_eq!(vram.read(0x2000), 0xAA);
        // 0x2400 maps to bank 1 which was never written under vertical mirroring
        assert_eq!(vram.read(0x2400), 0x00); // Different bank now, not written yet
    }

    #[test]
    fn test_reset() {
        let mut vram = Vram::new(Mirroring::Horizontal);

        vram.write(0x2000, 0x42);
        vram.write(0x3F00, 0x0F);

        vram.reset();

        assert_eq!(vram.read(0x2000), 0x00);
        assert_eq!(vram.read(0x3F00), 0x00);
    }
}
