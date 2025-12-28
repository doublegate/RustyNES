//! Main PPU (Picture Processing Unit) implementation
//!
//! The Ricoh 2C02 PPU is responsible for generating the video output
//! for the NES. It renders 256×240 pixel frames at 60Hz (NTSC).
//!
//! # Memory Map (PPU address space)
//!
//! ```text
//! $0000-$0FFF: Pattern Table 0 (CHR ROM/RAM, via mapper)
//! $1000-$1FFF: Pattern Table 1 (CHR ROM/RAM, via mapper)
//! $2000-$2FFF: Nametables (internal VRAM with mirroring)
//! $3F00-$3F1F: Palette RAM
//! ```
//!
//! # CPU Registers ($2000-$2007)
//!
//! ```text
//! $2000: PPUCTRL   - Control register
//! $2001: PPUMASK   - Mask register
//! $2002: PPUSTATUS - Status register
//! $2003: OAMADDR   - OAM address
//! $2004: OAMDATA   - OAM data
//! $2005: PPUSCROLL - Scroll position
//! $2006: PPUADDR   - VRAM address
//! $2007: PPUDATA   - VRAM data
//! ```

use crate::background::Background;
use crate::oam::{Oam, SecondaryOam};
use crate::registers::{PpuCtrl, PpuMask, PpuStatus};
use crate::scroll::ScrollRegisters;
use crate::sprites::{SpriteEvaluator, SpriteRenderer};
use crate::timing::Timing;
use crate::vram::{Mirroring, Vram};

/// Frame buffer width (256 pixels)
pub const FRAME_WIDTH: usize = 256;
/// Frame buffer height (240 pixels)
pub const FRAME_HEIGHT: usize = 240;
/// Frame buffer total size (256×240 = 61440 pixels)
pub const FRAME_SIZE: usize = FRAME_WIDTH * FRAME_HEIGHT;

/// PPU (Picture Processing Unit)
///
/// Implements the Ricoh 2C02 PPU for cycle-accurate NES emulation.
pub struct Ppu {
    // Registers
    ctrl: PpuCtrl,
    mask: PpuMask,
    status: PpuStatus,
    scroll: ScrollRegisters,

    // Memory
    vram: Vram,
    oam: Oam,

    // Rendering components
    background: Background,
    sprite_renderer: SpriteRenderer,
    sprite_evaluator: SpriteEvaluator,
    secondary_oam: SecondaryOam,

    // Timing
    timing: Timing,

    // Frame buffer (palette indices 0-63)
    frame_buffer: Vec<u8>,

    // Internal state
    vram_read_buffer: u8,
    open_bus_latch: u8,
    decay_counter: u32,
    nmi_pending: bool,
}

impl Ppu {
    /// Create new PPU
    pub fn new(mirroring: Mirroring) -> Self {
        Self {
            ctrl: PpuCtrl::empty(),
            mask: PpuMask::empty(),
            status: PpuStatus::empty(),
            scroll: ScrollRegisters::new(),
            vram: Vram::new(mirroring),
            oam: Oam::new(),
            background: Background::new(),
            sprite_renderer: SpriteRenderer::new(),
            sprite_evaluator: SpriteEvaluator::new(),
            secondary_oam: SecondaryOam::new(),
            timing: Timing::new(),
            frame_buffer: vec![0; FRAME_SIZE],
            vram_read_buffer: 0,
            open_bus_latch: 0,
            decay_counter: 0,
            nmi_pending: false,
        }
    }

    /// Refresh open bus decay counter (approx 1 second ~ 5.3M dots)
    #[inline]
    fn refresh_open_bus(&mut self) {
        self.decay_counter = 5_300_000;
    }

    /// Check if we're currently in a visible rendering position
    ///
    /// Returns true if:
    /// - We're on a visible scanline (0-239)
    /// - We're past dot 0 (rendering has started for this scanline)
    /// - Rendering is enabled
    ///
    /// This is used to detect mid-scanline scroll/address writes which
    /// are used by games for split-screen effects.
    #[inline]
    fn is_visible_rendering_position(&self) -> bool {
        self.mask.rendering_enabled() && self.timing.is_visible_scanline() && self.timing.dot() > 0
    }

