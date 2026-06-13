# [Milestone 6] Sprint 6.3: Input + ROM Library

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** 1 week (40 hours)
**Sprint:** M6-S3 (Input Handling + ROM Library Browser)
**Architecture:** Iced 0.13+ Elm architecture
**Progress:** 0%

---

## Overview

This sprint implements **dual-scope functionality**: (1) **Input Handling** (keyboard, gamepad via gilrs) for player controls, and (2) **ROM Library Browser** with grid/list views for game selection. This merged sprint ensures the emulator is both playable and has a proper content discovery system.

**Note:** Audio output was originally planned for this sprint but has been deferred to basic implementation in M6-S5 (Polish + Basic Run-Ahead) to keep MVP scope tight.

### Goals

**Input System:**
- ⏳ Keyboard input (Arrow keys, Z/X, Enter/Shift)
- ⏳ gilrs gamepad detection and mapping
- ⏳ Player 1 & 2 support
- ⏳ Input state management in Elm architecture
- ⏳ Zero unsafe code

**ROM Library:**
- ⏳ File system ROM discovery (`.nes` files)
- ⏳ Grid view with cover art placeholders
- ⏳ List view with metadata
- ⏳ Search/filter functionality
- ⏳ Double-click to launch ROM

### Prerequisites

- ✅ M6-S1 Iced Application Foundation complete
- ✅ M6-S2 wgpu Rendering Backend complete
- ✅ Console input API available (from Phase 1 M5-S6)

---

## Part A: Input Handling (20 hours)

### Task 1: Input State Model (4 hours)

**Files:**
- `crates/rustynes-desktop/src/input/mod.rs` (new)
- `crates/rustynes-desktop/src/input/keyboard.rs` (new)

**Objective:** Create Elm-compatible input state management.

#### 1.1 Input State Structure

```rust
// input/mod.rs
pub mod keyboard;
pub mod gamepad;

use iced::keyboard::Key;
use rustynes_core::Button as NesButton;

/// Input state for both players
#[derive(Debug, Clone, Default)]
pub struct InputState {
    pub player1: ControllerState,
    pub player2: ControllerState,
}

#[derive(Debug, Clone, Default)]
pub struct ControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl ControllerState {
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
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply input state to Console
    pub fn apply_to_console(&self, console: &mut rustynes_core::Console) {
        for button in [
            NesButton::A,
            NesButton::B,
            NesButton::Select,
            NesButton::Start,
            NesButton::Up,
            NesButton::Down,
            NesButton::Left,
            NesButton::Right,
        ] {
            console.set_button_1(button, self.player1.is_pressed(button));
            console.set_button_2(button, self.player2.is_pressed(button));
        }
    }
}
```

**Acceptance Criteria:**
- [ ] InputState structure compiles
- [ ] ControllerState manages 8 NES buttons
- [ ] apply_to_console() updates emulator state
- [ ] Zero unsafe code

---

### Task 2: Keyboard Mapping (6 hours)

**Files:**
- `crates/rustynes-desktop/src/input/keyboard.rs`

**Objective:** Implement keyboard → NES button mapping for Player 1 & 2.

#### 2.1 Default Keyboard Mappings

```rust
// input/keyboard.rs
use iced::keyboard::Key;
use rustynes_core::Button as NesButton;
use std::collections::HashMap;

pub struct KeyboardMapper {
    player1: HashMap<Key, NesButton>,
    player2: HashMap<Key, NesButton>,
}

impl KeyboardMapper {
    pub fn new() -> Self {
        Self {
            player1: Self::default_player1_mapping(),
            player2: Self::default_player2_mapping(),
        }
    }

    fn default_player1_mapping() -> HashMap<Key, NesButton> {
        let mut map = HashMap::new();

        // Arrow keys for D-pad
        map.insert(Key::Named(iced::keyboard::key::Named::ArrowUp), NesButton::Up);
        map.insert(Key::Named(iced::keyboard::key::Named::ArrowDown), NesButton::Down);
        map.insert(Key::Named(iced::keyboard::key::Named::ArrowLeft), NesButton::Left);
        map.insert(Key::Named(iced::keyboard::key::Named::ArrowRight), NesButton::Right);

        // Z/X for B/A (common emulator convention)
        map.insert(Key::Character("z".into()), NesButton::B);
        map.insert(Key::Character("x".into()), NesButton::A);

        // Shift/Enter for Select/Start
        map.insert(Key::Named(iced::keyboard::key::Named::Shift), NesButton::Select);
        map.insert(Key::Named(iced::keyboard::key::Named::Enter), NesButton::Start);

        map
    }

    fn default_player2_mapping() -> HashMap<Key, NesButton> {
        let mut map = HashMap::new();

        // Numpad for Player 2
        map.insert(Key::Character("8".into()), NesButton::Up);
        map.insert(Key::Character("5".into()), NesButton::Down);
        map.insert(Key::Character("4".into()), NesButton::Left);
        map.insert(Key::Character("6".into()), NesButton::Right);

        map.insert(Key::Character("1".into()), NesButton::B);
        map.insert(Key::Character("2".into()), NesButton::A);

        map.insert(Key::Character("+".into()), NesButton::Select);
        map.insert(Key::Character("*".into()), NesButton::Start);

        map
    }

    pub fn map_player1(&self, key: &Key) -> Option<NesButton> {
        self.player1.get(key).copied()
    }

    pub fn map_player2(&self, key: &Key) -> Option<NesButton> {
        self.player2.get(key).copied()
    }
}

impl Default for KeyboardMapper {
    fn default() -> Self {
        Self::new()
    }
}
```

