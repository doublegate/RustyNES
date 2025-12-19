//! Pulse Channel Module
//!
//! Implements the NES 2A03 pulse wave channels (also known as square wave channels).
//! The NES has two pulse channels that produce square wave sounds with four duty cycles.
//!
//! # Channel Differences
//!
//! Pulse 1 and Pulse 2 are nearly identical, with one key difference:
//! - **Pulse 1**: Uses one's complement for sweep negation (subtract change + 1)
//! - **Pulse 2**: Uses two's complement for sweep negation (subtract change only)

use crate::envelope::Envelope;
use crate::length_counter::LengthCounter;
use crate::sweep::Sweep;

/// Duty cycle waveforms (0 = low, 1 = high)
///
/// Each duty cycle is an 8-step sequence that repeats to form the square wave.
/// The patterns are:
/// - **12.5%**: One high bit (short, sharp sounds)
/// - **25%**: Two high bits (thin, hollow sounds for lead melody)
/// - **50%**: Four high bits (square, full sounds for harmony)
/// - **75%**: Six high bits (inverted 25%, rarely used)
const DUTY_SEQUENCES: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5% duty cycle
    [0, 1, 1, 0, 0, 0, 0, 0], // 25% duty cycle
    [0, 1, 1, 1, 1, 0, 0, 0], // 50% duty cycle
    [1, 0, 0, 1, 1, 1, 1, 1], // 75% duty cycle (inverted 25%)
];

/// Pulse channel for square wave audio generation
///
/// Integrates envelope, length counter, sweep, and duty cycle sequencer.
/// The channel produces a square wave at a frequency determined by the timer.
///
/// # Register Map
///
/// | Register | Bits | Description |
/// |----------|------|-------------|
/// | 0 | `DDLC VVVV` | Duty, halt, envelope |
/// | 1 | `EPPP NSSS` | Sweep enable, period, negate, shift |
/// | 2 | `TTTT TTTT` | Timer low 8 bits |
/// | 3 | `LLLL LTTT` | Length load, timer high 3 bits |
pub struct PulseChannel {
    // Components
    envelope: Envelope,
    length_counter: LengthCounter,
    sweep: Sweep,

    // Duty cycle
    duty: u8,          // 0-3 (12.5%, 25%, 50%, 75%)
    duty_position: u8, // 0-7 position in duty cycle

    // Timer (11-bit period)
    timer: u16,         // Target period
    timer_counter: u16, // Current countdown value

    // Channel state
    enabled: bool,
    channel: u8, // 0 = Pulse 1, 1 = Pulse 2
}

impl PulseChannel {
    /// Creates a new pulse channel
    ///
    /// # Arguments
    ///
    /// * `channel` - Channel number (0 = Pulse 1, 1 = Pulse 2)
    #[must_use]
    pub const fn new(channel: u8) -> Self {
        Self {
            envelope: Envelope::new(),
            length_counter: LengthCounter::new(),
            sweep: Sweep::new(channel),
            duty: 0,
            duty_position: 0,
            timer: 0,
            timer_counter: 0,
            enabled: false,
            channel,
        }
    }

    /// Returns the current duty cycle output (0 or 1)
    fn duty_output(&self) -> u8 {
        DUTY_SEQUENCES[self.duty as usize][self.duty_position as usize]
    }

    /// Advances the duty cycle sequencer by one step
    fn clock_duty(&mut self) {
        self.duty_position = (self.duty_position + 1) & 0x07;
    }

