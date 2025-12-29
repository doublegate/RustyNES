//! Frame Counter Module
//!
//! The frame counter divides time into frames and quarter-frames, timing
//! envelope, length counter, and sweep updates. Operates in 4-step (60 Hz) or 5-step (48 Hz) mode.

/// Actions triggered by the frame counter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAction {
    /// No action this cycle
    None,
    /// Clock envelopes and linear counter (quarter frame)
    QuarterFrame,
    /// Clock envelopes, linear counter, length counters, and sweep units (half frame)
    HalfFrame,
}

/// Frame counter state machine
///
/// Controls timing for envelope generators, length counters, and sweep units.
///
/// # Modes
///
/// - **4-step mode (Mode 0)**: 29,829 CPU cycles per frame, generates IRQ
/// - **5-step mode (Mode 1)**: 37,281 CPU cycles per frame, no IRQ
///
/// # $4017 Write Delay
///
/// Writing to $4017 does not immediately reset the frame counter. Per `NESdev` wiki:
/// - If the write occurs during an APU cycle (even CPU cycle): 3 cycle delay
/// - If the write occurs between APU cycles (odd CPU cycle): 4 cycle delay
pub struct FrameCounter {
    /// Frame counter mode: 0 = 4-step (60 Hz), 1 = 5-step (48 Hz)
    mode: u8,
    /// IRQ inhibit flag (from $4017 bit 6)
    irq_inhibit: bool,
    /// Current cycle count within the frame
    cycle_count: u64,
    /// Frame IRQ flag (read by CPU)
    pub irq_flag: bool,
    /// Pending $4017 write: (value, `cycles_remaining` until applied)
    pending_write: Option<(u8, u8)>,
}

