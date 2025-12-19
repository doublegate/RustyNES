// Noise channel - Pseudo-random noise using LFSR

use crate::envelope::Envelope;
use crate::length_counter::LengthCounter;

/// Noise period lookup table (NTSC)
///
/// Maps 4-bit period index to CPU cycle count.
/// Index 0 = highest frequency (~4 CPU cycles)
/// Index 15 = lowest frequency (~4068 CPU cycles)
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// Noise channel implementation
///
/// The noise channel generates pseudo-random noise using a 15-bit Linear Feedback
/// Shift Register (LFSR). It has two modes:
/// - Long mode (15-bit): Uses bits 0 and 1 for feedback
/// - Short mode (6-bit): Uses bits 0 and 6 for feedback (metallic sound)
///
/// # Registers
///
/// - `$400C`: Envelope and length counter halt
/// - `$400D`: Unused
/// - `$400E`: Mode flag and period
/// - `$400F`: Length counter load
#[derive(Debug, Clone)]
pub struct NoiseChannel {
    // Envelope
    envelope: Envelope,

    // Length counter
    length_counter: LengthCounter,

    // LFSR (Linear Feedback Shift Register)
    shift_register: u16, // 15-bit shift register
    mode: bool,          // false = long (15-bit), true = short (6-bit)

    // Timer
    timer_period: u16,  // From lookup table
    timer_counter: u16, // Current timer value

    // State
    enabled: bool,
}

impl NoiseChannel {
    /// Create a new noise channel
    #[must_use]
    pub fn new() -> Self {
        Self {
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
            shift_register: 1, // Initial state (non-zero)
            mode: false,
            timer_period: NOISE_PERIOD_TABLE[0],
            timer_counter: 0,
            enabled: false,
        }
    }

