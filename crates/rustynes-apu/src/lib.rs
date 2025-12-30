//! NES APU (Audio Processing Unit) Emulation.
//!
//! This crate provides a cycle-accurate implementation of the NES 2A03 APU,
//! which generates all audio for the NES. The APU contains five channels:
//!
//! - **Pulse 1 & 2**: Square wave generators with variable duty cycle
//! - **Triangle**: Triangle wave generator (fixed volume)
//! - **Noise**: Pseudo-random noise generator
//! - **DMC**: Delta modulation channel for sample playback
//!
//! # Architecture
//!
//! The APU runs at half the CPU clock rate. Each channel has its own
//! timer, and most have envelope and length counter units for volume
//! and duration control.
//!
//! The frame counter provides timing signals for envelope, length counter,
//! and sweep updates at specific cycle intervals.
//!
//! # Example
//!
//! ```no_run
//! use rustynes_apu::Apu;
//!
//! let mut apu = Apu::new();
//!
//! // Enable pulse channel 1
//! apu.write(0x4015, 0x01);
//!
//! // Configure pulse 1: 50% duty, constant volume 15
//! apu.write(0x4000, 0xBF);
//! apu.write(0x4002, 0xFD); // Timer low
//! apu.write(0x4003, 0x00); // Timer high + length counter
//!
//! // Clock the APU
//! for _ in 0..29780 {
//!     apu.clock();
//!     let sample = apu.output(); // 0.0 to 1.0
//! }
//! ```
//!
//! # no_std Support
//!
//! This crate supports `no_std` environments with the `alloc` crate.
//! Enable the `std` feature (enabled by default) for standard library support.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

mod apu;
mod dmc;
mod envelope;
mod frame_counter;
mod length_counter;
mod noise;
mod pulse;
mod sweep;
mod timer;
mod triangle;

pub use apu::Apu;
pub use dmc::Dmc;
pub use envelope::Envelope;
pub use frame_counter::{FrameCounter, FrameCounterMode, FrameEvent};
pub use length_counter::LengthCounter;
pub use noise::Noise;
pub use pulse::Pulse;
pub use sweep::{PulseChannel, Sweep};
pub use timer::Timer;
pub use triangle::Triangle;

/// NTSC CPU clock rate in Hz.
pub const CPU_CLOCK_NTSC: u32 = 1_789_773;

/// PAL CPU clock rate in Hz.
pub const CPU_CLOCK_PAL: u32 = 1_662_607;

/// NTSC APU sample rate (before resampling).
/// This is the CPU clock rate since we sample every CPU cycle.
pub const APU_SAMPLE_RATE_NTSC: u32 = CPU_CLOCK_NTSC;

/// Cycles per frame (NTSC).
pub const CYCLES_PER_FRAME_NTSC: u32 = 29780;

/// Cycles per frame (PAL).
pub const CYCLES_PER_FRAME_PAL: u32 = 33247;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(CPU_CLOCK_NTSC, 1_789_773);
        assert_eq!(CPU_CLOCK_PAL, 1_662_607);
        assert_eq!(CYCLES_PER_FRAME_NTSC, 29780);
    }

    #[test]
    fn test_apu_integration() {
        let mut apu = Apu::new();

        // Enable pulse 1
        apu.write(0x4015, 0x01);

        // Configure: 50% duty, constant volume 15
        apu.write(0x4000, 0xBF);
        apu.write(0x4002, 0xFD);
        apu.write(0x4003, 0xF8);

        // Clock for a while
        for _ in 0..1000 {
            apu.clock();
        }

        // Should produce some output
        // (exact value depends on sequencer position)
        let output = apu.output();
        assert!(output >= 0.0);
        assert!(output <= 1.0);
    }

    #[test]
    fn test_frame_counter_clocking() {
        let mut apu = Apu::new();

        // Set 5-step mode
        apu.write(0x4017, 0x80);

        // Clock through mode change delay
        for _ in 0..10 {
            apu.clock();
        }

        // Should be in 5-step mode now
        // Clock for a full frame
        for _ in 0..40000 {
            apu.clock();
        }

        // No IRQ in 5-step mode
        assert!(!apu.irq_pending());
    }

    #[test]
    fn test_triangle_channel() {
        let mut apu = Apu::new();

        // Enable triangle
        apu.write(0x4015, 0x04);

        // Configure: max linear counter
        apu.write(0x4008, 0xFF);
        apu.write(0x400A, 0x20);
        apu.write(0x400B, 0xF8);

        // Clock to load linear counter
        for _ in 0..10000 {
            apu.clock();
        }

        let output = apu.output();
        assert!(output >= 0.0);
    }

    #[test]
    fn test_noise_channel() {
        let mut apu = Apu::new();

        // Enable noise
        apu.write(0x4015, 0x08);

        // Configure: constant volume 15
        apu.write(0x400C, 0x3F);
        apu.write(0x400E, 0x00);
        apu.write(0x400F, 0xF8);

        // Clock for a while
        for _ in 0..1000 {
            apu.clock();
        }

        let output = apu.output();
        assert!(output >= 0.0);
    }

    #[test]
    fn test_dmc_direct_load() {
        let mut apu = Apu::new();

        // Direct load to DMC
        apu.write(0x4011, 0x40);

        // DMC should output this level
        let output = apu.output();
        assert!(output > 0.0);
    }

    #[test]
    fn test_status_register() {
        let mut apu = Apu::new();

        // Initial status: nothing active
        let status = apu.read_status();
        assert_eq!(status & 0x1F, 0);

        // Enable and activate channels
        apu.write(0x4015, 0x1F);
        apu.write(0x4003, 0xF8);
        apu.write(0x4007, 0xF8);
        apu.write(0x400B, 0xF8);
        apu.write(0x400F, 0xF8);

        let status = apu.read_status();
        // At least pulse and triangle should be active
        assert!(status & 0x07 != 0);
    }
}
