//! PPU scrolling implementation (Loopy's model)
//!
//! Implements the internal VRAM address registers (v and t) and fine X scroll
//! for hardware scrolling. Based on Brad Taylor (Loopy)'s PPU scrolling document.
//!
//! # Register Layout
//!
//! Both v and t are 15-bit registers:
//!
//! ```text
//!  yyy NN YYYYY XXXXX
//!  ||| || ||||| +++++- Coarse X scroll (0-31)
//!  ||| || +++++------- Coarse Y scroll (0-31)
//!  ||| ++------------- Nametable select (0-3)
//!  +++---------------- Fine Y scroll (0-7)
//! ```

/// PPU scrolling registers
///
/// Implements Loopy's scrolling model with v, t, x, and w registers.
///
/// # Mid-Scanline Updates
///
/// Games use mid-scanline writes to $2005/$2006 for split-screen effects:
/// - Super Mario Bros. 3: Status bar at top, scrolling gameplay below
/// - Kirby's Adventure: Complex multi-layer scrolling
///
/// The second write to $2006 immediately copies t to v, which is used
/// for the next tile fetch. Games time this write during HBlank (after dot 256)
/// to achieve clean screen splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollRegisters {
    /// Current VRAM address (15 bits)
    v: u16,
    /// Temporary VRAM address (15 bits)
    t: u16,
    /// Fine X scroll (3 bits, 0-7)
    x: u8,
    /// Write toggle (first/second write to $2005/$2006)
    w: bool,
    /// Last v value before mid-scanline update (for debugging)
    last_v_before_update: u16,
    /// Flag indicating a mid-scanline write occurred this frame
    mid_scanline_write_detected: bool,
}

impl ScrollRegisters {
    /// Create new scroll registers (power-up state)
    pub fn new() -> Self {
        Self {
            v: 0,
            t: 0,
            x: 0,
            w: false,
            last_v_before_update: 0,
            mid_scanline_write_detected: false,
        }
    }

    /// Reset frame-specific state (call at start of each frame)
    ///
    /// Clears mid-scanline detection flag for the new frame.
    pub fn start_frame(&mut self) {
        self.mid_scanline_write_detected = false;
    }

    /// Check if a mid-scanline write was detected this frame
    #[inline]
    pub fn mid_scanline_write_detected(&self) -> bool {
        self.mid_scanline_write_detected
    }

    /// Get the last v value before a mid-scanline update (for debugging)
    #[inline]
    pub fn last_v_before_update(&self) -> u16 {
        self.last_v_before_update
    }

    /// Get temporary VRAM address (t register)
    #[inline]
    pub fn temp_vram_addr(&self) -> u16 {
        self.t
    }

    /// Get write toggle state
    #[inline]
    pub fn write_toggle(&self) -> bool {
        self.w
    }

    /// Record a mid-scanline write occurred
    ///
    /// Should be called by PPU when a write to $2005/$2006 happens during
    /// visible rendering (scanline 0-239, after dot 0).
    pub fn record_mid_scanline_write(&mut self) {
        self.last_v_before_update = self.v;
        self.mid_scanline_write_detected = true;
    }

    /// Get current VRAM address (v register)
    #[inline]
    pub fn vram_addr(&self) -> u16 {
        self.v
    }

    /// Get fine X scroll (0-7)
    #[inline]
    pub fn fine_x(&self) -> u8 {
        self.x
    }

    /// Get coarse X scroll (tile column 0-31)
    #[inline]
    pub fn coarse_x(&self) -> u8 {
        (self.v & 0x001F) as u8
    }

    /// Get coarse Y scroll (tile row 0-31)
    #[inline]
    pub fn coarse_y(&self) -> u8 {
        ((self.v & 0x03E0) >> 5) as u8
    }

    /// Get fine Y scroll (pixel row 0-7)
    #[inline]
    pub fn fine_y(&self) -> u8 {
        ((self.v & 0x7000) >> 12) as u8
    }

