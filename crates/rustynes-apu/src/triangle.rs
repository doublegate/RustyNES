// Triangle channel - 32-step triangle wave with linear counter

use crate::length_counter::LengthCounter;

/// Triangle wave sequence (32 steps)
/// Produces values 15 → 0 → 15 in a triangle pattern
const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

/// Triangle channel implementation
///
/// The triangle channel generates a triangle wave using a 32-step sequencer.
/// Unlike pulse channels, it has no envelope and uses a linear counter instead
/// of just a length counter.
///
/// # Registers
///
/// - `$4008`: Control flag and linear counter reload
/// - `$4009`: Unused
/// - `$400A`: Timer low 8 bits
/// - `$400B`: Length counter load and timer high 3 bits
#[derive(Debug, Clone)]
pub struct TriangleChannel {
    // Sequencer
    sequence_position: u8, // 0-31 position in triangle wave

    // Linear counter (triangle-specific timing)
    linear_counter: u8, // 7-bit counter
    linear_reload: u8,  // Reload value
    control_flag: bool, // Halt length and reload linear
    reload_flag: bool,  // Reload linear counter flag

    // Length counter
    length_counter: LengthCounter,

    // Timer
    timer: u16,         // 11-bit period (0-2047)
    timer_counter: u16, // Current timer value

    // State
    enabled: bool,
}

impl TriangleChannel {
    /// Create a new triangle channel
    #[must_use]
    pub fn new() -> Self {
        Self {
            sequence_position: 0,
            linear_counter: 0,
            linear_reload: 0,
            control_flag: false,
            reload_flag: false,
            length_counter: LengthCounter::new(),
            timer: 0,
            timer_counter: 0,
            enabled: false,
        }
    }

    /// Write to triangle channel register
    ///
    /// # Arguments
    ///
    /// * `addr` - Register offset (0-3 for $4008-$400B)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $4008: CRRR RRRR
                // C = Control flag (halt length counter and linear counter reload)
                // R = Linear counter reload value
                self.control_flag = (value & 0x80) != 0;
                self.linear_reload = value & 0x7F;

                // Control flag also sets length counter halt
                self.length_counter.set_halt(self.control_flag);
            }
            2 => {
                // $400A: TTTT TTTT
                // T = Timer low 8 bits
                self.timer = (self.timer & 0xFF00) | u16::from(value);
            }
            3 => {
                // $400B: LLLL LTTT
                // L = Length counter index
                // T = Timer high 3 bits
                self.timer = (self.timer & 0x00FF) | (u16::from(value & 0x07) << 8);

                // Load length counter if enabled
                let length_index = (value >> 3) & 0x1F;
                if self.enabled {
                    self.length_counter.load(length_index);
                }

                // Set linear counter reload flag
                self.reload_flag = true;
            }
            _ => {}
        }
    }

    /// Set channel enable state
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter.set_enabled(false);
        }
    }

    /// Clock the linear counter (called on quarter frames)
    ///
    /// The linear counter is clocked every quarter frame:
    /// - If reload flag is set, reload the linear counter
    /// - Otherwise, decrement if non-zero
    /// - Clear reload flag if control flag is clear
    pub fn clock_linear_counter(&mut self) {
        if self.reload_flag {
            self.linear_counter = self.linear_reload;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.reload_flag = false;
        }
    }

    /// Clock the length counter (called on half frames)
    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }

    /// Clock the timer (called every CPU cycle)
    ///
    /// The timer is clocked every CPU cycle. When it reaches 0:
    /// - Reload from timer period
    /// - Advance sequencer position if channel is active
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;

            // Clock sequencer if channel is active
            if self.is_active() {
                self.sequence_position = (self.sequence_position + 1) % 32;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    /// Check if channel is active
    ///
    /// Triangle produces output only if:
    /// - Channel is enabled
    /// - Length counter is non-zero
    /// - Linear counter is non-zero
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.length_counter.is_active() && self.linear_counter > 0
    }

    /// Get current output value
    ///
    /// Returns the current value from the triangle sequence (0-15).
    /// Returns 0 if channel is inactive or frequency is ultrasonic.
    #[must_use]
    pub fn output(&self) -> u8 {
        if !self.is_active() {
            return 0;
        }

        // Silence ultrasonic frequencies (timer < 2) to reduce popping
        // Frequencies above ~50 kHz can cause audio artifacts
        if self.timer < 2 {
            return 0;
        }

        TRIANGLE_SEQUENCE[self.sequence_position as usize]
    }

    /// Check if length counter is active (for $4015 status read)
    #[must_use]
    pub fn length_counter_active(&self) -> bool {
        self.length_counter.is_active()
    }
}

