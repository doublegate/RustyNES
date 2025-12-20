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
pub struct FrameCounter {
    /// Frame counter mode: 0 = 4-step (60 Hz), 1 = 5-step (48 Hz)
    mode: u8,
    /// IRQ inhibit flag (from $4017 bit 6)
    irq_inhibit: bool,
    /// Current cycle count within the frame
    cycle_count: u64,
    /// Frame IRQ flag (read by CPU)
    pub irq_flag: bool,
}

impl FrameCounter {
    /// Creates a new frame counter in 4-step mode
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mode: 0,
            irq_inhibit: false,
            cycle_count: 0,
            irq_flag: false,
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
    /// Writing to this register:
    /// - Resets the cycle counter to 0
    /// - Clears IRQ flag if IRQ inhibit is set
    /// - If 5-step mode, immediately triggers half-frame action
    pub fn write_control(&mut self, value: u8) -> FrameAction {
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
        self.cycle_count += 1;

        match self.mode {
            0 => self.clock_4step(),
            1 => self.clock_5step(),
            _ => FrameAction::None,
        }
    }

    /// Clocks the 4-step mode sequencer
    ///
    /// # Sequence
    ///
    /// ```text
    /// Step   Cycles  Action
    /// ----   ------  ------
    /// 1      7457    Quarter frame (envelopes + linear counter)
    /// 2      14913   Half frame (envelopes, linear, length, sweep)
    /// 3      22372   Quarter frame (was 22371, corrected per NESdev research)
    /// 4      29829   Half frame + set IRQ flag (if enabled)
    ///        29830   Set IRQ flag
    ///        29831   Set IRQ flag, reset to 0
    /// ```
    ///
    /// # Timing Note
    ///
    /// APU frame counter ticks at 7456.5, 14912.5, 22371.5, 29829.5 APU cycles
    /// (which are half CPU cycles), so CPU sees them at 7457, 14913, 22372, 29830.
    /// The sequence was refined based on hardware testing documented on `NESdev`.
    fn clock_4step(&mut self) -> FrameAction {
        match self.cycle_count {
            // Quarter frames at 7457 and 22372 (corrected from 22371)
            7457 | 22372 => FrameAction::QuarterFrame,
            14913 => FrameAction::HalfFrame,
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
                FrameAction::None
            }
            29831 => {
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
    /// # Sequence
    ///
    /// ```text
    /// Step   Cycles  Action
    /// ----   ------  ------
    /// 1      7457    Quarter frame
    /// 2      14913   Half frame
    /// 3      22371   Quarter frame
    /// 4      29829   (nothing)
    /// 5      37281   Half frame, reset to 0
    /// ```
    fn clock_5step(&mut self) -> FrameAction {
        match self.cycle_count {
            7457 | 22371 => FrameAction::QuarterFrame,
            14913 => FrameAction::HalfFrame,
            37281 => {
                self.cycle_count = 0;
                FrameAction::HalfFrame
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

        // Clock to second quarter frame (cycle 22372, corrected from 22371)
        for _ in 0..7458 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to second half frame with IRQ (cycle 29829)
        for _ in 0..7456 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert!(!fc.irq_flag);
        assert_eq!(fc.clock(), FrameAction::HalfFrame);
        assert!(fc.irq_flag);

        // Additional IRQ flag sets (cycles 29830, 29831)
        assert_eq!(fc.clock(), FrameAction::None);
        assert!(fc.irq_flag);
        assert_eq!(fc.clock(), FrameAction::None);
        assert!(fc.irq_flag);

        // Should reset to 0
        assert_eq!(fc.cycle_count, 0);
    }

    #[test]
    fn test_frame_counter_5step_sequence() {
        let mut fc = FrameCounter::new();
        fc.write_control(0x80); // 5-step mode

        assert_eq!(fc.mode, 1);
        assert_eq!(fc.cycle_count, 0);

        // Clock to first quarter frame
        for _ in 0..7456 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to first half frame
        for _ in 0..7455 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::HalfFrame);

        // Clock to second quarter frame
        for _ in 0..7457 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::QuarterFrame);

        // Clock to cycle 29829 (no action in 5-step mode)
        for _ in 0..7457 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::None);

        // Clock to final half frame
        for _ in 0..7451 {
            assert_eq!(fc.clock(), FrameAction::None);
        }
        assert_eq!(fc.clock(), FrameAction::HalfFrame);

        // Should reset
        assert_eq!(fc.cycle_count, 0);
        // 5-step mode never sets IRQ
        assert!(!fc.irq_flag);
    }

    #[test]
    fn test_irq_inhibit() {
        let mut fc = FrameCounter::new();

        // Enable IRQ inhibit
        fc.write_control(0x40);
        assert!(fc.irq_inhibit);

        // Clock to IRQ point
        for _ in 0..29829 {
            fc.clock();
        }

        // IRQ should not be set
        assert!(!fc.irq_flag);
        assert!(!fc.irq_pending());
    }

    #[test]
    fn test_write_control_clears_irq() {
        let mut fc = FrameCounter::new();
        fc.irq_flag = true;

        // Write with IRQ inhibit
        fc.write_control(0x40);

        assert!(!fc.irq_flag);
    }

    #[test]
    fn test_5step_immediate_half_frame() {
        let mut fc = FrameCounter::new();

        // Writing 5-step mode should immediately trigger half frame
        let action = fc.write_control(0x80);
        assert_eq!(action, FrameAction::HalfFrame);
    }
}
