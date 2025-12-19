//! Input handling for keyboard and gamepad.
//!
//! This module provides input state management compatible with the Elm architecture,
//! supporting both keyboard and gamepad inputs for NES controller emulation.

pub mod gamepad;
pub mod keyboard;

use rustynes_core::Button as NesButton;

/// Combined input state for both players
#[derive(Debug, Clone, Default)]
pub struct InputState {
    /// Player 1 controller state
    pub player1: ControllerState,
    /// Player 2 controller state
    pub player2: ControllerState,
}

/// State of a single NES controller (8 buttons)
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)] // NES controller has 8 discrete buttons
pub struct ControllerState {
    /// A button state
    pub a: bool,
    /// B button state
    pub b: bool,
    /// Select button state
    pub select: bool,
    /// Start button state
    pub start: bool,
    /// D-pad Up state
    pub up: bool,
    /// D-pad Down state
    pub down: bool,
    /// D-pad Left state
    pub left: bool,
    /// D-pad Right state
    pub right: bool,
}

impl ControllerState {
    /// Create new controller with all buttons released
    #[must_use]
    #[allow(dead_code)] // Future: manual controller state construction in tests
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a button is pressed
    #[must_use]
    pub fn is_pressed(&self, button: NesButton) -> bool {
        match button {
            NesButton::A => self.a,
            NesButton::B => self.b,
            NesButton::Select => self.select,
            NesButton::Start => self.start,
            NesButton::Up => self.up,
            NesButton::Down => self.down,
            NesButton::Left => self.left,
            NesButton::Right => self.right,
        }
    }

    /// Set button state
    pub fn set(&mut self, button: NesButton, pressed: bool) {
        match button {
            NesButton::A => self.a = pressed,
            NesButton::B => self.b = pressed,
            NesButton::Select => self.select = pressed,
            NesButton::Start => self.start = pressed,
            NesButton::Up => self.up = pressed,
            NesButton::Down => self.down = pressed,
            NesButton::Left => self.left = pressed,
            NesButton::Right => self.right = pressed,
        }
    }

    /// Clear all button states
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

impl InputState {
    /// Create new input state with all buttons released
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply input state to emulator console
    ///
    /// This updates the emulator's controller state for both players.
    pub fn apply_to_console(&self, console: &mut rustynes_core::Console) {
        // All 8 NES buttons
        const BUTTONS: [NesButton; 8] = [
            NesButton::A,
            NesButton::B,
            NesButton::Select,
            NesButton::Start,
            NesButton::Up,
            NesButton::Down,
            NesButton::Left,
            NesButton::Right,
        ];

        // Apply Player 1
        for &button in &BUTTONS {
            console.set_button_1(button, self.player1.is_pressed(button));
        }

        // Apply Player 2
        for &button in &BUTTONS {
            console.set_button_2(button, self.player2.is_pressed(button));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_state_set_get() {
        let mut state = ControllerState::new();

        // Initially all released
        assert!(!state.is_pressed(NesButton::A));

        // Press A
        state.set(NesButton::A, true);
        assert!(state.is_pressed(NesButton::A));

        // Release A
        state.set(NesButton::A, false);
        assert!(!state.is_pressed(NesButton::A));
    }

    #[test]
    fn test_controller_state_clear() {
        let mut state = ControllerState::new();

        // Press multiple buttons
        state.set(NesButton::A, true);
        state.set(NesButton::Start, true);
        state.set(NesButton::Up, true);

        // Clear all
        state.clear();

        // All should be released
        assert!(!state.is_pressed(NesButton::A));
        assert!(!state.is_pressed(NesButton::Start));
        assert!(!state.is_pressed(NesButton::Up));
    }

    #[test]
    fn test_input_state_dual_player() {
        let mut input = InputState::new();

        // Player 1 presses A
        input.player1.set(NesButton::A, true);
        assert!(input.player1.is_pressed(NesButton::A));
        assert!(!input.player2.is_pressed(NesButton::A));

        // Player 2 presses B
        input.player2.set(NesButton::B, true);
        assert!(!input.player1.is_pressed(NesButton::B));
        assert!(input.player2.is_pressed(NesButton::B));
    }
}