    /// Read from PPU register (CPU memory map $2000-$2007)
    ///
    /// # Arguments
    ///
    /// * `addr` - Register address
    /// * `read_chr` - Callback to read CHR memory (mapper) for addresses < $2000
    pub fn read_register<F: FnMut(u16) -> u8>(&mut self, addr: u16, mut read_chr: F) -> u8 {
        match addr & 0x07 {
            // $2000: PPUCTRL (write-only) -> return open bus (do not refresh)
            0 => self.open_bus_latch,

            // $2001: PPUMASK (write-only) -> return open bus (do not refresh)
            1 => self.open_bus_latch,

            // $2002: PPUSTATUS
            2 => {
                // Reading $2002 only drives bits 7-5. Bits 4-0 are open bus (undriven).
                // Therefore, we do NOT refresh the decay counter, so bits 4-0 continue to decay.
                // (Technically bits 7-5 are refreshed, but our model has one counter).

                let status = self.status.bits();

                // Race condition: Reading $2002 on the exact cycle VBlank is set
                // suppresses the NMI. This happens at scanline 241, dot 1.
                if self.timing.scanline() == 241 && self.timing.dot() == 1 {
                    self.nmi_pending = false;
                }

                self.status.clear_vblank(); // Reading clears VBlank flag
                self.scroll.read_ppustatus(); // Reset write latch

                // Return status (bits 7-5) + open bus (bits 4-0)
                let result = (status & 0xE0) | (self.open_bus_latch & 0x1F);

                // Update latch with result (actually only bits 7-5 are new, 4-0 are preserved)
                self.open_bus_latch = result;

                result
            }

            // $2003: OAMADDR (write-only) -> return open bus (do not refresh)
            3 => self.open_bus_latch,

            // $2004: OAMDATA
            4 => {
                // Reading refreshes open bus
                self.refresh_open_bus();

                let data = self.oam.read();
                // Reading OAMDATA does NOT reliably update open bus on all revisions,
                // but usually it does. Most emulators update it.
                self.open_bus_latch = data;
                data
            }

            // $2005: PPUSCROLL (write-only) -> return open bus (do not refresh)
            5 => self.open_bus_latch,

            // $2006: PPUADDR (write-only) -> return open bus (do not refresh)
            6 => self.open_bus_latch,

            // $2007: PPUDATA
            7 => {
                // Reading refreshes open bus
                self.refresh_open_bus();

                let addr = self.scroll.vram_addr();

                // Read from CHR (mapper) or VRAM/Palette
                let data = if (addr & 0x3FFF) < 0x2000 {
                    read_chr(addr & 0x3FFF)
                } else {
                    self.vram.read(addr)
                };

                // Buffered read behavior
                let result = if addr >= 0x3F00 {
                    // Palette reads are immediate
                    // Bits 7-6 are open bus (from decay latch)
                    let pal_data = (data & 0x3F) | (self.open_bus_latch & 0xC0);

                    // Reading the palette also updates the VRAM read buffer with
                    // the contents of the mirrored nametable address ($2F00-$2FFF)
                    self.vram_read_buffer = self.vram.read(addr - 0x1000);

                    pal_data
                } else {
                    // Normal reads return previous buffer
                    let buffered = self.vram_read_buffer;
                    self.vram_read_buffer = data;
                    buffered
                };

                // Increment VRAM address
                let increment = self.ctrl.vram_increment();
                self.scroll.increment_vram(increment);

                // Update open bus latch with the value put on the bus
                self.open_bus_latch = result;

                result
            }
            _ => unreachable!(),
        }
    }
    /// Write to PPU register (CPU memory map $2000-$2007)
    ///
    /// # Arguments
    ///
    /// * `addr` - Register address
    /// * `value` - Value to write
    /// * `write_chr` - Callback to write CHR memory (mapper) for addresses < $2000
    pub fn write_register<F: FnMut(u16, u8)>(&mut self, addr: u16, value: u8, mut write_chr: F) {
        // Writing to any register updates the open bus latch and refreshes decay
        self.open_bus_latch = value;
        self.refresh_open_bus();

        match addr & 0x07 {
            // $2000: PPUCTRL
            0 => {
                self.ctrl = PpuCtrl::from_bits_truncate(value);
                self.scroll.write_ppuctrl(value);

                // Check NMI enable
                if self.ctrl.nmi_enabled() && self.status.in_vblank() {
                    self.nmi_pending = true;
                }
            }

            // $2001: PPUMASK
            1 => {
                self.mask = PpuMask::from_bits_truncate(value);
            }

            // $2002: PPUSTATUS (read-only)
            2 => {}

            // $2003: OAMADDR
            3 => {
                self.oam.set_addr(value);
            }

            // $2004: OAMDATA
            4 => {
                self.oam.write(value);
            }

            // $2005: PPUSCROLL
            5 => {
                // Detect mid-scanline write for split-screen effects
                if self.is_visible_rendering_position() {
                    self.scroll.record_mid_scanline_write();
                }
                self.scroll.write_ppuscroll(value);
            }

            // $2006: PPUADDR
            6 => {
                // Detect mid-scanline write for split-screen effects
                // The second write to $2006 copies t to v, which affects rendering
                if self.is_visible_rendering_position() {
                    self.scroll.record_mid_scanline_write();
                }
                self.scroll.write_ppuaddr(value);
            }

            // $2007: PPUDATA
            7 => {
                let addr = self.scroll.vram_addr();

                // Write to CHR (mapper) or VRAM/Palette
                if (addr & 0x3FFF) < 0x2000 {
                    write_chr(addr & 0x3FFF, value);
                } else {
                    self.vram.write(addr, value);
                }

                // Increment VRAM address
                let increment = self.ctrl.vram_increment();
                self.scroll.increment_vram(increment);
            }

            _ => unreachable!(),
        }
    }

