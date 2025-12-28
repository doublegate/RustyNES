//! `RustyNES` Core - NES emulator core integration layer
//!
//! This crate provides the main emulator integration, connecting the CPU, PPU, APU,
//! and mappers into a cohesive NES system.
//!
//! # Features
//!
//! - **Bus System**: Memory routing between CPU and all components
//! - **Console Coordinator**: Main emulation loop with accurate timing
//! - **Input Handling**: Controller shift register protocol
//! - **ROM Loading**: iNES/NES 2.0 format support
//! - **Save States**: Deterministic serialization and compression
//! - **Zero Unsafe Code**: All implementations use safe Rust
//!
//! # Example
//!
//! ```no_run
//! use rustynes_core::Console;
//! use std::fs;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load ROM file
//! let rom_data = fs::read("game.nes")?;
//! let mut console = Console::from_rom_bytes(&rom_data)?;
//!
//! // Main emulation loop
//! loop {
//!     // Step one frame (60 FPS)
//!     console.step_frame();
//!
//!     // Get framebuffer for rendering
//!     let framebuffer = console.framebuffer();
//!     // ... render to screen ...
//!
//!     // Handle input
//!     console.set_button_1(rustynes_core::Button::A, true);
//! }
//! # }
//! ```
//!
//! # Architecture
//!
//! The core consists of several key components:
//!
//! - **Bus**: Routes CPU memory access to RAM, PPU, APU, and cartridge
//! - **Console**: Orchestrates all components with cycle-accurate timing
//! - **Input**: Emulates NES controller shift register protocol
//! - **Save State**: Serializes/deserializes complete system state
//!
//! # Timing Model
//!
//! - Master clock: 21.477272 MHz (NTSC)
//! - CPU: 1.789773 MHz (master ÷ 12)
//! - PPU: 5.369318 MHz (master ÷ 4), 3 dots per CPU cycle
//! - APU: Clocked every CPU cycle
//! - Frame: 29,780 CPU cycles, 89,341 PPU dots

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod bus;
pub mod console;
pub mod input;
pub mod save_state;

// Re-exports
pub use bus::Bus;
pub use console::Console;
pub use input::{Button, Controller};

// Re-export commonly used types from dependencies
pub use rustynes_mappers::{Mapper, MapperError, Mirroring, Rom, RomError, create_mapper};
pub use rustynes_ppu::{FRAME_HEIGHT, FRAME_WIDTH};
