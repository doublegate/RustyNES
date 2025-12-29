//! Sprite rendering and evaluation
//!
//! The PPU can display up to 64 sprites, but only 8 per scanline.
//! Sprite evaluation occurs during dots 65-256 to determine which
//! sprites appear on the next scanline.
//!
//! # Sprite Evaluation (Dots 65-256)
//!
//! ```text
//! 1. Clear secondary OAM (dots 1-64)
//! 2. Scan primary OAM for sprites on next scanline (dots 65-256)
//! 3. Copy found sprites to secondary OAM (max 8)
//! 4. Set sprite overflow flag if more than 8 found
//! 5. Set sprite 0 in range flag if sprite 0 is in secondary OAM
//! ```
//!
//! # Sprite Rendering
//!
//! During dots 257-320, the PPU fetches tile data for the 8 sprites
//! in secondary OAM. Each sprite requires 8 memory fetches.

use crate::oam::{SecondaryOam, Sprite, SpriteAttributes};

/// Sprite renderer
///
/// Manages sprite evaluation and rendering.
pub struct SpriteRenderer {
    /// Active sprites for current scanline (up to 8)
    sprites: Vec<Option<Sprite>>,
    /// Sprite pattern shift registers (8 sprites × 2 bitplanes)
    pattern_shift_low: [u8; 8],
    pattern_shift_high: [u8; 8],
    /// Sprite attribute latches (8 sprites)
    attributes: [SpriteAttributes; 8],
    /// Sprite X position counters (8 sprites)
    x_counters: [u8; 8],
    /// Number of active sprites
    sprite_count: u8,
    /// Sprite 0 is on current scanline
    sprite_zero_on_scanline: bool,
}

impl SpriteRenderer {
    /// Create new sprite renderer
    pub fn new() -> Self {
        Self {
            sprites: vec![None; 8],
            pattern_shift_low: [0; 8],
            pattern_shift_high: [0; 8],
            attributes: [SpriteAttributes::empty(); 8],
            x_counters: [0; 8],
            sprite_count: 0,
            sprite_zero_on_scanline: false,
        }
    }

    /// Load sprites from secondary OAM
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn load_sprites(&mut self, secondary_oam: &SecondaryOam, sprite_zero_in_range: bool) {
        self.sprite_count = secondary_oam.count();
        self.sprite_zero_on_scanline = sprite_zero_in_range;

        for i in 0..8 {
            if let Some(sprite) = secondary_oam.get_sprite(i) {
                self.sprites[i as usize] = Some(sprite);
                self.attributes[i as usize] = sprite.attributes;
                self.x_counters[i as usize] = sprite.x;
            } else {
                self.sprites[i as usize] = None;
            }
        }
    }

    /// Load sprite pattern data
    ///
    /// Called during sprite fetch (dots 257-320).
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn load_sprite_pattern(&mut self, sprite_index: u8, pattern_low: u8, pattern_high: u8) {
        if (sprite_index as usize) < self.sprites.len() {
            self.pattern_shift_low[sprite_index as usize] = pattern_low;
            self.pattern_shift_high[sprite_index as usize] = pattern_high;
        }
    }

    /// Tick sprite rendering (shift registers, decrement counters)
    ///
    /// Called every dot during visible scanlines.
    pub fn tick(&mut self) {
        for i in 0..8 {
            if self.x_counters[i] == 0 {
                // Sprite is active, shift pattern
                self.pattern_shift_low[i] <<= 1;
                self.pattern_shift_high[i] <<= 1;
            } else {
                // Sprite not yet active, decrement counter
                self.x_counters[i] -= 1;
            }
        }
    }

