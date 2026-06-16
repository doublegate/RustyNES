//! Pulse channel (1 of 2). 4-step duty sequencer + envelope + sweep + length.
//!
//! Per `docs/apu-2a03.md` §Behavior and NESdev wiki "APU Pulse" page.
//!
//! Two pulse channels share the same architecture but differ in sweep
//! negation: pulse 1 uses one's-complement (`!t`), pulse 2 uses two's-
//! complement (`-t`).  This produces an audible difference at low periods.

use crate::envelope::Envelope;
use crate::length::LengthCounter;

/// Duty waveforms for the pulse channels.  Each entry is the 8-step output
/// pattern for one of the four duty values.  Index = duty selection
/// (`$4000` bits 6-7).  Step index runs 0..8 with 0 being the "current"
/// position; the LSB is the output bit at the current step.
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25.0%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50.0%
    [1, 0, 0, 1, 1, 1, 1, 1], // 25.0% negated
];

/// Pulse channel state.
#[derive(Debug, Clone, Copy)]
pub struct Pulse {
    /// Duty selection (0..=3).
    pub(crate) duty: u8,
    /// Step index into the duty table (0..=7). Decremented on each timer underflow.
    pub(crate) step: u8,
    /// 11-bit timer reload (from `$4002`/`$4003` low+high writes).
    pub(crate) timer_period: u16,
    /// Internal countdown timer.
    pub(crate) timer: u16,
    /// Envelope generator.
    pub envelope: Envelope,
    /// Length counter.
    pub length: LengthCounter,
    /// Sweep enabled (`$4001` bit 7).
    pub(crate) sweep_enabled: bool,
    /// Sweep divider period (3 bits, +1 -> 1..=8).
    pub(crate) sweep_period: u8,
    /// Sweep negate flag.
    pub(crate) sweep_negate: bool,
    /// Sweep shift count (3 bits).
    pub(crate) sweep_shift: u8,
    /// Sweep reload flag — set by `$4001` write; consumed at next half-frame.
    pub(crate) sweep_reload: bool,
    /// Internal sweep divider.
    pub(crate) sweep_divider: u8,
    /// Pulse 1 vs pulse 2 (controls one's-complement vs two's-complement).
    pub(crate) is_pulse1: bool,
}

impl Pulse {
    /// Construct a new pulse channel. `is_pulse1=true` for the pulse-1 sweep
    /// negation flavor (one's complement).
    #[must_use]
    pub const fn new(is_pulse1: bool) -> Self {
        Self {
            duty: 0,
            step: 0,
            timer_period: 0,
            timer: 0,
            envelope: Envelope {
                start: false,
                loop_flag: false,
                constant: false,
                volume_or_period: 0,
                divider: 0,
                decay: 0,
            },
            length: LengthCounter {
                count: 0,
                halt: false,
                enabled: false,
            },
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_reload: false,
            sweep_divider: 0,
            is_pulse1,
        }
    }

    /// `$4000` / `$4004` write: duty + length-halt + envelope.
    pub fn write_ctrl(&mut self, value: u8) {
        self.duty = (value >> 6) & 0x03;
        let halt = (value & 0x20) != 0;
        self.length.halt = halt;
        self.envelope.loop_flag = halt;
        self.envelope.constant = (value & 0x10) != 0;
        self.envelope.volume_or_period = value & 0x0F;
    }

    /// `$4001` / `$4005` write: sweep config.
    pub fn write_sweep(&mut self, value: u8) {
        self.sweep_enabled = (value & 0x80) != 0;
        self.sweep_period = (value >> 4) & 0x07;
        self.sweep_negate = (value & 0x08) != 0;
        self.sweep_shift = value & 0x07;
        self.sweep_reload = true;
    }