    /// Clocks the timer (called every APU cycle)
    ///
    /// When the timer reaches 0, it reloads and advances the duty cycle.
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer;
            self.clock_duty();
        } else {
            self.timer_counter -= 1;
        }
    }

    /// Clocks the envelope (called on quarter frame)
    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    /// Clocks the length counter (called on half frame)
    pub fn clock_length_counter(&mut self) {
        self.length_counter.clock();
    }

    /// Clocks the sweep unit (called on half frame)
    pub fn clock_sweep(&mut self) {
        self.sweep.clock(&mut self.timer);
    }

    /// Writes to a pulse channel register
    ///
    /// # Arguments
    ///
    /// * `addr` - Register offset (0-3)
    /// * `value` - Value to write
    ///
    /// # Register Format
    ///
    /// ## Register 0: `DDLC VVVV`
    /// - `DD`: Duty cycle (0-3)
    /// - `L`: Length counter halt / envelope loop
    /// - `C`: Constant volume flag
    /// - `VVVV`: Volume / envelope divider period
    ///
    /// ## Register 1: `EPPP NSSS`
    /// - `E`: Sweep enabled
    /// - `PPP`: Sweep divider period
    /// - `N`: Negate flag
    /// - `SSS`: Shift count
    ///
    /// ## Register 2: `TTTT TTTT`
    /// - Timer period low 8 bits
    ///
    /// ## Register 3: `LLLL LTTT`
    /// - `LLLLL`: Length counter load index
    /// - `TTT`: Timer period high 3 bits
    pub fn write_register(&mut self, addr: u8, value: u8) {
        match addr {
            0 => {
                // $4000/$4004: DDLC VVVV
                self.duty = (value >> 6) & 0x03;
                let halt = (value & 0x20) != 0;
                self.length_counter.set_halt(halt);
                self.envelope.write_register(value);
            }
            1 => {
                // $4001/$4005: EPPP NSSS
                self.sweep.write_register(value);
            }
            2 => {
                // $4002/$4006: TTTT TTTT
                self.timer = (self.timer & 0xFF00) | u16::from(value);
            }
            3 => {
                // $4003/$4007: LLLL LTTT
                self.timer = (self.timer & 0x00FF) | (u16::from(value & 0x07) << 8);

                // Load length counter if channel is enabled
                // Note: According to hardware behavior, length counter is only
                // loaded if the channel is currently enabled via $4015
                if self.enabled {
                    let length_index = (value >> 3) & 0x1F;
                    self.length_counter.load(length_index);
                }

                // Restart envelope and reset duty position
                self.envelope.start();
                self.duty_position = 0;
            }
            _ => {}
        }
    }

    /// Sets the channel enabled state
    ///
    /// Called when the status register ($4015) is written.
    /// Disabling the channel immediately clears the length counter.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.length_counter.set_enabled(enabled);
    }

    /// Returns whether the channel is silenced
    ///
    /// A pulse channel is silenced if:
    /// - Channel is disabled via $4015
    /// - Length counter has reached 0
    /// - Sweep unit is muting (timer < 8 or target period > $7FF)
    #[must_use]
    pub fn is_silenced(&self) -> bool {
        !self.enabled || !self.length_counter.is_active() || self.sweep.is_muted(self.timer)
    }

    /// Returns the current channel output (0-15)
    ///
    /// The output is determined by:
    /// 1. Check if channel is silenced (returns 0 if true)
    /// 2. Check duty cycle output (returns 0 if duty is low)
    /// 3. Return envelope volume (0-15)
    #[must_use]
    pub fn output(&self) -> u8 {
        if self.is_silenced() {
            return 0;
        }

        if self.duty_output() == 0 {
            return 0;
        }

        self.envelope.output()
    }

    /// Returns the channel's output frequency in Hz
    ///
    /// # Formula
    ///
    /// ```text
    /// frequency = CPU_CLOCK / (16 * (timer + 1))
    /// ```
    ///
    /// For NTSC NES: `frequency = 1789773 / (16 * (timer + 1))`
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn frequency_hz(&self) -> f32 {
        const CPU_CLOCK: f32 = 1_789_773.0;
        CPU_CLOCK / (16.0 * (f32::from(self.timer) + 1.0))
    }

    /// Returns whether the length counter is active (greater than 0)
    #[must_use]
    pub fn length_counter_active(&self) -> bool {
        self.length_counter.is_active()
    }
}

impl Default for PulseChannel {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pulse_channel_creation() {
        let pulse1 = PulseChannel::new(0);
        let pulse2 = PulseChannel::new(1);

        assert_eq!(pulse1.channel, 0);
        assert_eq!(pulse2.channel, 1);
        assert!(!pulse1.enabled);
        assert!(!pulse2.enabled);
    }

