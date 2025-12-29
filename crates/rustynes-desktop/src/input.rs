//! Input handling for keyboard and gamepad controllers.
//!
//! This module provides:
//! - Keyboard input mapping to NES controller buttons
//! - Gamepad support via gilrs
//! - Configurable key bindings

use crate::config::KeyboardBindings;
use gilrs::{Button, Event, EventType, Gilrs};
use log::{info, warn};
use std::collections::HashMap;

/// Keyboard key codes for input mapping.
///
/// These match the JavaScript KeyboardEvent.code values used in configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code, missing_docs)]
pub enum KeyCode {
    // Letters
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    // Numbers
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    // Arrow keys
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    // Special keys
    Enter,
    Space,
    Escape,
    Tab,
    Backspace,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

/// NES controller button flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NesButton {
    /// A button (right action button).
    A,
    /// B button (left action button).
    B,
    /// Select button.
    Select,
    /// Start button.
    Start,
    /// D-pad Up.
    Up,
    /// D-pad Down.
    Down,
    /// D-pad Left.
    Left,
    /// D-pad Right.
    Right,
}

impl NesButton {
    /// Get the bit mask for this button in the controller state.
    #[must_use]
    pub const fn mask(self) -> u8 {
        match self {
            Self::A => 0x01,
            Self::B => 0x02,
            Self::Select => 0x04,
            Self::Start => 0x08,
            Self::Up => 0x10,
            Self::Down => 0x20,
            Self::Left => 0x40,
            Self::Right => 0x80,
        }
    }
}

/// Controller state for a single player.
#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerState {
    /// Button state as a bitmask.
    buttons: u8,
}

impl ControllerState {
    /// Create a new controller state with no buttons pressed.
    #[must_use]
    pub const fn new() -> Self {
        Self { buttons: 0 }
    }

    /// Set a button state.
    pub fn set_button(&mut self, button: NesButton, pressed: bool) {
        if pressed {
            self.buttons |= button.mask();
        } else {
            self.buttons &= !button.mask();
        }
    }

    /// Check if a button is pressed.
    #[must_use]
    pub const fn is_pressed(&self, button: NesButton) -> bool {
        (self.buttons & button.mask()) != 0
    }

    /// Get the raw button state as a byte.
    #[must_use]
    pub const fn as_byte(&self) -> u8 {
        self.buttons
    }

    /// Clear all button states.
    pub fn clear(&mut self) {
        self.buttons = 0;
    }
}

/// Input handler for keyboard and gamepad.
pub struct InputHandler {
    /// Player 1 keyboard bindings.
    player1_keys: HashMap<KeyCode, NesButton>,
    /// Player 2 keyboard bindings.
    player2_keys: HashMap<KeyCode, NesButton>,
    /// Player 1 controller state.
    player1: ControllerState,
    /// Player 2 controller state.
    player2: ControllerState,
    /// Gilrs gamepad manager.
    gilrs: Option<Gilrs>,
    /// Active gamepad ID for player 1.
    gamepad_p1: Option<gilrs::GamepadId>,
    /// Active gamepad ID for player 2.
    gamepad_p2: Option<gilrs::GamepadId>,
}

impl InputHandler {
    /// Create a new input handler with the given key bindings.
    pub fn new(player1_bindings: &KeyboardBindings, player2_bindings: &KeyboardBindings) -> Self {
        let player1_keys = Self::parse_bindings(player1_bindings);
        let player2_keys = Self::parse_bindings(player2_bindings);

        // Initialize gilrs for gamepad support
        let gilrs = match Gilrs::new() {
            Ok(g) => {
                // Log connected gamepads
                for (id, gamepad) in g.gamepads() {
                    info!("Gamepad connected: {} ({:?})", gamepad.name(), id);
                }
                Some(g)
            }
            Err(e) => {
                warn!("Failed to initialize gamepad support: {e}");
                None
            }
        };

        Self {
            player1_keys,
            player2_keys,
            player1: ControllerState::new(),
            player2: ControllerState::new(),
            gilrs,
            gamepad_p1: None,
            gamepad_p2: None,
        }
    }

