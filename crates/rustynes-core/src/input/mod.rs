//! NES controller input handling.
//!
//! This module emulates the NES standard controller protocol, which uses a
//! **strobe-based parallel-to-serial shift register** (4021 IC) to read
//! 8 button states sequentially.
//!
//! # Hardware Protocol
//!
//! The NES controller protocol works as follows:
//!
//! 1. **Strobe** ($4016 write, bit 0):
//!    - Write 1: Continuously reload shift register (parallel mode)
//!    - Write 0: Enable serial reads (shift mode)
//!    - Falling edge (1 → 0) latches current button states
//!
//! 2. **Serial Read** ($4016/$4017 read):
//!    - Returns one button bit per read
//!    - Order: A, B, Select, Start, Up, Down, Left, Right
//!    - Reads 9+ always return 1
//!
//! # Registers
//!
//! - **$4016**: Controller 1 data (read) / Strobe (write)
//! - **$4017**: Controller 2 data (read) / APU Frame Counter (write)
//!
//! **Note**: $4016 writes strobe BOTH controllers simultaneously.
//!
//! # Usage Example
//!
//! ```no_run
//! use rustynes_core::{Console, Button};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rom = std::fs::read("game.nes")?;
//! let mut console = Console::from_rom_bytes(&rom)?;
//!
//! // Set controller 1 button state
//! console.set_button_1(Button::A, true);       // Press A
//! console.set_button_1(Button::Start, true);   // Press Start
//!
//! // Step frames
//! for _ in 0..60 {
//!     console.step_frame();
//! }
//!
//! // Release buttons
//! console.set_button_1(Button::A, false);
//! console.set_button_1(Button::Start, false);
//! # Ok(())
//! # }
//! ```

mod controller;

pub use controller::{Button, Controller};
