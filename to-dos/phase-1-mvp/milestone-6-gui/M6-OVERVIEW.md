# Milestone 6: Desktop GUI (MVP Core)

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~4 weeks (estimated)
**Progress:** 0%

---

## Overview

Milestone 6 delivers a **cross-platform desktop application** with **Iced + egui hybrid UI**, wgpu rendering, audio output, and controller support. This completes the Phase 1 MVP with a focus on core playability.

### Critical Architectural Decision

**UI Framework:** **Iced 0.13+ (primary)** + **egui 0.28 (debug overlay only)**

This differs from original planning which used egui as the primary framework. The change is justified by:
- Iced's superior support for complex applications with 5+ views
- Better animation system (critical for polished UI)
- Structured state management (Elm architecture prevents bugs)
- Theme system scales to future features (HTPC mode in Phase 3)

**Reference:** See `/home/parobek/Code/RustyNES/to-dos/phase-1-mvp/milestone-6-gui/M6-PLANNING-CHANGES.md` for detailed technology analysis.

### MVP Scope

**M6 focuses on core playability only.** Advanced features have been moved to future phases:
- **Kept in M6:** Basic UI, ROM loading, 60 FPS rendering, audio, input, basic run-ahead (RA=1)
- **Moved to Phase 2:** Advanced run-ahead system, RetroAchievements, netplay, TAS, debugger
- **Moved to Phase 3:** HTPC mode, Cover Flow, advanced CRT shaders, plugins
- **Moved to Phase 4:** TAS editor, advanced optimization, CLI automation

---

## Goals

### Core Features (MVP)

- ⏳ Iced-based user interface (Elm architecture)
- ⏳ wgpu rendering backend (60 FPS)
- ⏳ cpal audio output (<20ms latency)
- ⏳ gilrs gamepad support + keyboard input
- ⏳ ROM library browser (Grid/List views)
- ⏳ Settings system with persistence (TOML)
- ⏳ Basic CRT shader (3-5 presets)
- ⏳ Basic run-ahead (RA=1) - architectural foundation
- ⏳ Cross-platform (Linux, Windows, macOS)
- ⏳ Zero unsafe code (except FFI if needed)

### Quality Targets

- 60 FPS rendering (16.67ms frame time)
- Audio latency <20ms
- Startup time <500ms
- Memory footprint <100 MB
- Intuitive, polished UI

---

## Sprint Breakdown

### Sprint 1: Iced Application Foundation ⏳ PENDING

**Duration:** Week 1
**Target Files:** `crates/rustynes-desktop/src/main.rs`, `app.rs`, `views/`

**Goals:**

- [ ] Iced 0.13+ application skeleton
- [ ] Window management (single window, menu bar)
- [ ] wgpu integration for game viewport
- [ ] Basic theme system (light/dark modes)
- [ ] Navigation between views (Welcome, Library, Playing, Settings)
- [ ] File dialog for ROM loading
- [ ] Elm architecture (Message/Update/View pattern)

**Outcome:** Iced application shell with navigation.

**Reference:** [M6-S1-iced-application.md](M6-S1-iced-application.md)

---

### Sprint 2: Core Emulation Display ⏳ PENDING

**Duration:** Week 1-2
**Target Files:** `crates/rustynes-desktop/src/renderer/`, `audio/`

**Goals:**

- [ ] wgpu framebuffer rendering (256×240 → scaled)
- [ ] Aspect ratio handling (8:7 original, 4:3 TV, pixel-perfect)
- [ ] Nearest-neighbor scaling
- [ ] Basic CRT shader (scanlines, slight curvature) - 3 presets
- [ ] cpal audio output with ring buffer
- [ ] Audio/video synchronization
- [ ] 60 FPS target, VSync support
- [ ] Integer scaling option

**Outcome:** Game rendering and audio playback at 60 FPS.

**Reference:** [M6-S2-core-display.md](M6-S2-core-display.md)

---

### Sprint 3: Input and ROM Loading ⏳ PENDING

**Duration:** Week 2-3
**Target Files:** `crates/rustynes-desktop/src/input/`, `library/`

**Goals:**

