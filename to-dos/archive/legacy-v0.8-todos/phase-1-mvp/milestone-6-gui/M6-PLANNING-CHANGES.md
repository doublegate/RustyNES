# Milestone 6: Desktop GUI - Planning Changes & Technology Analysis

**Document Version:** 1.0.0
**Created:** December 19, 2025
**Status:** Planning Documentation
**Purpose:** Comprehensive M6 planning based on UI/UX Design v2.0.0

---

## Executive Summary

This document consolidates planning for Milestone 6 (Desktop GUI) based on the comprehensive UI/UX design documents (v1.0.0 and v2.0.0). The design specifications provide detailed guidance on technology choices, architecture, implementation phases, and feature requirements that significantly enhance the original M6 planning.

### Key Sources

- **Primary:** `/home/parobek/Code/RustyNES/ref-docs/RustyNES-UI_UX-Design-v2.md` (2,671 lines)
- **Secondary:** `/home/parobek/Code/RustyNES/ref-docs/RustyNES-UI_UX-Design-v1.md` (2,050 lines)
- **Original Planning:** `/home/parobek/Code/RustyNES/to-dos/phase-1-mvp/milestone-6-gui/M6-OVERVIEW.md`

### Critical Architectural Decision

**The v2 design specification recommends a hybrid UI architecture:**

- **Primary Framework:** Iced 0.13+ (not egui)
- **Debug Overlay:** egui 0.28 (developer tools only)
- **Optional:** Slint (for embedded/resource-constrained builds)

This differs from the original M6 planning which used egui as the primary framework. The change is justified by Iced's superior support for complex applications, animation systems, and structured state management.

---

## Version 2.0.0 Enhancements

The v2 design document adds significant features beyond v1:

### Latency Reduction System (NEW)

```
┌───────────────────────────────────────────────────────────────┐
│                  LATENCY REDUCTION FEATURES                   │
├───────────────────────────────────────────────────────────────┤
│                                                               │
│  • Run-Ahead (1-4 frames, auto-detect per game)               │
│  • Preemptive Frames (alternative mode)                       │
│  • Frame Delay auto-tuning (0-15 frames)                      │
│  • Just-In-Time input polling (<1ms optimization)             │
│  • Adaptive sync (VRR/FreeSync/G-Sync)                        │
│  • Black Frame Insertion (BFI) for high-Hz displays           │
│  • Dual-instance mode for audio stability during run-ahead    │
│                                                               │
│  TARGET: Sub-10ms input latency (faster than original NES!)   │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

### Enhanced CRT Shader Pipeline (EXPANDED)

```
┌───────────────────────────────────────────────────────────┐
│                     CRT SHADER FEATURES                   │
├───────────────────────────────────────────────────────────┤
│                                                           │
│  • 12+ shader presets (CRT-Royale, Lottes, Guest, etc.)   │
│  • Phosphor mask types (Aperture Grille, Slot, Shadow)    │
│  • Rolling scan CRT simulation (Blur Busters technique)   │
│  • NTSC composite signal simulation                       │
│  • Subpixel rendering for mask accuracy                   │
│  • HDR bloom with phosphor persistence                    │
│  • Scanline effects (adjustable intensity)                │
│  • Barrel distortion (CRT curvature)                      │
│  • Vignette darkening                                     │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

### HTPC Controller-First Mode (NEW)

```
┌────────────────────────────────────────────────────────┐
│                   HTPC/LIVING ROOM MODE                │
├────────────────────────────────────────────────────────┤
│                                                        │
│  • Full 10-foot UI for living room setups              │
│  • Cover Flow view (carousel-style ROM browsing)       │
│  • Virtual Shelf view (3D perspective)                 │
│  • Voice navigation support (optional)                 │
│  • Automatic metadata scraping (IGDB, ScreenScraper)   │
│  • Large text scaling (3XL: 48px for readability)      │
│  • Controller-only navigation (no keyboard required)   │
│  • Haptic feedback patterns                            │
│                                                        │
└────────────────────────────────────────────────────────┘
```

### Advanced Features (NEW)

- **HD Pack Support:** Mesen-compatible HD texture packs
- **Per-Game Configuration:** Automatic profiles for individual games
- **Plugin Architecture:** Extensible system for shaders, input mappers, scrapers
- **Cloud Sync:** Optional save state synchronization (Dropbox, GDrive)
- **Discord Rich Presence:** Show currently playing game
- **CLI Mode:** Full command-line interface for automation
- **Sound Design System:** Retro SFX for UI interactions
- **Glass Morphism Effects:** Modern backdrop blur styling

---

## Technology Stack

### Core Framework Comparison

The design documents extensively analyze framework choices:

| Aspect | Iced 0.13+ | egui 0.28 | Decision |
|--------|------------|-----------|----------|
| **Rendering** | Retained + Immediate hybrid | Pure Immediate | Iced for main UI |
| **Animation** | Native subscriptions, smooth | Manual per-frame | Iced superior |
| **Styling** | Theme system, CSS-like | Inline styles | Iced for consistency |
| **Architecture** | Elm (Model-Update-View) | Direct rendering | Iced scales better |
| **State Management** | Structured, typed messages | Ad-hoc | Iced for complexity |
| **Learning Curve** | Steeper (Elm architecture) | Simpler | Worth investment |
| **GPU Integration** | wgpu native | wgpu via backend | Equal quality |
| **Performance** | Excellent for large UIs | Excellent for tools | Both performant |

### Research Findings (December 2025)

**Iced vs egui in 2025:**

1. **egui strengths:**
   - Faster to prototype
   - Simpler immediate mode API
   - More existing examples
   - Better for debug tools and quick UIs

2. **Iced strengths:**
   - Better for large, structured applications
   - Superior animation system (critical for polished UI)
   - Theme system scales to complex designs
   - Elm architecture prevents state management bugs
   - Better suited for 10+ screen applications

3. **Decision Rationale:**
   - RustyNES will have 8+ major views (Welcome, Library, Playing, Settings, NetplayLobby, Achievements, Debugger, TasEditor)
   - Requires sophisticated animations and transitions
   - HTPC mode needs consistent theming across all views
   - egui's immediate mode becomes unwieldy at this scale

**Industry Examples:**
- **TetaNES** (NES emulator): Uses egui successfully for simpler UI
- **Plastic** (NES emulator): Uses egui + gilrs, acknowledges keyboard responsiveness issues
- **RetroArch**: C++ but demonstrates value of structured UI for feature-rich emulators