#### 2.2 Iced Keyboard Events

```rust
// lib.rs (Message enum)
#[derive(Debug, Clone)]
pub enum Message {
    // ... existing messages ...

    KeyPressed(iced::keyboard::Key),
    KeyReleased(iced::keyboard::Key),
}

// lib.rs (Update)
impl RustyNesModel {
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::KeyPressed(key) => {
                // Player 1
                if let Some(button) = self.keyboard_mapper.map_player1(&key) {
                    self.input_state.player1.set(button, true);
                }
                // Player 2
                if let Some(button) = self.keyboard_mapper.map_player2(&key) {
                    self.input_state.player2.set(button, true);
                }

                // Apply to console
                if let Some(console) = &mut self.console {
                    self.input_state.apply_to_console(console);
                }

                iced::Task::none()
            }
            Message::KeyReleased(key) => {
                if let Some(button) = self.keyboard_mapper.map_player1(&key) {
                    self.input_state.player1.set(button, false);
                }
                if let Some(button) = self.keyboard_mapper.map_player2(&key) {
                    self.input_state.player2.set(button, false);
                }

                if let Some(console) = &mut self.console {
                    self.input_state.apply_to_console(console);
                }

                iced::Task::none()
            }
            // ... other messages ...
        }
    }
}
```

#### 2.3 Subscription for Keyboard Events

```rust
// lib.rs (Subscription)
use iced::keyboard;

impl RustyNesModel {
    pub fn subscription(&self) -> iced::Subscription<Message> {
        keyboard::on_key_press(|key, _modifiers| {
            Some(Message::KeyPressed(key))
        })
        .merge(keyboard::on_key_release(|key, _modifiers| {
            Some(Message::KeyReleased(key))
        }))
    }
}
```

**Acceptance Criteria:**
- [ ] Player 1 keyboard input functional (Arrow keys + Z/X)
- [ ] Player 2 keyboard input functional (Numpad)
- [ ] Key press/release events handled
- [ ] Input state updates Console correctly
- [ ] No input lag (<10ms)

---

### Task 3: Gamepad Support (gilrs) (6 hours)

**Files:**
- `crates/rustynes-desktop/src/input/gamepad.rs` (new)

**Objective:** Add gamepad detection and button mapping via gilrs.

#### 3.1 Gamepad Manager

