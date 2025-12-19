//! Gamepad input support via gilrs.
//!
//! Handles gamepad detection, button mapping, and analog stick emulation
//! for NES controller inputs.

use gilrs::{Axis, Button as GilrsButton, Event, EventType, Gilrs};
use rustynes_core::Button as NesButton;
use tracing::{error, info, warn};

use super::ControllerState;

/// Gamepad manager for up to 2 players
#[derive(Debug)]
pub struct GamepadManager {
    /// gilrs context
    gilrs: Gilrs,
    /// Player 1 gamepad ID
    player1_id: Option<gilrs::GamepadId>,
    /// Player 2 gamepad ID
    player2_id: Option<gilrs::GamepadId>,
    /// Analog stick deadzone (0.0-1.0)
    deadzone: f32,
}

impl GamepadManager {
    /// Create new gamepad manager
    ///
    /// # Errors
    ///
    /// Returns error if gilrs fails to initialize
    pub fn new() -> Result<Self, String> {
        let gilrs = Gilrs::new().map_err(|e| format!("Failed to initialize gilrs: {e}"))?;

        info!("Gamepad manager initialized");

        Ok(Self {
            gilrs,
            player1_id: None,
            player2_id: None,
            deadzone: 0.2,
        })
    }

    /// Poll gamepads and update controller states
    ///
    /// This should be called every frame to process gamepad events.
    pub fn poll(&mut self, player1: &mut ControllerState, player2: &mut ControllerState) {
        // Process all pending events
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            self.handle_event(id, event, player1, player2);
        }

        // Handle analog sticks (continuous state, not events)
        self.handle_analog_sticks(player1, player2);
    }

    /// Handle a single gamepad event
    fn handle_event(
        &mut self,
        id: gilrs::GamepadId,
        event: EventType,
        player1: &mut ControllerState,
        player2: &mut ControllerState,
    ) {
        match event {
            EventType::ButtonPressed(button, _) => {
                if let Some(nes_button) = Self::map_button(button) {
                    if Some(id) == self.player1_id {
                        player1.set(nes_button, true);
                    } else if Some(id) == self.player2_id {
                        player2.set(nes_button, true);
                    } else {
                        // Auto-assign to first available player slot
                        self.auto_assign_gamepad(id);
                        if Some(id) == self.player1_id {
                            player1.set(nes_button, true);
                        } else if Some(id) == self.player2_id {
                            player2.set(nes_button, true);
                        }
                    }
                }
            }

            EventType::ButtonReleased(button, _) => {
                if let Some(nes_button) = Self::map_button(button) {
                    if Some(id) == self.player1_id {
                        player1.set(nes_button, false);
                    } else if Some(id) == self.player2_id {
                        player2.set(nes_button, false);
                    }
                }
            }

            EventType::Connected => {
                info!("Gamepad {id:?} connected");
                self.auto_assign_gamepad(id);
            }

            EventType::Disconnected => {
                info!("Gamepad {id:?} disconnected");
                if self.player1_id == Some(id) {
                    self.player1_id = None;
                    player1.clear();
                } else if self.player2_id == Some(id) {
                    self.player2_id = None;
                    player2.clear();
                }
            }

            _ => {}
        }
    }

    /// Auto-assign gamepad to first available player slot
    fn auto_assign_gamepad(&mut self, id: gilrs::GamepadId) {
        if self.player1_id.is_none() {
            self.player1_id = Some(id);
            info!("Gamepad {id:?} assigned to Player 1");
        } else if self.player2_id.is_none() {
            self.player2_id = Some(id);
            info!("Gamepad {id:?} assigned to Player 2");
        } else {
            warn!("Gamepad {id:?} could not be assigned (all slots full)");
        }
    }

    /// Map gilrs button to NES button
    ///
    /// Uses standard gamepad conventions (Xbox/PlayStation layout)
    fn map_button(button: GilrsButton) -> Option<NesButton> {
        match button {
            // Face buttons (Xbox: A/B, PlayStation: Cross/Circle)
            GilrsButton::South => Some(NesButton::B), // Xbox A / PS Cross
            GilrsButton::East => Some(NesButton::A),  // Xbox B / PS Circle

            // System buttons
            GilrsButton::Select => Some(NesButton::Select),
            GilrsButton::Start => Some(NesButton::Start),

            // D-pad
            GilrsButton::DPadUp => Some(NesButton::Up),
            GilrsButton::DPadDown => Some(NesButton::Down),
            GilrsButton::DPadLeft => Some(NesButton::Left),
            GilrsButton::DPadRight => Some(NesButton::Right),

            // Unmapped buttons
            _ => None,
        }
    }

    /// Handle analog stick input (convert to D-pad)
    fn handle_analog_sticks(
        &mut self,
        player1: &mut ControllerState,
        player2: &mut ControllerState,
    ) {
        // Player 1 analog stick
        if let Some(id) = self.player1_id {
            if let Some(gamepad) = self.gilrs.connected_gamepad(id) {
                let left_x = gamepad.value(Axis::LeftStickX);
                let left_y = gamepad.value(Axis::LeftStickY);

                // Only update D-pad from analog if D-pad buttons aren't already pressed
                // (D-pad buttons take priority)
                if !player1.left && !player1.right {
                    player1.set(NesButton::Left, left_x < -self.deadzone);
                    player1.set(NesButton::Right, left_x > self.deadzone);
                }

                if !player1.up && !player1.down {
                    player1.set(NesButton::Up, left_y < -self.deadzone);
                    player1.set(NesButton::Down, left_y > self.deadzone);
                }
            }
        }

        // Player 2 analog stick
        if let Some(id) = self.player2_id {
            if let Some(gamepad) = self.gilrs.connected_gamepad(id) {
                let left_x = gamepad.value(Axis::LeftStickX);
                let left_y = gamepad.value(Axis::LeftStickY);

                if !player2.left && !player2.right {
                    player2.set(NesButton::Left, left_x < -self.deadzone);
                    player2.set(NesButton::Right, left_x > self.deadzone);
                }

                if !player2.up && !player2.down {
                    player2.set(NesButton::Up, left_y < -self.deadzone);
                    player2.set(NesButton::Down, left_y > self.deadzone);
                }
            }
        }
    }

    /// Get number of connected gamepads
    #[must_use]
    #[allow(dead_code)] // Future: display in settings
    pub fn gamepad_count(&self) -> usize {
        self.gilrs.gamepads().count()
    }

    /// Set analog stick deadzone (0.0-1.0)
    #[allow(dead_code)] // Future: configurable in settings
    pub fn set_deadzone(&mut self, deadzone: f32) {
        self.deadzone = deadzone.clamp(0.0, 1.0);
    }

    /// Get current deadzone value
    #[must_use]
    #[allow(dead_code)] // Future: display in settings
    pub fn deadzone(&self) -> f32 {
        self.deadzone
    }
}