### Complete Dependency Graph

Based on v2 design specification:

```toml
[dependencies]
# ═══════════════════════════════════════════════════════════════
# UI FRAMEWORK (PRIMARY: ICED)
# ═══════════════════════════════════════════════════════════════
iced = { version = "0.13", features = [
    "wgpu",           # GPU-accelerated rendering
    "advanced",       # Custom shaders for CRT effects
    "tokio",          # Async runtime (file I/O, networking)
    "image",          # Box art, screenshots
    "svg",            # Vector icons
    "canvas",         # Custom game viewport rendering
    "lazy",           # Virtual scrolling for large ROM libraries
    "debug",          # Debug overlay integration
    "multi-window",   # Detached debugger/TAS editor windows
]}
iced_aw = "0.10"      # Additional widgets (badges, cards, modals, tabs)

# ═══════════════════════════════════════════════════════════════
# DEBUG OVERLAY (SECONDARY: EGUI)
# ═══════════════════════════════════════════════════════════════
egui = "0.28"         # Immediate mode GUI for debug tools
egui-wgpu = "0.28"    # wgpu backend integration
egui-winit = "0.28"   # Window event integration

# ═══════════════════════════════════════════════════════════════
# OPTIONAL: SLINT (EMBEDDED BUILDS)
# ═══════════════════════════════════════════════════════════════
slint = { version = "1.7", optional = true }

# ═══════════════════════════════════════════════════════════════
# GRAPHICS & SHADERS
# ═══════════════════════════════════════════════════════════════
wgpu = "0.20"              # Cross-platform GPU API
naga = "0.20"              # Shader compilation (WGSL → SPIR-V/MSL/HLSL)
image = "0.25"             # Image loading/processing
resvg = "0.42"             # SVG rendering for icons
fast_image_resize = "4.0"  # High-quality scaling algorithms
palette = "0.7"            # Color manipulation (CRT color accuracy)

# ═══════════════════════════════════════════════════════════════
# AUDIO
# ═══════════════════════════════════════════════════════════════
cpal = "0.15"         # Cross-platform audio (WASAPI/ALSA/CoreAudio)
rubato = "0.15"       # High-quality resampling (APU → 48kHz)
dasp = "0.11"         # Digital audio signal processing
symphonia = "0.5"     # Audio decoding (UI sounds, music)

# ═══════════════════════════════════════════════════════════════
# INPUT & HAPTICS
# ═══════════════════════════════════════════════════════════════
gilrs = "0.10"        # Gamepad support (SDL-compatible mappings)
gilrs-core = "0.5"    # Low-level gamepad access
winit = "0.30"        # Window/input events
sdl2 = { version = "0.36", features = ["haptic"], optional = true }

# ═══════════════════════════════════════════════════════════════
# FILE SYSTEM & PERSISTENCE
# ═══════════════════════════════════════════════════════════════
rfd = "0.14"          # Native file dialogs (cross-platform)
notify = "6.1"        # File system watching (hot reload ROMs)
directories = "5.0"   # Platform-specific paths (config, saves)
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"          # Config files (human-readable)
bincode = "1.3"       # Fast serialization (save states, run-ahead)
lz4_flex = "0.11"     # Compression (save states, rewind buffer)
zstd = "0.13"         # Higher compression (cloud sync)
memmap2 = "0.9"       # Memory-mapped files (large rewind buffers)

# ═══════════════════════════════════════════════════════════════
# ASYNC & CONCURRENCY
# ═══════════════════════════════════════════════════════════════
tokio = { version = "1.40", features = ["full"] }
crossbeam-channel = "0.5"
parking_lot = "0.12"
rayon = "1.10"        # Parallel iterators (library scanning)

# ═══════════════════════════════════════════════════════════════
# NETWORKING (NETPLAY)
# ═══════════════════════════════════════════════════════════════
backroll = "0.3"           # GGPO rollback (similar to run-ahead!)
quinn = "0.11"             # QUIC protocol
webrtc = "0.11"            # Browser-compatible P2P
matchbox_socket = "0.10"   # WebRTC signaling
stun-client = "0.1"        # NAT traversal

# ═══════════════════════════════════════════════════════════════
# SCRIPTING & DEBUGGING
# ═══════════════════════════════════════════════════════════════
mlua = { version = "0.9", features = ["lua54", "vendored", "async"] }
rhai = { version = "1.17", optional = true }  # Alternative scripting

# ═══════════════════════════════════════════════════════════════
# ACHIEVEMENTS & METADATA
# ═══════════════════════════════════════════════════════════════
rcheevos = "0.2"           # RetroAchievements (pure Rust)
reqwest = { version = "0.12", features = ["json"] }  # HTTP API calls
scraper = "0.19"           # HTML parsing (metadata scraping)

# ═══════════════════════════════════════════════════════════════
# UTILITIES
# ═══════════════════════════════════════════════════════════════
chrono = "0.4"             # Date/time handling
humantime = "2.1"          # Human-readable durations
fuzzy-matcher = "0.3"      # Fuzzy search (ROM library)
unicode-segmentation = "1.11"  # Text handling
tracing = "0.1"            # Structured logging
tracing-subscriber = "0.3" # Log output formatting

# ═══════════════════════════════════════════════════════════════
# SOCIAL INTEGRATION (OPTIONAL)
# ═══════════════════════════════════════════════════════════════
discord-rich-presence = { version = "0.2", optional = true }

# ═══════════════════════════════════════════════════════════════
# CLOUD SYNC (OPTIONAL)
# ═══════════════════════════════════════════════════════════════
aws-sdk-s3 = { version = "1.0", optional = true }
google-drive3 = { version = "5.0", optional = true }

[features]
default = ["haptics", "discord"]
haptics = ["sdl2"]
discord = ["discord-rich-presence"]
cloud = ["aws-sdk-s3", "google-drive3"]
embedded = ["slint"]
```

---

## Architecture Overview

### Hybrid UI Stack (v2.0.0)

