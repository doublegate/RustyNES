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
#[cfg(feature = "debug-hooks")]
pub use bus::{AccessRec, EventKind, EventRec, InterruptRec};
pub use controller::{Buttons, Controller};
pub use debug::{ApuDebugView, CpuDebugView, MapperDebugView, PpuDebugView};
pub use genie::{GenieCode, GenieError};
pub use input_device::{
    FamilyKeyboardState, InputDevice, PowerPadState, SnesMouseState, VausState, ZapperState,
};
pub use movie::{
    BYTES_PER_FRAME, FrameInput, MOVIE_FORMAT_VERSION, MOVIE_MAGIC, Movie, MovieError, MoviePlayer,
    MovieRecorder, StartPoint,
};
#[cfg(feature = "debug-hooks")]
pub use nes::TraceRec;
pub use nes::{FRAME_DURATION_DENDY, FRAME_DURATION_NTSC, FRAME_DURATION_PAL, Nes};
pub use rewind::{
    REWIND_DEFAULT_KEYFRAME_PERIOD, REWIND_DEFAULT_MAX_BYTES, RewindError, RewindRing,
};
pub use save_state::{
    BinReader, BinWriter, FORMAT_VERSION, HEADER_LEN, Header, MAGIC, ROM_HASH_TAG_LEN, Section,
    SectionIter, SnapshotError, THUMBNAIL_HEIGHT, THUMBNAIL_LEN, THUMBNAIL_VERSION,
    THUMBNAIL_WIDTH, parse_header, tag, tag_string, write_header, write_section,
};
pub use scheduler::M2Phase;
pub use vs_db::{VsDbEntry, lookup as vs_db_lookup};

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
