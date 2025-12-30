//! APU Pulse (Square Wave) Channel.
//!
//! The NES has two pulse channels that generate square waves with variable
//! duty cycles (12.5%, 25%, 50%, 75%). Each pulse channel has:
//! - An envelope generator for volume control
//! - A sweep unit for pitch bending
//! - A length counter for automatic silencing
//! - A timer for frequency control

use crate::{
    envelope::Envelope,
    length_counter::LengthCounter,
    sweep::{PulseChannel, Sweep},
    timer::Timer,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Duty cycle waveforms.
/// Each entry is an 8-bit pattern where 1 = high, 0 = low.
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 75% (25% inverted)
];

/// Pulse channel.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(dead_code)] // channel field reserved for debugging
pub struct Pulse {
    /// Channel identifier.
    channel: PulseChannel,
    /// Envelope generator.
    envelope: Envelope,
    /// Sweep unit.
    sweep: Sweep,
    /// Length counter.
    length_counter: LengthCounter,
    /// Timer.
    timer: Timer,
    /// Duty cycle select (0-3).
    duty: u8,
    /// Current sequencer position (0-7).
    sequencer: u8,
}

impl Pulse {
    /// Create a new pulse channel.
    #[must_use]
    pub fn new(channel: PulseChannel) -> Self {
        Self {
            channel,
            envelope: Envelope::new(),
            sweep: Sweep::new(channel),
            length_counter: LengthCounter::new(),
            timer: Timer::new(),
            duty: 0,
            sequencer: 0,
        }
    }

    /// Write to register $4000/$4004 (duty, envelope).
    pub fn write_ctrl(&mut self, value: u8) {
        self.duty = (value >> 6) & 0x03;
        self.envelope.write(value);
        self.length_counter.set_halt(self.envelope.loop_flag());
    }

    /// Write to register $4001/$4005 (sweep).
    pub fn write_sweep(&mut self, value: u8) {
        self.sweep.write(value);
    }

    /// Write to register $4002/$4006 (timer low).
    pub fn write_timer_lo(&mut self, value: u8) {
        self.timer.set_period_lo(value);
    }

    /// Write to register $4003/$4007 (length counter, timer high).
    pub fn write_timer_hi(&mut self, value: u8) {
        self.timer.set_period_hi(value);
        self.length_counter.load(value >> 3);
        self.envelope.start();
        self.sequencer = 0;
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
        if self.timer.clock() {
            self.sequencer = (self.sequencer + 1) & 0x07;
        }
    }

    /// Clock the envelope. Should be called on quarter frames.
    pub fn clock_envelope(&mut self) {
        self.envelope.clock();
    }

    /// Clock the length counter. Should be called on half frames.
    pub fn clock_length(&mut self) {
        self.length_counter.clock();
    }

    /// Clock the sweep unit. Should be called on half frames.
    pub fn clock_sweep(&mut self) {
        if let Some(new_period) = self.sweep.clock(self.timer.period()) {
            self.timer.set_period(new_period);
        }
    }

    /// Get the current output value (0-15).
    #[must_use]
    pub fn output(&self) -> u8 {
        // Channel is silenced if:
        // - Length counter is 0
        // - Sweep unit is muting
        // - Current duty output is 0
        if !self.length_counter.active() {
            return 0;
        }

        if self.sweep.muted(self.timer.period()) {
            return 0;
        }

        if DUTY_TABLE[self.duty as usize][self.sequencer as usize] == 0 {
            return 0;
        }

        self.envelope.output()
    }

    /// Get the length counter value.
    #[must_use]
    pub fn length_counter_value(&self) -> u8 {
        self.length_counter.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::naive_bytecount)]
    fn test_duty_table() {
        // 12.5% duty: only position 1 is high
        assert_eq!(DUTY_TABLE[0].iter().filter(|&&x| x == 1).count(), 1);
        // 25% duty: positions 1,2 are high
        assert_eq!(DUTY_TABLE[1].iter().filter(|&&x| x == 1).count(), 2);
        // 50% duty: positions 1,2,3,4 are high
        assert_eq!(DUTY_TABLE[2].iter().filter(|&&x| x == 1).count(), 4);
        // 75% duty: 6 positions high
        assert_eq!(DUTY_TABLE[3].iter().filter(|&&x| x == 1).count(), 6);
    }

    #[test]
    fn test_pulse_output() {
        let mut pulse = Pulse::new(PulseChannel::One);
        pulse.set_enabled(true);
        pulse.write_ctrl(0x3F); // Duty 0, constant volume 15
        pulse.write_timer_lo(0x10); // Period >= 8 to avoid sweep muting
        pulse.write_timer_hi(0xF8); // Load length counter

        // At sequencer position 1, duty 0 should output
        pulse.sequencer = 1;
        assert_eq!(pulse.output(), 15);

        // At sequencer position 0, duty 0 should be silent
        pulse.sequencer = 0;
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn test_pulse_muted_when_disabled() {
        let mut pulse = Pulse::new(PulseChannel::One);
        pulse.set_enabled(false);
        pulse.write_ctrl(0x3F);
        pulse.write_timer_lo(0x00);
        pulse.write_timer_hi(0xF8);

        // Should be silent when disabled
        pulse.sequencer = 1;
        assert_eq!(pulse.output(), 0);
    }

    #[test]
    fn test_pulse_sweep_mute() {
        let mut pulse = Pulse::new(PulseChannel::One);
        pulse.set_enabled(true);
        pulse.write_ctrl(0x3F);
        pulse.write_timer_lo(0x01); // Very low period
        pulse.write_timer_hi(0xF8);
        pulse.sequencer = 1;

        // Period < 8 should mute
        assert_eq!(pulse.output(), 0);
    }
}
