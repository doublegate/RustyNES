//! Noise channel: 15-bit LFSR + envelope + length counter.
//!
//! Per `docs/apu-2a03.md` §Behavior and NESdev wiki "APU Noise" page.
//!
//! - Mode 0 (long): feedback = bit 0 XOR bit 1, 15-bit period.
//! - Mode 1 (short): feedback = bit 0 XOR bit 6, 93-bit period.

use crate::envelope::Envelope;
use crate::length::LengthCounter;
use crate::Region;

/// 16-entry NTSC noise period table (NESdev wiki).  Index = bits 0-3 of `$400E`.
pub const NTSC_NOISE_PERIODS: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// 16-entry PAL noise period table (NESdev wiki).
pub const PAL_NOISE_PERIODS: [u16; 16] = [
    4, 7, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708, 944, 1890, 3778,
];

/// Noise channel state.
#[derive(Debug, Clone, Copy)]
pub struct Noise {
    /// LFSR (initialized to 1 on power-up; only bottom 15 bits used).
    pub(crate) lfsr: u16,
    /// Mode (false = long / 15-bit, true = short / 6-bit).
    pub(crate) mode: bool,
    /// Timer reload (from period table).
    pub(crate) timer_period: u16,
    /// Internal countdown timer.
    pub(crate) timer: u16,
    /// Envelope generator.
    pub envelope: Envelope,
    /// Length counter.
    pub length: LengthCounter,
    /// Region (selects period table).
    pub(crate) region: Region,
}

impl Noise {
    /// New noise channel.
    #[must_use]
    pub const fn new(region: Region) -> Self {
        Self {
            lfsr: 1,
            mode: false,
            timer_period: 4,
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
            region,
        }
    }

    /// `$400C` write.
    pub fn write_ctrl(&mut self, value: u8) {
        let halt = (value & 0x20) != 0;
        self.length.halt = halt;
        self.envelope.loop_flag = halt;
        self.envelope.constant = (value & 0x10) != 0;
        self.envelope.volume_or_period = value & 0x0F;
    }

    /// `$400E` write: mode + period index.
    pub fn write_period(&mut self, value: u8) {
        self.mode = (value & 0x80) != 0;
        let idx = (value & 0x0F) as usize;
        self.timer_period = match self.region {
            Region::Pal => PAL_NOISE_PERIODS[idx],
            _ => NTSC_NOISE_PERIODS[idx],
        };
    }

    /// `$400F` write: length load + envelope restart.
    pub fn write_length(&mut self, value: u8) {
        self.length.load(value);
        self.envelope.start = true;
    }

    /// One APU clock.
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            // LFSR step.
            let bit_a = self.lfsr & 1;
            let bit_b = if self.mode {
                (self.lfsr >> 6) & 1
            } else {
                (self.lfsr >> 1) & 1
            };
            let feedback = bit_a ^ bit_b;
            self.lfsr = (self.lfsr >> 1) | (feedback << 14);
        } else {
            self.timer -= 1;
        }
    }

    /// Half-frame clock: length.
    pub fn clock_half_frame(&mut self) {
        self.length.clock();
    }

    /// Quarter-frame clock: envelope.
    pub fn clock_quarter_frame(&mut self) {
        self.envelope.clock();
    }

    /// Per-cycle output (0..=15).
    #[must_use]
    pub fn output(&self) -> u8 {
        if self.length.count == 0 || (self.lfsr & 1) != 0 {
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
    fn lfsr_long_mode_taps_bit1() {
        let mut n = Noise::new(Region::Ntsc);
        n.timer = 0;
        n.timer_period = 0;
        n.mode = false;
        n.lfsr = 1;
        n.clock_timer();
        // bit0=1 ^ bit1=0 = 1 -> shift right yields 0 with feedback in bit14.
        assert_eq!(n.lfsr, 0x4000);
    }

    #[test]
    fn lfsr_short_mode_taps_bit6() {
        let mut n = Noise::new(Region::Ntsc);
        n.timer = 0;
        n.timer_period = 0;
        n.mode = true;
        n.lfsr = 1;
        n.clock_timer();
        // bit0=1 ^ bit6=0 = 1.
        assert_eq!(n.lfsr, 0x4000);
    }
}
