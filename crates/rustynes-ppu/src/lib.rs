//! NES 2C02 PPU (Picture Processing Unit) emulation.
//!
//! This crate provides a cycle-accurate implementation of the NES PPU,
//! responsible for all graphics rendering.
//!
//! # Overview
//!
//! The PPU operates at 3x the CPU clock rate and generates a 256x240 pixel
//! image. It consists of several subsystems:
//!
//! - **Registers**: Control, Mask, Status, OAM Address, Scroll, Address, Data
//! - **Background rendering**: Nametables, pattern tables, attribute tables
//! - **Sprite rendering**: OAM, sprite evaluation, sprite 0 hit detection
//! - **Palette**: 32-byte palette RAM with mirroring
//!
//! # Timing
//!
//! NTSC timing (the primary target):
//! - Master clock: 21.477272 MHz
//! - PPU clock: 5.369318 MHz (master / 4)
//! - 341 dots per scanline
//! - 262 scanlines per frame
//! - 89,341-89,342 dots per frame (odd frame skip)
//!
//! # Usage
//!
//! ```no_run
//! use rustynes_ppu::{Ppu, PpuBus};
//!
//! // Implement PpuBus for your memory system
//! struct MyBus {
//!     // VRAM, CHR ROM/RAM, etc.
//! }
//!
//! impl PpuBus for MyBus {
//!     fn read(&mut self, addr: u16) -> u8 {
//!         // Read from VRAM/CHR memory
//!         0
//!     }
//!
//!     fn write(&mut self, addr: u16, value: u8) {
//!         // Write to VRAM/CHR memory
//!     }
//! }
//!
//! let mut ppu = Ppu::new();
//! let mut bus = MyBus {};
//!
//! // Step the PPU (call 3 times per CPU cycle for NTSC)
//! let nmi = ppu.step(&mut bus);
//! if nmi {
//!     // Trigger NMI in CPU
//! }
//!
//! // Access registers from CPU
//! ppu.write_register(0x2000, 0x80, &mut bus); // Enable NMI
//! let status = ppu.read_register(0x2002, &mut bus);
//! ```
//!
//! # Features
//!
//! - `serde`: Enable serialization support for save states

#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod ctrl;
mod mask;
mod ppu;
mod scroll;
mod sprite;
mod status;

pub use ctrl::Ctrl;
pub use mask::Mask;
pub use ppu::{
    DOTS_PER_SCANLINE, FRAME_HEIGHT, FRAME_WIDTH, PRE_RENDER_SCANLINE, Ppu, PpuBus,
    SCANLINES_PER_FRAME, VBLANK_START_SCANLINE,
};
pub use scroll::Scroll;
pub use sprite::{
    MAX_SPRITES_PER_LINE, OAM_SIZE, SECONDARY_OAM_SIZE, Sprite, SpriteAttr, SpriteEval,
    SpriteRender,
};
pub use status::Status;

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyBus;

    impl PpuBus for DummyBus {
        fn read(&mut self, _addr: u16) -> u8 {
            0
        }
        fn write(&mut self, _addr: u16, _value: u8) {}
    }

    #[test]
    fn test_ppu_integration() {
        let mut ppu = Ppu::new();
        let mut bus = DummyBus;

        // Basic register operations
        ppu.write_register(0x2000, 0x80, &mut bus); // Enable NMI
        ppu.write_register(0x2001, 0x1E, &mut bus); // Enable rendering

        assert!(ppu.ctrl().nmi_enabled());
        assert!(ppu.mask().rendering_enabled());
    }

    #[test]
    fn test_frame_completion() {
        let mut ppu = Ppu::new();
        let mut bus = DummyBus;

        // Run for one frame
        for _ in 0..(DOTS_PER_SCANLINE as u32 * SCANLINES_PER_FRAME as u32) {
            ppu.step(&mut bus);
        }

        // Should have completed at least one frame
        assert!(ppu.frame() >= 1);
    }

    #[test]
    fn test_vblank_nmi() {
        let mut ppu = Ppu::new();
        let mut bus = DummyBus;

        // Enable NMI
        ppu.write_register(0x2000, 0x80, &mut bus);

        // Step until we get NMI
        let mut nmi_triggered = false;
        for _ in 0..100_000 {
            if ppu.step(&mut bus) {
                nmi_triggered = true;
                break;
            }
        }

        assert!(nmi_triggered, "NMI should have been triggered");
    }
}
