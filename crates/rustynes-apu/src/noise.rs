//! APU Noise Channel.
//!
//! The noise channel generates pseudo-random noise using a 15-bit linear
//! feedback shift register (LFSR). It can operate in two modes:
//! - Normal mode: 32767-step sequence
//! - Short mode: 93-step sequence (more metallic sound)

use crate::{envelope::Envelope, length_counter::LengthCounter};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Noise period lookup table (NTSC).
/// Index is the 4-bit period value from the register.
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// Noise channel.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Noise {
    /// Envelope generator.
    envelope: Envelope,
    /// Length counter.
    length_counter: LengthCounter,
    /// Timer counter.
    timer: u16,
    /// Timer period (from lookup table).
    period: u16,
    /// Shift register (15-bit LFSR).
    shift_register: u16,
    /// Mode flag (short mode when true).
    mode: bool,
}

impl Noise {
    /// Create a new noise channel.
    #[must_use]
    pub fn new() -> Self {
        Self {
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
            timer: 0,
            period: NOISE_PERIOD_TABLE[0],
            shift_register: 1, // Must not be 0
            mode: false,
        }
    }

    /// Write to register $400C (envelope).
    pub fn write_ctrl(&mut self, value: u8) {
        self.envelope.write(value);
        self.length_counter.set_halt(self.envelope.loop_flag());
    }

    /// Write to register $400E (mode, period).
    pub fn write_period(&mut self, value: u8) {
        self.mode = value & 0x80 != 0;
        self.period = NOISE_PERIOD_TABLE[(value & 0x0F) as usize];
    }

    /// Write to register $400F (length counter).
    pub fn write_length(&mut self, value: u8) {
        self.length_counter.load(value >> 3);
        self.envelope.start();
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

    /// Clock the timer. Should be called every APU cycle (CPU/2).
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.clock_shift_register();
        } else {
            self.timer -= 1;
        }
    }

    /// Clock the shift register (LFSR).
    fn clock_shift_register(&mut self) {
        // Feedback bit is XOR of bit 0 and bit 1 (normal) or bit 6 (short mode)
        let feedback_bit = if self.mode { 6 } else { 1 };
        let feedback = (self.shift_register & 1) ^ ((self.shift_register >> feedback_bit) & 1);

        // Shift right and set bit 14 with feedback
        self.shift_register >>= 1;
        self.shift_register |= feedback << 14;
    }

    /// Clock the envelope. Should be called on quarter frames.
    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    /// Clock the length counter. Should be called on half frames.
    pub fn clock_length(&mut self) {
        self.length_counter.clock();
    }

    /// Get the current output value (0-15).
    #[must_use]
    pub fn output(&self) -> u8 {
        // Silenced if length counter is zero
        if !self.length_counter.active() {
            return 0;
        }

        // Output depends on bit 0 of shift register
        if self.shift_register & 1 == 0 {
            self.envelope.output()
        } else {
            0
        }
    }

    /// Get the length counter value.
    #[must_use]
    pub fn length_counter_value(&self) -> u8 {
        self.length_counter.value()
    }
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_period_table() {
        // Verify first and last entries
        assert_eq!(NOISE_PERIOD_TABLE[0], 4);
        assert_eq!(NOISE_PERIOD_TABLE[15], 4068);
    }

    #[test]
    fn test_noise_shift_register_initial() {
        let noise = Noise::new();
        assert_eq!(noise.shift_register, 1);
    }

    #[test]
    fn test_noise_output() {
        let mut noise = Noise::new();
        noise.set_enabled(true);
        noise.write_ctrl(0x3F); // Constant volume 15
        noise.write_length(0xF8); // Load length counter

        // Initial shift register has bit 0 = 1, so output should be 0
        assert_eq!(noise.output(), 0);

        // Clock until bit 0 becomes 0
        for _ in 0..100 {
            noise.clock_timer();
        }
        // Output depends on shift register state
        let expected = if noise.shift_register & 1 == 0 { 15 } else { 0 };
        assert_eq!(noise.output(), expected);
    }

    #[test]
    fn test_noise_muted_when_disabled() {
        let mut noise = Noise::new();
        noise.set_enabled(false);
        noise.write_ctrl(0x3F);
        noise.write_length(0xF8);

        assert_eq!(noise.output(), 0);
    }

    #[test]
    fn test_noise_short_mode() {
        let mut noise = Noise::new();
        noise.write_period(0x80); // Short mode enabled

        // Clock the shift register a few times
        let initial = noise.shift_register;
        for _ in 0..10 {
            noise.clock_shift_register();
        }
        // Shift register should have changed
        assert_ne!(noise.shift_register, initial);
    }

    #[test]
    fn test_noise_normal_mode() {
        let mut noise = Noise::new();
        noise.write_period(0x00); // Normal mode

        let initial = noise.shift_register;
        for _ in 0..10 {
            noise.clock_shift_register();
        }
        assert_ne!(noise.shift_register, initial);
    }
}