```rust
// input/gamepad.rs
use gilrs::{Gilrs, Event, EventType, Button as GilrsButton, Axis};
use rustynes_core::Button as NesButton;
use super::ControllerState;

pub struct GamepadManager {
    gilrs: Gilrs,
    player1_id: Option<gilrs::GamepadId>,
    player2_id: Option<gilrs::GamepadId>,
    deadzone: f32,
}

impl GamepadManager {
    pub fn new() -> Result<Self, String> {
        let gilrs = Gilrs::new()
            .map_err(|e| format!("Failed to initialize gilrs: {}", e))?;

        Ok(Self {
            gilrs,
            player1_id: None,
            player2_id: None,
            deadzone: 0.2,
        })
    }

    pub fn poll(&mut self, player1: &mut ControllerState, player2: &mut ControllerState) {
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            // Auto-assign gamepads
            if self.player1_id.is_none() {
                self.player1_id = Some(id);
                log::info!("Gamepad {} assigned to Player 1", id);
            } else if self.player1_id != Some(id) && self.player2_id.is_none() {
                self.player2_id = Some(id);
                log::info!("Gamepad {} assigned to Player 2", id);
            }

            // Handle events
            match event {
                EventType::ButtonPressed(button, _) => {
                    if Some(id) == self.player1_id {
                        if let Some(nes_button) = Self::map_button(button) {
                            player1.set(nes_button, true);
                        }
                    } else if Some(id) == self.player2_id {
                        if let Some(nes_button) = Self::map_button(button) {
                            player2.set(nes_button, true);
                        }
                    }
                }

                EventType::ButtonReleased(button, _) => {
                    if Some(id) == self.player1_id {
                        if let Some(nes_button) = Self::map_button(button) {
                            player1.set(nes_button, false);
                        }
                    } else if Some(id) == self.player2_id {
                        if let Some(nes_button) = Self::map_button(button) {
                            player2.set(nes_button, false);
                        }
                    }
                }

                EventType::Disconnected => {
                    log::info!("Gamepad {} disconnected", id);
                    if self.player1_id == Some(id) {
                        self.player1_id = None;
                    } else if self.player2_id == Some(id) {
                        self.player2_id = None;
                    }
                }

                _ => {}
            }
        }

        // Handle analog sticks
        self.handle_analog_sticks(player1, player2);
    }

    fn map_button(button: GilrsButton) -> Option<NesButton> {
        match button {
            GilrsButton::South => Some(NesButton::B),       // Xbox A / PS Cross
            GilrsButton::East => Some(NesButton::A),        // Xbox B / PS Circle
            GilrsButton::Select => Some(NesButton::Select),
            GilrsButton::Start => Some(NesButton::Start),
            GilrsButton::DPadUp => Some(NesButton::Up),
            GilrsButton::DPadDown => Some(NesButton::Down),
            GilrsButton::DPadLeft => Some(NesButton::Left),
            GilrsButton::DPadRight => Some(NesButton::Right),
            _ => None,
        }
    }

    fn handle_analog_sticks(&mut self, player1: &mut ControllerState, player2: &mut ControllerState) {
        // Player 1
        if let Some(id) = self.player1_id {
            if let Some(gamepad) = self.gilrs.connected_gamepad(id) {
                let left_x = gamepad.value(Axis::LeftStickX);
                let left_y = gamepad.value(Axis::LeftStickY);

                player1.set(NesButton::Left, left_x < -self.deadzone);
                player1.set(NesButton::Right, left_x > self.deadzone);
                player1.set(NesButton::Up, left_y < -self.deadzone);
                player1.set(NesButton::Down, left_y > self.deadzone);
            }
        }

        // Player 2
        if let Some(id) = self.player2_id {
            if let Some(gamepad) = self.gilrs.connected_gamepad(id) {
                let left_x = gamepad.value(Axis::LeftStickX);
                let left_y = gamepad.value(Axis::LeftStickY);

                player2.set(NesButton::Left, left_x < -self.deadzone);
                player2.set(NesButton::Right, left_x > self.deadzone);
                player2.set(NesButton::Up, left_y < -self.deadzone);
                player2.set(NesButton::Down, left_y > self.deadzone);
            }
        }
    }

    pub fn gamepad_count(&self) -> usize {
        self.gilrs.gamepads().count()
    }
}

impl Default for GamepadManager {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            // Fallback if gilrs fails
            Self {
                gilrs: Gilrs::new().expect("gilrs should initialize"),
                player1_id: None,
                player2_id: None,
                deadzone: 0.2,
            }
        })
    }
}
```

#### 3.2 Poll Gamepads in Update Loop

```rust
// lib.rs (Message)
pub enum Message {
    // ... existing ...
    PollGamepads,
}

// lib.rs (Update)
impl RustyNesModel {
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PollGamepads => {
                if let Some(gamepad_mgr) = &mut self.gamepad_manager {
                    gamepad_mgr.poll(
                        &mut self.input_state.player1,
                        &mut self.input_state.player2,
                    );

                    if let Some(console) = &mut self.console {
                        self.input_state.apply_to_console(console);
                    }
                }

                iced::Task::none()
            }
            // ... other messages ...
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        // Poll gamepads every frame (16ms at 60 FPS)
        iced::time::every(std::time::Duration::from_millis(16))
            .map(|_| Message::PollGamepads)
            .merge(/* keyboard subscriptions */)
    }
}
```

