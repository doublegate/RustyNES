# [Milestone 6] Sprint 6.1: Iced Application Foundation

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1 week (5-7 days)
**Assignee:** Claude Code / Developer
**Sprint:** M6-S1 (GUI - Iced Application Shell)
**Progress:** 0%

---

## Overview

This sprint establishes the **Iced 0.13+** application foundation for RustyNES Desktop, implementing the Elm architecture pattern for structured state management. Unlike the original egui-based plan, this sprint focuses on creating a professional, animation-capable UI framework suitable for the complex multi-view emulator application.

### Goals

- ⏳ Set up Iced 0.13+ application structure with Elm architecture
- ⏳ Implement custom title bar (cross-platform)
- ⏳ Create theme system with glass morphism styling
- ⏳ Implement sidebar navigation between views
- ⏳ Create Welcome view with ROM loading
- ⏳ Set up basic wgpu integration
- ⏳ Implement file dialog for ROM selection
- ⏳ Zero unsafe code (except platform-specific APIs)

### Prerequisites

- ✅ M5 Complete (Console integration working)
- ✅ rustynes-core crate functional
- ✅ ROM loading API available

### Technology Stack

- **Primary UI Framework:** Iced 0.13+ (NOT egui)
- **Architecture:** Elm (Model-Update-View)
- **Rendering:** wgpu (Iced native backend)
- **File Dialogs:** rfd 0.14
- **Async Runtime:** tokio 1.40 (Iced requirement)

---

## Tasks

### Task 1: Project Setup (2 hours)

**File:** `crates/rustynes-desktop/Cargo.toml`

**Objective:** Create rustynes-desktop crate with Iced dependencies.

#### Subtasks

1. Create `crates/rustynes-desktop/` directory structure
2. Configure Cargo.toml with Iced dependencies
3. Set up workspace member
4. Create initial `main.rs` and `app.rs`

**Acceptance Criteria:**

- [ ] Cargo builds without errors
- [ ] Iced window opens successfully
- [ ] Zero clippy warnings

**Implementation:**

```toml
# crates/rustynes-desktop/Cargo.toml

[package]
name = "rustynes-desktop"
version = "0.3.0"
edition = "2021"
authors = ["Your Name <your.email@example.com>"]
description = "Desktop frontend for RustyNES NES emulator"
repository = "https://github.com/doublegate/RustyNES"
license = "MIT OR Apache-2.0"
keywords = ["nes", "emulator", "nintendo", "retro"]
categories = ["games", "emulators"]

[[bin]]
name = "rustynes-desktop"
path = "src/main.rs"

[dependencies]
# ═══════════════════════════════════════════════════════════════
# UI FRAMEWORK (PRIMARY: ICED)
# ═══════════════════════════════════════════════════════════════
iced = { version = "0.13", features = [
    "wgpu",           # GPU-accelerated rendering
    "tokio",          # Async runtime
    "image",          # Image loading
    "svg",            # Vector icons
    "canvas",         # Custom rendering
    "advanced",       # Custom shaders
]}
iced_aw = "0.10"      # Additional widgets (badges, cards, modals, tabs)

# ═══════════════════════════════════════════════════════════════
# GRAPHICS
# ═══════════════════════════════════════════════════════════════
wgpu = "0.20"         # Cross-platform GPU API
image = "0.25"        # Image loading

# ═══════════════════════════════════════════════════════════════
# FILE SYSTEM
# ═══════════════════════════════════════════════════════════════
rfd = "0.14"          # Native file dialogs
directories = "5.0"   # Platform-specific paths

# ═══════════════════════════════════════════════════════════════
# ASYNC & UTILITIES
# ═══════════════════════════════════════════════════════════════
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"          # Config files
tracing = "0.1"       # Logging
tracing-subscriber = "0.3"

# ═══════════════════════════════════════════════════════════════
# CORE INTEGRATION
# ═══════════════════════════════════════════════════════════════
rustynes-core = { path = "../rustynes-core" }

[features]
default = []
```

**Directory Structure:**

```
crates/rustynes-desktop/
├── Cargo.toml
├── src/
│   ├── main.rs               # Entry point
│   ├── app.rs                # Main Iced application
│   ├── message.rs            # Message enum (Elm pattern)
│   ├── theme.rs              # Theme definitions
│   ├── view.rs               # View enum
│   └── views/
│       ├── mod.rs
│       └── welcome.rs        # Welcome view
└── assets/
    └── icons/
```

