//! APU Sweep Unit.
//!
//! The sweep unit is used by the pulse channels to periodically adjust
//! the channel's period, creating effects like rising or falling pitch.
//!
//! The sweep unit computes a target period and optionally mutes the channel
//! if the target period would be invalid.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Pulse channel identifier (needed for one's complement behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PulseChannel {
    /// Pulse 1 uses one's complement for negate.
    One,
    /// Pulse 2 uses two's complement for negate.
    Two,
}

/// Sweep unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Sweep {
    /// Enabled flag.
    enabled: bool,
    /// Divider period.
    period: u8,
    /// Negate flag.
    negate: bool,
    /// Shift count.
    shift: u8,
    /// Reload flag.
    reload: bool,
    /// Divider counter.
    divider: u8,
    /// Channel identifier (affects negate behavior).
    channel: PulseChannel,
}

impl Sweep {
    /// Create a new sweep unit.
    #[must_use]
    pub const fn new(channel: PulseChannel) -> Self {
        Self {
            enabled: false,
            period: 0,
            negate: false,
            shift: 0,
            reload: false,
            divider: 0,
            channel,
        }
    }

    /// Write the sweep register.
    /// Bits: EPPP NSSS
    /// - E: Enabled
    /// - P: Period (divider period is P + 1)
    /// - N: Negate
    /// - S: Shift count
    pub fn write(&mut self, value: u8) {
        self.enabled = value & 0x80 != 0;
        self.period = (value >> 4) & 0x07;
        self.negate = value & 0x08 != 0;
        self.shift = value & 0x07;
        self.reload = true;
    }

    /// Calculate the target period.
    /// Returns None if the channel should be muted.
    #[must_use]
    pub fn target_period(&self, current_period: u16) -> Option<u16> {
        let change = current_period >> self.shift;

        let target = if self.negate {
            match self.channel {
                PulseChannel::One => current_period.wrapping_sub(change).wrapping_sub(1),
                PulseChannel::Two => current_period.wrapping_sub(change),
            }
        } else {
            current_period.wrapping_add(change)
        };

        // Mute if target period > $7FF
        if target > 0x7FF { None } else { Some(target) }
    }

    /// Check if the channel is muted due to sweep.
    #[must_use]
    pub fn muted(&self, current_period: u16) -> bool {
        // Mute if current period < 8 or target period > $7FF
        current_period < 8 || self.target_period(current_period).is_none()
    }

    /// Clock the sweep unit. Returns the new period if it should change.
    pub fn clock(&mut self, current_period: u16) -> Option<u16> {
        let result = if self.divider == 0 && self.enabled && self.shift > 0 {
            // Update period if not muted
            if self.muted(current_period) {
                None
            } else {
                self.target_period(current_period)
            }
        } else {
            None
        };

        if self.divider == 0 || self.reload {
            self.divider = self.period;
            self.reload = false;
        } else {
            self.divider -= 1;
        }

        result
    }
}

impl Default for Sweep {
    fn default() -> Self {
        Self::new(PulseChannel::One)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_period_no_negate() {
        let mut sweep = Sweep::new(PulseChannel::One);
        sweep.write(0x01); // Shift = 1, negate = false

        // Period 400, shift right 1 = 200, target = 600
        assert_eq!(sweep.target_period(400), Some(600));
    }

    #[test]
    fn test_target_period_negate_pulse1() {
        let mut sweep = Sweep::new(PulseChannel::One);
        sweep.write(0x09); // Shift = 1, negate = true

        // Period 400, shift right 1 = 200, target = 400 - 200 - 1 = 199
        assert_eq!(sweep.target_period(400), Some(199));
    }

    #[test]
    fn test_target_period_negate_pulse2() {
        let mut sweep = Sweep::new(PulseChannel::Two);
        sweep.write(0x09); // Shift = 1, negate = true

        // Period 400, shift right 1 = 200, target = 400 - 200 = 200
        assert_eq!(sweep.target_period(400), Some(200));
    }

    #[test]
    fn test_target_period_overflow() {
        let mut sweep = Sweep::new(PulseChannel::One);
        sweep.write(0x01); // Shift = 1, negate = false

        // Period $700, shift right 1 = $380, target = $A80 > $7FF
        assert_eq!(sweep.target_period(0x700), None);
    }

    #[test]
    fn test_muted_low_period() {
        let sweep = Sweep::new(PulseChannel::One);
        assert!(sweep.muted(7)); // Period < 8
        assert!(!sweep.muted(8)); // Period >= 8
    }

    #[test]
    fn test_clock_updates_period() {
        let mut sweep = Sweep::new(PulseChannel::One);
        sweep.write(0x81); // Enabled, period = 0, shift = 1

        // First clock should update
        let result = sweep.clock(400);
        assert_eq!(result, Some(600));
    }
}
