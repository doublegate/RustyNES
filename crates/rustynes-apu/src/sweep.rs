//! Sweep Unit Module
//!
//! The sweep unit modulates the frequency of pulse channels by automatically
//! adjusting the timer period. Used for pitch bending effects.

/// Sweep unit for pulse channel frequency modulation
///
/// The sweep unit can automatically increase or decrease the pulse timer period,
/// creating pitch bend effects. Each pulse channel has its own sweep unit.
///
/// # Differences Between Channels
///
/// - **Pulse 1**: Uses one's complement for negation (subtract change + 1)
/// - **Pulse 2**: Uses two's complement for negation (subtract change only)
pub struct Sweep {
    /// Sweep enabled flag
    enabled: bool,
    /// Negate flag: 0 = add to period, 1 = subtract from period
    negate: bool,
    /// Shift count (0-7): determines change amount
    shift: u8,
    /// Divider period (0-7): how often sweep updates occur
    period: u8,
    /// Divider counter
    divider: u8,
    /// Reload flag: set when register is written
    reload_flag: bool,
    /// Channel ID: 0 = Pulse 1, 1 = Pulse 2 (affects negation behavior)
    channel: u8,
}

impl Sweep {
    /// Creates a new sweep unit for the specified channel
    ///
    /// # Arguments
    ///
    /// * `channel` - Channel ID (0 = Pulse 1, 1 = Pulse 2)
    #[must_use]
    pub const fn new(channel: u8) -> Self {
        Self {
            enabled: false,
            negate: false,
            shift: 0,
            period: 0,
            divider: 0,
            reload_flag: false,
            channel,
        }
    }

    /// Writes to the sweep control register
    ///
    /// # Register Format
    ///
    /// ```text
    /// 7  bit  0
    /// ---- ----
    /// EPPP NSSS
    /// |||| ||||
    /// |||| |+++- Shift count (S)
    /// |||| +---- Negate flag (0: add, 1: subtract)
    /// |+++------ Divider period (P)
    /// +--------- Enabled flag
    /// ```
    pub fn write_register(&mut self, value: u8) {
        self.enabled = (value & 0x80) != 0;
        self.period = (value >> 4) & 0x07;
        self.negate = (value & 0x08) != 0;
        self.shift = value & 0x07;
        self.reload_flag = true;
    }

    /// Clocks the sweep unit (called on half frame)
    ///
    /// Updates the timer period if conditions are met.
    ///
    /// # Arguments
    ///
    /// * `timer` - Mutable reference to the channel's timer period
    pub fn clock(&mut self, timer: &mut u16) {
        // Update timer if divider reached 0, sweep enabled, shift non-zero, not muted, and not reloading
        // When reload_flag is set, we reload the divider but don't update the timer
        if self.divider == 0
            && !self.reload_flag
            && self.enabled
            && self.shift != 0
            && !self.is_muted(*timer)
        {
            *timer = self.target_period(*timer);
        }

        // Clock divider
        if self.divider == 0 || self.reload_flag {
            self.divider = self.period;
            self.reload_flag = false;
        } else {
            self.divider -= 1;
        }
    }

    /// Calculates the target period after sweep adjustment
    ///
    /// # Sweep Calculation
    ///
    /// - Change amount = timer >> shift
    /// - If negate:
    ///   - Pulse 1: timer - change - 1 (one's complement)
    ///   - Pulse 2: timer - change (two's complement)
    /// - Otherwise: timer + change
    ///
    /// # Arguments
    ///
    /// * `timer` - Current timer period
    ///
    /// # Returns
    ///
    /// The new timer period after sweep adjustment
    fn target_period(&self, timer: u16) -> u16 {
        let change = timer >> self.shift;

        if self.negate {
            // One's complement for Pulse 1, two's complement for Pulse 2
            if self.channel == 0 {
                timer.wrapping_sub(change).wrapping_sub(1)
            } else {
                timer.wrapping_sub(change)
            }
        } else {
            timer.wrapping_add(change)
        }
    }