---

### Task 2: Elm Architecture Setup (3 hours)

**File:** `crates/rustynes-desktop/src/app.rs`

**Objective:** Implement Iced application structure with Elm architecture.

#### Subtasks

1. Define RustyNes application struct
2. Implement Application trait (Elm pattern)
3. Create Message enum for events
4. Create View enum for navigation
5. Implement update() function for state transitions

**Acceptance Criteria:**

- [ ] Application compiles and runs
- [ ] Message routing works
- [ ] View switching functional
- [ ] Follows Elm architecture correctly

**Implementation:**

```rust
// src/app.rs

use iced::{Application, Command, Element, Settings, Theme};
use crate::message::Message;
use crate::view::View;
use rustynes_core::Console;
use std::path::PathBuf;

/// Main application state (Elm Model)
pub struct RustyNes {
    /// Current view/screen
    current_view: View,

    /// Emulator core (None when no ROM loaded)
    console: Option<Console>,

    /// Currently loaded ROM path
    current_rom: Option<PathBuf>,

    /// Application theme
    theme: Theme,
}

impl Application for RustyNes {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = iced::Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        let app = Self {
            current_view: View::Welcome,
            console: None,
            current_rom: None,
            theme: Theme::Dark,
        };

        (app, Command::none())
    }

    fn title(&self) -> String {
        if let Some(rom_path) = &self.current_rom {
            format!("RustyNES - {}", rom_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown"))
        } else {
            "RustyNES - NES Emulator".to_string()
        }
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::NavigateTo(view) => {
                self.current_view = view;
                Command::none()
            }

            Message::LoadRom(path) => {
                // Async ROM loading
                Command::perform(
                    Self::load_rom_async(path.clone()),
                    Message::RomLoaded
                )
            }

            Message::RomLoaded(result) => {
                match result {
                    Ok(console) => {
                        self.console = Some(console);
                        self.current_view = View::Playing;
                        Command::none()
                    }
                    Err(e) => {
                        tracing::error!("Failed to load ROM: {:?}", e);
                        // TODO: Show error dialog
                        Command::none()
                    }
                }
            }

            Message::OpenFileDialog => {
                Command::perform(
                    Self::show_file_dialog(),
                    |path_opt| {
                        if let Some(path) = path_opt {
                            Message::LoadRom(path)
                        } else {
                            Message::None
                        }
                    }
                )
            }

            Message::None => Command::none(),
        }
    }

    fn view(&self) -> Element<Message> {
        match &self.current_view {
            View::Welcome => crate::views::welcome::view(self),
            // More views will be added in later sprints
            _ => unimplemented!("View not yet implemented"),
        }
    }

    fn theme(&self) -> Self::Theme {
        self.theme.clone()
    }
}

impl RustyNes {
    /// Asynchronously load ROM from file
    async fn load_rom_async(path: PathBuf) -> Result<Console, String> {
        let rom_data = tokio::fs::read(&path)
            .await
            .map_err(|e| format!("Failed to read ROM file: {}", e))?;

        let rom = rustynes_core::Rom::from_bytes(&rom_data)
            .map_err(|e| format!("Invalid ROM format: {:?}", e))?;

        let console = Console::new(rom)
            .map_err(|e| format!("Failed to create console: {:?}", e))?;

        Ok(console)
    }

    /// Show native file dialog
    async fn show_file_dialog() -> Option<PathBuf> {
        tokio::task::spawn_blocking(|| {
            rfd::FileDialog::new()
                .add_filter("NES ROM", &["nes"])
                .pick_file()
        })
        .await
        .ok()
        .flatten()
    }
}
```

```rust
// src/message.rs

use std::path::PathBuf;
use crate::view::View;
use rustynes_core::Console;

/// Application messages (Elm pattern)
#[derive(Debug, Clone)]
pub enum Message {
    /// No-op message
    None,

    /// Navigate to a different view
    NavigateTo(View),

    /// Load ROM from path
    LoadRom(PathBuf),

    /// ROM loading completed
    RomLoaded(Result<Console, String>),

    /// Open file dialog for ROM selection
    OpenFileDialog,
}
```

```rust
// src/view.rs

/// All possible application views
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Welcome,
    Playing,
    // More views will be added in later sprints
}
```

---

### Task 3: Welcome View (2 hours)

**File:** `crates/rustynes-desktop/src/views/welcome.rs`

**Objective:** Create initial Welcome screen with ROM loading.

