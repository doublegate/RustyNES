# [Milestone 6] Sprint 6.4: Controller Support

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week
**Assignee:** Claude Code / Developer
**Sprint:** M6-S4 (GUI - Input System)
**Progress:** 0%

---

## Overview

This sprint implements **keyboard and gamepad input** for the desktop frontend, mapping physical controls to NES controller buttons and providing configuration UI for custom mappings.

### Goals

- ⏳ Keyboard input (default mappings)
- ⏳ Gamepad detection (gilrs)
- ⏳ Gamepad button mapping
- ⏳ Controller configuration UI
- ⏳ Save controller mappings (config file)
- ⏳ Player 1/2 selection
- ⏳ Hotkey support (save states, reset, etc.)
- ⏳ Zero unsafe code

### Prerequisites

- ✅ M5-S6 Input Handling (Console input API)
- ✅ M6-S1 Application structure

---

## Tasks

### Task 1: Keyboard Input (2 hours)

**File:** `crates/rustynes-desktop/src/input.rs`

**Objective:** Map keyboard keys to NES controller buttons.

#### Subtasks

1. Define default keyboard mappings
2. Handle key down/up events from egui
3. Update Console controller state
4. Support Player 1 & 2 bindings

**Acceptance Criteria:**

- [ ] Keyboard input works for Player 1
- [ ] Player 2 keyboard bindings work (numpad)
- [ ] Key presses register correctly
- [ ] Key releases register correctly
- [ ] No input lag

**Implementation:**

```rust
use rustynes_core::Button;
use eframe::egui::Key;
use std::collections::HashMap;

/// Input manager for keyboard and gamepad
pub struct InputManager {
    /// Keyboard mapping for Player 1
    keyboard_p1: HashMap<Key, Button>,

    /// Keyboard mapping for Player 2
    keyboard_p2: HashMap<Key, Button>,

    /// Currently pressed keys
    pressed_keys: HashMap<Key, bool>,
}

impl InputManager {
    pub fn new() -> Self {
        Self {
            keyboard_p1: Self::default_keyboard_p1(),
            keyboard_p2: Self::default_keyboard_p2(),
            pressed_keys: HashMap::new(),
        }
    }

    /// Default keyboard mapping for Player 1
    fn default_keyboard_p1() -> HashMap<Key, Button> {
        let mut map = HashMap::new();

        // Arrow keys for D-pad
        map.insert(Key::ArrowUp, Button::Up);
        map.insert(Key::ArrowDown, Button::Down);
        map.insert(Key::ArrowLeft, Button::Left);
        map.insert(Key::ArrowRight, Button::Right);

        // Z/X for B/A (common mapping)
        map.insert(Key::Z, Button::B);
        map.insert(Key::X, Button::A);

        // Shift/Enter for Select/Start
        map.insert(Key::S, Button::Select);
        map.insert(Key::Enter, Button::Start);

        map
    }

    /// Default keyboard mapping for Player 2 (numpad)
    fn default_keyboard_p2() -> HashMap<Key, Button> {
        let mut map = HashMap::new();

        // Numpad 8/5/4/6 for D-pad
        map.insert(Key::Num8, Button::Up);
        map.insert(Key::Num5, Button::Down);
        map.insert(Key::Num4, Button::Left);
        map.insert(Key::Num6, Button::Right);

        // Numpad 1/2 for B/A
        map.insert(Key::Num1, Button::B);
        map.insert(Key::Num2, Button::A);

        // Numpad +/* for Select/Start
        map.insert(Key::NumpadAdd, Button::Select);
        map.insert(Key::NumpadMultiply, Button::Start);

        map
    }

    /// Handle key event from egui
    pub fn handle_key_event(&mut self, key: Key, pressed: bool, console: &mut rustynes_core::Console) {
        // Track key state
        self.pressed_keys.insert(key, pressed);

        // Player 1 mapping
        if let Some(&button) = self.keyboard_p1.get(&key) {
            console.set_button_1(button, pressed);
        }

        // Player 2 mapping
        if let Some(&button) = self.keyboard_p2.get(&key) {
            console.set_button_2(button, pressed);
        }
    }

    /// Check if a key is currently pressed
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.pressed_keys.get(&key).copied().unwrap_or(false)
    }

    /// Set custom keyboard mapping for Player 1
    pub fn set_keyboard_p1(&mut self, key: Key, button: Button) {
        self.keyboard_p1.insert(key, button);
    }

    /// Set custom keyboard mapping for Player 2
    pub fn set_keyboard_p2(&mut self, key: Key, button: Button) {
        self.keyboard_p2.insert(key, button);
    }

    /// Get keyboard mapping for Player 1
    pub fn keyboard_p1(&self) -> &HashMap<Key, Button> {
        &self.keyboard_p1
    }

    /// Get keyboard mapping for Player 2
    pub fn keyboard_p2(&self) -> &HashMap<Key, Button> {
        &self.keyboard_p2
    }
}

impl Default for InputManager {
    fn default() -> Self {
        Self::new()
    }
}
```

