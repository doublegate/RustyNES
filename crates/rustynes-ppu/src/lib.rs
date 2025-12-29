//! RustyNES PPU - Cycle-accurate Ricoh 2C02 PPU emulation
//!
//! This crate provides a cycle-accurate implementation of the Ricoh 2C02 PPU
//! as used in the Nintendo Entertainment System (NES). It includes:
//!
//! - Dot-accurate timing (341 dots × 262 scanlines)
//! - Complete register implementation (PPUCTRL, PPUMASK, PPUSTATUS, etc.)
//! - Loopy scrolling model (v, t, x, w registers)
//! - Background rendering with shift registers
//! - Sprite evaluation and rendering (up to 8 per scanline)
//! - VBlank and NMI generation
//! - Palette RAM and nametable mirroring
//! - Zero unsafe code
//!
//! # Example
//!
//! ```no_run
//! use rustynes_ppu::{Ppu, Mirroring};
//!
//! fn main() {
//!     let mut ppu = Ppu::new(Mirroring::Horizontal);
//!
//!     // Reset PPU
//!     ppu.reset();
//!
//!     // Main emulation loop
//!     loop {
//!         // Step PPU by one dot (3 dots per CPU cycle)
//!         let (frame_complete, nmi) = ppu.step();
//!
//!         if nmi {
//!             // Trigger NMI interrupt on CPU
//!         }
//!
//!         if frame_complete {
//!             // Render frame to screen
//!             let frame_buffer = ppu.frame_buffer();
//!             // ... convert palette indices to RGB and display
//!         }
//!     }
//! }
//! ```
//!
//! # Timing
//!
//! The PPU operates at 5.369318 MHz (NTSC), which is 3× the CPU clock.
//! Each PPU step represents one dot (pixel clock).
//!
//! Frame structure:
//! - 341 dots per scanline
//! - 262 scanlines per frame
//! - 89,342 dots per frame (29,780.67 CPU cycles)
//! - ~60 Hz frame rate
//!
//! # Memory Map
//!
//! The PPU has its own 16KB address space:
//!
//! ```text
//! $0000-$0FFF: Pattern Table 0 (CHR ROM/RAM via mapper)
//! $1000-$1FFF: Pattern Table 1 (CHR ROM/RAM via mapper)
//! $2000-$23FF: Nametable 0
//! $2400-$27FF: Nametable 1
//! $2800-$2BFF: Nametable 2
//! $2C00-$2FFF: Nametable 3
//! $3000-$3EFF: Mirror of $2000-$2EFF
//! $3F00-$3F1F: Palette RAM (32 bytes)
//! $3F20-$3FFF: Mirror of $3F00-$3F1F
//! ```
//!
//! # Registers
//!
//! The CPU accesses the PPU through 8 memory-mapped registers at $2000-$2007:
//!
//! - **$2000 PPUCTRL**: Control register (NMI enable, sprite/bg tables, etc.)
//! - **$2001 PPUMASK**: Mask register (rendering enable, color effects)
//! - **$2002 PPUSTATUS**: Status register (VBlank, sprite 0 hit, overflow)
//! - **$2003 OAMADDR**: OAM address register
//! - **$2004 OAMDATA**: OAM data port
//! - **$2005 PPUSCROLL**: Scroll position (X then Y)
//! - **$2006 PPUADDR**: VRAM address (high then low byte)
//! - **$2007 PPUDATA**: VRAM data port
//!
//! Additionally, **$4014 OAMDMA** in CPU memory performs a fast 256-byte
//! copy to OAM (handled by the emulator core, not this crate).
//!
//! # Accuracy
//!
//! This implementation is designed to pass:
//! - blargg's ppu_vbl_nmi tests
//! - blargg's sprite_hit_tests_2005
//! - blargg's ppu_tests
//! - sprite_overflow tests
//!
//! # Feature Flags
//!
//! Currently no optional features. All functionality is included by default.

// Lints are configured in the workspace Cargo.toml
// This ensures consistent settings across all crates

mod background;
mod oam;
mod ppu;
mod registers;
mod scroll;
mod sprites;
mod timing;
mod vram;

// Public exports
pub use oam::{Oam, SecondaryOam, Sprite, SpriteAttributes};
pub use ppu::{FRAME_HEIGHT, FRAME_SIZE, FRAME_WIDTH, Ppu};
pub use registers::{PpuCtrl, PpuMask, PpuStatus};
pub use scroll::ScrollRegisters;
pub use vram::{Mirroring, Vram};