**Acceptance Criteria:**
- [ ] Gamepad detection works (1-2 controllers)
- [ ] D-pad and face buttons mapped correctly
- [ ] Analog sticks function as D-pad (with deadzone)
- [ ] Multiple gamepads assigned to Player 1/2
- [ ] Disconnection handled gracefully

---

### Task 4: Input Testing & Polish (4 hours)

**Objective:** Verify input system works across platforms.

#### 4.1 Input Latency Test

```rust
// tests/input_latency.rs
#[test]
fn test_input_latency() {
    use std::time::Instant;

    let mut input_state = InputState::new();
    let start = Instant::now();

    // Simulate key press
    input_state.player1.set(NesButton::A, true);

    let latency = start.elapsed();
    assert!(latency.as_millis() < 1, "Input latency >1ms");
}
```

#### 4.2 Controller State Verification

```rust
#[test]
fn test_controller_state() {
    let mut state = ControllerState::default();

    state.set(NesButton::A, true);
    assert!(state.is_pressed(NesButton::A));

    state.set(NesButton::A, false);
    assert!(!state.is_pressed(NesButton::A));
}
```

**Acceptance Criteria:**
- [ ] Input latency <1ms (unit test)
- [ ] Controller state transitions verified
- [ ] Keyboard and gamepad tested on Linux/macOS/Windows
- [ ] No input conflicts or ghost inputs

---

## Part B: ROM Library Browser (20 hours)

### Task 5: ROM Discovery (6 hours)

**Files:**
- `crates/rustynes-desktop/src/library/mod.rs` (new)
- `crates/rustynes-desktop/src/library/scanner.rs` (new)

**Objective:** Scan file system for `.nes` ROM files.

#### 5.1 ROM Scanner

```rust
// library/scanner.rs
use std::path::{Path, PathBuf};
use std::fs;

#[derive(Debug, Clone)]
pub struct RomEntry {
    pub path: PathBuf,
    pub title: String,
    pub size: u64,
}

impl RomEntry {
    pub fn from_path(path: PathBuf) -> Self {
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let size = fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);

        Self { path, title, size }
    }
}

pub struct RomScanner;

impl RomScanner {
    pub fn scan_directory(dir: &Path) -> Vec<RomEntry> {
        let mut roms = Vec::new();

        if !dir.exists() || !dir.is_dir() {
            log::warn!("ROM directory does not exist: {}", dir.display());
            return roms;
        }

        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();

                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("nes") {
                        roms.push(RomEntry::from_path(path));
                    } else if path.is_dir() {
                        // Recursive scan (1 level deep)
                        roms.extend(Self::scan_directory(&path));
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to read ROM directory: {}", e);
            }
        }

        roms.sort_by(|a, b| a.title.cmp(&b.title));
        roms
    }
}
```

#### 5.2 Library State

```rust
// library/mod.rs
pub mod scanner;

use scanner::{RomScanner, RomEntry};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LibraryState {
    pub roms: Vec<RomEntry>,
    pub rom_directory: Option<PathBuf>,
    pub view_mode: ViewMode,
    pub search_query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    List,
}

impl LibraryState {
    pub fn new() -> Self {
        Self {
            roms: Vec::new(),
            rom_directory: None,
            view_mode: ViewMode::Grid,
            search_query: String::new(),
        }
    }

    pub fn scan_directory(&mut self, dir: PathBuf) {
        self.rom_directory = Some(dir.clone());
        self.roms = RomScanner::scan_directory(&dir);
        log::info!("Scanned {} ROMs from {}", self.roms.len(), dir.display());
    }

    pub fn filtered_roms(&self) -> Vec<&RomEntry> {
        if self.search_query.is_empty() {
            self.roms.iter().collect()
        } else {
            let query_lower = self.search_query.to_lowercase();
            self.roms
                .iter()
                .filter(|rom| rom.title.to_lowercase().contains(&query_lower))
                .collect()
        }
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Grid => ViewMode::List,
            ViewMode::List => ViewMode::Grid,
        };
    }
}

impl Default for LibraryState {
    fn default() -> Self {
        Self::new()
    }
}
```

**Acceptance Criteria:**
- [ ] Scans `.nes` files from directory
- [ ] Recursive scan (1 level deep)
- [ ] ROMs sorted alphabetically
- [ ] Handles missing directories gracefully
- [ ] Performance <100ms for 1000 ROMs

---

### Task 6: Library Views (Grid + List) (8 hours)

**Files:**
- `crates/rustynes-desktop/src/views/library.rs` (new)

**Objective:** Create Grid and List views for ROM library.