    #[test]
    fn test_duty_cycles() {
        let mut pulse = PulseChannel::new(0);

        // Test all four duty cycles
        for duty in 0..4 {
            pulse.duty = duty;
            for position in 0..8 {
                pulse.duty_position = position;
                let expected = DUTY_SEQUENCES[duty as usize][position as usize];
                assert_eq!(pulse.duty_output(), expected);
            }
        }
    }

    #[test]
    fn test_duty_cycle_advancement() {
        let mut pulse = PulseChannel::new(0);

        for i in 0..16 {
            assert_eq!(pulse.duty_position, (i % 8) as u8);
            pulse.clock_duty();
        }
    }

    #[test]
    fn test_register_write_duty() {
        let mut pulse = PulseChannel::new(0);

        // Write duty cycle 0 (12.5%)
        pulse.write_register(0, 0x00);
        assert_eq!(pulse.duty, 0);

        // Write duty cycle 2 (50%)
        pulse.write_register(0, 0x80);
        assert_eq!(pulse.duty, 2);

        // Write duty cycle 3 (75%)
        pulse.write_register(0, 0xC0);
        assert_eq!(pulse.duty, 3);
    }

    #[test]
    fn test_register_write_envelope() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);

        // Write constant volume mode, volume = 15
        pulse.write_register(0, 0x1F);
        assert_eq!(pulse.envelope.output(), 15);

        // Write constant volume mode, volume = 8
        pulse.write_register(0, 0x18);
        assert_eq!(pulse.envelope.output(), 8);
    }

    #[test]
    fn test_register_write_timer() {
        let mut pulse = PulseChannel::new(0);

        // Write timer low byte
        pulse.write_register(2, 0x54);
        assert_eq!(pulse.timer & 0xFF, 0x54);

        // Write timer high 3 bits
        pulse.write_register(3, 0x07);
        assert_eq!(pulse.timer, 0x754);
    }

    #[test]
    fn test_register_write_length_counter() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);

        // Write length counter load index 0 (value = 10)
        pulse.write_register(3, 0x00);
        assert!(pulse.length_counter_active());

        // Disable channel should clear length counter
        pulse.set_enabled(false);
        assert!(!pulse.length_counter_active());
    }

    #[test]
    fn test_register_write_resets_duty() {
        let mut pulse = PulseChannel::new(0);

        pulse.duty_position = 5;
        pulse.write_register(3, 0x00); // Write to register 3 resets duty position
        assert_eq!(pulse.duty_position, 0);
    }

    #[test]
    fn test_timer_countdown() {
        let mut pulse = PulseChannel::new(0);
        pulse.timer = 10;
        pulse.timer_counter = 10;

        let initial_pos = pulse.duty_position;

        // Clock timer 11 times to reach 0
        for _ in 0..11 {
            pulse.clock_timer();
        }

        // Duty position should advance once
        assert_eq!(pulse.duty_position, (initial_pos + 1) & 0x07);

        // Timer should reload
        assert_eq!(pulse.timer_counter, 10);
    }

    #[test]
    fn test_output_when_disabled() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true); // Enable first
        pulse.length_counter.load(0); // Load non-zero length
        pulse.duty_position = 1; // Set to non-zero duty output
        pulse.envelope.write_register(0x1F); // Constant volume 15
        pulse.timer = 100; // Set valid timer (> 8 to avoid sweep muting)

        // Channel enabled with valid state -> output non-zero
        assert!(pulse.output() > 0);

        // Disable channel -> output 0
        pulse.set_enabled(false);
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn test_output_with_duty_cycle() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);
        pulse.timer = 100;
        pulse.length_counter.load(0); // Load non-zero length
        pulse.envelope.write_register(0x1F); // Constant volume 15
        pulse.duty = 0; // 12.5% duty cycle

        // Position 0: duty = 0 -> output 0
        pulse.duty_position = 0;
        assert_eq!(pulse.output(), 0);

        // Position 1: duty = 1 -> output 15
        pulse.duty_position = 1;
        assert_eq!(pulse.output(), 15);

        // Position 2-7: duty = 0 -> output 0
        for i in 2..8 {
            pulse.duty_position = i;
            assert_eq!(pulse.output(), 0);
        }
    }

    #[test]
    fn test_sweep_muting() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);
        pulse.length_counter.load(0); // Load non-zero
        pulse.duty_position = 1; // Non-zero duty output

        // Timer < 8 should silence
        pulse.timer = 7;
        assert_eq!(pulse.output(), 0);
        assert!(pulse.is_silenced());

        // Valid timer should produce output
        pulse.timer = 100;
        pulse.envelope.write_register(0x1F); // Constant volume 15
        assert_eq!(pulse.output(), 15);
        assert!(!pulse.is_silenced());
    }

    #[test]
    fn test_length_counter_silencing() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);
        pulse.timer = 100;
        pulse.duty_position = 1;
        pulse.envelope.write_register(0x1F); // Constant volume 15

        // With active length counter
        pulse.length_counter.load(0);
        assert_eq!(pulse.output(), 15);

        // Clock length counter to 0
        for _ in 0..10 {
            pulse.clock_length_counter();
        }
        assert_eq!(pulse.output(), 0);
        assert!(pulse.is_silenced());
    }

    #[test]
    fn test_sweep_one_vs_two_complement() {
        let mut pulse1 = PulseChannel::new(0);
        let mut pulse2 = PulseChannel::new(1);

        pulse1.timer = 200;
        pulse2.timer = 200;

        // Configure sweep: enabled, period=0, negate, shift=1
        pulse1.sweep.write_register(0x89);
        pulse2.sweep.write_register(0x89);

        // Clock sweep (with reload flag, shouldn't update timer)
        pulse1.clock_sweep();
        pulse2.clock_sweep();

        // Timer shouldn't change on first clock (reload flag set)
        assert_eq!(pulse1.timer, 200);
        assert_eq!(pulse2.timer, 200);

        // Clock again to actually update
        pulse1.clock_sweep();
        pulse2.clock_sweep();

        // Pulse 1 uses one's complement: 200 - 100 - 1 = 99
        // Pulse 2 uses two's complement: 200 - 100 = 100
        assert_eq!(pulse1.timer, 99);
        assert_eq!(pulse2.timer, 100);
    }

    #[test]
    fn test_frequency_calculation() {
        let mut pulse = PulseChannel::new(0);

        // A4 (440 Hz) - timer = 253
        pulse.timer = 253;
        let freq = pulse.frequency_hz();
        assert!((freq - 440.0).abs() < 1.0); // Within 1 Hz

        // C4 (261.63 Hz) - timer ≈ 428
        pulse.timer = 428;
        let freq = pulse.frequency_hz();
        assert!((freq - 261.5).abs() < 1.0);
    }

    #[test]
    fn test_envelope_integration() {
        let mut pulse = PulseChannel::new(0);
        pulse.set_enabled(true);
        pulse.timer = 100;
        pulse.duty_position = 1;
        pulse.length_counter.load(0);

        // Envelope mode, V = 0 (fast decay)
        pulse.write_register(0, 0x00);
        pulse.envelope.start();

        // First clock after start sets decay to 15
        pulse.clock_envelope();
        assert_eq!(pulse.output(), 15);

        // Subsequent clocks decrement decay
        pulse.clock_envelope();
        assert_eq!(pulse.output(), 14);

        pulse.clock_envelope();
        assert_eq!(pulse.output(), 13);
    }

    #[test]
    fn test_full_channel_integration() {
        let mut pulse = PulseChannel::new(0);

        // Configure channel: 50% duty, constant volume 12, timer = 100
        pulse.set_enabled(true);
        pulse.write_register(0, 0xBC); // Duty=2, constant volume, volume=12
        pulse.write_register(2, 0x64); // Timer low = 100
        pulse.write_register(3, 0x08); // Length index 1 (254), timer high = 0

        assert_eq!(pulse.duty, 2);
        assert_eq!(pulse.timer, 0x064);
        assert!(pulse.length_counter_active());

        // Clock duty cycle through timer
        for _ in 0..101 {
            pulse.clock_timer();
        }

        // Duty position should have advanced
        assert_eq!(pulse.duty_position, 1);

        // Output should reflect duty cycle and volume
        pulse.duty_position = 0; // Low part of 50% duty
        assert_eq!(pulse.output(), 0);

        pulse.duty_position = 1; // High part of 50% duty
        assert_eq!(pulse.output(), 12);
    }
}