---

### Task 2: egui Input Integration (1 hour)

**File:** `crates/rustynes-desktop/src/app.rs`

**Objective:** Wire egui keyboard events to InputManager.

#### Subtasks

1. Add InputManager to RustyNesApp
2. Poll egui keyboard events each frame
3. Update Console controller state

**Acceptance Criteria:**

- [ ] Keyboard events captured
- [ ] Console receives input updates
- [ ] No input lag

**Implementation:**

```rust
use crate::input::InputManager;

pub struct RustyNesApp {
    // ... existing fields ...

    /// Input manager
    input: InputManager,
}

impl RustyNesApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // ... existing initialization ...

        Self {
            // ... existing fields ...
            input: InputManager::new(),
        }
    }
}

impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // ... existing code ...

        // Handle keyboard input
        if let Some(console) = &mut self.console {
            self.handle_input(ctx, console);
        }

        // ... rest of update ...
    }
}

impl RustyNesApp {
    fn handle_input(&mut self, ctx: &egui::Context, console: &mut rustynes_core::Console) {
        // Get input events from egui
        ctx.input(|i| {
            // Check all potential input keys
            for key in &[
                // Player 1
                egui::Key::ArrowUp, egui::Key::ArrowDown,
                egui::Key::ArrowLeft, egui::Key::ArrowRight,
                egui::Key::Z, egui::Key::X,
                egui::Key::S, egui::Key::Enter,

                // Player 2
                egui::Key::Num8, egui::Key::Num5,
                egui::Key::Num4, egui::Key::Num6,
                egui::Key::Num1, egui::Key::Num2,
                egui::Key::NumpadAdd, egui::Key::NumpadMultiply,
            ] {
                let pressed = i.key_down(*key);
                let was_pressed = self.input.is_key_pressed(*key);

                // Only update on state change
                if pressed != was_pressed {
                    self.input.handle_key_event(*key, pressed, console);
                }
            }
        });
    }
}
```

---

### Task 3: Gamepad Support (3 hours)

**File:** `crates/rustynes-desktop/src/input/gamepad.rs`

**Objective:** Add gamepad detection and input using gilrs.

#### Subtasks

1. Add gilrs dependency
2. Detect connected gamepads
3. Map gamepad buttons to NES buttons
4. Handle analog stick → D-pad conversion
5. Support multiple gamepads (Player 1/2)

**Acceptance Criteria:**

- [ ] Gamepad detection works
- [ ] Button presses register correctly
- [ ] Analog sticks work as D-pad
- [ ] Multiple gamepads supported
- [ ] Works on Linux, Windows, macOS

**Dependencies:**

```toml
# Add to crates/rustynes-desktop/Cargo.toml

[dependencies]
gilrs = "0.10"
```

**Implementation:**

