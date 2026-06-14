//! Cycle-accurate NES emulator core.
//!
//! This crate is the public entry point for embedders (frontend, test harness,
//! future ports). It owns the scheduler, the bus, save-state serialization,
//! the rewind ring, and the `Nes` facade. Per-chip implementations live in
//! `rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, and `rustynes-mappers`, and are re-exported
//! from this crate so downstream consumers depend on `rustynes-core` only.
//!
//! See `docs/architecture.md` and `docs/scheduler.md` for the design.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

#[cfg(test)]
extern crate std;

pub use rustynes_apu;
pub use rustynes_cpu;
pub use rustynes_mappers;
pub use rustynes_ppu;

mod bus;
// v2.0 R1c-1 diagnostic — per-instruction (PC, cpu_cycle) trace ring (gated).
#[cfg(feature = "cpu-instr-cycle-trace")]
pub use bus::instr_trace;
mod bus_snapshot;
mod controller;
#[cfg(feature = "cpu-boot-trace")]
pub mod cpu_boot_trace;
pub mod debug;
pub mod genie;
pub mod input_device;
#[cfg(feature = "irq-timing-trace")]
pub mod irq_trace;
mod movie;
mod nes;
mod rewind;
pub mod save_state;
pub mod scheduler;
pub mod vs_db;

pub use bus::LockstepBus;
pub use controller::{Buttons, Controller};
pub use debug::{ApuDebugView, CpuDebugView, MapperDebugView, PpuDebugView};
pub use genie::{GenieCode, GenieError};
pub use input_device::{InputDevice, PowerPadState, VausState, ZapperState};
pub use movie::{
    FrameInput, Movie, MovieError, MoviePlayer, MovieRecorder, StartPoint, BYTES_PER_FRAME,
    MOVIE_FORMAT_VERSION, MOVIE_MAGIC,
};
#[cfg(feature = "debug-hooks")]
pub use nes::TraceRec;
pub use nes::{Nes, FRAME_DURATION_DENDY, FRAME_DURATION_NTSC, FRAME_DURATION_PAL};
pub use rewind::{
    RewindError, RewindRing, REWIND_DEFAULT_KEYFRAME_PERIOD, REWIND_DEFAULT_MAX_BYTES,
};
pub use save_state::{
    parse_header, tag, tag_string, write_header, write_section, BinReader, BinWriter, Header,
    Section, SectionIter, SnapshotError, FORMAT_VERSION, HEADER_LEN, MAGIC, ROM_HASH_TAG_LEN,
    THUMBNAIL_HEIGHT, THUMBNAIL_LEN, THUMBNAIL_VERSION, THUMBNAIL_WIDTH,
};
pub use scheduler::M2Phase;
pub use vs_db::{lookup as vs_db_lookup, VsDbEntry};

/// Returns the crate version string.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// NES region (governs clock dividers, scanline counts, audio rate tables).
///
/// See `docs/glossary.md` for definitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Region {
    /// NTSC (Japan, North America, Australia). 60 Hz, 262 scanlines.
    Ntsc,
    /// PAL (Europe). 50 Hz, 312 scanlines.
    Pal,
    /// Dendy (Russian PAL famiclone). 50 Hz, 312 scanlines, NTSC-style timing.
    Dendy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