    /// Get sprite pixel and palette
    ///
    /// Returns (pixel, palette, priority, sprite_zero_hit) where:
    /// - pixel: 2-bit pattern value (0-3), 0 = transparent
    /// - palette: 2-bit palette select (4-7 for sprites)
    /// - priority: true if sprite is behind background
    /// - sprite_zero_hit: true if this pixel is from sprite 0
    pub fn get_pixel(&self) -> Option<(u8, u8, bool, bool)> {
        // Check sprites in priority order (0 first)
        for i in 0..self.sprite_count as usize {
            if self.x_counters[i] != 0 {
                continue; // Sprite not active yet
            }

            // Get pattern bits (MSB of shift registers)
            let pattern_low_bit = u8::from(self.pattern_shift_low[i] & 0x80 != 0);
            let pattern_high_bit = u8::from(self.pattern_shift_high[i] & 0x80 != 0);

            let pixel = pattern_low_bit | (pattern_high_bit << 1);

            // Skip transparent pixels
            if pixel == 0 {
                continue;
            }

            // Found opaque pixel
            let palette = self.attributes[i].palette();
            let priority = self.attributes[i].behind_background();
            let is_sprite_zero = i == 0 && self.sprite_zero_on_scanline;

            return Some((pixel, palette, priority, is_sprite_zero));
        }

        None
    }

    /// Check if sprite 0 is on current scanline
    #[inline]
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn sprite_zero_on_scanline(&self) -> bool {
        self.sprite_zero_on_scanline
    }

    /// Reset to power-up state
    pub fn reset(&mut self) {
        self.sprites.fill(None);
        self.pattern_shift_low.fill(0);
        self.pattern_shift_high.fill(0);
        self.attributes.fill(SpriteAttributes::empty());
        self.x_counters.fill(0);
        self.sprite_count = 0;
        self.sprite_zero_on_scanline = false;
    }

    /// Clear for new scanline
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn clear_scanline(&mut self) {
        self.sprites.fill(None);
        self.pattern_shift_low.fill(0);
        self.pattern_shift_high.fill(0);
        self.x_counters.fill(0);
        self.sprite_count = 0;
        self.sprite_zero_on_scanline = false;
    }
}

impl Default for SpriteRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Sprite evaluator
///
/// Scans primary OAM to find sprites on next scanline.
///
/// This implementation includes the famous NES sprite overflow bug.
/// When checking for a 9th sprite after secondary OAM is full, the PPU
/// incorrectly increments both the sprite index (n) and the byte offset (m),
/// causing false positives and negatives in overflow detection.
pub struct SpriteEvaluator {
    /// Current sprite being evaluated (0-63) - called 'n' in hardware
    current_sprite: u8,
    /// Current byte within sprite (0-3) - called 'm' in hardware
    /// This is the key to the sprite overflow bug: m increments incorrectly
    /// during overflow checking, causing the PPU to compare scanline against
    /// tile/attribute/X bytes instead of Y coordinates.
    current_byte: u8,
    /// Evaluation phase
    phase: EvalPhase,
    /// Sprite overflow flag
    overflow: bool,
    /// Sprite 0 in range flag
    sprite_zero_in_range: bool,
    /// Track if overflow flag has been set (to avoid re-triggering)
    overflow_detected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalPhase {
    /// Scanning primary OAM
    Scanning,
    /// Secondary OAM full, checking for overflow
    OverflowCheck,
    /// Evaluation complete
    Done,
}

impl SpriteEvaluator {
    /// Create new sprite evaluator
    pub fn new() -> Self {
        Self {
            current_sprite: 0,
            current_byte: 0,
            phase: EvalPhase::Scanning,
            overflow: false,
            sprite_zero_in_range: false,
            overflow_detected: false,
        }
    }

    /// Start sprite evaluation for next scanline
    pub fn start_evaluation(&mut self) {
        self.current_sprite = 0;
        self.current_byte = 0;
        self.phase = EvalPhase::Scanning;
        self.overflow = false;
        self.sprite_zero_in_range = false;
        self.overflow_detected = false;
    }

