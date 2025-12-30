//! RustyNES Core - NES Emulation Integration Layer.
//!
//! This crate provides the high-level NES emulation API, integrating the CPU,
//! PPU, APU, and mapper components into a complete console emulator.
//!
//! # Architecture
//!
//! The core crate connects all NES components through a central bus:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        Console                              │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │                      NesBus                          │   │
//! │  │  ┌─────┐  ┌─────┐  ┌─────┐  ┌────────┐  ┌────────┐ │   │
//! │  │  │ RAM │  │ PPU │  │ APU │  │ Mapper │  │ Input  │ │   │
//! │  │  │ 2KB │  │     │  │     │  │        │  │        │ │   │
//! │  │  └─────┘  └─────┘  └─────┘  └────────┘  └────────┘ │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │                          ▲                                  │
//! │                          │                                  │
//! │                     ┌────┴────┐                             │
//! │                     │   CPU   │                             │
//! │                     │  6502   │                             │
//! │                     └─────────┘                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use rustynes_core::{Console, ControllerState};
//!
//! // Load a ROM file
//! let rom_data = std::fs::read("game.nes").expect("Failed to read ROM");
//! let mut console = Console::new(&rom_data).expect("Failed to create console");
//!
//! // Power on and run
//! console.power_on();
//!
//! loop {
//!     // Set controller input
//!     let mut input = ControllerState::default();
//!     input.buttons = ControllerState::A | ControllerState::START;
//!     console.set_controller1(input);
//!
//!     // Run one frame
//!     console.step_frame();
//!
//!     // Get framebuffer for display (256x240 RGBA)
//!     let _framebuffer = console.framebuffer();
//!
//!     // Get audio samples
//!     let _audio = console.take_audio();
//! }
//! ```
//!
//! # Features
//!
//! - `std` (default): Enable standard library support
//! - `serde`: Enable serialization for save states

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

mod bus;
mod console;
pub mod palette;

// Re-export main types
pub use bus::{ControllerState, NesBus};
pub use console::{Console, ConsoleError, timing};

// Re-export commonly used types from dependencies
pub use rustynes_apu::Apu;
pub use rustynes_cpu::Cpu;
pub use rustynes_mappers::{Mapper, Mirroring, Rom, RomError, RomFormat, RomHeader, create_mapper};
pub use rustynes_ppu::Ppu;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// NES screen dimensions.
pub mod screen {
    /// Screen width in pixels.
    pub const WIDTH: u32 = 256;
    /// Screen height in pixels.
    pub const HEIGHT: u32 = 240;
    /// Total pixels per frame.
    pub const PIXELS: u32 = WIDTH * HEIGHT;
    /// Bytes per frame (RGBA).
    pub const FRAMEBUFFER_SIZE: usize = (PIXELS * 4) as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_mappers::{Mirroring, Nrom, RomFormat, RomHeader};

    #[cfg(not(feature = "std"))]
    use alloc::{boxed::Box, vec, vec::Vec};

    fn create_test_rom() -> Rom {
        Rom {
            header: RomHeader {
                format: RomFormat::INes,
                mapper: 0,
                prg_rom_size: 2,
                chr_rom_size: 1,
                prg_ram_size: 0,
                chr_ram_size: 0,
                mirroring: Mirroring::Vertical,
                has_battery: false,
                has_trainer: false,
                tv_system: 0,
            },
            prg_rom: {
                let mut prg = vec![0xEA; 32768]; // Fill with NOPs
                // Reset vector at $FFFC points to $8000
                prg[0x7FFC] = 0x00;
                prg[0x7FFD] = 0x80;
                prg
            },
            chr_rom: vec![0; 8192],
            trainer: None,
        }
    }

    #[test]
    fn test_console_creation_with_mapper() {
        let rom = create_test_rom();
        let mapper = Box::new(Nrom::new(&rom));
        let console = Console::with_mapper(mapper).unwrap();

        assert_eq!(console.mapper_number(), 0);
        assert_eq!(console.mapper_name(), "NROM");
    }

    #[test]
    fn test_screen_constants() {
        assert_eq!(screen::WIDTH, 256);
        assert_eq!(screen::HEIGHT, 240);
        assert_eq!(screen::PIXELS, 61440);
        assert_eq!(screen::FRAMEBUFFER_SIZE, 245_760);
    }

    #[test]
    fn test_timing_constants() {
        assert_eq!(timing::MASTER_CLOCK_NTSC, 21_477_272);
        assert_eq!(timing::CPU_CLOCK_NTSC, 1_789_772);
        assert_eq!(timing::PPU_CLOCK_NTSC, 5_369_318);
        assert_eq!(timing::CPU_CYCLES_PER_FRAME, 29_780);
    }

    #[test]
    fn test_controller_state_buttons() {
        let mut state = ControllerState::default();
        assert_eq!(state.buttons, 0);

        state.buttons = ControllerState::A | ControllerState::B;
        assert_eq!(state.buttons, 0x03);

        state.buttons |= ControllerState::START;
        assert_eq!(state.buttons, 0x0B);
    }

    #[test]
    fn test_palette_module() {
        // Verify palette is accessible
        assert_eq!(palette::NES_PALETTE.len(), 64);

        // Check some known colors
        let white = palette::palette_to_rgb(0x20);
        assert_eq!(white, (0xFF, 0xFF, 0xFF));

        let black = palette::palette_to_rgb(0x0D);
        assert_eq!(black, (0, 0, 0));
    }

    #[test]
    fn test_console_step() {
        let rom = create_test_rom();
        let mapper = Box::new(Nrom::new(&rom));
        let mut console = Console::with_mapper(mapper).unwrap();

        console.reset();

        // Step a few instructions
        let mut total_cycles = 0u64;
        for _ in 0..10 {
            total_cycles += u64::from(console.step());
        }

        assert!(total_cycles > 0);
        assert_eq!(console.total_cycles(), total_cycles);
    }

    #[test]
    fn test_console_audio_buffer() {
        let rom = create_test_rom();
        let mapper = Box::new(Nrom::new(&rom));
        let mut console = Console::with_mapper(mapper).unwrap();

        console.reset();

        // Run some cycles to generate audio
        for _ in 0..1000 {
            console.step();
        }

        // Take audio samples
        let audio = console.take_audio();
        assert!(!audio.is_empty());

        // Buffer should be empty after take
        assert!(console.audio_buffer().is_empty());
    }
}
