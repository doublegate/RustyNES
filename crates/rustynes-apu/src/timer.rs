//! APU Timer.
//!
//! The timer is a divider that clocks the waveform generator at a rate
//! determined by the period value. It counts down and reloads when it
//! reaches 0.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Timer unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Timer {
    /// Current counter value.
    counter: u16,
    /// Period (reload value).
    period: u16,
}

impl Timer {
    /// Create a new timer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: 0,
            period: 0,
        }
    }

    /// Set the timer period low byte.
    pub fn set_period_lo(&mut self, value: u8) {
        self.period = (self.period & 0xFF00) | u16::from(value);
    }

    /// Set the timer period high byte (only lower 3 bits used).
    pub fn set_period_hi(&mut self, value: u8) {
        self.period = (self.period & 0x00FF) | (u16::from(value & 0x07) << 8);
    }

    /// Set the full timer period.
    pub fn set_period(&mut self, period: u16) {
        self.period = period & 0x07FF; // 11-bit period
    }

    /// Get the current period.
    #[must_use]
    #[inline]
    pub const fn period(&self) -> u16 {
        self.period
    }

    /// Reload the counter with the period value.
    pub fn reload(&mut self) {
        self.counter = self.period;
    }

    /// Clock the timer. Returns true if the counter wrapped (output should clock).
    pub fn clock(&mut self) -> bool {
        if self.counter == 0 {
            self.counter = self.period;
            true
        } else {
            self.counter -= 1;
            false
        }
    }

    /// Get the current counter value.
    #[must_use]
    #[inline]
    pub const fn counter(&self) -> u16 {
        self.counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_period() {
        let mut timer = Timer::new();
        timer.set_period_lo(0xAB);
        timer.set_period_hi(0x05);
        assert_eq!(timer.period(), 0x05AB);
    }

    #[test]
    fn test_period_mask() {
        let mut timer = Timer::new();
        timer.set_period(0xFFFF);
        assert_eq!(timer.period(), 0x07FF); // 11-bit max
    }

    #[test]
    fn test_clock() {
        let mut timer = Timer::new();
        timer.set_period(3);
        timer.reload();

        assert!(!timer.clock()); // 3 -> 2
        assert!(!timer.clock()); // 2 -> 1
        assert!(!timer.clock()); // 1 -> 0
        assert!(timer.clock()); // 0 -> reload, output
        assert_eq!(timer.counter(), 3);
    }

    #[test]
    fn test_clock_period_zero() {
        let mut timer = Timer::new();
        timer.set_period(0);
        timer.reload();

        assert!(timer.clock()); // Always outputs with period 0
        assert!(timer.clock());
    }
}
