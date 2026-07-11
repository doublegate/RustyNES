//! Triangle channel: 32-step waveform + linear counter + length counter.
//!
//! Per `docs/apu-2a03.md` §Behavior and NESdev wiki "APU Triangle" page.
//!
//! - Timer counts at the **CPU** clock (not the APU clock — twice as fast as
//!   the pulse channels' timers for the same period value).
//! - Length and linear counters both gate the sequencer; if either is 0 the
//!   sequencer freezes (output holds last value, no click).

use crate::length::LengthCounter;

/// 32-step triangle output sequence (15 down to 0 then 0 up to 15).
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

/// Triangle channel state.
#[derive(Debug, Clone, Copy)]
pub struct Triangle {
    /// 11-bit timer reload (counts at CPU clock).
    pub(crate) timer_period: u16,
    /// Internal countdown timer.
    pub(crate) timer: u16,
    /// Sequencer step (0..=31).
    pub(crate) step: u8,
    /// Length counter (uses control bit `$4008` bit 7 as halt-flag too).
    pub length: LengthCounter,
    /// Linear counter reload value (`$4008` bits 0-6).
    pub(crate) linear_reload_value: u8,
    /// Linear counter current value.
    pub(crate) linear_counter: u8,
    /// Linear counter control flag (`$4008` bit 7) — if clear, the linear
    /// counter clears its reload-flag at the end of the frame; if set, the
    /// reload-flag stays set forever (linear counter behaves like a length
    /// counter halt).
    pub(crate) linear_control: bool,
    /// Linear-counter reload flag — set by `$400B` write, consumed at quarter
    /// frame.
    pub(crate) linear_reload_flag: bool,
}

impl Default for Triangle {
    fn default() -> Self {
        Self::new()
    }
}

impl Triangle {
    /// New triangle channel.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timer_period: 0,
            timer: 0,
            step: 0,
            length: LengthCounter {
                count: 0,
                halt: false,
                new_halt: false,
                enabled: false,
                reload_val: 0,
                previous_count: 0,
            },
            linear_reload_value: 0,
            linear_counter: 0,
            linear_control: false,
            linear_reload_flag: false,
        }
    }

    /// `$4008` write: control bit + linear counter reload value.
    pub fn write_linear(&mut self, value: u8) {
        self.linear_control = (value & 0x80) != 0;
        // Length counter halt = control bit (per NESdev wiki). Deferred:
        // applied after the same-cycle half-frame clock (`LengthCounter::reload`).
        self.length.set_halt(self.linear_control);
        self.linear_reload_value = value & 0x7F;
    }

    /// `$400A` write: timer low.
    pub fn write_timer_lo(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0xFF00) | u16::from(value);
    }

    /// `$400B` write: length load + timer high. Sets the linear reload flag.
    pub fn write_timer_hi(&mut self, value: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (u16::from(value & 0x07) << 8);
        self.length.load(value);
        self.linear_reload_flag = true;
    }

    /// One CPU clock.
    pub fn clock_timer(&mut self) {
        // Ultrasonic-silence (NESdev wiki "APU Triangle"): a timer period below
        // 2 (frequency above ~55.9 kHz) would clock the sequencer faster than
        // hardware can follow; the real channel effectively halts there and the
        // output holds its current step. Most emulators freeze the sequencer to
        // avoid the resulting pop (Mega Man 2's "Crash Man" stage relies on
        // this). We hold the sequencer — output stays at the current step.
        if self.timer_period < 2 {
            return;
        }
        if self.timer == 0 {
            self.timer = self.timer_period;
            // Only advance sequencer if both gates are open.
            if self.length.count > 0 && self.linear_counter > 0 {
                self.step = (self.step + 1) & 0x1F;
            }
        } else {
            self.timer -= 1;
        }
    }

    /// Quarter-frame clock: linear counter.
    pub fn clock_quarter_frame(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        // Control bit clear -> reload flag cleared at quarter clock.
        if !self.linear_control {
            self.linear_reload_flag = false;
        }
    }

    /// Half-frame clock: length counter.
    pub fn clock_half_frame(&mut self) {
        self.length.clock();
    }

    /// Per-cycle output (0..=15).
    ///
    /// The ultrasonic-silence behavior is implemented in [`Self::clock_timer`]
    /// by freezing the sequencer when `timer_period < 2`; the output simply
    /// holds the current step (so it does not pop), matching hardware.
    #[must_use]
    pub fn output(&self) -> u8 {
        TRIANGLE_TABLE[self.step as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_reload_on_quarter_frame() {
        let mut t = Triangle::new();
        t.write_linear(0x40); // control=0, reload=0x40
        t.write_timer_hi(0x08);
        assert!(t.linear_reload_flag);
        t.clock_quarter_frame();
        assert_eq!(t.linear_counter, 0x40);
        assert!(!t.linear_reload_flag); // control=0 clears flag
    }

    #[test]
    fn linear_control_keeps_reload_flag() {
        let mut t = Triangle::new();
        t.write_linear(0xC0); // control=1, reload=0x40
        t.write_timer_hi(0x08);
        t.clock_quarter_frame();
        assert!(t.linear_reload_flag);
    }

    #[test]
    fn sequencer_advances_on_timer_underflow() {
        let mut t = Triangle::new();
        t.length.count = 5;
        t.linear_counter = 5;
        // A non-ultrasonic period (>= 2) so the sequencer is not frozen.
        t.timer_period = 2;
        t.timer = 0;
        t.clock_timer();
        assert_eq!(t.step, 1);
    }

    #[test]
    fn sequencer_frozen_when_length_zero() {
        let mut t = Triangle::new();
        t.length.count = 0;
        t.linear_counter = 5;
        // Non-ultrasonic period so only the length gate (not the ultrasonic
        // freeze) is what holds the sequencer.
        t.timer_period = 2;
        t.timer = 0;
        t.clock_timer();
        assert_eq!(t.step, 0);
    }

    #[test]
    fn ultrasonic_period_freezes_sequencer() {
        // Period < 2 (ultrasonic): hardware halts the sequencer; the step must
        // not advance even with both gates open and the timer expired.
        let mut t = Triangle::new();
        t.length.count = 5;
        t.linear_counter = 5;
        t.timer_period = 1;
        t.timer = 0;
        t.clock_timer();
        assert_eq!(t.step, 0, "step must not advance at period<2");
        // Period 0 is also ultrasonic-silenced.
        t.timer_period = 0;
        t.timer = 0;
        t.clock_timer();
        assert_eq!(t.step, 0, "step must not advance at period==0");
    }

    #[test]
    fn period_two_resumes_clocking() {
        // The threshold is strictly < 2; period == 2 still clocks normally.
        let mut t = Triangle::new();
        t.length.count = 5;
        t.linear_counter = 5;
        t.timer_period = 2;
        t.timer = 0;
        t.clock_timer();
        assert_eq!(t.step, 1, "step must advance at period==2");
    }
}