impl Default for GamepadManager {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            error!("Failed to initialize gamepad manager: {e}");
            // Return a dummy instance that won't process any events
            // This is safe because gilrs::new() only fails if the platform doesn't support gamepads
            panic!("Gamepad support is required but failed to initialize");
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for floating-point comparisons
    const EPSILON: f32 = 0.001;

    /// Helper to compare floats with tolerance
    fn assert_float_eq(actual: f32, expected: f32, msg: &str) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{msg}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn test_button_mapping() {
        // Test standard button mappings
        assert_eq!(
            GamepadManager::map_button(GilrsButton::South),
            Some(NesButton::B)
        );
        assert_eq!(
            GamepadManager::map_button(GilrsButton::East),
            Some(NesButton::A)
        );
        assert_eq!(
            GamepadManager::map_button(GilrsButton::Start),
            Some(NesButton::Start)
        );
        assert_eq!(
            GamepadManager::map_button(GilrsButton::DPadUp),
            Some(NesButton::Up)
        );

        // Test unmapped button
        assert_eq!(GamepadManager::map_button(GilrsButton::North), None);
    }

    #[test]
    fn test_deadzone() {
        let mut manager = GamepadManager::new().unwrap();

        // Default deadzone
        assert_float_eq(manager.deadzone(), 0.2, "default deadzone");

        // Set new deadzone
        manager.set_deadzone(0.3);
        assert_float_eq(manager.deadzone(), 0.3, "custom deadzone");

        // Clamp to valid range
        manager.set_deadzone(1.5);
        assert_float_eq(manager.deadzone(), 1.0, "clamped max");

        manager.set_deadzone(-0.5);
        assert_float_eq(manager.deadzone(), 0.0, "clamped min");
    }
}
