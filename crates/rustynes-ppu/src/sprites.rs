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
pub struct SpriteEvaluator {
    /// Current sprite being evaluated (0-63)
    current_sprite: u8,
    /// Current byte within sprite (0-3)
    current_byte: u8,
    /// Evaluation phase
    phase: EvalPhase,
    /// Sprite overflow flag
    overflow: bool,
    /// Sprite 0 in range flag
    sprite_zero_in_range: bool,
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
        }
    }

    /// Start sprite evaluation for next scanline
    pub fn start_evaluation(&mut self) {
        self.current_sprite = 0;
        self.current_byte = 0;
        self.phase = EvalPhase::Scanning;
        self.overflow = false;
        self.sprite_zero_in_range = false;
    }

    /// Perform one step of sprite evaluation
    ///
    /// Returns Some(sprite_data) if a sprite should be added to secondary OAM.
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

                // Read Y coordinate
                let sprite_index = self.current_sprite as usize;
                let y = oam_data[sprite_index * 4];

                // Check if sprite is on next scanline
                // OAM Y value specifies the top scanline of the sprite (minus 1 for display)
                // Sprites with Y >= 239 effectively appear at scanline 240+ (off-screen)
                // Y = 255 is a special case that hides the sprite completely
                let sprite_top = (y as u16).wrapping_add(1); // Actual top scanline of sprite
                let height = sprite_height as u16;

                // Skip sprites that would be entirely off-screen
                // sprite_top >= 240 means the sprite doesn't appear on any visible scanline
                if sprite_top >= 240 {
                    self.current_sprite += 1;
                    return true;
                }

                if scanline >= sprite_top && scanline < sprite_top.wrapping_add(height) {
                    // Sprite is in range
                    let sprite_data = [
                        oam_data[sprite_index * 4],
                        oam_data[sprite_index * 4 + 1],
                        oam_data[sprite_index * 4 + 2],
                        oam_data[sprite_index * 4 + 3],
                    ];

                    if secondary_oam.add_sprite(&sprite_data) {
                        // Track if sprite 0 is in range
                        if self.current_sprite == 0 {
                            self.sprite_zero_in_range = true;
                        }
                    } else {
                        // Secondary OAM full, check for overflow
                        self.phase = EvalPhase::OverflowCheck;
                        self.overflow = true;
                    }
                }

                self.current_sprite += 1;
                true
            }

            EvalPhase::OverflowCheck => {
                // Continue scanning for hardware sprite overflow bug
                // (Simplified - real hardware has complex buggy behavior)
                if self.current_sprite >= 64 {
                    self.phase = EvalPhase::Done;
                }
                self.current_sprite += 1;
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
        renderer.attributes[0] = SpriteAttributes::from_bits_truncate(0x01); // Palette 5
        renderer.sprite_zero_on_scanline = true;

        let result = renderer.get_pixel();
        assert!(result.is_some());

        let (pixel, palette, priority, sprite_zero) = result.unwrap();
        assert_eq!(pixel, 0b11); // Both bits set
        assert_eq!(palette, 5); // Palette 4 + 1
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

        // Create OAM with sprite at Y=50 (appears at scanline 51 due to Y+1 offset)
        let mut oam_data = vec![0xFF; 256];
        oam_data[0] = 50; // Sprite 0 Y position (minus 1 in OAM format)
        oam_data[1] = 0x42; // Tile
        oam_data[2] = 0x00; // Attributes
        oam_data[3] = 100; // X position

        evaluator.start_evaluation();

        // Evaluate at scanline 51 (sprite with OAM Y=50 appears at scanline 51)
        let step = evaluator.evaluate_step(&oam_data, 51, 8, &mut secondary_oam);
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

        // Create OAM with 10 sprites all at Y=50 (appear at scanline 51)
        let mut oam_data = vec![0xFF; 256];
        for i in 0..10 {
            oam_data[i * 4] = 50;
        }

        evaluator.start_evaluation();

        // Evaluate all sprites at scanline 51 (OAM Y=50 + 1 offset)
        for _ in 0..10 {
            evaluator.evaluate_step(&oam_data, 51, 8, &mut secondary_oam);
        }

        // Secondary OAM should be full (8 sprites)
        assert_eq!(secondary_oam.count(), 8);
        // Overflow should be set
        assert!(evaluator.overflow());
    }

    #[test]
    fn test_sprite_evaluator_y_255_always_skipped() {
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        // Create OAM with sprite 0 at Y=255 (should always be off-screen)
        // Y=255 means sprite_top = 256, which is >= 240, so it should be skipped
        let mut oam_data = vec![0xFF; 256];
        oam_data[0] = 255; // Sprite 0 at Y=255
        oam_data[1] = 0x42; // Tile
        oam_data[2] = 0x00; // Attributes
        oam_data[3] = 100; // X position

        // Test on multiple scanlines - sprite should never be in range
        for scanline in 0..240 {
            evaluator.start_evaluation();
            secondary_oam.clear();

            evaluator.evaluate_step(&oam_data, scanline, 8, &mut secondary_oam);

            assert_eq!(
                secondary_oam.count(),
                0,
                "Sprite at Y=255 should not be in secondary OAM on scanline {scanline}"
            );
            assert!(
                !evaluator.sprite_zero_in_range(),
                "Sprite 0 at Y=255 should not be in range on scanline {scanline}"
            );
        }
    }

    #[test]
    fn test_sprite_evaluator_y_239_skipped() {
        let mut evaluator = SpriteEvaluator::new();
        let mut secondary_oam = SecondaryOam::new();

        // Y=239 means sprite_top = 240, which is >= 240, so it should be skipped
        let mut oam_data = vec![0xFF; 256];
        oam_data[0] = 239; // Sprite 0 at Y=239

        evaluator.start_evaluation();

        // Evaluate for any visible scanline - sprite should be skipped
        evaluator.evaluate_step(&oam_data, 100, 8, &mut secondary_oam);

        assert_eq!(
            secondary_oam.count(),
            0,
            "Sprite at Y=239 should not be in secondary OAM"
        );
        assert!(
            !evaluator.sprite_zero_in_range(),
            "Sprite 0 at Y=239 should not be in range"
        );
    }
}
