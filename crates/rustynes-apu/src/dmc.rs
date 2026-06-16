//! DMC (delta-modulation channel).
//!
//! Per `docs/apu-2a03.md` §DMC channel and NESdev wiki "APU DMC" page.
//!
//! Architecture:
//! - **Memory reader**: fetches sample bytes via DMA.
//! - **Sample buffer**: 1-byte; loaded from memory reader, drained into the
//!   bit-shift register.
//! - **Output unit**: 8-bit shift register + bits-remaining counter; clocks
//!   the 7-bit DAC value by ±2 per bit.
//! - **Timer**: counts at the APU clock; period from rate table.

use crate::Region;

/// v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): swept byte-timer realignment.
///
/// A signed byte-timer phase shift (in APU-rate timer units) applied ONCE at
/// the `$4015` re-enable exclusion boundary (when `cannot_run == 2` blocks a
/// would-be looping-reload arm). `0` = no shift (the bare exclusion gate). Env
/// `RUSTYNES_REENABLE_BUMP`.
pub static REENABLE_BUMP: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// v2.0 final lever #1 (`mc-r1-dmc-halt-subpos`): swept per-CPU-cycle arm DELAY.
///
/// Number of CPU cycles to DELAY the `$540` X=10/11 reload arm (pattern-A
/// boundary) past the natural cannot_run-deferred re-arm. Sub-APU-cycle
/// granularity the byte-timer phase shift cannot express. `0` = arm at the
/// boundary (no delay). Env `RUSTYNES_SUBPOS_DELAY` (default 1).
pub static SUBPOS_DELAY: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(1);

/// 16-entry NTSC rate table (CPU cycles per output bit).  NESdev wiki.
pub const NTSC_DMC_RATES: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// 16-entry PAL rate table.
pub const PAL_DMC_RATES: [u16; 16] = [
    398, 354, 316, 298, 276, 236, 210, 198, 176, 148, 132, 118, 98, 78, 66, 50,
];

/// DMC channel state.
#[derive(Debug, Clone, Copy)]
pub struct Dmc {
    // ----- Configuration (set by `$4010-$4013`) -----
    /// IRQ enable bit (`$4010` bit 7).
    pub irq_enable: bool,
    /// Loop bit (`$4010` bit 6).
    pub loop_flag: bool,
    /// Rate index (`$4010` bits 0-3).
    pub(crate) rate_index: u8,
    /// Sample address (`$4012` × 64 + 0xC000).
    pub(crate) sample_addr: u16,
    /// Sample length (`$4013` × 16 + 1).
    pub(crate) sample_length: u16,

    // ----- Memory reader -----
    /// Current memory address.
    pub current_addr: u16,
    /// Bytes remaining to fetch.
    pub bytes_remaining: u16,

    // ----- Sample buffer -----
    /// Sample byte awaiting transfer to shift register.
    pub(crate) sample_buffer: Option<u8>,

    // ----- Output unit -----
    /// 8-bit shift register.
    pub(crate) shift_register: u8,
    /// Bits remaining in shift register (0..=8).
    pub(crate) bits_remaining: u8,
    /// 7-bit DAC value (0..=127).
    pub dac: u8,
    /// Silenced flag (no sample data when output cycle began).
    pub(crate) silence: bool,

    // ----- Timer -----
    /// Timer reload (from rate table).
    pub(crate) timer_period: u16,
    /// Current timer.
    pub(crate) timer: u16,

    // ----- IRQ -----
    /// Latched IRQ flag (cleared by writing `$4015`).
    pub irq_flag: bool,

    /// Region.
    region: Region,
}

impl Dmc {
    /// Construct a new DMC channel.
    #[must_use]
    pub const fn new(region: Region) -> Self {
        let cpu_period = match region {
            Region::Pal => PAL_DMC_RATES[0],
            _ => NTSC_DMC_RATES[0],
        };
        let timer_period = cpu_period / 2 - 1;
        Self {
            irq_enable: false,
            loop_flag: false,
            rate_index: 0,
            sample_addr: 0xC000,
            sample_length: 1,
            current_addr: 0xC000,
            bytes_remaining: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: 0,
            dac: 0,
            silence: true,
            timer_period,
            timer: 0,
            irq_flag: false,
            region,
        }
    }