    /// Parse keyboard bindings from config strings to `KeyCode` mappings.
    fn parse_bindings(bindings: &KeyboardBindings) -> HashMap<KeyCode, NesButton> {
        let mut map = HashMap::new();

        if let Some(key) = Self::parse_key(&bindings.a) {
            map.insert(key, NesButton::A);
        }
        if let Some(key) = Self::parse_key(&bindings.b) {
            map.insert(key, NesButton::B);
        }
        if let Some(key) = Self::parse_key(&bindings.select) {
            map.insert(key, NesButton::Select);
        }
        if let Some(key) = Self::parse_key(&bindings.start) {
            map.insert(key, NesButton::Start);
        }
        if let Some(key) = Self::parse_key(&bindings.up) {
            map.insert(key, NesButton::Up);
        }
        if let Some(key) = Self::parse_key(&bindings.down) {
            map.insert(key, NesButton::Down);
        }
        if let Some(key) = Self::parse_key(&bindings.left) {
            map.insert(key, NesButton::Left);
        }
        if let Some(key) = Self::parse_key(&bindings.right) {
            map.insert(key, NesButton::Right);
        }

        map
    }

    /// Parse a key string to a `KeyCode`.
    fn parse_key(key_str: &str) -> Option<KeyCode> {
        match key_str {
            // Letters
            "KeyA" => Some(KeyCode::KeyA),
            "KeyB" => Some(KeyCode::KeyB),
            "KeyC" => Some(KeyCode::KeyC),
            "KeyD" => Some(KeyCode::KeyD),
            "KeyE" => Some(KeyCode::KeyE),
            "KeyF" => Some(KeyCode::KeyF),
            "KeyG" => Some(KeyCode::KeyG),
            "KeyH" => Some(KeyCode::KeyH),
            "KeyI" => Some(KeyCode::KeyI),
            "KeyJ" => Some(KeyCode::KeyJ),
            "KeyK" => Some(KeyCode::KeyK),
            "KeyL" => Some(KeyCode::KeyL),
            "KeyM" => Some(KeyCode::KeyM),
            "KeyN" => Some(KeyCode::KeyN),
            "KeyO" => Some(KeyCode::KeyO),
            "KeyP" => Some(KeyCode::KeyP),
            "KeyQ" => Some(KeyCode::KeyQ),
            "KeyR" => Some(KeyCode::KeyR),
            "KeyS" => Some(KeyCode::KeyS),
            "KeyT" => Some(KeyCode::KeyT),
            "KeyU" => Some(KeyCode::KeyU),
            "KeyV" => Some(KeyCode::KeyV),
            "KeyW" => Some(KeyCode::KeyW),
            "KeyX" => Some(KeyCode::KeyX),
            "KeyY" => Some(KeyCode::KeyY),
            "KeyZ" => Some(KeyCode::KeyZ),

            // Numbers
            "Digit0" => Some(KeyCode::Digit0),
            "Digit1" => Some(KeyCode::Digit1),
            "Digit2" => Some(KeyCode::Digit2),
            "Digit3" => Some(KeyCode::Digit3),
            "Digit4" => Some(KeyCode::Digit4),
            "Digit5" => Some(KeyCode::Digit5),
            "Digit6" => Some(KeyCode::Digit6),
            "Digit7" => Some(KeyCode::Digit7),
            "Digit8" => Some(KeyCode::Digit8),
            "Digit9" => Some(KeyCode::Digit9),

            // Arrow keys
            "ArrowUp" => Some(KeyCode::ArrowUp),
            "ArrowDown" => Some(KeyCode::ArrowDown),
            "ArrowLeft" => Some(KeyCode::ArrowLeft),
            "ArrowRight" => Some(KeyCode::ArrowRight),

            // Special keys
            "Enter" => Some(KeyCode::Enter),
            "Space" => Some(KeyCode::Space),
            "Escape" => Some(KeyCode::Escape),
            "Tab" => Some(KeyCode::Tab),
            "Backspace" => Some(KeyCode::Backspace),
            "ShiftLeft" => Some(KeyCode::ShiftLeft),
            "ShiftRight" => Some(KeyCode::ShiftRight),
            "ControlLeft" => Some(KeyCode::ControlLeft),
            "ControlRight" => Some(KeyCode::ControlRight),
            "AltLeft" => Some(KeyCode::AltLeft),
            "AltRight" => Some(KeyCode::AltRight),

            // Function keys
            "F1" => Some(KeyCode::F1),
            "F2" => Some(KeyCode::F2),
            "F3" => Some(KeyCode::F3),
            "F4" => Some(KeyCode::F4),
            "F5" => Some(KeyCode::F5),
            "F6" => Some(KeyCode::F6),
            "F7" => Some(KeyCode::F7),
            "F8" => Some(KeyCode::F8),
            "F9" => Some(KeyCode::F9),
            "F10" => Some(KeyCode::F10),
            "F11" => Some(KeyCode::F11),
            "F12" => Some(KeyCode::F12),

            _ => {
                warn!("Unknown key binding: {key_str}");
                None
            }
        }
    }