    /// Returns whether the sweep unit is muting the channel
    ///
    /// A pulse channel is muted if:
    /// - Timer period < 8, OR
    /// - Target period > $7FF (2047)
    ///
    /// # Arguments
    ///
    /// * `timer` - Current timer period
    #[must_use]
    pub fn is_muted(&self, timer: u16) -> bool {
        timer < 8 || self.target_period(timer) > 0x7FF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sweep_register_write() {
        let mut sweep = Sweep::new(0);

        // Write: enabled, period=3, negate, shift=4
        sweep.write_register(0xBC);

        assert!(sweep.enabled);
        assert_eq!(sweep.period, 3);
        assert!(sweep.negate);
        assert_eq!(sweep.shift, 4);
        assert!(sweep.reload_flag);
    }

    #[test]
    fn test_sweep_target_period_add() {
        // Create sweep with shift = 1, no negate
        let mut sweep = Sweep::new(0);
        sweep.write_register(0x81); // Enabled, period=0, no negate, shift=1

        // Timer = 100, change = 100 >> 1 = 50
        let target = sweep.target_period(100);
        assert_eq!(target, 150); // 100 + 50
    }

    #[test]
    fn test_sweep_target_period_subtract_pulse1() {
        let mut sweep = Sweep::new(0); // Pulse 1
        sweep.write_register(0x89); // Enabled, period=0, negate, shift=1

        // Timer = 200, change = 200 >> 1 = 100
        // Pulse 1 uses one's complement: 200 - 100 - 1 = 99
        let target = sweep.target_period(200);
        assert_eq!(target, 99);
    }

    #[test]
    fn test_sweep_target_period_subtract_pulse2() {
        let mut sweep = Sweep::new(1); // Pulse 2
        sweep.write_register(0x89); // Enabled, period=0, negate, shift=1

        // Timer = 200, change = 200 >> 1 = 100
        // Pulse 2 uses two's complement: 200 - 100 = 100
        let target = sweep.target_period(200);
        assert_eq!(target, 100);
    }

    #[test]
    fn test_sweep_muting_low_timer() {
        let sweep = Sweep::new(0);

        // Timer < 8 should mute
        assert!(sweep.is_muted(7));
        assert!(sweep.is_muted(0));

        // Timer >= 8 should not mute (if target also valid)
        assert!(!sweep.is_muted(8));
        assert!(!sweep.is_muted(100));
    }

    #[test]
    fn test_sweep_muting_high_target() {
        let mut sweep = Sweep::new(0);

        // Configure sweep to add
        sweep.write_register(0x80); // Enabled, period=0, no negate, shift=0

        // Timer = 2000, shift = 0, change = 2000, target = 4000 (muted because > $7FF)
        sweep.shift = 0;
        assert!(sweep.is_muted(2000));

        // Timer = 1024, shift = 0, change = 1024, target = 2048 (muted because > $7FF)
        // Note: 0x7FF = 2047, so 2048 > 2047 means muted
        sweep.shift = 0;
        assert!(sweep.is_muted(1024));

        // Timer = 100, shift = 1, change = 50, target = 150 (not muted)
        sweep.shift = 1;
        assert!(!sweep.is_muted(100));
    }

    #[test]
    fn test_sweep_clock_updates_timer() {
        let mut sweep = Sweep::new(0);
        let mut timer: u16 = 100;

        // Configure sweep: enabled, period=0, no negate, shift=1
        sweep.write_register(0x81);
        assert!(sweep.reload_flag);

        // First clock loads divider from period
        sweep.clock(&mut timer);
        assert_eq!(timer, 100); // Not updated yet (divider just loaded)
        assert_eq!(sweep.divider, 0);
        assert!(!sweep.reload_flag);

        // Second clock should update timer
        sweep.clock(&mut timer);
        assert_eq!(timer, 150); // 100 + (100 >> 1) = 150
    }

    #[test]
    fn test_sweep_clock_divider() {
        let mut sweep = Sweep::new(0);
        let mut timer: u16 = 100;

        // Configure sweep: enabled, period=3, no negate, shift=1
        sweep.write_register(0xB1);

        // First clock reloads divider
        sweep.clock(&mut timer);
        assert_eq!(sweep.divider, 3);

        // Clock 3 more times
        sweep.clock(&mut timer);
        assert_eq!(sweep.divider, 2);

        sweep.clock(&mut timer);
        assert_eq!(sweep.divider, 1);

        sweep.clock(&mut timer);
        assert_eq!(sweep.divider, 0);

        // Next clock should reload and update timer
        let prev_timer = timer;
        sweep.clock(&mut timer);
        assert_eq!(sweep.divider, 3);
        assert_ne!(timer, prev_timer); // Timer should have updated
    }

    #[test]
    fn test_sweep_disabled_no_update() {
        let mut sweep = Sweep::new(0);
        let mut timer: u16 = 100;

        // Configure sweep: disabled, shift=1
        sweep.write_register(0x01);
        assert!(!sweep.enabled);

        // Clock multiple times
        for _ in 0..10 {
            sweep.clock(&mut timer);
        }

        // Timer should not have changed
        assert_eq!(timer, 100);
    }

    #[test]
    fn test_sweep_zero_shift_no_update() {
        let mut sweep = Sweep::new(0);
        let mut timer: u16 = 100;

        // Configure sweep: enabled, period=0, shift=0
        sweep.write_register(0x80);
        assert_eq!(sweep.shift, 0);

        // Clock multiple times
        for _ in 0..10 {
            sweep.clock(&mut timer);
        }

        // Timer should not have changed (shift = 0 means no update)
        assert_eq!(timer, 100);
    }
}
