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
pub mod bk2_interop;
mod bus_snapshot;
mod controller;
#[cfg(feature = "cpu-boot-trace")]
pub mod cpu_boot_trace;
pub mod debug;
pub mod genie;
pub mod input_device;
#[cfg(feature = "irq-timing-trace")]
pub mod irq_trace;
// v1.7.0 "Forge" G4 — legacy NES TAS movie importers (.fcm / .fmv / .vmv;
// .mc2 is PC Engine and rejected). `no_std`-clean byte parsers, mirroring the
// `.fm2`/`.bk2` interop design; reuse the canonical power-on alignment.
pub mod legacy_movie;
mod movie;
pub mod movie_interop;
mod nes;
mod rewind;
pub mod save_state;
pub mod scheduler;
pub mod vs_db;
// v2.0.0 beta.5 (Workstream C) — the Vs. DualSystem dual-core wrapper: two
// complete Nes instances + the cabinet's $4016-bit-1 comms protocol, the
// shared 2 KiB WRAM swap, and the 5-CPU-cycle soft-lockstep. See
// `docs/audit/vs-dualsystem-design-2026-06-11.md`.
pub mod vs_dualsystem;
// v1.7.0 "Forge" Workstream D2 — the Zwinder-class compressed, density-tiered
// state manager (XOR-delta + LZ4 over the v1.6.0 uncompressed greenzone, with
// reserved anchors), scaling the TAStudio greenzone to feature-length TASes.
// Determinism-neutral: lossless round-trip, no timebase change. See
// `docs/rewind.md` §Zwinder.
pub mod zwinder;

pub use bus::LockstepBus;
#[cfg(feature = "debug-hooks")]
pub use bus::{AccessRec, EventBpKind, EventBreakHit, EventKind, EventRec, InterruptRec};
pub use controller::{Buttons, Controller};
pub use debug::{ApuDebugView, CpuDebugView, MapperDebugView, PpuDebugView};
pub use genie::{GenieCode, GenieError};
pub use input_device::{
    BandaiHyperShotState, FamilyKeyboardState, InputDevice, KonamiHyperShotState, PowerPadState,
    SnesMouseState, VausState, ZapperState,
};
pub use legacy_movie::{
    LegacyMeta, LegacyMovieError, import_fcm, import_fmv, import_mc2, import_vmv,
};
pub use movie::{
    BYTES_PER_FRAME, FrameInput, MOVIE_FORMAT_VERSION, MOVIE_MAGIC, Movie, MovieError, MoviePlayer,
    MovieRecorder, StartPoint, recorded_before_v2_timebase,
};
#[cfg(feature = "debug-hooks")]
pub use nes::TraceRec;
pub use nes::{
    FRAME_DURATION_DENDY, FRAME_DURATION_NTSC, FRAME_DURATION_PAL, Nes, PowerOnConfig, PowerOnRam,
};
// v2.1.7 P5 — re-export the PPU-side hardware-revision knobs at the core surface
// so downstream consumers (frontend, test-harness) depend on `rustynes-core`.
pub use rewind::{
    REWIND_DEFAULT_KEYFRAME_PERIOD, REWIND_DEFAULT_MAX_BYTES, RewindError, RewindRing,
};
pub use rustynes_ppu::{PaletteInit, PpuRevision};
pub use save_state::{
    BinReader, BinWriter, FORMAT_VERSION, HEADER_LEN, Header, MAGIC, ROM_HASH_TAG_LEN, Section,
    SectionIter, SnapshotError, THUMBNAIL_HEIGHT, THUMBNAIL_LEN, THUMBNAIL_VERSION,
    THUMBNAIL_WIDTH, parse_header, tag, tag_string, write_header, write_section,
};
pub use scheduler::M2Phase;
pub use vs_db::{VsDbEntry, lookup as vs_db_lookup};
pub use vs_dualsystem::{Emu, VsDualSystem};
pub use zwinder::{
    ZWINDER_DEFAULT_BUDGET_BYTES, ZWINDER_DEFAULT_KEYFRAME_INTERVAL, ZwinderError,
    ZwinderStateManager,
};

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

/// Ricoh 2A03 CPU/APU die revision, selecting the small hardware-revision
/// differences in the DMA unit (v2.1.7 "Hardware Revisions & DMA Frontier").
///
/// The 2A03 (and its PAL sibling the 2A07) shipped in several mask revisions
/// over the console's life. For the emulated deterministic core the *only*
/// externally-observable revision difference this enum gates is the DMA
/// unit's **"unexpected DMA" extra halt-read** behavior (nesdev
/// [DMA](https://www.nesdev.org/wiki/DMA) §"DMC DMA during OAM DMA" /
/// §"Unexpected DMA"): a DMC DMA whose halt is requested on the *same* CPU
/// cycle that the CPU already has a `$4014` OAM-DMA halt in flight (the "double
/// halt" alignment) inserts one **additional** re-read of the parked address
/// bus on the earlier-die parts before the transfer resumes. Everything else
/// about the DMA engine — the get/put alternation, OAM alignment, the aborted
/// DMC-DMA path, and the DMC-glitch register-readout corruption on
/// `$2007`/`$4015`/`$4016`/`$4017` — is revision-invariant in this model and
/// stays exactly as it is on every revision.
///
/// **Default = [`Cpu2A03Revision::Rp2A03G`]**, which is byte-identical to the
/// core as it shipped before v2.1.7 (`AccuracyCoin` 141/141, nestest 0-diff,
/// every DMA oracle ROM `Passed`). Selecting [`Cpu2A03Revision::Rp2A03H`] is a
/// purely additive, opt-in accuracy knob; it changes only the one alignment
/// bracket described above and is **not** part of the save-state (a config knob
/// re-applied on load, like the optional OAM-decay model — the DMA-engine
/// transient state it influences is fully re-derived from the deterministic
/// timeline, so a save/restore round-trip stays byte-identical for a fixed
/// revision). See `docs/adr/0033-cpu-2a03-revision-dma-frontier.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum Cpu2A03Revision {
    /// RP2A03G — the common early/mid die (front-loading NES-CPU-* boards,
    /// the vast majority of North-American/Japanese carts and the revision
    /// the accuracy oracles were captured against). Models the "unexpected
    /// DMA" extra halt-read. This is the **default** and the byte-identical
    /// baseline.
    #[default]
    Rp2A03G,
    /// RP2A03H — the later die (found on some top-loader / AV-Famicom era
    /// units) whose DMA unit omits the extra halt-read on the double-halt
    /// alignment. Opt-in; additive; not the default.
    Rp2A03H,
}

impl Cpu2A03Revision {
    /// Whether this revision inserts the "unexpected DMA" extra halt-read on
    /// the DMC-halt-coincides-with-OAM-halt alignment. `true` for
    /// [`Cpu2A03Revision::Rp2A03G`] (the default / byte-identical baseline),
    /// `false` for [`Cpu2A03Revision::Rp2A03H`].
    #[must_use]
    pub const fn has_unexpected_dma_extra_read(self) -> bool {
        matches!(self, Self::Rp2A03G)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