```
┌─────────────────────────────────────────────────────────────┐
│                      RUSTYNES UI STACK v2.0                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                    ICED APPLICATION LAYER            │   │
│  │  • Main window chrome & title bar                    │   │
│  │  • ROM browser & library (Grid/List/CoverFlow)       │   │
│  │  • Settings panels with live preview                 │   │
│  │  • Netplay lobby with voice chat indicators          │   │
│  │  • Achievement overlays & unlock animations          │   │
│  │  • HTPC Controller-First navigation mode             │   │
│  └──────────────────────────────────────────────────────┘   │
│                              │                              │
│                              ▼                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                    WGPU RENDER LAYER                 │   │
│  │  • Game viewport (256×240 → scaled)                  │   │
│  │  • CRT shader pipeline (12+ presets)                 │   │
│  │  • Phosphor mask simulation (Aperture/Slot/Shadow)   │   │
│  │  • Scanline + bloom + curvature effects              │   │
│  │  • NTSC composite simulation                         │   │
│  │  • Rolling scan CRT (high-Hz displays)               │   │
│  │  • HDR tone mapping (when available)                 │   │
│  └──────────────────────────────────────────────────────┘   │
│                              │                              │
│                              ▼                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                 EGUI DEBUG OVERLAY (F12)             │   │
│  │  • CPU/PPU/APU state viewers with live graphs        │   │
│  │  • Memory hex editor with search & watch             │   │
│  │  • Trace logger with filtering                       │   │
│  │  • Lua scripting console with autocomplete           │   │
│  │  • Run-ahead frame visualizer                        │   │
│  │  • Latency measurement display                       │   │
│  └──────────────────────────────────────────────────────┘   │
│                              │                              │
│                              ▼                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                  LATENCY REDUCTION ENGINE            │   │
│  │  • Run-Ahead (1-4 frames, auto-detect per game)      │   │
│  │  • Preemptive Frames (alternative mode)              │   │
│  │  • Frame Delay auto-tuning (0-15 frames)             │   │
│  │  • Just-In-Time input polling                        │   │
│  │  • Dual-instance mode for audio stability            │   │
│  │  • Adaptive sync (VRR/FreeSync/G-Sync)               │   │
│  └──────────────────────────────────────────────────────┘   │
│                              │                              │
│                              ▼                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                   PLUGIN SYSTEM                      │   │
│  │  • Shader plugins (.wgsl files)                      │   │
│  │  • Input mapper plugins                              │   │
│  │  • Metadata scraper plugins                          │   │
│  │  • Cloud sync plugins (Dropbox, GDrive)              │   │
│  │  • Social integration plugins (Discord, Twitch)      │   │
│  └──────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Elm Architecture (Iced State Management)

```rust
/// Root application state
pub struct RustyNes {
    // ═══════════════════════════════════════════════════════════
    // CORE STATE
    // ═══════════════════════════════════════════════════════════

    /// Current view/screen
    view: View,

    /// Emulator core (None when no ROM loaded)
    console: Option<Console>,

    /// Emulation state
    emulation: EmulationState,

    // ═══════════════════════════════════════════════════════════
    // UI STATE
    // ═══════════════════════════════════════════════════════════

    /// Theme configuration
    theme: Theme,

    /// Window state
    window: WindowState,

    /// Current modal (if any)
    modal: Option<Modal>,

    /// Toast notifications queue
    toasts: Vec<Toast>,

    /// Quick menu state
    quick_menu: QuickMenuState,

    // ═══════════════════════════════════════════════════════════
    // FEATURE STATES
    // ═══════════════════════════════════════════════════════════

    /// ROM library state
    library: LibraryState,

    /// Settings state
    settings: Settings,

    /// Debugger state (lazy loaded)
    debugger: Option<DebuggerState>,

    /// Netplay state
    netplay: NetplayState,

    /// Achievements state
    achievements: AchievementsState,

    /// TAS recorder state
    tas: TasState,

    // ═══════════════════════════════════════════════════════════
    // LATENCY REDUCTION
    // ═══════════════════════════════════════════════════════════

    /// Run-ahead configuration
    run_ahead: RunAheadConfig,

    /// Latency statistics
    latency_stats: LatencyStats,
}

/// All possible views
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Welcome,
    Library,
    Playing,
    Settings(SettingsTab),
    NetplayLobby,
    Achievements,
    Debugger,
    TasEditor,
}