```rust
use gilrs::{Gilrs, Event, EventType, Button as GilrsButton, Axis};
use rustynes_core::Button;

/// Gamepad manager
pub struct GamepadManager {
    gilrs: Gilrs,

    /// Player 1 gamepad ID
    player1_gamepad: Option<gilrs::GamepadId>,

    /// Player 2 gamepad ID
    player2_gamepad: Option<gilrs::GamepadId>,

    /// Deadzone for analog sticks (0.0 to 1.0)
    deadzone: f32,
}

impl GamepadManager {
    pub fn new() -> Result<Self, String> {
        let gilrs = Gilrs::new()
            .map_err(|e| format!("Failed to initialize gamepad system: {}", e))?;

        Ok(Self {
            gilrs,
            player1_gamepad: None,
            player2_gamepad: None,
            deadzone: 0.2,
        })
    }

    /// Poll gamepad events and update console
    pub fn poll_events(&mut self, console: &mut rustynes_core::Console) {
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            // Auto-assign gamepads to players
            if self.player1_gamepad.is_none() {
                self.player1_gamepad = Some(id);
                log::info!("Gamepad {} assigned to Player 1", id);
            } else if self.player1_gamepad != Some(id) && self.player2_gamepad.is_none() {
                self.player2_gamepad = Some(id);
                log::info!("Gamepad {} assigned to Player 2", id);
            }

            // Handle event
            match event {
                EventType::ButtonPressed(button, _) => {
                    if Some(id) == self.player1_gamepad {
                        if let Some(nes_button) = self.map_button(button) {
                            console.set_button_1(nes_button, true);
                        }
                    } else if Some(id) == self.player2_gamepad {
                        if let Some(nes_button) = self.map_button(button) {
                            console.set_button_2(nes_button, true);
                        }
                    }
                }

                EventType::ButtonReleased(button, _) => {
                    if Some(id) == self.player1_gamepad {
                        if let Some(nes_button) = self.map_button(button) {
                            console.set_button_1(nes_button, false);
                        }
                    } else if Some(id) == self.player2_gamepad {
                        if let Some(nes_button) = self.map_button(button) {
                            console.set_button_2(nes_button, false);
                        }
                    }
                }

                EventType::Disconnected => {
                    log::info!("Gamepad {} disconnected", id);
                    if self.player1_gamepad == Some(id) {
                        self.player1_gamepad = None;
                    } else if self.player2_gamepad == Some(id) {
                        self.player2_gamepad = None;
                    }
                }

                _ => {}
            }
        }

        // Handle analog sticks
        self.handle_analog_sticks(console);
    }

    fn map_button(&self, button: GilrsButton) -> Option<Button> {
        match button {
            GilrsButton::South => Some(Button::B),       // Xbox A / PS Cross
            GilrsButton::East => Some(Button::A),        // Xbox B / PS Circle
            GilrsButton::Select => Some(Button::Select), // Back / Share
            GilrsButton::Start => Some(Button::Start),   // Start / Options
            GilrsButton::DPadUp => Some(Button::Up),
            GilrsButton::DPadDown => Some(Button::Down),
            GilrsButton::DPadLeft => Some(Button::Left),
            GilrsButton::DPadRight => Some(Button::Right),
            _ => None,
        }
    }

    fn handle_analog_sticks(&mut self, console: &mut rustynes_core::Console) {
        // Player 1
        if let Some(id) = self.player1_gamepad {
            if let Some(gamepad) = self.gilrs.gamepad(id).upgrade() {
                let left_x = gamepad.value(Axis::LeftStickX);
                let left_y = gamepad.value(Axis::LeftStickY);

                // Convert analog to digital with deadzone
                let left = left_x < -self.deadzone;
                let right = left_x > self.deadzone;
                let up = left_y < -self.deadzone;
                let down = left_y > self.deadzone;

                console.set_button_1(Button::Left, left);
                console.set_button_1(Button::Right, right);
                console.set_button_1(Button::Up, up);
                console.set_button_1(Button::Down, down);
            }
        }

        // Player 2
        if let Some(id) = self.player2_gamepad {
            if let Some(gamepad) = self.gilrs.gamepad(id).upgrade() {
                let left_x = gamepad.value(Axis::LeftStickX);
                let left_y = gamepad.value(Axis::LeftStickY);

                let left = left_x < -self.deadzone;
                let right = left_x > self.deadzone;
                let up = left_y < -self.deadzone;
                let down = left_y > self.deadzone;

                console.set_button_2(Button::Left, left);
                console.set_button_2(Button::Right, right);
                console.set_button_2(Button::Up, up);
                console.set_button_2(Button::Down, down);
            }
        }
    }

    /// Get connected gamepad count
    pub fn gamepad_count(&self) -> usize {
        self.gilrs.gamepads().count()
    }

    /// Set analog stick deadzone
    pub fn set_deadzone(&mut self, deadzone: f32) {
        self.deadzone = deadzone.clamp(0.0, 1.0);
    }
}
```

