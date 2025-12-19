# [Milestone 6] Sprint 6.1: egui Application Structure

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week
**Assignee:** Claude Code / Developer
**Sprint:** M6-S1 (GUI - Application Shell)
**Progress:** 0%

---

## Overview

This sprint establishes the **egui application shell** for the RustyNES desktop frontend, creating the window, menu bar, main viewport, and application lifecycle management. This provides the foundation for rendering, audio, and input in subsequent sprints.

### Goals

- ⏳ eframe application window
- ⏳ Menu bar (File, Emulation, Settings, Help)
- ⏳ File dialog for ROM loading
- ⏳ Main viewport for game screen
- ⏳ Status bar (FPS, emulation status)
- ⏳ Keyboard shortcuts
- ⏳ Basic application state management
- ⏳ Zero unsafe code

### Prerequisites

- ✅ M5 Complete (rustynes-core crate exists)
- ✅ Console API available

---

## Tasks

### Task 1: Project Setup (2 hours)

**File:** `crates/rustynes-desktop/Cargo.toml`

**Objective:** Create desktop frontend crate with dependencies.

#### Subtasks

1. Create `rustynes-desktop` crate
2. Add dependencies:
   - eframe (egui application framework)
   - egui (immediate mode GUI)
   - rfd (native file dialogs)
   - log, env_logger
   - rustynes-core (workspace dependency)

3. Create basic file structure
4. Set up workspace integration

**Acceptance Criteria:**

- [ ] Crate builds successfully
- [ ] All dependencies resolve
- [ ] Workspace integration works

**Implementation:**

```toml
[package]
name = "rustynes-desktop"
version = "0.1.0"
edition = "2021"
authors = ["RustyNES Contributors"]
description = "Desktop frontend for RustyNES emulator"
license = "MIT OR Apache-2.0"

[[bin]]
name = "rustynes"
path = "src/main.rs"

[dependencies]
# GUI Framework
eframe = "0.24"
egui = "0.24"

# File Dialogs
rfd = "0.12"

# Logging
log = "0.4"
env_logger = "0.10"

# Emulator Core
rustynes-core = { path = "../rustynes-core" }

[target.'cfg(target_os = "windows")'.dependencies]
winapi = { version = "0.3", features = ["winuser"] }
```

**File Structure:**

```text
crates/rustynes-desktop/
├── src/
│   ├── main.rs          # Entry point
│   ├── app.rs           # Main application struct
│   ├── renderer.rs      # (Sprint 2)
│   ├── audio.rs         # (Sprint 3)
│   ├── input.rs         # (Sprint 4)
│   ├── config.rs        # (Sprint 5)
│   └── ui/
│       ├── mod.rs
│       ├── menu_bar.rs
│       ├── status_bar.rs
│       └── dialogs.rs   # (Sprint 5)
├── assets/
│   └── icon.png
└── Cargo.toml
```

---

### Task 2: Application Structure (3 hours)

**File:** `crates/rustynes-desktop/src/app.rs`

**Objective:** Define main application struct and state management.

#### Subtasks

1. Create `RustyNesApp` struct with:
   - Optional `Console` (None until ROM loaded)
   - Emulation state (running, paused, stopped)
   - FPS counter
   - Configuration (minimal for now)

2. Implement `eframe::App` trait
3. Add ROM loading logic
4. Frame timing and FPS calculation

**Acceptance Criteria:**

- [ ] App struct manages emulation lifecycle
- [ ] FPS calculation is accurate
- [ ] ROM loading works from file path
- [ ] Clean separation of concerns

**Implementation:**