#### 6.1 Grid View

```rust
// views/library.rs
use iced::{Element, widget::{container, column, row, text, button, scrollable, text_input}};
use iced::{Length, Alignment};
use crate::library::{LibraryState, ViewMode, RomEntry};

pub fn view<'a>(library: &'a LibraryState) -> Element<'a, Message> {
    let search_bar = row![
        text_input("Search ROMs...", &library.search_query)
            .on_input(Message::LibrarySearch)
            .padding(10)
            .width(Length::Fill),

        button(if library.view_mode == ViewMode::Grid {
            "List View"
        } else {
            "Grid View"
        })
        .on_press(Message::ToggleLibraryView)
        .padding(10),
    ]
    .spacing(10)
    .padding(10);

    let content = match library.view_mode {
        ViewMode::Grid => grid_view(library),
        ViewMode::List => list_view(library),
    };

    column![
        search_bar,
        content,
    ]
    .into()
}

fn grid_view<'a>(library: &'a LibraryState) -> Element<'a, Message> {
    let roms = library.filtered_roms();

    if roms.is_empty() {
        return container(
            text("No ROMs found. Add .nes files to your ROM directory.")
                .size(16)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into();
    }

    // Grid layout (4 columns)
    let mut grid = column![].spacing(10).padding(10);
    let mut current_row = row![].spacing(10);
    let mut count = 0;

    for rom in roms {
        current_row = current_row.push(rom_card(rom));
        count += 1;

        if count % 4 == 0 {
            grid = grid.push(current_row);
            current_row = row![].spacing(10);
        }
    }

    // Add remaining items
    if count % 4 != 0 {
        grid = grid.push(current_row);
    }

    scrollable(grid).into()
}

fn rom_card<'a>(rom: &'a RomEntry) -> Element<'a, Message> {
    let rom_path = rom.path.clone();

    button(
        column![
            // Placeholder for cover art (future)
            container(text("🎮").size(48))
                .width(Length::Fixed(150.0))
                .height(Length::Fixed(150.0))
                .center_x()
                .center_y()
                .style(iced::theme::Container::Box),

            text(&rom.title)
                .size(14)
                .width(Length::Fixed(150.0))
                .horizontal_alignment(iced::alignment::Horizontal::Center),
        ]
        .align_items(Alignment::Center)
    )
    .on_press(Message::LoadRom(rom_path))
    .padding(10)
    .into()
}
```

#### 6.2 List View

```rust
// views/library.rs (continued)
fn list_view<'a>(library: &'a LibraryState) -> Element<'a, Message> {
    let roms = library.filtered_roms();

    if roms.is_empty() {
        return container(
            text("No ROMs found.")
                .size(16)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into();
    }

    let mut list = column![].spacing(2);

    // Header
    list = list.push(
        row![
            text("Title").width(Length::FillPortion(3)),
            text("Size").width(Length::FillPortion(1)),
        ]
        .padding(10)
        .style(iced::theme::Container::Box)
    );

    // ROM entries
    for rom in roms {
        let rom_path = rom.path.clone();

        list = list.push(
            button(
                row![
                    text(&rom.title).width(Length::FillPortion(3)),
                    text(format!("{:.2} MB", rom.size as f64 / 1_048_576.0))
                        .width(Length::FillPortion(1)),
                ]
                .padding(10)
            )
            .on_press(Message::LoadRom(rom_path))
            .style(iced::theme::Button::Secondary)
            .width(Length::Fill)
        );
    }

    scrollable(list).into()
}
```

**Acceptance Criteria:**
- [ ] Grid view displays ROMs in 4-column layout
- [ ] List view shows ROM title and size
- [ ] Search filters results in real-time
- [ ] View mode toggle functional
- [ ] Scrollable for 100+ ROMs
- [ ] Performance <16ms per frame

---

### Task 7: ROM Loading (4 hours)

**Files:**
- `crates/rustynes-desktop/src/lib.rs` (Message handling)

**Objective:** Load selected ROM from library.

#### 7.1 Message Handling

