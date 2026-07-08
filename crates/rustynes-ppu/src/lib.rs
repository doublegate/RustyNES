//! Cycle-accurate Ricoh 2C02 PPU implementation.
//!
//! See `docs/ppu-2c02.md` for the implementation spec and
//! `ref-docs/research-report.md` §PPU for the source material.
//!
//! Background rendering, sprite evaluation + rendering, sprite-zero hit,
//! sprite overflow, and the open-bus latch are all implemented at PPU-dot
//! resolution. PPUSTATUS / PPUDATA / PPUSCROLL / PPUADDR register quirks
//! match the test-ROM corpus. The 2-PPU-clock PPUMASK pipeline delay
//! between a mask write and the odd-frame dot-skip check is wired through.
//!
//! Region timing (NTSC vs PAL vs Dendy) is parameterized via
//! [`PpuRegion`]; the structural difference between them is the post-
//! render-to-pre-render scanline span (NTSC: 241..=260; PAL: 241..=310;
//! Dendy: 241..=290).

#![no_std]
#![warn(missing_docs)]
// Truncating casts are the canonical encoding for the PPU's 8/16-bit register
// arithmetic; we annotate this once at module level rather than per-line.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_possible_wrap
)]

extern crate alloc;

mod bus;
mod palette;
mod ppu;
mod registers;
mod snapshot;
#[cfg(feature = "ppu-state-trace")]
pub mod state_trace;

pub use bus::{BgSplitState, ExAttribute, PpuBus};
pub use palette::{NES_PALETTE, PpuPalette, nes_color_to_rgba, palette_color_to_rgba};
pub use ppu::MASK_WRITE_DELAY;
pub use ppu::octal_trace;
pub use ppu::read2007_diag;
pub use ppu::{FRAMEBUFFER_LEN, Ppu, PpuRegion};
#[cfg(feature = "hd-pack")]
pub use ppu::{HD_CHR_RAM, HD_TILE_NONE, HdSprite, HdTileSource};
pub use snapshot::{PPU_SNAPSHOT_VERSION, PpuSnapshotError};

#[cfg(feature = "ppu-state-trace")]
pub use state_trace::{
    BINARY_MAGIC, HEADER_SIZE, PPU_TRACE_SCHEMA_VERSION, PpuStateRecord, PpuStateTrace,
    PpuTraceConfig, RECORD_SIZE, fnv1a64,
};

/// Returns the crate version string.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