```rust
use eframe::egui;
use rustynes_core::{Console, Button};
use std::path::PathBuf;
use std::time::Instant;

/// Main application state
pub struct RustyNesApp {
    /// Emulator console (None if no ROM loaded)
    console: Option<Console>,

    /// Emulation state
    state: EmulationState,

    /// FPS tracking
    fps_counter: FpsCounter,

    /// Last ROM path (for window title)
    rom_path: Option<PathBuf>,

    /// Frame timing
    last_frame: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationState {
    NoRom,
    Running,
    Paused,
}

impl RustyNesApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure fonts (optional)
        Self::configure_fonts(&cc.egui_ctx);

        Self {
            console: None,
            state: EmulationState::NoRom,
            fps_counter: FpsCounter::new(),
            rom_path: None,
            last_frame: Instant::now(),
        }
    }

    fn configure_fonts(ctx: &egui::Context) {
        // Optional: Load custom fonts
        let mut fonts = egui::FontDefinitions::default();
        // ... font configuration ...
        ctx.set_fonts(fonts);
    }

    /// Load ROM from file path
    pub fn load_rom(&mut self, path: PathBuf) -> Result<(), String> {
        let rom_data = std::fs::read(&path)
            .map_err(|e| format!("Failed to read ROM: {}", e))?;

        let console = Console::from_rom_bytes(&rom_data)
            .map_err(|e| format!("Failed to load ROM: {}", e))?;

        self.console = Some(console);
        self.state = EmulationState::Paused;
        self.rom_path = Some(path);

        log::info!("ROM loaded successfully");
        Ok(())
    }

    /// Reset emulation
    pub fn reset(&mut self) {
        if let Some(console) = &mut self.console {
            console.reset();
            log::info!("Emulation reset");
        }
    }

    /// Toggle pause state
    pub fn toggle_pause(&mut self) {
        match self.state {
            EmulationState::Running => {
                self.state = EmulationState::Paused;
                log::info!("Emulation paused");
            }
            EmulationState::Paused => {
                self.state = EmulationState::Running;
                log::info!("Emulation resumed");
            }
            EmulationState::NoRom => {}
        }
    }

    /// Step single frame (for debugging)
    pub fn step_frame(&mut self) {
        if let Some(console) = &mut self.console {
            console.step_frame();
            self.fps_counter.tick();
        }
    }

    /// Get window title
    pub fn window_title(&self) -> String {
        if let Some(path) = &self.rom_path {
            let rom_name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            format!("RustyNES - {}", rom_name)
        } else {
            "RustyNES".to_string()
        }
    }
}

impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Update window title
        frame.set_window_title(&self.window_title());

        // UI will be built in subsequent tasks

        // Request continuous repaint for smooth emulation
        ctx.request_repaint();
    }
}

/// FPS counter
struct FpsCounter {
    frame_times: Vec<Instant>,
    window: usize, // Number of frames to average
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            frame_times: Vec::with_capacity(60),
            window: 60, // 1 second at 60 FPS
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.frame_times.push(now);

        // Keep only last N frames
        if self.frame_times.len() > self.window {
            self.frame_times.remove(0);
        }
    }

    fn fps(&self) -> f64 {
        if self.frame_times.len() < 2 {
            return 0.0;
        }

        let duration = self.frame_times.last().unwrap()
            .duration_since(*self.frame_times.first().unwrap());

        let frames = self.frame_times.len() as f64 - 1.0;
        frames / duration.as_secs_f64()
    }

    fn frame_time_ms(&self) -> f64 {
        if self.frame_times.len() < 2 {
            return 0.0;
        }

        let fps = self.fps();
        if fps > 0.0 {
            1000.0 / fps
        } else {
            0.0
        }
    }
}
```

---

### Task 3: Menu Bar (2 hours)

**File:** `crates/rustynes-desktop/src/ui/menu_bar.rs`

**Objective:** Create menu bar with File, Emulation, Settings, Help menus.

#### Subtasks

1. File menu:
   - Open ROM... (Ctrl+O)
   - Recent ROMs (placeholder)
   - Exit (Ctrl+Q)

2. Emulation menu:
   - Reset (Ctrl+R)
   - Pause/Resume (Ctrl+P)
   - Step Frame (Ctrl+N) - for debugging

3. Settings menu:
   - Video settings (Sprint 2)
   - Audio settings (Sprint 3)
   - Input settings (Sprint 4)

