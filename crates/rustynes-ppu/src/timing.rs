//! PPU timing and scanline state machine
//!
//! The NES PPU operates on a cycle-by-cycle (dot) basis:
//! - 341 dots per scanline (NTSC)
//! - 262 scanlines per frame (NTSC)
//! - 29,780 CPU cycles per frame (89,341 PPU dots)
//!
//! # Scanline Types
//!
//! ```text
//! Scanline   Dots    Description
//! --------   ----    -----------
//! 0-239      0-340   Visible scanlines (rendering)
//! 240        0-340   Post-render scanline (idle)
//! 241        0-340   VBlank start (dot 1: set VBlank flag, trigger NMI)
//! 242-260    0-340   VBlank scanlines (idle)
//! 261        0-340   Pre-render scanline (clear VBlank, sprite flags)
//! ```
//!
//! # Rendering Timing (Scanlines 0-239, 261)
//!
//! ```text
//! Dot        Action
//! ---        ------
//! 0          Idle
//! 1-256      Fetch tile data, render pixels
//! 257        Copy horizontal scroll from t to v
//! 258-320    Sprite fetching for next scanline
//! 321-336    Fetch first two tiles of next scanline
//! 337-340    Unknown fetches (nametable bytes)
//! ```
//!
//! # Odd Frame Skip
//!
//! On odd frames, if rendering is enabled, dot 0 of scanline 0 is skipped.
//! This makes odd frames 1 dot (1/3 CPU cycle) shorter than even frames.

/// PPU timing state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// Current scanline (0-261)
    scanline: u16,
    /// Current dot/cycle within scanline (0-340)
    dot: u16,
    /// Frame number (odd/even affects timing)
    frame: u64,
}

impl Timing {
    /// Create new timing state
    pub fn new() -> Self {
        Self {
            scanline: 0,
            dot: 0,
            frame: 0,
        }
    }

    /// Get current scanline
    #[inline]
    pub fn scanline(&self) -> u16 {
        self.scanline
    }

    /// Get current dot
    #[inline]
    pub fn dot(&self) -> u16 {
        self.dot
    }

    /// Get current frame number
    #[inline]
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Check if on an odd frame
    #[inline]
    pub fn is_odd_frame(&self) -> bool {
        self.frame % 2 == 1
    }

    /// Check if on visible scanline (0-239)
    #[inline]
    pub fn is_visible_scanline(&self) -> bool {
        self.scanline < 240
    }

    /// Check if on pre-render scanline (261)
    #[inline]
    pub fn is_prerender_scanline(&self) -> bool {
        self.scanline == 261
    }

    /// Check if on post-render scanline (240)
    #[inline]
    pub fn is_postrender_scanline(&self) -> bool {
        self.scanline == 240
    }

    /// Check if in VBlank period (scanlines 241-260)
    #[inline]
    pub fn is_vblank_scanline(&self) -> bool {
        self.scanline >= 241 && self.scanline <= 260
    }

    /// Check if on rendering scanline (visible or pre-render)
    #[inline]
    pub fn is_rendering_scanline(&self) -> bool {
        self.is_visible_scanline() || self.is_prerender_scanline()
    }

    /// Check if in visible dot range (1-256)
    #[inline]
    pub fn is_visible_dot(&self) -> bool {
        self.dot >= 1 && self.dot <= 256
    }

    /// Check if in prefetch dot range (321-336)
    #[inline]
    pub fn is_prefetch_dot(&self) -> bool {
        self.dot >= 321 && self.dot <= 336
    }

    /// Check if on VBlank set dot (241, 1)
    #[inline]
    pub fn is_vblank_set_dot(&self) -> bool {
        self.scanline == 241 && self.dot == 1
    }

    /// Check if on VBlank clear dot (261, 1)
    #[inline]
    pub fn is_vblank_clear_dot(&self) -> bool {
        self.scanline == 261 && self.dot == 1
    }

