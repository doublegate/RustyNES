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

/// Ricoh 2A03 CPU/APU die revision, selecting the hardware-revision difference
/// in the DMA unit's **"unexpected DMA" extra halt-read** (v2.1.7 "Hardware
/// Revisions & DMA Frontier").
///
/// # The frontier — read this before trusting the non-default arm
///
/// The 2A03 shipped in several mask revisions. nesdev
/// ([DMA](https://www.nesdev.org/wiki/DMA)) documents that when a DMC DMA halt
/// is requested on a CPU cycle where an OAM (`$4014`) DMA is *also* halting —
/// the "double-halt" overlap — some silicon performs an **extra** re-read of
/// the parked 6502 address bus before the transfer resumes (the "unexpected
/// DMA" read), and this differs by die revision.
///
/// **No public reference emulator models this die-revision difference, and no
/// public test ROM verifies it.** A survey of Mesen2, ares, `BizHawk`,
/// `TriCNES`, fceux, nestopia, `GeraNES`, and higan (v2.1.7, see ADR 0033)
/// found that *none*
/// branch DMA cycle behavior on 2A03 die stepping — the only revision-like
/// switch any of them models is the orthogonal **console-type** distinction
/// (Mesen2 `isNesBehavior`: NES-001/AV-Famicom clock a controller only on the
/// *first* DMA idle read, original Famicom on *every* one), which is a
/// different axis and is already reflected in this core's default
/// register-readout model. The die-revision extra-read is therefore a genuine
/// open frontier: this enum provides the **config surface** for it and a
/// conservative, deterministic model, but the [`Rp2A03H`](Self::Rp2A03H) arm's
/// direction is an **unverified hypothesis**, not an oracle-proven behavior.
///
/// # Contract
///
/// * **Default = [`Rp2A03G`](Self::Rp2A03G)** is **byte-identical** to the core
///   as it shipped before v2.1.7 (`AccuracyCoin` 141/141, nestest 0-diff, and
///   every committed DMA oracle ROM — the five `dmc_dma_during_read4` ROMs and
///   both `sprdma_and_dmc_dma` ROMs — still `Passed`).
/// * **[`Rp2A03H`](Self::Rp2A03H)** is a purely additive, opt-in knob that
///   *omits* the double-halt extra read in the model. It is deterministic and
///   reachable only when explicitly selected; the shipped/default build never
///   touches it. **On this engine the extra-read gate is a documented no-op on
///   every committed oracle**, so today `Rp2A03H` produces a **byte-identical**
///   result to `Rp2A03G` across the entire committed DMA corpus (proven by the
///   `cpu_2a03_revision` tests): the halted-DMC overlap-read fires but its
///   parked address is always the post-`$4014` instruction fetch, never a
///   side-effect register (see below). The revision difference is therefore a
///   mechanism-level *model*, not an observable divergence — ADR 0033.
///
/// The revision is a **config knob re-applied on load, not part of the
/// save-state** (like the optional OAM-decay model): the only state it
/// influences is fully re-derived from the deterministic timeline, so a
/// save/restore round-trip stays byte-identical for a fixed revision. See
/// `docs/adr/0033-cpu-2a03-revision-dma-frontier.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum Cpu2A03Revision {
    /// RP2A03G — the common early/mid die the accuracy oracles were captured
    /// against. **Performs** the double-halt "unexpected DMA" extra read.
    /// This is the **default** and the byte-identical baseline.
    #[default]
    Rp2A03G,
    /// RP2A03H — a later die modeled as **omitting** the double-halt extra
    /// read. Opt-in, additive, deterministic — but an **unverified** direction
    /// (no reference / no ROM proves it; see the type-level docs and ADR 0033).
    Rp2A03H,
}

impl Cpu2A03Revision {
    /// Whether this revision performs the "unexpected DMA" extra re-read of the
    /// parked address bus on the DMC-halt-coincides-with-OAM-halt overlap
    /// cycle. `true` for [`Rp2A03G`](Self::Rp2A03G) (the default /
    /// byte-identical baseline), `false` for [`Rp2A03H`](Self::Rp2A03H).
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