    /// Get nametable X bit
    #[inline]
    pub fn nametable_x(&self) -> u8 {
        ((self.v & 0x0400) >> 10) as u8
    }

    /// Get nametable Y bit
    #[inline]
    pub fn nametable_y(&self) -> u8 {
        ((self.v & 0x0800) >> 11) as u8
    }

    /// Write to PPUCTRL ($2000)
    ///
    /// Updates nametable select bits in t register.
    pub fn write_ppuctrl(&mut self, value: u8) {
        // t: ....BA.. ........ = d: ......BA
        self.t = (self.t & 0xF3FF) | (((value & 0x03) as u16) << 10);
    }

    /// Write to PPUSCROLL ($2005)
    ///
    /// First write: X scroll (coarse X and fine X)
    /// Second write: Y scroll (coarse Y and fine Y)
    pub fn write_ppuscroll(&mut self, value: u8) {
        if self.w {
            // Second write: Y scroll
            // t: .YYY.... ........ = d[2:0]
            // t: ........ YYYYY... = d[7:3]
            self.t = (self.t & 0x8FFF) | (((value & 0x07) as u16) << 12);
            self.t = (self.t & 0xFC1F) | (((value & 0xF8) as u16) << 2);
        } else {
            // First write: X scroll
            // t: ........ ...XXXXX = d[7:3]
            // x:       XXX         = d[2:0]
            self.t = (self.t & 0xFFE0) | ((value >> 3) as u16);
            self.x = value & 0x07;
        }

        self.w = !self.w;
    }

    /// Write to PPUADDR ($2006)
    ///
    /// First write: High byte (sets t[13:8])
    /// Second write: Low byte (sets t[7:0] and copies t to v)
    pub fn write_ppuaddr(&mut self, value: u8) {
        if self.w {
            // Second write: low byte
            // t: ........ AAAAAAAA = d[7:0]
            // v = t
            self.t = (self.t & 0xFF00) | (value as u16);
            self.v = self.t;
        } else {
            // First write: high byte
            // t: ..AAAAAA ........ = d[5:0]
            // t: .0...... ........ (clear bit 14)
            self.t = (self.t & 0x00FF) | (((value & 0x3F) as u16) << 8);
            self.t &= 0x7FFF;
        }

        self.w = !self.w;
    }

    /// Read from PPUSTATUS ($2002)
    ///
    /// Resets the write toggle.
    pub fn read_ppustatus(&mut self) {
        self.w = false;
    }

    /// Increment horizontal position (coarse X)
    ///
    /// Called every 8 dots during rendering.
    /// Wraps at nametable boundaries.
    pub fn increment_x(&mut self) {
        if (self.v & 0x001F) == 31 {
            // Coarse X = 31, wrap to 0 and switch horizontal nametable
            self.v &= !0x001F;
            self.v ^= 0x0400;
        } else {
            // Increment coarse X
            self.v += 1;
        }
    }

    /// Increment vertical position (fine Y and coarse Y)
    ///
    /// Called at dot 256 of each visible scanline.
    /// Handles fine Y overflow and nametable switching.
    pub fn increment_y(&mut self) {
        if (self.v & 0x7000) == 0x7000 {
            // Fine Y = 7, wrap and increment coarse Y
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5;

            if y == 29 {
                // Coarse Y = 29 (last visible row), wrap and switch vertical nametable
                y = 0;
                self.v ^= 0x0800;
            } else if y == 31 {
                // Coarse Y = 31 (out of bounds), wrap without switching nametable
                y = 0;
            } else {
                // Increment coarse Y
                y += 1;
            }

            self.v = (self.v & !0x03E0) | (y << 5);
        } else {
            // Increment fine Y
            self.v += 0x1000;
        }
    }

    /// Copy horizontal bits from t to v
    ///
    /// Called at dot 257 of each visible scanline.
    /// Resets horizontal scroll position.
    pub fn copy_horizontal(&mut self) {
        // v: ....F.. ...EDCBA = t: ....F.. ...EDCBA
        self.v = (self.v & 0xFBE0) | (self.t & 0x041F);
    }