    /// Perform OAM DMA (copy 256 bytes from CPU memory)
    pub fn oam_dma(&mut self, data: &[u8; 256]) {
        self.oam.dma_write(data);
    }

    /// Step PPU by one dot (without CHR access - for backwards compatibility)
    ///
    /// Returns (frame_complete, nmi_triggered).
    /// Note: This method won't render tiles properly. Use `step_with_chr` for full rendering.
    #[inline]
    pub fn step(&mut self) -> (bool, bool) {
        self.step_with_chr(|_| 0)
    }

    /// Step PPU by one dot with CHR ROM access
    ///
    /// This method allows the PPU to read pattern table data from the mapper's CHR ROM.
    ///
    /// # Arguments
    ///
    /// * `read_chr` - Function to read CHR ROM at a given address (0x0000-0x1FFF)
    ///
    /// # Returns
    ///
    /// (frame_complete, nmi_triggered)
    #[inline]
    #[allow(clippy::too_many_lines)] // PPU step naturally handles many timing states
    pub fn step_with_chr<F: Fn(u16) -> u8>(&mut self, read_chr: F) -> (bool, bool) {
        // Open bus decay
        if self.decay_counter > 0 {
            self.decay_counter -= 1;
            if self.decay_counter == 0 {
                self.open_bus_latch = 0;
            }
        }

        let rendering_enabled = self.mask.rendering_enabled();

        // Tick timing FIRST to advance to the next position
        let frame_complete = self.timing.tick(rendering_enabled);

        let scanline = self.timing.scanline();
        let dot = self.timing.dot();

        // VBlank flag management (check AFTER tick)
        if self.timing.is_vblank_set_dot() {
            self.status.set_vblank();
            if self.ctrl.nmi_enabled() {
                self.nmi_pending = true;
            }
        }

        if self.timing.is_vblank_clear_dot() {
            self.status.clear_vblank();
            self.status.clear_sprite_flags();
            self.nmi_pending = false;
            // Reset frame-specific scroll tracking for mid-scanline detection
            self.scroll.start_frame();
        }

        // Rendering logic (visible and pre-render scanlines)
        if rendering_enabled && self.timing.is_rendering_scanline() {
            // Background rendering
            if self.timing.is_visible_dot() || self.timing.is_prefetch_dot() {
                self.background.shift_registers();

                // 8-dot tile fetch cycle
                // Dots are 1-indexed: 1-256 visible, 321-336 prefetch
                let fetch_dot = dot;
                match fetch_dot % 8 {
                    1 => {
                        // Fetch nametable byte (tile index)
                        let nt_addr = 0x2000 | (self.scroll.vram_addr() & 0x0FFF);
                        let tile_index = self.vram.read(nt_addr);
                        self.background.set_nametable_byte(tile_index);
                    }
                    3 => {
                        // Fetch attribute byte
                        let v = self.scroll.vram_addr();
                        let attr_addr =
                            0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
                        let attr_byte = self.vram.read(attr_addr);
                        self.background.set_attribute_byte(
                            attr_byte,
                            self.scroll.coarse_x(),
                            self.scroll.coarse_y(),
                        );
                    }
                    5 => {
                        // Fetch pattern table low byte
                        let bg_base = self.ctrl.bg_table_addr();
                        let tile_index = self.background.nametable_byte();
                        let fine_y = self.scroll.fine_y();
                        let pattern_addr = bg_base + u16::from(tile_index) * 16 + u16::from(fine_y);
                        let pattern_low = read_chr(pattern_addr);
                        self.background.set_pattern_low(pattern_low);
                    }
                    7 => {
                        // Fetch pattern table high byte
                        let bg_base = self.ctrl.bg_table_addr();
                        let tile_index = self.background.nametable_byte();
                        let fine_y = self.scroll.fine_y();
                        let pattern_addr =
                            bg_base + u16::from(tile_index) * 16 + u16::from(fine_y) + 8;
                        let pattern_high = read_chr(pattern_addr);
                        self.background.set_pattern_high(pattern_high);
                    }
                    0 => {
                        // Load shift registers and increment coarse X
                        self.background.load_shift_registers();
                        self.scroll.increment_x();
                    }
                    _ => {}
                }

                // Increment Y at dot 256
                if dot == 256 {
                    self.scroll.increment_y();
                }
            }

            // Sprite rendering
            if self.timing.is_visible_dot() {
                self.sprite_renderer.tick();
            }

            // Scrolling updates
            if self.timing.is_hori_copy_dot() {
                self.scroll.copy_horizontal();
            }

            if self.timing.is_vert_copy_range() {
                self.scroll.copy_vertical();
            }

            // Sprite evaluation (visible scanlines only)
            if self.timing.is_visible_scanline() {
                if self.timing.is_sprite_eval_start() {
                    self.sprite_evaluator.start_evaluation();
                    self.secondary_oam.clear();
                }

                if self.timing.is_sprite_eval_range() {
                    self.sprite_evaluator.evaluate_step(
                        self.oam.data(),
                        scanline + 1, // Evaluate for next scanline
                        self.ctrl.sprite_height(),
                        &mut self.secondary_oam,
                    );
                }
            }

            // Sprite fetching (all rendering scanlines)
            if self.timing.is_sprite_fetch_start() {
                // Load sprites from secondary OAM into sprite renderer
                let sprite_zero_in_range = self.sprite_evaluator.sprite_zero_in_range();
                self.sprite_renderer
                    .load_sprites(&self.secondary_oam, sprite_zero_in_range);
            }

            if self.timing.is_sprite_fetch_range() {
                // Fetch sprite pattern data during dots 257-320 (8 dots per sprite, 8 sprites)
                let fetch_cycle = dot - 257; // 0-63
                let sprite_index = fetch_cycle / 8; // 0-7 (which sprite)
                let fetch_step = fetch_cycle % 8; // 0-7 (which step in the 8-dot cycle)

                // On step 7, fetch both pattern bytes and load into sprite renderer
                // (simplified from hardware timing which fetches in steps 4 and 6)
                if fetch_step == 7
                    && let Some(sprite) = self.secondary_oam.get_sprite(sprite_index as u8)
                {
                    let sprite_base = self.ctrl.sprite_table_addr();
                    let tile_index = sprite.tile_index;

                    // Calculate which row of the sprite to fetch
                    // Note: We're fetching for scanline+1 (next scanline) since
                    // sprite evaluation fills secondary OAM with sprites for next scanline
                    let next_scanline = scanline + 1;
                    let sprite_y = next_scanline.saturating_sub(sprite.y as u16);

                    // Clamp sprite_y to valid range (0-7 for 8x8 sprites)
                    // This prevents overflow when calculating flipped row
                    let sprite_y = sprite_y.min(7);

                    // Handle vertical flip
                    let row = if sprite.attributes.flip_vertical() {
                        7 - sprite_y
                    } else {
                        sprite_y
                    };

                    // Fetch pattern table low byte
                    let pattern_addr_low = sprite_base + u16::from(tile_index) * 16 + row;
                    let mut pattern_low = read_chr(pattern_addr_low);

                    // Fetch pattern table high byte
                    let pattern_addr_high = pattern_addr_low + 8;
                    let mut pattern_high = read_chr(pattern_addr_high);

                    // Handle horizontal flip
                    if sprite.attributes.flip_horizontal() {
                        pattern_low = pattern_low.reverse_bits();
                        pattern_high = pattern_high.reverse_bits();
                    }

                    // Load pattern data into sprite renderer
                    self.sprite_renderer.load_sprite_pattern(
                        sprite_index as u8,
                        pattern_low,
                        pattern_high,
                    );
                }
            }

            // Render pixel (visible scanlines only)
            if self.timing.is_visible_scanline() && self.timing.is_visible_dot() {
                let x = dot - 1;
                let y = scanline;
                self.render_pixel(x as usize, y as usize);
            }
        }

        let nmi = self.nmi_pending;
        if nmi {
            self.nmi_pending = false;
        }

        (frame_complete, nmi)
    }

