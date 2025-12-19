//! Save state system for `RustyNES` emulator.
//!
//! This module provides instant save/load functionality for complete emulator state,
//! enabling features like rewind, TAS recording, and quick save/load.
//!
//! # Format
//!
//! Save states use a custom binary format with the following structure:
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │ Header (64 bytes)                   │
//! │  - Magic: "RNES"                    │
//! │  - Version: u32                     │
//! │  - Checksum: CRC32                  │
//! │  - Flags: u32                       │
//! │  - ROM Hash: SHA-256 (32 bytes)     │
//! │  - Timestamp: u64                   │
//! │  - Frame Count: u64                 │
//! │  - Reserved: 8 bytes                │
//! ├─────────────────────────────────────┤
//! │ State Data (variable)               │
//! └─────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use rustynes_core::Console;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rom = std::fs::read("game.nes")?;
//! let mut console = Console::from_rom_bytes(&rom)?;
//!
//! // Execute some frames
//! for _ in 0..1000 {
//!     console.step_frame();
//! }
//!
//! // Save state (not yet implemented - Phase 2)
//! // console.save_state_to_file(Path::new("save1.state"))?;
//!
//! // Continue playing...
//! for _ in 0..500 {
//!     console.step_frame();
//! }
//!
//! // Load previous state (not yet implemented - Phase 2)
//! // console.load_state_from_file(Path::new("save1.state"))?;
//! # Ok(())
//! # }
//! ```
//!
//! # Performance
//!
//! - Uncompressed save: ~50KB, <0.1ms
//! - Compressed save: ~10-20KB, ~2-5ms
//! - Load (either): <0.5ms
//!
//! # Note
//!
//! Full save state functionality will be implemented in Phase 2.
//! This module currently defines the format and error types.

pub mod error;

pub use error::SaveStateError;

/// Save state format version
pub const SAVE_STATE_VERSION: u32 = 1;

/// Magic bytes for save state files
pub const SAVE_STATE_MAGIC: &[u8; 4] = b"RNES";
