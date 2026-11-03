//! Noise channel: 15-bit LFSR + envelope + length counter.
//!
//! Per `docs/apu-2a03.md` §Behavior and NESdev wiki "APU Noise" page.
//!
//! - Mode 0 (long): feedback = bit 0 XOR bit 1, a 32767-step period.
//! - Mode 1 (short): feedback = bit 0 XOR bit 6, a 93- or 31-step period
//!   (which one depends on the register's state when the mode is set).

use crate::Region;
use crate::envelope::Envelope;
use crate::length::LengthCounter;

/// 16-entry NTSC noise period table (NESdev wiki), in CPU cycles.
///
/// Each entry is the CPU cycles between shift-register clocks; index = bits
/// 0-3 of `$400E`. The timer runs once
/// per APU cycle, so [`Noise::write_period`] converts an entry to a reload
/// of `N / 2 - 1` timer clocks.
pub const NTSC_NOISE_PERIODS: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// 16-entry PAL noise period table (NESdev wiki), in CPU cycles. Index 1
/// read 7 until v3.6.0; the page gives 8, and its periods are all even.
pub const PAL_NOISE_PERIODS: [u16; 16] = [
    4, 8, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708, 944, 1890, 3778,
];

/// Noise channel state.
#[derive(Debug, Clone, Copy)]
pub struct Noise {
    /// LFSR (initialized to 1 on power-up; only bottom 15 bits used).
    pub(crate) lfsr: u16,
    /// Mode: false = long (feedback from bit 1, 32767 steps), true = short
    /// (feedback from bit 6, 93 or 31 steps depending on where the register
    /// was when the mode was set; NESdev wiki "APU Noise").
    pub(crate) mode: bool,
    /// Timer reload in APU cycles: the table's CPU-cycle period halved,
    /// minus one, because the timer reloads on the clock after it reaches 0.
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
            timer_period: NTSC_NOISE_PERIODS[0] / 2 - 1,
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
                new_halt: false,
                enabled: false,
                reload_val: 0,
                previous_count: 0,
            },
            region,
        }
    }

    /// `$400C` write.
    pub fn write_ctrl(&mut self, value: u8) {
        let halt = (value & 0x20) != 0;
        // Length-halt is deferred (applied after the same-cycle half-frame
        // clock, per `LengthCounter::reload`); the envelope loop flag is not.
        self.length.set_halt(halt);
        self.envelope.loop_flag = halt;
        self.envelope.constant = (value & 0x10) != 0;
        self.envelope.volume_or_period = value & 0x0F;
    }

    /// `$400E` write: mode + period index.
    pub fn write_period(&mut self, value: u8) {
        self.mode = (value & 0x80) != 0;
        let idx = (value & 0x0F) as usize;
        let cpu_cycles = match self.region {
            Region::Pal => PAL_NOISE_PERIODS[idx],
            _ => NTSC_NOISE_PERIODS[idx],
        };
        // NESdev "APU Noise": the table gives CPU cycles between shifts, and
        // the timer is clocked once per APU cycle (two CPU cycles). A reload
        // of R gives R + 1 timer clocks per shift, so R = N / 2 - 1. Until
        // v3.6.0 the entry itself was the reload, which shifted every
        // 2 x (N + 1) CPU cycles: about half the documented rate.
        self.timer_period = cpu_cycles / 2 - 1;
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

    /// Count `clock_timer` calls (APU cycles, two CPU cycles each) between
    /// two consecutive LFSR shifts at period index `idx`.
    fn apu_cycles_per_shift(region: Region, idx: u8) -> u32 {
        let mut n = Noise::new(region);
        n.write_period(idx);
        n.lfsr = 1;
        // Run to the first shift so the count starts on a reload.
        while n.lfsr == 1 {
            n.clock_timer();
        }
        let before = n.lfsr;
        let mut count = 0;
        while n.lfsr == before {
            n.clock_timer();
            count += 1;
        }
        count
    }

    /// NESdev "APU Noise": "The period determines how many CPU cycles happen
    /// between shift register clocks. These periods are all even numbers
    /// because there are 2 CPU cycles in an APU cycle." The timer is clocked
    /// once per APU cycle, so a table entry of N CPU cycles is N / 2 timer
    /// clocks. Until v3.6.0 the table value was loaded as the timer's reload,
    /// which shifted every 2 x (N + 1) CPU cycles: the noise ran at about
    /// half its documented rate on every region (MiSTer sibling finding 9.2).
    #[test]
    fn the_lfsr_shifts_every_table_period_in_cpu_cycles() {
        for idx in 0..16u8 {
            let i = idx as usize;
            assert_eq!(
                apu_cycles_per_shift(Region::Ntsc, idx) * 2,
                u32::from(NTSC_NOISE_PERIODS[i]),
                "NTSC index {idx}"
            );
            assert_eq!(
                apu_cycles_per_shift(Region::Pal, idx) * 2,
                u32::from(PAL_NOISE_PERIODS[i]),
                "PAL index {idx}"
            );
        }
    }

    /// The PAL table as the NESdev page gives it: index 1 is 8 CPU cycles.
    /// Until v3.6.0 it read 7, an odd period the page's own note ("all even
    /// numbers") rules out (MiSTer sibling finding 9.1).
    #[test]
    fn the_pal_table_matches_the_page() {
        assert_eq!(
            PAL_NOISE_PERIODS,
            [
                4, 8, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708, 944, 1890, 3778
            ]
        );
        assert!(PAL_NOISE_PERIODS.iter().all(|p| p % 2 == 0));
        assert!(NTSC_NOISE_PERIODS.iter().all(|p| p % 2 == 0));
    }
}