    /// Perform one step of sprite evaluation
    ///
    /// This implements the hardware-accurate sprite overflow bug.
    ///
    /// # The Sprite Overflow Bug
    ///
    /// When secondary OAM is full (8 sprites found), the PPU continues checking
    /// for additional sprites to set the overflow flag. However, it has a bug:
    ///
    /// - It increments both `n` (sprite index) AND `m` (byte offset within sprite)
    /// - This causes it to read tile/attribute/X bytes instead of Y coordinates
    /// - Result: false positives (overflow set incorrectly) and false negatives
    ///   (overflow not set when it should be)
    ///
    /// This bug is important for hardware accuracy and some games rely on it.
    pub fn evaluate_step(
        &mut self,
        oam_data: &[u8],
        scanline: u16,
        sprite_height: u8,
        secondary_oam: &mut SecondaryOam,
    ) -> bool {
        match self.phase {
            EvalPhase::Scanning => {
                if self.current_sprite >= 64 {
                    self.phase = EvalPhase::Done;
                    return false;
                }

                // Read Y coordinate (byte 0 of sprite)
                let sprite_index = self.current_sprite as usize;
                let y = oam_data[sprite_index * 4];

                // Check if sprite is on next scanline
                let y_u16 = y as u16;
                let height = sprite_height as u16;

                if scanline >= y_u16 && scanline < y_u16.wrapping_add(height) {
                    // Sprite is in range - copy all 4 bytes to secondary OAM
                    let sprite_data = [
                        oam_data[sprite_index * 4],
                        oam_data[sprite_index * 4 + 1],
                        oam_data[sprite_index * 4 + 2],
                        oam_data[sprite_index * 4 + 3],
                    ];

                    if secondary_oam.add_sprite(&sprite_data) {
                        // Successfully added sprite
                        // Track if sprite 0 is in range
                        if self.current_sprite == 0 {
                            self.sprite_zero_in_range = true;
                        }

                        // Check if secondary OAM is now full (8 sprites)
                        // If so, switch to overflow check mode for remaining sprites
                        if secondary_oam.count() >= 8 {
                            self.phase = EvalPhase::OverflowCheck;
                            self.current_byte = 0;
                        }
                    } else {
                        // Secondary OAM was already full when we tried to add
                        // This shouldn't normally happen with our logic, but handle it
                        self.overflow = true;
                        self.overflow_detected = true;
                        self.phase = EvalPhase::Done;
                    }
                }

                self.current_sprite += 1;
                true
            }

            EvalPhase::OverflowCheck => {
                // Hardware sprite overflow bug implementation
                //
                // The PPU continues scanning OAM looking for a 9th sprite.
                // However, it has a bug where it increments BOTH n (sprite index)
                // AND m (byte offset) when a sprite is NOT in range.
                //
                // This means:
                // - Sprite n+0: checks byte m+0 (might be Y, tile, attr, or X)
                // - Sprite n+1: checks byte m+1 (if previous wasn't in range)
                // - Sprite n+2: checks byte m+2
                // - etc.
                //
                // When m wraps around (m >= 4), it's reset to 0 but n continues.
                // This causes the PPU to skip sprites and compare random bytes.

                if self.current_sprite >= 64 {
                    self.phase = EvalPhase::Done;
                    return false;
                }

                // Calculate which byte to read (the buggy part!)
                // On real hardware, m is incremented along with n when sprite is not in range
                let sprite_index = self.current_sprite as usize;
                let byte_offset = self.current_byte as usize;

                // Read the byte at the current offset (which may NOT be the Y coordinate!)
                // This is the core of the overflow bug - we might be reading tile/attr/X
                let oam_byte = oam_data[sprite_index * 4 + byte_offset];

                // Compare against scanline as if it were a Y coordinate
                let height = sprite_height as u16;
                let oam_byte_u16 = oam_byte as u16;

                let in_range =
                    scanline >= oam_byte_u16 && scanline < oam_byte_u16.wrapping_add(height);

                if in_range {
                    // Found a "sprite" in range (even if we're reading wrong byte!)
                    // Set the overflow flag and stop checking
                    if !self.overflow_detected {
                        self.overflow = true;
                        self.overflow_detected = true;
                    }
                    self.phase = EvalPhase::Done;
                } else {
                    // Sprite not in range - increment both n AND m (the bug!)
                    self.current_sprite += 1;
                    self.current_byte = (self.current_byte + 1) & 0x03; // m wraps at 4
                }

                true
            }

            EvalPhase::Done => false,
        }
    }

