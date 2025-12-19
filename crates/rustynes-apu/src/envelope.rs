//! Envelope Generator Module
//!
//! The envelope generator controls volume fade-in/fade-out for pulse and noise channels.
//! It can operate in constant volume mode or envelope decay mode.

/// Envelope generator for pulse and noise channels
///
/// Controls volume using either:
/// - **Constant volume**: Fixed volume level (0-15)
/// - **Envelope decay**: Automatic fade from 15 to 0
///
/// The envelope is clocked by the frame counter at ~240 Hz (every quarter frame).
#[derive(Debug, Clone, Copy)]
pub struct Envelope {
    /// Start flag: restarts the envelope
    start_flag: bool,
    /// Loop flag: reload to 15 when reaching 0 (same as length counter halt)
    loop_flag: bool,
    /// Constant volume flag: use volume directly instead of envelope
    constant_volume: bool,
    /// Volume/envelope divider period (V bits: 0-15)
    volume: u8,

    /// Divider counter (counts down from volume)
    divider: u8,
    /// Decay level counter (0-15, current envelope output)
    decay_level: u8,
}

impl Envelope {
    /// Creates a new envelope generator
    #[must_use]
    pub const fn new() -> Self {
        Self {
            start_flag: false,
            loop_flag: false,
            constant_volume: false,
            volume: 0,
            divider: 0,
            decay_level: 0,
        }
    }

    /// Writes to the envelope control register
    ///
    /// # Register Format
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// --LC VVVV
    ///   || ||||
    ///   || ++++- Volume/Envelope divider period (V)
    ///   |+------ Constant volume flag (0: use envelope, 1: use V)
    ///   +------- Length counter halt / Envelope loop flag
    /// ```
    pub fn write_register(&mut self, value: u8) {
        self.loop_flag = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.volume = value & 0x0F;
    }

    /// Returns the loop flag (also used as length counter halt)
    #[must_use]
    pub const fn loop_flag(self) -> bool {
        self.loop_flag
    }

    /// Starts the envelope (called when length counter is loaded)
    pub fn start(&mut self) {
        self.start_flag = true;
    }

    /// Clocks the envelope generator (called on quarter frame)
    ///
    /// # Envelope Operation
    ///
    /// 1. If start flag is set:
    ///    - Clear start flag
    ///    - Reset decay level to 15
    ///    - Reset divider to V
    /// 2. Otherwise:
    ///    - If divider is 0:
    ///      - Reload divider to V
    ///      - If decay level is 0:
    ///        - If loop flag: reset decay to 15
    ///      - Otherwise: decrement decay level
    ///    - Otherwise: decrement divider
    pub fn clock(&mut self) {
        if self.start_flag {
            self.start_flag = false;
            self.decay_level = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;

            if self.decay_level == 0 {
                if self.loop_flag {
                    self.decay_level = 15;
                }
            } else {
                self.decay_level -= 1;
            }
        } else {
            self.divider -= 1;
        }
    }

    /// Returns the current envelope output (0-15)
    ///
    /// Returns either the constant volume or the decay level.
    #[must_use]
    pub const fn output(self) -> u8 {
        if self.constant_volume {
            self.volume
        } else {
            self.decay_level
        }
    }

    /// Returns whether constant volume mode is enabled
    #[must_use]
    pub const fn is_constant_volume(self) -> bool {
        self.constant_volume
    }

    /// Returns whether the start flag is set
    #[must_use]
    pub const fn is_start_flag_set(self) -> bool {
        self.start_flag
    }
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_constant_volume() {
        let mut env = Envelope::new();

        // Set constant volume mode, volume = 15
        env.write_register(0x1F);
        assert!(env.constant_volume);
        assert_eq!(env.volume, 15);
        assert_eq!(env.output(), 15);

        // Clock envelope (should not change in constant volume mode)
        env.clock();
        assert_eq!(env.output(), 15);

        env.clock();
        assert_eq!(env.output(), 15);
    }

    #[test]
    fn test_envelope_decay() {
        let mut env = Envelope::new();

        // Set envelope mode, V = 1 (fast decay)
        env.write_register(0x01);
        assert!(!env.constant_volume);
        assert_eq!(env.volume, 1);

        // Start envelope
        env.start();
        assert!(env.start_flag);

        // First clock after start sets decay to 15
        env.clock();
        assert!(!env.start_flag);
        assert_eq!(env.decay_level, 15);
        assert_eq!(env.divider, 1);
        assert_eq!(env.output(), 15);

        // Next clock decrements divider
        env.clock();
        assert_eq!(env.divider, 0);
        assert_eq!(env.decay_level, 15);

        // Divider reached 0, reload and decrement decay
        env.clock();
        assert_eq!(env.divider, 1);
        assert_eq!(env.decay_level, 14);
        assert_eq!(env.output(), 14);
    }

    #[test]
    fn test_envelope_decay_to_zero() {
        let mut env = Envelope::new();

        // Set envelope mode, V = 0 (fastest decay, clocks every time)
        env.write_register(0x00);
        env.start();

        // First clock: start -> decay = 15
        env.clock();
        assert_eq!(env.output(), 15);

        // Decay from 15 to 0
        for expected in (0..15).rev() {
            env.clock();
            assert_eq!(env.output(), expected);
        }

        // Should stay at 0 (no loop)
        env.clock();
        assert_eq!(env.output(), 0);
        env.clock();
        assert_eq!(env.output(), 0);
    }

    #[test]
    fn test_envelope_loop() {
        let mut env = Envelope::new();

        // Set envelope mode with loop, V = 0
        env.write_register(0x20); // Loop flag set
        assert!(env.loop_flag);
        env.start();

        // First clock: start -> decay = 15
        env.clock();
        assert_eq!(env.output(), 15);

        // Decay from 15 to 0
        for _ in 0..15 {
            env.clock();
        }
        assert_eq!(env.output(), 0);

        // Next clock should loop back to 15
        env.clock();
        assert_eq!(env.output(), 15);
    }

    #[test]
    fn test_envelope_slow_decay() {
        let mut env = Envelope::new();

        // Set envelope mode, V = 15 (slowest decay)
        env.write_register(0x0F);
        env.start();

        // First clock: start -> decay = 15
        env.clock();
        assert_eq!(env.output(), 15);
        assert_eq!(env.divider, 15);

        // Clock divider 15 times
        for _ in 0..15 {
            env.clock();
            assert_eq!(env.output(), 15); // Decay level unchanged
        }

        // Divider reached 0, should decrement decay
        env.clock();
        assert_eq!(env.output(), 14);
        assert_eq!(env.divider, 15);
    }

    #[test]
    fn test_start_flag_resets_envelope() {
        let mut env = Envelope::new();
        env.write_register(0x00);
        env.start();

        // Decay to some lower value
        // Clock 1: start -> 15
        // Clocks 2-10: 9 decrements -> 15 - 9 = 6
        for _ in 0..10 {
            env.clock();
        }
        assert_eq!(env.output(), 6);

        // Start again
        env.start();
        env.clock();
        assert_eq!(env.output(), 15); // Reset to 15
    }
}