/// Application messages (Elm pattern)
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    NavigateTo(View),
    GoBack,

    // Emulation control
    LoadRom(PathBuf),
    RomLoaded(Result<Console, EmulatorError>),
    Play,
    Pause,
    Reset,
    PowerCycle,

    // Run-ahead
    UpdateRunAhead(u8),  // 0-4 frames
    AutoDetectRunAhead,
    TogglePreemptiveFrames,

    // ... (more messages)
}
```

---

## Sprint Breakdown

### Phase 1: Foundation (Week 1-2)

Based on v2 design specification, this phase establishes the core application and basic run-ahead:

#### Week 1: Application Shell

- [ ] Create `rustynes-desktop` crate structure
- [ ] Set up **Iced** application skeleton (not egui!)
- [ ] Implement custom title bar (Windows/Linux/macOS)
- [ ] Create basic theme system with **glass morphism**
- [ ] Implement sidebar navigation
- [ ] Set up wgpu render pipeline
- [ ] Integrate game viewport with NES framebuffer

#### Week 2: Core Playback + Latency Foundation

- [ ] Implement ROM loading via file dialog
- [ ] Create cpal audio pipeline with **dynamic rate control**
- [ ] Set up input handling (keyboard + **JIT polling**)
- [ ] Implement basic play/pause/reset
- [ ] Add FPS counter and **frame timing display**
- [ ] Create quick menu overlay
- [ ] Implement save states (UI + serialization)
- [ ] **Implement basic run-ahead (1 frame)**

**Deliverable:** Playable emulator with basic UI and RA=1

### Phase 2: Polish (Week 3-4)

#### Week 3: Library & Settings

- [ ] Implement ROM library browser (**Grid/List views**)
- [ ] Add box art loading (local + scraping)
- [ ] Create settings panel UI with all categories
- [ ] Implement video settings (scale, shaders basic)
- [ ] Implement audio settings (volume, latency)
- [ ] Implement input settings (keyboard + gamepad mapping)
- [ ] Add gamepad support (gilrs)
- [ ] Persist configuration to TOML
- [ ] **Implement per-game profiles**

#### Week 4: Visual Polish + Latency System

- [ ] Implement CRT shader pipeline (5 presets minimum)
- [ ] Add animation system (Iced subscriptions)
- [ ] Create toast notification system
- [ ] Implement modal dialogs
- [ ] Add loading states and progress indicators
- [ ] Polish transitions and micro-interactions
- [ ] Implement theme switching (light/dark/retro)
- [ ] **Full run-ahead system (0-4 frames, auto-detect)**
- [ ] **Frame delay system with auto-tuning**
- [ ] **Latency calibration wizard**

**Deliverable:** Polished MVP with full latency reduction

### Phase 3: Advanced Features (Week 5-8)

#### Week 5-6: HTPC Mode + Enhanced Shaders

- [ ] **Implement HTPC Controller-First mode**
- [ ] **Create Cover Flow view**
- [ ] **Create Virtual Shelf view**
- [ ] Full CRT shader pipeline (12+ presets)
- [ ] Phosphor mask simulation (all types)
- [ ] NTSC composite simulation
- [ ] **Rolling scan mode for 120Hz+ displays**
- [ ] HD Pack support
- [ ] Automatic metadata scraping (IGDB)
- [ ] **Haptic feedback system**
- [ ] **UI sound design system**

#### Week 7-8: Netplay, Achievements & Plugins

- [ ] Implement netplay lobby UI
- [ ] Add session creation/joining flow
- [ ] Create in-game netplay overlay
- [ ] Integrate RetroAchievements login
- [ ] Implement achievement browser
- [ ] Add achievement unlock notifications
- [ ] Create leaderboard UI
- [ ] **Implement plugin system architecture**
- [ ] **Discord Rich Presence plugin**
- [ ] **Cloud sync plugin (optional)**

**Deliverable:** Feature-complete with HTPC and plugins

### Phase 4: Debug & TAS (Week 9-12)

#### Week 9-10: Debugger

- [ ] Implement debugger view (**egui overlay**)
- [ ] Add CPU/PPU/APU state viewers
- [ ] Create memory hex editor
- [ ] Implement breakpoint system UI
- [ ] Add conditional breakpoints
- [ ] Memory watch expressions
- [ ] Trace logging to file
- [ ] **Run-ahead frame visualizer**

#### Week 11-12: TAS & Accessibility

- [ ] Add rewind timeline
- [ ] Create TAS editor (piano roll)
- [ ] Implement movie recording/playback
- [ ] Add Lua scripting console
- [ ] Greenzone system
- [ ] Branch management
- [ ] Full accessibility audit and fixes
- [ ] Screen reader testing
- [ ] One-handed mode polish

**Deliverable:** Full-featured emulator exceeding Mesen2

---

## Research Findings

### Key Technologies

#### 1. Iced GUI Framework

**Status (Dec 2025):** Mature, actively maintained, v0.13+ stable

**Pros:**
- Elm architecture prevents state management bugs in large apps
- Excellent animation system (critical for polished emulator UI)
- Theme system scales to complex multi-screen applications
- wgpu native integration (no middleware needed)
- Multi-window support (detached debugger/TAS editor)

**Cons:**
- Steeper learning curve than egui
- Fewer examples than egui (but improving)
- Larger binary size

**Recommended for:**
- Applications with 5+ major views
- Complex state management requirements
- Need for sophisticated animations
- Long-term maintainability

**References:**
- [Iced GitHub](https://github.com/iced-rs/iced)
- [Performance comparison](http://lukaskalbertodt.github.io/2023/02/03/tauri-iced-egui-performance-comparison.html)

#### 2. wgpu Rendering

**Status (Dec 2025):** Industry standard, v0.20 stable, excellent WebGPU support

**For NES Emulation:**
- 256×240 texture = ~180KB per frame (trivial for modern GPUs)
- Nearest-neighbor scaling trivial to implement
- CRT shaders via WGSL (WebGPU Shading Language)
- Cross-platform (Vulkan/Metal/DX12/WebGPU)
- HDR support available

**Performance:**
- 60 FPS easily achievable for NES resolution
- Rolling scan at 120Hz+ possible
- Shader complexity is main bottleneck (not texture upload)

**Reference Projects:**
- **TetaNES:** Excellent Rust NES emulator using wgpu
- [GitHub: lukexor/tetanes](https://github.com/lukexor/tetanes)

**Best Practices:**
- Use `write_texture()` for frame updates (not staging buffers)
- Nearest-neighbor sampler for pixel-perfect scaling
- Separate pipelines for game rendering vs CRT effects
- Pre-compile shaders at build time (faster startup)

#### 3. cpal Audio

**Status (Dec 2025):** De facto standard for Rust audio, v0.15 stable

**Backend Support:**
- **Windows:** WASAPI (low latency), ASIO (pro audio, <5ms)
- **macOS:** CoreAudio (excellent latency)
- **Linux:** ALSA, PulseAudio, JACK
- **WebAssembly:** Web Audio API

**For Emulator Audio:**
- Ring buffer design recommended (4096-8192 samples)
- APU outputs ~44.1kHz, resample to 48kHz (rubato crate)
- Target latency: <20ms for responsive gameplay
- Run-ahead requires dynamic sample rate adjustment

**Best Practices:**
- Use exclusive mode on Windows (lower latency)
- Monitor buffer underruns (audio crackling indicator)
- Implement adaptive buffer sizing
- Separate audio thread from emulation thread

**References:**
- [cpal GitHub](https://github.com/RustAudio/cpal)
- [Rust Audio Programming 2025](https://andrewodendaal.com/rust-audio-programming-ecosystem/)

#### 4. gilrs Gamepad Support

**Status (Dec 2025):** Mature, v0.10 stable, SDL-compatible mappings

**Features:**
- **Hot-plug support:** Automatically assigns/reuses gamepad IDs
- **SDL mappings:** Works with 1000+ controller types
- **Cross-platform:** Windows (XInput/DirectInput), macOS, Linux
- **Haptic feedback:** Via optional sdl2 feature

**For NES Emulation:**
- Map D-pad → NES D-pad
- Map A/B buttons → NES B/A (or configurable)
- Support for turbo buttons
- Player 1/2 selection

**Known Issues:**
- Keyboard responsiveness can be lower than gamepad (use JIT polling)
- Some third-party controllers need custom mappings

**Successful Implementations:**
- **Plastic NES emulator:** Uses gilrs, reports "working very nicely"

**References:**
- [gilrs GitHub](https://github.com/Arvamer/gilrs)
- [gilrs documentation](https://docs.rs/gilrs/latest/gilrs/)

#### 5. Run-Ahead Latency Reduction

**Status:** Proven technique, popularized by RetroArch, challenging to implement

**How It Works:**

```
Traditional Emulation:
  Frame N:   Read Input → Emulate → Render (input visible in Frame N+1 or N+2)
  Latency:   1-2 frames (16-33ms) internal lag