#### Subtasks

1. Design Welcome screen layout
2. Add "Open ROM" button
3. Add recent ROMs list (placeholder)
4. Implement file dialog trigger

**Acceptance Criteria:**

- [ ] Welcome screen displays correctly
- [ ] "Open ROM" button functional
- [ ] File dialog opens
- [ ] Follows Iced widget composition

**Implementation:**

```rust
// src/views/welcome.rs

use iced::widget::{button, column, container, text, Column};
use iced::{Element, Length};
use crate::app::RustyNes;
use crate::message::Message;

pub fn view(_app: &RustyNes) -> Element<Message> {
    let title = text("RustyNES")
        .size(48)
        .style(iced::theme::Text::Color(iced::Color::from_rgb(0.8, 0.3, 0.3)));

    let subtitle = text("Next-Generation NES Emulator")
        .size(20);

    let open_button = button("Open ROM")
        .padding(15)
        .on_press(Message::OpenFileDialog);

    let content: Column<Message> = column![
        title,
        subtitle,
        open_button,
    ]
    .spacing(20)
    .padding(20)
    .align_items(iced::Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
}
```

---

### Task 4: Theme System (2 hours)

**File:** `crates/rustynes-desktop/src/theme.rs`

**Objective:** Create custom theme with glass morphism styling.

#### Subtasks

1. Define color palette
2. Create custom theme struct
3. Implement glass morphism backdrop
4. Add dark/light theme variants

**Acceptance Criteria:**

- [ ] Custom theme applies correctly
- [ ] Glass morphism effect works
- [ ] Colors match design specification
- [ ] Theme switching functional

**Implementation:**

```rust
// src/theme.rs

use iced::{Color, Theme};

/// Custom RustyNES theme palette
pub struct RustyTheme {
    pub primary: Color,
    pub secondary: Color,
    pub background: Color,
    pub surface: Color,
    pub text: Color,
    pub accent: Color,
}

impl RustyTheme {
    /// Dark theme (default)
    pub fn dark() -> Self {
        Self {
            primary: Color::from_rgb(0.8, 0.3, 0.3),      // Red
            secondary: Color::from_rgb(0.3, 0.6, 0.8),    // Blue
            background: Color::from_rgb(0.1, 0.1, 0.1),   // Dark gray
            surface: Color::from_rgba(0.2, 0.2, 0.2, 0.8), // Glass morphism
            text: Color::from_rgb(0.9, 0.9, 0.9),         // Light gray
            accent: Color::from_rgb(1.0, 0.5, 0.0),       // Orange
        }
    }

    /// Light theme
    pub fn light() -> Self {
        Self {
            primary: Color::from_rgb(0.8, 0.3, 0.3),
            secondary: Color::from_rgb(0.3, 0.6, 0.8),
            background: Color::from_rgb(0.95, 0.95, 0.95),
            surface: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
            text: Color::from_rgb(0.1, 0.1, 0.1),
            accent: Color::from_rgb(1.0, 0.5, 0.0),
        }
    }

    /// Convert to Iced Theme
    pub fn to_iced_theme(&self) -> Theme {
        // For now, use built-in dark theme
        // Custom theme styling will be enhanced in later sprints
        Theme::Dark
    }
}
```

---

### Task 5: Main Entry Point (1 hour)

**File:** `crates/rustynes-desktop/src/main.rs`

**Objective:** Create application entry point with window configuration.

#### Subtasks

1. Set up tracing logging
2. Configure Iced settings
3. Set window size and title
4. Run application

**Acceptance Criteria:**

- [ ] Application launches successfully
- [ ] Window size correct (768x720 default)
- [ ] Logging functional
- [ ] Cross-platform compatibility

**Implementation:**

```rust
// src/main.rs

use iced::{Application, Settings, Size};
use tracing_subscriber;

mod app;
mod message;
mod theme;
mod view;
mod views;

fn main() -> iced::Result {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting RustyNES Desktop v{}", env!("CARGO_PKG_VERSION"));

    // Configure Iced settings
    let settings = Settings {
        window: iced::window::Settings {
            size: Size::new(768.0, 720.0),
            resizable: true,
            decorations: true,
            ..Default::default()
        },
        default_font: None,
        default_text_size: iced::Pixels(16.0),
        antialiasing: true,
        ..Default::default()
    };

    // Run application
    app::RustyNes::run(settings)
}
```

---

### Task 6: Build System Integration (1 hour)