    /// `$4002` / `$4006` write: timer low.
    pub fn write_timer_lo(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0xFF00) | u16::from(value);
    }

    /// `$4003` / `$4007` write: length load + timer high.
    pub fn write_timer_hi(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (u16::from(value & 0x07) << 8);
        self.length.load(value);
        self.step = 0;
        self.envelope.start = true;
    }

    /// One APU clock (half CPU clock).
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            // 8-step duty sequencer, decremented (NESdev wiki).
            self.step = (self.step + 1) & 0x07;
        } else {
            self.timer -= 1;
        }
    }

    /// Half-frame clock: sweep + length.
    pub fn clock_half_frame(&mut self) {
        // Sweep first (because length might mute the channel).
        let target = self.sweep_target();
        if self.sweep_divider == 0 && self.sweep_enabled && self.sweep_shift > 0 && !self.muted() {
            // Apply sweep: write the new period.
            self.timer_period = target;
        }
        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
        self.length.clock();
    }

    /// Quarter-frame clock: envelope.
    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    /// Compute the sweep target period (one's vs two's complement per channel).
    fn sweep_target(&self) -> u16 {
        let shifted = self.timer_period >> self.sweep_shift;
        if self.sweep_negate {
            if self.is_pulse1 {
                self.timer_period.wrapping_sub(shifted).wrapping_sub(1)
            } else {
                self.timer_period.wrapping_sub(shifted)
            }
        } else {
            self.timer_period.wrapping_add(shifted)
        }
    }

    /// Sweep mute: timer < 8 OR target > $7FF.
    #[must_use]
    pub fn muted(&self) -> bool {
        self.timer_period < 8 || self.sweep_target() > 0x7FF
    }

    /// Per-cycle output volume (0..=15).
    #[must_use]
    pub fn output(&self) -> u8 {
        if self.length.count == 0
            || self.muted()
            || DUTY_TABLE[self.duty as usize][self.step as usize] == 0
        {
            0
        } else {
            self.envelope.output()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_ctrl_sets_duty_and_envelope_period() {
        let mut p = Pulse::new(true);
        p.write_ctrl(0b1011_0101); // duty=2, halt=1, const=1, period=5
        assert_eq!(p.duty, 2);
        assert!(p.length.halt);
        assert!(p.envelope.loop_flag);
        assert!(p.envelope.constant);
        assert_eq!(p.envelope.volume_or_period, 5);
    }

    #[test]
    fn timer_underflow_advances_duty_step() {
        let mut p = Pulse::new(true);
        p.timer_period = 1;
        p.timer = 0;
        p.clock_timer();
        assert_eq!(p.step, 1);
    }

    #[test]
    fn pulse1_sweep_negation_is_ones_complement() {
        let mut p = Pulse::new(true);
        p.timer_period = 0x100;
        p.sweep_negate = true;
        p.sweep_shift = 1;
        // shifted = 0x80; 0x100 - 0x80 - 1 = 0x7F.
        assert_eq!(p.sweep_target(), 0x7F);
    }

    #[test]
    fn pulse2_sweep_negation_is_twos_complement() {
        let mut p = Pulse::new(false);
        p.timer_period = 0x100;
        p.sweep_negate = true;
        p.sweep_shift = 1;
        // 0x100 - 0x80 = 0x80.
        assert_eq!(p.sweep_target(), 0x80);
    }

    #[test]
    fn sweep_mutes_when_period_too_low() {
        let mut p = Pulse::new(true);
        p.timer_period = 7;
        assert!(p.muted());
        p.timer_period = 8;
        assert!(!p.muted());
    }

    #[test]
    fn sweep_mutes_when_target_above_7ff() {
        let mut p = Pulse::new(true);
        p.timer_period = 0x780;
        p.sweep_negate = false;
        p.sweep_shift = 1; // target = 0x780 + 0x3C0 = 0xB40 > $7FF
        assert!(p.muted());
    }

    #[test]
    fn output_zero_when_length_zero() {
        let mut p = Pulse::new(true);
        p.length.count = 0;
        p.envelope.constant = true;
        p.envelope.volume_or_period = 15;
        assert_eq!(p.output(), 0);
    }

    #[test]
    fn timer_hi_write_resets_duty_phase_not_divider() {
        // NESdev "APU Pulse": writing $4003/$4007 resets the duty sequencer
        // phase to step 0 but does NOT reset the timer divider.
        let mut p = Pulse::new(true);
        p.step = 5;
        p.timer = 42;
        p.write_timer_hi(0x03);
        assert_eq!(p.step, 0, "duty sequencer phase must reset to 0");
        assert_eq!(p.timer, 42, "timer divider must be preserved");
        assert!(p.envelope.start, "envelope restart flag must be set");
    }

    #[test]
    fn length_load_only_when_enabled() {
        let mut p = Pulse::new(true);
        p.length.enabled = false;
        p.write_timer_hi(0x08);
        assert_eq!(p.length.count, 0);
        p.length.enabled = true;
        p.write_timer_hi(0x08);
        assert_ne!(p.length.count, 0);
    }
}