---

### Task 4: Controller Configuration UI (3 hours)

**File:** `crates/rustynes-desktop/src/ui/input_settings.rs`

**Objective:** Create UI for configuring keyboard and gamepad mappings.

#### Subtasks

1. Controller settings window
2. Keyboard remapping interface
3. Gamepad detection display
4. Deadzone slider
5. Save/load mappings

**Acceptance Criteria:**

- [ ] Settings window displays correctly
- [ ] Can remap keys
- [ ] Gamepad status displayed
- [ ] Deadzone adjustable
- [ ] Changes apply immediately

**Implementation:**

```rust
use eframe::egui;
use rustynes_core::Button;
use crate::app::RustyNesApp;

pub struct InputSettings {
    /// Currently remapping this button
    remapping: Option<(u8, Button)>, // (player, button)
}

impl InputSettings {
    pub fn new() -> Self {
        Self {
            remapping: None,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, app: &mut RustyNesApp) {
        egui::Window::new("Input Settings")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Player 1 Keyboard");

                self.show_keyboard_mapping(ui, 1, app);

                ui.separator();

                ui.heading("Player 2 Keyboard");

                self.show_keyboard_mapping(ui, 2, app);

                ui.separator();

                ui.heading("Gamepad");

                self.show_gamepad_settings(ui, app);
            });
    }

    fn show_keyboard_mapping(&mut self, ui: &mut egui::Ui, player: u8, app: &mut RustyNesApp) {
        let buttons = [
            ("Up", Button::Up),
            ("Down", Button::Down),
            ("Left", Button::Left),
            ("Right", Button::Right),
            ("A", Button::A),
            ("B", Button::B),
            ("Select", Button::Select),
            ("Start", Button::Start),
        ];

        for (name, button) in &buttons {
            ui.horizontal(|ui| {
                ui.label(format!("{}: ", name));

                // Show current key binding
                let current_key = if player == 1 {
                    app.input.keyboard_p1().iter()
                        .find(|(_, b)| *b == button)
                        .map(|(k, _)| format!("{:?}", k))
                        .unwrap_or_else(|| "None".to_string())
                } else {
                    app.input.keyboard_p2().iter()
                        .find(|(_, b)| *b == button)
                        .map(|(k, _)| format!("{:?}", k))
                        .unwrap_or_else(|| "None".to_string())
                };

                if ui.button(&current_key).clicked() {
                    self.remapping = Some((player, *button));
                }

                if self.remapping == Some((player, *button)) {
                    ui.label("Press a key...");

                    // TODO: Capture next key press and update mapping
                }
            });
        }
    }

    fn show_gamepad_settings(&self, ui: &mut egui::Ui, app: &RustyNesApp) {
        if let Some(gamepad_mgr) = &app.gamepad {
            let count = gamepad_mgr.gamepad_count();

            ui.label(format!("Connected gamepads: {}", count));

            if count == 0 {
                ui.colored_label(egui::Color32::YELLOW, "No gamepads detected");
            } else {
                ui.label("Player 1: Gamepad 1");
                if count >= 2 {
                    ui.label("Player 2: Gamepad 2");
                }
            }

            ui.separator();

            // Deadzone slider
            ui.label("Analog Stick Deadzone:");
            // TODO: Add deadzone slider
        } else {
            ui.colored_label(egui::Color32::RED, "Gamepad support unavailable");
        }
    }
}
```