    /// `$4010` write: IRQ enable + loop + rate index.
    ///
    /// Note: the public NTSC/PAL rate tables are the period in CPU cycles
    /// per output bit.  The DMC timer ticks at the APU clock (= half CPU
    /// rate), so the internal reload value is `cpu_period / 2 - 1`.
    pub fn write_ctrl(&mut self, value: u8) {
        self.irq_enable = (value & 0x80) != 0;
        self.loop_flag = (value & 0x40) != 0;
        self.rate_index = value & 0x0F;
        let cpu_period = match self.region {
            Region::Pal => PAL_DMC_RATES[self.rate_index as usize],
            _ => NTSC_DMC_RATES[self.rate_index as usize],
        };
        // The DMC timer ticks at the APU clock (= half CPU rate), so the
        // internal reload value is `cpu_period / 2 - 1`.
        self.timer_period = (cpu_period / 2).saturating_sub(1);
        if !self.irq_enable {
            self.irq_flag = false;
        }
    }

    /// `$4011` write: direct 7-bit DAC.
    pub fn write_dac(&mut self, value: u8) {
        self.dac = value & 0x7F;
    }

    /// `$4012` write: sample address.
    pub fn write_sample_addr(&mut self, value: u8) {
        self.sample_addr = 0xC000 | (u16::from(value) << 6);
    }

    /// `$4013` write: sample length.
    pub fn write_sample_length(&mut self, value: u8) {
        self.sample_length = (u16::from(value) << 4) | 1;
    }