4. Help menu:
   - About

**Acceptance Criteria:**

- [ ] All menus render correctly
- [ ] Keyboard shortcuts work
- [ ] Menu items enable/disable based on state
- [ ] File dialog opens on "Open ROM"

**Implementation:**

```rust
use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;

use crate::app::{RustyNesApp, EmulationState};

impl RustyNesApp {
    pub fn render_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            self.file_menu(ui);
            self.emulation_menu(ui);
            self.settings_menu(ui);
            self.help_menu(ui);
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            // Open ROM
            if ui.add(egui::Button::new("Open ROM...")
                .shortcut_text("Ctrl+O"))
                .clicked()
            {
                self.open_rom_dialog();
                ui.close_menu();
            }

            // Recent ROMs (placeholder)
            ui.menu_button("Recent ROMs", |ui| {
                ui.label("(No recent ROMs)");
            });

            ui.separator();

            // Exit
            if ui.add(egui::Button::new("Exit")
                .shortcut_text("Ctrl+Q"))
                .clicked()
            {
                std::process::exit(0);
            }
        });
    }

    fn emulation_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Emulation", |ui| {
            let has_rom = self.console.is_some();

            // Reset
            if ui.add_enabled(has_rom,
                egui::Button::new("Reset").shortcut_text("Ctrl+R"))
                .clicked()
            {
                self.reset();
                ui.close_menu();
            }

            // Pause/Resume
            let pause_text = match self.state {
                EmulationState::Running => "Pause",
                EmulationState::Paused => "Resume",
                EmulationState::NoRom => "Pause",
            };

            if ui.add_enabled(has_rom,
                egui::Button::new(pause_text).shortcut_text("Ctrl+P"))
                .clicked()
            {
                self.toggle_pause();
                ui.close_menu();
            }

            ui.separator();

            // Step Frame (debug feature)
            if ui.add_enabled(has_rom && self.state == EmulationState::Paused,
                egui::Button::new("Step Frame").shortcut_text("Ctrl+N"))
                .clicked()
            {
                self.step_frame();
                ui.close_menu();
            }
        });
    }

    fn settings_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Settings", |ui| {
            ui.label("Video settings (Sprint 2)");
            ui.label("Audio settings (Sprint 3)");
            ui.label("Input settings (Sprint 4)");
        });
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Help", |ui| {
            if ui.button("About").clicked() {
                self.show_about_dialog();
                ui.close_menu();
            }
        });
    }

    fn open_rom_dialog(&mut self) {
        if let Some(path) = FileDialog::new()
            .add_filter("NES ROM", &["nes"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            if let Err(e) = self.load_rom(path) {
                log::error!("Failed to load ROM: {}", e);
                // TODO: Show error dialog (Sprint 5)
            }
        }
    }

    fn show_about_dialog(&self) {
        // TODO: Implement About dialog (Sprint 5)
        log::info!("About: RustyNES v{}", env!("CARGO_PKG_VERSION"));
    }

    /// Handle keyboard shortcuts
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+O: Open ROM
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL, egui::Key::O
        ))) {
            self.open_rom_dialog();
        }

        // Ctrl+Q: Quit
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL, egui::Key::Q
        ))) {
            std::process::exit(0);
        }

        // Ctrl+R: Reset
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL, egui::Key::R
        ))) {
            self.reset();
        }

        // Ctrl+P: Pause/Resume
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL, egui::Key::P
        ))) {
            self.toggle_pause();
        }

        // Ctrl+N: Step Frame
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(
            egui::Modifiers::CTRL, egui::Key::N
        ))) && self.state == EmulationState::Paused {
            self.step_frame();
        }
    }
}
```

---

### Task 4: Main Viewport (2 hours)

**File:** `crates/rustynes-desktop/src/app.rs` (continued)

**Objective:** Create central panel for game rendering.

#### Subtasks

1. Create `CentralPanel` for game viewport
2. Show "No ROM loaded" message when no ROM
3. Allocate space for game screen (256×240)
4. Prepare for wgpu rendering (Sprint 2)