    /// Check if sprite overflow occurred
    #[inline]
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn overflow(&self) -> bool {
        self.overflow
    }

    /// Check if sprite 0 is in range
    #[inline]
    #[allow(dead_code)] // Used in full rendering implementation
    pub fn sprite_zero_in_range(&self) -> bool {
        self.sprite_zero_in_range
    }
}

impl Default for SpriteEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_renderer_load() {
        let mut renderer = SpriteRenderer::new();
        let mut secondary_oam = SecondaryOam::new();

        // Add sprite to secondary OAM
        let sprite_data = [50, 0x42, 0x01, 100]; // Y, tile, attr, X
        secondary_oam.add_sprite(&sprite_data);

        renderer.load_sprites(&secondary_oam, true);

        assert_eq!(renderer.sprite_count, 1);
        assert!(renderer.sprite_zero_on_scanline);
        assert_eq!(renderer.x_counters[0], 100);
    }

    #[test]
    fn test_sprite_renderer_tick() {
        let mut renderer = SpriteRenderer::new();

        renderer.x_counters[0] = 2;
        renderer.pattern_shift_low[0] = 0b1010_1010;

        // First tick - decrement counter
        renderer.tick();
        assert_eq!(renderer.x_counters[0], 1);
        assert_eq!(renderer.pattern_shift_low[0], 0b1010_1010); // No shift

        // Second tick - decrement to 0
        renderer.tick();
        assert_eq!(renderer.x_counters[0], 0);

        // Third tick - shift pattern
        renderer.tick();
        assert_eq!(renderer.pattern_shift_low[0], 0b0101_0100); // Shifted
    }

    #[test]
    fn test_sprite_renderer_get_pixel() {
        let mut renderer = SpriteRenderer::new();

        renderer.sprite_count = 2;
        renderer.x_counters[0] = 0; // Active
        renderer.x_counters[1] = 1; // Not active yet

        renderer.pattern_shift_low[0] = 0b1000_0000;
        renderer.pattern_shift_high[0] = 0b1000_0000;
        renderer.attributes[0] = SpriteAttributes::from_bits_truncate(0x01); // Palette 1 (raw bits)
        renderer.sprite_zero_on_scanline = true;

        let result = renderer.get_pixel();
        assert!(result.is_some());

        let (pixel, palette, priority, sprite_zero) = result.unwrap();
        assert_eq!(pixel, 0b11); // Both bits set
        assert_eq!(palette, 1); // Raw palette bits (0-3), render_pixel adds +16 for sprite base
        assert!(!priority); // Front of background
        assert!(sprite_zero); // Sprite 0
    }

    #[test]
    fn test_sprite_renderer_transparent() {
        let mut renderer = SpriteRenderer::new();

        renderer.sprite_count = 1;
        renderer.x_counters[0] = 0;
        renderer.pattern_shift_low[0] = 0b0000_0000; // Transparent
        renderer.pattern_shift_high[0] = 0b0000_0000;

        let result = renderer.get_pixel();
        assert!(result.is_none()); // No opaque pixel
    }

    #[test]
    fn test_sprite_evaluator_basic() {
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        // Create OAM with sprite at Y=50
        let mut oam_data = vec![0xFF; 256];
        oam_data[0] = 50; // Sprite 0 Y position
        oam_data[1] = 0x42; // Tile
        oam_data[2] = 0x00; // Attributes
        oam_data[3] = 100; // X position

        evaluator.start_evaluation();

        // Evaluate at scanline 50 (sprite should be found)
        let step = evaluator.evaluate_step(&oam_data, 50, 8, &mut secondary_oam);
        assert!(step);
        assert_eq!(secondary_oam.count(), 1);
        assert!(evaluator.sprite_zero_in_range());
    }