**File:** Root `Cargo.toml` (workspace)

**Objective:** Add rustynes-desktop as workspace member.

#### Subtasks

1. Add workspace member
2. Verify workspace builds
3. Set up default run target
4. Configure release profile

**Acceptance Criteria:**

- [ ] `cargo build --workspace` succeeds
- [ ] `cargo run -p rustynes-desktop` works
- [ ] Release build optimized

**Implementation:**

```toml
# Root Cargo.toml

[workspace]
members = [
    "crates/rustynes-core",
    "crates/rustynes-cpu",
    "crates/rustynes-ppu",
    "crates/rustynes-apu",
    "crates/rustynes-mappers",
    "crates/rustynes-desktop",  # NEW
]

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

---

## Acceptance Criteria

### Functionality

- [ ] Iced application launches without errors
- [ ] Welcome screen displays correctly
- [ ] "Open ROM" button opens file dialog
- [ ] File dialog filters for .nes files
- [ ] ROM loading triggers (even if playback not yet implemented)
- [ ] Window title updates with ROM name
- [ ] Application closes cleanly

### User Experience

- [ ] Window opens at correct size (768x720)
- [ ] UI renders at 60 FPS
- [ ] Theme looks professional
- [ ] Button hover effects work
- [ ] File dialog is native (platform-specific)

### Quality

- [ ] Zero unsafe code
- [ ] Zero clippy warnings with `clippy::pedantic`
- [ ] Proper error handling for file I/O
- [ ] Async operations don't block UI
- [ ] Works on Linux, Windows, macOS

---

## Dependencies

### External Crates

```toml
iced = { version = "0.13", features = ["wgpu", "tokio", "image", "svg", "canvas", "advanced"] }
iced_aw = "0.10"
rfd = "0.14"
directories = "5.0"
tokio = { version = "1.40", features = ["full"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

### System Requirements

**Linux:**
- libxcb-dev
- libxkbcommon-dev
- libwayland-dev (Wayland support)

**Windows:**
- None (static linking)

**macOS:**
- None

---

## Related Documentation

- [Iced Book](https://book.iced.rs/) - Official Iced documentation
- [Elm Architecture](https://guide.elm-lang.org/architecture/) - Architectural pattern
- [M6-PLANNING-CHANGES.md](M6-PLANNING-CHANGES.md) - Technology analysis
- [RustyNES-UI_UX-Design-v2.md](../../../ref-docs/RustyNES-UI_UX-Design-v2.md) - Full design spec

---

## Performance Targets

- **Startup Time:** <500ms cold start
- **UI Render Rate:** 60 FPS minimum
- **File Dialog:** <100ms to open
- **ROM Loading:** <100ms for typical ROM
- **Memory Usage:** <50 MB base application

---

## Success Criteria

- [ ] All tasks complete
- [ ] Iced application shell functional
- [ ] Welcome view displays correctly
- [ ] File dialog integration works
- [ ] ROM loading triggers successfully
- [ ] Zero clippy warnings
- [ ] Zero unsafe code
- [ ] Ready for Sprint M6-S2 (wgpu rendering)

---

## Notes

### Why Iced Instead of egui?

See [M6-PLANNING-CHANGES.md](M6-PLANNING-CHANGES.md) for detailed analysis. Key reasons:

1. **Elm architecture** prevents state management bugs in large applications
2. **Superior animation system** critical for polished UI
3. **Better suited for 8+ major views** (Welcome, Library, Playing, Settings, etc.)
4. **Theme system** scales to complex designs
5. **Multi-window support** for detached debugger/TAS editor

### Elm Architecture Pattern

```
┌─────────────────────────────────────────────────┐
│                 ELM ARCHITECTURE                │
├─────────────────────────────────────────────────┤
│                                                 │
│  Model (State)                                  │
│    ↓                                            │
│  View (Render UI)                               │
│    ↓                                            │
│  User Interaction (Button click, etc.)          │
│    ↓                                            │
│  Message (Event)                                │
│    ↓                                            │
│  Update (State transition)                      │
│    ↓                                            │
│  Model (New state)                              │
│    ↓                                            │
│  View (Re-render)                               │
│                                                 │
└─────────────────────────────────────────────────┘
```

This unidirectional data flow prevents many common UI bugs and makes state management predictable.

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** None (M5 complete)
**Next Sprint:** [M6-S2 Core Emulation Display](M6-S2-wgpu-rendering.md)