Run-Ahead (RA=2):
  Frame N:   Read Input → Emulate → Save State → Emulate +1 frame → Emulate +2 frames → Load State → Render
  Latency:   0 frames (input visible immediately!)
```

**Requirements:**
- Fast save state serialization (bincode recommended)
- Deterministic emulation (critical!)
- Sufficient CPU overhead (2-3x emulation speed for RA=2)
- Audio requires dual-instance or dynamic rate control

**NES-Specific:**
- Most NES games have 1-2 frames of internal lag
- Run-ahead can achieve **lower latency than original hardware**
- Auto-detection: Use frame advance to measure input → response delay

**Challenges:**
- Non-deterministic games break (rare on NES)
- Audio sync requires careful handling
- CPU intensive (needs performance headroom)

**Implementation Notes:**
- Start with RA=1 for MVP
- Add auto-detection per game in Phase 2
- Dual-instance mode: Run two emulators (one for video, one for audio)
- Preemptive Frames: Alternative technique (pre-render multiple possible inputs)

**References:**
- [RetroArch Run-Ahead Docs](https://docs.libretro.com/guides/runahead/)
- [byuu's Run-Ahead Article](https://bsnes.org/articles/input-run-ahead/)
- [Emulation Wiki: Input Lag](https://emulation.gametechwiki.com/index.php/Input_lag)

---

## Implementation Priorities

### Critical Path (Phase 1 MVP)

1. **Iced application shell** (Week 1)
   - Window creation, menu bar, basic navigation
   - File dialog, ROM loading
   - Establishes architecture for all future features

2. **wgpu rendering** (Week 1-2)
   - Game viewport integration
   - 60 FPS rendering
   - Nearest-neighbor scaling
   - Foundation for CRT shaders

3. **cpal audio** (Week 2)
   - Audio output without crackling
   - <20ms latency
   - Required for playability

4. **gilrs input** (Week 2-3)
   - Keyboard + gamepad support
   - JIT input polling
   - Foundation for run-ahead

5. **Save states** (Week 2)
   - Serialization/deserialization
   - Required for run-ahead
   - User-facing feature

6. **Basic run-ahead (RA=1)** (Week 2)
   - Proof of concept
   - Demonstrates latency reduction
   - Validates architecture

### High-Value Features (Phase 2)

1. **Full run-ahead system (RA=0-4, auto-detect)** (Week 4)
   - Competitive advantage
   - Measurably better than most emulators
   - Requires robust save state system

2. **CRT shader pipeline** (Week 4)
   - Nostalgia factor
   - Differentiates from basic emulators
   - Leverages wgpu investment

3. **Per-game profiles** (Week 3)
   - Auto-applies optimal settings
   - Improves UX significantly
   - Enables game-specific run-ahead

4. **ROM library browser** (Week 3)
   - Professional presentation
   - Box art + metadata
   - Essential for HTPC mode

### Deferred to Phase 3

1. **HTPC Controller-First mode** (Week 5-6)
   - Major UX feature but not critical for MVP
   - Requires ROM library foundation
   - Targets living room gaming

2. **Advanced CRT shaders** (Week 5-6)
   - 12+ presets, phosphor masks, rolling scan
   - Builds on basic CRT pipeline
   - Appeals to enthusiasts

3. **Plugin system** (Week 7-8)
   - Extensibility for community
   - Complex architecture
   - Not needed for core functionality

### Phase 4 (Advanced Tools)

1. **Debugger** (Week 9-10)
   - Developer-focused
   - Uses egui overlay
   - Not needed by casual users

2. **TAS tools** (Week 11-12)
   - Niche audience
   - Complex implementation
   - Builds on save state + input recording

---

## Files Created/Modified

### New Files (to be created)

#### Core Application

```
crates/rustynes-desktop/
├── Cargo.toml                          # Dependencies (Iced 0.13+, wgpu, cpal, gilrs)
├── src/
│   ├── main.rs                         # Entry point, Iced application setup
│   ├── app.rs                          # RustyNes application state (Elm architecture)
│   ├── theme.rs                        # Theme definitions (colors, fonts, glass morphism)
│   ├── message.rs                      # Message enum (all application events)
│   ├── view.rs                         # View enum (screen navigation)
│   │
│   ├── views/                          # Iced view implementations
│   │   ├── mod.rs
│   │   ├── welcome.rs                  # Welcome screen
│   │   ├── library.rs                  # ROM library browser
│   │   ├── playing.rs                  # Active gameplay view
│   │   ├── settings.rs                 # Settings panel
│   │   ├── netplay_lobby.rs            # Netplay setup
│   │   ├── achievements.rs             # Achievement browser
│   │   ├── debugger.rs                 # Debug view (egui integration)
│   │   └── tas_editor.rs               # TAS tools
│   │
│   ├── widgets/                        # Custom Iced widgets
│   │   ├── mod.rs
│   │   ├── game_viewport.rs            # NES framebuffer display widget
│   │   ├── quick_menu.rs               # Overlay menu widget
│   │   ├── toast.rs                    # Notification widget
│   │   ├── modal.rs                    # Dialog widget
│   │   ├── cover_flow.rs               # HTPC carousel view
│   │   └── virtual_shelf.rs            # HTPC 3D shelf view
│   │
│   ├── renderer/                       # wgpu rendering subsystem
│   │   ├── mod.rs
│   │   ├── game_renderer.rs            # Game viewport (256×240 texture)
│   │   ├── crt_shaders.rs              # CRT shader pipeline
│   │   ├── shaders/                    # WGSL shader files
│   │   │   ├── nearest_neighbor.wgsl   # Basic scaling
│   │   │   ├── crt_basic.wgsl          # Basic CRT effect
│   │   │   ├── crt_royale.wgsl         # CRT-Royale preset
│   │   │   ├── crt_lottes.wgsl         # Timothy Lottes preset
│   │   │   ├── rolling_scan.wgsl       # High-Hz CRT simulation
│   │   │   └── ntsc_composite.wgsl     # NTSC simulation
│   │   └── scaling.rs                  # Scaling algorithms
│   │
│   ├── audio/                          # cpal audio subsystem
│   │   ├── mod.rs
│   │   ├── output.rs                   # Audio output stream
│   │   ├── resampler.rs                # APU → 48kHz resampling
│   │   └── ring_buffer.rs              # Lock-free audio buffer
│   │
│   ├── input/                          # Input handling
│   │   ├── mod.rs
│   │   ├── keyboard.rs                 # Keyboard mapping
│   │   ├── gamepad.rs                  # gilrs integration
│   │   ├── hotkeys.rs                  # Global hotkey handling
│   │   └── jit_polling.rs              # Just-in-time input polling
│   │
│   ├── latency/                        # Run-ahead system
│   │   ├── mod.rs
│   │   ├── run_ahead.rs                # Run-ahead implementation
│   │   ├── frame_delay.rs              # Frame delay auto-tuning
│   │   ├── auto_detect.rs              # Per-game lag detection
│   │   └── dual_instance.rs            # Dual-emulator mode
│   │
│   ├── config/                         # Configuration management
│   │   ├── mod.rs
│   │   ├── settings.rs                 # Global settings struct
│   │   ├── game_profile.rs             # Per-game configuration
│   │   ├── persistence.rs              # TOML save/load
│   │   └── defaults.rs                 # Default values
│   │
│   ├── library/                        # ROM library management
│   │   ├── mod.rs
│   │   ├── scanner.rs                  # ROM directory scanning
│   │   ├── metadata.rs                 # Game metadata
│   │   ├── scraper.rs                  # IGDB/ScreenScraper integration
│   │   ├── box_art.rs                  # Cover art management
│   │   └── database.rs                 # Library persistence
│   │
│   ├── netplay/                        # Netplay (Phase 3)
│   │   ├── mod.rs
│   │   ├── lobby.rs
│   │   ├── session.rs
│   │   └── sync.rs
│   │
│   ├── achievements/                   # RetroAchievements (Phase 3)
│   │   ├── mod.rs
│   │   ├── client.rs
│   │   └── ui.rs
│   │
│   ├── plugins/                        # Plugin system (Phase 3)
│   │   ├── mod.rs
│   │   ├── loader.rs
│   │   ├── api.rs
│   │   └── builtin/
│   │       ├── discord_presence.rs
│   │       └── cloud_sync.rs
│   │
│   └── utils/                          # Utilities
│       ├── mod.rs
│       ├── fuzzy_search.rs
│       └── platform.rs
│
└── assets/                             # Static resources
    ├── icons/                          # App icons
    ├── fonts/                          # JetBrains Mono, Press Start 2P
    ├── shaders/                        # Additional shader presets
    └── sounds/                         # UI sound effects
