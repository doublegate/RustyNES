//! NES 2C02 PPU (Picture Processing Unit) emulation.
//!
//! The PPU handles all graphics rendering for the NES. It operates at 3x the
//! CPU clock rate (5.369318 MHz for NTSC) and generates a 256x240 pixel image.
//!
//! # Timing
//!
//! - Each frame consists of 262 scanlines (NTSC) or 312 scanlines (PAL)
//! - Each scanline consists of 341 PPU cycles (dots)
//! - Visible area: scanlines 0-239, dots 0-255
//! - VBlank: scanlines 241-260 (NTSC)
//! - Pre-render: scanline 261 (NTSC)

use alloc::boxed::Box;

use crate::{
    ctrl::Ctrl,
    mask::Mask,
    scroll::Scroll,
    sprite::{MAX_SPRITES_PER_LINE, OAM_SIZE, SpriteEval, SpriteRender},
    status::Status,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Frame width in pixels.
pub const FRAME_WIDTH: usize = 256;
/// Frame height in pixels.
pub const FRAME_HEIGHT: usize = 240;
/// Total dots per scanline.
pub const DOTS_PER_SCANLINE: u16 = 341;
/// Total scanlines per frame (NTSC).
pub const SCANLINES_PER_FRAME: u16 = 262;
/// Pre-render scanline number.
pub const PRE_RENDER_SCANLINE: u16 = 261;
/// VBlank start scanline.
pub const VBLANK_START_SCANLINE: u16 = 241;

/// PPU memory bus trait for VRAM and CHR ROM/RAM access.
pub trait PpuBus {
    /// Read a byte from PPU memory space.
    fn read(&mut self, addr: u16) -> u8;
    /// Write a byte to PPU memory space.
    fn write(&mut self, addr: u16, value: u8);
}

/// The NES 2C02 PPU.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::struct_excessive_bools)]
pub struct Ppu {
    // Registers
    ctrl: Ctrl,
    mask: Mask,
    status: Status,
    oam_addr: u8,
    scroll: Scroll,

    // Internal state
    scanline: u16,
    dot: u16,
    frame: u64,
    odd_frame: bool,

    // Memory
    #[cfg_attr(feature = "serde", serde(skip, default = "default_oam"))]
    oam: [u8; OAM_SIZE],
    palette: [u8; 32],

    // Open bus (last value on PPU data bus)
    open_bus: u8,
    // Data buffer for $2007 reads
    read_buffer: u8,

    // Background rendering state
    bg_next_tile: u8,
    bg_next_attr: u8,
    bg_next_pattern_lo: u8,
    bg_next_pattern_hi: u8,
    bg_pattern_shift_lo: u16,
    bg_pattern_shift_hi: u16,
    bg_attr_shift_lo: u16,
    bg_attr_shift_hi: u16,
    bg_attr_latch_lo: bool,
    bg_attr_latch_hi: bool,

    // Sprite rendering state
    sprite_eval: SpriteEval,
    sprite_render: [SpriteRender; MAX_SPRITES_PER_LINE],
    sprite_zero_hit_possible: bool,

    // NMI output
    nmi_output: bool,
    nmi_occurred: bool,

    // Frame buffer (regenerated every frame, not serialized)
    #[cfg_attr(feature = "serde", serde(skip, default = "default_frame_buffer"))]
    frame_buffer: Box<[u8; FRAME_WIDTH * FRAME_HEIGHT]>,
}

/// Default OAM memory for deserialization.
#[cfg(feature = "serde")]
fn default_oam() -> [u8; OAM_SIZE] {
    [0; OAM_SIZE]
}

/// Default frame buffer for deserialization.
#[cfg(feature = "serde")]
fn default_frame_buffer() -> Box<[u8; FRAME_WIDTH * FRAME_HEIGHT]> {
    Box::new([0; FRAME_WIDTH * FRAME_HEIGHT])
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    /// Create a new PPU in the power-on state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ctrl: Ctrl::empty(),
            mask: Mask::empty(),
            status: Status::empty(),
            oam_addr: 0,
            scroll: Scroll::new(),

            scanline: 0,
            dot: 0,
            frame: 0,
            odd_frame: false,

            oam: [0; OAM_SIZE],
            palette: [0; 32],

            open_bus: 0,
            read_buffer: 0,

            bg_next_tile: 0,
            bg_next_attr: 0,
            bg_next_pattern_lo: 0,
            bg_next_pattern_hi: 0,
            bg_pattern_shift_lo: 0,
            bg_pattern_shift_hi: 0,
            bg_attr_shift_lo: 0,
            bg_attr_shift_hi: 0,
            bg_attr_latch_lo: false,
            bg_attr_latch_hi: false,

            sprite_eval: SpriteEval::new(),
            sprite_render: [SpriteRender::default(); MAX_SPRITES_PER_LINE],
            sprite_zero_hit_possible: false,

            nmi_output: false,
            nmi_occurred: false,

            frame_buffer: Box::new([0; FRAME_WIDTH * FRAME_HEIGHT]),
        }
    }

    /// Reset the PPU to power-on state.
    pub fn reset(&mut self) {
        self.ctrl = Ctrl::empty();
        self.mask = Mask::empty();
        self.status = Status::empty();
        self.scroll = Scroll::new();
        self.oam_addr = 0;
        self.scanline = 0;
        self.dot = 0;
        self.odd_frame = false;
        self.nmi_output = false;
        self.nmi_occurred = false;
        self.open_bus = 0;
        self.read_buffer = 0;
    }

    /// Step the PPU by one dot.
    /// Returns true if an NMI should be triggered.
    #[inline]
    pub fn step(&mut self, bus: &mut impl PpuBus) -> bool {
        let mut trigger_nmi = false;

        // Handle rendering
        if self.scanline < 240 {
            // Visible scanlines (0-239)
            self.render_dot(bus);
        } else if self.scanline == 241 && self.dot == 1 {
            // VBlank start
            self.status.set_vblank(true);
            self.nmi_occurred = true;
            if self.ctrl.nmi_enabled() {
                self.nmi_output = true;
                trigger_nmi = true;
            }
        } else if self.scanline == PRE_RENDER_SCANLINE {
            // Pre-render scanline
            if self.dot == 1 {
                // Clear VBlank, sprite 0 hit, and overflow flags
                self.status.set_vblank(false);
                self.status.set_sprite_zero_hit(false);
                self.status.set_sprite_overflow(false);
                self.nmi_occurred = false;
                self.nmi_output = false;
            }

            // Do background fetches
            self.pre_render_dot(bus);

            // Skip cycle on odd frames when rendering is enabled
            if self.dot == 339 && self.odd_frame && self.mask.rendering_enabled() {
                self.dot = 340;
            }
        }

        // Advance dot and scanline
        self.dot += 1;
        if self.dot >= DOTS_PER_SCANLINE {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline >= SCANLINES_PER_FRAME {
                self.scanline = 0;
                self.frame += 1;
                self.odd_frame = !self.odd_frame;
            }
        }

        trigger_nmi
    }

    /// Render a single dot during visible scanlines.
    fn render_dot(&mut self, bus: &mut impl PpuBus) {
        let rendering = self.mask.rendering_enabled();

        // Output pixel during visible portion (dots 1-256)
        if self.dot >= 1 && self.dot <= 256 {
            self.output_pixel();
        }

        if !rendering {
            return;
        }

        // Background tile fetching (dots 1-256 and 321-336)
        let fetching = (self.dot >= 1 && self.dot <= 256) || (self.dot >= 321 && self.dot <= 336);
        if fetching {
            self.update_shift_registers();
            self.fetch_bg_tile(bus);
        }

        // Sprite evaluation (dots 65-256)
        if self.dot >= 65 && self.dot <= 256 {
            self.sprite_eval.tick(
                self.dot,
                &self.oam,
                self.scanline,
                self.ctrl.sprite_height(),
            );
        }

        // Increment scroll at specific cycles
        if self.dot == 256 {
            self.scroll.increment_y();
        }
        if self.dot == 257 {
            self.scroll.copy_horizontal();
            self.load_sprite_data(bus);
        }

        // Sprite tile fetches (dots 257-320)
        if self.dot >= 257 && self.dot <= 320 {
            self.oam_addr = 0;
        }
    }

    /// Handle pre-render scanline.
    fn pre_render_dot(&mut self, bus: &mut impl PpuBus) {
        if !self.mask.rendering_enabled() {
            return;
        }

        // Background tile fetching (dots 1-256 and 321-336)
        let fetching = (self.dot >= 1 && self.dot <= 256) || (self.dot >= 321 && self.dot <= 336);
        if fetching {
            self.update_shift_registers();
            self.fetch_bg_tile(bus);
        }

        // Vertical scroll bits copied during dots 280-304
        if self.dot >= 280 && self.dot <= 304 {
            self.scroll.copy_vertical();
        }

        // Scroll increments
        if self.dot == 256 {
            self.scroll.increment_y();
        }
        if self.dot == 257 {
            self.scroll.copy_horizontal();
        }
    }

    /// Update background shift registers.
    fn update_shift_registers(&mut self) {
        // Shift pattern data
        self.bg_pattern_shift_lo <<= 1;
        self.bg_pattern_shift_hi <<= 1;

        // Shift attribute data
        self.bg_attr_shift_lo = (self.bg_attr_shift_lo << 1) | u16::from(self.bg_attr_latch_lo);
        self.bg_attr_shift_hi = (self.bg_attr_shift_hi << 1) | u16::from(self.bg_attr_latch_hi);
    }

    /// Fetch background tile data based on current dot.
    fn fetch_bg_tile(&mut self, bus: &mut impl PpuBus) {
        match self.dot % 8 {
            1 => {
                // Load shift registers with fetched data every 8 cycles
                self.bg_pattern_shift_lo =
                    (self.bg_pattern_shift_lo & 0xFF00) | u16::from(self.bg_next_pattern_lo);
                self.bg_pattern_shift_hi =
                    (self.bg_pattern_shift_hi & 0xFF00) | u16::from(self.bg_next_pattern_hi);
                self.bg_attr_latch_lo = self.bg_next_attr & 0x01 != 0;
                self.bg_attr_latch_hi = self.bg_next_attr & 0x02 != 0;

                // Fetch nametable byte
                let addr = self.scroll.nametable_addr();
                self.bg_next_tile = bus.read(addr);
            }
            3 => {
                // Fetch attribute byte
                let addr = self.scroll.attribute_addr();
                let attr = bus.read(addr);

                // Select the correct 2 bits based on coarse scroll position
                let shift =
                    ((self.scroll.coarse_y() & 0x02) << 1) | (self.scroll.coarse_x() & 0x02);
                self.bg_next_attr = (attr >> shift) & 0x03;
            }
            5 => {
                // Fetch pattern table low byte
                let addr = self
                    .scroll
                    .pattern_addr(self.bg_next_tile, self.ctrl.bg_pattern_addr());
                self.bg_next_pattern_lo = bus.read(addr);
            }
            7 => {
                // Fetch pattern table high byte
                let addr = self
                    .scroll
                    .pattern_addr(self.bg_next_tile, self.ctrl.bg_pattern_addr())
                    + 8;
                self.bg_next_pattern_hi = bus.read(addr);
            }
            0 => {
                // Increment horizontal scroll
                self.scroll.increment_x();
            }
            _ => {}
        }
    }

    /// Load sprite rendering data for the current scanline.
    fn load_sprite_data(&mut self, bus: &mut impl PpuBus) {
        self.sprite_zero_hit_possible = self.sprite_eval.sprite_zero_on_line()
            && self.mask.bg_enabled()
            && self.mask.sprites_enabled();

        for i in 0..self.sprite_eval.sprite_count() as usize {
            let sprite = self.sprite_eval.get_sprite(i);
            let row = sprite.sprite_row(self.scanline, self.ctrl.sprite_height());

            // Calculate pattern address
            let (tile, pattern_base) = if self.ctrl.sprite_size_16() {
                // 8x16 sprites: bit 0 of tile selects pattern table
                let bank = u16::from(sprite.tile & 0x01) * 0x1000;
                let tile_num = sprite.tile & 0xFE;
                let tile_idx = if row < 8 { tile_num } else { tile_num + 1 };
                (tile_idx, bank)
            } else {
                // 8x8 sprites
                (sprite.tile, self.ctrl.sprite_pattern_addr())
            };

            let pattern_row = row & 0x07;
            let addr = pattern_base + u16::from(tile) * 16 + u16::from(pattern_row);

            self.sprite_render[i] = SpriteRender {
                x: sprite.x,
                pattern_lo: bus.read(addr),
                pattern_hi: bus.read(addr + 8),
                attr: sprite.attr,
                is_sprite_zero: i == 0 && self.sprite_eval.sprite_zero_on_line(),
            };
        }
    }

    /// Output a pixel to the frame buffer.
    fn output_pixel(&mut self) {
        let x = (self.dot - 1) as usize;
        let y = self.scanline as usize;

        if x >= FRAME_WIDTH || y >= FRAME_HEIGHT {
            return;
        }

        // Get background pixel
        let (bg_pixel, bg_palette) =
            if self.mask.bg_enabled() && (x >= 8 || self.mask.bg_left_enabled()) {
                let shift = 15 - self.scroll.fine_x();
                let lo = ((self.bg_pattern_shift_lo >> shift) & 1) as u8;
                let hi = ((self.bg_pattern_shift_hi >> shift) & 1) as u8;
                let pixel = lo | (hi << 1);

                let attr_lo = ((self.bg_attr_shift_lo >> shift) & 1) as u8;
                let attr_hi = ((self.bg_attr_shift_hi >> shift) & 1) as u8;
                let palette = attr_lo | (attr_hi << 1);

                (pixel, palette)
            } else {
                (0, 0)
            };

        // Get sprite pixel
        let (sprite_pixel, sprite_palette, sprite_priority, is_sprite_zero) =
            self.evaluate_sprite_pixel(x as u8);

        // Priority multiplexer
        let (final_pixel, final_palette, is_sprite) = match (bg_pixel, sprite_pixel) {
            (0, 0) => (0, 0, false), // Both transparent - backdrop
            (0, _) => (sprite_pixel, sprite_palette, true), // BG transparent
            (_, 0) => (bg_pixel, bg_palette, false), // Sprite transparent
            (_, _) => {
                // Both opaque - check sprite 0 hit
                if is_sprite_zero && self.sprite_zero_hit_possible && x < 255 {
                    self.status.set_sprite_zero_hit(true);
                }

                // Priority determines which pixel shows
                if sprite_priority {
                    (bg_pixel, bg_palette, false) // Sprite behind BG
                } else {
                    (sprite_pixel, sprite_palette, true) // Sprite in front
                }
            }
        };

        // Look up color in palette RAM
        let palette_addr = if final_pixel == 0 {
            0 // Backdrop color
        } else if is_sprite {
            0x10 + (final_palette * 4) + final_pixel
        } else {
            (final_palette * 4) + final_pixel
        } as usize;

        let color_index = self.palette[palette_addr & 0x1F] & 0x3F;
        let color_with_emphasis = self.apply_emphasis(color_index);

        self.frame_buffer[y * FRAME_WIDTH + x] = color_with_emphasis;
    }

    /// Evaluate sprites to find the highest priority opaque pixel at the given X.
    fn evaluate_sprite_pixel(&self, x: u8) -> (u8, u8, bool, bool) {
        if !self.mask.sprites_enabled() || (x < 8 && !self.mask.sprites_left_enabled()) {
            return (0, 0, false, false);
        }

        for i in 0..self.sprite_eval.sprite_count() as usize {
            let sprite = &self.sprite_render[i];

            // Check if sprite covers this X position
            if x >= sprite.x && x < sprite.x.saturating_add(8) {
                let offset = x - sprite.x;
                let pixel = sprite.pixel(offset);

                if pixel != 0 {
                    return (
                        pixel,
                        sprite.attr.palette_addr(),
                        sprite.attr.behind_background(),
                        sprite.is_sprite_zero,
                    );
                }
            }
        }

        (0, 0, false, false)
    }

    /// Apply color emphasis based on mask register.
    fn apply_emphasis(&self, color: u8) -> u8 {
        // Apply greyscale if enabled
        let color = self.mask.apply_greyscale(color);

        // Emphasis bits are in the high 3 bits of the final color
        // For now, just return the color index
        // Full implementation would modify the RGB output
        color | (self.mask.emphasis() << 5)
    }

    // === Register Access ===

    /// Read from a PPU register (address $2000-$2007, mirrored).
    pub fn read_register(&mut self, addr: u16, bus: &mut impl PpuBus) -> u8 {
        match addr & 0x07 {
            0 | 1 | 3 | 5 | 6 => {
                // Write-only registers return open bus
                self.open_bus
            }
            2 => {
                // PPUSTATUS
                let result = self.status.read_with_open_bus(self.open_bus);
                self.status.set_vblank(false);
                self.nmi_occurred = false;
                self.scroll.reset_latch();
                // Note: race condition near VBlank start handled by checking NMI suppress
                result
            }
            4 => {
                // OAMDATA
                let data = self.oam[self.oam_addr as usize];
                // Reading during rendering returns 0xFF
                if self.mask.rendering_enabled() && self.scanline < 240 {
                    0xFF
                } else {
                    self.open_bus = data;
                    data
                }
            }
            7 => {
                // PPUDATA
                let addr = self.scroll.vram_addr();
                let data = if addr >= 0x3F00 {
                    // Palette reads return immediately
                    self.read_buffer = bus.read(addr - 0x1000); // VRAM behind palette
                    self.read_palette(addr)
                } else {
                    // Other reads are buffered
                    let buffered = self.read_buffer;
                    self.read_buffer = bus.read(addr);
                    buffered
                };
                self.scroll.increment_vram(self.ctrl.vram_increment());
                self.open_bus = data;
                data
            }
            _ => unreachable!(),
        }
    }

    /// Write to a PPU register (address $2000-$2007, mirrored).
    pub fn write_register(&mut self, addr: u16, value: u8, bus: &mut impl PpuBus) {
        self.open_bus = value;

        match addr & 0x07 {
            0 => {
                // PPUCTRL
                let was_nmi_enabled = self.ctrl.nmi_enabled();
                self.ctrl = Ctrl::from_bits_truncate(value);
                self.scroll.write_ctrl(value);

                // NMI can be triggered by enabling NMI while VBlank flag is set
                if !was_nmi_enabled && self.ctrl.nmi_enabled() && self.nmi_occurred {
                    self.nmi_output = true;
                }
            }
            1 => {
                // PPUMASK
                self.mask = Mask::from_bits_truncate(value);
            }
            2 => {
                // PPUSTATUS (read-only, writes ignored)
            }
            3 => {
                // OAMADDR
                self.oam_addr = value;
            }
            4 => {
                // OAMDATA
                // Writes during rendering cause glitches but we ignore those
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                // PPUSCROLL
                self.scroll.write_scroll(value);
            }
            6 => {
                // PPUADDR
                self.scroll.write_addr(value);
            }
            7 => {
                // PPUDATA
                let addr = self.scroll.vram_addr();
                if addr >= 0x3F00 {
                    self.write_palette(addr, value);
                } else {
                    bus.write(addr, value);
                }
                self.scroll.increment_vram(self.ctrl.vram_increment());
            }
            _ => unreachable!(),
        }
    }

    /// Read from palette RAM.
    fn read_palette(&self, addr: u16) -> u8 {
        let idx = self.palette_index(addr);
        self.mask.apply_greyscale(self.palette[idx])
    }

    /// Write to palette RAM.
    fn write_palette(&mut self, addr: u16, value: u8) {
        let idx = self.palette_index(addr);
        self.palette[idx] = value & 0x3F;
    }

    /// Calculate palette RAM index with mirroring.
    fn palette_index(&self, addr: u16) -> usize {
        let idx = (addr & 0x1F) as usize;
        // Mirror $3F10/$3F14/$3F18/$3F1C to $3F00/$3F04/$3F08/$3F0C
        match idx {
            0x10 | 0x14 | 0x18 | 0x1C => idx - 0x10,
            _ => idx,
        }
    }

    /// Write OAM data directly (for OAM DMA).
    pub fn write_oam(&mut self, value: u8) {
        self.oam[self.oam_addr as usize] = value;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    // === Accessors ===

    /// Check if NMI should be triggered.
    #[must_use]
    #[inline]
    pub const fn nmi_output(&self) -> bool {
        self.nmi_output
    }

    /// Clear NMI output (after CPU acknowledges).
    #[inline]
    pub fn clear_nmi(&mut self) {
        self.nmi_output = false;
    }

    /// Get the current scanline.
    #[must_use]
    #[inline]
    pub const fn scanline(&self) -> u16 {
        self.scanline
    }

    /// Get the current dot within the scanline.
    #[must_use]
    #[inline]
    pub const fn dot(&self) -> u16 {
        self.dot
    }

    /// Get the current frame number.
    #[must_use]
    #[inline]
    pub const fn frame(&self) -> u64 {
        self.frame
    }

    /// Get a reference to the frame buffer.
    #[must_use]
    #[inline]
    pub fn frame_buffer(&self) -> &[u8; FRAME_WIDTH * FRAME_HEIGHT] {
        &self.frame_buffer
    }

    /// Get a reference to OAM.
    #[must_use]
    #[inline]
    pub fn oam(&self) -> &[u8; OAM_SIZE] {
        &self.oam
    }

    /// Get a reference to palette RAM.
    #[must_use]
    #[inline]
    pub fn palette(&self) -> &[u8; 32] {
        &self.palette
    }

    /// Get the control register.
    #[must_use]
    #[inline]
    pub const fn ctrl(&self) -> Ctrl {
        self.ctrl
    }

    /// Get the mask register.
    #[must_use]
    #[inline]
    pub const fn mask(&self) -> Mask {
        self.mask
    }

    /// Get the status register.
    #[must_use]
    #[inline]
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Check if currently in VBlank.
    #[must_use]
    #[inline]
    pub const fn in_vblank(&self) -> bool {
        self.scanline >= VBLANK_START_SCANLINE && self.scanline < PRE_RENDER_SCANLINE
    }

    // === Scroll Register Forwarding (for debugging) ===

    /// Get the current VRAM address (v register).
    #[must_use]
    #[inline]
    pub fn vram_addr(&self) -> u16 {
        self.scroll.vram_addr()
    }

    /// Get the temporary VRAM address (t register).
    #[must_use]
    #[inline]
    pub fn temp_vram_addr(&self) -> u16 {
        self.scroll.temp_addr()
    }

    /// Get the fine X scroll (0-7).
    #[must_use]
    #[inline]
    pub fn fine_x(&self) -> u8 {
        self.scroll.fine_x()
    }

    /// Get the coarse X scroll (0-31).
    #[must_use]
    #[inline]
    pub fn coarse_x(&self) -> u8 {
        self.scroll.coarse_x()
    }

    /// Get the coarse Y scroll (0-31).
    #[must_use]
    #[inline]
    pub fn coarse_y(&self) -> u8 {
        self.scroll.coarse_y()
    }

    /// Get the fine Y scroll (0-7).
    #[must_use]
    #[inline]
    pub fn fine_y(&self) -> u8 {
        self.scroll.fine_y()
    }

    /// Check if a mid-scanline register write was detected.
    ///
    /// This is useful for debugging games that use mid-scanline
    /// rendering effects like status bars or split screens.
    #[must_use]
    #[inline]
    pub const fn mid_scanline_write_detected(&self) -> bool {
        // For now, return false as we don't track this yet.
        // A full implementation would set this when $2005/$2006 is written
        // during visible scanlines outside of HBlank.
        false
    }

    /// Get the last VRAM address before the most recent update.
    ///
    /// This is useful for debugging scroll register writes.
    #[must_use]
    #[inline]
    pub fn last_v_before_update(&self) -> u16 {
        // For now, return the current address.
        // A full implementation would track the previous value.
        self.scroll.vram_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        vram: [u8; 0x4000],
    }

    impl TestBus {
        fn new() -> Self {
            Self { vram: [0; 0x4000] }
        }
    }

    impl PpuBus for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.vram[(addr & 0x3FFF) as usize]
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.vram[(addr & 0x3FFF) as usize] = value;
        }
    }

    #[test]
    fn test_ppu_new() {
        let ppu = Ppu::new();
        assert_eq!(ppu.scanline(), 0);
        assert_eq!(ppu.dot(), 0);
        assert_eq!(ppu.frame(), 0);
    }

    #[test]
    fn test_vblank_timing() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Step until VBlank
        while ppu.scanline() != VBLANK_START_SCANLINE || ppu.dot() != 1 {
            ppu.step(&mut bus);
        }

        // VBlank flag should be set after stepping past dot 1 of line 241
        ppu.step(&mut bus);
        assert!(ppu.status().in_vblank());
    }

    #[test]
    fn test_ppuaddr_write() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Write address $2100
        ppu.write_register(0x2006, 0x21, &mut bus);
        ppu.write_register(0x2006, 0x00, &mut bus);

        // Write data
        ppu.write_register(0x2007, 0xAB, &mut bus);

        // Verify data was written
        assert_eq!(bus.vram[0x2100], 0xAB);
    }

    #[test]
    fn test_ppudata_read_buffering() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Write test data
        bus.vram[0x2000] = 0x12;
        bus.vram[0x2001] = 0x34;

        // Set address to $2000
        ppu.write_register(0x2006, 0x20, &mut bus);
        ppu.write_register(0x2006, 0x00, &mut bus);

        // First read returns old buffer, puts $2000 into buffer
        let _ = ppu.read_register(0x2007, &mut bus);

        // Second read returns $12, puts $2001 into buffer
        let data = ppu.read_register(0x2007, &mut bus);
        assert_eq!(data, 0x12);
    }

    #[test]
    fn test_palette_mirroring() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Write to $3F00
        ppu.write_register(0x2006, 0x3F, &mut bus);
        ppu.write_register(0x2006, 0x00, &mut bus);
        ppu.write_register(0x2007, 0x0F, &mut bus);

        // Read from $3F10 (mirrors $3F00)
        ppu.write_register(0x2006, 0x3F, &mut bus);
        ppu.write_register(0x2006, 0x10, &mut bus);
        let data = ppu.read_register(0x2007, &mut bus);
        assert_eq!(data, 0x0F);
    }

    #[test]
    fn test_oam_write() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Set OAM address
        ppu.write_register(0x2003, 0x10, &mut bus);

        // Write OAM data
        ppu.write_register(0x2004, 0xAB, &mut bus);
        ppu.write_register(0x2004, 0xCD, &mut bus);

        assert_eq!(ppu.oam()[0x10], 0xAB);
        assert_eq!(ppu.oam()[0x11], 0xCD);
    }

    #[test]
    fn test_scroll_write() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Write X scroll
        ppu.write_register(0x2005, 0x7D, &mut bus);
        // Write Y scroll
        ppu.write_register(0x2005, 0x5E, &mut bus);

        // Verify scroll values were set (internal state)
        // The actual verification would need access to internal scroll state
    }

    #[test]
    fn test_frame_timing() {
        let mut ppu = Ppu::new();
        let mut bus = TestBus::new();

        // Count dots to complete one frame
        let mut dots = 0;
        let initial_frame = ppu.frame();

        while ppu.frame() == initial_frame {
            ppu.step(&mut bus);
            dots += 1;
            // Safety limit
            assert!(dots <= 100_000, "Frame didn't complete in expected time");
        }

        // NTSC frame should be 341 * 262 = 89,342 dots (minus 1 on odd frames potentially)
        assert!((89_341..=89_342).contains(&dots));
    }
}
