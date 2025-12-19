//! NES standard controller implementation.

/// NES controller buttons
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Button {
    /// A button
    A = 0b0000_0001,
    /// B button
    B = 0b0000_0010,
    /// Select button
    Select = 0b0000_0100,
    /// Start button
    Start = 0b0000_1000,
    /// D-pad Up
    Up = 0b0001_0000,
    /// D-pad Down
    Down = 0b0010_0000,
    /// D-pad Left
    Left = 0b0100_0000,
    /// D-pad Right
    Right = 0b1000_0000,
}

/// NES standard controller (8 buttons)
#[derive(Debug, Clone)]
pub struct Controller {
    /// Current button states (bit field)
    buttons: u8,

    /// Shift register for serial reads
    shift_register: u8,

    /// Strobe state (true = latching, false = serial mode)
    strobe: bool,

    /// Current bit index (0-8, then 8+ returns 1)
    bit_index: u8,
}

impl Controller {
    /// Create new controller (all buttons released)
    #[must_use]
    pub fn new() -> Self {
        Self {
            buttons: 0,
            shift_register: 0,
            strobe: false,
            bit_index: 0,
        }
    }

    /// Set button state
    ///
    /// # Arguments
    ///
    /// * `button` - Button to set
    /// * `pressed` - true if pressed, false if released
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.buttons |= button as u8;
        } else {
            self.buttons &= !(button as u8);
        }
    }

    /// Set all button states at once
    ///
    /// # Arguments
    ///
    /// * `buttons` - 8-bit field where each bit represents a button (see Button enum)
    pub fn set_buttons(&mut self, buttons: u8) {
        self.buttons = buttons;
    }

    /// Get current button state
    ///
    /// # Arguments
    ///
    /// * `button` - Button to check
    ///
    /// # Returns
    ///
    /// true if pressed, false if released
    #[must_use]
    pub fn get_button(&self, button: Button) -> bool {
        (self.buttons & (button as u8)) != 0
    }

    /// Get all button states
    ///
    /// # Returns
    ///
    /// 8-bit field where each bit represents a button (see Button enum)
    #[must_use]
    pub fn buttons(&self) -> u8 {
        self.buttons
    }

    /// Write to strobe register ($4016)
    ///
    /// Bit 0: Strobe (1 = latch buttons, 0 = serial mode)
    ///
    /// Falling edge (1 → 0) latches current button states into shift register.
    pub fn write_strobe(&mut self, value: u8) {
        let new_strobe = (value & 0x01) != 0;

        // Detect falling edge: strobe goes from 1 → 0
        if self.strobe && !new_strobe {
            // Latch current button states into shift register
            self.shift_register = self.buttons;
            self.bit_index = 0;
        }

        self.strobe = new_strobe;
    }

    /// Read from controller data register ($4016 or $4017)
    ///
    /// Returns:
    /// - Bit 0: Current button bit (1 = pressed, 0 = not pressed)
    /// - Bits 1-4: Expansion port data (unused, varies)
    /// - Bits 5-7: Open bus (typically $40)
    ///
    /// Reading sequence (after strobe):
    /// 1. A
    /// 2. B
    /// 3. Select
    /// 4. Start
    /// 5. Up
    /// 6. Down
    /// 7. Left
    /// 8. Right
    ///    9+: Always 1
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            // While strobing, always return A button state
            // (hardware continuously reloads shift register)
            return (self.buttons & 0x01) | 0x40;
        }

        // Read current bit from shift register
        let bit = if self.bit_index < 8 {
            (self.shift_register >> self.bit_index) & 0x01
        } else {
            // Bits 8+ always return 1 (open bus behavior)
            1
        };

        // Advance to next bit
        self.bit_index = self.bit_index.saturating_add(1);

        // Return bit 0 + open bus $40
        bit | 0x40
    }

    /// Power-on reset
    pub fn reset(&mut self) {
        self.buttons = 0;
        self.shift_register = 0;
        self.strobe = false;
        self.bit_index = 0;
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_state() {
        let mut controller = Controller::new();

        // Initially all buttons released
        assert_eq!(controller.buttons(), 0);

        // Press A
        controller.set_button(Button::A, true);
        assert!(controller.get_button(Button::A));
        assert_eq!(controller.buttons(), 0b0000_0001);

        // Press Start
        controller.set_button(Button::Start, true);
        assert!(controller.get_button(Button::Start));
        assert_eq!(controller.buttons(), 0b0000_1001);

        // Release A
        controller.set_button(Button::A, false);
        assert!(!controller.get_button(Button::A));
        assert_eq!(controller.buttons(), 0b0000_1000);
    }

    #[test]
    fn test_strobe_latch() {
        let mut controller = Controller::new();

        // Press A and Start
        controller.set_button(Button::A, true);
        controller.set_button(Button::Start, true);

        // Strobe high
        controller.write_strobe(0x01);
        assert!(controller.strobe);

        // Strobe low (falling edge = latch)
        controller.write_strobe(0x00);
        assert!(!controller.strobe);

        // Shift register should now contain button state
        assert_eq!(controller.shift_register, 0b0000_1001);
        assert_eq!(controller.bit_index, 0);
    }

    #[test]
    fn test_serial_read_sequence() {
        let mut controller = Controller::new();

        // Press A, Select, Down, Right
        controller.set_button(Button::A, true);
        controller.set_button(Button::Select, true);
        controller.set_button(Button::Down, true);
        controller.set_button(Button::Right, true);
        // buttons = 0b1010_0101

        // Strobe
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);

        // Read 8 buttons in order
        let reads = [
            controller.read() & 0x01, // A      = 1
            controller.read() & 0x01, // B      = 0
            controller.read() & 0x01, // Select = 1
            controller.read() & 0x01, // Start  = 0
            controller.read() & 0x01, // Up     = 0
            controller.read() & 0x01, // Down   = 1
            controller.read() & 0x01, // Left   = 0
            controller.read() & 0x01, // Right  = 1
        ];

        assert_eq!(reads, [1, 0, 1, 0, 0, 1, 0, 1]);
    }

    #[test]
    fn test_continuous_strobe() {
        let mut controller = Controller::new();

        controller.set_button(Button::A, true);
        controller.set_button(Button::B, true);

        // Set strobe high
        controller.write_strobe(0x01);

        // While strobing, reads always return A button
        for _ in 0..10 {
            let value = controller.read();
            assert_eq!(value & 0x01, 1); // A is pressed
        }
    }

    #[test]
    fn test_open_bus_behavior() {
        let mut controller = Controller::new();

        // Strobe and latch
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);

        // Read 8 buttons
        for _ in 0..8 {
            let value = controller.read();
            assert_eq!(value & 0x40, 0x40); // Open bus $40
        }

        // Read bits 9-15 (should return 1 + open bus)
        for _ in 0..7 {
            let value = controller.read();
            assert_eq!(value, 0x41); // Bit 0 = 1, open bus $40
        }
    }

    #[test]
    fn test_strobe_edge_detection() {
        let mut controller = Controller::new();

        controller.set_button(Button::A, true);

        // Multiple high writes don't latch
        controller.write_strobe(0x01);
        controller.write_strobe(0x01);
        controller.write_strobe(0x01);
        assert_eq!(controller.bit_index, 0);
        assert_eq!(controller.shift_register, 0); // Not latched yet

        // Falling edge latches
        controller.write_strobe(0x00);
        assert_eq!(controller.shift_register, 0b0000_0001);

        // Multiple low writes don't re-latch
        let prev = controller.shift_register;
        controller.write_strobe(0x00);
        controller.write_strobe(0x00);
        assert_eq!(controller.shift_register, prev);
    }

    #[test]
    fn test_reset() {
        let mut controller = Controller::new();

        // Set some state
        controller.set_button(Button::A, true);
        controller.write_strobe(0x01);
        controller.write_strobe(0x00);
        controller.read();

        // Reset
        controller.reset();

        // All state cleared
        assert_eq!(controller.buttons, 0);
        assert_eq!(controller.shift_register, 0);
        assert!(!controller.strobe);
        assert_eq!(controller.bit_index, 0);
    }
}