```

### Modified Files

```
Cargo.toml                              # Workspace member: rustynes-desktop
README.md                               # Desktop application section
docs/platform/DESKTOP.md                # Desktop-specific documentation
docs/dev/BUILD.md                       # Desktop build instructions
.github/workflows/ci.yml                # Desktop GUI tests
```

### Documentation Updates Needed

```
docs/
├── gui/
│   ├── ICED_ARCHITECTURE.md            # Elm architecture guide
│   ├── THEME_SYSTEM.md                 # Theming documentation
│   ├── CUSTOM_WIDGETS.md               # Widget development
│   └── HTPC_MODE.md                    # Controller-first UI
├── features/
│   ├── RUN_AHEAD.md                    # Latency reduction guide
│   ├── CRT_SHADERS.md                  # Shader customization
│   └── PLUGINS.md                      # Plugin development
└── api/
    └── GUI_INTEGRATION.md              # Integrating rustynes-core
```

---

## Performance Requirements

Based on v2 design specification:

### Frame Rate

- **Target:** 60 FPS (16.67ms per frame)
- **UI Refresh:** 120Hz+ where hardware permits
- **Animation:** Smooth 60Hz minimum (Iced subscriptions)
- **CRT Shaders:** 60 FPS with all effects enabled

### Latency

- **Input to Display:** <10ms (with run-ahead)
- **Audio Latency:** <20ms (cpal exclusive mode)
- **UI Responsiveness:** <8ms (half a frame)
- **File Dialog:** <100ms to open

### Memory

- **Base Application:** <50 MB
- **ROM Library (1000 games):** <200 MB
- **Rewind Buffer (10 seconds):** <100 MB
- **Total (typical):** <400 MB

### Startup Time

- **Cold Start:** <500ms (Iced compilation)
- **ROM Loading:** <100ms (average)
- **Shader Compilation:** <200ms (cached after first run)

---

## Challenges & Risk Mitigation

### Challenge 1: Iced Learning Curve

**Risk Level:** Medium
**Impact:** Slower initial development

**Mitigation:**
- Study Iced examples and documentation thoroughly
- Start with simple views, iterate to complexity
- Use egui for debug overlay (familiar territory)
- Budget extra time for Week 1 (learning phase)

**Resources:**
- [Iced GitHub Examples](https://github.com/iced-rs/iced/tree/master/examples)
- [Iced Book](https://book.iced.rs/)

### Challenge 2: Run-Ahead Complexity

**Risk Level:** High
**Impact:** Latency reduction may not work as expected

**Mitigation:**
- Implement basic run-ahead (RA=1) in Phase 1 as proof of concept
- Ensure save states are fast (<1ms serialization)
- Test determinism thoroughly (use same ROM across restores)
- Defer dual-instance audio to Phase 2
- Provide fallback to traditional emulation (RA=0)

**Success Criteria:**
- RA=1 provides measurable latency improvement (test with slow-motion camera)
- No visual glitches or audio crackling
- <10% performance overhead

### Challenge 3: Cross-Platform Audio Latency

**Risk Level:** Medium
**Impact:** Audio crackling or high latency on some platforms

**Mitigation:**
- Test on all platforms early (Windows WASAPI, macOS CoreAudio, Linux ALSA)
- Implement adaptive buffer sizing (auto-tune based on underruns)
- Provide ASIO support on Windows (for pro audio users)
- Monitor latency metrics in status bar

**Known Issues:**
- Linux PulseAudio can have higher latency than ALSA (document workaround)
- Windows WASAPI shared mode has higher latency than exclusive

### Challenge 4: CRT Shader Performance

**Risk Level:** Low
**Impact:** 60 FPS not achievable with complex shaders

**Mitigation:**
- Implement shader complexity levels (Low/Medium/High)
- Profile each shader preset on target hardware
- Provide "Performance Mode" that disables shaders
- Use pre-compiled SPIR-V shaders (faster than runtime compilation)

**Fallback:**
- Nearest-neighbor scaling with scanlines only (minimal overhead)

### Challenge 5: HTPC Mode UX

**Risk Level:** Medium
**Impact:** Controller-first navigation may be awkward

**Mitigation:**
- Study existing HTPC interfaces (Kodi, Steam Big Picture, RetroArch)
- Implement controller navigation early in Phase 3
- User testing with actual controllers and 10-foot displays
- Provide keyboard shortcuts as fallback

---

## Testing Strategy

### Unit Tests

```rust
// Example test structure
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_ahead_state_preservation() {
        let mut console = Console::new(rom);
        let state_before = console.save_state();

        // Emulate 2 frames
        console.step_frame();
        console.step_frame();

        // Load state (should be identical to before)
        console.load_state(&state_before);
        let state_after = console.save_state();

        assert_eq!(state_before, state_after);
    }

    #[test]
    fn test_audio_resampling() {
        let apu_samples = vec![/* 44.1kHz samples */];
        let resampled = resample_to_48khz(&apu_samples);

        assert_eq!(resampled.len(), (apu_samples.len() as f32 * 48000.0 / 44100.0) as usize);
    }
}
```

### Integration Tests

- ROM loading (valid/invalid files)
- Save state persistence (save → load → verify)
- Controller hot-plugging (connect → disconnect → reconnect)
- Theme switching (light → dark → retro)

### Performance Tests

```bash
# Benchmark frame rendering
cargo bench --bench frame_rendering