    /// Check if on horizontal scroll copy dot (257)
    #[inline]
    pub fn is_hori_copy_dot(&self) -> bool {
        self.dot == 257
    }

    /// Check if in vertical scroll copy range (280-304 of pre-render)
    #[inline]
    pub fn is_vert_copy_range(&self) -> bool {
        self.is_prerender_scanline() && self.dot >= 280 && self.dot <= 304
    }

    /// Check if at sprite evaluation start (dot 65)
    #[inline]
    pub fn is_sprite_eval_start(&self) -> bool {
        self.dot == 65
    }

    /// Check if in sprite evaluation range (65-256)
    #[inline]
    pub fn is_sprite_eval_range(&self) -> bool {
        self.dot >= 65 && self.dot <= 256
    }

    /// Check if at sprite fetch start (dot 257)
    #[inline]
    pub fn is_sprite_fetch_start(&self) -> bool {
        self.dot == 257
    }

    /// Check if in sprite fetch range (257-320)
    #[inline]
    pub fn is_sprite_fetch_range(&self) -> bool {
        self.dot >= 257 && self.dot <= 320
    }

    /// Advance timing by one dot
    ///
    /// Returns true if a new frame started.
    pub fn tick(&mut self, rendering_enabled: bool) -> bool {
        self.dot += 1;

        // Handle odd frame skip
        // On odd frames, if rendering is enabled, the pre-render scanline (261) is one dot shorter.
        // Specifically, dot 339 is skipped.
        if self.scanline == 261 && self.dot == 339 && self.is_odd_frame() && rendering_enabled {
            self.dot = 340; // Skip dot 339
        }

        // End of scanline
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;

            // End of frame
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame = self.frame.wrapping_add(1);
                return true;
            }
        }

        false
    }

    /// Reset to power-up state
    pub fn reset(&mut self) {
        self.scanline = 0;
        self.dot = 0;
        self.frame = 0;
    }

    /// Set timing state (for testing/debugging)
    #[cfg(test)]
    pub fn set_state(&mut self, scanline: u16, dot: u16, frame: u64) {
        self.scanline = scanline;
        self.dot = dot;
        self.frame = frame;
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_tick() {
        let mut timing = Timing::new();

        assert_eq!(timing.scanline(), 0);
        assert_eq!(timing.dot(), 0);

        // Tick once
        timing.tick(false);
        assert_eq!(timing.dot(), 1);
        assert_eq!(timing.scanline(), 0);
    }

    #[test]
    fn test_timing_scanline_wrap() {
        let mut timing = Timing::new();
        timing.set_state(0, 340, 0);

        // Tick should wrap to next scanline
        timing.tick(false);
        assert_eq!(timing.scanline(), 1);
        assert_eq!(timing.dot(), 0);
    }

    #[test]
    fn test_timing_frame_wrap() {
        let mut timing = Timing::new();
        timing.set_state(261, 340, 0);

        // Tick should wrap to next frame
        let frame_ended = timing.tick(false);
        assert!(frame_ended);
        assert_eq!(timing.scanline(), 0);
        assert_eq!(timing.dot(), 0);
        assert_eq!(timing.frame(), 1);
    }

    #[test]
    fn test_odd_frame_skip() {
        let mut timing = Timing::new();

        // Even frame (0) - no skip
        timing.set_state(0, 0, 0);
        timing.tick(true);
        assert_eq!(timing.dot(), 1);

        // Odd frame (1) - skip dot 0
        timing.set_state(0, 0, 1);
        timing.tick(true);
        assert_eq!(timing.dot(), 2); // Skipped to dot 2

        // Odd frame but rendering disabled - no skip
        timing.set_state(0, 0, 1);
        timing.tick(false);
        assert_eq!(timing.dot(), 1); // No skip
    }

    #[test]
    fn test_scanline_type_checks() {
        let mut timing = Timing::new();

        // Visible scanline
        timing.set_state(100, 0, 0);
        assert!(timing.is_visible_scanline());
        assert!(!timing.is_vblank_scanline());
        assert!(!timing.is_prerender_scanline());
        assert!(timing.is_rendering_scanline());

        // Post-render scanline
        timing.set_state(240, 0, 0);
        assert!(!timing.is_visible_scanline());
        assert!(timing.is_postrender_scanline());
        assert!(!timing.is_rendering_scanline());

        // VBlank scanline
        timing.set_state(245, 0, 0);
        assert!(timing.is_vblank_scanline());
        assert!(!timing.is_visible_scanline());

        // Pre-render scanline
        timing.set_state(261, 0, 0);
        assert!(timing.is_prerender_scanline());
        assert!(!timing.is_visible_scanline());
        assert!(timing.is_rendering_scanline());
    }

    #[test]
    fn test_dot_range_checks() {
        let mut timing = Timing::new();

        // Visible dot
        timing.set_state(0, 100, 0);
        assert!(timing.is_visible_dot());

        // Not visible dot
        timing.set_state(0, 0, 0);
        assert!(!timing.is_visible_dot());
        timing.set_state(0, 257, 0);
        assert!(!timing.is_visible_dot());

        // Prefetch dots
        timing.set_state(0, 321, 0);
        assert!(timing.is_prefetch_dot());
        timing.set_state(0, 336, 0);
        assert!(timing.is_prefetch_dot());
        timing.set_state(0, 337, 0);
        assert!(!timing.is_prefetch_dot());
    }

    #[test]
    fn test_vblank_timing() {
        let mut timing = Timing::new();

        // VBlank set
        timing.set_state(241, 1, 0);
        assert!(timing.is_vblank_set_dot());

        // VBlank clear
        timing.set_state(261, 1, 0);
        assert!(timing.is_vblank_clear_dot());
    }

    #[test]
    fn test_scroll_copy_timing() {
        let mut timing = Timing::new();

        // Horizontal copy
        timing.set_state(0, 257, 0);
        assert!(timing.is_hori_copy_dot());

        // Vertical copy range
        timing.set_state(261, 280, 0);
        assert!(timing.is_vert_copy_range());
        timing.set_state(261, 304, 0);
        assert!(timing.is_vert_copy_range());
        timing.set_state(261, 305, 0);
        assert!(!timing.is_vert_copy_range());
    }

    #[test]
    fn test_sprite_timing() {
        let mut timing = Timing::new();

        // Sprite evaluation
        timing.set_state(0, 65, 0);
        assert!(timing.is_sprite_eval_start());
        assert!(timing.is_sprite_eval_range());

        timing.set_state(0, 200, 0);
        assert!(timing.is_sprite_eval_range());

        // Sprite fetch
        timing.set_state(0, 257, 0);
        assert!(timing.is_sprite_fetch_start());
        assert!(timing.is_sprite_fetch_range());

        timing.set_state(0, 300, 0);
        assert!(timing.is_sprite_fetch_range());
    }

    #[test]
    fn test_full_frame() {
        let mut timing = Timing::new();

        // Simulate full frame (341 * 262 = 89342 dots, minus 1 for odd frame skip)
        for _ in 0..(341 * 262 - 1) {
            timing.tick(false);
        }

        // Should be at end of frame
        assert_eq!(timing.scanline(), 261);
        assert_eq!(timing.dot(), 340);

        // One more tick should wrap to next frame
        let frame_ended = timing.tick(false);
        assert!(frame_ended);
        assert_eq!(timing.scanline(), 0);
        assert_eq!(timing.dot(), 0);
        assert_eq!(timing.frame(), 1);
    }

    #[test]
    fn test_reset() {
        let mut timing = Timing::new();
        timing.set_state(100, 200, 5);

        timing.reset();

        assert_eq!(timing.scanline(), 0);
        assert_eq!(timing.dot(), 0);
        assert_eq!(timing.frame(), 0);
    }
}