impl Default for TriangleChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triangle_sequence() {
        // Verify sequence matches expected triangle wave
        // First half: 15 → 0
        for i in 0..16 {
            assert_eq!(TRIANGLE_SEQUENCE[i], 15 - i as u8);
        }
        // Second half: 0 → 15
        for i in 16..32 {
            assert_eq!(TRIANGLE_SEQUENCE[i], (i - 16) as u8);
        }
    }

    #[test]
    fn test_triangle_new() {
        let triangle = TriangleChannel::new();
        assert_eq!(triangle.sequence_position, 0);
        assert_eq!(triangle.linear_counter, 0);
        assert_eq!(triangle.linear_reload, 0);
        assert!(!triangle.control_flag);
        assert!(!triangle.reload_flag);
        assert!(!triangle.enabled);
    }

    #[test]
    fn test_linear_counter_reload() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0); // Non-zero length

        // Set linear counter reload value WITH control flag
        triangle.write_register(0, 0xFF); // Control = 1, reload = 127

        // Write to $400B sets reload flag
        triangle.write_register(3, 0x00);
        assert!(triangle.reload_flag);

        // Clock linear counter
        triangle.clock_linear_counter();
        assert_eq!(triangle.linear_counter, 127);
        assert!(triangle.reload_flag); // Still set due to control flag
    }

    #[test]
    fn test_linear_counter_countdown() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0);

        // Set reload value without control flag
        triangle.write_register(0, 0x05); // Reload = 5, control = 0
        triangle.write_register(3, 0x00); // Trigger reload

        // First clock reloads
        triangle.clock_linear_counter();
        assert_eq!(triangle.linear_counter, 5);
        assert!(!triangle.reload_flag); // Cleared because control flag is 0

        // Subsequent clocks decrement
        triangle.clock_linear_counter();
        assert_eq!(triangle.linear_counter, 4);

        triangle.clock_linear_counter();
        assert_eq!(triangle.linear_counter, 3);
    }

    #[test]
    fn test_control_flag_behavior() {
        let mut triangle = TriangleChannel::new();

        // Control flag set
        triangle.write_register(0, 0x80); // Control = 1
        assert!(triangle.control_flag);
        assert!(triangle.length_counter.is_halted());

        // Control flag clear
        triangle.write_register(0, 0x00); // Control = 0
        assert!(!triangle.control_flag);
        assert!(!triangle.length_counter.is_halted());
    }

    #[test]
    fn test_timer_period() {
        let mut triangle = TriangleChannel::new();

        // Set timer period = 256 (0x100)
        triangle.write_register(2, 0x00); // Low = 0
        triangle.write_register(3, 0x08); // Bits 0-2 of 0x08 = 0, length index = 1
                                          // Actually 0x08 = 0000 1000, so bits 0-2 = 000 = 0
                                          // Let's set it correctly: we want high 3 bits = 1
        triangle.write_register(2, 0x00); // Low = 0
        triangle.write_register(3, 0x09); // 0000 1001, bits 0-2 = 001 = 1
        assert_eq!(triangle.timer, 0x100);

        // Set timer period = 0x7FF (max 11-bit)
        triangle.write_register(2, 0xFF); // Low = 255
        triangle.write_register(3, 0x3F); // 0011 1111, bits 0-2 = 111 = 7
        assert_eq!(triangle.timer, 0x7FF);
    }

    #[test]
    fn test_ultrasonic_silencing() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0); // Non-zero length
        triangle.linear_counter = 10;

        // Ultrasonic frequency (timer < 2) should silence
        triangle.timer = 1;
        assert_eq!(triangle.output(), 0);

        // Normal frequency should produce output
        triangle.timer = 100;
        assert!(triangle.output() > 0);
    }

    #[test]
    fn test_timer_clocking() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0);
        triangle.linear_counter = 10;
        triangle.timer = 3; // Short period for testing
        triangle.timer_counter = 3;

        let initial_pos = triangle.sequence_position;

        // Clock timer 4 times (3, 2, 1, 0)
        for _ in 0..4 {
            triangle.clock_timer();
        }

        // Sequence position should have advanced
        assert_eq!(triangle.sequence_position, (initial_pos + 1) % 32);
        assert_eq!(triangle.timer_counter, 3); // Reloaded
    }

    #[test]
    fn test_sequence_wraparound() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0);
        triangle.linear_counter = 10;
        triangle.timer = 0;
        triangle.sequence_position = 31;

        triangle.clock_timer();

        // Should wrap to 0
        assert_eq!(triangle.sequence_position, 0);
    }

    #[test]
    fn test_is_active() {
        let mut triangle = TriangleChannel::new();

        // Not active when disabled
        assert!(!triangle.is_active());

        // Enable but no length counter
        triangle.set_enabled(true);
        assert!(!triangle.is_active());

        // Add length counter but no linear counter
        triangle.length_counter.load(0);
        assert!(!triangle.is_active());

        // Add linear counter
        triangle.linear_counter = 1;
        assert!(triangle.is_active());
    }

    #[test]
    fn test_output_when_inactive() {
        let mut triangle = TriangleChannel::new();
        triangle.timer = 100; // Non-ultrasonic

        // Should output 0 when inactive
        assert_eq!(triangle.output(), 0);

        // Activate
        triangle.set_enabled(true);
        triangle.length_counter.load(0);
        triangle.linear_counter = 1;

        // Should now output from sequence
        assert_eq!(triangle.output(), TRIANGLE_SEQUENCE[0]);
    }

    #[test]
    fn test_enable_clears_length() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0);

        assert!(triangle.length_counter.is_active());

        // Disable should clear length counter
        triangle.set_enabled(false);
        assert!(!triangle.length_counter.is_active());
    }

    #[test]
    fn test_length_counter_load_when_disabled() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(false);

        // Write length counter index
        triangle.write_register(3, 0x08); // Index = 1

        // Length counter should NOT load when disabled
        assert!(!triangle.length_counter.is_active());
    }

    #[test]
    fn test_linear_counter_halt_by_control_flag() {
        let mut triangle = TriangleChannel::new();
        triangle.set_enabled(true);
        triangle.length_counter.load(0);

        // Set control flag and reload
        triangle.write_register(0, 0x85); // Control = 1, reload = 5
        triangle.write_register(3, 0x00); // Trigger reload flag

        // Clock linear counter multiple times
        for _ in 0..10 {
            triangle.clock_linear_counter();
        }

        // Linear counter should reload every time due to control flag
        assert_eq!(triangle.linear_counter, 5);
        assert!(triangle.reload_flag);
    }
}