- [ ] Keyboard input mapping (configurable)
- [ ] gilrs gamepad support (hot-plug, SDL mappings)
- [ ] Player 1/2 selection
- [ ] ROM file browser (Grid/List views)
- [ ] iNES/NES 2.0 ROM loading
- [ ] Recent ROMs list
- [ ] Box art display (local files)
- [ ] ROM library scanning

**Outcome:** Full input support and ROM library browser.

**Reference:** [M6-S3-input-library.md](M6-S3-input-library.md)

---

### Sprint 4: Settings and Persistence ⏳ PENDING

**Duration:** Week 3
**Target Files:** `crates/rustynes-desktop/src/config/`, `views/settings.rs`

**Goals:**

- [ ] Settings screen UI (Video, Audio, Input, Paths)
- [ ] Video settings (scale, filter, aspect ratio, shader selection)
- [ ] Audio settings (volume, latency, sample rate)
- [ ] Input settings (keyboard/gamepad mapping)
- [ ] Path settings (ROM directory, save states)
- [ ] Configuration file (TOML format)
- [ ] Window state persistence (size, position)
- [ ] Per-game configuration (automatic profiles)

**Outcome:** Complete settings system with persistence.

**Reference:** [M6-S4-settings-persistence.md](M6-S4-settings-persistence.md)

---

### Sprint 5: Polish and Release ⏳ PENDING

**Duration:** Week 4
**Target Files:** `crates/rustynes-desktop/src/latency/`, polish passes

**Goals:**

- [ ] **Basic run-ahead (RA=1)** - proof of concept
- [ ] Fast save state serialization (<1ms with bincode)
- [ ] Toast notification system
- [ ] Modal dialogs (errors, confirmations)
- [ ] Error handling and user feedback
- [ ] About dialog with version info
- [ ] Application icon
- [ ] Cross-platform packaging (AppImage, DMG, EXE)
- [ ] v1.0.0-alpha release

**Outcome:** Polished MVP desktop application with basic latency reduction.

**Reference:** [M6-S5-polish-release.md](M6-S5-polish-release.md)

---

## Technical Stack

### UI Framework (Hybrid Architecture)

```
┌─────────────────────────────────────────────┐
│          ICED APPLICATION LAYER             │
│  • Main window and chrome                   │
│  • ROM browser (Grid/List views)            │
│  • Settings panels                          │
│  • Theme system (light/dark)                │
│  • Elm architecture (structured state)      │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│           WGPU RENDER LAYER                 │
│  • Game viewport (256×240 → scaled)         │
│  • Basic CRT shaders (3-5 presets)          │
│  • Nearest-neighbor filtering               │
│  • Aspect ratio modes                       │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│      EGUI DEBUG OVERLAY (Future: M10)       │
│  • CPU/PPU/APU state viewers                │
│  • Memory hex editor                        │
│  • Trace logger                             │
│  (NOT part of M6 - deferred to Phase 2)     │
└─────────────────────────────────────────────┘
```

### Core Dependencies

```toml
[dependencies]
# UI Framework (PRIMARY: ICED)
iced = { version = "0.13", features = ["wgpu", "tokio", "image"] }

# Graphics
wgpu = "0.20"
image = "0.25"

# Audio
cpal = "0.15"
rubato = "0.15"  # Resampling (APU → 48kHz)

# Input
gilrs = "0.10"
winit = "0.30"

# File System
rfd = "0.14"  # Native file dialogs
directories = "5.0"
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# Fast Serialization (for run-ahead save states)
bincode = "1.3"

# Utilities
log = "0.4"
env_logger = "0.10"

# Internal
rustynes-core = { path = "../rustynes-core" }
```

---

## Elm Architecture (Iced State Management)

