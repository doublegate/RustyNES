//! APU Frame Counter.
//!
//! The frame counter is responsible for clocking the envelope, length counter,
//! and sweep units at specific cycle intervals. It operates in two modes:
//!
//! - 4-step mode: Generates quarter frame signals at cycles 3728.5, 7456.5,
//!   11185.5, 14914.5, and can optionally trigger an IRQ.
//!
//! - 5-step mode: Generates quarter frame signals at cycles 3728.5, 7456.5,
//!   11185.5, 14914.5, 18640.5. Does not generate IRQ.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Frame counter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FrameCounterMode {
    /// 4-step mode (NTSC: 14915 cycles per frame).
    #[default]
    FourStep,
    /// 5-step mode (NTSC: 18641 cycles per frame).
    FiveStep,
}

/// Frame counter events that occur on specific cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameEvent {
    /// Quarter frame (clock envelope and linear counter).
    QuarterFrame,
    /// Half frame (clock length counter and sweep).
    HalfFrame,
    /// IRQ (only in 4-step mode with IRQ enabled).
    Irq,
}

/// Frame counter.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FrameCounter {
    /// Current cycle within the frame.
    cycle: u16,
    /// Frame counter mode.
    mode: FrameCounterMode,
    /// IRQ inhibit flag.
    irq_inhibit: bool,
    /// IRQ pending flag.
    irq_pending: bool,
    /// Reset delay (cycles until mode change takes effect).
    reset_delay: u8,
    /// Pending mode to set after reset delay.
    pending_mode: Option<FrameCounterMode>,
}

/// 4-step mode cycle points (NTSC).
const FOUR_STEP_CYCLES: [u16; 5] = [7457, 14913, 22371, 29828, 29829];

/// 5-step mode cycle points (NTSC).
const FIVE_STEP_CYCLES: [u16; 5] = [7457, 14913, 22371, 29829, 37281];

