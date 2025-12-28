//! `RustyNES` APU - Nintendo Entertainment System Audio Processing Unit Emulation
//!
//! This crate provides a cycle-accurate emulation of the NES 2A03 APU (Audio Processing Unit).
//!
//! # Features
//!
//! - **5 Audio Channels**:
//!   - 2× Pulse channels (square waves with sweep)
//!   - 1× Triangle channel (triangle wave)
//!   - 1× Noise channel (pseudo-random noise)
//!   - 1× DMC channel (Delta Modulation Channel for sample playback)
//!
//! - **Hardware-Accurate Components**:
//!   - Frame counter with 4-step and 5-step modes
//!   - Envelope generators
//!   - Length counters
//!   - Sweep units
//!   - Non-linear mixing
//!
//! - **Zero Unsafe Code**: No `unsafe` blocks (enforced by `#![forbid(unsafe_code)]`)
//!
//! # Example Usage
//!
//! ```rust
//! use rustynes_apu::Apu;
//!
//! let mut apu = Apu::new();
//!
//! // Enable pulse 1
//! apu.write_register(0x4015, 0x01);
//!
//! // Configure pulse 1: 50% duty, constant volume 15
//! apu.write_register(0x4000, 0xBF);
//!
//! // Set frequency (A4 = 440 Hz)
//! // Timer = CPU_CLOCK / (16 * frequency) - 1
//! // Timer = 1789773 / (16 * 440) - 1 = 253
//! let timer: u16 = 253;
//! apu.write_register(0x4002, (timer & 0xFF) as u8);
//! apu.write_register(0x4003, ((timer >> 8) & 0x07) as u8);
//!
//! // Step the APU each CPU cycle
//! for _ in 0..1000 {
//!     apu.step();
//! }
//! ```
//!
//! # Architecture
//!
//! The APU consists of several interconnected components:
//!
//! - **Frame Counter** (`frame_counter`): Times envelope, length counter, and sweep updates
//! - **Envelope Generator** (`envelope`): Controls volume fade-in/fade-out
//! - **Length Counter** (`length_counter`): Automatically silences channels after a duration
//! - **Sweep Unit** (`sweep`): Modulates pulse channel frequencies
//! - **Channels** (`pulse`, `triangle`, `noise`, `dmc`): Individual audio generators
//! - **Mixer** (`mixer`): Combines channel outputs with non-linear mixing
//! - **Resampler** (`resampler`): Converts APU rate to target audio sample rate
//!
//! # Register Map
//!
//! The APU is controlled via memory-mapped registers at CPU addresses `$4000-$4017`:
//!
//! | Address | Channel | Register |
//! |---------|---------|----------|
//! | `$4000-$4003` | Pulse 1 | Duty, sweep, timer, length |
//! | `$4004-$4007` | Pulse 2 | Duty, sweep, timer, length |
//! | `$4008-$400B` | Triangle | Control, timer, length |
//! | `$400C-$400F` | Noise | Envelope, period, length |
//! | `$4010-$4013` | DMC | Flags, load, address, length |
//! | `$4015` | Status | Enable/status |
//! | `$4017` | Frame Counter | Mode, IRQ inhibit |

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
// Allow dead code until all components are integrated (Sprint 3.2+)
#![allow(dead_code)]

mod apu;
mod dmc;
mod envelope;
mod frame_counter;
mod length_counter;
pub mod mixer;
mod noise;
mod pulse;
pub mod resampler;
mod sweep;
mod triangle;

// Re-export public API
pub use apu::Apu;
pub use dmc::{DmcChannel, System};
pub use frame_counter::{FrameAction, FrameCounter};
pub use mixer::Mixer;
pub use noise::NoiseChannel;
pub use pulse::PulseChannel;
pub use resampler::{
    APU_RATE_NTSC, APU_RATE_PAL, FilterChain, HighPassFilter, HighQualityResampler,
    LinearResampler, LowPassFilter, Resampler, SAMPLE_RATE_44100, SAMPLE_RATE_48000,
};
pub use triangle::TriangleChannel;

// Keep internal components private for now
// They may be exposed later if needed for debugging/testing
