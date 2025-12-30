//! APU Envelope Generator.
//!
//! The envelope generator is used by the pulse and noise channels to control
//! volume. It can operate in constant volume mode or decaying envelope mode.
//!
//! In envelope mode, the volume starts at 15 and decreases to 0, optionally
//! looping back to 15.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Envelope generator unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Envelope {
    /// Start flag - when set, the envelope restarts on next clock.
    start: bool,
    /// Divider counter.
    divider: u8,
    /// Decay level counter (0-15).
    decay: u8,
    /// Constant volume / envelope period (from register).
    volume: u8,
    /// Loop flag (also used as length counter halt).
    loop_flag: bool,
    /// Constant volume mode flag.
    constant: bool,
}

impl Envelope {
    /// Create a new envelope generator.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            start: false,
            divider: 0,
            decay: 0,
            volume: 0,
            loop_flag: false,
            constant: false,
        }
    }

    /// Write the envelope control register.
    /// Bits: --LC VVVV
    /// - L: Loop/length counter halt flag
    /// - C: Constant volume flag
    /// - V: Volume/envelope period
    pub fn write(&mut self, value: u8) {
        self.loop_flag = value & 0x20 != 0;
        self.constant = value & 0x10 != 0;
        self.volume = value & 0x0F;
    }

    /// Start the envelope (called when length counter is loaded).
    pub fn start(&mut self) {
        self.start = true;
    }

    /// Clock the envelope generator.
    /// Should be called on quarter frames.
    pub fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.loop_flag {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    /// Get the current output volume (0-15).
    #[must_use]
    #[inline]
    pub const fn output(&self) -> u8 {
        if self.constant {
            self.volume
        } else {
            self.decay
        }
    }

    /// Check if the loop flag is set (also length counter halt).
    #[must_use]
    #[inline]
    pub const fn loop_flag(&self) -> bool {
        self.loop_flag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_volume() {
        let mut env = Envelope::new();
        env.write(0x1F); // Constant volume = 15
        assert_eq!(env.output(), 15);

        env.clock();
        assert_eq!(env.output(), 15); // Should stay constant
    }

    #[test]
    fn test_envelope_decay() {
        let mut env = Envelope::new();
        env.write(0x00); // Envelope mode, period 0
        env.start();

        env.clock(); // First clock: decay = 15, divider = 0
        assert_eq!(env.output(), 15);

        env.clock(); // Second clock: divider wraps, decay = 14
        assert_eq!(env.output(), 14);
    }

    #[test]
    fn test_envelope_loop() {
        let mut env = Envelope::new();
        env.write(0x20); // Envelope with loop, period 0
        env.start();

        // Clock through all decay levels
        for _ in 0..16 {
            env.clock();
        }
        // Should loop back to 15
        env.clock();
        assert_eq!(env.output(), 15);
    }

    #[test]
    fn test_envelope_no_loop() {
        let mut env = Envelope::new();
        env.write(0x00); // Envelope without loop, period 0
        env.start();

        // Clock through all decay levels and beyond
        for _ in 0..20 {
            env.clock();
        }
        // Should stay at 0
        assert_eq!(env.output(), 0);
    }
}