**Acceptance Criteria:**

- [ ] Central panel renders correctly
- [ ] Viewport scales with window size
- [ ] Maintains aspect ratio (4:3 or pixel-perfect)
- [ ] Placeholder shown when no ROM

**Implementation:**

```rust
impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Update window title
        frame.set_window_title(&self.window_title());

        // Handle keyboard shortcuts
        self.handle_shortcuts(ctx);

        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.render_menu_bar(ui);
        });

        // Status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.render_status_bar(ui);
        });

        // Main viewport
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.console.is_some() {
                // Emulator running
                self.render_game_viewport(ui);

                // Step frame if running
                if self.state == EmulationState::Running {
                    self.step_frame();
                }
            } else {
                // No ROM loaded
                self.render_no_rom_screen(ui);
            }
        });

        // Request continuous repaint for smooth emulation
        ctx.request_repaint();
    }
}

impl RustyNesApp {
    fn render_game_viewport(&mut self, ui: &mut egui::Ui) {
        // Allocate space for game screen
        // NES resolution: 256×240
        // Use 4:3 aspect ratio for now

        let available = ui.available_size();

        // Calculate viewport size maintaining aspect ratio
        let aspect_ratio = 256.0 / 240.0; // ~1.067 (NES native)
        let display_aspect = 4.0 / 3.0;   // Classic CRT aspect ratio

        let (width, height) = if available.x / available.y > display_aspect {
            // Width-constrained
            (available.y * display_aspect, available.y)
        } else {
            // Height-constrained
            (available.x, available.x / display_aspect)
        };

        // Center the viewport
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                (available.x - width) / 2.0,
                (available.y - height) / 2.0,
            ),
            egui::vec2(width, height),
        );

        // Placeholder for wgpu rendering (Sprint 2)
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);

        // Show message
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("Game rendering (Sprint 2)")
                    .color(egui::Color32::WHITE)
                    .size(20.0)
            );
        });
    }

    fn render_no_rom_screen(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("No ROM loaded")
                        .size(24.0)
                        .strong()
                );
                ui.add_space(10.0);
                ui.label("File → Open ROM to begin");
                ui.add_space(5.0);
                ui.label("or press Ctrl+O");
            });
        });
    }
}
```

---

### Task 5: Status Bar (1 hour)

**File:** `crates/rustynes-desktop/src/ui/status_bar.rs`

**Objective:** Create bottom status bar showing FPS and emulation state.

#### Subtasks

1. Display FPS (frames per second)
2. Display frame time (milliseconds)
3. Display emulation state (Running/Paused/No ROM)
4. Display ROM name

**Acceptance Criteria:**

- [ ] Status bar shows accurate FPS
- [ ] Updates in real-time
- [ ] Clean, readable layout

**Implementation:**

```rust
use eframe::egui;
use crate::app::{RustyNesApp, EmulationState};

impl RustyNesApp {
    pub fn render_status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Emulation state
            let state_text = match self.state {
                EmulationState::NoRom => "No ROM",
                EmulationState::Running => "Running",
                EmulationState::Paused => "Paused",
            };

            let state_color = match self.state {
                EmulationState::Running => egui::Color32::GREEN,
                EmulationState::Paused => egui::Color32::YELLOW,
                EmulationState::NoRom => egui::Color32::GRAY,
            };

            ui.label(
                egui::RichText::new(state_text)
                    .color(state_color)
                    .strong()
            );

            ui.separator();

            // FPS
            if self.console.is_some() {
                ui.label(format!("FPS: {:.1}", self.fps_counter.fps()));
                ui.separator();
                ui.label(format!("Frame: {:.2}ms", self.fps_counter.frame_time_ms()));
            }

            // Spacer
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // ROM name
                if let Some(path) = &self.rom_path {
                    let rom_name = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown");
                    ui.label(
                        egui::RichText::new(rom_name)
                            .italics()
                    );
                }
            });
        });
    }
}
```

---

### Task 6: Main Entry Point (1 hour)

