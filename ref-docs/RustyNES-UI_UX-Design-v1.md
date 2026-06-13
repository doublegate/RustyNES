# RustyNES UI/UX Design Specification

**Document Version:** 1.0.0  
**Last Updated:** December 19, 2025  
**Status:** Design Specification (design history)  
**Target:** Phase 1 MVP + Advanced Features

---

> **Status (v1.0.0 — what shipped).** This is the original aspirational UI/UX
> spec, retained as design history. The shipped RustyNES v1.0.0 frontend is built
> on **winit 0.30 + wgpu + egui 0.29 + cpal** (NOT eframe/glow, NOT SDL2; the
> framework decision in `RustyNES-GUI_Framework-Change.md` was superseded). What
> actually shipped from this UX vision: an always-on egui menu bar
> (File / Emulation / Tools / View / Debug / Help) plus a status bar (ROM name,
> run state, fading messages, FPS), toggled with `M`; Open Recent (MRU, max 10,
> persisted) + Clear Recent; a tabbed **Settings** window (Display / Audio /
> Input / Advanced); **themes** (Light / Dark / System); an **8:7 pixel-aspect**
> correction toggle; Window Size 1x-4x that scales only the game (chrome stays a
> fixed readable size, the game letterboxes); Fullscreen; a first-run **Welcome**
> modal ("Get Started" / "Keyboard Shortcuts"); an **About** window; an opt-in
> **Pause When Unfocused**; and the egui debugger overlay (CPU/PPU/APU/memory/
> OAM/mapper panels). Many of the more elaborate visions below (Cover Flow / 10-
> foot HTPC shelf, animated cartridge-insert motion design, full game-library
> manager) are design intent, not shipped state. See v2 of this spec for the
> expanded vision; read both as design history.

---

## Table of Contents