    /// Copy vertical bits from t to v
    ///
    /// Called during dots 280-304 of pre-render scanline.
    /// Resets vertical scroll position for next frame.
    pub fn copy_vertical(&mut self) {
        // v: IHGF.ED CBA..... = t: IHGF.ED CBA.....
        self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
    }

    /// Increment VRAM address after $2007 access
    ///
    /// Increments by 1 (across) or 32 (down) based on PPUCTRL.
    pub fn increment_vram(&mut self, increment: u16) {
        self.v = self.v.wrapping_add(increment) & 0x7FFF;
    }

    /// Set VRAM address directly (for testing)
    #[cfg(test)]
    pub fn set_vram_addr(&mut self, addr: u16) {
        self.v = addr & 0x7FFF;
    }
}

impl Default for ScrollRegisters {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppuscroll_x_write() {
        let mut scroll = ScrollRegisters::new();

        // Write X scroll = 125 ($7D)
        scroll.write_ppuscroll(0x7D);

        // Coarse X = 125 / 8 = 15
        // Fine X = 125 % 8 = 5
        assert_eq!((scroll.t & 0x001F) as u8, 15);
        assert_eq!(scroll.x, 5);
        assert!(scroll.w);
    }

    #[test]
    fn test_ppuscroll_y_write() {
        let mut scroll = ScrollRegisters::new();

        // First write: X
        scroll.write_ppuscroll(0x00);
        // Second write: Y scroll = 94 ($5E)
        scroll.write_ppuscroll(0x5E);

        // Coarse Y = 94 / 8 = 11
        // Fine Y = 94 % 8 = 6
        assert_eq!(((scroll.t & 0x03E0) >> 5) as u8, 11);
        assert_eq!(((scroll.t & 0x7000) >> 12) as u8, 6);
        assert!(!scroll.w);
    }

    #[test]
    fn test_ppuaddr_write() {
        let mut scroll = ScrollRegisters::new();

        // Write $3F00
        scroll.write_ppuaddr(0x3F);
        assert!(scroll.w);

        scroll.write_ppuaddr(0x00);
        assert!(!scroll.w);
        assert_eq!(scroll.v, 0x3F00);
    }

    #[test]
    fn test_increment_x_no_wrap() {
        let mut scroll = ScrollRegisters::new();
        scroll.set_vram_addr(0x2000);

        scroll.increment_x();
        assert_eq!(scroll.v & 0x001F, 1);
    }

    #[test]
    fn test_increment_x_wrap_nametable() {
        let mut scroll = ScrollRegisters::new();
        scroll.set_vram_addr(0x201F); // Coarse X = 31

        scroll.increment_x();
        // Coarse X wraps to 0, nametable switches
        assert_eq!(scroll.v & 0x001F, 0);
        assert_eq!(scroll.v & 0x0400, 0x0400);
    }

    #[test]
    fn test_increment_y_fine() {
        let mut scroll = ScrollRegisters::new();
        scroll.set_vram_addr(0x0000); // Fine Y = 0

        scroll.increment_y();
        // Fine Y increments
        assert_eq!((scroll.v & 0x7000) >> 12, 1);
    }

    #[test]
    fn test_increment_y_wrap_coarse() {
        let mut scroll = ScrollRegisters::new();
        scroll.set_vram_addr(0x7000); // Fine Y = 7

        scroll.increment_y();
        // Fine Y wraps, coarse Y increments
        assert_eq!(scroll.v & 0x7000, 0);
        assert_eq!((scroll.v & 0x03E0) >> 5, 1);
    }