    #[test]
    fn test_sprite_evaluator_not_in_range() {
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        // Create OAM with sprite at Y=50
        let mut oam_data = vec![0xFF; 256];
        oam_data[0] = 50;

        evaluator.start_evaluation();

        // Evaluate at scanline 100 (sprite not in range)
        evaluator.evaluate_step(&oam_data, 100, 8, &mut secondary_oam);
        assert_eq!(secondary_oam.count(), 0);
        assert!(!evaluator.sprite_zero_in_range());
    }

    #[test]
    fn test_sprite_evaluator_overflow() {
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        // Create OAM with 10 sprites all at Y=50
        let mut oam_data = vec![0xFF; 256];
        for i in 0..10 {
            oam_data[i * 4] = 50;
        }

        evaluator.start_evaluation();

        // Evaluate all sprites
        for _ in 0..10 {
            evaluator.evaluate_step(&oam_data, 50, 8, &mut secondary_oam);
        }

        // Secondary OAM should be full (8 sprites)
        assert_eq!(secondary_oam.count(), 8);
        // Overflow should be set
        assert!(evaluator.overflow());
    }

    #[test]
    fn test_sprite_overflow_bug_false_positive() {
        // Test the hardware sprite overflow bug: false positive case
        //
        // After 8 sprites fill secondary OAM, we enter overflow check mode.
        // The bug causes the PPU to read wrong bytes (tile/attr/X instead of Y).
        //
        // Setup:
        // - 8 sprites at Y=50 fill secondary OAM
        // - Sprite 8 (index 8) NOT in range (Y=200) - m=0, n=8, not in range
        // - Sprite 9 (index 9) NOT in range (Y=200) but tile=50 - m=1, n=9, checks tile
        // - Due to bug, we check sprite 9's byte 1 (tile=50) which IS "in range"
        //
        // Result: FALSE POSITIVE - overflow set even though only 8 sprites are in range
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        let mut oam_data = vec![0xFF; 256];

        // 8 sprites at Y=50 (fill secondary OAM)
        for i in 0..8 {
            oam_data[i * 4] = 50; // Y
            oam_data[i * 4 + 1] = 200; // Tile (not in range if checked as Y)
            oam_data[i * 4 + 2] = 200; // Attributes
            oam_data[i * 4 + 3] = 200; // X
        }

        // Sprite 8: Y=200 (not in range), all other bytes also not in range
        // This causes m to increment to 1 and n to increment to 9
        oam_data[8 * 4] = 200;
        oam_data[8 * 4 + 1] = 200;
        oam_data[8 * 4 + 2] = 200;
        oam_data[8 * 4 + 3] = 200;

        // Sprite 9: Y=200 (not in range), BUT tile=50 which IS "in range"
        // Due to the bug, we check byte 1 (tile) instead of byte 0 (Y)
        oam_data[9 * 4] = 200; // Y - not in range (but we won't check this!)
        oam_data[9 * 4 + 1] = 50; // Tile - in range when checked as Y (bug!)
        oam_data[9 * 4 + 2] = 200;
        oam_data[9 * 4 + 3] = 200;

        evaluator.start_evaluation();

        // Evaluate enough sprites to trigger the bug
        for _ in 0..16 {
            evaluator.evaluate_step(&oam_data, 50, 8, &mut secondary_oam);
        }

        // Secondary OAM should be full (8 sprites)
        assert_eq!(secondary_oam.count(), 8);

        // Overflow SHOULD be set due to the bug (false positive)
        // Only 8 sprites are genuinely in range, but the bug causes overflow
        assert!(
            evaluator.overflow(),
            "Bug false positive: overflow should be set due to bug reading tile as Y"
        );
    }

