//! Length Counter Module
//!
//! The length counter automatically silences a channel after a specific duration.
//! Used by all channels except DMC.

/// Length counter lookup table (32 entries)
///
/// Indexed by the 5-bit length index from channel registers.
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, // 0-7
    160, 8, 60, 10, 14, 12, 26, 14, // 8-15
    12, 16, 24, 18, 48, 20, 96, 22, // 16-23
    192, 24, 72, 26, 16, 28, 32, 30, // 24-31
];

/// Length counter for audio channel duration
///
/// The length counter decrements at ~120 Hz (every half frame) and silences
/// the channel when it reaches 0.
///
/// # Halt Behavior
///
/// If the halt flag is set, the length counter will not decrement.
/// For pulse and noise channels, the halt flag is the same as the envelope loop flag.
#[derive(Debug, Clone, Copy)]
pub struct LengthCounter {
    /// Current counter value (0-254)
    counter: u8,
    /// Halt flag: prevents counter from decrementing
    halt: bool,
}

impl LengthCounter {
    /// Creates a new length counter (starts at 0, disabled)
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counter: 0,
            halt: false,
        }
    }

    /// Loads a new value into the length counter from the lookup table
    ///
    /// # Arguments
    ///
    /// * `index` - 5-bit index (0-31) into the length table
    ///
    /// # Panics
    ///
    /// Panics if index >= 32 (should never happen with properly masked input)
    pub fn load(&mut self, index: u8) {
        self.counter = LENGTH_TABLE[index as usize];
    }

    /// Sets the halt flag
    ///
    /// When halt is true, the counter will not decrement.
    pub fn set_halt(&mut self, halt: bool) {
        self.halt = halt;
    }

    /// Returns the halt flag state
    #[must_use]
    pub const fn halt(self) -> bool {
        self.halt
    }

    /// Clocks the length counter (called on half frame)
    ///
    /// Decrements the counter if not halted and greater than 0.
    pub fn clock(&mut self) {
        if !self.halt && self.counter > 0 {
            self.counter -= 1;
        }
    }

    /// Returns whether the length counter is active (greater than 0)
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.counter > 0
    }

    /// Returns the current counter value
    #[must_use]
    pub const fn value(self) -> u8 {
        self.counter
    }

    /// Sets the enabled state (called from $4015)
    ///
    /// When disabled, the counter is immediately set to 0.
    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.counter = 0;
        }
    }

    /// Returns whether the halt flag is set
    #[must_use]
    pub const fn is_halted(self) -> bool {
        self.halt
    }
}

impl Default for LengthCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_length_table() {
        // Verify known length table values
        assert_eq!(LENGTH_TABLE[0], 10);
        assert_eq!(LENGTH_TABLE[1], 254);
        assert_eq!(LENGTH_TABLE[31], 30);
    }

    #[test]
    fn test_length_counter_load() {
        let mut lc = LengthCounter::new();

        // Load value at index 0 (should be 10)
        lc.load(0);
        assert_eq!(lc.counter, 10);
        assert!(lc.is_active());

        // Load value at index 1 (should be 254)
        lc.load(1);
        assert_eq!(lc.counter, 254);
    }

    #[test]
    fn test_length_counter_clock() {
        let mut lc = LengthCounter::new();
        lc.load(0); // Load 10

        // Clock once
        lc.clock();
        assert_eq!(lc.counter, 9);
        assert!(lc.is_active());

        // Clock 8 more times to reach 1
        for _ in 0..8 {
            lc.clock();
        }
        assert_eq!(lc.counter, 1);
        assert!(lc.is_active());

        // Clock once more to reach 0
        lc.clock();
        assert_eq!(lc.counter, 0);
        assert!(!lc.is_active());

        // Clock again (should stay at 0)
        lc.clock();
        assert_eq!(lc.counter, 0);
    }

    #[test]
    fn test_length_counter_halt() {
        let mut lc = LengthCounter::new();
        lc.load(0); // Load 10

        // Set halt flag
        lc.set_halt(true);
        assert!(lc.halt());

        // Clock (should not decrement)
        lc.clock();
        assert_eq!(lc.counter, 10);

        lc.clock();
        assert_eq!(lc.counter, 10);

        // Clear halt flag
        lc.set_halt(false);
        assert!(!lc.halt());

        // Clock (should now decrement)
        lc.clock();
        assert_eq!(lc.counter, 9);
    }

    #[test]
    fn test_set_enabled() {
        let mut lc = LengthCounter::new();
        lc.load(0); // Load 10
        assert_eq!(lc.counter, 10);

        // Disable channel
        lc.set_enabled(false);
        assert_eq!(lc.counter, 0);
        assert!(!lc.is_active());

        // Re-enable (should not affect counter)
        lc.set_enabled(true);
        assert_eq!(lc.counter, 0);

        // Load new value
        lc.load(1); // Load 254
        assert_eq!(lc.counter, 254);
        assert!(lc.is_active());
    }

    #[test]
    fn test_length_counter_full_decay() {
        let mut lc = LengthCounter::new();
        lc.load(3); // Load value 2

        assert_eq!(lc.counter, 2);

        // Decrement to 0
        lc.clock();
        assert_eq!(lc.counter, 1);

        lc.clock();
        assert_eq!(lc.counter, 0);
        assert!(!lc.is_active());
    }
}