1. [Design Philosophy](#design-philosophy)
2. [Visual Design Language](#visual-design-language)
3. [Technology Stack](#technology-stack)
4. [Application Architecture](#application-architecture)
5. [Core Interface Components](#core-interface-components)
6. [Main Views & Layouts](#main-views--layouts)
7. [Animation & Motion Design](#animation--motion-design)
8. [Feature-Specific UI](#feature-specific-ui)
9. [Accessibility & Inclusivity](#accessibility--inclusivity)
10. [Performance Requirements](#performance-requirements)
11. [Implementation Roadmap](#implementation-roadmap)

---

## Design Philosophy

### Core Principles

RustyNES's interface embodies **"Nostalgic Futurism"** — honoring the NES's iconic 8-bit heritage while delivering a modern, buttery-smooth experience that surpasses every existing emulator.

#### 1. **Playful Authenticity**

The UI should feel like stepping into a 1980s living room with a time-traveling upgrade. Every interaction evokes the tactile joy of inserting a cartridge, pressing power, and hearing that familiar hum — but with zero friction.

#### 2. **Invisible Complexity**

Advanced features (debugging, TAS tools, netplay) remain hidden until needed. The interface scales from "plug and play" simplicity to professional-grade tooling without overwhelming the user.

#### 3. **Fluid Responsiveness**

Every interaction responds in under 8ms (half a frame). Animations run at 120Hz+ where hardware permits. The interface never blocks, stutters, or drops frames.

#### 4. **Contextual Intelligence**

The UI anticipates user needs: recently played games surface automatically, save states organize themselves, and settings adapt to detected hardware.

#### 5. **Delight in Details**

Micro-interactions, Easter eggs, and attention to pixel-perfect alignment create an experience worth exploring. Every tooltip, every transition, every sound effect is intentional.

---

## Visual Design Language

### Color Palette

```
┌─────────────────────────────────────────────────────────────────────┐
│                        RUSTYNES COLOR SYSTEM                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  PRIMARY PALETTE (NES-Inspired)                                     │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐       │
│  │ #1A1A2E │ │ #16213E │ │ #0F3460 │ │ #E94560 │ │ #FF6B6B │       │
│  │ Console │ │  Deep   │ │  NES    │ │ Power   │ │  Coral  │       │
│  │  Black  │ │  Navy   │ │  Blue   │ │   Red   │ │  Accent │       │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘       │
│                                                                     │
│  SECONDARY PALETTE (CRT Glow)                                       │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐       │
│  │ #00FF88 │ │ #00D4FF │ │ #FFD93D │ │ #C084FC │ │ #F8F8F2 │       │
│  │Phosphor │ │  Cyan   │ │  Gold   │ │  Purple │ │  White  │       │
│  │  Green  │ │  Glow   │ │ Accent  │ │   Glow  │ │  Text   │       │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘       │
│                                                                     │
│  SEMANTIC COLORS                                                    │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐                   │
│  │ #22C55E │ │ #EAB308 │ │ #EF4444 │ │ #3B82F6 │                   │
│  │ Success │ │ Warning │ │  Error  │ │  Info   │                   │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Typography

**Primary Font Stack:**
```css
/* UI Text */
font-family: "JetBrains Mono", "Cascadia Code", "Fira Code", monospace;

/* Headers & Branding */
font-family: "Press Start 2P", "VT323", "Perfect DOS VGA 437", monospace;

/* Fallback for Accessibility */
font-family: system-ui, -apple-system, sans-serif;
```

**Font Scale (8px base grid):**
```
XS:   10px / 0.625rem  — Tooltips, timestamps
SM:   12px / 0.75rem   — Secondary text, labels
BASE: 14px / 0.875rem  — Body text, menus
MD:   16px / 1rem      — Emphasized text
LG:   20px / 1.25rem   — Section headers
XL:   24px / 1.5rem    — View titles
2XL:  32px / 2rem      — Hero text
```

### Iconography

**Icon Style: Pixel Art + Modern Hybrid**

```
┌──────────────────────────────────────────────────────────────┐
│  ICON DESIGN PRINCIPLES                                      │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  • 16×16 and 24×24 base sizes (NES-authentic grids)          │
│  • Clean pixel art with subtle anti-aliasing on hover        │
│  • Monochrome by default, colorized on interaction           │
│  • Consistent 2px stroke weight                              │
│  • Animated icon states (idle → hover → active)              │
│                                                              │
│  EXAMPLE ICON SET:                                           │
│                                                              │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐    │
│  │ ▶  │ │ ⏸ │ │ ⏹ │  │ ⏪ │ │ ⏩ │ │ 💾 │  │ 📁 │ │ ⚙  │    │
│  │Play│ │Paus│ │Stop│ │Rwnd│ │FFwd│ │Save│ │Load│ │Conf│    │
│  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘    │
│                                                              │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐    │
│  │ 🎮 │ │ 🔊  │ │ 🔇 │ │ 🖥 │ │ 🐛  │ │ 📡 │ │ 🏆 │  │ 📝 │    │
│  │Ctrl│ │ Vol│ │Mute│ │Full│ │Debg│ │ Net│ │Achv│ │ TAS│    │
│  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘    │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

### Depth & Elevation

**Layering System (inspired by CRT depth):**

```
┌─────────────────────────────────────────────────────────────────┐
│  ELEVATION LEVELS                                               │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Level 0: Background        — Console body, deep shadows        │
│  Level 1: Surface           — Main panels, game viewport        │
│  Level 2: Raised            — Cards, buttons, menu items        │
│  Level 3: Floating          — Dropdowns, tooltips               │
│  Level 4: Overlay           — Modals, quick menu                │
│  Level 5: Critical          — Alerts, confirmation dialogs      │
│                                                                 │
│  SHADOW STYLE (Soft CRT Glow):                                  │
│                                                                 │
│  Level 2: box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3),            │
│                       0 0 20px rgba(15, 52, 96, 0.1);           │
│                                                                 │
│  Level 4: box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5),           │
│                       0 0 60px rgba(233, 69, 96, 0.15);         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Border & Corner Radii

```
Sharp:      0px    — Pixel-perfect elements, retro buttons
Subtle:     4px    — Input fields, small cards
Rounded:    8px    — Panels, menu containers
Soft:       12px   — Large cards, modal windows
Pill:       9999px — Tags, badges, toggle switches
```

---

## Technology Stack

### Core Framework: Iced 0.13+ (Primary) + egui (Overlay)

**Why Iced over pure egui?**

| Aspect | Iced | egui | Decision |
|--------|------|------|----------|
| **Rendering Model** | Retained + Immediate hybrid | Pure immediate | Iced for main UI, egui for debug overlays |
| **Animation Support** | Native subscriptions, smooth | Manual, per-frame | Iced for fluid animations |
| **Styling Flexibility** | Theme system, CSS-like | Inline styles | Iced for consistent theming |
| **GPU Integration** | wgpu native | wgpu via egui_wgpu | Equal, both excellent |
| **Learning Curve** | Elm architecture | Simpler | Iced's architecture scales better |
| **Custom Rendering** | Canvas widget | Painter API | Both capable |

**Hybrid Architecture:**
```
┌─────────────────────────────────────────────────────────────────┐
│                    RUSTYNES UI STACK                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    ICED APPLICATION                      │   │
│  │  • Main window chrome                                    │   │
│  │  • ROM browser & library                                 │   │
│  │  • Settings panels                                       │   │
│  │  • Netplay lobby                                         │   │
│  │  • Achievement overlays                                  │   │
│  └─────────────────────────────────────────────────────────┘   │
│                          │                                      │
│                          ▼                                      │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                  WGPU RENDER LAYER                       │   │
│  │  • Game viewport (256×240 → scaled)                      │   │
│  │  • CRT shaders & filters                                 │   │
│  │  • Scanline effects                                      │   │
│  │  • Phosphor glow                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
│                          │                                      │
│                          ▼                                      │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                 EGUI DEBUG OVERLAY                       │   │
│  │  • CPU/PPU/APU state viewers                            │   │
│  │  • Memory hex editor                                     │   │
│  │  • Trace logger                                          │   │
│  │  • Lua console                                           │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Complete Dependency Graph

```toml
[dependencies]
# ═══════════════════════════════════════════════════════════════
# UI FRAMEWORK
# ═══════════════════════════════════════════════════════════════
iced = { version = "0.13", features = [
    "wgpu",           # GPU-accelerated rendering
    "advanced",       # Custom shaders
    "tokio",          # Async runtime integration
    "image",          # Image loading
    "svg",            # Vector graphics
    "canvas",         # Custom drawing
    "lazy",           # Virtual scrolling
    "debug",          # Debug overlay
]}
iced_aw = "0.10"      # Additional widgets (badges, cards, modals)

# Debug overlay (for developer tools)
egui = "0.28"
egui-wgpu = "0.28"
egui-winit = "0.28"

# ═══════════════════════════════════════════════════════════════
# GRAPHICS & SHADERS
# ═══════════════════════════════════════════════════════════════
wgpu = "0.20"
naga = "0.20"         # Shader compilation
image = "0.25"        # Image loading/processing
resvg = "0.42"        # SVG rendering

# ═══════════════════════════════════════════════════════════════
# AUDIO
# ═══════════════════════════════════════════════════════════════
cpal = "0.15"         # Cross-platform audio
rubato = "0.15"       # High-quality resampling
dasp = "0.11"         # Digital audio signal processing

# ═══════════════════════════════════════════════════════════════
# INPUT
# ═══════════════════════════════════════════════════════════════
gilrs = "0.10"        # Gamepad support
winit = "0.30"        # Window/input events

# ═══════════════════════════════════════════════════════════════
# FILE SYSTEM & PERSISTENCE
# ═══════════════════════════════════════════════════════════════
rfd = "0.14"          # Native file dialogs
notify = "6.1"        # File system watching (hot reload)
directories = "5.0"   # Platform-specific paths
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"          # Config files
bincode = "1.3"       # Fast serialization (savestates)
lz4_flex = "0.11"     # Compression

# ═══════════════════════════════════════════════════════════════
# ASYNC & CONCURRENCY
# ═══════════════════════════════════════════════════════════════
tokio = { version = "1.40", features = ["full"] }
crossbeam-channel = "0.5"
parking_lot = "0.12"

# ═══════════════════════════════════════════════════════════════
# NETWORKING (Netplay)
# ═══════════════════════════════════════════════════════════════
backroll = "0.3"      # GGPO rollback
quinn = "0.11"        # QUIC protocol
webrtc = "0.11"       # Browser-compatible P2P
matchbox_socket = "0.10" # WebRTC signaling

# ═══════════════════════════════════════════════════════════════
# SCRIPTING & DEBUGGING
# ═══════════════════════════════════════════════════════════════
mlua = { version = "0.9", features = ["lua54", "vendored", "async"] }

# ═══════════════════════════════════════════════════════════════
# ACHIEVEMENTS
# ═══════════════════════════════════════════════════════════════
rcheevos = "0.2"      # RetroAchievements (pure Rust reimplementation)

# ═══════════════════════════════════════════════════════════════
# UTILITIES
# ═══════════════════════════════════════════════════════════════
chrono = "0.4"        # Date/time
humantime = "2.1"     # Human-readable durations
fuzzy-matcher = "0.3" # Fuzzy search
unicode-segmentation = "1.11" # Text handling
```

---

## Application Architecture

### State Management (Elm Architecture)

```rust
/// Root application state
pub struct RustyNes {
    // ═══════════════════════════════════════════════════════════
    // CORE STATE
    // ═══════════════════════════════════════════════════════════

    /// Current view/screen
    view: View,

    /// Emulator core (optional - None when no ROM loaded)
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
}

/// All possible views
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    /// Welcome screen (no ROM loaded)
    Welcome,

    /// ROM library browser
    Library,

    /// Active gameplay
    Playing,

    /// Settings panel
    Settings(SettingsTab),

    /// Netplay lobby
    NetplayLobby,

    /// Achievement browser
    Achievements,

    /// Debugger (advanced)
    Debugger,

    /// TAS editor
    TasEditor,
}

/// Application messages
#[derive(Debug, Clone)]
pub enum Message {
    // ═══════════════════════════════════════════════════════════
    // NAVIGATION
    // ═══════════════════════════════════════════════════════════
    NavigateTo(View),
    GoBack,

    // ═══════════════════════════════════════════════════════════
    // EMULATION CONTROL
    // ═══════════════════════════════════════════════════════════
    LoadRom(PathBuf),
    RomLoaded(Result<Console, EmulatorError>),
    Play,
    Pause,
    Reset,
    PowerCycle,

    // ═══════════════════════════════════════════════════════════
    // SAVE STATES
    // ═══════════════════════════════════════════════════════════
    SaveState(u8),         // Slot 0-9
    LoadState(u8),
    QuickSave,
    QuickLoad,

    // ═══════════════════════════════════════════════════════════
    // INPUT
    // ═══════════════════════════════════════════════════════════
    ControllerInput(u8, ControllerState),
    KeyboardInput(KeyEvent),
    GamepadConnected(GamepadId),
    GamepadDisconnected(GamepadId),

    // ═══════════════════════════════════════════════════════════
    // QUICK MENU
    // ═══════════════════════════════════════════════════════════
    ToggleQuickMenu,
    QuickMenuAction(QuickAction),

    // ═══════════════════════════════════════════════════════════
    // SETTINGS
    // ═══════════════════════════════════════════════════════════
    UpdateSetting(SettingKey, SettingValue),
    ResetToDefaults,
    ImportSettings(PathBuf),
    ExportSettings(PathBuf),

    // ═══════════════════════════════════════════════════════════
    // LIBRARY
    // ═══════════════════════════════════════════════════════════
    ScanRomDirectory(PathBuf),
    ScanComplete(Vec<RomEntry>),
    SearchLibrary(String),
    SortLibrary(SortOrder),
    FilterLibrary(Filter),

    // ═══════════════════════════════════════════════════════════
    // NETPLAY
    // ═══════════════════════════════════════════════════════════
    HostSession,
    JoinSession(SessionCode),
    NetplayConnected(PeerId),
    NetplayDisconnected,
    NetplaySync(SyncState),

    // ═══════════════════════════════════════════════════════════
    // ACHIEVEMENTS
    // ═══════════════════════════════════════════════════════════
    AchievementLogin(String, String),
    AchievementUnlocked(Achievement),
    AchievementProgress(AchievementId, f32),

    // ═══════════════════════════════════════════════════════════
    // TAS
    // ═══════════════════════════════════════════════════════════
    StartRecording,
    StopRecording,
    PlaybackMovie(PathBuf),
    SeekFrame(u64),

    // ═══════════════════════════════════════════════════════════
    // UI
    // ═══════════════════════════════════════════════════════════
    ShowToast(Toast),
    DismissToast(ToastId),
    ShowModal(Modal),
    DismissModal,
    ThemeChanged(ThemeVariant),
    WindowEvent(WindowEvent),

    // ═══════════════════════════════════════════════════════════
    // SYSTEM
    // ═══════════════════════════════════════════════════════════
    Tick(Instant),  // 60Hz+ animation tick
    AudioCallback,
    Exit,
}
```

### Component Hierarchy

```
┌─────────────────────────────────────────────────────────────────────┐
│                         APPLICATION SHELL                           │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                      TITLE BAR (Custom)                       │ │
│  │  [🎮 RustyNES]  [─] [□] [×]               FPS: 60.0 | 16.2ms  │ │
│  └───────────────────────────────────────────────────────────────┘ │
│  ┌──────────┬────────────────────────────────────────────────────┐ │
│  │          │                                                    │ │
│  │  SIDEBAR │                   MAIN CONTENT                     │ │
│  │          │                                                    │ │
│  │ [🏠 Home]│  ┌────────────────────────────────────────────┐   │ │
│  │ [📚 Lib] │  │                                            │   │ │
│  │ [⚙ Set] │  │              VIEW CONTAINER                │   │ │
│  │ [📡 Net] │  │                                            │   │ │
│  │ [🏆 Ach] │  │    (Welcome / Library / Playing / etc.)   │   │ │
│  │ [🐛 Dbg] │  │                                            │   │ │
│  │ [📝 TAS] │  │                                            │   │ │
│  │          │  └────────────────────────────────────────────┘   │ │
│  │          │                                                    │ │
│  │  ──────  │  ┌────────────────────────────────────────────┐   │ │
│  │ Recently │  │              STATUS BAR                    │   │ │
│  │ [SMB3]   │  │  [🔊 100%] [▶ Running] [Frame: 123456]     │   │ │
│  │ [Zelda]  │  └────────────────────────────────────────────┘   │ │
│  │ [Mega]   │                                                    │ │
│  └──────────┴────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                     TOAST CONTAINER                           │ │
│  │  [Achievement Unlocked! "First Steps" +10 pts     ✕]          │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                    QUICK MENU (Overlay)                       │ │
│  │             (Shown on Escape / Start+Select)                  │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                     MODAL CONTAINER                           │ │
│  │                   (Dialogs, Confirmations)                    │ │
│  └───────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Core Interface Components

### 1. Custom Title Bar

```
┌─────────────────────────────────────────────────────────────────────┐
│                        CUSTOM TITLE BAR                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ [🎮]  RustyNES v0.3.0  │  Super Mario Bros. 3  │  ▶ 60.0 FPS │   │
│  │                                                              │   │
│  │              ← Draggable area →              [─] [□] [×]     │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  FEATURES:                                                          │
│  • Custom window chrome (no native decorations)                     │
│  • Draggable anywhere in title bar                                  │
│  • Double-click to maximize                                         │
│  • Right-click context menu                                         │
│  • Real-time FPS counter with color coding:                         │
│    - Green (58-62 FPS): Perfect                                     │
│    - Yellow (50-57 FPS): Slight slowdown                           │
│    - Red (<50 FPS): Performance issue                              │
│  • Current game title (auto-detected from ROM header)               │
│  • Emulation state indicator (▶ Playing / ⏸ Paused / ⏹ Stopped)   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 2. Collapsible Sidebar

```rust
pub struct Sidebar {
    expanded: bool,
    selected: SidebarItem,
    recent_roms: Vec<RecentRom>,
    hover_item: Option<SidebarItem>,
}

impl Sidebar {
    pub fn view(&self) -> Element<Message> {
        let width = if self.expanded { 220 } else { 56 };

        container(
            column![
                // Logo
                self.logo_section(),

                // Main navigation
                self.nav_section(),

                // Divider
                horizontal_rule(1),

                // Recent ROMs (scrollable)
                self.recent_section(),

                // Bottom actions
                self.bottom_section(),
            ]
            .spacing(8)
        )
        .width(width)
        .style(theme::Container::Sidebar)
    }

    fn nav_item(&self, item: SidebarItem, icon: &str, label: &str) -> Element<Message> {
        let is_selected = self.selected == item;
        let is_hovered = self.hover_item == Some(item);

        button(
            row![
                text(icon).size(20),
                if self.expanded {
                    text(label).size(14)
                } else {
                    text("")
                }
            ]
            .spacing(12)
            .align_items(Alignment::Center)
        )
        .style(if is_selected {
            theme::Button::SidebarActive
        } else if is_hovered {
            theme::Button::SidebarHover
        } else {
            theme::Button::Sidebar
        })
        .on_press(Message::NavigateTo(item.into()))
        .padding([12, 16])
        .width(if self.expanded { Length::Fill } else { Length::Shrink })
    }
}
```

### 3. Game Viewport with CRT Frame

```
┌─────────────────────────────────────────────────────────────────────┐
│                        GAME VIEWPORT                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────┐     │
│  │ ╭─────────────────────────────────────────────────────╮   │     │
│  │ │                                                     │   │     │
│  │ │   ┌───────────────────────────────────────────┐     │   │     │
│  │ │   │░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│     │   │     │
│  │ │   │░░░░░░░░░░░░░ GAME SCREEN ░░░░░░░░░░░░░░░░│     │   │     │
│  │ │   │░░░░░░░░░░░░░  256 × 240  ░░░░░░░░░░░░░░░░│     │   │     │
│  │ │   │░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│     │   │     │
│  │ │   │░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│     │   │     │
│  │ │   └───────────────────────────────────────────┘     │   │     │
│  │ │                    Scanlines                        │   │     │
│  │ ╰───────────────────────┬─────────────────────────────╯   │     │
│  │                         │                                 │     │
│  │  CRT Bezel (optional)   │  Phosphor Glow Effect           │     │
│  │                        ═══                                │     │
│  │                    Power LED                              │     │
│  └───────────────────────────────────────────────────────────┘     │
│                                                                     │
│  SHADER OPTIONS:                                                    │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ [✓] Scanlines      [○] Light  [●] Medium  [○] Heavy          │  │
│  │ [✓] CRT Curvature  Intensity: ████████░░ 80%                 │  │
│  │ [✓] Phosphor Glow  Color Bleed: ██████░░░░ 60%               │  │
│  │ [✓] Vignette       Strength: ████░░░░░░ 40%                  │  │
│  │ [○] NTSC Filter    Artifacts: ░░░░░░░░░░ Off                 │  │
│  │ [✓] Integer Scale  Current: 4× (1024 × 960)                  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 4. Quick Menu (Overlay)

```
┌─────────────────────────────────────────────────────────────────────┐
│                         QUICK MENU                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Trigger: ESC key, Start+Select combo, or shoulder buttons         │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                    ╔═══════════════════╗                      │ │
│  │                    ║   QUICK MENU      ║                      │ │
│  │                    ╠═══════════════════╣                      │ │
│  │                    ║ ▶ Resume          ║ ← Selected           │ │
│  │                    ║ ⏸ Pause           ║                      │ │
│  │                    ║ ↺ Reset           ║                      │ │
│  │                    ║ ⏻ Power Cycle     ║                      │ │
│  │                    ╠═══════════════════╣                      │ │
│  │                    ║ 💾 Save State     ║                      │ │
│  │                    ║ 📂 Load State     ║                      │ │
│  │                    ║ ⏪ Rewind         ║                      │ │
│  │                    ╠═══════════════════╣                      │ │
│  │                    ║ ⚙ Quick Settings  ║                      │ │
│  │                    ║ 📤 Exit to Menu   ║                      │ │
│  │                    ╚═══════════════════╝                      │ │
│  │                                                               │ │
│  │  Background: Blurred game + dark overlay (60% opacity)        │ │
│  │  Navigation: D-pad/arrow keys, A/Enter to select              │ │
│  │  Animation: Slide in from right (200ms ease-out)              │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  SAVE STATE SUB-MENU:                                               │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  ╔═══════════════════════════════════════════════╗            │ │
│  │  ║            SAVE STATES                        ║            │ │
│  │  ╠═══════════════════════════════════════════════╣            │ │
│  │  ║  [1] ████████  Dec 19, 2025 - 14:32  World 3  ║            │ │
│  │  ║  [2] ████████  Dec 19, 2025 - 12:15  World 2  ║            │ │
│  │  ║  [3] ░░░░░░░░  Empty                          ║            │ │
│  │  ║  [4] ░░░░░░░░  Empty                          ║            │ │
│  │  ║  [5] ████████  Dec 18, 2025 - 22:47  World 1  ║            │ │
│  │  ║  ...                                          ║            │ │
│  │  ╚═══════════════════════════════════════════════╝            │ │
│  │                                                               │ │
│  │  • Thumbnail preview of each slot (animated on hover)         │ │
│  │  • Timestamp and optional note                                │ │
│  │  • Auto-save indicator (★)                                    │ │
│  │  • 10 manual slots + 1 auto-save + unlimited cloud saves     │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 5. Toast Notifications

```
┌─────────────────────────────────────────────────────────────────────┐
│                      TOAST NOTIFICATIONS                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  POSITION: Bottom-right corner, stacked vertically                  │
│  MAX VISIBLE: 3 toasts at once                                      │
│                                                                     │
│  TOAST TYPES:                                                       │
│                                                                     │
│  ┌────────────────────────────────────────────┐                     │
│  │ ✓  State saved to Slot 3                   │  SUCCESS            │
│  │                                     [×]    │  (Green accent)     │
│  └────────────────────────────────────────────┘                     │
│                                                                     │
│  ┌────────────────────────────────────────────┐                     │
│  │ ⚠  Controller disconnected                 │  WARNING            │
│  │    Press any button to reconnect    [×]    │  (Yellow accent)    │
│  └────────────────────────────────────────────┘                     │
│                                                                     │
│  ┌────────────────────────────────────────────┐                     │
│  │ ✗  Failed to load ROM                      │  ERROR              │
│  │    Invalid header format            [×]    │  (Red accent)       │
│  └────────────────────────────────────────────┘                     │
│                                                                     │
│  ┌────────────────────────────────────────────┐                     │
│  │ 🏆 Achievement Unlocked!                   │  ACHIEVEMENT        │
│  │    "First Steps" - Start your journey      │  (Gold accent)      │
│  │    +10 points                       [×]    │                     │
│  └────────────────────────────────────────────┘                     │
│                                                                     │
│  ANIMATIONS:                                                        │
│  • Entry: Slide in from right + fade (300ms ease-out)               │
│  • Exit: Slide out right + fade (200ms ease-in)                     │
│  • Auto-dismiss: 4 seconds (achievements: 6 seconds)                │
│  • Progress bar for auto-dismiss countdown                          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Main Views & Layouts

### Welcome Screen (First Launch)

```
┌─────────────────────────────────────────────────────────────────────┐
│                       WELCOME SCREEN                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ╔═══════════════════════════════════════════════════════════════╗ │
│  ║                                                               ║ │
│  ║         ██████╗ ██╗   ██╗███████╗████████╗██╗   ██╗           ║ │
│  ║         ██╔══██╗██║   ██║██╔════╝╚══██╔══╝╚██╗ ██╔╝           ║ │
│  ║         ██████╔╝██║   ██║███████╗   ██║    ╚████╔╝            ║ │
│  ║         ██╔══██╗██║   ██║╚════██║   ██║     ╚██╔╝             ║ │
│  ║         ██║  ██║╚██████╔╝███████║   ██║      ██║              ║ │
│  ║         ╚═╝  ╚═╝ ╚═════╝ ╚══════╝   ╚═╝      ╚═╝              ║ │
│  ║                        ███╗   ██╗███████╗███████╗             ║ │
│  ║                        ████╗  ██║██╔════╝██╔════╝             ║ │
│  ║                        ██╔██╗ ██║█████╗  ███████╗             ║ │
│  ║                        ██║╚██╗██║██╔══╝  ╚════██║             ║ │
│  ║                        ██║ ╚████║███████╗███████║             ║ │
│  ║                        ╚═╝  ╚═══╝╚══════╝╚══════╝             ║ │
│  ║                                                               ║ │
│  ║                 Precise. Pure. Powerful.                      ║ │
│  ║                      Version 0.3.0                            ║ │
│  ║                                                               ║ │
│  ║    ╭──────────────────────────────────────────────────╮       ║ │
│  ║    │         [ 📁 Open ROM... ]                       │       ║ │
│  ║    ╰──────────────────────────────────────────────────╯       ║ │
│  ║                                                               ║ │
│  ║    ╭──────────────────────────────────────────────────╮       ║ │
│  ║    │         [ 📚 Browse Library ]                    │       ║ │
│  ║    ╰──────────────────────────────────────────────────╯       ║ │
│  ║                                                               ║ │
│  ║    ╭──────────────────────────────────────────────────╮       ║ │
│  ║    │         [ ⚙ First-Time Setup ]                   │       ║ │
│  ║    ╰──────────────────────────────────────────────────╯       ║ │
│  ║                                                               ║ │
│  ║    ─────────────  OR DROP A ROM FILE HERE  ─────────────      ║ │
│  ║                                                               ║ │
│  ╚═══════════════════════════════════════════════════════════════╝ │
│                                                                     │
│  ANIMATION: Logo pulses with subtle CRT glow effect                 │
│  INTERACTION: Drag-and-drop ROM anywhere on window                  │
│  EASTER EGG: Konami code unlocks retro theme                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### ROM Library Browser

```
┌─────────────────────────────────────────────────────────────────────┐
│                         ROM LIBRARY                                 │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ 🔍 Search: [Super Mario                        ] [🎮] [📅] [↕] │ │
│  │                                                               │ │
│  │ Filters: [All ▼] [Platform ▼] [Mapper ▼] [Year ▼] [Rating ▼] │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  VIEW MODES: [▦ Grid] [≡ List] [◫ Compact]                         │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                                                             │   │
│  │  GRID VIEW (Default):                                       │   │
│  │                                                             │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           │   │
│  │  │ ░░░░░░░ │ │ ░░░░░░░ │ │ ░░░░░░░ │ │ ░░░░░░░ │           │   │
│  │  │ ░░ 📦 ░░ │ │ ░░ 🗡️ ░░ │ │ ░░ 🏃 ░░ │ │ ░░ 🔫 ░░ │           │   │
│  │  │ ░░░░░░░ │ │ ░░░░░░░ │ │ ░░░░░░░ │ │ ░░░░░░░ │           │   │
│  │  │ Box Art │ │ Box Art │ │ Box Art │ │ Box Art │           │   │
│  │  ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤           │   │
│  │  │Super    │ │Legend of│ │Mega Man │ │Contra   │           │   │
│  │  │Mario 3  │ │Zelda    │ │2        │ │         │           │   │
│  │  │────────│ │────────│ │────────│ │────────│           │   │
│  │  │⏱ 4h 32m│ │⏱ 12h 5m│ │⏱ 45m   │ │⏱ Never │           │   │
│  │  │★★★★★  │ │★★★★★  │ │★★★★☆  │ │☆☆☆☆☆  │           │   │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           │   │
│  │                                                             │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           │   │
│  │  │ ░░░░░░░ │ │ ░░░░░░░ │ │ ░░░░░░░ │ │  + Add  │           │   │
│  │  │ ░░ 🏎️ ░░ │ │ ░░ 👊 ░░ │ │ ░░ ⚔️ ░░ │ │  More   │           │   │
│  │  │ ░░░░░░░ │ │ ░░░░░░░ │ │ ░░░░░░░ │ │  ROMs   │           │   │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘           │   │
│  │                                                             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  HOVER STATE: Card lifts, shows "Play" button + quick actions       │
│  ANIMATIONS:                                                        │
│  • Staggered fade-in on load (50ms delay per card)                 │
│  • Smooth scroll with momentum                                      │
│  • Cover art loads progressively (blur → sharp)                    │
│                                                                     │
│  RIGHT-CLICK CONTEXT MENU:                                          │
│  ┌────────────────────────┐                                        │
│  │ ▶ Play                 │                                        │
│  │ 📋 View Details        │                                        │
│  │ ⭐ Add to Favorites    │                                        │
│  │ 📁 Open File Location  │                                        │
│  │ 🗑 Remove from Library │                                        │
│  └────────────────────────┘                                        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Game Detail Panel

```
┌─────────────────────────────────────────────────────────────────────┐
│                      GAME DETAIL PANEL                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                                                             │   │
│  │  ┌───────────────┐  ╭──────────────────────────────────╮   │   │
│  │  │               │  │  SUPER MARIO BROS. 3             │   │   │
│  │  │   ░░░░░░░░░   │  │  ═══════════════════════════════ │   │   │
│  │  │   ░░ BOX ░░   │  │                                  │   │   │
│  │  │   ░░ ART ░░   │  │  Publisher: Nintendo             │   │   │
│  │  │   ░░░░░░░░░   │  │  Year: 1990                      │   │   │
│  │  │               │  │  Mapper: MMC3 (4)                │   │   │
│  │  │  ┌─────────┐  │  │  Region: NTSC                    │   │   │
│  │  │  │CARTRIDGE│  │  │                                  │   │   │
│  │  │  │ PREVIEW │  │  │  ★★★★★  (Your Rating)           │   │   │
│  │  │  └─────────┘  │  │                                  │   │   │
│  │  └───────────────┘  │  ──────────────────────────────  │   │   │
│  │                     │                                  │   │   │
│  │                     │  Play Time: 12h 34m              │   │   │
│  │                     │  Last Played: Today, 2:34 PM     │   │   │
│  │                     │  Times Played: 47                │   │   │
│  │                     │                                  │   │   │
│  │  ╭────────────────╮ │  ──────────────────────────────  │   │   │
│  │  │   ▶ PLAY      │ │                                  │   │   │
│  │  ╰────────────────╯ │  Save States: 3 / 10            │   │   │
│  │                     │  [Slot 1] [Slot 2] [Slot 5]      │   │   │
│  │  ╭────────────────╮ │                                  │   │   │
│  │  │  ↻ Continue   │ │  Achievements: 12 / 45           │   │   │
│  │  │  from Slot 1  │ │  ████████░░░░░░░ 27%             │   │   │
│  │  ╰────────────────╯ ╰──────────────────────────────────╯   │   │
│  │                                                             │   │
│  │  ╭─────────────────────────────────────────────────────╮   │   │
│  │  │  DESCRIPTION                                        │   │   │
│  │  │                                                     │   │   │
│  │  │  The third main installment of the Super Mario     │   │   │
│  │  │  Bros. series, featuring eight unique worlds,      │   │   │
│  │  │  power-ups including the Super Leaf and Tanooki   │   │   │
│  │  │  Suit, and a world map system...                   │   │   │
│  │  │                                              [More] │   │   │
│  │  ╰─────────────────────────────────────────────────────╯   │   │
│  │                                                             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Settings Panel

```
┌─────────────────────────────────────────────────────────────────────┐
│                        SETTINGS PANEL                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌────────────────┬────────────────────────────────────────────┐   │
│  │                │                                            │   │
│  │  CATEGORIES    │          VIDEO SETTINGS                    │   │
│  │                │                                            │   │
│  │  ┌──────────┐  │  ┌──────────────────────────────────────┐ │   │
│  │  │ 🎮 Video │  │  │                                      │ │   │
│  │  └──────────┘  │  │  Display Mode                        │ │   │
│  │  ┌──────────┐  │  │  ◉ Windowed   ○ Fullscreen          │ │   │
│  │  │ 🔊 Audio │  │  │                                      │ │   │
│  │  └──────────┘  │  │  ──────────────────────────────────  │ │   │
│  │  ┌──────────┐  │  │                                      │ │   │
│  │  │ 🎮 Input │  │  │  Scale Mode                          │ │   │
│  │  └──────────┘  │  │  ◉ Integer   ○ Stretch   ○ Fit      │ │   │
│  │  ┌──────────┐  │  │                                      │ │   │
│  │  │ 📁 Paths │  │  │  Scale Factor: [▼ 4×]               │ │   │
│  │  └──────────┘  │  │                                      │ │   │
│  │  ┌──────────┐  │  │  ──────────────────────────────────  │ │   │
│  │  │ 🌐 Network│ │  │                                      │ │   │
│  │  └──────────┘  │  │  SHADERS                             │ │   │
│  │  ┌──────────┐  │  │                                      │ │   │
│  │  │ 🏆 Achiev │  │  │  [✓] Enable Shaders                 │ │   │
│  │  └──────────┘  │  │                                      │ │   │
│  │  ┌──────────┐  │  │  Preset: [▼ CRT Royale]             │ │   │
│  │  │ 🔧 Advanc│  │  │                                      │ │   │
│  │  └──────────┘  │  │  ┌─────────────────────────────────┐│ │   │
│  │                │  │  │ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ ││ │   │
│  │                │  │  │ ░░░░░░░░░ PREVIEW ░░░░░░░░░░░░░ ││ │   │
│  │  ──────────    │  │  │ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ ││ │   │
│  │                │  │  └─────────────────────────────────┘│ │   │
│  │  [Reset All]   │  │                                      │ │   │
│  │                │  │  Scanline Intensity: ████████░░ 80%  │ │   │
│  │                │  │  Curvature:          ██████░░░░ 60%  │ │   │
│  │                │  │  Bloom:              ████░░░░░░ 40%  │ │   │
│  │                │  │  Mask Type: [▼ Aperture Grille]      │ │   │
│  │                │  │                                      │ │   │
│  │                │  └──────────────────────────────────────┘ │   │
│  │                │                                            │   │
│  └────────────────┴────────────────────────────────────────────┘   │
│                                                                     │
│  FEATURES:                                                          │
│  • Live preview of shader changes                                   │
│  • Reset to defaults per-category                                   │
│  • Import/Export settings profiles                                  │
│  • Search settings (Ctrl+F)                                         │
│  • Keyboard navigation (Tab, Arrow keys)                           │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Netplay Lobby

```
┌─────────────────────────────────────────────────────────────────────┐
│                        NETPLAY LOBBY                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                                                               │ │
│  │  ╭─────────────────────────────────────────────────────────╮ │ │
│  │  │                  🌐 NETPLAY                             │ │ │
│  │  ╰─────────────────────────────────────────────────────────╯ │ │
│  │                                                               │ │
│  │  ┌─────────────────────┐  ┌─────────────────────────────┐   │ │
│  │  │                     │  │                             │   │ │
│  │  │   [ 📡 HOST ]       │  │   [ 🔗 JOIN ]              │   │ │
│  │  │                     │  │                             │   │ │
│  │  │   Create a new      │  │   Enter session code:       │   │ │
│  │  │   session for       │  │                             │   │ │
│  │  │   others to join    │  │   [ A B C D - 1 2 3 4 ]     │   │ │
│  │  │                     │  │                             │   │ │
│  │  │   ──────────────    │  │   ─────────────────────     │   │ │
│  │  │                     │  │                             │   │ │
│  │  │   Your Name:        │  │   Or scan QR code:          │   │ │
│  │  │   [ Player1     ]   │  │   ┌─────────────┐           │   │ │
│  │  │                     │  │   │ ░░░ QR ░░░ │           │   │ │
│  │  │   Rollback Frames:  │  │   │ ░░░░░░░░░░ │           │   │ │
│  │  │   [ 2 ▼ ]           │  │   └─────────────┘           │   │ │
│  │  │                     │  │                             │   │ │
│  │  └─────────────────────┘  └─────────────────────────────┘   │ │
│  │                                                               │ │
│  │  ═════════════════════════════════════════════════════════   │ │
│  │                                                               │ │
│  │                    ACTIVE SESSION                             │ │
│  │                                                               │ │
│  │  ┌───────────────────────────────────────────────────────┐   │ │
│  │  │                                                       │   │ │
│  │  │  Session: ABCD-1234          Status: ● Connected      │   │ │
│  │  │                                                       │   │ │
│  │  │  ┌─────────────────┐    ┌─────────────────┐          │   │ │
│  │  │  │  👤 Player 1    │    │  👤 Player 2    │          │   │ │
│  │  │  │  ──────────────  │    │  ──────────────  │          │   │ │
│  │  │  │  You (Host)      │ ⚔ │  Guest          │          │   │ │
│  │  │  │  Ping: 12ms      │    │  Ping: 24ms     │          │   │ │
│  │  │  │  🎮 Ready       │    │  ⏳ Waiting     │          │   │ │
│  │  │  └─────────────────┘    └─────────────────┘          │   │ │
│  │  │                                                       │   │ │
│  │  │  Game: Super Mario Bros. 3                           │   │ │
│  │  │  ROM Match: ✓ Verified (CRC32 matches)               │   │ │
│  │  │                                                       │   │ │
│  │  │  ╭─────────────────────────────────────────────────╮ │   │ │
│  │  │  │              [ 🚀 START GAME ]                  │ │   │ │
│  │  │  ╰─────────────────────────────────────────────────╯ │   │ │
│  │  │                                                       │   │ │
│  │  │  Chat:                                                │   │ │
│  │  │  ┌───────────────────────────────────────────────┐   │   │ │
│  │  │  │ Player1: Ready when you are!                  │   │   │ │
│  │  │  │ Player2: Let's gooo                           │   │   │ │
│  │  │  ├───────────────────────────────────────────────┤   │   │ │
│  │  │  │ [Type message...                        ] [↵] │   │   │ │
│  │  │  └───────────────────────────────────────────────┘   │   │ │
│  │  │                                                       │   │ │
│  │  └───────────────────────────────────────────────────────┘   │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Achievement Browser

```
┌─────────────────────────────────────────────────────────────────────┐
│                      ACHIEVEMENT BROWSER                            │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                                                               │ │
│  │  🏆 RETROACHIEVEMENTS                      [ 🔐 Login ]      │ │
│  │                                                               │ │
│  │  ┌─────────────────────────────────────────────────────────┐ │ │
│  │  │  Welcome, RetroPlayer123!                              │ │ │
│  │  │  Rank: #4,521  |  Points: 12,450  |  Mastered: 8      │ │ │
│  │  └─────────────────────────────────────────────────────────┘ │ │
│  │                                                               │ │
│  │  ═══════════════════════════════════════════════════════════ │ │
│  │                                                               │ │
│  │  CURRENT GAME: Super Mario Bros. 3                           │ │
│  │                                                               │ │
│  │  Progress: 12 / 45 achievements  |  ████████░░░░░░░░ 27%     │ │
│  │  Points: 145 / 500                                           │ │
│  │                                                               │ │
│  │  ┌─────────────────────────────────────────────────────────┐ │ │
│  │  │                                                         │ │ │
│  │  │  UNLOCKED (12)                                         │ │ │
│  │  │                                                         │ │ │
│  │  │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐       │ │ │
│  │  │  │ 🏆 5pt │  │ 🏆 10pt│  │ 🏆 10pt│  │ 🏆 25pt│       │ │ │
│  │  │  │First   │  │World 1 │  │World 2 │  │Warp    │       │ │ │
│  │  │  │Steps   │  │Complete│  │Complete│  │Whistle │       │ │ │
│  │  │  └────────┘  └────────┘  └────────┘  └────────┘       │ │ │
│  │  │                                                         │ │ │
│  │  │  LOCKED (33)                                           │ │ │
│  │  │                                                         │ │ │
│  │  │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐       │ │ │
│  │  │  │ 🔒 50pt│  │ 🔒 25pt│  │ 🔒 10pt│  │ 🔒 10pt│       │ │ │
│  │  │  │ Master │  │World 8 │  │No Hits │  │Speedrun│       │ │ │
│  │  │  │  ????  │  │Complete│  │ Boss   │  │ 30min  │       │ │ │
│  │  │  └────────┘  └────────┘  └────────┘  └────────┘       │ │ │
│  │  │                                                         │ │ │
│  │  │  ───────────────────────────────────────────────────   │ │ │
│  │  │                                                         │ │ │
│  │  │  ACTIVE CHALLENGES:                                    │ │ │
│  │  │                                                         │ │ │
│  │  │  🔥 "Speedrunner" - Complete game in under 30 minutes  │ │ │
│  │  │     Progress: 45:32 (Best: 38:12)                      │ │ │
│  │  │                                                         │ │ │
│  │  │  💎 "Coin Collector" - Collect 10,000 coins total      │ │ │
│  │  │     Progress: 7,234 / 10,000  ██████████████░░░ 72%    │ │ │
│  │  │                                                         │ │ │
│  │  └─────────────────────────────────────────────────────────┘ │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  UNLOCK NOTIFICATION STYLE:                                         │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │  ╔═══════════════════════════════════════════════════════╗   │ │
│  │  ║  🏆 ACHIEVEMENT UNLOCKED!                             ║   │ │
│  │  ║  ════════════════════════════════════════════════════ ║   │ │
│  │  ║                                                       ║   │ │
│  │  ║  [Icon]  "World 1 Complete"                          ║   │ │
│  │  ║          Finish all levels in World 1                ║   │ │
│  │  ║                                                       ║   │ │
│  │  ║          +10 points  ★★☆☆☆                           ║   │ │
│  │  ║                                                       ║   │ │
│  │  ╚═══════════════════════════════════════════════════════╝   │ │
│  │                                                               │ │
│  │  Animation: Slide in from top, gold particle burst,           │ │
│  │             retro "chime" sound effect                        │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Debugger View

```
┌─────────────────────────────────────────────────────────────────────┐
│                        DEBUGGER VIEW                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │                                                              │  │
│  │  🐛 DEBUGGER    [▶ Run] [⏸ Pause] [→ Step] [⟳ Reset]        │  │
│  │                                                              │  │
│  │  ┌────────────────┬────────────────┬────────────────┐       │  │
│  │  │                │                │                │       │  │
│  │  │  CPU STATE     │  DISASSEMBLY   │  MEMORY VIEW   │       │  │
│  │  │                │                │                │       │  │
│  │  │  PC: $8000     │  $7FF8: BRK    │  0000: 4C F5   │       │  │
│  │  │  A:  $00       │  $7FF9: ---    │  0002: 60 A9   │       │  │
│  │  │  X:  $00       │  $7FFA: ---    │  0004: 00 8D   │       │  │
│  │  │  Y:  $00       │  $7FFB: ---    │  0006: 00 20   │       │  │
│  │  │  SP: $FD       │  $7FFC: F5 C5  │  0008: A9 10   │       │  │
│  │  │  P:  NV-BDIZC  │  $7FFE: F5 C5  │  000A: 8D 01   │       │  │
│  │  │      00110100  │ ►$8000: SEI    │  000C: 20 78   │       │  │
│  │  │                │  $8001: CLD    │  000E: 40 A9   │       │  │
│  │  │  Cycles: 0     │  $8002: LDX #F │  0010: A0 8D   │       │  │
│  │  │  Scanline: 0   │  $8004: TXS    │  ...           │       │  │
│  │  │  Dot: 0        │  $8005: LDA #0 │                │       │  │
│  │  │                │  $8007: STA $0 │  [ Goto: 0000] │       │  │
│  │  │                │  ...           │                │       │  │
│  │  └────────────────┴────────────────┴────────────────┘       │  │
│  │                                                              │  │
│  │  ┌─────────────────────────────────────────────────────┐    │  │
│  │  │  BREAKPOINTS                                        │    │  │
│  │  │                                                     │    │  │
│  │  │  [+] Add   [🗑] Clear All                            │    │  │
│  │  │                                                     │    │  │
│  │  │  ● $8000  Execute   Enabled                        │    │  │
│  │  │  ○ $2002  Read      Disabled                       │    │  │
│  │  │  ● $4016  Write     Enabled (Controller)           │    │  │
│  │  │                                                     │    │  │
│  │  └─────────────────────────────────────────────────────┘    │  │
│  │                                                              │  │
│  │  ┌─────────────────────────────────────────────────────┐    │  │
│  │  │  PPU VIEWER                                         │    │  │
│  │  │                                                     │    │  │
│  │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐            │    │  │
│  │  │  │ Pattern  │ │ Nametable│ │   OAM    │            │    │  │
│  │  │  │ Table 0  │ │  Viewer  │ │  Viewer  │            │    │  │
│  │  │  │          │ │          │ │          │            │    │  │
│  │  │  │  ░░░░░░  │ │  ░░░░░░  │ │ Spr 0:   │            │    │  │
│  │  │  │  ░░░░░░  │ │  ░░░░░░  │ │ X:$40    │            │    │  │
│  │  │  │  ░░░░░░  │ │  ░░░░░░  │ │ Y:$80    │            │    │  │
│  │  │  └──────────┘ └──────────┘ └──────────┘            │    │  │
│  │  │                                                     │    │  │
│  │  │  Palette:  ██ ██ ██ ██ | ██ ██ ██ ██              │    │  │
│  │  │            ██ ██ ██ ██ | ██ ██ ██ ██              │    │  │
│  │  │                                                     │    │  │
│  │  └─────────────────────────────────────────────────────┘    │  │
│  │                                                              │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  FEATURES:                                                          │
│  • Conditional breakpoints (A == $50, PC >= $C000)                 │
│  • Memory watch expressions                                         │
│  • Trace logging to file                                           │
│  • Frame advance with rewind                                        │
│  • Lua console for scripting                                        │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Animation & Motion Design

### Timing Functions

```
┌─────────────────────────────────────────────────────────────────────┐
│                     ANIMATION TIMING CURVES                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  STANDARD CURVES (Bézier):                                          │
│                                                                     │
│  ease-out-expo:    cubic-bezier(0.16, 1, 0.3, 1)                   │
│  ease-in-out-quint: cubic-bezier(0.83, 0, 0.17, 1)                 │
│  ease-out-back:    cubic-bezier(0.34, 1.56, 0.64, 1)               │
│  spring:           spring(1, 100, 10, 0)                           │
│                                                                     │
│  DURATIONS:                                                         │
│                                                                     │
│  instant:    0ms    — State changes, toggles                       │
│  micro:      100ms  — Hover states, focus rings                    │
│  fast:       200ms  — Tooltips, dropdown menus                     │
│  normal:     300ms  — Panel transitions, modals                    │
│  slow:       500ms  — Page transitions, complex animations         │
│  cinematic:  800ms+ — Achievement unlocks, special effects         │
│                                                                     │
│  ANIMATION EXAMPLES:                                                │
│                                                                     │
│  Button Hover:                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Idle ───(100ms ease-out)───► Hover ───(100ms ease-in)───► Idle │
│  │                                                             │   │
│  │  Scale:     1.0  →  1.02  →  1.0                            │   │
│  │  Shadow:    sm   →  md    →  sm                             │   │
│  │  Glow:      0%   →  30%   →  0%                             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  Modal Open:                                                        │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Trigger ──(300ms ease-out-expo)──► Open                    │   │
│  │                                                             │   │
│  │  Backdrop: opacity 0 → 0.6 (200ms)                          │   │
│  │  Modal:    scale 0.95, y+20px → scale 1.0, y+0px (300ms)   │   │
│  │  Content:  opacity 0 → 1 (200ms, 100ms delay)              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  Achievement Unlock:                                                │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  0ms:     Hidden (y: -100%, opacity: 0)                     │   │
│  │  0-300ms: Slide in (ease-out-back), scale bounce            │   │
│  │  300ms:   Gold particle burst (50 particles, 600ms)         │   │
│  │  400ms:   Icon pulse (2x scale, spring back)                │   │
│  │  5000ms:  Auto-dismiss (fade out, 300ms)                    │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  CRT Power-On Effect:                                               │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  0ms:     White flash (full screen)                         │   │
│  │  0-50ms:  Horizontal line expanding vertically              │   │
│  │  50-150ms: Static noise fade                                │   │
│  │  150-400ms: Image fade in with slight distortion            │   │
│  │  400ms+:  Stable image                                      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Micro-Interactions

```rust
/// Animation system for smooth, 120Hz+ capable transitions
pub struct AnimationSystem {
    /// Active animations
    animations: Vec<Animation>,

    /// Animation clock (high-resolution)
    clock: Instant,
}

#[derive(Clone)]
pub struct Animation {
    /// Unique identifier
    id: AnimationId,

    /// Start time
    start: Instant,

    /// Duration
    duration: Duration,

    /// Easing function
    easing: Easing,

    /// Property being animated
    property: AnimatedProperty,

    /// From value
    from: f32,

    /// To value
    to: f32,

    /// Current value (cached)
    current: f32,

    /// Animation state
    state: AnimationState,
}

#[derive(Clone)]
pub enum AnimatedProperty {
    Opacity,
    Scale,
    TranslateX,
    TranslateY,
    Rotation,
    BorderRadius,
    BackgroundColor { from: Color, to: Color },
    Custom(String),
}

#[derive(Clone, Copy)]
pub enum Easing {
    Linear,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseOutExpo,
    EaseOutBack,
    Spring { stiffness: f32, damping: f32 },
}

impl Easing {
    pub fn apply(&self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::EaseOutExpo => {
                if t == 1.0 { 1.0 } else { 1.0 - 2.0_f32.powf(-10.0 * t) }
            }
            Easing::EaseOutBack => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            Easing::Spring { stiffness, damping } => {
                // Damped harmonic oscillator
                let omega = stiffness.sqrt();
                let zeta = *damping / (2.0 * omega);

                if zeta < 1.0 {
                    // Underdamped
                    let omega_d = omega * (1.0 - zeta * zeta).sqrt();
                    1.0 - (-zeta * omega * t).exp() *
                        ((zeta * omega * t).cos() +
                         (zeta * omega / omega_d) * (omega_d * t).sin())
                } else {
                    // Critically/overdamped
                    1.0 - (1.0 + omega * t) * (-omega * t).exp()
                }
            }
            _ => t, // Fallback
        }
    }
}

/// Particle system for visual effects
pub struct ParticleSystem {
    particles: Vec<Particle>,
    emitters: Vec<ParticleEmitter>,
}

pub struct Particle {
    position: (f32, f32),
    velocity: (f32, f32),
    acceleration: (f32, f32),
    color: Color,
    size: f32,
    lifetime: f32,
    age: f32,
    sprite: Option<ParticleSprite>,
}

pub struct ParticleEmitter {
    position: (f32, f32),
    emission_rate: f32,
    particle_lifetime: Range<f32>,
    initial_velocity: Range<(f32, f32)>,
    particle_color: ColorGradient,
    particle_size: Range<f32>,
    spread_angle: f32,
}

impl ParticleSystem {
    /// Emit achievement unlock particles
    pub fn emit_achievement_burst(&mut self, position: (f32, f32)) {
        let emitter = ParticleEmitter {
            position,
            emission_rate: 100.0, // Burst mode
            particle_lifetime: 0.5..1.5,
            initial_velocity: (-200.0..200.0, -300.0..-100.0),
            particle_color: ColorGradient::new(vec![
                (0.0, Color::from_rgb(1.0, 0.84, 0.0)),  // Gold
                (0.5, Color::from_rgb(1.0, 0.65, 0.0)),  // Orange
                (1.0, Color::from_rgba(1.0, 0.5, 0.0, 0.0)), // Fade out
            ]),
            particle_size: 4.0..12.0,
            spread_angle: 360.0,
        };

        // Emit 50 particles immediately
        for _ in 0..50 {
            self.emit_particle(&emitter);
        }
    }
}
```

---

## Feature-Specific UI

### Rewind Timeline

```
┌─────────────────────────────────────────────────────────────────────┐
│                       REWIND TIMELINE                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Trigger: Hold L2/LT, or dedicated rewind key                      │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                                                               │ │
│  │  ◀◀ REWINDING... ▶▶                                          │ │
│  │                                                               │ │
│  │  ┌─────────────────────────────────────────────────────────┐ │ │
│  │  │                                                         │ │ │
│  │  │   -10s     -5s      NOW                                │ │ │
│  │  │    │        │        ▼                                  │ │ │
│  │  │  ──●────────●────────●─────────────────────────────    │ │ │
│  │  │    │        │        │                                  │ │ │
│  │  │  ┌───┐    ┌───┐    ┌───┐                               │ │ │
│  │  │  │ ░ │    │ ░ │    │ ░ │  ← Thumbnail previews         │ │ │
│  │  │  └───┘    └───┘    └───┘                               │ │ │
│  │  │                                                         │ │ │
│  │  │  Available: 30 seconds of history                      │ │ │
│  │  │  Memory: 45 MB / 100 MB                                │ │ │
│  │  │                                                         │ │ │
│  │  └─────────────────────────────────────────────────────────┘ │ │
│  │                                                               │ │
│  │  Controls:                                                    │ │
│  │  • Left/Right: Scrub through time                            │ │
│  │  • Release: Resume from selected point                        │ │
│  │  • A/Enter: Create bookmark                                   │ │
│  │  • B/Escape: Cancel and return to present                    │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  VISUAL EFFECTS:                                                    │
│  • Game desaturates during rewind                                   │
│  • VHS-style tracking lines overlay                                 │
│  • Slight blur on fast scrubbing                                    │
│  • Time indicator pulses at present moment                          │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### TAS Editor

```
┌─────────────────────────────────────────────────────────────────────┐
│                         TAS EDITOR                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                                                               │ │
│  │  📝 TAS EDITOR - movie.fm2                                   │ │
│  │                                                               │ │
│  │  ┌──────────┬─────────────────────────────────────┬────────┐ │ │
│  │  │          │                                     │        │ │ │
│  │  │ CONTROLS │         PIANO ROLL                  │ GAME   │ │ │
│  │  │          │                                     │ VIEW   │ │ │
│  │  │ [▶ Play] │  Frame │ ← ↑ ↓ → │ Sel│Sta│ B │ A ││        │ │ │
│  │  │ [⏸ Pause]│  ──────┼─────────┼────┼───┼───┼───┤│ ░░░░░░ │ │ │
│  │  │ [→ Step] │  00001 │ . . . . │ . │ . │ . │ . ││ ░░░░░░ │ │ │
│  │  │ [← Back] │  00002 │ . . . . │ . │ . │ . │ . ││ ░░░░░░ │ │ │
│  │  │          │  00003 │ . . . ■ │ . │ . │ . │ . ││ ░░░░░░ │ │ │
│  │  │ ──────── │  00004 │ . . . ■ │ . │ . │ . │ . ││        │ │ │
│  │  │          │  00005 │ . . . ■ │ . │ . │ . │ . ││        │ │ │
│  │  │ Frame:   │  00006 │ . . . ■ │ . │ . │ ■ │ . ││        │ │ │
│  │  │  00006   │► 00007 │ . . . ■ │ . │ . │ . │ ■ │← Current│ │ │
│  │  │          │  00008 │ . . . . │ . │ . │ . │ . ││        │ │ │
│  │  │ Lag: 0   │  00009 │ . . . . │ . │ . │ . │ . ││        │ │ │
│  │  │          │  00010 │ . . . . │ . │ . │ . │ . ││        │ │ │
│  │  │ ──────── │                                     │        │ │ │
│  │  │          │  ■ = Pressed   . = Released        │        │ │ │
│  │  │ Recording│                                     │        │ │ │
│  │  │  [OFF]   │  [Insert Pattern] [Delete Selection]│        │ │ │
│  │  │          │                                     │        │ │ │
│  │  └──────────┴─────────────────────────────────────┴────────┘ │ │
│  │                                                               │ │
│  │  ┌─────────────────────────────────────────────────────────┐ │ │
│  │  │  GREENZONE                                              │ │ │
│  │  │                                                         │ │ │
│  │  │  00000 ████████████████████████████████░░░░░░░░ 00256   │ │ │
│  │  │        ▲                              ▲                 │ │ │
│  │  │     Current                        Verified             │ │ │
│  │  │                                                         │ │ │
│  │  │  Verified states: 187 | Unverified: 69                 │ │ │
│  │  └─────────────────────────────────────────────────────────┘ │ │
│  │                                                               │ │
│  │  ┌─────────────────────────────────────────────────────────┐ │ │
│  │  │  BRANCHES                                               │ │ │
│  │  │                                                         │ │ │
│  │  │  ● Main ──────────────────────────────────────►        │ │ │
│  │  │       ╲                                                 │ │ │
│  │  │        ● Alt Jump Route ──────►                        │ │ │
│  │  │                                                         │ │ │
│  │  │  [+ New Branch] [Merge] [Delete]                       │ │ │
│  │  └─────────────────────────────────────────────────────────┘ │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Accessibility & Inclusivity

### Accessibility Features

```
┌─────────────────────────────────────────────────────────────────────┐
│                    ACCESSIBILITY FEATURES                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  VISUAL ACCESSIBILITY                                               │
│  ────────────────────                                               │
│                                                                     │
│  • High Contrast Mode                                               │
│    - Increases text/background contrast to WCAG AAA (7:1+)          │
│    - Removes decorative elements that reduce clarity                │
│    - Thicker focus indicators                                       │
│                                                                     │
│  • Reduced Motion Mode                                              │
│    - Disables all animations                                        │
│    - Instant transitions                                            │
│    - No particle effects                                            │
│                                                                     │
│  • Font Scaling (100% - 200%)                                       │
│    - Respects system font size preferences                          │
│    - UI scales proportionally                                       │
│                                                                     │
│  • Colorblind Modes                                                 │
│    - Deuteranopia (Red-Green)                                       │
│    - Protanopia (Red)                                               │
│    - Tritanopia (Blue-Yellow)                                       │
│    - Grayscale                                                      │
│                                                                     │
│  MOTOR ACCESSIBILITY                                                │
│  ──────────────────                                                 │
│                                                                     │
│  • Full Keyboard Navigation                                         │
│    - Tab order follows visual layout                                │
│    - Arrow keys for list/grid navigation                           │
│    - Shortcuts for all major actions                                │
│                                                                     │
│  • Input Remapping                                                  │
│    - Any key/button to any action                                  │
│    - Multiple keys per action                                       │
│    - Turbo/autofire support                                        │
│                                                                     │
│  • Adjustable Timing                                                │
│    - Key repeat delay: 100ms - 1000ms                              │
│    - Double-click speed adjustment                                  │
│    - Hold-to-confirm duration                                       │
│                                                                     │
│  COGNITIVE ACCESSIBILITY                                            │
│  ───────────────────────                                            │
│                                                                     │
│  • Simplified UI Mode                                               │
│    - Hides advanced features                                        │
│    - Larger touch targets                                           │
│    - Clearer labeling                                               │
│                                                                     │
│  • Tooltips & Hints                                                 │
│    - Detailed tooltips on hover (optional)                          │
│    - First-use walkthroughs                                        │
│    - Contextual help (? icons)                                     │
│                                                                     │
│  SCREEN READER SUPPORT                                              │
│  ─────────────────────                                              │
│                                                                     │
│  • ARIA Labels on all interactive elements                         │
│  • Live regions for dynamic content (toasts, achievements)         │
│  • Semantic structure for navigation                               │
│  • Game state announcements (optional TTS)                         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Keyboard Shortcuts

```
┌─────────────────────────────────────────────────────────────────────┐
│                     KEYBOARD SHORTCUTS                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  GLOBAL                           PLAYBACK                          │
│  ───────                          ────────                          │
│  Ctrl+O     Open ROM              Space      Play/Pause             │
│  Ctrl+S     Quick Save            Escape     Quick Menu             │
│  Ctrl+L     Quick Load            F1-F10     Save State 1-10        │
│  Ctrl+Q     Quit                  Shift+F1-10 Load State 1-10       │
│  F11        Fullscreen            R          Reset                  │
│  F12        Screenshot            Shift+R    Power Cycle            │
│                                   \          Fast Forward (hold)    │
│  NAVIGATION                       Tab        Frame Advance          │
│  ──────────                       `          Rewind (hold)          │
│  Tab        Next Element                                            │
│  Shift+Tab  Previous Element      AUDIO/VIDEO                       │
│  Enter      Activate              ───────────                       │
│  Escape     Back/Cancel           M          Mute Toggle            │
│  Arrow Keys Navigate Lists        +/-        Volume Up/Down         │
│                                   1-5        Integer Scale          │
│  LIBRARY                          P          Pause Emulation        │
│  ───────                                                            │
│  Ctrl+F     Search                DEBUG                             │
│  Delete     Remove Selected       ─────                             │
│  Enter      Launch Selected       D          Open Debugger          │
│                                   F9         Toggle Breakpoint      │
│  NETPLAY                          F10        Step Over              │
│  ───────                          F11        Step Into              │
│  Ctrl+N     New Session           Shift+F11  Step Out               │
│  Ctrl+J     Join Session                                            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Performance Requirements

### Target Metrics

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PERFORMANCE TARGETS                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  FRAME TIMING                                                       │
│  ────────────                                                       │
│                                                                     │
│  UI Render:           < 2ms    (120Hz capable)                     │
│  Emulation Frame:     < 8ms    (16.67ms budget, 50% headroom)      │
│  Total Frame:         < 12ms   (Leaves 4ms for OS/driver)          │
│  Input Latency:       < 16ms   (1 frame at 60Hz)                   │
│                                                                     │
│  AUDIO                                                              │
│  ─────                                                              │
│                                                                     │
│  Latency:             < 20ms   (Imperceptible)                     │
│  Buffer Underruns:    0        (No crackling)                      │
│  Sample Rate:         48kHz    (Native hardware rate)              │
│                                                                     │
│  MEMORY                                                             │
│  ──────                                                             │
│                                                                     │
│  Base Application:    < 50 MB                                      │
│  Per-Game Overhead:   < 10 MB                                      │
│  Rewind Buffer:       < 100 MB (30 seconds)                        │
│  Shader Cache:        < 20 MB                                      │
│  Total (Typical):     < 150 MB                                     │
│                                                                     │
│  STARTUP                                                            │
│  ───────                                                            │
│                                                                     │
│  Cold Start:          < 500ms  (From click to window)              │
│  ROM Load:            < 100ms  (To first frame)                    │
│  Save State Load:     < 50ms   (Compressed, ~1MB)                  │
│                                                                     │
│  GPU                                                                │
│  ───                                                                │
│                                                                     │
│  VRAM:                < 64 MB                                      │
│  GPU Usage:           < 10%    (Modern integrated GPU)             │
│  Shader Compile:      < 100ms  (First launch, cached after)        │
│                                                                     │
│  MINIMUM SYSTEM REQUIREMENTS                                        │
│  ───────────────────────────                                        │
│                                                                     │
│  CPU:    Dual-core 2.0 GHz (SSE4.2)                                │
│  RAM:    2 GB                                                       │
│  GPU:    OpenGL 3.3 / DirectX 11 / Metal 2.0                       │
│  Disk:   100 MB (Application)                                       │
│  OS:     Windows 10+, macOS 11+, Linux (glibc 2.31+)               │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Performance Optimization Strategies

```rust
/// Performance-critical rendering pipeline
pub struct RenderPipeline {
    /// Double-buffered frame data
    frame_buffers: [FrameBuffer; 2],

    /// Current buffer index
    current_buffer: usize,

    /// GPU resources
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    render_pipeline: wgpu::RenderPipeline,

    /// Shader variants (precompiled)
    shaders: HashMap<ShaderPreset, wgpu::ShaderModule>,

    /// Frame timing
    frame_times: RingBuffer<Duration, 60>,

    /// Vsync state
    vsync_enabled: bool,
}

impl RenderPipeline {
    /// Upload frame to GPU (< 1ms)
    pub fn upload_frame(&mut self, frame: &[u8; 256 * 240]) {
        // Use staging buffer for async upload
        let staging = self.staging_buffer.as_mut().unwrap();
        staging.copy_from_slice(frame);

        // Queue upload (non-blocking)
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(240),
            },
            wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Render with shader (< 1ms)
    pub fn render(&mut self, surface: &wgpu::Surface) -> Result<(), wgpu::SurfaceError> {
        let output = surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&Default::default());

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("NES Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.draw(0..6, 0..1); // Fullscreen quad
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

/// Audio pipeline with lock-free ring buffer
pub struct AudioPipeline {
    /// Lock-free SPSC ring buffer
    producer: ringbuf::Producer<f32, Arc<ringbuf::HeapRb<f32>>>,
    consumer: ringbuf::Consumer<f32, Arc<ringbuf::HeapRb<f32>>>,

    /// Audio stream handle
    stream: cpal::Stream,

    /// Resampler (APU rate → output rate)
    resampler: rubato::SincFixedIn<f32>,

    /// Sample accumulator
    sample_buffer: Vec<f32>,
}

impl AudioPipeline {
    /// Queue samples from APU (called every frame)
    pub fn queue_samples(&mut self, samples: &[f32]) {
        // Resample from ~1.789MHz effective to 48kHz
        let resampled = self.resampler.process(&[samples], None).unwrap();

        // Push to ring buffer (non-blocking)
        for &sample in &resampled[0] {
            let _ = self.producer.push(sample);
        }
    }
}
```

---

## Implementation Roadmap

### Phase 1: Foundation (Week 1-2)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PHASE 1: FOUNDATION                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  WEEK 1: Application Shell                                          │
│  ─────────────────────────                                          │
│                                                                     │
│  [ ] Create rustynes-desktop crate structure                       │
│  [ ] Set up Iced application skeleton                               │
│  [ ] Implement custom title bar (Windows/Linux/macOS)              │
│  [ ] Create basic theme system                                      │
│  [ ] Implement sidebar navigation                                   │
│  [ ] Set up wgpu render pipeline                                    │
│  [ ] Integrate game viewport with NES framebuffer                  │
│                                                                     │
│  WEEK 2: Core Playback                                              │
│  ──────────────────────                                             │
│                                                                     │
│  [ ] Implement ROM loading via file dialog                         │
│  [ ] Create cpal audio pipeline                                     │
│  [ ] Set up input handling (keyboard)                              │
│  [ ] Implement basic play/pause/reset                              │
│  [ ] Add FPS counter and frame timing                              │
│  [ ] Create quick menu overlay                                      │
│  [ ] Implement save states (UI only)                               │
│                                                                     │
│  DELIVERABLE: Playable emulator with basic UI                      │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Phase 2: Polish (Week 3-4)

```
┌─────────────────────────────────────────────────────────────────────┐
│                      PHASE 2: POLISH                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  WEEK 3: Library & Settings                                         │
│  ─────────────────────────                                          │
│                                                                     │
│  [ ] Implement ROM library browser                                  │
│  [ ] Add box art loading (local + scraping)                        │
│  [ ] Create settings panel UI                                       │
│  [ ] Implement video settings (scale, shaders)                     │
│  [ ] Implement audio settings (volume, latency)                    │
│  [ ] Implement input settings (keyboard mapping)                   │
│  [ ] Add gamepad support (gilrs)                                    │
│  [ ] Persist configuration to TOML                                  │
│                                                                     │
│  WEEK 4: Visual Polish                                              │
│  ───────────────────                                                │
│                                                                     │
│  [ ] Implement CRT shader pipeline                                  │
│  [ ] Add animation system                                           │
│  [ ] Create toast notification system                               │
│  [ ] Implement modal dialogs                                        │
│  [ ] Add loading states and progress indicators                    │
│  [ ] Polish transitions and micro-interactions                     │
│  [ ] Implement theme switching (light/dark/retro)                  │
│                                                                     │
│  DELIVERABLE: Polished, feature-complete MVP                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Phase 3: Advanced Features (Week 5-8)

```
┌─────────────────────────────────────────────────────────────────────┐
│                   PHASE 3: ADVANCED FEATURES                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  WEEK 5-6: Netplay & Achievements                                   │
│  ────────────────────────────────                                   │
│                                                                     │
│  [ ] Implement netplay lobby UI                                     │
│  [ ] Add session creation/joining flow                             │
│  [ ] Create in-game netplay overlay                                │
│  [ ] Integrate RetroAchievements login                             │
│  [ ] Implement achievement browser                                  │
│  [ ] Add achievement unlock notifications                          │
│  [ ] Create leaderboard UI                                          │
│                                                                     │
│  WEEK 7-8: Debug & TAS Tools                                        │
│  ───────────────────────────                                        │
│                                                                     │
│  [ ] Implement debugger view (egui overlay)                        │
│  [ ] Add CPU/PPU/APU state viewers                                 │
│  [ ] Create memory hex editor                                       │
│  [ ] Implement breakpoint system UI                                │
│  [ ] Add rewind timeline                                           │
│  [ ] Create TAS editor (piano roll)                                │
│  [ ] Implement movie recording/playback                            │
│  [ ] Add Lua scripting console                                      │
│                                                                     │
│  DELIVERABLE: Full-featured emulator matching/exceeding Mesen2     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Appendix: Shader Examples

### CRT Shader (WGSL)

```wgsl
// CRT shader for authentic retro look
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    screen_size: vec2<f32>,
    time: f32,
    scanline_intensity: f32,
    curvature: f32,
    vignette: f32,
    bloom: f32,
};

@group(0) @binding(0) var t_nes: texture_2d<f32>;
@group(0) @binding(1) var s_nes: sampler;
@group(0) @binding(2) var<uniform> uniforms: Uniforms;

// Barrel distortion for CRT curvature
fn barrel_distortion(uv: vec2<f32>, curvature: f32) -> vec2<f32> {
    let centered = uv * 2.0 - 1.0;
    let dist = dot(centered, centered);
    let distorted = centered * (1.0 + dist * curvature);
    return distorted * 0.5 + 0.5;
}

// Scanline effect
fn scanlines(uv: vec2<f32>, intensity: f32) -> f32 {
    let line = sin(uv.y * uniforms.screen_size.y * 3.14159);
    return 1.0 - intensity * (1.0 - line * line);
}

// Vignette darkening at edges
fn vignette(uv: vec2<f32>, strength: f32) -> f32 {
    let centered = uv * 2.0 - 1.0;
    let dist = dot(centered, centered);
    return 1.0 - dist * strength;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Apply barrel distortion
    let uv = barrel_distortion(in.uv, uniforms.curvature);

    // Check if we're outside the screen
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Sample NES framebuffer
    var color = textureSample(t_nes, s_nes, uv).rgb;

    // Apply bloom (simple box blur on bright areas)
    let bloom_sample = textureSample(t_nes, s_nes, uv + vec2<f32>(0.002, 0.0)).rgb +
                       textureSample(t_nes, s_nes, uv - vec2<f32>(0.002, 0.0)).rgb +
                       textureSample(t_nes, s_nes, uv + vec2<f32>(0.0, 0.002)).rgb +
                       textureSample(t_nes, s_nes, uv - vec2<f32>(0.0, 0.002)).rgb;
    color = color + (bloom_sample * 0.25 - color) * uniforms.bloom * 0.3;

    // Apply scanlines
    color = color * scanlines(uv, uniforms.scanline_intensity);

    // Apply vignette
    color = color * vignette(uv, uniforms.vignette);

    // Phosphor glow (subtle color bleeding)
    color.r = color.r * 0.95 + color.g * 0.05;
    color.b = color.b * 0.95 + color.g * 0.05;

    return vec4<f32>(color, 1.0);
}
```

---

## Conclusion

This UI/UX design specification provides a comprehensive blueprint for creating a **world-class NES emulator interface** that surpasses existing solutions. By combining:

- **Nostalgic aesthetics** (CRT effects, pixel art, retro colors)
- **Modern UX patterns** (smooth animations, intuitive navigation)
- **Advanced features** (netplay, achievements, TAS tools, debugger)
- **Accessibility** (full keyboard navigation, screen reader support)
- **Performance** (120Hz+ UI, <16ms latency)

RustyNES will deliver an experience that's not just functional, but genuinely **fun to use** — honoring the NES's legacy while embracing the future of emulation.

---

**Document Version:** 1.0.0  
**Author:** RustyNES Team  
**Status:** Design Complete, Ready for Implementation