```rust
/// Root application state
pub struct RustyNes {
    /// Current view/screen
    view: View,

    /// Emulator core (None when no ROM loaded)
    console: Option<Console>,

    /// Theme configuration
    theme: Theme,

    /// ROM library state
    library: LibraryState,

    /// Settings state
    settings: Settings,

    /// Run-ahead configuration (basic RA=1 only in M6)
    run_ahead: RunAheadConfig,
}

/// All possible views
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Welcome,
    Library,
    Playing,
    Settings(SettingsTab),
}

/// Application messages (Elm pattern)
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    NavigateTo(View),

    // Emulation control
    LoadRom(PathBuf),
    Play,
    Pause,
    Reset,

    // Run-ahead (basic)
    ToggleRunAhead,

    // Settings
    UpdateSetting(SettingChange),
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Loads ROMs via file dialog
- [ ] Renders at 60 FPS (16.67ms frame time)
- [ ] Audio plays without crackling (<20ms latency)
- [ ] Keyboard controls work (configurable)
- [ ] Gamepad detected and usable (gilrs)
- [ ] Can reset/pause/resume emulation
- [ ] Settings persist across sessions
- [ ] ROM library browser functional (Grid/List)
- [ ] Basic CRT shader applies correctly
- [ ] Basic run-ahead (RA=1) reduces latency measurably
- [ ] Cross-platform builds work (Linux, Windows, macOS)

### User Experience

- [ ] Intuitive Iced UI with clear navigation
- [ ] Responsive UI (no lag, smooth animations)
- [ ] Clear error messages with actionable feedback
- [ ] Keyboard shortcuts work
- [ ] Window resizes correctly
- [ ] Theme switching works (light/dark)
- [ ] Recent ROMs easily accessible

### Quality

- [ ] No crashes on invalid ROMs
- [ ] No audio crackling
- [ ] No visual tearing
- [ ] Zero unsafe code (except necessary FFI)
- [ ] Clean shutdown
- [ ] Consistent 60 FPS on target hardware

---

## Code Structure

```text
crates/rustynes-desktop/
├── src/
│   ├── main.rs              # Entry point, Iced setup
│   ├── app.rs               # RustyNes application state (Elm)
│   ├── message.rs           # Message enum (all events)
│   ├── theme.rs             # Theme definitions
│   │
│   ├── views/               # Iced view implementations
│   │   ├── mod.rs
│   │   ├── welcome.rs       # Welcome screen
│   │   ├── library.rs       # ROM library browser
│   │   ├── playing.rs       # Active gameplay view
│   │   └── settings.rs      # Settings panel
│   │
│   ├── widgets/             # Custom Iced widgets
│   │   ├── mod.rs
│   │   ├── game_viewport.rs # NES framebuffer display
│   │   ├── toast.rs         # Notification widget
│   │   └── modal.rs         # Dialog widget
│   │
│   ├── renderer/            # wgpu rendering subsystem
│   │   ├── mod.rs
│   │   ├── game_renderer.rs # Game viewport rendering
│   │   ├── crt_shaders.rs   # CRT shader pipeline
│   │   └── shaders/         # WGSL shader files (3-5 basic presets)
│   │
│   ├── audio/               # cpal audio subsystem
│   │   ├── mod.rs
│   │   ├── output.rs        # Audio output stream
│   │   ├── resampler.rs     # APU → 48kHz resampling
│   │   └── ring_buffer.rs   # Lock-free audio buffer
│   │
│   ├── input/               # Input handling
│   │   ├── mod.rs
│   │   ├── keyboard.rs      # Keyboard mapping
│   │   └── gamepad.rs       # gilrs integration
│   │
│   ├── latency/             # Run-ahead system (basic RA=1 only)
│   │   ├── mod.rs
│   │   └── run_ahead.rs     # Basic run-ahead implementation
│   │
│   ├── config/              # Configuration management
│   │   ├── mod.rs
│   │   ├── settings.rs      # Global settings
│   │   ├── persistence.rs   # TOML save/load
│   │   └── defaults.rs      # Default values
│   │
│   └── library/             # ROM library management
│       ├── mod.rs
│       ├── scanner.rs       # ROM directory scanning
│       ├── metadata.rs      # Game metadata
│       └── box_art.rs       # Cover art management
│
└── assets/                  # Static resources
    ├── icons/               # App icons
    ├── fonts/               # JetBrains Mono, Press Start 2P
    └── shaders/             # Basic shader presets