    /// Poll gamepad events and update controller states.
    pub fn poll_gamepads(&mut self) {
        let Some(gilrs) = &mut self.gilrs else {
            return;
        };

        // Process gamepad events
        while let Some(Event { id, event, .. }) = gilrs.next_event() {
            match event {
                EventType::Connected => {
                    let gamepad = gilrs.gamepad(id);
                    info!("Gamepad connected: {} ({:?})", gamepad.name(), id);

                    // Assign to first available player slot
                    if self.gamepad_p1.is_none() {
                        self.gamepad_p1 = Some(id);
                        info!("Assigned gamepad to Player 1");
                    } else if self.gamepad_p2.is_none() {
                        self.gamepad_p2 = Some(id);
                        info!("Assigned gamepad to Player 2");
                    }
                }
                EventType::Disconnected => {
                    info!("Gamepad disconnected: {id:?}");
                    if self.gamepad_p1 == Some(id) {
                        self.gamepad_p1 = None;
                    }
                    if self.gamepad_p2 == Some(id) {
                        self.gamepad_p2 = None;
                    }
                }
                EventType::ButtonPressed(button, _) => {
                    if let Some(nes_button) = Self::map_gamepad_button(button) {
                        if self.gamepad_p1 == Some(id) {
                            self.player1.set_button(nes_button, true);
                        } else if self.gamepad_p2 == Some(id) {
                            self.player2.set_button(nes_button, true);
                        }
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(nes_button) = Self::map_gamepad_button(button) {
                        if self.gamepad_p1 == Some(id) {
                            self.player1.set_button(nes_button, false);
                        } else if self.gamepad_p2 == Some(id) {
                            self.player2.set_button(nes_button, false);
                        }
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    // Handle D-pad via analog stick
                    use gilrs::Axis;
                    const THRESHOLD: f32 = 0.5;

                    let controller = if self.gamepad_p1 == Some(id) {
                        Some(&mut self.player1)
                    } else if self.gamepad_p2 == Some(id) {
                        Some(&mut self.player2)
                    } else {
                        None
                    };

                    if let Some(ctrl) = controller {
                        match axis {
                            Axis::LeftStickX | Axis::DPadX => {
                                ctrl.set_button(NesButton::Left, value < -THRESHOLD);
                                ctrl.set_button(NesButton::Right, value > THRESHOLD);
                            }
                            Axis::LeftStickY | Axis::DPadY => {
                                ctrl.set_button(NesButton::Down, value < -THRESHOLD);
                                ctrl.set_button(NesButton::Up, value > THRESHOLD);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Map a gilrs button to an NES button.
    fn map_gamepad_button(button: Button) -> Option<NesButton> {
        match button {
            Button::South => Some(NesButton::A), // A/X
            Button::East => Some(NesButton::B),  // B/Circle
            Button::Select => Some(NesButton::Select),
            Button::Start => Some(NesButton::Start),
            Button::DPadUp => Some(NesButton::Up),
            Button::DPadDown => Some(NesButton::Down),
            Button::DPadLeft => Some(NesButton::Left),
            Button::DPadRight => Some(NesButton::Right),
            _ => None,
        }
    }

    /// Get the current state of player 1's controller.
    #[must_use]
    pub const fn player1_state(&self) -> ControllerState {
        self.player1
    }

    /// Get the current state of player 2's controller.
    #[must_use]
    pub const fn player2_state(&self) -> ControllerState {
        self.player2
    }

    /// Get player 1's button state as a byte for the emulator.
    #[must_use]
    pub const fn player1_buttons(&self) -> u8 {
        self.player1.as_byte()
    }

    /// Get player 2's button state as a byte for the emulator.
    #[must_use]
    pub const fn player2_buttons(&self) -> u8 {
        self.player2.as_byte()
    }

    /// Update key bindings for player 1.
    pub fn update_player1_bindings(&mut self, bindings: &KeyboardBindings) {
        self.player1_keys = Self::parse_bindings(bindings);
    }

    /// Update key bindings for player 2.
    pub fn update_player2_bindings(&mut self, bindings: &KeyboardBindings) {
        self.player2_keys = Self::parse_bindings(bindings);
    }

    /// Handle a keyboard key event for player 1 using configured bindings.
    ///
    /// Returns true if the key was bound to a button.
    pub fn handle_key_1(&mut self, key: KeyCode, pressed: bool) -> bool {
        if let Some(&button) = self.player1_keys.get(&key) {
            self.player1.set_button(button, pressed);
            true
        } else {
            false
        }
    }

    /// Handle a keyboard key event for player 2 using configured bindings.
    ///
    /// Returns true if the key was bound to a button.
    pub fn handle_key_2(&mut self, key: KeyCode, pressed: bool) -> bool {
        if let Some(&button) = self.player2_keys.get(&key) {
            self.player2.set_button(button, pressed);
            true
        } else {
            false
        }
    }

    /// Set a button state for a specific player.
    ///
    /// # Arguments
    /// * `player` - Player number (1 or 2)
    /// * `button` - The NES button to set
    /// * `pressed` - Whether the button is pressed
    pub fn set_button(&mut self, player: u8, button: NesButton, pressed: bool) {
        match player {
            1 => self.player1.set_button(button, pressed),
            2 => self.player2.set_button(button, pressed),
            _ => {}
        }
    }
}

/// Convert an `egui::Key` to a `KeyCode`.
///
/// Returns `None` if the key is not mappable to a NES controller binding.
#[must_use]
pub fn egui_key_to_keycode(key: egui::Key) -> Option<KeyCode> {
    use egui::Key;
    match key {
        // Letters
        Key::A => Some(KeyCode::KeyA),
        Key::B => Some(KeyCode::KeyB),
        Key::C => Some(KeyCode::KeyC),
        Key::D => Some(KeyCode::KeyD),
        Key::E => Some(KeyCode::KeyE),
        Key::F => Some(KeyCode::KeyF),
        Key::G => Some(KeyCode::KeyG),
        Key::H => Some(KeyCode::KeyH),
        Key::I => Some(KeyCode::KeyI),
        Key::J => Some(KeyCode::KeyJ),
        Key::K => Some(KeyCode::KeyK),
        Key::L => Some(KeyCode::KeyL),
        Key::M => Some(KeyCode::KeyM),
        Key::N => Some(KeyCode::KeyN),
        Key::O => Some(KeyCode::KeyO),
        Key::P => Some(KeyCode::KeyP),
        Key::Q => Some(KeyCode::KeyQ),
        Key::R => Some(KeyCode::KeyR),
        Key::S => Some(KeyCode::KeyS),
        Key::T => Some(KeyCode::KeyT),
        Key::U => Some(KeyCode::KeyU),
        Key::V => Some(KeyCode::KeyV),
        Key::W => Some(KeyCode::KeyW),
        Key::X => Some(KeyCode::KeyX),
        Key::Y => Some(KeyCode::KeyY),
        Key::Z => Some(KeyCode::KeyZ),
        // Numbers
        Key::Num0 => Some(KeyCode::Digit0),
        Key::Num1 => Some(KeyCode::Digit1),
        Key::Num2 => Some(KeyCode::Digit2),
        Key::Num3 => Some(KeyCode::Digit3),
        Key::Num4 => Some(KeyCode::Digit4),
        Key::Num5 => Some(KeyCode::Digit5),
        Key::Num6 => Some(KeyCode::Digit6),
        Key::Num7 => Some(KeyCode::Digit7),
        Key::Num8 => Some(KeyCode::Digit8),
        Key::Num9 => Some(KeyCode::Digit9),
        // Arrow keys
        Key::ArrowUp => Some(KeyCode::ArrowUp),
        Key::ArrowDown => Some(KeyCode::ArrowDown),
        Key::ArrowLeft => Some(KeyCode::ArrowLeft),
        Key::ArrowRight => Some(KeyCode::ArrowRight),
        // Special keys
        Key::Enter => Some(KeyCode::Enter),
        Key::Space => Some(KeyCode::Space),
        Key::Escape => Some(KeyCode::Escape),
        Key::Tab => Some(KeyCode::Tab),
        Key::Backspace => Some(KeyCode::Backspace),
        // Function keys
        Key::F1 => Some(KeyCode::F1),
        Key::F2 => Some(KeyCode::F2),
        Key::F3 => Some(KeyCode::F3),
        Key::F4 => Some(KeyCode::F4),
        Key::F5 => Some(KeyCode::F5),
        Key::F6 => Some(KeyCode::F6),
        Key::F7 => Some(KeyCode::F7),
        Key::F8 => Some(KeyCode::F8),
        Key::F9 => Some(KeyCode::F9),
        Key::F10 => Some(KeyCode::F10),
        Key::F11 => Some(KeyCode::F11),
        Key::F12 => Some(KeyCode::F12),
        _ => None,
    }
}

impl std::fmt::Debug for InputHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputHandler")
            .field("player1", &self.player1)
            .field("player2", &self.player2)
            .field("gamepad_p1", &self.gamepad_p1)
            .field("gamepad_p2", &self.gamepad_p2)
            .finish_non_exhaustive()
    }
}