    /// Status (`$4015` read bit 4): bytes-remaining > 0.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.bytes_remaining > 0
    }

    /// `$4015` write effect on DMC: bit 4 set restarts sample if not running;
    /// bit 4 clear silences (sets bytes-remaining=0).  Always clears `irq_flag`.
    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled {
            if self.bytes_remaining == 0 {
                self.current_addr = self.sample_addr;
                self.bytes_remaining = self.sample_length;
            }
        } else {
            self.bytes_remaining = 0;
        }
        // `$4015` write clears DMC IRQ flag (per nesdev: any write to $4015).
        self.irq_flag = false;
    }

    /// Returns `true` if the DMC needs a DMA fetch right now (sample buffer
    /// empty and bytes remaining).  Caller is responsible for halting the
    /// CPU and supplying the byte via [`Self::deliver_sample`].
    #[must_use]
    pub const fn needs_dma(&self) -> bool {
        self.sample_buffer.is_none() && self.bytes_remaining > 0
    }

    /// The address the DMA controller must read.
    #[must_use]
    pub const fn dma_addr(&self) -> u16 {
        self.current_addr
    }

    /// Consume a fetched byte from the DMA controller.
    pub fn deliver_sample(&mut self, byte: u8) {
        self.sample_buffer = Some(byte);
        // Advance memory reader: addr wraps from $FFFF -> $8000.
        self.current_addr = match self.current_addr {
            0xFFFF => 0x8000,
            other => other.wrapping_add(1),
        };
        // A `$4015` bit-4 clear that races a DMA in flight can land between
        // `needs_dma() == true` and `deliver_sample`. In that case
        // `bytes_remaining` is already 0; accept the fetched byte into the
        // buffer (the playback unit consumes it) but don't underflow the
        // counter or re-trigger a DMA chain.
        if self.bytes_remaining == 0 {
            return;
        }
        self.bytes_remaining -= 1;
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.current_addr = self.sample_addr;
                self.bytes_remaining = self.sample_length;
            } else if self.irq_enable {
                self.irq_flag = true;
            }
        }
    }

    /// One APU clock — drives the timer, output unit, and (on buffer empty)
    /// loads the shift register from the sample buffer.
    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.clock_output();
        } else {
            self.timer -= 1;
        }
    }

    fn clock_output(&mut self) {
        if !self.silence {
            // Bit 0 of shift register modifies DAC by ±2.
            if (self.shift_register & 1) != 0 {
                if self.dac <= 125 {
                    self.dac += 2;
                }
            } else if self.dac >= 2 {
                self.dac -= 2;
            }
        }
        self.shift_register >>= 1;
        if self.bits_remaining > 0 {
            self.bits_remaining -= 1;
        }
        if self.bits_remaining == 0 {
            // Reload from sample buffer.
            self.bits_remaining = 8;
            if let Some(b) = self.sample_buffer.take() {
                self.silence = false;
                self.shift_register = b;
            } else {
                self.silence = true;
            }
        }
    }

    /// Per-cycle output (0..=127).
    #[must_use]
    pub const fn output(&self) -> u8 {
        self.dac
    }

    /// v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): apply a one-time signed phase
    /// shift to the byte-timer at the `$4015` re-enable boundary, realigning the
    /// looping-reload chain by `delta` CPU cycles (the TriCNES re-enable model
    /// resets `APU_ChannelTimer_DMC` to a phase RustyNES otherwise lands 1 cycle
    /// off for the Implicit-DMA-Abort X=10/11 entries). `delta` is in the
    /// timer's own (APU-rate) units; wraps within `[0, timer_period]`.
    pub(crate) fn bump_timer_phase(&mut self, delta: i32) {
        let p = i32::from(self.timer_period) + 1;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let t = (i32::from(self.timer) + delta).rem_euclid(p) as u16;
        self.timer = t;
    }

    /// Diagnostic: current byte-timer countdown value. Used by the per-cycle
    /// DMC-DMA cross-diff tracing (`crates/rustynes-test-harness/src/bin/
    /// trace_dma_4015.rs`) to expose the internal byte-timer phase that the
    /// abort-context reload arm depends on.
    #[must_use]
    pub const fn timer(&self) -> u16 {
        self.timer
    }

    /// Diagnostic: bits remaining in the output shift register (0..=8).
    #[must_use]
    pub const fn bits_remaining(&self) -> u8 {
        self.bits_remaining
    }

    /// Diagnostic: output-unit silence flag.
    #[must_use]
    pub const fn silence(&self) -> bool {
        self.silence
    }

    /// Diagnostic: sample buffer occupied (a byte awaits transfer to the
    /// shift register).
    #[must_use]
    pub const fn buffer_full(&self) -> bool {
        self.sample_buffer.is_some()
    }

    /// v2.0 abort-context reload-arm phase fix (`mc-r1-dmc-abort-timer-phase`).
    /// When the output unit is silent (the byte-timer boundary's `clock_output`
    /// took an empty buffer this cycle) but a LOAD DMA has just filled the
    /// buffer, consume that byte into the shift register and clear silence —
    /// matching TriCNES, whose LOAD completes before the boundary so the boundary
    /// consumes the load byte. Emptying the buffer here lets the per-cycle
    /// reload-arm fire promptly (the reload that RustyNES otherwise deferred 4
    /// cycles). Returns `true` if a byte was consumed.
    pub(crate) fn consume_buffer_into_shifter_if_silent(&mut self) -> bool {
        // Gate on the byte-timer having JUST reset this cycle (`timer ==
        // timer_period`): that is the exact boundary-coincidence — `clock_output`
        // wrapped the timer + reloaded bits this same cycle and took the empty
        // buffer (silence) before the LOAD `deliver_sample` filled it. A LOAD
        // that delivers mid-byte (timer below period — e.g. the Loop1/Loop2
        // implicit-abort 1-byte loads) must NOT be re-consumed here; that would
        // corrupt the implicit-abort 1-cycle-DMA measurement.
        if self.silence
            && self.timer == self.timer_period
            && let Some(b) = self.sample_buffer.take()
        {
            self.shift_register = b;
            self.silence = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_dac_masks_high_bit() {
        let mut d = Dmc::new(Region::Ntsc);
        d.write_dac(0xFF);
        assert_eq!(d.dac, 0x7F);
    }

    #[test]
    fn enabling_starts_sample() {
        let mut d = Dmc::new(Region::Ntsc);
        d.write_sample_addr(0x10); // $C000 + 0x10*64 = $C400
        d.write_sample_length(0x10); // 0x10*16 + 1 = 0x101
        d.set_enabled(true);
        assert_eq!(d.current_addr, 0xC400);
        assert_eq!(d.bytes_remaining, 0x101);
    }

    #[test]
    fn disabling_silences() {
        let mut d = Dmc::new(Region::Ntsc);
        d.write_sample_length(0x10);
        d.set_enabled(true);
        d.set_enabled(false);
        assert_eq!(d.bytes_remaining, 0);
    }

    #[test]
    fn deliver_sample_wraps_address() {
        let mut d = Dmc::new(Region::Ntsc);
        d.bytes_remaining = 2;
        d.current_addr = 0xFFFF;
        d.deliver_sample(0xAA);
        assert_eq!(d.current_addr, 0x8000);
    }

    #[test]
    fn end_of_sample_raises_irq_when_enabled() {
        let mut d = Dmc::new(Region::Ntsc);
        d.irq_enable = true;
        d.bytes_remaining = 1;
        d.deliver_sample(0xAA);
        assert!(d.irq_flag);
    }

    #[test]
    fn writing_4015_clears_irq_flag() {
        let mut d = Dmc::new(Region::Ntsc);
        d.irq_flag = true;
        d.set_enabled(false);
        assert!(!d.irq_flag);
    }

    #[test]
    fn deliver_sample_after_disable_does_not_underflow() {
        // Race: DMA scheduled (needs_dma == true), then `$4015` bit 4 cleared
        // before the bus could service it. The in-flight byte arrives with
        // bytes_remaining already 0 — must not underflow the u16.
        let mut d = Dmc::new(Region::Ntsc);
        d.bytes_remaining = 0;
        d.deliver_sample(0xAA);
        assert_eq!(d.bytes_remaining, 0);
        assert_eq!(d.sample_buffer, Some(0xAA));
    }
}
