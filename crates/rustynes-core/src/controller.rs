//! Standard NES controller (4016/4017) shift-register state.
//!
//! Per <https://www.nesdev.org/wiki/Standard_controller>:
//!
//! - Writing `$4016` with bit 0 set holds the controllers in *strobe* mode:
//!   the shift register is continuously reloaded with the current button
//!   state, and a read of `$4016` / `$4017` returns the state of the **A**
//!   button (the LSB of the latch).
//! - Writing `$4016` with bit 0 clear takes the controllers out of strobe
//!   mode; the latched button state remains in the shift register and is
//!   shifted out one bit per `$4016`/`$4017` read in the order
//!   `A, B, Select, Start, Up, Down, Left, Right`.
//! - After all eight buttons have been read, subsequent reads return `1`
//!   (open-bus + a stuck-high data line on the standard pad).
//!
//! Frontends update the *current* button state via
//! [`Controller::set_buttons`]; the bus latches that state into the shift
//! register on the rising edge of strobe (and continuously while strobe is
//! held high).

use bitflags::bitflags;

bitflags! {
    /// Standard NES controller buttons. Bits ordered to match the wire
    /// shift order (LSB first): A, B, Select, Start, Up, Down, Left, Right.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct Buttons: u8 {
        /// A button.
        const A      = 1 << 0;
        /// B button.
        const B      = 1 << 1;
        /// Select button.
        const SELECT = 1 << 2;
        /// Start button.
        const START  = 1 << 3;
        /// D-pad up.
        const UP     = 1 << 4;
        /// D-pad down.
        const DOWN   = 1 << 5;
        /// D-pad left.
        const LEFT   = 1 << 6;
        /// D-pad right.
        const RIGHT  = 1 << 7;
    }
}

/// One standard NES controller plugged into `$4016` (player 1) or `$4017`
/// (player 2).
#[derive(Clone, Copy, Debug, Default)]
pub struct Controller {
    /// Current button state — set externally by the frontend.
    pub(crate) buttons: Buttons,
    /// Latched shift register: shifted right on each read.
    pub(crate) shift: u8,
    /// Strobe state (last bit-0 written to `$4016`).
    pub(crate) strobe: bool,
}

impl Controller {
    /// New controller with no buttons pressed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buttons: Buttons::empty(),
            shift: 0,
            strobe: false,
        }
    }

    /// Set the current button state. Takes effect on the next strobe edge
    /// (or immediately, while strobe is held high).
    pub const fn set_buttons(&mut self, buttons: Buttons) {
        self.buttons = buttons;
        if self.strobe {
            self.shift = buttons.bits();
        }
    }

    /// Get the current button state.
    #[must_use]
    pub const fn buttons(&self) -> Buttons {
        self.buttons
    }

    /// Handle a write to `$4016`. Only bit 0 matters for the standard
    /// controller. While strobe is held high the shift register continuously
    /// reloads from the live button state.
    pub const fn write_strobe(&mut self, value: u8) {
        let new_strobe = value & 1 != 0;
        // Falling edge latches: while strobe was high, shift mirrors live
        // buttons; on the falling edge, that snapshot becomes the value
        // shifted out by subsequent reads.
        if new_strobe {
            self.shift = self.buttons.bits();
        }
        self.strobe = new_strobe;
    }

    /// Handle a read of `$4016` / `$4017`. Returns the LSB of the shift
    /// register and shifts. While strobe is held high, the LSB is always
    /// the A button (bit 0 of `buttons`).
    ///
    /// Per the wiki, when the shift register has been emptied subsequent
    /// reads return 1.
    pub const fn read(&mut self) -> u8 {
        if self.strobe {
            self.buttons.bits() & 1
        } else {
            let bit = self.shift & 1;
            // Shift in 1s from the left so post-empty reads yield 1.
            self.shift = (self.shift >> 1) | 0x80;
            bit
        }
    }

    /// Side-effect-free sample of the next bit (debugger).
    #[must_use]
    pub const fn peek(&self) -> u8 {
        if self.strobe {
            self.buttons.bits() & 1
        } else {
            self.shift & 1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_controller_reads_zero_then_ones() {
        let mut c = Controller::new();
        // Pulse strobe high then low to load.
        c.write_strobe(1);
        c.write_strobe(0);
        for _ in 0..8 {
            assert_eq!(c.read(), 0);
        }
        // After 8 reads, ROMs see 1s.
        for _ in 0..4 {
            assert_eq!(c.read(), 1);
        }
    }

    #[test]
    fn each_button_appears_in_canonical_shift_order() {
        let mut c = Controller::new();
        c.set_buttons(Buttons::A | Buttons::SELECT | Buttons::DOWN);
        c.write_strobe(1);
        c.write_strobe(0);
        // A, B, Select, Start, Up, Down, Left, Right
        let expected = [1u8, 0, 1, 0, 0, 1, 0, 0];
        for &want in &expected {
            assert_eq!(c.read(), want);
        }
    }

    #[test]
    fn strobe_high_reads_a_button_repeatedly() {
        let mut c = Controller::new();
        c.set_buttons(Buttons::A);
        c.write_strobe(1);
        for _ in 0..16 {
            assert_eq!(c.read(), 1, "while strobing, $4016 returns A bit");
        }
    }

    #[test]
    fn buttons_set_during_strobe_reflect_immediately() {
        let mut c = Controller::new();
        c.write_strobe(1);
        c.set_buttons(Buttons::A);
        assert_eq!(c.read(), 1);
        c.set_buttons(Buttons::empty());
        assert_eq!(c.read(), 0);
    }

    #[test]
    fn buttons_set_after_latch_take_effect_on_next_strobe() {
        let mut c = Controller::new();
        c.set_buttons(Buttons::A);
        c.write_strobe(1);
        c.write_strobe(0);
        // Change buttons mid-readout — should NOT affect this scan.
        c.set_buttons(Buttons::A | Buttons::B);
        assert_eq!(c.read(), 1, "A");
        assert_eq!(c.read(), 0, "B (latched as not pressed)");
        // New strobe latches the new state.
        c.write_strobe(1);
        c.write_strobe(0);
        assert_eq!(c.read(), 1, "A");
        assert_eq!(c.read(), 1, "B (now latched as pressed)");
    }
}