---

### Task 5: Hotkeys (2 hours)

**File:** `crates/rustynes-desktop/src/input.rs` (hotkeys)

**Objective:** Add hotkeys for save states, reset, screenshots, etc.

#### Subtasks

1. F1-F12 for save state slots
2. Shift+F1-F12 for load state slots
3. Ctrl+R for reset
4. F9 for screenshot
5. Handle hotkey conflicts (don't send to emulator)

**Acceptance Criteria:**

- [ ] All hotkeys work
- [ ] Hotkeys don't interfere with gameplay
- [ ] Clear documentation of hotkey bindings

**Implementation:**

```rust
impl RustyNesApp {
    pub fn handle_hotkeys(&mut self, ctx: &egui::Context) {
        // Save state slots (F1-F12)
        for i in 1..=12 {
            let key = match i {
                1 => egui::Key::F1,
                2 => egui::Key::F2,
                3 => egui::Key::F3,
                4 => egui::Key::F4,
                5 => egui::Key::F5,
                6 => egui::Key::F6,
                7 => egui::Key::F7,
                8 => egui::Key::F8,
                9 => egui::Key::F9,
                10 => egui::Key::F10,
                11 => egui::Key::F11,
                12 => egui::Key::F12,
                _ => continue,
            };

            // Save state: F1-F12
            if ctx.input(|i| i.key_pressed(key) && !i.modifiers.shift) {
                self.save_state_slot(i);
            }

            // Load state: Shift+F1-F12
            if ctx.input(|i| i.key_pressed(key) && i.modifiers.shift) {
                self.load_state_slot(i);
            }
        }

        // Reset: Ctrl+R (handled in shortcuts)
        // Screenshot: F9
        if ctx.input(|i| i.key_pressed(egui::Key::F9)) {
            self.take_screenshot();
        }
    }

    fn save_state_slot(&mut self, slot: u32) {
        if let Some(console) = &self.console {
            log::info!("Saving state to slot {}", slot);
            // TODO: Implement save state to file
        }
    }

    fn load_state_slot(&mut self, slot: u32) {
        if let Some(console) = &mut self.console {
            log::info!("Loading state from slot {}", slot);
            // TODO: Implement load state from file
        }
    }

    fn take_screenshot(&self) {
        log::info!("Taking screenshot");
        // TODO: Implement screenshot
    }
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Keyboard input works for Player 1 & 2
- [ ] Gamepad detection works
- [ ] Gamepad buttons map correctly
- [ ] Analog sticks work as D-pad
- [ ] Multiple gamepads supported
- [ ] Configuration UI functional
- [ ] Hotkeys work
- [ ] Mappings persist (Sprint 5)

### User Experience

- [ ] Input feels responsive (<10ms latency)
- [ ] Configuration UI is intuitive
- [ ] Clear indication of connected gamepads
- [ ] Hotkeys discoverable

### Quality

- [ ] Zero unsafe code
- [ ] Works on all platforms
- [ ] No input conflicts
- [ ] Clean handling of gamepad disconnect

---

## Dependencies

### External Crates

```toml
gilrs = "0.10"  # Gamepad input library
```

---

## Related Documentation

- [INPUT_HANDLING.md](../../../docs/input/INPUT_HANDLING.md) - NES controller protocol
- [M5-S6-input-handling.md](../../milestone-5-integration/M5-S6-input-handling.md) - Core input system

---

## Performance Targets

- **Input Latency:** <10ms (keyboard + gamepad)
- **Polling Rate:** 60 Hz (every frame)
- **CPU Usage:** <1% for input processing

---

## Success Criteria

- [ ] All tasks complete
- [ ] Keyboard and gamepad input work
- [ ] Configuration UI complete
- [ ] Hotkeys functional
- [ ] Two-player support works
- [ ] Ready for configuration persistence (Sprint 5)

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1 (Application shell)
**Next Sprint:** [M6-S5 Configuration & Polish](M6-S5-configuration-polish.md)