// Re-export timing for debugging/testing
pub use timing::Timing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppu_integration() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Reset PPU
        ppu.reset();

        // Enable rendering
        ppu.write_register(0x2001, 0x18, |_, _| {}); // Show BG and sprites

        // Step through one frame
        let mut steps = 0;
        let mut frame_complete = false;

        while !frame_complete {
            let (complete, _) = ppu.step();
            frame_complete = complete;
            steps += 1;

            // Safety check
            assert!(steps <= 100_000, "Frame didn't complete in reasonable time");
        }

        // NTSC frame: 341 * 262 = 89342 dots
        // But odd frames skip one dot
        assert!(steps <= 89342);
    }

    #[test]
    fn test_mirroring_modes() {
        // Test all mirroring modes can be created
        let _ = Ppu::new(Mirroring::Horizontal);
        let _ = Ppu::new(Mirroring::Vertical);
        let _ = Ppu::new(Mirroring::SingleScreenLower);
        let _ = Ppu::new(Mirroring::SingleScreenUpper);
        let _ = Ppu::new(Mirroring::FourScreen);
    }

    #[test]
    fn test_vblank_timing() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Step to start of VBlank (scanline 241, dot 1)
        while ppu.step().0 {} // Complete one frame
        while ppu.step().0 {} // Complete second frame

        // Now at scanline 0
        // Step to scanline 241
        let mut steps = 0;
        while steps < 241 * 341 {
            ppu.step();
            steps += 1;
        }

        // Next step should set VBlank
        ppu.step();
        let status = ppu.read_register(0x2002, |_| 0);
        assert_eq!(status & 0x80, 0x80); // VBlank flag set
    }

    #[test]
    fn test_frame_buffer_size() {
        let ppu = Ppu::new(Mirroring::Horizontal);
        assert_eq!(ppu.frame_buffer().len(), FRAME_SIZE);
        assert_eq!(FRAME_SIZE, FRAME_WIDTH * FRAME_HEIGHT);
    }

    #[test]
    fn test_register_read_write() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Write PPUCTRL
        ppu.write_register(0x2000, 0x80, |_, _| {});
        // Write PPUMASK
        ppu.write_register(0x2001, 0x18, |_, _| {});

        // Read PPUSTATUS (this is the only readable register besides OAMDATA/PPUDATA)
        let status = ppu.read_register(0x2002, |_| 0);
        assert_eq!(status & 0x80, 0); // VBlank not set yet
    }

    #[test]
    fn test_oam_operations() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Set OAM address
        ppu.write_register(0x2003, 0x00, |_, _| {});

        // Write sprite data
        ppu.write_register(0x2004, 50, |_, _| {}); // Y position
        ppu.write_register(0x2004, 0x42, |_, _| {}); // Tile index
        ppu.write_register(0x2004, 0x00, |_, _| {}); // Attributes
        ppu.write_register(0x2004, 100, |_, _| {}); // X position

        // Read back
        ppu.write_register(0x2003, 0x00, |_, _| {});
        assert_eq!(ppu.read_register(0x2004, |_| 0), 50);
    }

    #[test]
    fn test_vram_operations() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Write to nametable
        ppu.write_register(0x2006, 0x20, |_, _| {}); // High byte
        ppu.write_register(0x2006, 0x00, |_, _| {}); // Low byte
        // Step PPU to apply delayed PPUADDR update (2 PPU cycles)
        ppu.step();
        ppu.step();
        ppu.write_register(0x2007, 0x55, |_, _| {}); // Data

        // Read back (with buffered read)
        ppu.write_register(0x2006, 0x20, |_, _| {});
        ppu.write_register(0x2006, 0x00, |_, _| {});
        // Step PPU to apply delayed PPUADDR update
        ppu.step();
        ppu.step();
        let _ = ppu.read_register(0x2007, |_| 0); // Dummy read
        let value = ppu.read_register(0x2007, |_| 0); // Actual data
        assert_eq!(value, 0x55);
    }

    #[test]
    fn test_scroll_operations() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);

        // Write scroll position
        ppu.write_register(0x2005, 100, |_, _| {}); // X scroll
        ppu.write_register(0x2005, 50, |_, _| {}); // Y scroll

        // Reading PPUSTATUS should reset write latch
        ppu.read_register(0x2002, |_| 0);

        // Next write should be X scroll again
        ppu.write_register(0x2005, 10, |_, _| {});
    }
}