# Profile run-ahead overhead
cargo run --release -- --benchmark 1000 game.nes --run-ahead 2

# Measure input latency (requires external hardware)
# Use high-speed camera + LED on button press
```

### Manual Testing Checklist

Phase 1 (MVP):
- [ ] Load 10 different ROMs
- [ ] Test keyboard controls (all buttons)
- [ ] Test gamepad controls (Xbox, PlayStation, Switch Pro)
- [ ] Verify 60 FPS rendering (use FPS counter)
- [ ] Verify audio plays without crackling
- [ ] Test save states (quick save/load)
- [ ] Verify run-ahead reduces latency (camera test)
- [ ] Test on Windows 10+, macOS 12+, Ubuntu 22.04+

Phase 2 (Polish):
- [ ] Test all CRT shader presets
- [ ] Verify settings persistence
- [ ] Test ROM library scanner (1000+ ROMs)
- [ ] Verify box art scraping
- [ ] Test per-game profiles
- [ ] Verify auto-detect run-ahead

Phase 3 (Advanced):
- [ ] Test HTPC mode with controller only
- [ ] Verify netplay (LAN + Internet)
- [ ] Test achievement unlock flow
- [ ] Verify plugin loading

Phase 4 (Tools):
- [ ] Test debugger (breakpoints, memory editor)
- [ ] Test TAS recording/playback
- [ ] Verify Lua scripting

---

## Platform-Specific Notes

### Windows

**Dependencies:** None (static linking recommended)

**Features:**
- WASAPI audio (low latency)
- Optional ASIO support (pro audio, <5ms latency)
- XInput gamepad support (Xbox controllers)

**Packaging:**
- `.exe` installer (Inno Setup recommended)
- Portable `.zip` (no installation required)

**Testing:**
- Windows 10 (version 21H2+)
- Windows 11

**Known Issues:**
- DPI scaling may affect rendering (test on high-DPI displays)

### macOS

**Dependencies:** None

**Features:**
- CoreAudio (excellent latency, <10ms typical)
- Metal backend (wgpu → Metal)
- System gamepad support

**Packaging:**
- `.dmg` disk image
- `.app` bundle (code signing required for distribution)

**Testing:**
- macOS 12 (Monterey) minimum
- Test on Apple Silicon (M1/M2/M3) + Intel

**Known Issues:**
- Notarization required for distribution (Apple Developer account)
- Retina displays: Test integer scaling

### Linux

**Dependencies:**
- libxcb (X11 support)
- libasound / libpulse (audio)
- libudev (gamepad detection)

**Features:**
- Vulkan backend (wgpu → Vulkan)
- ALSA / PulseAudio / JACK audio
- Wayland support (via winit)

**Packaging:**
- AppImage (universal, no dependencies)
- Flatpak (sandboxed, Flathub distribution)
- `.deb` package (Debian/Ubuntu)

**Testing:**
- Ubuntu 22.04 LTS (baseline)
- Fedora 38+ (cutting-edge)
- Arch Linux (latest packages)

**Known Issues:**
- PulseAudio latency higher than ALSA (document ALSA configuration)
- Wayland may have different input handling than X11

---

## CLI Interface (Phase 3 Feature)

Based on v2 design, RustyNES includes a full CLI for automation:

```bash
# Basic usage
rustynes <rom_file>                    # Launch GUI and play ROM
rustynes --headless <rom_file>         # Run without GUI (testing)

# Configuration
rustynes --config <path>               # Use specific config file
rustynes --profile <name>              # Use named game profile
rustynes --run-ahead <0-4>             # Set run-ahead frames
rustynes --shader <preset>             # Set CRT shader preset

# Recording/TAS
rustynes --record <output.fm2>         # Record input to FM2 format
rustynes --playback <input.fm2>        # Play back recorded input
rustynes --benchmark <frames>          # Run benchmark for N frames

# Library management
rustynes scan <directory>              # Scan directory for ROMs
rustynes scrape                        # Update metadata from online
rustynes export-saves <directory>      # Export all saves