impl FrameCounter {
    /// Creates a new frame counter in 4-step mode
    ///
    /// Power-on state: Behaves as if $4017 written with $00
    /// - Mode 0 (4-step)
    /// - IRQ inhibit disabled (IRQ enabled)
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: 0,
            irq_inhibit: false, // Power-on: IRQ enabled (as if $4017=$00 written)
            cycle_count: 0,
            irq_flag: false,
            pending_write: None,
        }
    }

    /// Writes to the frame counter control register ($4017)
    ///
    /// # Register Format
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// MI-- ----
    /// ||
    /// |+------- IRQ inhibit (0: IRQ enabled, 1: IRQ disabled)
    /// +-------- Mode (0: 4-step, 1: 5-step)
    /// ```
    ///
    /// Writing to this register (after delay):
    /// - Resets the cycle counter to 0
    /// - Clears IRQ flag if IRQ inhibit is set
    /// - If 5-step mode, immediately triggers half-frame action
    ///
    /// # Arguments
    ///
    /// * `value` - The value being written to $4017
    /// * `cpu_cycle_odd` - True if the CPU is on an odd cycle (determines delay)
    ///
    /// # Delay Timing
    ///
    /// The reset aligns to the next APU cycle boundary:
    /// - Even CPU cycle (during APU tick): 2 cycle delay
    /// - Odd CPU cycle (between APU ticks): 3 cycle delay
    pub fn write_control(&mut self, value: u8, cpu_cycle_odd: bool) -> FrameAction {
        // Clear IRQ flag immediately if inhibit is set in the new value
        if (value & 0x40) != 0 {
            self.irq_flag = false;
        }

        // Calculate delay based on CPU cycle parity
        // The reset takes effect after the write cycle completes and aligns to APU cycle
        // Even CPU cycle (during APU cycle): 3 cycle delay
        // Odd CPU cycle (between APU cycles): 4 cycle delay
        let delay = if cpu_cycle_odd { 4 } else { 3 };

        // Schedule the write to take effect after the delay
        self.pending_write = Some((value, delay));

        // The write doesn't take effect immediately, so no action yet
        FrameAction::None
    }

    /// Apply a pending $4017 write (internal helper)
    fn apply_pending_write(&mut self, value: u8) -> FrameAction {
        self.mode = (value >> 7) & 1;
        self.irq_inhibit = (value & 0x40) != 0;

        // Reset cycle counter
        self.cycle_count = 0;

        // Clear IRQ flag if inhibit set
        if self.irq_inhibit {
            self.irq_flag = false;
        }

        // If 5-step mode, immediately clock half frame
        if self.mode == 1 {
            FrameAction::HalfFrame
        } else {
            FrameAction::None
        }
    }

    /// Clocks the frame counter by one CPU cycle
    ///
    /// Returns the frame action to be taken (if any).
    pub fn clock(&mut self) -> FrameAction {
        // Handle pending $4017 write
        if let Some((value, cycles_remaining)) = self.pending_write {
            if cycles_remaining <= 1 {
                // Apply the write now
                self.pending_write = None;
                return self.apply_pending_write(value);
            }
            // Decrement delay counter
            self.pending_write = Some((value, cycles_remaining - 1));
        }

        self.cycle_count += 1;

        match self.mode {
            0 => self.clock_4step(),
            1 => self.clock_5step(),
            _ => FrameAction::None,
        }
    }

    /// Clocks the 4-step mode sequencer
    ///
    /// # Sequence (per `NESdev` wiki)
    ///
    /// ```text
    /// Step   Cycles  Action
    /// ----   ------  ------
    /// 1      7457    Quarter frame (envelopes + linear counter)
    /// 2      14913   Half frame (envelopes, linear, length, sweep)
    /// 3      22371   Quarter frame
    /// 4      29828   Set IRQ flag (if enabled)
    ///        29829   Set IRQ flag, half frame
    ///        29830   Set IRQ flag, reset to 0
    /// ```
    fn clock_4step(&mut self) -> FrameAction {
        match self.cycle_count {
            // Quarter frames at 7457 and 22371 (per NESdev wiki)
            7457 | 22371 => FrameAction::QuarterFrame,
            14913 => FrameAction::HalfFrame,
            29828 => {
                if !self.irq_inhibit {
                    self.irq_flag = true;
                }
                FrameAction::None
            }
            29829 => {
                if !self.irq_inhibit {
                    self.irq_flag = true;
                }
                FrameAction::HalfFrame
            }
            29830 => {
                if !self.irq_inhibit {
                    self.irq_flag = true;
                }
                self.cycle_count = 0;
                FrameAction::None
            }
            _ => FrameAction::None,
        }
    }

    /// Clocks the 5-step mode sequencer
    ///
    /// # Sequence (per `NESdev` wiki - validated against Blargg test ROMs)
    ///
    /// ```text
    /// Step   Cycles  Action
    /// ----   ------  ------
    /// 1      7457    Quarter frame
    /// 2      14913   Half frame
    /// 3      22371   Quarter frame
    /// 4      29829   (nothing)
    /// 5      37281   Half frame
    /// 0      37282   Reset to 0
    /// ```
    fn clock_5step(&mut self) -> FrameAction {
        match self.cycle_count {
            7457 | 22371 => FrameAction::QuarterFrame,
            14913 | 37281 => FrameAction::HalfFrame,
            37282 => {
                self.cycle_count = 0;
                FrameAction::None
            }
            _ => FrameAction::None,
        }
    }

    /// Returns whether a frame IRQ is pending
    #[must_use]
    pub const fn irq_pending(&self) -> bool {
        self.irq_flag && !self.irq_inhibit
    }

    /// Clears the IRQ flag (called when status register is read)
    pub fn clear_irq(&mut self) {
        self.irq_flag = false;
    }
}

impl Default for FrameCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_counter_4step_sequence() {
        let mut fc = FrameCounter::new();
        assert_eq!(fc.mode, 0);

