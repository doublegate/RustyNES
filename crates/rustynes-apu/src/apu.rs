//! APU (Audio Processing Unit) Main Module.
//!
//! The NES APU (2A03) contains:
//! - Two pulse (square wave) channels
//! - One triangle channel
//! - One noise channel
//! - One DMC (delta modulation channel)
//! - Frame counter
//! - Mixer
//!
//! The APU runs at half the CPU clock rate (CPU/2).

use crate::{
    dmc::Dmc,
    frame_counter::{FrameCounter, FrameEvent},
    noise::Noise,
    pulse::Pulse,
    sweep::PulseChannel,
    triangle::Triangle,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Pulse output lookup table for the non-linear mixer.
/// pulse_out = 95.52 / (8128.0 / (pulse1 + pulse2) + 100)
#[allow(clippy::cast_precision_loss)] // Mixer table index fits in f32 mantissa
const PULSE_TABLE: [f32; 31] = {
    let mut table = [0.0f32; 31];
    let mut i = 0;
    while i < 31 {
        if i == 0 {
            table[i] = 0.0;
        } else {
            table[i] = 95.52 / (8128.0 / (i as f32) + 100.0);
        }
        i += 1;
    }
    table
};

/// TND (Triangle, Noise, DMC) output lookup table for the non-linear mixer.
/// tnd_out = 163.67 / (24329.0 / (3*triangle + 2*noise + dmc) + 100)
#[allow(clippy::cast_precision_loss)] // Mixer table index fits in f32 mantissa
const TND_TABLE: [f32; 203] = {
    let mut table = [0.0f32; 203];
    let mut i = 0;
    while i < 203 {
        if i == 0 {
            table[i] = 0.0;
        } else {
            table[i] = 163.67 / (24329.0 / (i as f32) + 100.0);
        }
        i += 1;
    }
    table
};

/// APU structure.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(dead_code)] // last_output reserved for future high-pass filtering
pub struct Apu {
    /// Pulse channel 1.
    pulse1: Pulse,
    /// Pulse channel 2.
    pulse2: Pulse,
    /// Triangle channel.
    triangle: Triangle,
    /// Noise channel.
    noise: Noise,
    /// DMC channel.
    dmc: Dmc,
    /// Frame counter.
    frame_counter: FrameCounter,
    /// Cycle counter (for APU cycles).
    cycle: u64,
    /// Last sampled output (for high-pass filtering).
    last_output: f32,
}

impl Apu {
    /// Create a new APU.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pulse1: Pulse::new(PulseChannel::One),
            pulse2: Pulse::new(PulseChannel::Two),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: Dmc::new(),
            frame_counter: FrameCounter::new(),
            cycle: 0,
            last_output: 0.0,
        }
    }

    /// Reset the APU to initial state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Read from an APU register.
    /// Only $4015 is readable.
    #[must_use]
    pub fn read_status(&mut self) -> u8 {
        let status = self.peek_status();

        // Reading status clears frame counter IRQ
        self.frame_counter.clear_irq();

        status
    }

    /// Peek at APU status without side effects.
    ///
    /// Returns the same value as `read_status()` but does not clear the
    /// frame counter IRQ. Useful for debugging/display purposes.
    #[must_use]
    pub fn peek_status(&self) -> u8 {
        let mut status = 0u8;

        if self.pulse1.active() {
            status |= 0x01;
        }
        if self.pulse2.active() {
            status |= 0x02;
        }
        if self.triangle.active() {
            status |= 0x04;
        }
        if self.noise.active() {
            status |= 0x08;
        }
        if self.dmc.active() {
            status |= 0x10;
        }
        if self.frame_counter.irq_pending() {
            status |= 0x40;
        }
        if self.dmc.irq_pending() {
            status |= 0x80;
        }

        status
    }

    /// Write to an APU register.
    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // Pulse 1
            0x4000 => self.pulse1.write_ctrl(value),
            0x4001 => self.pulse1.write_sweep(value),
            0x4002 => self.pulse1.write_timer_lo(value),
            0x4003 => self.pulse1.write_timer_hi(value),

            // Pulse 2
            0x4004 => self.pulse2.write_ctrl(value),
            0x4005 => self.pulse2.write_sweep(value),
            0x4006 => self.pulse2.write_timer_lo(value),
            0x4007 => self.pulse2.write_timer_hi(value),

            // Triangle
            0x4008 => self.triangle.write_linear_counter(value),
            0x400A => self.triangle.write_timer_lo(value),
            0x400B => self.triangle.write_timer_hi(value),

            // Noise
            0x400C => self.noise.write_ctrl(value),
            0x400E => self.noise.write_period(value),
            0x400F => self.noise.write_length(value),

            // DMC
            0x4010 => self.dmc.write_ctrl(value),
            0x4011 => self.dmc.write_direct_load(value),
            0x4012 => self.dmc.write_sample_address(value),
            0x4013 => self.dmc.write_sample_length(value),

            // Status
            0x4015 => {
                self.pulse1.set_enabled(value & 0x01 != 0);
                self.pulse2.set_enabled(value & 0x02 != 0);
                self.triangle.set_enabled(value & 0x04 != 0);
                self.noise.set_enabled(value & 0x08 != 0);
                self.dmc.set_enabled(value & 0x10 != 0);
            }

            // Frame counter
            0x4017 => self.frame_counter.write(value),

            _ => {}
        }
    }

    /// Clock the APU for one CPU cycle.
    /// The APU runs at half the CPU clock rate.
    pub fn clock(&mut self) {
        // Triangle timer clocks every CPU cycle
        self.triangle.clock_timer();

        // Other timers clock every other CPU cycle (APU cycle)
        if self.cycle % 2 == 1 {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
            self.dmc.clock_timer();
        }

        // Frame counter
        let events = self.frame_counter.clock();
        for event in events.iter().flatten() {
            match event {
                FrameEvent::QuarterFrame => {
                    self.pulse1.clock_envelope();
                    self.pulse2.clock_envelope();
                    self.triangle.clock_linear_counter();
                    self.noise.clock_envelope();
                }
                FrameEvent::HalfFrame => {
                    self.pulse1.clock_length();
                    self.pulse2.clock_length();
                    self.pulse1.clock_sweep();
                    self.pulse2.clock_sweep();
                    self.triangle.clock_length();
                    self.noise.clock_length();
                }
                FrameEvent::Irq => {
                    // IRQ is handled by checking irq_pending()
                }
            }
        }

        self.cycle = self.cycle.wrapping_add(1);
    }

    /// Get the mixed audio output (0.0 to 1.0).
    #[must_use]
    pub fn output(&self) -> f32 {
        let pulse1 = u16::from(self.pulse1.output());
        let pulse2 = u16::from(self.pulse2.output());
        let triangle = u16::from(self.triangle.output());
        let noise = u16::from(self.noise.output());
        let dmc = u16::from(self.dmc.output());

        // Use lookup tables for non-linear mixing
        let pulse_out = PULSE_TABLE[(pulse1 + pulse2) as usize];
        let tnd_index = 3 * triangle + 2 * noise + dmc;
        let tnd_out = TND_TABLE[tnd_index.min(202) as usize];

        pulse_out + tnd_out
    }

    /// Check if DMC needs a sample byte.
    #[must_use]
    pub fn dmc_needs_sample(&self) -> bool {
        self.dmc.needs_sample()
    }

    /// Get the DMC sample address.
    #[must_use]
    pub fn dmc_sample_addr(&self) -> u16 {
        self.dmc.sample_addr()
    }

    /// Fill the DMC sample buffer.
    pub fn dmc_fill_sample(&mut self, sample: u8) {
        self.dmc.fill_sample_buffer(sample);
    }

    /// Check if any APU IRQ is pending.
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.frame_counter.irq_pending() || self.dmc.irq_pending()
    }

    /// Get the current APU cycle count.
    #[must_use]
    pub fn cycle(&self) -> u64 {
        self.cycle
    }

    /// Get the current APU cycle count (alias for `cycle()`).
    #[must_use]
    pub fn cycles(&self) -> u64 {
        self.cycle
    }

    /// Get the DMC channel output (0-127).
    #[must_use]
    pub fn dmc_output(&self) -> u8 {
        self.dmc.output()
    }

    /// Get pulse 1 length counter value.
    #[must_use]
    pub fn pulse1_length(&self) -> u8 {
        self.pulse1.length_counter_value()
    }

    /// Get pulse 2 length counter value.
    #[must_use]
    pub fn pulse2_length(&self) -> u8 {
        self.pulse2.length_counter_value()
    }

    /// Get triangle length counter value.
    #[must_use]
    pub fn triangle_length(&self) -> u8 {
        self.triangle.length_counter_value()
    }

    /// Get noise length counter value.
    #[must_use]
    pub fn noise_length(&self) -> u8 {
        self.noise.length_counter_value()
    }

    /// Get DMC bytes remaining.
    #[must_use]
    pub fn dmc_bytes_remaining(&self) -> u16 {
        self.dmc.bytes_remaining()
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apu_initial() {
        let apu = Apu::new();
        assert_eq!(apu.cycle(), 0);
        assert!(!apu.irq_pending());
    }

    #[test]
    fn test_apu_status_read() {
        let mut apu = Apu::new();
        let status = apu.read_status();
        assert_eq!(status, 0); // All channels disabled initially
    }

    #[test]
    fn test_apu_enable_channels() {
        let mut apu = Apu::new();
        apu.write(0x4015, 0x1F); // Enable all channels

        // Write timer high to load length counters
        apu.write(0x4003, 0xF8);
        apu.write(0x4007, 0xF8);
        apu.write(0x400B, 0xF8);
        apu.write(0x400F, 0xF8);
        apu.write(0x4013, 0x10);
        apu.dmc.set_enabled(true); // DMC needs separate handling

        let status = apu.read_status();
        // Channels should be active
        assert!(status & 0x0F != 0);
    }

    #[test]
    fn test_apu_clock() {
        let mut apu = Apu::new();
        apu.clock();
        assert_eq!(apu.cycle(), 1);
        apu.clock();
        assert_eq!(apu.cycle(), 2);
    }

    #[test]
    fn test_apu_output_range() {
        let apu = Apu::new();
        let output = apu.output();
        assert!(output >= 0.0);
        assert!(output <= 1.0);
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::assertions_on_constants)]
    fn test_pulse_table() {
        assert_eq!(PULSE_TABLE[0], 0.0);
        assert!(PULSE_TABLE[30] > 0.0);
        assert!(PULSE_TABLE[30] < 1.0);
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::assertions_on_constants)]
    fn test_tnd_table() {
        assert_eq!(TND_TABLE[0], 0.0);
        assert!(TND_TABLE[202] > 0.0);
        assert!(TND_TABLE[202] < 1.0);
    }

    #[test]
    fn test_apu_reset() {
        let mut apu = Apu::new();
        apu.clock();
        apu.clock();
        apu.reset();
        assert_eq!(apu.cycle(), 0);
    }
}