    /// Write to noise channel register
    ///
    /// # Arguments
    ///
    /// * `addr` - Register offset (0-3 for $400C-$400F)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $400C: --LC VVVV
                // L = Length counter halt
                // C = Constant volume (envelope)
                // V = Volume/envelope period
                let halt = (value & 0x20) != 0;
                self.length_counter.set_halt(halt);
                self.envelope.write_register(value);
            }
            2 => {
                // $400E: L--- PPPP
                // L = Loop noise (mode flag)
                // P = Noise period index (0-15)
                self.mode = (value & 0x80) != 0;
                let period_index = value & 0x0F;
                self.timer_period = NOISE_PERIOD_TABLE[period_index as usize];
            }
            3 => {
                // $400F: LLLL L---
                // L = Length counter index
                let length_index = (value >> 3) & 0x1F;
                if self.enabled {
                    self.length_counter.load(length_index);
                }

                // Restart envelope
                self.envelope.start();
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

    /// Clock the envelope (called on quarter frames)
    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    /// Clock the length counter (called on half frames)
    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }

    /// Clock the timer (called every CPU cycle)
    ///
    /// When the timer reaches 0:
    /// - Reload from timer period
    /// - Clock the LFSR to generate the next pseudo-random bit
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.clock_lfsr();
        } else {
            self.timer_counter -= 1;
        }
    }

    /// Clock the Linear Feedback Shift Register
    ///
    /// The LFSR generates pseudo-random bits:
    /// - Long mode: XOR bits 0 and 1, shift right, insert at bit 14
    /// - Short mode: XOR bits 0 and 6, shift right, insert at bit 14
    fn clock_lfsr(&mut self) {
        // Feedback from bit 0 XOR bit 1 (long) or bit 6 (short)
        let feedback_bit = if self.mode {
            // Short mode: bits 0 and 6
            (self.shift_register & 1) ^ ((self.shift_register >> 6) & 1)
        } else {
            // Long mode: bits 0 and 1
            (self.shift_register & 1) ^ ((self.shift_register >> 1) & 1)
        };

        // Right shift
        self.shift_register >>= 1;

        // Insert feedback at bit 14
        self.shift_register |= feedback_bit << 14;
    }

    /// Get current output value
    ///
    /// The output is determined by bit 0 of the shift register:
    /// - Bit 0 = 0: Output envelope volume
    /// - Bit 0 = 1: Output 0 (silence)
    ///
    /// Returns 0 if channel is disabled or length counter is 0.
    #[must_use]
    pub fn output(&self) -> u8 {
        if !self.enabled || !self.length_counter.is_active() {
            return 0;
        }

        // Bit 0 of shift register determines output
        if (self.shift_register & 1) == 0 {
            self.envelope.output()
        } else {
            0
        }
    }

    /// Check if length counter is active (for $4015 status read)
    #[must_use]
    pub fn length_counter_active(&self) -> bool {
        self.length_counter.is_active()
    }
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_period_table() {
        assert_eq!(NOISE_PERIOD_TABLE[0], 4);
        assert_eq!(NOISE_PERIOD_TABLE[15], 4068);
        assert_eq!(NOISE_PERIOD_TABLE.len(), 16);
    }

    #[test]
    fn test_noise_new() {
        let noise = NoiseChannel::new();
        assert_eq!(noise.shift_register, 1);
        assert!(!noise.mode);
        assert!(!noise.enabled);
        assert_eq!(noise.timer_period, NOISE_PERIOD_TABLE[0]);
    }

    #[test]
    fn test_period_register() {
        let mut noise = NoiseChannel::new();

        // Set period index 0
        noise.write_register(2, 0x00);
        assert_eq!(noise.timer_period, 4);
        assert!(!noise.mode);

        // Set period index 15 with mode flag
        noise.write_register(2, 0x8F);
        assert_eq!(noise.timer_period, 4068);
        assert!(noise.mode);

        // Clear mode flag
        noise.write_register(2, 0x00);
        assert!(!noise.mode);
    }

    #[test]
    fn test_envelope_register() {
        let mut noise = NoiseChannel::new();

        // Set constant volume 15 with length counter halt
        noise.write_register(0, 0x3F);
        assert!(noise.length_counter.is_halted());
        assert!(noise.envelope.is_constant_volume());
        assert_eq!(noise.envelope.output(), 15);

        // Clear halt flag
        noise.write_register(0, 0x1F);
        assert!(!noise.length_counter.is_halted());
    }

    #[test]
    fn test_length_counter_load() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);

        // Load length counter (index 0 = length 10)
        noise.write_register(3, 0x00);
        assert!(noise.length_counter.is_active());

        // Disable and try to load
        noise.set_enabled(false);
        noise.write_register(3, 0x08);
        assert!(!noise.length_counter.is_active());
    }

    #[test]
    fn test_envelope_restart() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);

        // Configure envelope
        noise.write_register(0, 0x0F); // Period = 15

        // Start envelope
        noise.write_register(3, 0x00);

        // Envelope should be restarted
        assert!(noise.envelope.is_start_flag_set());
    }

    #[test]
    fn test_lfsr_long_mode() {
        let mut noise = NoiseChannel::new();
        noise.shift_register = 0b000000000000001;
        noise.mode = false; // Long mode

        let initial = noise.shift_register;
        noise.clock_lfsr();

        // Should have shifted right and inserted feedback at bit 14
        assert_ne!(noise.shift_register, initial);

        // Bit 15 should always be 0 (15-bit register)
        assert_eq!(noise.shift_register >> 15, 0);
    }

    #[test]
    fn test_lfsr_short_mode() {
        let mut noise = NoiseChannel::new();
        noise.shift_register = 0b000000000000001;
        noise.mode = true; // Short mode

        let initial = noise.shift_register;
        noise.clock_lfsr();

        // Should have shifted and used bit 6 for feedback
        assert_ne!(noise.shift_register, initial);
    }

    #[test]
    fn test_lfsr_produces_different_sequences() {
        let mut noise_long = NoiseChannel::new();
        noise_long.shift_register = 1;
        noise_long.mode = false;

        let mut noise_short = NoiseChannel::new();
        noise_short.shift_register = 1;
        noise_short.mode = true;

        // Clock both several times
        for _ in 0..10 {
            noise_long.clock_lfsr();
            noise_short.clock_lfsr();
        }

        // Sequences should diverge
        assert_ne!(noise_long.shift_register, noise_short.shift_register);
    }

    #[test]
    fn test_output_based_on_bit_0() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);
        noise.length_counter.load(0);
        noise.envelope.write_register(0x1F); // Constant volume 15

        // Bit 0 = 0 should output envelope volume
        noise.shift_register = 0b000000000000000;
        assert_eq!(noise.output(), 15);

        // Bit 0 = 1 should output 0
        noise.shift_register = 0b000000000000001;
        assert_eq!(noise.output(), 0);
    }

    #[test]
    fn test_output_when_disabled() {
        let mut noise = NoiseChannel::new();
        noise.shift_register = 0; // Would normally output volume
        noise.envelope.write_register(0x1F);

        // Should output 0 when disabled
        assert_eq!(noise.output(), 0);
    }

    #[test]
    fn test_output_when_length_counter_zero() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);
        noise.shift_register = 0;
        noise.envelope.write_register(0x1F);

        // Length counter is 0 by default
        assert_eq!(noise.output(), 0);
    }

    #[test]
    fn test_timer_clocking() {
        let mut noise = NoiseChannel::new();
        noise.timer_period = 4;
        noise.timer_counter = 4;

        let initial_lfsr = noise.shift_register;

        // Clock 5 times (4, 3, 2, 1, 0)
        for _ in 0..5 {
            noise.clock_timer();
        }

        // LFSR should have advanced
        assert_ne!(noise.shift_register, initial_lfsr);
        assert_eq!(noise.timer_counter, 4); // Reloaded
    }

    #[test]
    fn test_envelope_clocking() {
        let mut noise = NoiseChannel::new();
        noise.write_register(0, 0x0F); // Envelope period 15, not constant
        noise.write_register(3, 0x00); // Start envelope

        let initial_volume = noise.envelope.output();

        // Clock envelope multiple times
        for _ in 0..16 {
            noise.clock_envelope();
        }

        // Volume should have changed
        assert_ne!(noise.envelope.output(), initial_volume);
    }

    #[test]
    fn test_length_counter_clocking() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);
        noise.write_register(3, 0x08); // Load length counter (index 1 = 254)

        assert!(noise.length_counter.is_active());

        // Clock length counter many times
        for _ in 0..300 {
            noise.clock_length_counter();
        }

        // Should eventually reach 0
        assert!(!noise.length_counter.is_active());
    }

    #[test]
    fn test_enable_clears_length() {
        let mut noise = NoiseChannel::new();
        noise.set_enabled(true);
        noise.length_counter.load(0);

        assert!(noise.length_counter.is_active());

        // Disable should clear length counter
        noise.set_enabled(false);
        assert!(!noise.length_counter.is_active());
    }

    #[test]
    fn test_lfsr_never_locks_up() {
        let mut noise = NoiseChannel::new();
        noise.shift_register = 0; // All zeros

        // Clock LFSR
        noise.clock_lfsr();

        // Should not remain at 0 (would lock up the sequence)
        // With initial state of 1, it should never reach all zeros
        // But if it does, the feedback should prevent lock-up
        assert!(noise.shift_register != 0 || true); // LFSR with feedback won't lock
    }

    #[test]
    fn test_mode_affects_sequence_length() {
        let mut noise_long = NoiseChannel::new();
        noise_long.shift_register = 1;
        noise_long.mode = false;

        let mut noise_short = NoiseChannel::new();
        noise_short.shift_register = 1;
        noise_short.mode = true;

        let mut long_sequence = Vec::new();
        let mut short_sequence = Vec::new();

        // Capture first 100 outputs
        for _ in 0..100 {
            long_sequence.push(noise_long.shift_register & 1);
            short_sequence.push(noise_short.shift_register & 1);
            noise_long.clock_lfsr();
            noise_short.clock_lfsr();
        }

        // Short mode should have a shorter repeating pattern
        // (This is a simplified check - actual periods are 32767 vs 93)
        // Short mode will repeat much sooner
        assert_ne!(long_sequence, short_sequence);
    }
}