    #[test]
    fn test_sprite_overflow_bug_byte_offset_increment() {
        // Test the hardware sprite overflow bug: verify m (byte offset) increment pattern
        //
        // After secondary OAM is full, the PPU checks remaining sprites but
        // incorrectly increments both n (sprite index) and m (byte offset).
        //
        // Pattern: sprite 8 at m=0, sprite 9 at m=1, sprite 10 at m=2, sprite 11 at m=3,
        //          sprite 12 at m=0 (wraps), etc.
        //
        // We trigger overflow at sprite 10 by setting its byte 2 (attribute) to 50.
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        let mut oam_data = vec![200u8; 256]; // All bytes = 200 (not in range)

        // 8 sprites at Y=50 (fill secondary OAM)
        for i in 0..8 {
            oam_data[i * 4] = 50; // Y
        }

        // Sprite 8: checked at m=0, Y=200 (not in range) -> m++, n++
        // Sprite 9: checked at m=1, tile=200 (not in range) -> m++, n++
        // Sprite 10: checked at m=2, attr=50 (IN RANGE due to bug!)
        oam_data[10 * 4 + 2] = 50; // Attribute of sprite 10 = 50 (triggers false positive)

        evaluator.start_evaluation();

        // Evaluate enough sprites
        for _ in 0..16 {
            evaluator.evaluate_step(&oam_data, 50, 8, &mut secondary_oam);
        }

        // Overflow should be set because sprite 10's attribute (byte 2) = 50 is "in range"
        assert!(
            evaluator.overflow(),
            "Bug: overflow should be set when sprite 10's attribute is checked as Y"
        );
    }

    #[test]
    fn test_sprite_overflow_bug_false_negative() {
        // Test the hardware sprite overflow bug: false negative case
        //
        // Setup:
        // - 8 sprites at Y=50 fill secondary OAM
        // - Sprite 8 (index 8) NOT in range (Y=200) -> m=0, not in range, m++, n++
        // - Sprite 9 (index 9) IS in range (Y=50) but we check byte 1 (tile=200)
        //
        // Result: FALSE NEGATIVE - overflow NOT set even though 9 sprites are in range
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        let mut oam_data = vec![200u8; 256]; // Default: not in range

        // 8 sprites at Y=50 (fill secondary OAM)
        for i in 0..8 {
            oam_data[i * 4] = 50; // Y
            oam_data[i * 4 + 1] = 200; // Tile (not in range)
            oam_data[i * 4 + 2] = 200; // Attr
            oam_data[i * 4 + 3] = 200; // X
        }

        // Sprite 8: Y=200 (not in range at byte 0) -> m=1, n=9
        oam_data[8 * 4] = 200;
        oam_data[8 * 4 + 1] = 200;
        oam_data[8 * 4 + 2] = 200;
        oam_data[8 * 4 + 3] = 200;

        // Sprite 9: Y=50 (genuinely in range!) but we check byte 1 (tile=200)
        // Due to bug, we check tile instead of Y, and tile=200 is not in range
        oam_data[9 * 4] = 50; // Y = 50 (genuinely in range, but we won't check this!)
        oam_data[9 * 4 + 1] = 200; // Tile = 200 (we check this instead, not in range)
        oam_data[9 * 4 + 2] = 200;
        oam_data[9 * 4 + 3] = 200;

        // All remaining sprites: all bytes = 200 (not in range)
        // (already set by vec![200u8; 256])

        evaluator.start_evaluation();

        // Evaluate all 64 sprites
        for _ in 0..64 {
            evaluator.evaluate_step(&oam_data, 50, 8, &mut secondary_oam);
        }

        // Secondary OAM should be full (8 sprites)
        assert_eq!(secondary_oam.count(), 8);

        // Overflow should NOT be set (false negative due to bug)
        // Sprite 9 is genuinely at Y=50 (in range), making it the 9th sprite
        // But the bug causes us to check byte 1 (tile=200) instead, missing it
        assert!(
            !evaluator.overflow(),
            "Bug false negative: overflow should NOT be set even though sprite 9 is in range"
        );
    }
}
