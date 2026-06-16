//! Cycle-accurate Ricoh 2A03 APU implementation.
//!
//! See `docs/apu-2a03.md` for the implementation spec and
//! `ref-docs/research-report.md` §APU for the source material.
//!
//! Five-channel APU (pulse 1, pulse 2, triangle, noise, DMC) with the
//! lookup-table non-linear mixer, analog highpass / lowpass filter chain,
//! frame counter (4-step + 5-step modes with the documented IRQ flag),
//! band-limited synthesis at host sample rate, and DMC sample DMA. The
//! bus-side DMC DMA scheduling lives in `rustynes-core::LockstepBus`.

#![no_std]
#![warn(missing_docs)]
// The APU is full of orthogonal hardware-latch booleans that map directly to
// real chip state; collapsing into enums obscures the model.
#![allow(clippy::struct_excessive_bools)]
// Many small mutator helpers are pure register-bit unpackers; the pedantic
// `const fn` lint generates a wave of suggestions that don't change behavior.
// We accept the lint at module level rather than salt every method.
#![allow(clippy::missing_const_for_fn)]
// Floating-point exact comparisons (`x == 0.0`, `phase >= 1.0`) are deliberate
// initial-state checks against zero; using `EPSILON` is the wrong tool for
// the test ROM coverage we're targeting.
#![allow(clippy::float_cmp, clippy::while_float)]
// "NESdev" is a proper noun, not a code identifier.
#![allow(clippy::doc_markdown)]
// Match arms collapse only sometimes; we keep them split for readability
// because the address ranges document the register layout.
#![allow(clippy::match_same_arms)]
// Performance-sensitive math wants explicit FMA / no-FMA control.  The
// pedantic `suboptimal_flops` lint suggests `mul_add` which has different
// rounding properties — we don't accept that for a deterministic build.
#![allow(clippy::suboptimal_flops)]

extern crate alloc;

mod apu;
mod blip;
mod blip_kernel;
mod dmc;
mod envelope;
mod frame_counter;
mod length;
mod mixer;
mod noise;
mod opll;
mod pulse;
mod snapshot;
mod triangle;

pub use apu::{Apu, ApuBus, CHANNEL_MASK_ALL};
// v2.0 R-1 core C-1 diagnostic — re-export the gated abort-scheduling probe
// counters so the harness can read `rustynes_core::rustynes_apu::abort_probe`.
#[cfg(feature = "mc-r1-dmc-abort-probe")]
pub use apu::abort_probe;
pub use blip::{BlipBuf, CPU_HZ_NTSC, CPU_HZ_PAL};
pub use dmc::Dmc;
pub use dmc::REENABLE_BUMP;
pub use dmc::SUBPOS_DELAY;
pub use envelope::Envelope;
pub use frame_counter::{FrameCounter, FrameEvents, Mode as FrameCounterMode};
pub use length::{LENGTH_TABLE, LengthCounter};
pub use mixer::{FilterChain, Mixer, OnePole};
pub use noise::{NTSC_NOISE_PERIODS, Noise, PAL_NOISE_PERIODS};
pub use opll::{ChipType as OpllChipType, Opll, Patch as OpllPatch};
pub use pulse::Pulse;
pub use snapshot::{APU_SNAPSHOT_VERSION, ApuSnapshotError};
pub use triangle::Triangle;

/// NES region — picks clock dividers and per-region tables.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Region {
    /// NTSC (60 Hz, 1.7898 MHz CPU).
    Ntsc,
    /// PAL (50 Hz, 1.6626 MHz CPU).
    Pal,
    /// Dendy (PAL famiclone with NTSC-like timing).
    Dendy,
}

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