```

**Estimated Total:** ~1,500-2,000 lines of code (MVP core only)

---

## Performance Requirements

### Frame Rate
- **Target:** 60 FPS (16.67ms per frame)
- **UI Refresh:** 60Hz minimum (Iced subscriptions)
- **CRT Shaders:** 60 FPS with basic effects enabled

### Latency
- **Input to Display:** <15ms (with basic run-ahead RA=1)
- **Audio Latency:** <20ms (cpal exclusive mode where available)
- **UI Responsiveness:** <8ms (half a frame)

### Memory
- **Base Application:** <50 MB
- **ROM Library (100 games):** <50 MB
- **Total (typical):** <100 MB

### Startup Time
- **Cold Start:** <500ms (Iced compilation)
- **ROM Loading:** <100ms (average)

---

## Advanced Features (Deferred to Future Phases)

### Phase 2: Advanced Features (M7-M10)

**See:** `/home/parobek/Code/RustyNES/to-dos/phase-2-features/PHASE-2-OVERVIEW.md`

- **M7:** RetroAchievements + **Advanced Run-Ahead (RA=0-4, auto-detect, frame delay)**
- **M8:** GGPO Netplay (backroll-rs)
- **M9:** TAS recording/playback + Lua scripting
- **M10:** Advanced debugger (egui overlay)

### Phase 3: Expansion (M11-M14)

**See:** `/home/parobek/Code/RustyNES/to-dos/phase-3-expansion/PHASE-3-OVERVIEW.md`

- **M11:** Expansion Audio + **Advanced CRT shaders (12+ presets, rolling scan)**
- **M12:** Advanced Mappers + **HD Pack support**
- **M13:** WebAssembly + **HTPC mode (Controller-First UI, Cover Flow, Virtual Shelf)**
- **M14:** Mobile + **Plugin architecture + Discord Rich Presence + Cloud sync**

### Phase 4: Polish (M15-M18)

**See:** `/home/parobek/Code/RustyNES/to-dos/phase-4-polish/PHASE-4-OVERVIEW.md`

- **M15:** Video Filters + Advanced shader optimization
- **M16:** **TAS editor (piano roll)** + advanced TAS tools
- **M17:** Performance optimization + Full run-ahead optimization
- **M18:** v1.0 Release + **CLI automation mode** + Full accessibility audit

---

## Challenges & Risk Mitigation

| Challenge | Risk | Mitigation |
|-----------|------|------------|
| Iced learning curve | Medium | Study examples, budget extra time Week 1 |
| Audio latency/crackling | Medium | Use cpal, tune ring buffer size, platform testing |
| Cross-platform builds | Low | CI matrix testing from Day 1 |
| Run-ahead complexity (RA=1) | Medium | Start simple, ensure fast save states (<1ms) |
| Gamepad compatibility | Low | Test with common controllers (Xbox, PlayStation, Switch Pro) |

---

## Platform-Specific Notes

### Linux
- Dependencies: libxcb, libasound, libudev
- Package: AppImage
- Test on: Ubuntu 22.04+, Fedora 38+, Arch Linux

### Windows
- Dependencies: None (static linking)
- Package: Portable .exe or installer
- Test on: Windows 10+, Windows 11

### macOS
- Dependencies: None
- Package: .dmg or .app bundle
- Code signing required for distribution
- Test on: macOS 12+ (Intel + Apple Silicon)

---

## Related Documentation

- [M6-PLANNING-CHANGES.md](M6-PLANNING-CHANGES.md) - Comprehensive planning and technology analysis
- [Desktop Frontend](../../../docs/platform/DESKTOP.md)
- [Build Instructions](../../../docs/dev/BUILD.md)
- [Configuration](../../../docs/api/CONFIGURATION.md)
- [UI/UX Design v2](../../../ref-docs/RustyNES-UI_UX-Design-v2.md) - Full design specification

---

## Next Steps

### Pre-Sprint Preparation

1. **Set Up Crate**
   - Create rustynes-desktop/Cargo.toml with Iced dependencies
   - Add wgpu, cpal, gilrs dependencies
   - Create basic Iced window

2. **Research**
   - Study Iced 0.13 examples (Elm architecture pattern)
   - Review wgpu texture updates
   - Test cpal audio on all target platforms

3. **Design**
   - Plan Iced view structure (Welcome, Library, Playing, Settings)
   - Design Message enum (all application events)
   - Design theme system (light/dark modes)

### Sprint 1 Kickoff

- Create Iced application skeleton
- Implement Elm architecture (Model-Update-View)
- Add navigation between views
- Integrate wgpu for game viewport
- Load and display ROM

---

**Milestone Status:** ⏳ PENDING
**Blocked By:** M5 ⏳ (needs rustynes-core)
**Deliverable:** Phase 1 MVP Complete - Playable emulator with Iced UI!
**Total Duration:** 4 weeks (focused MVP scope)