    #[test]
    fn test_increment_y_wrap_nametable() {
        let mut scroll = ScrollRegisters::new();
        scroll.set_vram_addr(0x73A0); // Fine Y = 7, Coarse Y = 29

        scroll.increment_y();
        // Fine Y wraps, coarse Y wraps, nametable switches
        assert_eq!(scroll.v & 0x7000, 0);
        assert_eq!((scroll.v & 0x03E0) >> 5, 0);
        assert_eq!(scroll.v & 0x0800, 0x0800);
    }

    #[test]
    fn test_copy_horizontal() {
        let mut scroll = ScrollRegisters::new();
        scroll.t = 0x041F; // Set horizontal bits in t
        scroll.v = 0x0000;

        scroll.copy_horizontal();
        assert_eq!(scroll.v & 0x041F, 0x041F);
    }

    #[test]
    fn test_copy_vertical() {
        let mut scroll = ScrollRegisters::new();
        scroll.t = 0x7BE0; // Set vertical bits in t
        scroll.v = 0x0000;

        scroll.copy_vertical();
        assert_eq!(scroll.v & 0x7BE0, 0x7BE0);
    }

    #[test]
    fn test_read_ppustatus_resets_latch() {
        let mut scroll = ScrollRegisters::new();

        scroll.write_ppuscroll(0x00);
        assert!(scroll.w);

        scroll.read_ppustatus();
        assert!(!scroll.w);
    }

    #[test]
    fn test_mid_scanline_detection_initial_state() {
        let scroll = ScrollRegisters::new();

        // Initially no mid-scanline write detected
        assert!(!scroll.mid_scanline_write_detected());
        assert_eq!(scroll.last_v_before_update(), 0);
    }

    #[test]
    fn test_mid_scanline_write_recording() {
        let mut scroll = ScrollRegisters::new();

        // Set up v to some value
        scroll.write_ppuaddr(0x21);
        scroll.write_ppuaddr(0x00); // v = $2100

        // Record a mid-scanline write
        scroll.record_mid_scanline_write();

        assert!(scroll.mid_scanline_write_detected());
        assert_eq!(scroll.last_v_before_update(), 0x2100);
    }

    #[test]
    fn test_start_frame_clears_mid_scanline_flag() {
        let mut scroll = ScrollRegisters::new();

        // Record a mid-scanline write
        scroll.record_mid_scanline_write();
        assert!(scroll.mid_scanline_write_detected());

        // Start new frame should clear the flag
        scroll.start_frame();
        assert!(!scroll.mid_scanline_write_detected());
    }

    #[test]
    fn test_temp_vram_addr_getter() {
        let mut scroll = ScrollRegisters::new();

        // First write to PPUADDR sets high byte of t
        scroll.write_ppuaddr(0x21);
        assert_eq!(scroll.temp_vram_addr() >> 8, 0x21);

        // Second write completes t and copies to v
        scroll.write_ppuaddr(0xAB);
        assert_eq!(scroll.temp_vram_addr(), 0x21AB);
        assert_eq!(scroll.vram_addr(), 0x21AB);
    }

    #[test]
    fn test_write_toggle_getter() {
        let mut scroll = ScrollRegisters::new();

        // Initially false
        assert!(!scroll.write_toggle());

        // After first write, should be true
        scroll.write_ppuscroll(0x00);
        assert!(scroll.write_toggle());

        // After second write, should be false again
        scroll.write_ppuscroll(0x00);
        assert!(!scroll.write_toggle());
    }

    #[test]
    fn test_mid_scanline_preserves_v_before_update() {
        let mut scroll = ScrollRegisters::new();

        // Set initial v value
        scroll.write_ppuaddr(0x23);
        scroll.write_ppuaddr(0x45); // v = $2345

        // Record first mid-scanline write
        scroll.record_mid_scanline_write();
        assert_eq!(scroll.last_v_before_update(), 0x2345);

        // Change v
        scroll.write_ppuaddr(0x20);
        scroll.write_ppuaddr(0x00); // v = $2000

        // Record second mid-scanline write
        scroll.record_mid_scanline_write();
        assert_eq!(scroll.last_v_before_update(), 0x2000);
    }
}
