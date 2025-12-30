//! APU Length Counter.
//!
//! The length counter is used by the pulse, triangle, and noise channels to
//! automatically silence a channel after a specified time. It decrements on
//! each half-frame clock and silences the channel when it reaches 0.
//!
//! The counter can be halted, which prevents it from decrementing.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Length counter lookup table.
/// Index is the 5-bit value written to the length counter load register.
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

/// Length counter unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LengthCounter {
    /// Current counter value.
    counter: u8,
    /// Halt flag (when true, counter doesn't decrement).
    halt: bool,
    /// Enabled flag (when false, counter stays at 0).
    enabled: bool,
}

impl LengthCounter {
    /// Create a new length counter.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: 0,
            halt: false,
            enabled: false,
        }
    }

    /// Load a new length value using the lookup table.
    pub fn load(&mut self, index: u8) {
        if self.enabled {
            self.counter = LENGTH_TABLE[(index & 0x1F) as usize];
        }
    }

    /// Set the halt flag.
    pub fn set_halt(&mut self, halt: bool) {
        self.halt = halt;
    }

    /// Set the enabled flag.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.counter = 0;
        }
    }

    /// Clock the length counter.
    /// Should be called on half frames.
    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    /// Check if the counter is non-zero (channel should output).
    #[must_use]
    #[inline]
    pub const fn active(&self) -> bool {
        self.counter > 0
    }

    /// Get the current counter value.
    #[must_use]
    #[inline]
    pub const fn value(&self) -> u8 {
        self.counter
    }

    /// Check if the counter is enabled.
    #[must_use]
    #[inline]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_length_table() {
        // Verify some known values from the table
        assert_eq!(LENGTH_TABLE[0], 10);
        assert_eq!(LENGTH_TABLE[1], 254);
        assert_eq!(LENGTH_TABLE[30], 32);
        assert_eq!(LENGTH_TABLE[31], 30);
    }

    #[test]
    fn test_load() {
        let mut lc = LengthCounter::new();
        lc.set_enabled(true);
        lc.load(0);
        assert_eq!(lc.value(), 10);

        lc.load(1);
        assert_eq!(lc.value(), 254);
    }

    #[test]
    fn test_load_disabled() {
        let mut lc = LengthCounter::new();
        lc.set_enabled(false);
        lc.load(1);
        assert_eq!(lc.value(), 0);
    }

    #[test]
    fn test_clock() {
        let mut lc = LengthCounter::new();
        lc.set_enabled(true);
        lc.load(0); // Load 10

        for i in (0..10).rev() {
            lc.clock();
            assert_eq!(lc.value(), i);
        }

        // Should stay at 0
        lc.clock();
        assert_eq!(lc.value(), 0);
    }

    #[test]
    fn test_halt() {
        let mut lc = LengthCounter::new();
        lc.set_enabled(true);
        lc.load(0); // Load 10
        lc.set_halt(true);

        lc.clock();
        lc.clock();
        assert_eq!(lc.value(), 10); // Should not decrement
    }

    #[test]
    fn test_active() {
        let mut lc = LengthCounter::new();
        assert!(!lc.active());

        lc.set_enabled(true);
        lc.load(0);
        assert!(lc.active());
    }

    #[test]
    fn test_disable_clears_counter() {
        let mut lc = LengthCounter::new();
        lc.set_enabled(true);
        lc.load(0);
        assert!(lc.active());

        lc.set_enabled(false);
        assert!(!lc.active());
        assert_eq!(lc.value(), 0);
    }
}