        // Clock to first quarter frame (cycle 7457)
        for _ in 0..7456 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to first half frame (cycle 14913)
        for _ in 0..7455 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::HalfFrame);

        // Clock to second quarter frame (cycle 22371)
        for _ in 0..7457 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to IRQ point (cycle 29828) - IRQ starts here
        for _ in 0..7456 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert!(!fc.irq_flag);
        // Cycle 29828: IRQ set, no action
        assert_eq!(fc.clock(), FrameAction::None);
        assert!(fc.irq_flag);

        // Cycle 29829: IRQ set, half frame
        assert_eq!(fc.clock(), FrameAction::HalfFrame);
        assert!(fc.irq_flag);

        // Cycle 29830: IRQ set, reset to 0
        assert_eq!(fc.clock(), FrameAction::None);
        assert!(fc.irq_flag);

        // Should reset to 0
        assert_eq!(fc.cycle_count, 0);
    }

    #[test]
    fn test_frame_counter_5step_sequence() {
        let mut fc = FrameCounter::new();

        // Write 5-step mode with even CPU cycle (3 cycle delay)
        fc.write_control(0x80, false);

        // Clock through the delay (3 cycles)
        assert_eq!(fc.clock(), FrameAction::None); // delay 2 remaining
        assert_eq!(fc.clock(), FrameAction::None); // delay 1 remaining
        assert_eq!(fc.clock(), FrameAction::HalfFrame); // write applied

        assert_eq!(fc.mode, 1);
        assert_eq!(fc.cycle_count, 0);

        // Clock to first quarter frame (7457)
        for _ in 0..7456 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to first half frame (14913)
        for _ in 0..7455 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::HalfFrame);

        // Clock to second quarter frame (22371)
        for _ in 0..7457 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to cycle 29829 (no action in 5-step mode)
        for _ in 0..7457 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::None);

        // Clock to final half frame (37281)
        for _ in 0..7451 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::HalfFrame);
        assert_eq!(fc.cycle_count, 37281);

        // Clock one more to reset (37282)
        assert_eq!(fc.clock(), FrameAction::None);
        assert_eq!(fc.cycle_count, 0);
        // 5-step mode never sets IRQ
        assert!(!fc.irq_flag);
    }

    #[test]
    fn test_irq_inhibit() {
        let mut fc = FrameCounter::new();

        // Enable IRQ inhibit with even CPU cycle (3 cycle delay)
        fc.write_control(0x40, false);

        // Clock through delay to apply write (3 cycles)
        fc.clock(); // delay 2 remaining
        fc.clock(); // delay 1 remaining
        fc.clock(); // write applied
        assert!(fc.irq_inhibit);

        // Clock to IRQ point (need more cycles since counter was reset after delay)
        for _ in 0..29829 {
            fc.clock();
        }

        // IRQ should not be set
        assert!(!fc.irq_flag);
        assert!(!fc.irq_pending());
    }

    #[test]
    fn test_write_control_clears_irq_immediately() {
        let mut fc = FrameCounter::new();
        fc.irq_flag = true;

        // Write with IRQ inhibit - IRQ should clear immediately (before delay)
        fc.write_control(0x40, false);

        // IRQ should be cleared immediately, not after delay
        assert!(!fc.irq_flag);
    }

    #[test]
    fn test_5step_delayed_half_frame() {
        let mut fc = FrameCounter::new();

        // Writing 5-step mode schedules a delayed write
        let action = fc.write_control(0x80, false);
        // The write itself returns None (delayed)
        assert_eq!(action, FrameAction::None);

        // Clock through delay - half frame happens when write is applied (3 cycle delay for even)
        assert_eq!(fc.clock(), FrameAction::None); // delay 2 remaining
        assert_eq!(fc.clock(), FrameAction::None); // delay 1 remaining
        assert_eq!(fc.clock(), FrameAction::HalfFrame); // write applied with half frame
    }

    #[test]
    fn test_write_delay_odd_cycle() {
        let mut fc = FrameCounter::new();

        // Write with odd CPU cycle should have 4 cycle delay
        fc.write_control(0x80, true);

        // Clock through the 4-cycle delay
        assert_eq!(fc.clock(), FrameAction::None); // delay 3 remaining
        assert_eq!(fc.clock(), FrameAction::None); // delay 2 remaining
        assert_eq!(fc.clock(), FrameAction::None); // delay 1 remaining
        assert_eq!(fc.clock(), FrameAction::HalfFrame); // write applied

        assert_eq!(fc.mode, 1);
    }
}
