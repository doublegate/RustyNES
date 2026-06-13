//! Envelope generator (pulse channels + noise channel).
//!
//! Per `docs/apu-2a03.md` §State and the NESdev wiki "APU Envelope" page.
//! Two modes:
//! - **Constant volume**: output = `volume_or_period`.
//! - **Decay**: a 4-bit counter decays from 15 to 0 (looping if `loop_flag`).
//!
//! The envelope is clocked once per quarter-frame.  A "start" flag (set by
//! `$4003`/`$4007`/`$400F` writes) reloads the decay counter and the divider
//! on the next clock.

/// Envelope generator.
#[derive(Debug, Clone, Copy, Default)]
pub struct Envelope {
    /// Restart flag — set by length-load writes; consumed at next quarter clock.
    pub start: bool,
    /// Loop flag (a.k.a. length-counter halt, depending on the channel).
    pub loop_flag: bool,
    /// Constant-volume bit. When set, output = `volume_or_period`.
    pub constant: bool,
    /// Volume (constant mode) or divider period - 1 (decay mode).
    pub volume_or_period: u8,
    /// Internal divider counter.
    pub divider: u8,
    /// Internal decay counter (4-bit, 15 -> 0).
    pub decay: u8,
}

impl Envelope {
    /// Quarter-frame clock.
    pub fn clock(&mut self) {
        if self.start {
            // Reload: decay = 15, divider = period.
            self.start = false;
            self.decay = 15;
            self.divider = self.volume_or_period;
        } else if self.divider == 0 {
            self.divider = self.volume_or_period;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.loop_flag {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    /// Output volume (0..=15).
    #[must_use]
    pub const fn output(&self) -> u8 {
        if self.constant {
            self.volume_or_period
        } else {
            self.decay
        }
    }
}