**File:** `crates/rustynes-desktop/src/main.rs`

**Objective:** Create application entry point with eframe setup.

#### Subtasks

1. Initialize logging
2. Configure eframe options (window size, icon, vsync)
3. Run application

**Acceptance Criteria:**

- [ ] Application starts and shows window
- [ ] Logging works
- [ ] Window has correct initial size
- [ ] Icon displayed (if provided)

**Implementation:**

```rust
mod app;
mod ui;

use app::RustyNesApp;

fn main() -> Result<(), eframe::Error> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    log::info!("Starting RustyNES v{}", env!("CARGO_PKG_VERSION"));

    // Configure eframe options
    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(1024.0, 768.0)),
        min_window_size: Some(egui::vec2(512.0, 480.0)),
        resizable: true,
        vsync: true,
        multisampling: 0,
        ..Default::default()
    };

    // Run application
    eframe::run_native(
        "RustyNES",
        options,
        Box::new(|cc| Box::new(RustyNesApp::new(cc))),
    )
}
```

**File:** `crates/rustynes-desktop/src/ui/mod.rs`

```rust
pub mod menu_bar;
pub mod status_bar;
```

---

## Acceptance Criteria

### Functionality

- [ ] Application window opens
- [ ] Menu bar renders with all menus
- [ ] File dialog opens and loads ROMs
- [ ] Main viewport shows placeholder
- [ ] Status bar displays FPS and state
- [ ] Keyboard shortcuts work
- [ ] Pause/Resume works
- [ ] Reset works
- [ ] Window title updates with ROM name

### User Experience

- [ ] Intuitive menu layout
- [ ] Responsive UI (no lag)
- [ ] Clear visual feedback for states
- [ ] Keyboard shortcuts are discoverable
- [ ] Window resizes smoothly

### Quality

- [ ] Zero unsafe code
- [ ] No crashes on invalid ROMs
- [ ] Clean shutdown
- [ ] Logging is informative

---

## Dependencies

### External Crates

```toml
eframe = "0.24"       # Application framework
egui = "0.24"         # Immediate mode GUI
rfd = "0.12"          # Native file dialogs
log = "0.4"           # Logging facade
env_logger = "0.10"   # Logger implementation
```

### Internal Dependencies

- rustynes-core (Console API)

---

## Related Documentation

- [M6-OVERVIEW.md](M6-OVERVIEW.md) - Milestone overview
- [DESKTOP.md](../../../docs/platform/DESKTOP.md) - Desktop platform guide
- [CORE_API.md](../../../docs/api/CORE_API.md) - Console API

---

## Technical Notes

### egui Immediate Mode

egui uses **immediate mode** rendering, meaning the UI is rebuilt every frame. This simplifies state management but requires careful performance consideration.

**Best Practices:**

- Minimize allocations in `update()`
- Use `ui.ctx().request_repaint()` to control frame rate
- Cache expensive computations

### File Dialogs

`rfd` provides native file dialogs on all platforms:

- Windows: Win32 native dialog
- macOS: Cocoa native dialog
- Linux: GTK/KDE native dialog

### Application Icon

To set application icon, place `icon.png` in `assets/` and configure in `eframe::NativeOptions`:

```rust
let icon_data = include_bytes!("../assets/icon.png");
let icon = load_icon(icon_data);

let options = eframe::NativeOptions {
    icon_data: Some(icon),
    // ...
};
```

---

## Performance Targets

- **UI Frame Time:** <16ms (60 FPS)
- **Startup Time:** <500ms
- **Memory:** <50 MB (before ROM load)

---

## Success Criteria

- [ ] All tasks complete
- [ ] Application runs on Linux, Windows, macOS
- [ ] Clean, professional UI
- [ ] No crashes or panics
- [ ] Keyboard shortcuts work
- [ ] Ready for wgpu rendering integration (Sprint 2)

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M5 (rustynes-core)
**Next Sprint:** [M6-S2 wgpu Rendering](M6-S2-wgpu-rendering.md)