```rust
// lib.rs (Message)
#[derive(Debug, Clone)]
pub enum Message {
    // ... existing ...

    LibrarySearch(String),
    ToggleLibraryView,
    LoadRom(PathBuf),
    SelectRomDirectory,
    RomDirectorySelected(Option<PathBuf>),
}

// lib.rs (Update)
impl RustyNesModel {
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::LibrarySearch(query) => {
                self.library.set_search_query(query);
                iced::Task::none()
            }

            Message::ToggleLibraryView => {
                self.library.toggle_view_mode();
                iced::Task::none()
            }

            Message::LoadRom(path) => {
                match self.try_load_rom(&path) {
                    Ok(()) => {
                        self.current_view = View::Gameplay;
                        log::info!("ROM loaded: {}", path.display());
                    }
                    Err(e) => {
                        log::error!("Failed to load ROM: {}", e);
                        // TODO: Show error dialog
                    }
                }
                iced::Task::none()
            }

            Message::SelectRomDirectory => {
                iced::Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select ROM Directory")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::RomDirectorySelected
                )
            }

            Message::RomDirectorySelected(Some(dir)) => {
                self.library.scan_directory(dir);
                iced::Task::none()
            }

            Message::RomDirectorySelected(None) => {
                iced::Task::none()
            }

            // ... other messages ...
        }
    }

    fn try_load_rom(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let rom_data = std::fs::read(path)?;
        let rom = rustynes_core::Rom::from_bytes(&rom_data)?;

        self.console = Some(rustynes_core::Console::new(rom)?);
        self.current_rom_path = Some(path.to_path_buf());

        Ok(())
    }
}
```

**Acceptance Criteria:**
- [ ] Clicking ROM in library loads and starts emulation
- [ ] Switch to Gameplay view automatically
- [ ] Error handling for invalid ROMs
- [ ] ROM directory selection dialog functional
- [ ] Library rescans after directory selection

---

### Task 8: Integration & Testing (2 hours)

**Objective:** Verify full input + library workflow.

#### 8.1 End-to-End Test

```bash
# Manual test checklist:
1. Launch app → Library view visible
2. Click "Select ROM Directory" → Choose folder with .nes files
3. Library populates with ROM list
4. Toggle Grid ↔ List view
5. Search for ROM by name
6. Double-click ROM → Gameplay view loads
7. Test keyboard input (Arrow keys, Z/X)
8. Test gamepad input (if available)
9. Verify input responsive (<10ms latency)
```

**Acceptance Criteria:**
- [ ] Full workflow functional (library → gameplay → input)
- [ ] No crashes or panics
- [ ] Input works in Gameplay view
- [ ] Library persists selected directory (config in M6-S4)

---

## Acceptance Criteria

### Input System

- [ ] Keyboard input works (Player 1 & 2)
- [ ] Gamepad detection and mapping functional
- [ ] Analog sticks work as D-pad
- [ ] Input latency <10ms
- [ ] Two-player support verified
- [ ] Zero unsafe code

### ROM Library

- [ ] ROM scanner finds all `.nes` files
- [ ] Grid view (4 columns) renders correctly
- [ ] List view shows title and size
- [ ] Search/filter functional
- [ ] ROM loading works
- [ ] View mode toggle responsive
- [ ] Handles 100+ ROMs smoothly

### Integration

- [ ] Library → Gameplay transition seamless
- [ ] Input works immediately after ROM load
- [ ] No performance degradation with large libraries
- [ ] All features tested on Linux/macOS/Windows

---

## Dependencies

### External Crates

```toml
[dependencies]
gilrs = "0.10"
rfd = "0.14"  # Async file dialogs
```

---

## Related Documentation

- [M6-S1-iced-application.md](M6-S1-iced-application.md) - Iced application foundation
- [M6-S2-wgpu-rendering.md](M6-S2-wgpu-rendering.md) - wgpu rendering
- [M6-OVERVIEW.md](M6-OVERVIEW.md) - Milestone overview
- [docs/input/INPUT_HANDLING.md](../../../docs/input/INPUT_HANDLING.md) - NES controller protocol

---

## Performance Targets

- **Input Latency:** <10ms (keyboard + gamepad)
- **ROM Scan:** <100ms for 1000 ROMs
- **Library Render:** <16ms per frame
- **Grid View:** Smooth scrolling with 200+ ROMs
- **Search:** Real-time filtering (<5ms)

---

## Success Criteria

- [ ] All tasks complete (Input + Library)
- [ ] Keyboard and gamepad input functional
- [ ] ROM library browser complete
- [ ] Grid and List views working
- [ ] Search/filter responsive
- [ ] ROM loading tested with 10+ games
- [ ] Zero unsafe code confirmed
- [ ] Ready for Settings + Persistence (M6-S4)

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1 (Iced Application), M6-S2 (wgpu Rendering)
**Next Sprint:** [M6-S4 Settings + Persistence](M6-S4-settings-persistence.md)