impl FrameCounter {
    /// Create a new frame counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cycle: 0,
            mode: FrameCounterMode::FourStep,
            irq_inhibit: false,
            irq_pending: false,
            reset_delay: 0,
            pending_mode: None,
        }
    }

    /// Write to the frame counter register ($4017).
    /// Bits: MI-- ----
    /// - M: Mode (0 = 4-step, 1 = 5-step)
    /// - I: IRQ inhibit
    pub fn write(&mut self, value: u8) {
        self.irq_inhibit = value & 0x40 != 0;

        if self.irq_inhibit {
            self.irq_pending = false;
        }

        let new_mode = if value & 0x80 != 0 {
            FrameCounterMode::FiveStep
        } else {
            FrameCounterMode::FourStep
        };

        // Mode change takes effect after a delay
        self.pending_mode = Some(new_mode);
        self.reset_delay = if self.cycle.is_multiple_of(2) { 4 } else { 3 };
    }

    /// Clock the frame counter. Returns any events that occurred.
    pub fn clock(&mut self) -> [Option<FrameEvent>; 3] {
        let mut events = [None; 3];
        let mut event_idx = 0;

        // Handle pending mode change
        if self.reset_delay > 0 {
            self.reset_delay -= 1;
            if self.reset_delay == 0
                && let Some(mode) = self.pending_mode.take()
            {
                self.mode = mode;
                self.cycle = 0;

                // 5-step mode immediately clocks half frame on mode set
                if self.mode == FrameCounterMode::FiveStep {
                    events[event_idx] = Some(FrameEvent::QuarterFrame);
                    event_idx += 1;
                    events[event_idx] = Some(FrameEvent::HalfFrame);
                    return events;
                }
            }
        }

        self.cycle += 1;

        match self.mode {
            FrameCounterMode::FourStep => {
                self.clock_four_step(&mut events);
            }
            FrameCounterMode::FiveStep => {
                self.clock_five_step(&mut events);
            }
        }

        events
    }

    /// Clock in 4-step mode.
    fn clock_four_step(&mut self, events: &mut [Option<FrameEvent>; 3]) {
        match self.cycle {
            c if c == FOUR_STEP_CYCLES[0] => {
                events[0] = Some(FrameEvent::QuarterFrame);
            }
            c if c == FOUR_STEP_CYCLES[1] => {
                events[0] = Some(FrameEvent::QuarterFrame);
                events[1] = Some(FrameEvent::HalfFrame);
            }
            c if c == FOUR_STEP_CYCLES[2] => {
                events[0] = Some(FrameEvent::QuarterFrame);
            }
            c if c == FOUR_STEP_CYCLES[3] => {
                // Set IRQ flag
                if !self.irq_inhibit {
                    self.irq_pending = true;
                    events[0] = Some(FrameEvent::Irq);
                }
            }
            c if c == FOUR_STEP_CYCLES[4] => {
                events[0] = Some(FrameEvent::QuarterFrame);
                events[1] = Some(FrameEvent::HalfFrame);
                if !self.irq_inhibit {
                    self.irq_pending = true;
                    events[2] = Some(FrameEvent::Irq);
                }
                // Frame complete, reset
                self.cycle = 0;
            }
            _ => {}
        }
    }

    /// Clock in 5-step mode.
    fn clock_five_step(&mut self, events: &mut [Option<FrameEvent>; 3]) {
        match self.cycle {
            c if c == FIVE_STEP_CYCLES[0] => {
                events[0] = Some(FrameEvent::QuarterFrame);
            }
            c if c == FIVE_STEP_CYCLES[1] => {
                events[0] = Some(FrameEvent::QuarterFrame);
                events[1] = Some(FrameEvent::HalfFrame);
            }
            c if c == FIVE_STEP_CYCLES[2] => {
                events[0] = Some(FrameEvent::QuarterFrame);
            }
            c if c == FIVE_STEP_CYCLES[3] => {
                // Nothing happens at step 4 in 5-step mode
            }
            c if c == FIVE_STEP_CYCLES[4] => {
                events[0] = Some(FrameEvent::QuarterFrame);
                events[1] = Some(FrameEvent::HalfFrame);
                // Frame complete, reset
                self.cycle = 0;
            }
            _ => {}
        }
    }

    /// Check if an IRQ is pending.
    #[must_use]
    pub fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    /// Clear the IRQ pending flag (called when status is read).
    pub fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    /// Get the current cycle.
    #[must_use]
    pub fn cycle(&self) -> u16 {
        self.cycle
    }

    /// Get the current mode.
    #[must_use]
    pub fn mode(&self) -> FrameCounterMode {
        self.mode
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
    fn test_frame_counter_initial() {
        let fc = FrameCounter::new();
        assert_eq!(fc.mode(), FrameCounterMode::FourStep);
        assert!(!fc.irq_pending());
    }

    #[test]
    fn test_four_step_quarter_frame() {
        let mut fc = FrameCounter::new();

        // Clock until first quarter frame
        for _ in 0..FOUR_STEP_CYCLES[0] {
            let events = fc.clock();
            if fc.cycle == FOUR_STEP_CYCLES[0] {
                assert!(events.contains(&Some(FrameEvent::QuarterFrame)));
            }
        }
    }

    #[test]
    fn test_four_step_irq() {
        let mut fc = FrameCounter::new();
        fc.write(0x00); // 4-step mode, IRQ enabled

        // Wait for mode to take effect
        for _ in 0..10 {
            fc.clock();
        }

        // Clock until IRQ
        while fc.cycle < FOUR_STEP_CYCLES[3] - 1 {
            fc.clock();
        }
        fc.clock();
        assert!(fc.irq_pending());
    }

    #[test]
    fn test_irq_inhibit() {
        let mut fc = FrameCounter::new();
        fc.write(0x40); // 4-step mode, IRQ inhibit

        // Wait for mode to take effect
        for _ in 0..10 {
            fc.clock();
        }

        // Clock past IRQ point
        for _ in 0..30000 {
            fc.clock();
        }
        assert!(!fc.irq_pending());
    }

    #[test]
    fn test_five_step_mode() {
        let mut fc = FrameCounter::new();
        fc.write(0x80); // 5-step mode

        // Wait for mode to take effect
        for _ in 0..10 {
            fc.clock();
        }

        assert_eq!(fc.mode(), FrameCounterMode::FiveStep);
    }

    #[test]
    fn test_five_step_no_irq() {
        let mut fc = FrameCounter::new();
        fc.write(0x80); // 5-step mode

        // Clock for a full frame
        for _ in 0..40000 {
            fc.clock();
        }
        assert!(!fc.irq_pending());
    }

    #[test]
    fn test_clear_irq() {
        let mut fc = FrameCounter::new();
        fc.irq_pending = true;
        fc.clear_irq();
        assert!(!fc.irq_pending());
    }
}