    /// Render a single pixel
    #[inline]
    fn render_pixel(&mut self, x: usize, y: usize) {
        let mut bg_pixel = 0;
        let mut bg_palette = 0;

        // Get background pixel
        if self.mask.show_background() {
            let fine_x = self.scroll.fine_x();
            let (pixel, palette) = self.background.get_pixel(fine_x);
            bg_pixel = pixel;
            bg_palette = palette;
        }

        let mut sprite_pixel = 0;
        let mut sprite_palette = 0;
        let mut sprite_priority = false;
        let mut sprite_zero = false;

        // Get sprite pixel
        if self.mask.show_sprites()
            && let Some((pixel, palette, priority, is_sprite_zero)) =
                self.sprite_renderer.get_pixel()
        {
            sprite_pixel = pixel;
            sprite_palette = palette;
            sprite_priority = priority;
            sprite_zero = is_sprite_zero;
        }

        // Sprite 0 hit detection
        if sprite_zero && bg_pixel != 0 && sprite_pixel != 0 {
            self.status.set_sprite_zero_hit();
        }

        // Multiplexing (determine final pixel)
        let (final_pixel, final_palette) = if bg_pixel == 0 && sprite_pixel == 0 {
            // Both transparent - use backdrop color
            (0, 0)
        } else if bg_pixel == 0 {
            // Background transparent - show sprite
            (sprite_pixel, sprite_palette)
        } else if sprite_pixel == 0 {
            // Sprite transparent - show background
            (bg_pixel, bg_palette)
        } else {
            // Both opaque - check priority
            if sprite_priority {
                (bg_pixel, bg_palette)
            } else {
                (sprite_pixel, sprite_palette)
            }
        };

        // Read palette and write to frame buffer
        let palette_addr = (final_palette << 2) | final_pixel;
        let color_index = self.vram.read_palette(palette_addr);

        let offset = y * FRAME_WIDTH + x;
        self.frame_buffer[offset] = color_index;
    }