# Examples
rustynes "Super Mario Bros.nes" --run-ahead 2 --shader crt-lottes
rustynes --headless --benchmark 10000 game.nes  # Performance test
rustynes scan ~/ROMs/NES --scrape              # Build library
```

**Implementation:**
- Use `clap` crate for argument parsing
- Headless mode: Skip Iced initialization, run emulation loop directly
- Benchmark mode: Measure FPS over N frames, report statistics

---

## Conclusion

This planning document consolidates the comprehensive UI/UX design specifications (v1 and v2) into actionable implementation guidance for Milestone 6. The key takeaways:

### Technology Decisions

1. **Iced (not egui) as primary framework**
   - Justified by application complexity (8+ views)
   - Superior animation and theming support
   - Elm architecture prevents state bugs

2. **Hybrid architecture**
   - Iced for user-facing UI
   - egui for developer tools (debugger)
   - Best of both worlds

3. **Run-ahead as first-class feature**
   - Competitive advantage over most emulators
   - Requires robust save state system
   - Measurable latency reduction

4. **HTPC mode as differentiator**
   - Living room gaming experience
   - Controller-first navigation
   - Automatic metadata scraping

### Implementation Phases

- **Phase 1 (Weeks 1-2):** Functional MVP with basic run-ahead
- **Phase 2 (Weeks 3-4):** Polished UI with full latency system
- **Phase 3 (Weeks 5-8):** HTPC mode, advanced shaders, plugins
- **Phase 4 (Weeks 9-12):** Debug tools, TAS editor

### Research Validation

All technology choices validated through:
- Performance benchmarks (wgpu at 60 FPS for NES trivial)
- Industry adoption (cpal standard for Rust audio)
- Reference implementations (TetaNES for wgpu, Plastic for gilrs)
- Community feedback (Iced vs egui tradeoffs well-documented)

### Next Steps

1. Review this document with project stakeholders
2. Update M6-OVERVIEW.md to reflect Iced decision
3. Create detailed sprint task breakdowns for M6-S1 through M6-S5
4. Begin Week 1 implementation (Iced application shell)

---

**Document Status:** ✅ COMPLETE
**Review Status:** Pending
**Implementation Status:** Ready to begin Phase 1

**Related Files:**
- `/home/parobek/Code/RustyNES/ref-docs/RustyNES-UI_UX-Design-v2.md`
- `/home/parobek/Code/RustyNES/to-dos/phase-1-mvp/milestone-6-gui/M6-OVERVIEW.md`
- Sprint files: M6-S1 through M6-S5 (require updates to reflect Iced)

---

## ADDENDUM: M6 Rephasing Implementation (December 19, 2025)

### Status Update

Following comprehensive analysis of the UI/UX Design v2.0.0 specification and technology research, the following reorganization has been completed:

### Files Updated

| File | Status | Changes |
|------|--------|---------|
| **M6-OVERVIEW.md** | ✅ COMPLETE | Rewritten for Iced 0.13+, reduced scope to 4 weeks MVP |
| **M6-S1-iced-application.md** | ✅ COMPLETE | New sprint file replacing egui-based S1 |
| **M6-REORGANIZATION-SUMMARY.md** | ✅ COMPLETE | Comprehensive change summary document |
| M6-S2-wgpu-rendering.md | 🔄 PENDING | Requires update for Iced integration |
| M6-S3-audio-output.md | 🔄 PENDING | Requires rewrite (input + library merge) |
| M6-S4-controller-support.md | 🔄 PENDING | Requires rewrite (settings + persistence) |
| M6-S5-configuration-polish.md | 🔄 PENDING | Requires rewrite (polish + basic run-ahead) |

### Feature Rephasing Summary

**PHASE 1 (M6): MVP Core - 4 Weeks**
- ✅ Iced application foundation (Elm architecture)
- ✅ wgpu game viewport (60 FPS)
- ✅ cpal audio output (<20ms latency)
- ✅ gilrs gamepad + keyboard input
- ✅ ROM library browser (Grid/List views)
- ✅ Settings persistence (TOML)
- ✅ Basic CRT shader (3-5 presets)
- ✅ **Basic run-ahead (RA=1)** - architectural foundation

**PHASE 2 (M7-M10): Advanced Features - 4 Months**
- ➡️ **M7:** Advanced Run-Ahead (RA=0-4, auto-detect, frame delay, dual-instance)
- ✅ **M8:** GGPO Netplay (unchanged)
- ➡️ **M9:** TAS recording/playback (enhanced from scripting milestone)
- ➡️ **M10:** Debugger with egui overlay integration

**PHASE 3 (M11-M14): Expansion - 6 Months**
- ➡️ **M11:** Advanced CRT Shaders (12+ presets, rolling scan, phosphor masks)
- ✅ **M12:** Expansion Audio (unchanged)
- ➡️ **M13:** HTPC Mode (Cover Flow, Virtual Shelf, controller-first UI)
- ➡️ **M14:** Plugin Architecture (shaders, input mappers, cloud sync, Discord)

**PHASE 4 (M15-M18): Polish - 6 Months**
- ➡️ **M15:** Advanced Shader Pipeline (optimization, pre-compiled SPIR-V)
- ➡️ **M16:** TAS Editor (piano roll interface, greenzone, multi-branch)
- ➡️ **M17:** Full Run-Ahead Optimization (performance profiling, memory pools)
- ➡️ **M18:** CLI Automation + Full Accessibility Audit

### Key Architectural Decisions

1. **Framework Change:** egui → Iced 0.13+ (Elm architecture)
   - Rationale: 8+ major views require structured state management
   - egui retained for debug overlay only (M10)

2. **Timeline Reduction:** 12 weeks → 4 weeks MVP
   - Playable emulator delivered faster
   - Advanced features properly distributed across phases

3. **Run-Ahead Phasing:**
   - M6 (Phase 1): Basic RA=1 for architectural foundation
   - M7 (Phase 2): Advanced RA=0-4 with auto-detection
   - M17 (Phase 4): Performance optimization

4. **HTPC Mode Deferral:** M6 → M13 (Phase 3)
   - Requires mature ROM library foundation
   - Cover Flow and Virtual Shelf need metadata scraping infrastructure

5. **Debugger Integration:** Delayed to M10 (Phase 2)
   - Uses egui for immediate-mode debug tools
   - Properly integrates with Iced via overlay layer

### Next Actions

**Immediate (Sprint M6-S1):**
1. ✅ Complete M6-REORGANIZATION-SUMMARY.md
2. ✅ Update M6-PLANNING-CHANGES.md (this addendum)
3. 🔄 Rewrite M6-S2 through M6-S5 sprint files
4. 🔄 Update Phase 2-4 milestone README files
5. ▶️ Begin M6-S1 implementation (Iced application shell)

**Phase 2 Planning:**
- Create M7-OVERVIEW.md for Advanced Run-Ahead milestone
- Detail auto-detection algorithm and dual-instance mode
- Plan frame delay auto-tuning system

### Success Criteria for M6 Completion

✅ **M6 MVP Complete When:**
- Iced application runs on Linux, Windows, macOS
- ROM loading functional via file dialog
- 60 FPS emulation with wgpu rendering
- Audio output without crackling (<20ms latency)
- Keyboard + gamepad input working
- ROM library Grid/List views operational
- Settings persist across restarts (TOML)
- Basic CRT shader functional (3-5 presets)
- Basic run-ahead (RA=1) reduces latency measurably
- Zero clippy warnings, zero unsafe code
- All tests pass on all platforms

---

**Addendum Status:** ✅ COMPLETE
**Date:** December 19, 2025
**Next Review:** After M6-S1 completion
