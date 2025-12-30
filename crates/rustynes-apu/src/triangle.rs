//! APU Triangle Channel.
//!
//! The triangle channel generates a triangle wave at a frequency controlled
//! by an 11-bit timer. Unlike the pulse channels, it has no volume control
//! but uses a linear counter for note duration control.
//!
//! The triangle wave cycles through values 15, 14, 13, ..., 1, 0, 0, 1, ..., 14, 15
//! producing a 32-step waveform.

use crate::{length_counter::LengthCounter, timer::Timer};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Triangle waveform lookup table (32 steps).
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

/// Triangle channel.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Triangle {
    /// Length counter.
    length_counter: LengthCounter,
    /// Timer.
    timer: Timer,
    /// Linear counter reload value.
    linear_counter_reload: u8,
    /// Linear counter current value.
    linear_counter: u8,
    /// Linear counter reload flag.
    linear_counter_reload_flag: bool,
    /// Control flag (also length counter halt).
    control_flag: bool,
    /// Current sequencer position (0-31).
    sequencer: u8,
}

impl Triangle {
    /// Create a new triangle channel.
    #[must_use]
    pub fn new() -> Self {
        Self {
            length_counter: LengthCounter::new(),
            timer: Timer::new(),
            linear_counter_reload: 0,
            linear_counter: 0,
            linear_counter_reload_flag: false,
            control_flag: false,
            sequencer: 0,
        }
    }

    /// Write to register $4008 (linear counter).
    pub fn write_linear_counter(&mut self, value: u8) {
        self.control_flag = value & 0x80 != 0;
        self.linear_counter_reload = value & 0x7F;
        self.length_counter.set_halt(self.control_flag);
    }

    /// Write to register $400A (timer low).
    pub fn write_timer_lo(&mut self, value: u8) {
        self.timer.set_period_lo(value);
    }

    /// Write to register $400B (length counter, timer high).
    pub fn write_timer_hi(&mut self, value: u8) {
        self.timer.set_period_hi(value);
        self.length_counter.load(value >> 3);
        self.linear_counter_reload_flag = true;
    }

    /// Set the enabled state.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.length_counter.set_enabled(enabled);
    }

    /// Check if the channel is active (length counter > 0).
    #[must_use]
    pub fn active(&self) -> bool {
        self.length_counter.active()
    }

    /// Clock the timer. Should be called every CPU cycle.
    /// Note: Triangle timer is clocked every CPU cycle, not APU cycle.
    pub fn clock_timer(&mut self) {
        if self.timer.clock() {
            // Only advance sequencer if both counters are non-zero
            if self.length_counter.active() && self.linear_counter > 0 {
                self.sequencer = (self.sequencer + 1) & 0x1F;
            }
        }
    }

    /// Clock the linear counter. Should be called on quarter frames.
    pub fn clock_linear_counter(&mut self) {
        if self.linear_counter_reload_flag {
            self.linear_counter = self.linear_counter_reload;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.linear_counter_reload_flag = false;
        }
    }

    /// Clock the length counter. Should be called on half frames.
    pub fn clock_length(&mut self) {
        self.length_counter.clock();
    }

    /// Get the current output value (0-15).
    #[must_use]
    pub fn output(&self) -> u8 {
        // Silenced if either counter is zero
        if !self.length_counter.active() || self.linear_counter == 0 {
            return 0;
        }

        // Also silence on ultrasonic frequencies (period < 2)
        // This prevents popping artifacts
        if self.timer.period() < 2 {
            return 0;
        }

        TRIANGLE_TABLE[self.sequencer as usize]
    }

    /// Get the length counter value.
    #[must_use]
    pub fn length_counter_value(&self) -> u8 {
        self.length_counter.value()
    }
}

impl Default for Triangle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triangle_table() {
        // First half descends from 15 to 0
        assert_eq!(TRIANGLE_TABLE[0], 15);
        assert_eq!(TRIANGLE_TABLE[15], 0);
        // Second half ascends from 0 to 15
        assert_eq!(TRIANGLE_TABLE[16], 0);
        assert_eq!(TRIANGLE_TABLE[31], 15);
    }

    #[test]
    fn test_triangle_output() {
        let mut triangle = Triangle::new();
        triangle.set_enabled(true);
        triangle.write_linear_counter(0x7F); // Max linear counter
        triangle.write_timer_lo(0x10); // Period > 2
        triangle.write_timer_hi(0xF8); // Load length counter

        // Clock linear counter to load value
        triangle.clock_linear_counter();

        assert_eq!(triangle.output(), TRIANGLE_TABLE[0]);
    }

    #[test]
    fn test_triangle_muted_when_disabled() {
        let mut triangle = Triangle::new();
        triangle.set_enabled(false);
        triangle.write_linear_counter(0x7F);
        triangle.write_timer_lo(0x10);
        triangle.write_timer_hi(0xF8);
        triangle.clock_linear_counter();

        assert_eq!(triangle.output(), 0);
    }

    #[test]
    fn test_triangle_muted_ultrasonic() {
        let mut triangle = Triangle::new();
        triangle.set_enabled(true);
        triangle.write_linear_counter(0x7F);
        triangle.write_timer_lo(0x01); // Period < 2
        triangle.write_timer_hi(0xF8);
        triangle.clock_linear_counter();

        // Period < 2 should mute
        assert_eq!(triangle.output(), 0);
    }

    #[test]
    fn test_linear_counter() {
        let mut triangle = Triangle::new();
        triangle.set_enabled(true);
        triangle.write_linear_counter(0x03); // Control=0, reload=3
        triangle.write_timer_lo(0x10);
        triangle.write_timer_hi(0xF8);

        // First clock loads the linear counter
        triangle.clock_linear_counter();
        assert!(triangle.linear_counter > 0);

        // Clear reload flag on next clock (control=0)
        triangle.clock_linear_counter();

        // Count down
        for _ in 0..10 {
            triangle.clock_linear_counter();
        }
        assert_eq!(triangle.linear_counter, 0);
    }
}