    /// Get frame buffer (palette indices 0-63)
    #[inline]
    pub fn frame_buffer(&self) -> &[u8] {
        &self.frame_buffer
    }

    /// Set nametable mirroring
    pub fn set_mirroring(&mut self, mirroring: Mirroring) {
        self.vram.set_mirroring(mirroring);
    }

    /// Reset to power-up state
    pub fn reset(&mut self) {
        self.ctrl = PpuCtrl::empty();
        self.mask = PpuMask::empty();
        self.status = PpuStatus::empty();
        self.scroll = ScrollRegisters::new();
        self.vram.reset();
        self.oam.reset();
        self.background.reset();
        self.sprite_renderer.reset();
        self.timing.reset();
        self.frame_buffer.fill(0);
        self.vram_read_buffer = 0;
        self.nmi_pending = false;
    }

    /// Get current scanline number (0-261)
    pub fn scanline(&self) -> u16 {
        self.timing.scanline()
    }

    /// Get current dot within scanline (0-340)
    pub fn dot(&self) -> u16 {
        self.timing.dot()
    }

    /// Get current VRAM address (v register)
    pub fn vram_addr(&self) -> u16 {
        self.scroll.vram_addr()
    }

    /// Get temporary VRAM address (t register)
    pub fn temp_vram_addr(&self) -> u16 {
        self.scroll.temp_vram_addr()
    }

    /// Get fine X scroll (0-7)
    pub fn fine_x(&self) -> u8 {
        self.scroll.fine_x()
    }

    /// Get coarse X scroll (tile column 0-31)
    pub fn coarse_x(&self) -> u8 {
        self.scroll.coarse_x()
    }

    /// Get coarse Y scroll (tile row 0-31)
    pub fn coarse_y(&self) -> u8 {
        self.scroll.coarse_y()
    }

    /// Get fine Y scroll (pixel row 0-7)
    pub fn fine_y(&self) -> u8 {
        self.scroll.fine_y()
    }

    /// Check if a mid-scanline write was detected this frame
    ///
    /// Games use mid-scanline writes to $2005/$2006 for split-screen effects
    /// like Super Mario Bros. 3's status bar.
    pub fn mid_scanline_write_detected(&self) -> bool {
        self.scroll.mid_scanline_write_detected()
    }

    /// Get the last v value before a mid-scanline update (for debugging)
    pub fn last_v_before_update(&self) -> u16 {
        self.scroll.last_v_before_update()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppu_creation() {
        let ppu = Ppu::new(Mirroring::Horizontal);
        assert_eq!(ppu.frame_buffer().len(), FRAME_SIZE);
    }

    #[test]
    fn test_ppuctrl_write() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        ppu.write_register(0x2000, 0x80, |_, _| {}); // Enable NMI
        assert!(ppu.ctrl.nmi_enabled());
    }

    #[test]
    fn test_ppustatus_read() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        ppu.status.set_vblank();
        let status = ppu.read_register(0x2002, |_| 0);

        assert_eq!(status & 0x80, 0x80); // VBlank bit set
        assert!(!ppu.status.in_vblank()); // Should be cleared after read
    }

    #[test]
    fn test_oam_write() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        ppu.write_register(0x2003, 0x00, |_, _| {}); // OAMADDR = 0
        ppu.write_register(0x2004, 0x42, |_, _| {}); // OAMDATA = $42

        ppu.write_register(0x2003, 0x00, |_, _| {}); // Reset OAMADDR
        let value = ppu.read_register(0x2004, |_| 0);
        assert_eq!(value, 0x42);
    }

    #[test]
    fn test_vram_write_read() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Write address $2000
        ppu.write_register(0x2006, 0x20, |_, _| {});
        ppu.write_register(0x2006, 0x00, |_, _| {});

        // Write data
        ppu.write_register(0x2007, 0x55, |_, _| {});

        // Read address $2000
        ppu.write_register(0x2006, 0x20, |_, _| {});
        ppu.write_register(0x2006, 0x00, |_, _| {});

        // First read is buffered (returns garbage)
        let _ = ppu.read_register(0x2007, |_| 0);
        // Second read returns actual data
        let value = ppu.read_register(0x2007, |_| 0);
        assert_eq!(value, 0x55);
    }

    #[test]
    fn test_palette_immediate_read() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Write to palette
        ppu.write_register(0x2006, 0x3F, |_, _| {});
        ppu.write_register(0x2006, 0x00, |_, _| {});
        ppu.write_register(0x2007, 0x0F, |_, _| {});

        // Read from palette (immediate, no buffer)
        ppu.write_register(0x2006, 0x3F, |_, _| {});
        ppu.write_register(0x2006, 0x00, |_, _| {});
        let value = ppu.read_register(0x2007, |_| 0);
        assert_eq!(value, 0x0F);
    }

    #[test]
    fn test_vblank_flag() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Step to VBlank set point (scanline 241, dot 1)
        while ppu.timing.scanline() != 241 || ppu.timing.dot() != 0 {
            ppu.step();
        }

        // Next step should set VBlank
        ppu.step();
        assert!(ppu.status.in_vblank());
    }

    #[test]
    fn test_nmi_trigger() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Enable NMI
        ppu.write_register(0x2000, 0x80, |_, _| {});

        // Step to VBlank
        while ppu.timing.scanline() != 241 || ppu.timing.dot() != 0 {
            ppu.step();
        }

        // Next step should trigger NMI
        let (_, nmi) = ppu.step();
        assert!(nmi);
    }

    #[test]
    fn test_scroll_write() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Write X scroll = 100
        ppu.write_register(0x2005, 100, |_, _| {});
        // Write Y scroll = 50
        ppu.write_register(0x2005, 50, |_, _| {});

        // Verify scroll registers updated
        assert_eq!(ppu.scroll.fine_x(), 100 & 0x07);
    }

    #[test]
    fn test_oam_dma() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        let mut data = [0u8; 256];

        // Fill with test pattern
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = i as u8;
        }

        ppu.oam_dma(&data);

        // Verify OAM contents by reading each address
        for i in 0..256u16 {
            ppu.oam.set_addr(i as u8);
            let expected = if i % 4 == 2 {
                // Attribute bytes (byte 2 of each sprite) have bits 2-4 masked
                // due to hardware - these bits don't physically exist in PPU OAM
                (i as u8) & 0xE3
            } else {
                i as u8
            };
            assert_eq!(ppu.oam.read(), expected);
        }
    }
}
