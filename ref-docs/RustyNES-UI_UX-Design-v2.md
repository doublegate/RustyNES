# RustyNES UI/UX Design Specification

**Document Version:** 2.0.0  
**Last Updated:** December 19, 2025  
**Status:** Enhanced Design Specification (design history)  
**Target:** World-Class Production-Ready Interface

---

> **Status (v1.0.0 — what shipped).** This is the expanded, aspirational UI/UX
> spec (run-ahead latency reduction, a 12+-preset CRT pipeline, HTPC / 10-foot
> mode, HD packs, metadata scraping, etc.), retained as design history. Note
> that "Version 2.0.0" here is the DOCUMENT revision, not a RustyNES release —
> RustyNES shipped as **v1.0.0**. The realized frontend runs on **winit 0.30 +
> wgpu + egui 0.29 + cpal** (NOT eframe/glow, NOT SDL2). Realized from this
> spec's wish list: **run-ahead** (default 1, 0-3) for input-latency reduction,
> a display-sync pacing matrix with audio dynamic-rate-control (VRR/G-Sync/
> FreeSync aware), a simplified NTSC WGSL post-pass, the always-on menu bar +
> status bar shell, the tabbed Settings window, themes (light/dark/system), the
> 8:7 pixel-aspect toggle, the Welcome and About modals, and the egui debugger.
> Deliberately NOT shipped at v1.0.0 (design intent only): the multi-preset
> CRT-Royale/Lottes/Guest shader bank, phosphor-mask/BFI/rolling-scan effects,
> the HTPC 10-foot / Cover Flow library UI, voice navigation, online metadata
> scraping, HD packs, and Lua scripting (post-1.0). Read the body as the design
> ceiling RustyNES aimed at, not a feature manifest of v1.0.0.

---

## Changelog from v1.0.0

```
┌────────────────────────────────────────────────────────────────────┐
│                    VERSION 2.0.0 ENHANCEMENTS                      │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  LATENCY REDUCTION (NEW)                                           │
│  • Run-Ahead frame prediction (1-4 frames configurable)            │
│  • Preemptive Frames alternative mode                              │
│  • Frame Delay auto-tuning (0-15 frames)                           │
│  • Just-In-Time input polling (<1ms optimization)                  │
│  • Adaptive sync support (VRR/FreeSync/G-Sync)                     │
│  • Black Frame Insertion (BFI) for high-Hz displays                │
│                                                                    │
│  ENHANCED CRT PIPELINE (EXPANDED)                                  │
│  • 12+ shader presets (CRT-Royale, Lottes, Guest, etc.)            │
│  • Phosphor mask types (Aperture Grille, Slot Mask, Shadow Mask)   │
│  • Rolling scan CRT simulation (Blur Busters technique)            │
│  • NTSC composite signal simulation                                │
│  • Subpixel rendering for mask accuracy                            │
│  • HDR bloom with phosphor persistence                             │
│                                                                    │
│  HTPC / CONTROLLER-FIRST MODE (NEW)                                │
│  • Full 10-foot UI for living room setups                          │
│  • Cover Flow and Virtual Shelf views                              │
│  • Voice navigation support                                        │
│  • Automatic metadata scraping (IGDB, ScreenScraper)               │
│                                                                    │
│  ADVANCED FEATURES (NEW)                                           │
│  • HD Pack support (Mesen-compatible)                              │
│  • Per-game configuration profiles                                 │
│  • Automatic run-ahead detection per game                          │
│  • Plugin/extension architecture                                   │
│  • Cloud save synchronization (optional)                           │
│  • Discord Rich Presence integration                               │
│  • CLI mode for automation/scripting                               │
│                                                                    │
│  ARCHITECTURE IMPROVEMENTS                                         │
│  • Slint UI option for embedded/resource-constrained targets       │
│  • Hardware Abstraction Layer (HAL) for portability                │
│  • Enhanced rewind with memory-mapped compression                  │
│  • Dual-instance run-ahead for audio stability                     │
│                                                                    │
│  UI/UX POLISH                                                      │
│  • Glass morphism effects with backdrop blur                       │
│  • Haptic feedback patterns for gamepads                           │
│  • Animated sprite-based loading indicators                        │
│  • Parallax scrolling backgrounds                                  │
│  • Sound design system with retro SFX                              │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

---

## Table of Contents

1. [Design Philosophy](#design-philosophy)
2. [Visual Design Language](#visual-design-language)
3. [Technology Stack](#technology-stack)
4. [Application Architecture](#application-architecture)
5. [Core Interface Components](#core-interface-components)
6. [Main Views & Layouts](#main-views--layouts)
7. [HTPC Controller-First Mode](#htpc-controller-first-mode)
8. [Latency Reduction System](#latency-reduction-system)
9. [Enhanced CRT Shader Pipeline](#enhanced-crt-shader-pipeline)
10. [Animation & Motion Design](#animation--motion-design)
11. [Sound Design System](#sound-design-system)
12. [Feature-Specific UI](#feature-specific-ui)
13. [Plugin & Extension System](#plugin--extension-system)
14. [Accessibility & Inclusivity](#accessibility--inclusivity)
15. [Performance Requirements](#performance-requirements)
16. [Implementation Roadmap](#implementation-roadmap)

---

## Design Philosophy

### Core Principles

RustyNES's interface embodies **"Nostalgic Futurism"** — honoring the NES's iconic 8-bit heritage while delivering a modern, buttery-smooth experience that surpasses every existing emulator including Mesen, RetroArch, and FCEUX.

#### 1. **Playful Authenticity**

The UI should feel like stepping into a 1980s living room with a time-traveling upgrade. Every interaction evokes the tactile joy of inserting a cartridge, pressing power, and hearing that familiar hum — but with zero friction.

#### 2. **Invisible Complexity**

Advanced features (debugging, TAS tools, netplay, run-ahead) remain hidden until needed. The interface scales from "plug and play" simplicity to professional-grade tooling without overwhelming the user.

#### 3. **Sub-Frame Responsiveness**

Every interaction responds in under 8ms (half a frame). Animations run at 120Hz+ where hardware permits. The interface never blocks, stutters, or drops frames. **Run-ahead enables response times faster than original hardware.**

#### 4. **Contextual Intelligence**

The UI anticipates user needs: recently played games surface automatically, save states organize themselves, settings adapt to detected hardware, and **optimal run-ahead frames are auto-detected per game**.

#### 5. **Delight in Details**

Micro-interactions, Easter eggs, and attention to pixel-perfect alignment create an experience worth exploring. Every tooltip, every transition, every sound effect is intentional.

#### 6. **Living Room Ready**

Full HTPC support with controller-first navigation, 10-foot UI scaling, and automatic CRT shader selection based on display characteristics.

---

## Visual Design Language

### Color Palette

```
┌────────────────────────────────────────────────────────────────────┐
│                        RUSTYNES COLOR SYSTEM                       │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  PRIMARY PALETTE (NES-Inspired)                                    │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐       │
│  │ #1A1A2E │ │ #16213E │ │ #0F3460 │ │ #E94560 │ │ #FF6B6B │       │
│  │ Console │ │  Deep   │ │  NES    │ │ Power   │ │  Coral  │       │
│  │  Black  │ │  Navy   │ │  Blue   │ │   Red   │ │  Accent │       │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘       │
│                                                                    │
│  SECONDARY PALETTE (CRT Glow)                                      │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐       │
│  │ #00FF88 │ │ #00D4FF │ │ #FFD93D │ │ #C084FC │ │ #F8F8F2 │       │
│  │Phosphor │ │  Cyan   │ │  Gold   │ │  Purple │ │  White  │       │
│  │  Green  │ │  Glow   │ │ Accent  │ │   Glow  │ │  Text   │       │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘       │
│                                                                    │
│  SEMANTIC COLORS                                                   │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐                   │
│  │ #22C55E │ │ #EAB308 │ │ #EF4444 │ │ #3B82F6 │                   │
│  │ Success │ │ Warning │ │  Error  │ │  Info   │                   │
│  └─────────┘ └─────────┘ └─────────┘ └─────────┘                   │
│                                                                    │
│  GLASS MORPHISM (NEW)                                              │
│  ┌─────────────────────────────────────────────────────────┐       │
│  │  Background: rgba(26, 26, 46, 0.7)                      │       │
│  │  Backdrop-filter: blur(20px) saturate(180%)             │       │
│  │  Border: 1px solid rgba(255, 255, 255, 0.1)             │       │
│  │  Shadow: 0 8px 32px rgba(0, 0, 0, 0.3)                  │       │
│  └─────────────────────────────────────────────────────────┘       │
│                                                                    │
│  PHOSPHOR COLORS (CRT Accurate)                                    │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐                               │
│  │ #FF2020 │ │ #20FF20 │ │ #2020FF │                               │
│  │   P22   │ │   P22   │ │   P22   │                               │
│  │   Red   │ │  Green  │ │  Blue   │                               │
│  └─────────┘ └─────────┘ └─────────┘                               │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Typography

**Primary Font Stack:**
```css
/* UI Text - Monospace for technical precision */
font-family: "JetBrains Mono", "Cascadia Code", "Fira Code", monospace;

/* Headers & Branding - Authentic 8-bit feel */
font-family: "Press Start 2P", "VT323", "Perfect DOS VGA 437", monospace;

/* Body Text - High readability */
font-family: "Inter", "SF Pro Text", system-ui, sans-serif;

/* Fallback for Accessibility */
font-family: system-ui, -apple-system, sans-serif;
```

**Font Scale (8px base grid):**
```
XS:   10px / 0.625rem  — Tooltips, timestamps, frame counters
SM:   12px / 0.75rem   — Secondary text, labels, status indicators
BASE: 14px / 0.875rem  — Body text, menus, settings
MD:   16px / 1rem      — Emphasized text, button labels
LG:   20px / 1.25rem   — Section headers, game titles
XL:   24px / 1.5rem    — View titles, modal headers
2XL:  32px / 2rem      — Hero text, welcome screen
3XL:  48px / 3rem      — HTPC mode titles (10-foot UI)
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
│  • SVG with embedded pixel-perfect rendering hints           │
│                                                              │
│  CORE ICON SET:                                              │
│                                                              │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐     │
│  │ ▶  │ │ ⏸ │ │ ⏹ │  │ ⏪ │ │ ⏩ │  │ 💾 │ │ 📁 │ │ ⚙  │     │
│  │Play│ │Paus│ │Stop│ │Rwnd│ │FFwd│ │Save│ │Load│ │Conf│     │
│  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘     │
│                                                              │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐     │
│  │ 🎮 │ │ 🔊  │ │ 🔇 │ │ 🖥 │ │ 🐛  │ │ 📡 │ │ 🏆 │ │ 📝  │     │
│  │Ctrl│ │ Vol│ │Mute│ │Full│ │Debg│ │ Net│ │Achv│ │ TAS│     │
│  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘     │
│                                                              │
│  NEW LATENCY & SHADER ICONS:                                 │
│                                                              │
│  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐     │
│  │ ⚡ │ │ 📺  │ │ 🔬 │ │ 📊 │ │ 🔄 │  │ 🎯 │ │ 🌐  │ │ 🔌 │     │
│  │ Lag│ │ CRT│ │Scan│ │Stat│ │Sync│ │Targ│ │ Web│ │Plug│     │
│  └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘ └────┘     │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

### Depth & Elevation

**Layering System (Glass Morphism + CRT Depth):**

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
│  Level 6: HUD               — In-game overlays (run-ahead, FPS) │
│                                                                 │
│  SHADOW STYLE (Glass + CRT Glow):                               │
│                                                                 │
│  Level 2: box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3),             │
│                       0 0 20px rgba(15, 52, 96, 0.1);           │
│           backdrop-filter: blur(8px);                           │
│                                                                 │
│  Level 4: box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5),            │
│                       0 0 60px rgba(233, 69, 96, 0.15);         │
│           backdrop-filter: blur(20px) saturate(180%);           │
│                                                                 │
│  CRT Glow (for retro elements):                                 │
│           box-shadow: 0 0 10px currentColor,                    │
│                       0 0 20px currentColor,                    │
│                       0 0 40px currentColor;                    │
│           filter: brightness(1.1);                              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Border & Corner Radii

```
Sharp:      0px    — Pixel-perfect elements, retro buttons, CRT bezels
Subtle:     4px    — Input fields, small cards
Rounded:    8px    — Panels, menu containers
Soft:       12px   — Large cards, modal windows
Pill:       9999px — Tags, badges, toggle switches
Beveled:    2px chamfer — NES cartridge style elements
```

---

## Technology Stack

### Core Framework: Iced 0.13+ (Primary) + egui (Debug) + Slint (Optional)

**Why This Hybrid Architecture?**

| Aspect | Iced | egui | Slint | Decision |
|--------|------|------|-------|----------|
| **Rendering Model** | Retained + Immediate | Pure Immediate | Retained | Iced main UI, egui debug, Slint embedded |
| **Animation Support** | Native subscriptions | Manual per-frame | Property bindings | Iced for fluid animations |
| **Styling Flexibility** | Theme system | Inline styles | .slint markup | Iced for consistent theming |
| **GPU Integration** | wgpu native | wgpu via egui_wgpu | Multiple backends | All excellent |
| **Memory Footprint** | Medium | Small | Very Small | Slint for resource-constrained |
| **Learning Curve** | Elm architecture | Simpler | Declarative | Iced scales best |
| **HTPC/Controller** | Good | Limited | Excellent | Iced + custom widgets |

**Architecture Diagram:**

```
┌────────────────────────────────────────────────────────────────────┐
│                      RUSTYNES UI STACK v2.0                        │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    ICED APPLICATION LAYER                   │   │
│  │  • Main window chrome & title bar                           │   │
│  │  • ROM browser & library (Grid/List/CoverFlow)              │   │
│  │  • Settings panels with live preview                        │   │
│  │  • Netplay lobby with voice chat indicators                 │   │
│  │  • Achievement overlays & unlock animations                 │   │
│  │  • HTPC Controller-First navigation mode                    │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                     │
│                              ▼                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                    WGPU RENDER LAYER                        │   │
│  │  • Game viewport (256×240 → scaled with integer/adaptive)   │   │
│  │  • CRT shader pipeline (12+ presets)                        │   │
│  │  • Phosphor mask simulation (Aperture/Slot/Shadow)          │   │
│  │  • Scanline + bloom + curvature effects                     │   │
│  │  • NTSC composite simulation                                │   │
│  │  • Rolling scan CRT simulation (high-Hz displays)           │   │
│  │  • HDR tone mapping (when available)                        │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                     │
│                              ▼                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                 EGUI DEBUG OVERLAY (F12)                    │   │
│  │  • CPU/PPU/APU state viewers with live graphs               │   │
│  │  • Memory hex editor with search & watch                    │   │
│  │  • Trace logger with filtering                              │   │
│  │  • Lua scripting console with autocomplete                  │   │
│  │  • Run-ahead frame visualizer                               │   │
│  │  • Latency measurement display                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                     │
│                              ▼                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                  LATENCY REDUCTION ENGINE                   │   │
│  │  • Run-Ahead (1-4 frames, auto-detect per game)             │   │
│  │  • Preemptive Frames (alternative mode)                     │   │
│  │  • Frame Delay auto-tuning (0-15 frames)                    │   │
│  │  • Just-In-Time input polling                               │   │
│  │  • Dual-instance mode for audio stability                   │   │
│  │  • Adaptive sync (VRR/FreeSync/G-Sync)                      │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                     │
│                              ▼                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                   PLUGIN SYSTEM (NEW)                       │   │
│  │  • Shader plugins (.wgsl files)                             │   │
│  │  • Input mapper plugins                                     │   │
│  │  • Metadata scraper plugins                                 │   │
│  │  • Cloud sync plugins (Dropbox, GDrive, etc.)               │   │
│  │  • Social integration plugins (Discord, Twitch)             │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
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
    "lazy",           # Virtual scrolling (library performance)
    "debug",          # Debug overlay
    "multi-window",   # Detached debugger windows
]}
iced_aw = "0.10"      # Additional widgets (badges, cards, modals)

# Debug overlay (for developer tools)
egui = "0.28"
egui-wgpu = "0.28"
egui-winit = "0.28"

# Optional: Slint for embedded/resource-constrained builds
slint = { version = "1.7", optional = true }

# ═══════════════════════════════════════════════════════════════
# GRAPHICS & SHADERS
# ═══════════════════════════════════════════════════════════════
wgpu = "0.20"
naga = "0.20"              # Shader compilation
image = "0.25"             # Image loading/processing
resvg = "0.42"             # SVG rendering
fast_image_resize = "4.0"  # High-quality scaling
palette = "0.7"            # Color manipulation for CRT accuracy

# ═══════════════════════════════════════════════════════════════
# AUDIO
# ═══════════════════════════════════════════════════════════════
cpal = "0.15"         # Cross-platform audio
rubato = "0.15"       # High-quality resampling
dasp = "0.11"         # Digital audio signal processing
symphonia = "0.5"     # Audio decoding (for UI sounds)

# ═══════════════════════════════════════════════════════════════
# INPUT & HAPTICS
# ═══════════════════════════════════════════════════════════════
gilrs = "0.10"        # Gamepad support
gilrs-core = "0.5"    # Low-level gamepad access
winit = "0.30"        # Window/input events
sdl2 = { version = "0.36", features = ["haptic"], optional = true }

# ═══════════════════════════════════════════════════════════════
# FILE SYSTEM & PERSISTENCE
# ═══════════════════════════════════════════════════════════════
rfd = "0.14"          # Native file dialogs
notify = "6.1"        # File system watching (hot reload)
directories = "5.0"   # Platform-specific paths
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"          # Config files
bincode = "1.3"       # Fast serialization (savestates)
lz4_flex = "0.11"     # Compression (savestates, rewind)
zstd = "0.13"         # Higher compression for cloud sync
memmap2 = "0.9"       # Memory-mapped files for large rewind buffers

# ═══════════════════════════════════════════════════════════════
# ASYNC & CONCURRENCY
# ═══════════════════════════════════════════════════════════════
tokio = { version = "1.40", features = ["full"] }
crossbeam-channel = "0.5"
parking_lot = "0.12"
rayon = "1.10"        # Parallel iterators for library scanning

# ═══════════════════════════════════════════════════════════════
# NETWORKING (Netplay)
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
reqwest = { version = "0.12", features = ["json"] }  # API calls
scraper = "0.19"           # HTML parsing for metadata

# ═══════════════════════════════════════════════════════════════
# UTILITIES
# ═══════════════════════════════════════════════════════════════
chrono = "0.4"             # Date/time
humantime = "2.1"          # Human-readable durations
fuzzy-matcher = "0.3"      # Fuzzy search
unicode-segmentation = "1.11"  # Text handling
tracing = "0.1"            # Structured logging
tracing-subscriber = "0.3" # Log output

# ═══════════════════════════════════════════════════════════════
# SOCIAL INTEGRATION (Optional)
# ═══════════════════════════════════════════════════════════════
discord-rich-presence = { version = "0.2", optional = true }

# ═══════════════════════════════════════════════════════════════
# CLOUD SYNC (Optional)
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

## Application Architecture

### State Management (Enhanced Elm Architecture)

```rust
/// Root application state with latency-aware design
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

    /// Run-ahead engine (NEW)
    run_ahead: RunAheadEngine,

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

    /// HTPC mode state (NEW)
    htpc_mode: HtpcModeState,

    // ═══════════════════════════════════════════════════════════
    // FEATURE STATES
    // ═══════════════════════════════════════════════════════════

    /// ROM library state with metadata
    library: LibraryState,

    /// Settings state with per-game profiles
    settings: Settings,

    /// Active game profile (overrides global settings)
    game_profile: Option<GameProfile>,

    /// Debugger state (lazy loaded)
    debugger: Option<DebuggerState>,

    /// Netplay state
    netplay: NetplayState,

    /// Achievements state
    achievements: AchievementsState,

    /// TAS recorder state
    tas: TasState,

    /// Plugin manager (NEW)
    plugins: PluginManager,

    /// Cloud sync state (NEW)
    cloud_sync: Option<CloudSyncState>,
}

/// Run-Ahead Engine for sub-frame latency (NEW)
pub struct RunAheadEngine {
    /// Number of frames to run ahead (0 = disabled)
    frames: u8,

    /// Use second instance for audio (prevents pops)
    use_second_instance: bool,

    /// Auto-detect optimal frames per game
    auto_detect: bool,

    /// Preemptive frames mode (alternative to run-ahead)
    preemptive_mode: bool,

    /// Frame delay for GPU sync (0-15)
    frame_delay: u8,

    /// Frame delay auto-tuning enabled
    frame_delay_auto: bool,

    /// Cached save state for run-ahead
    cached_state: Option<Vec<u8>>,

    /// Second emulator instance (for audio stability)
    secondary_console: Option<Console>,

    /// Measured latency (for display)
    measured_latency_ms: f32,
}

/// Per-game configuration profile (NEW)
pub struct GameProfile {
    /// ROM hash (CRC32 or SHA-1)
    rom_hash: String,

    /// Game title (from database or user)
    title: String,

    /// Optimal run-ahead frames (auto-detected or manual)
    run_ahead_frames: u8,

    /// Video settings overrides
    video: VideoSettingsOverride,

    /// Audio settings overrides
    audio: AudioSettingsOverride,

    /// Input mapping overrides
    input: InputMappingOverride,

    /// Shader preset override
    shader_preset: Option<ShaderPreset>,

    /// Notes/comments
    notes: String,
}

/// HTPC Mode State (NEW)
pub struct HtpcModeState {
    /// Controller-first navigation enabled
    enabled: bool,

    /// Current focus path
    focus_path: Vec<FocusableElement>,

    /// Voice navigation enabled
    voice_nav: bool,

    /// 10-foot UI scaling factor
    scale_factor: f32,

    /// Cover flow position
    cover_flow_index: f32,

    /// Virtual shelf scroll position
    shelf_scroll: f32,
}

/// All possible views
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    /// Welcome screen (no ROM loaded)
    Welcome,

    /// ROM library browser
    Library,

    /// Library with Cover Flow (HTPC)
    LibraryCoverFlow,

    /// Library with Virtual Shelf (HTPC)
    LibraryShelf,

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

    /// Latency calibration wizard (NEW)
    LatencyWizard,

    /// Plugin manager (NEW)
    Plugins,
}

/// Extended settings tabs
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsTab {
    Video,
    Audio,
    Input,
    Paths,
    Network,
    Achievements,
    Latency,      // NEW
    Shaders,      // NEW (separated from Video)
    CloudSync,    // NEW
    Accessibility,
    Advanced,
}

/// Application messages (extended)
#[derive(Debug, Clone)]
pub enum Message {
    // ═══════════════════════════════════════════════════════════
    // NAVIGATION
    // ═══════════════════════════════════════════════════════════
    NavigateTo(View),
    GoBack,
    ToggleHtpcMode,

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
    // LATENCY CONTROL (NEW)
    // ═══════════════════════════════════════════════════════════
    SetRunAheadFrames(u8),
    ToggleRunAheadAutoDetect,
    ToggleSecondInstance,
    SetFrameDelay(u8),
    ToggleFrameDelayAuto,
    TogglePreemptiveMode,
    CalibrateLatency,
    LatencyMeasured(f32),

    // ═══════════════════════════════════════════════════════════
    // SHADER CONTROL (NEW)
    // ═══════════════════════════════════════════════════════════
    SetShaderPreset(ShaderPreset),
    SetPhosphorMask(PhosphorMaskType),
    SetScanlineIntensity(f32),
    SetCurvature(f32),
    SetBloom(f32),
    ToggleNtscSimulation,
    ToggleRollingScan,

    // ═══════════════════════════════════════════════════════════
    // SAVE STATES
    // ═══════════════════════════════════════════════════════════
    SaveState(u8),
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
    HapticFeedback(HapticPattern),  // NEW

    // ═══════════════════════════════════════════════════════════
    // HTPC MODE (NEW)
    // ═══════════════════════════════════════════════════════════
    HtpcNavigate(HtpcNavDirection),
    HtpcSelect,
    HtpcBack,
    CoverFlowScroll(f32),
    ShelfScroll(f32),
    VoiceCommand(String),

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
    SaveGameProfile,
    LoadGameProfile(String),
    DeleteGameProfile(String),

    // ═══════════════════════════════════════════════════════════
    // LIBRARY
    // ═══════════════════════════════════════════════════════════
    ScanRomDirectory(PathBuf),
    ScanComplete(Vec<RomEntry>),
    SearchLibrary(String),
    SortLibrary(SortOrder),
    FilterLibrary(Filter),
    ScrapeMetadata(Vec<RomEntry>),   // NEW
    MetadataScraped(RomEntry),       // NEW

    // ═══════════════════════════════════════════════════════════
    // PLUGINS (NEW)
    // ═══════════════════════════════════════════════════════════
    LoadPlugin(PathBuf),
    UnloadPlugin(PluginId),
    PluginMessage(PluginId, PluginEvent),

    // ═══════════════════════════════════════════════════════════
    // CLOUD SYNC (NEW)
    // ═══════════════════════════════════════════════════════════
    CloudSyncStart,
    CloudSyncComplete,
    CloudSyncConflict(SyncConflict),
    CloudSyncResolve(SyncResolution),

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
    PlaySound(UiSound),  // NEW

    // ═══════════════════════════════════════════════════════════
    // SYSTEM
    // ═══════════════════════════════════════════════════════════
    Tick(Instant),
    AudioCallback,
    DiscordPresenceUpdate,  // NEW
    Exit,
}

/// Shader presets (expanded)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaderPreset {
    None,
    // Basic
    Scanlines,
    ScanlinesLight,
    // CRT Family
    CrtEasymode,
    CrtGeom,
    CrtGeomCurvature,
    CrtLottes,
    CrtLottesFast,
    CrtRoyale,
    CrtRoyaleLite,
    CrtGuest,
    CrtGuestAdvanced,
    CrtPi,
    CrtFakeLottes,
    CrtNewpixie,
    // Special
    NtscComposite,
    NtscSvideo,
    NtscRgb,
    HdPack,
    // Rolling scan (for high-Hz displays)
    RollingScan60,
    RollingScan120,
    RollingScan240,
    // Custom
    Custom(String),
}

/// Phosphor mask types (NEW)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhosphorMaskType {
    None,
    ApertureGrille,      // Sony Trinitron style
    SlotMask,            // Most consumer CRTs
    ShadowMask,          // Classic dot pattern
    Edp,                 // Enhanced dot pitch
    ChromaticAberration, // Color fringing
}
```

### Component Hierarchy

```
┌───────────────────────────────────────────────────────────────────┐
│                         APPLICATION SHELL                         │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │                      TITLE BAR (Custom)                      │ │
│  │  [🎮 RustyNES]  [─] [□] [×]    ⚡ 2.1ms | FPS: 60.0 | 16.2ms  │ │
│  │                              └── Latency indicator (NEW)     │ │
│  └──────────────────────────────────────────────────────────────┘ │
│  ┌──────────┬───────────────────────────────────────────────────┐ │
│  │          │                                                   │ │
│  │  SIDEBAR │                   MAIN CONTENT                    │ │
│  │          │                                                   │ │
│  │ [🏠 Home]│  ┌────────────────────────────────────────────┐   │ │
│  │ [📚 Lib] │  │                                            │   │ │
│  │ [⚙ Set]  │  │              VIEW CONTAINER                │   │ │
│  │ [📡 Net] │  │                                            │   │ │
│  │ [🏆 Ach] │  │    (Welcome / Library / Playing / etc.)    │   │ │
│  │ [🐛 Dbg] │  │                                            │   │ │
│  │ [📝 TAS] │  │                                            │   │ │
│  │ [🔌 Plug]│  └────────────────────────────────────────────┘   │ │
│  │          │                                                   │ │
│  │  ──────  │  ┌────────────────────────────────────────────┐   │ │
│  │ Recently │  │              STATUS BAR                    │   │ │
│  │ [SMB3]   │  │  [🔊 100%] [▶ Running] [⚡ Run-Ahead: 2]    │   │ │
│  │ [Zelda]  │  │  [Frame: 123456] [CRT: Royale]             │   │ │
│  │ [Mega]   │  └────────────────────────────────────────────┘   │ │
│  └──────────┴───────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │                     TOAST CONTAINER                          │ │
│  │  [Achievement Unlocked! "First Steps" +10 pts     ✕]         │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │                    QUICK MENU (Overlay)                      │ │
│  │             (Shown on Escape / Start+Select)                 │ │
│  │                                                              │ │
│  │  ┌───────────────────────────────────────────────────────┐   │ │
│  │  │  ⚡ LATENCY: 2.1ms (Run-Ahead: 2 frames)              │   │ │
│  │  │  ───────────────────────────────────────────────────  │   │ │
│  │  │  [▶ Resume]  [💾 Quick Save]  [📂 Quick Load]          │   │ │
│  │  │  [⏪ Rewind] [📊 Stats]       [⚙ Settings]             │   │ │
│  │  │  [📷 Screenshot]  [🎬 Record]  [🚪 Exit Game]          │   │ │
│  │  └───────────────────────────────────────────────────────┘   │ │
│  │                                                              │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │                     MODAL CONTAINER                          │ │
│  │                   (Dialogs, Confirmations)                   │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │                  LATENCY HUD (In-Game, F3)                   │ │
│  │  ┌─────────────────────────────────────────────────────────┐ │ │
│  │  │ Input: 0.8ms | Run-Ahead: 2 | Total: 2.1ms | GPU: 1.3ms │ │ │
│  │  └─────────────────────────────────────────────────────────┘ │ │
│  └──────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────┘
```

---

## HTPC Controller-First Mode

### 10-Foot UI Design

RustyNES includes a dedicated HTPC mode designed for living room gaming with controllers. This mode is activated via Settings or automatically when a gamepad is detected at startup with no keyboard present.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HTPC COVER FLOW VIEW                             │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ╔═══════════════════════════════════════════════════════════════╗  │
│  ║                                                               ║  │
│  ║          ┌───────┐  ┌─────────────┐  ┌───────┐                ║  │
│  ║          │░░░░░░░│  │░░░░░░░░░░░░░│  │░░░░░░░│                ║  │
│  ║          │░░░░░░░│  │░░░░░░░░░░░░░│  │░░░░░░░│                ║  │
│  ║      ← ─ │░ SMB ░│  │░░ ZELDA ░░░│  │░CONTRA│ ─ →             ║  │
│  ║          │░░ 1 ░░│  │░░░░░░░░░░░░░│  │░░░░░░░│                ║  │
│  ║          │░░░░░░░│  │░░░░░░░░░░░░░│  │░░░░░░░│                ║  │
│  ║          └───────┘  │░░░░░░░░░░░░░│  └───────┘                ║  │
│  ║           (small)   │░░░░░░░░░░░░░│   (small)                 ║  │
│  ║                     │░░░░░░░░░░░░░│                           ║  │
│  ║                     └─────────────┘                           ║  │
│  ║                        (focused)                              ║  │
│  ║                                                               ║  │
│  ║  ══════════════════════════════════════════════════════════   ║  │
│  ║                                                               ║  │
│  ║          THE LEGEND OF ZELDA                                  ║  │
│  ║          ────────────────────                                 ║  │
│  ║          Nintendo • 1986 • Action-Adventure                   ║  │
│  ║          Play Time: 12h 34m • Last: Yesterday                 ║  │
│  ║          ★★★★★                                                ║  │
│  ║                                                               ║  │
│  ║  ──────────────────────────────────────────────────────────── ║  │
│  ║                                                               ║  │
│  ║    [A] Play    [X] Favorites    [Y] Options    [B] Back       ║  │
│  ║                                                               ║  │
│  ╚═══════════════════════════════════════════════════════════════╝  │
│                                                                     │
│  FEATURES:                                                          │
│  • 3D perspective Cover Flow with reflection                        │
│  • Smooth 60fps scrolling with momentum physics                     │
│  • Gamepad rumble feedback on selection                             │
│  • Large 48px fonts for 10-foot readability                         │
│  • Auto-scraping for box art and metadata                           │
│  • Recently played section at top                                   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Virtual Shelf View

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HTPC VIRTUAL SHELF VIEW                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ╔═══════════════════════════════════════════════════════════════╗  │
│  ║                                                               ║  │
│  ║  RECENTLY PLAYED                                     [See All]║  │
│  ║  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐       ║  │
│  ║  │████████│ │████████│ │████████│ │████████│ │████████│       ║  │
│  ║  │████████│ │████████│ │████████│ │████████│ │████████│       ║  │
│  ║  │ Zelda  │ │  SMB3  │ │Mega Man│ │Castlev.│ │ Contra │       ║  │
│  ║  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘       ║  │
│  ║                                                               ║  │
│  ║  FAVORITES                                           [See All]║  │
│  ║  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐       ║  │
│  ║  │████████│ │████████│ │████████│ │████████│ │████████│       ║  │
│  ║  │████████│ │████████│ │████████│ │████████│ │████████│       ║  │
│  ║  │ Zelda  │ │ Metroid│ │Kid Icar│ │ Kirby  │ │ Tetris │       ║  │
│  ║  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘       ║  │
│  ║                                                               ║  │
│  ║  ALL GAMES (247)                                     [See All]║  │
│  ║  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐       ║  │
│  ║  │▓▓▓▓▓▓▓▓│ │████████│ │████████│ │████████│ │████████│       ║  │
│  ║  │▓▓▓▓▓▓▓▓│ │████████│ │████████│ │████████│ │████████│       ║  │
│  ║  │1942    │ │ Abadox │ │ActRaiser│ │Adv.Lolo│ │ Airwolf│►     ║  │
│  ║  └────────┘ └────────┘ └────────┘ └────────┘ └────────┘       ║  │
│  ║   (focused)                                                   ║  │
│  ║                                                               ║  │
│  ║  ──────────────────────────────────────────────────────────── ║  │
│  ║                                                               ║  │
│  ║    [A] Play    [X] Info    [LB/RB] Scroll Row    [B] Back     ║  │
│  ║                                                               ║  │
│  ╚═══════════════════════════════════════════════════════════════╝  │
│                                                                     │
│  NAVIGATION:                                                        │
│  • D-Pad Left/Right: Move within row                                │
│  • D-Pad Up/Down: Move between rows                                 │
│  • LB/RB: Page scroll within row                                    │
│  • Left Stick: Smooth scroll with momentum                          │
│  • Hold A: Quick launch (skip confirmation)                         │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Controller Mapping for HTPC

```
┌─────────────────────────────────────────────────────────────────────┐
│                    HTPC CONTROLLER MAPPING                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  MENU NAVIGATION:                                                   │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                                                               │  │
│  │     [LB]            [RB]           Page scroll                │  │
│  │      ╭─────────────────────────╮                              │  │
│  │      │     [MENU]    [START]   │   Menu / Pause               │  │
│  │  [LT]│  ╭───╮          ╭───╮   │[RT]                          │  │
│  │      │  │ L │   [SEL]  │ R │   │   Stick scroll               │  │
│  │      │  ╰───╯          ╰───╯   │                              │  │
│  │      │     ╭───╮   [A]         │   A = Select/Confirm         │  │
│  │      │     │ D │        [X]    │   B = Back/Cancel            │  │
│  │      │ ←───┼───┼───→    [Y]    │   X = Favorite/Action        │  │
│  │      │     │   │        [B]    │   Y = Options/Context        │  │
│  │      │     ╰───╯               │                              │  │
│  │      ╰─────────────────────────╯                              │  │
│  │                                                               │  │
│  │  D-Pad: Discrete navigation                                   │  │
│  │  Left Stick: Smooth scroll / Cover Flow rotation              │  │
│  │  Right Stick: Quick jump by letter (in alphabetical view)     │  │
│  │                                                               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  IN-GAME QUICK MENU (Start + Select):                               │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                                                               │  │
│  │  LB/RB: Navigate tabs                                         │  │
│  │  D-Pad: Navigate options                                      │  │
│  │  A: Select                                                    │  │
│  │  B: Resume game                                               │  │
│  │  Y: Quick save                                                │  │
│  │  X: Quick load                                                │  │
│  │                                                               │  │
│  │  SPECIAL COMBOS:                                              │  │
│  │  Start + Select: Quick menu                                   │  │
│  │  Start + Select + LB: Screenshot                              │  │
│  │  Start + Select + RB: Toggle run-ahead                        │  │
│  │  Hold LT + RT (3s): Emergency exit to menu                    │  │
│  │                                                               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Latency Reduction System

### Run-Ahead Implementation

Run-Ahead is a technique that can achieve **lower input latency than original NES hardware on a CRT**. Based on research from byuu/Near and RetroArch's implementation.

```
┌─────────────────────────────────────────────────────────────────────┐
│                      RUN-AHEAD SYSTEM                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  HOW IT WORKS:                                                      │
│  ────────────                                                       │
│                                                                     │
│  Standard emulation:                                                │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │ Frame 1 → Frame 2 → Frame 3 → [Display]                    │     │
│  │    ↑         ↑         ↑                                   │     │
│  │ [Input]   (delay)   (delay)                                │     │
│  │                                                            │     │
│  │ Result: 2-3 frame lag (33-50ms @ 60Hz NTSC)                │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
│  With Run-Ahead (frames=2):                                         │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │ [Input] → Save State                                       │     │
│  │              ↓                                             │     │
│  │         Frame 1 (discarded)                                │     │
│  │              ↓                                             │     │
│  │         Frame 2 (discarded)                                │     │
│  │              ↓                                             │     │
│  │         Frame 3 → [Display] ← You see THIS frame!          │     │
│  │              ↓                                             │     │
│  │         Load State (restore to Frame 1 state)              │     │
│  │                                                            │     │
│  │ Result: 0-1 frame lag (~16ms or less!)                     │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
│  CONFIGURATION:                                                     │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                                                             │    │
│  │  Run-Ahead Frames: [▼ 2]  (Auto-detect: ☑)                  │    │
│  │                                                             │    │
│  │  ○ 0 - Disabled                                             │    │
│  │  ○ 1 - Safe for all games (~16ms reduction)                 │    │
│  │  ● 2 - Optimal for most games (~32ms reduction)             │    │
│  │  ○ 3 - Some games may glitch                                │    │
│  │  ○ 4 - Maximum (few games support this)                     │    │
│  │                                                             │    │
│  │  ────────────────────────────────────────────────────────   │    │
│  │                                                             │    │
│  │  [✓] Use Second Instance (prevents audio pops)              │    │
│  │  [ ] Preemptive Frames Mode (alternative algorithm)         │    │
│  │                                                             │    │
│  │  ────────────────────────────────────────────────────────   │    │
│  │                                                             │    │
│  │  CPU Usage: ████████████░░░░░░░░ 60%                        │    │
│  │  (Higher run-ahead = more CPU required)                     │    │
│  │                                                             │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                     │
│  AUTO-DETECTION:                                                    │
│  ──────────────                                                     │
│                                                                     │
│  RustyNES can automatically detect optimal run-ahead per game:      │
│                                                                     │
│  1. Pause emulation                                                 │
│  2. Press and hold a direction                                      │
│  3. Frame advance until character moves                             │
│  4. Count frames - 1 = safe run-ahead setting                       │
│                                                                     │
│  This is stored in per-game profiles automatically.                 │
│                                                                     │
│  KNOWN OPTIMAL VALUES:                                              │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │ Game                    │ Internal Lag │ Optimal Run-Ahead │     │
│  │─────────────────────────│──────────────│────────────────── │     │
│  │ Super Mario Bros.       │ 1 frame      │ 1                 │     │
│  │ Super Mario Bros. 3     │ 2 frames     │ 2                 │     │
│  │ The Legend of Zelda     │ 2 frames     │ 2                 │     │
│  │ Mega Man 2              │ 1 frame      │ 1                 │     │
│  │ Castlevania             │ 2 frames     │ 2                 │     │
│  │ Contra                  │ 1 frame      │ 1                 │     │
│  │ Metroid                 │ 2 frames     │ 2                 │     │
│  │ Kirby's Adventure       │ 2 frames     │ 2                 │     │
│  │ Battletoads             │ 1 frame      │ 1                 │     │
│  └────────────────────────────────────────────────────────────┘     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Run-Ahead Implementation Code

```rust
/// Run-ahead engine for sub-frame latency reduction
pub struct RunAheadEngine {
    /// Number of frames to run ahead (0 = disabled)
    pub frames: u8,

    /// Use secondary instance for clean audio
    pub use_second_instance: bool,

    /// Secondary console instance (when enabled)
    secondary_console: Option<Console>,

    /// Cached save state for rollback
    cached_state: Vec<u8>,

    /// LZ4 compressor for fast state compression
    compressor: lz4_flex::frame::FrameEncoder<Vec<u8>>,
}

impl RunAheadEngine {
    /// Execute one frame with run-ahead latency reduction
    ///
    /// This achieves latency lower than original hardware by
    /// "predicting" future frames based on current input.
    pub fn run_frame(
        &mut self,
        console: &mut Console,
        input: ControllerState,
    ) -> (FrameBuffer, AudioSamples) {
        if self.frames == 0 {
            // Run-ahead disabled, normal execution
            console.set_input(input);
            return console.run_frame();
        }

        // Poll input FIRST (Just-In-Time polling)
        console.set_input(input);

        // Save state for rollback
        self.cached_state.clear();
        console.serialize(&mut self.cached_state);

        // Run ahead frames (discarded)
        for _ in 0..self.frames {
            let _ = console.run_frame();  // Discard output
        }

        // Run the DISPLAYED frame
        let (video, audio) = if self.use_second_instance {
            // Use secondary instance for audio to prevent pops
            if let Some(ref mut secondary) = self.secondary_console {
                secondary.unserialize(&self.cached_state);
                secondary.set_input(input);

                // Run secondary to current position
                for _ in 0..self.frames {
                    let _ = secondary.run_frame();
                }

                // Get audio from secondary, video from primary
                let (video, _) = console.run_frame();
                let (_, audio) = secondary.run_frame();
                (video, audio)
            } else {
                console.run_frame()
            }
        } else {
            console.run_frame()
        };

        // Rollback primary to saved state
        console.unserialize(&self.cached_state);

        // Advance primary by ONE frame (the "real" frame)
        let _ = console.run_frame();

        (video, audio)
    }

    /// Auto-detect optimal run-ahead frames for current game
    ///
    /// Returns the number of frames between input and visible response.
    pub fn auto_detect_frames(&mut self, console: &mut Console) -> u8 {
        // Save current state
        let mut original_state = Vec::new();
        console.serialize(&mut original_state);

        // Get baseline frame (no input)
        console.set_input(ControllerState::default());
        let baseline = console.run_frame().0;

        // Restore
        console.unserialize(&original_state);

        // Apply input and count frames until change
        console.set_input(ControllerState::RIGHT);

        for frame_count in 1..=4 {
            let current = console.run_frame().0;

            // Simple pixel difference check
            if frames_different(&baseline, &current) {
                console.unserialize(&original_state);
                return frame_count.saturating_sub(1).max(1);
            }
        }

        // Default to 1 if no change detected
        console.unserialize(&original_state);
        1
    }
}

/// Check if two frames are visually different
fn frames_different(a: &FrameBuffer, b: &FrameBuffer) -> bool {
    a.iter().zip(b.iter()).any(|(pa, pb)| pa != pb)
}
```

### Frame Delay System

```
┌────────────────────────────────────────────────────────────────────┐
│                      FRAME DELAY SYSTEM                            │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  Frame Delay optimizes the timing of when input is polled          │
│  relative to when the frame is sent to the GPU.                    │
│                                                                    │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                                                            │    │
│  │  Frame Delay: [▼ Auto]                                     │    │
│  │                                                            │    │
│  │  Manual values: 0-15 (higher = poll input later)           │    │
│  │                                                            │    │
│  │  ────────────────────────────────────────────────────────  │    │
│  │                                                            │    │
│  │  [✓] Auto Frame Delay                                      │    │
│  │      Automatically adjusts based on frame time headroom    │    │
│  │                                                            │    │
│  │  Current: 8 (of 16.67ms frame time)                        │    │
│  │  ██████████████░░░░░░░░░░░░░░░░░░░░                        │    │
│  │  └─ Input polled here (8ms before vsync)                   │    │
│  │                                                            │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                    │
│  COMBINED LATENCY REDUCTION:                                       │
│  ────────────────────────────                                      │
│                                                                    │
│  │ Technique              │ Reduction  │ CPU Cost │                │
│  │────────────────────────│────────────│──────────│                │
│  │ Run-Ahead (2 frames)   │ ~32ms      │ 2-3×     │                │
│  │ Frame Delay (auto)     │ ~8ms       │ None     │                │
│  │ JIT Input Polling      │ ~1ms       │ None     │                │
│  │ Adaptive Sync          │ Variable   │ None     │                │
│  │────────────────────────│────────────│──────────│                │
│  │ TOTAL                  │ ~41ms      │ 2-3×     │                │
│                                                                    │
│  Original NES on CRT: ~50ms                                        │
│  RustyNES with full optimization: ~9ms (!)                         │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Latency Settings Panel

```
┌──────────────────────────────────────────────────────────────────┐
│                      LATENCY SETTINGS                            │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────────────────────────────────────────────┐│
│  │                                                              ││
│  │  CURRENT LATENCY                                             ││
│  │  ══════════════                                              ││
│  │                                                              ││
│  │  ┌──────────────────────────────────────────────────────┐    ││
│  │  │                                                      │    ││
│  │  │    ⚡ 2.1ms Total Input Latency                      │    ││
│  │  │                                                      │    ││
│  │  │    Breakdown:                                        │    ││
│  │  │    • Input Polling:     0.8ms                        │    ││
│  │  │    • Run-Ahead Saved:  32.0ms (2 frames)             │    ││
│  │  │    • Frame Delay:       8.0ms                        │    ││
│  │  │    • GPU Present:       1.3ms                        │    ││
│  │  │    • Display Lag:      ~5.0ms (monitor dependent)    │    ││
│  │  │                                                      │    ││
│  │  │    vs Original Hardware: ~50ms                       │    ││
│  │  │    Improvement: 96% faster! 🎉                       │    ││
│  │  │                                                      │    ││
│  │  └──────────────────────────────────────────────────────┘    ││
│  │                                                              ││
│  │  ──────────────────────────────────────────────────────────  ││
│  │                                                              ││
│  │  RUN-AHEAD                                                   ││
│  │  ──────────                                                  ││
│  │                                                              ││
│  │  Frames: [◀ 2 ▶]  (Auto-Detect: ☑)                           ││
│  │                                                              ││
│  │  ℹ️  Run-ahead "predicts" future frames to eliminate the     ││
│  │     built-in lag that exists in all NES games.               ││
│  │                                                              ││
│  │  [✓] Use Second Instance (cleaner audio)                     ││
│  │                                                              ││
│  │  ──────────────────────────────────────────────────────────  ││
│  │                                                              ││
│  │  FRAME DELAY                                                 ││
│  │  ───────────                                                 ││
│  │                                                              ││
│  │  Delay: [◀ Auto ▶]                                           ││
│  │                                                              ││
│  │  ℹ️  Frame delay polls input as late as possible within      ││
│  │     each frame to reduce input-to-display latency.           ││
│  │                                                              ││
│  │  ──────────────────────────────────────────────────────────  ││
│  │                                                              ││
│  │  VSYNC & DISPLAY                                             ││
│  │  ───────────────                                             ││
│  │                                                              ││
│  │  VSync Mode: [▼ Adaptive (VRR)]                              ││
│  │                                                              ││
│  │  Options:                                                    ││
│  │  • Off (tearing, lowest latency)                             ││
│  │  • On (no tearing, +1 frame latency)                         ││
│  │  • Adaptive (VRR/FreeSync/G-Sync) ← Recommended              ││
│  │  • Fast (NVIDIA low-latency mode)                            ││
│  │                                                              ││
│  │  [ ] Black Frame Insertion (BFI) - requires 120Hz+           ││
│  │                                                              ││
│  │  ──────────────────────────────────────────────────────────  ││
│  │                                                              ││
│  │  [🎯 Calibrate Latency]  [↻ Reset to Defaults]               ││
│  │                                                              ││
│  └──────────────────────────────────────────────────────────────┘│
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

---

## Enhanced CRT Shader Pipeline

### Shader Preset Gallery

```
┌───────────────────────────────────────────────────────────────────┐
│                    CRT SHADER PRESETS                             │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌───────────────────────────────────────────────────────────────┐│
│  │                                                               ││
│  │  [Grid View]  [List View]  Search: [________________]         ││
│  │                                                               ││
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐           ││
│  │  │░░░░░░░░░░░░░░│ │▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ │██████████████│           ││
│  │  │░░ PREVIEW ░░░│ │▓▓ PREVIEW ▓▓▓│ │██ PREVIEW ███│           ││
│  │  │░░░░░░░░░░░░░░│ │▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ │██████████████│           ││
│  │  ├──────────────┤ ├──────────────┤ ├──────────────┤           ││
│  │  │ None         │ │ Scanlines    │ │ CRT Easymode │           ││
│  │  │ Clean pixels │ │ Light lines  │ │ Balanced     │           ││
│  │  │ GPU: Low     │ │ GPU: Low     │ │ GPU: Low     │           ││
│  │  └──────────────┘ └──────────────┘ └──────────────┘           ││
│  │       ○                 ○                ○                    ││
│  │                                                               ││
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐           ││
│  │  │██████████████│ │▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ │░░░░░░░░░░░░░░│           ││
│  │  │██ PREVIEW ███│ │▓▓ PREVIEW ▓▓▓│ │░░ PREVIEW ░░░│           ││
│  │  │██████████████│ │▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ │░░░░░░░░░░░░░░│           ││
│  │  ├──────────────┤ ├──────────────┤ ├──────────────┤           ││
│  │  │ CRT Geom     │ │ CRT Lottes   │ │ CRT Royale   │           ││
│  │  │ + Curvature  │ │ Arcade style │ │ Full sim     │           ││
│  │  │ GPU: Medium  │ │ GPU: Low     │ │ GPU: High    │           ││
│  │  └──────────────┘ └──────────────┘ └──────────────┘           ││
│  │       ○                 ●                ○                    ││
│  │                      (selected)                               ││
│  │                                                               ││
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐           ││
│  │  │██████████████│ │▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ │░░░░░░░░░░░░░░│           ││
│  │  │██ PREVIEW ███│ │▓▓ PREVIEW ▓▓▓│ │░░ PREVIEW ░░░│           ││
│  │  │██████████████│ │▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ │░░░░░░░░░░░░░░│           ││
│  │  ├──────────────┤ ├──────────────┤ ├──────────────┤           ││
│  │  │ NTSC Composi.│ │ Rolling Scan │ │ HD Pack      │           ││
│  │  │ Retro look   │ │ 240Hz+ only  │ │ Enhanced GFX │           ││
│  │  │ GPU: Medium  │ │ GPU: High    │ │ GPU: Variable│           ││
│  │  └──────────────┘ └──────────────┘ └──────────────┘           ││
│  │       ○                 ○                ○                    ││
│  │                                                               ││
│  └───────────────────────────────────────────────────────────────┘│
│                                                                   │
└───────────────────────────────────────────────────────────────────┘
```

### Advanced Shader Configuration

```
┌────────────────────────────────────────────────────────────────────┐
│                 ADVANCED SHADER CONFIGURATION                      │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  ┌────────────────┬────────────────────────────────────────────┐   │
│  │                │                                            │   │
│  │  CATEGORIES    │          CRT LOTTES (Selected)             │   │
│  │                │                                            │   │
│  │  ┌──────────┐  │  ┌──────────────────────────────────────┐  │   │
│  │  │📺 Display│  │  │                                      │  │   │
│  │  └──────────┘  │  │   ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  │  │   │
│  │  ┌──────────┐  │  │   ░░░ LIVE PREVIEW (Mario World) ░░  │  │   │
│  │  │〰️ Scanlin│  │  │   ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  │  │   │
│  │  └──────────┘  │  │                                      │  │   │
│  │  ┌──────────┐  │  └──────────────────────────────────────┘  │   │
│  │  │🔲 Mask   │  │                                            │   │
│  │  └──────────┘  │  ────────────────────────────────────────  │   │
│  │  ┌──────────┐  │                                            │   │
│  │  │✨ Effects│  │  SCANLINES                                 │   │
│  │  └──────────┘  │  ──────────                                │   │
│  │  ┌──────────┐  │                                            │   │
│  │  │🎨 Color  │  │  Intensity: ████████████░░░░ 75%           │   │
│  │  └──────────┘  │                                            │   │
│  │                │  Thickness:  ██████░░░░░░░░░░ 40%          │   │
│  │  ──────────    │                                            │   │
│  │                │  Bloom:      ████████░░░░░░░░ 50%          │   │
│  │  PRESETS       │                                            │   │
│  │  ────────      │  ────────────────────────────────────────  │   │
│  │  [▼ Lottes]    │                                            │   │
│  │                │  PHOSPHOR MASK                             │   │
│  │  [💾 Save As]  │  ─────────────                             │   │
│  │  [📂 Import]   │                                            │   │
│  │  [↻ Reset]     │  Type: [▼ Aperture Grille]                 │   │
│  │                │                                            │   │
│  │                │  ┌─────────────────────────────────────┐   │   │
│  │                │  │ ◉ Aperture Grille (Sony Trinitron)  │   │   │
│  │                │  │ ○ Slot Mask (Most consumer CRTs)    │   │   │
│  │                │  │ ○ Shadow Mask (Classic dot pattern) │   │   │
│  │                │  │ ○ EDP (Enhanced dot pitch)          │   │   │
│  │                │  │ ○ None                              │   │   │
│  │                │  └─────────────────────────────────────┘   │   │
│  │                │                                            │   │
│  │                │  Mask Intensity: ██████████░░░░ 65%        │   │
│  │                │                                            │   │
│  │                │  Dot Pitch: ██████░░░░░░░░░░ 2.0           │   │
│  │                │  (Smaller = sharper, like PC CRT)          │   │
│  │                │                                            │   │
│  │                │  ────────────────────────────────────────  │   │
│  │                │                                            │   │
│  │                │  CURVATURE                                 │   │
│  │                │  ─────────                                 │   │
│  │                │                                            │   │
│  │                │  [✓] Enable Curvature                      │   │
│  │                │                                            │   │
│  │                │  Horizontal: █████████░░░░░░░ 55%          │   │
│  │                │  Vertical:   ███████░░░░░░░░░ 45%          │   │
│  │                │                                            │   │
│  │                │  Corner Size: ████░░░░░░░░░░░░ 25%         │   │
│  │                │                                            │   │
│  │                │  ────────────────────────────────────────  │   │
│  │                │                                            │   │
│  │                │  BLOOM & HALATION                          │   │
│  │                │  ────────────────                          │   │
│  │                │                                            │   │
│  │                │  Bloom Amount: ████████░░░░░░░░ 50%        │   │
│  │                │  Bloom Radius: ██████░░░░░░░░░░ 35%        │   │
│  │                │                                            │   │
│  │                │  [✓] Halation (internal reflections)       │   │
│  │                │  Halation Amount: █████░░░░░░░░░ 30%       │   │
│  │                │                                            │   │
│  └────────────────┴────────────────────────────────────────────┘   │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### CRT Shader Implementation (WGSL)

```wgsl
// Enhanced CRT shader with phosphor mask simulation
// Based on CRT-Lottes and CRT-Royale techniques

struct CrtUniforms {
    screen_size: vec2<f32>,
    texture_size: vec2<f32>,
    time: f32,

    // Scanlines
    scanline_intensity: f32,
    scanline_thickness: f32,
    scanline_bloom: f32,

    // Phosphor mask
    mask_type: i32,  // 0=none, 1=aperture, 2=slot, 3=shadow, 4=edp
    mask_intensity: f32,
    dot_pitch: f32,

    // Curvature
    curvature_enabled: i32,
    curvature_x: f32,
    curvature_y: f32,
    corner_size: f32,

    // Bloom
    bloom_amount: f32,
    bloom_radius: f32,
    halation_amount: f32,

    // Color
    brightness: f32,
    contrast: f32,
    saturation: f32,
};

@group(0) @binding(0) var nes_texture: texture_2d<f32>;
@group(0) @binding(1) var nes_sampler: sampler;
@group(0) @binding(2) var<uniform> u: CrtUniforms;

const PI: f32 = 3.14159265359;

// Barrel distortion for CRT curvature
fn barrel_distort(uv: vec2<f32>) -> vec2<f32> {
    if (u.curvature_enabled == 0) {
        return uv;
    }

    let centered = uv * 2.0 - 1.0;
    let dist = dot(centered, centered);
    let curvature = vec2<f32>(u.curvature_x, u.curvature_y) * 0.1;
    let distorted = centered * (1.0 + dist * curvature);
    return distorted * 0.5 + 0.5;
}

// Corner vignette for rounded CRT edges
fn corner_mask(uv: vec2<f32>) -> f32 {
    let centered = abs(uv * 2.0 - 1.0);
    let corner = 1.0 - u.corner_size;
    let edge = smoothstep(corner, 1.0, max(centered.x, centered.y));
    return 1.0 - edge;
}

// Scanline function with bloom
fn scanline(uv: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let y = uv.y * u.texture_size.y;
    let line = sin(y * PI * 2.0);

    // Brighter pixels have thicker scanlines (beam dynamics)
    let luminance = dot(color, vec3<f32>(0.299, 0.587, 0.114));
    let beam_width = mix(u.scanline_thickness, 1.0, luminance * u.scanline_bloom);

    let scanline_mask = 1.0 - u.scanline_intensity * (1.0 - smoothstep(0.0, beam_width, abs(line)));
    return color * scanline_mask;
}

// Aperture grille phosphor mask (vertical RGB stripes)
fn aperture_grille_mask(coord: vec2<f32>) -> vec3<f32> {
    let x = coord.x * u.screen_size.x / u.dot_pitch;
    let phase = fract(x);

    var mask = vec3<f32>(1.0);
    if (phase < 0.333) {
        mask = vec3<f32>(1.0, u.mask_intensity, u.mask_intensity);
    } else if (phase < 0.666) {
        mask = vec3<f32>(u.mask_intensity, 1.0, u.mask_intensity);
    } else {
        mask = vec3<f32>(u.mask_intensity, u.mask_intensity, 1.0);
    }

    return mask;
}

// Slot mask phosphor pattern
fn slot_mask(coord: vec2<f32>) -> vec3<f32> {
    let pos = coord * u.screen_size / u.dot_pitch;
    let x_phase = fract(pos.x);
    let y_offset = fract(pos.y * 0.5) > 0.5;

    let adjusted_x = x_phase + select(0.0, 0.5, y_offset);
    let final_phase = fract(adjusted_x);

    var mask = vec3<f32>(1.0);
    if (final_phase < 0.333) {
        mask = vec3<f32>(1.0, u.mask_intensity, u.mask_intensity);
    } else if (final_phase < 0.666) {
        mask = vec3<f32>(u.mask_intensity, 1.0, u.mask_intensity);
    } else {
        mask = vec3<f32>(u.mask_intensity, u.mask_intensity, 1.0);
    }

    return mask;
}

// Shadow mask (dot triad pattern)
fn shadow_mask(coord: vec2<f32>) -> vec3<f32> {
    let pos = coord * u.screen_size / u.dot_pitch;
    let cell = floor(pos);
    let sub = fract(pos);

    let row_offset = select(0.0, 0.5, fract(cell.y * 0.5) > 0.25);
    let x = fract(sub.x + row_offset);

    // Circular phosphor dots
    let dist_r = length(vec2<f32>(x - 0.166, sub.y - 0.5));
    let dist_g = length(vec2<f32>(x - 0.5, sub.y - 0.5));
    let dist_b = length(vec2<f32>(x - 0.833, sub.y - 0.5));

    let dot_size = 0.25;
    var mask = vec3<f32>(
        smoothstep(dot_size, dot_size * 0.5, dist_r),
        smoothstep(dot_size, dot_size * 0.5, dist_g),
        smoothstep(dot_size, dot_size * 0.5, dist_b)
    );

    return mix(vec3<f32>(u.mask_intensity), vec3<f32>(1.0), mask);
}

// Get phosphor mask based on type
fn get_phosphor_mask(coord: vec2<f32>) -> vec3<f32> {
    switch (u.mask_type) {
        case 1: { return aperture_grille_mask(coord); }
        case 2: { return slot_mask(coord); }
        case 3: { return shadow_mask(coord); }
        default: { return vec3<f32>(1.0); }
    }
}

// Simple bloom (box blur approximation)
fn bloom(uv: vec2<f32>) -> vec3<f32> {
    let texel = 1.0 / u.texture_size;
    let radius = u.bloom_radius * 3.0;

    var sum = vec3<f32>(0.0);
    var weight_sum = 0.0;

    for (var y = -2.0; y <= 2.0; y += 1.0) {
        for (var x = -2.0; x <= 2.0; x += 1.0) {
            let offset = vec2<f32>(x, y) * texel * radius;
            let weight = 1.0 / (1.0 + length(vec2<f32>(x, y)));
            sum += textureSample(nes_texture, nes_sampler, uv + offset).rgb * weight;
            weight_sum += weight;
        }
    }

    return sum / weight_sum;
}

@fragment
fn fs_main(@location(0) uv_in: vec2<f32>) -> @location(0) vec4<f32> {
    // Apply barrel distortion
    let uv = barrel_distort(uv_in);

    // Check bounds (black outside CRT area)
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Sample main texture
    var color = textureSample(nes_texture, nes_sampler, uv).rgb;

    // Apply bloom
    if (u.bloom_amount > 0.0) {
        let bloom_color = bloom(uv);
        color = mix(color, bloom_color, u.bloom_amount * 0.3);
    }

    // Apply halation (internal CRT reflections)
    if (u.halation_amount > 0.0) {
        let halation = bloom(uv) * u.halation_amount * 0.2;
        color += halation;
    }

    // Apply scanlines
    color = scanline(uv, color);

    // Apply phosphor mask
    let mask = get_phosphor_mask(uv_in);  // Use original UV for mask alignment
    color *= mask;

    // Apply corner vignette
    color *= corner_mask(uv);

    // Color adjustments
    color = (color - 0.5) * u.contrast + 0.5 + u.brightness - 1.0;
    let gray = dot(color, vec3<f32>(0.299, 0.587, 0.114));
    color = mix(vec3<f32>(gray), color, u.saturation);

    // Phosphor glow (slight color bleeding)
    color.r = color.r * 0.95 + color.g * 0.05;
    color.b = color.b * 0.95 + color.g * 0.05;

    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
```

---

## Sound Design System

### UI Audio Feedback

```
┌────────────────────────────────────────────────────────────────────┐
│                     SOUND DESIGN SYSTEM                            │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  PHILOSOPHY:                                                       │
│  ───────────                                                       │
│                                                                    │
│  UI sounds should feel like they belong on an NES, using the       │
│  same audio characteristics: square waves, triangle waves,         │
│  simple ADSR envelopes, and the familiar "chirpy" quality.         │
│                                                                    │
│  SOUND CATEGORIES:                                                 │
│                                                                    │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                                                            │    │
│  │  NAVIGATION                                                │    │
│  │  • Menu Move:     Short blip (440Hz square, 20ms)          │    │
│  │  • Menu Select:   Confirmation chime (880Hz→440Hz, 100ms)  │    │
│  │  • Menu Back:     Lower tone (220Hz, 50ms)                 │    │
│  │  • Page Change:   Swoosh (noise + triangle sweep)          │    │
│  │                                                            │    │
│  │  ACTIONS                                                   │    │
│  │  • Save State:    "Power-up" arpeggio (C-E-G, 150ms)       │    │
│  │  • Load State:    Reverse arpeggio (G-E-C, 150ms)          │    │
│  │  • Screenshot:    Camera click (noise burst, 30ms)         │    │
│  │  • Error:         Low buzz (110Hz square, 200ms)           │    │
│  │                                                            │    │
│  │  ACHIEVEMENTS                                              │    │
│  │  • Unlock:        Full fanfare (500ms, multi-channel)      │    │
│  │  • Progress:      Subtle ding (1000Hz triangle, 50ms)      │    │
│  │                                                            │    │
│  │  SYSTEM                                                    │    │
│  │  • Boot:          NES power-on simulation (optional)       │    │
│  │  • Shutdown:      Soft fade-out tone                       │    │
│  │  • Notification:  Soft chime (non-intrusive)               │    │
│  │                                                            │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                    │
│  SETTINGS:                                                         │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                                                            │    │
│  │  UI Sound Volume: ████████████░░░░░░░░ 60%                 │    │
│  │                                                            │    │
│  │  Sound Theme: [▼ Classic NES]                              │    │
│  │  • Classic NES (authentic square waves)                    │    │
│  │  • Modern Soft (sine waves, gentler)                       │    │
│  │  • Arcade (brighter, more energetic)                       │    │
│  │  • Silent (no UI sounds)                                   │    │
│  │                                                            │    │
│  │  [✓] Navigation Sounds                                     │    │
│  │  [✓] Action Feedback                                       │    │
│  │  [✓] Achievement Fanfares                                  │    │
│  │  [ ] Boot Sound                                            │    │
│  │                                                            │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Sound Generation Code

```rust
/// NES-authentic sound generator for UI feedback
pub struct UiSoundGenerator {
    sample_rate: u32,
    phase: f32,
}

impl UiSoundGenerator {
    /// Generate a short NES-style blip sound
    pub fn menu_move(&mut self) -> Vec<f32> {
        self.generate_tone(440.0, 0.02, Waveform::Square, Envelope::Quick)
    }

    /// Generate a confirmation sound
    pub fn menu_select(&mut self) -> Vec<f32> {
        let mut samples = Vec::new();
        samples.extend(self.generate_tone(880.0, 0.05, Waveform::Square, Envelope::Attack));
        samples.extend(self.generate_tone(440.0, 0.05, Waveform::Square, Envelope::Decay));
        samples
    }

    /// Generate save state sound (power-up arpeggio)
    pub fn save_state(&mut self) -> Vec<f32> {
        let mut samples = Vec::new();
        // C-E-G arpeggio
        samples.extend(self.generate_tone(523.25, 0.05, Waveform::Square, Envelope::Quick));
        samples.extend(self.generate_tone(659.25, 0.05, Waveform::Square, Envelope::Quick));
        samples.extend(self.generate_tone(783.99, 0.05, Waveform::Square, Envelope::Sustain));
        samples
    }

    /// Generate achievement unlock fanfare
    pub fn achievement_unlock(&mut self) -> Vec<f32> {
        let mut samples = Vec::new();
        // Triumphant arpeggio with harmonics
        let notes = [523.25, 659.25, 783.99, 1046.5]; // C5, E5, G5, C6
        for (i, &freq) in notes.iter().enumerate() {
            let duration = if i == notes.len() - 1 { 0.2 } else { 0.08 };
            samples.extend(self.generate_tone(freq, duration, Waveform::Triangle, Envelope::Sustain));
        }
        samples
    }

    fn generate_tone(
        &mut self,
        frequency: f32,
        duration: f32,
        waveform: Waveform,
        envelope: Envelope,
    ) -> Vec<f32> {
        let num_samples = (self.sample_rate as f32 * duration) as usize;
        let mut samples = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            let t = i as f32 / self.sample_rate as f32;
            self.phase += frequency / self.sample_rate as f32;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }

            let wave = match waveform {
                Waveform::Square => if self.phase < 0.5 { 1.0 } else { -1.0 },
                Waveform::Triangle => 4.0 * (self.phase - 0.5).abs() - 1.0,
                Waveform::Noise => fastrand::f32() * 2.0 - 1.0,
            };

            let env = match envelope {
                Envelope::Quick => (1.0 - t / duration).max(0.0),
                Envelope::Attack => (t / duration * 4.0).min(1.0),
                Envelope::Decay => (1.0 - t / duration).powf(2.0),
                Envelope::Sustain => if t < duration * 0.1 { t / (duration * 0.1) }
                                     else if t > duration * 0.7 { (duration - t) / (duration * 0.3) }
                                     else { 1.0 },
            };

            samples.push(wave * env * 0.3); // 0.3 = master volume
        }

        samples
    }
}

#[derive(Clone, Copy)]
enum Waveform { Square, Triangle, Noise }

#[derive(Clone, Copy)]
enum Envelope { Quick, Attack, Decay, Sustain }
```

---

## Plugin & Extension System

### Plugin Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                      PLUGIN SYSTEM                                 │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  RustyNES supports a modular plugin system for extensibility:      │
│                                                                    │
│  PLUGIN TYPES:                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                                                            │    │
│  │  🎨 SHADER PLUGINS (.wgsl files)                           │    │
│  │     Custom post-processing shaders                         │    │
│  │     Hot-reloadable during development                      │    │
│  │                                                            │    │
│  │  🎮 INPUT PLUGINS (Lua or native)                          │    │
│  │     Custom input mappers                                   │    │
│  │     Accessibility input adapters                           │    │
│  │     Motion control mapping                                 │    │
│  │                                                            │    │
│  │  📊 METADATA SCRAPERS (Lua)                                │    │
│  │     Custom database sources                                │    │
│  │     Local file scanners                                    │    │
│  │     NFO file parsers                                       │    │
│  │                                                            │    │
│  │  ☁️ CLOUD SYNC PROVIDERS (native)                          │    │
│  │     Dropbox, Google Drive, OneDrive                        │    │
│  │     Custom servers (WebDAV, SFTP)                          │    │
│  │                                                            │    │
│  │  🎭 SOCIAL INTEGRATIONS (native)                           │    │
│  │     Discord Rich Presence                                  │    │
│  │     Twitch integration                                     │    │
│  │     Steam Deck gyro support                                │    │
│  │                                                            │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                    │
│  PLUGIN MANAGER UI:                                                │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                                                            │    │
│  │  INSTALLED PLUGINS (5)                                     │    │
│  │  ─────────────────────                                     │    │
│  │                                                            │    │
│  │  [✓] Discord Rich Presence          v1.2.0    [Configure]  │    │
│  │      Shows current game in Discord status                  │    │
│  │                                                            │    │
│  │  [✓] CRT-Royale Shader              v3.1.0    [Configure]  │    │
│  │      Advanced CRT simulation shader                        │    │
│  │                                                            │    │
│  │  [✓] IGDB Metadata Scraper          v1.0.0    [Configure]  │    │
│  │      Fetches game info from IGDB                           │    │
│  │                                                            │    │
│  │  [ ] Dropbox Cloud Sync             v0.9.0    [Configure]  │    │
│  │      Sync saves and states to Dropbox                      │    │
│  │                                                            │    │
│  │  [✓] Turbo/Autofire Input           v1.1.0    [Configure]  │    │
│  │      Adds turbo button functionality                       │    │
│  │                                                            │    │
│  │  ────────────────────────────────────────────────────────  │    │
│  │                                                            │    │
│  │  [📦 Browse Plugin Repository]  [📁 Install from File]      │    │
│  │                                                            │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Plugin API

```rust
/// Plugin trait that all plugins must implement
pub trait Plugin: Send + Sync {
    /// Plugin metadata
    fn info(&self) -> PluginInfo;

    /// Called when plugin is loaded
    fn on_load(&mut self, context: &PluginContext) -> Result<(), PluginError>;

    /// Called when plugin is unloaded
    fn on_unload(&mut self);

    /// Called each frame (optional)
    fn on_frame(&mut self, _frame: &FrameContext) {}

    /// Handle plugin-specific events
    fn on_event(&mut self, event: PluginEvent) -> Option<PluginResponse>;
}

/// Plugin information
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: Version,
    pub author: String,
    pub description: String,
    pub plugin_type: PluginType,
    pub config_schema: Option<ConfigSchema>,
}

/// Plugin types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginType {
    Shader,
    Input,
    MetadataScraper,
    CloudSync,
    Social,
    Other,
}

/// Context provided to plugins
pub struct PluginContext {
    /// Access to settings
    pub settings: Arc<RwLock<Settings>>,

    /// Access to library
    pub library: Arc<RwLock<Library>>,

    /// Event sender
    pub event_tx: mpsc::Sender<PluginEvent>,

    /// Plugin data directory
    pub data_dir: PathBuf,
}

/// Example: Discord Rich Presence Plugin
pub struct DiscordPlugin {
    client: Option<discord_rich_presence::DiscordIpcClient>,
    current_game: Option<String>,
}

impl Plugin for DiscordPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "discord-rpc".into(),
            name: "Discord Rich Presence".into(),
            version: Version::new(1, 2, 0),
            author: "RustyNES Team".into(),
            description: "Shows current game in Discord status".into(),
            plugin_type: PluginType::Social,
            config_schema: None,
        }
    }

    fn on_load(&mut self, _context: &PluginContext) -> Result<(), PluginError> {
        self.client = Some(DiscordIpcClient::new("YOUR_APP_ID")?);
        self.client.as_mut().unwrap().connect()?;
        Ok(())
    }

    fn on_event(&mut self, event: PluginEvent) -> Option<PluginResponse> {
        match event {
            PluginEvent::GameLoaded { title, .. } => {
                self.current_game = Some(title.clone());
                if let Some(ref mut client) = self.client {
                    let _ = client.set_activity(|a| a
                        .state("Playing")
                        .details(&title)
                        .assets(|a| a.large_image("rustynes-logo"))
                    );
                }
            }
            PluginEvent::GameClosed => {
                self.current_game = None;
                if let Some(ref mut client) = self.client {
                    let _ = client.clear_activity();
                }
            }
            _ => {}
        }
        None
    }

    fn on_unload(&mut self) {
        if let Some(ref mut client) = self.client {
            let _ = client.close();
        }
    }
}
```

---

## Accessibility & Inclusivity

### Extended Accessibility Features

```
┌────────────────────────────────────────────────────────────────────┐
│                    ACCESSIBILITY FEATURES                          │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  VISUAL ACCESSIBILITY                                              │
│  ────────────────────                                              │
│                                                                    │
│  • High Contrast Mode                                              │
│    - WCAG AAA compliance (7:1+ contrast)                           │
│    - Removes decorative elements                                   │
│    - Thicker focus indicators (4px)                                │
│                                                                    │
│  • Reduced Motion Mode                                             │
│    - Disables all animations                                       │
│    - Instant transitions                                           │
│    - No particle effects                                           │
│                                                                    │
│  • Font Scaling (100% - 300%)                                      │
│    - Respects system preferences                                   │
│    - UI scales proportionally                                      │
│    - HTPC mode defaults to 150%                                    │
│                                                                    │
│  • Colorblind Modes                                                │
│    - Deuteranopia (Red-Green)                                      │
│    - Protanopia (Red)                                              │
│    - Tritanopia (Blue-Yellow)                                      │
│    - Achromatopsia (Grayscale)                                     │
│    - Custom color remapping                                        │
│                                                                    │
│  • Screen Reader Support                                           │
│    - Full ARIA labels                                              │
│    - Live regions for dynamic content                              │
│    - Semantic navigation structure                                 │
│    - Game state announcements (TTS)                                │
│                                                                    │
│  MOTOR ACCESSIBILITY                                               │
│  ──────────────────                                                │
│                                                                    │
│  • Full Keyboard Navigation                                        │
│    - Tab order follows visual layout                               │
│    - Arrow keys for lists/grids                                    │
│    - Shortcuts for all major actions                               │
│                                                                    │
│  • Input Remapping                                                 │
│    - Any key/button to any action                                  │
│    - Multiple keys per action                                      │
│    - Turbo/Autofire support (1-60Hz)                               │
│    - Sticky keys support                                           │
│                                                                    │
│  • Adjustable Timing                                               │
│    - Key repeat delay: 100ms - 2000ms                              │
│    - Double-click speed adjustment                                 │
│    - Hold-to-confirm duration                                      │
│    - Slow-motion gameplay mode                                     │
│                                                                    │
│  • One-Handed Mode                                                 │
│    - Alternate control schemes                                     │
│    - D-pad emulation on analog stick                               │
│    - Sequential button combos                                      │
│                                                                    │
│  COGNITIVE ACCESSIBILITY                                           │
│  ───────────────────────                                           │
│                                                                    │
│  • Simplified UI Mode                                              │
│    - Hides advanced features                                       │
│    - Larger touch targets (48px minimum)                           │
│    - Clearer labeling                                              │
│    - Reduced option count                                          │
│                                                                    │
│  • Guided Tutorials                                                │
│    - First-use walkthroughs                                        │
│    - Contextual help (? icons)                                     │
│    - Video guides embedded                                         │
│                                                                    │
│  • Memory Aids                                                     │
│    - Recent actions history                                        │
│    - Bookmark system for games                                     │
│    - Notes per game                                                │
│                                                                    │
│  HEARING ACCESSIBILITY                                             │
│  ─────────────────────                                             │
│                                                                    │
│  • Visual Audio Cues                                               │
│    - Screen flash for important sounds                             │
│    - Haptic feedback for audio events                              │
│    - On-screen volume meters                                       │
│                                                                    │
│  • Subtitle System                                                 │
│    - Auto-generated for known games                                │
│    - Custom subtitle files (.srt)                                  │
│    - Adjustable size and background                                │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Accessibility Settings Panel

```
┌────────────────────────────────────────────────────────────────────────┐
│                   ACCESSIBILITY SETTINGS                               │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│  ┌───────────────────────────────────────────────────────────────────┐ │
│  │                                                                   │ │
│  │  QUICK PRESETS                                                    │ │
│  │  ─────────────                                                    │ │
│  │                                                                   │ │
│  │  [👁 Vision]  [🖐 Motor]  [🧠 Cognitive]  [👂 Hearing]  [⚙ Custom] │ │
│  │                                                                   │ │
│  │  ════════════════════════════════════════════════════════════     │ │
│  │                                                                   │ │
│  │  DISPLAY                                                          │ │
│  │  ───────                                                          │ │
│  │                                                                   │ │
│  │  UI Scale: [◀ 125% ▶]                                             │ │
│  │  ▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░                              │ │
│  │  100%                             300%                            │ │
│  │                                                                   │ │
│  │  [✓] High Contrast Mode                                           │ │
│  │  [✓] Reduce Motion                                                │ │
│  │  [ ] Large Cursor                                                 │ │
│  │                                                                   │ │
│  │  Colorblind Mode: [▼ Deuteranopia (Red-Green)]                    │ │
│  │                                                                   │ │
│  │  ────────────────────────────────────────────────────────────     │ │
│  │                                                                   │ │
│  │  INPUT                                                            │ │
│  │  ─────                                                            │ │
│  │                                                                   │ │
│  │  [✓] Sticky Keys (hold-to-toggle)                                 │ │
│  │  [ ] One-Handed Mode                                              │ │
│  │  [✓] Turbo Buttons Enabled                                        │ │
│  │                                                                   │ │
│  │  Key Repeat Delay: [◀ 500ms ▶]                                    │ │
│  │                                                                   │ │
│  │  Turbo Speed: [◀ 15 Hz ▶]                                         │ │
│  │                                                                   │ │
│  │  ────────────────────────────────────────────────────────────     │ │
│  │                                                                   │ │
│  │  AUDIO                                                            │ │
│  │  ─────                                                            │ │
│  │                                                                   │ │
│  │  [✓] Visual Audio Cues (screen flash)                             │ │
│  │  [✓] Haptic Feedback for Audio Events                             │ │
│  │  [ ] Text-to-Speech Narration                                     │ │
│  │                                                                   │ │
│  │  ────────────────────────────────────────────────────────────     │ │
│  │                                                                   │ │
│  │  GAMEPLAY ASSISTS                                                 │ │
│  │  ────────────────                                                 │ │
│  │                                                                   │ │
│  │  [ ] Slow Motion Mode (50% speed)                                 │ │
│  │  [✓] Unlimited Rewind                                             │ │
│  │  [ ] Auto-Save Every 60 Seconds                                   │ │
│  │                                                                   │ │
│  │  ════════════════════════════════════════════════════════════     │ │
│  │                                                                   │ │
│  │  [↻ Reset to Defaults]  [💾 Save Profile]  [📂 Load Profile]       │ │
│  │                                                                   │ │
│  └───────────────────────────────────────────────────────────────────┘ │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Performance Requirements

### Targets (Enhanced)

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PERFORMANCE TARGETS                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  FRAME TIMING                                                       │
│  ────────────                                                       │
│                                                                     │
│  │ Metric                    │ Target    │ Acceptable │             │
│  │───────────────────────────│───────────│────────────│             │
│  │ Frame time (60Hz)         │ 16.67ms   │ 16.67ms    │             │
│  │ Frame time variance       │ <0.5ms    │ <1.0ms     │             │
│  │ Input-to-display (no RA)  │ <25ms     │ <35ms      │             │
│  │ Input-to-display (RA=2)   │ <10ms     │ <15ms      │ ← NEW       │
│  │ UI interaction response   │ <8ms      │ <16ms      │             │
│  │ ROM load time             │ <100ms    │ <500ms     │             │
│  │ Save state (save)         │ <50ms     │ <100ms     │             │
│  │ Save state (load)         │ <30ms     │ <50ms      │             │
│  │ Run-ahead overhead        │ <200%     │ <300%      │ ← NEW       │
│                                                                     │
│  AUDIO                                                              │
│  ─────                                                              │
│                                                                     │
│  │ Metric                    │ Target    │ Acceptable │             │
│  │───────────────────────────│───────────│────────────│             │
│  │ Audio latency             │ <30ms     │ <50ms      │             │
│  │ Sample rate               │ 48kHz     │ 44.1kHz    │             │
│  │ Buffer underruns/hour     │ 0         │ <5         │             │
│  │ Dynamic rate control      │ ±2%       │ ±5%        │ ← NEW       │
│                                                                     │
│  MEMORY                                                             │
│  ──────                                                             │
│                                                                     │
│  │ Metric                    │ Target    │ Acceptable │             │
│  │───────────────────────────│───────────│────────────│             │
│  │ Base memory (no ROM)      │ <50MB     │ <100MB     │             │
│  │ Per-game overhead         │ <10MB     │ <25MB      │             │
│  │ Rewind buffer (60s)       │ <200MB    │ <500MB     │             │
│  │ Run-ahead state cache     │ <5MB      │ <10MB      │ ← NEW       │
│  │ Library (1000 ROMs)       │ <100MB    │ <200MB     │             │
│  │ CRT shader VRAM           │ <50MB     │ <100MB     │ ← NEW       │
│                                                                     │
│  CPU USAGE                                                          │
│  ─────────                                                          │
│                                                                     │
│  │ Scenario                  │ Target    │ Max        │             │
│  │───────────────────────────│───────────│────────────│             │
│  │ Idle (menu)               │ <5%       │ <10%       │             │
│  │ Gameplay (no RA)          │ <15%      │ <25%       │             │
│  │ Gameplay (RA=1)           │ <30%      │ <50%       │ ← NEW       │
│  │ Gameplay (RA=2)           │ <50%      │ <75%       │ ← NEW       │
│  │ Debugger active           │ <40%      │ <60%       │             │
│  │ Recording/streaming       │ <60%      │ <80%       │             │
│                                                                     │
│  MINIMUM HARDWARE                                                   │
│  ────────────────                                                   │
│                                                                     │
│  CPU:    Any x86-64 or ARM64 (2015+)                                │
│  RAM:    2 GB                                                       │
│  GPU:    OpenGL 3.3 / DirectX 11 / Metal 2.0 / Vulkan 1.0           │
│  Disk:   150 MB (application + shaders)                             │
│  OS:     Windows 10+, macOS 11+, Linux (glibc 2.31+)                │
│                                                                     │
│  RECOMMENDED HARDWARE (for full features)                           │
│  ─────────────────────────────────────────                          │
│                                                                     │
│  CPU:    4+ cores, 3.0 GHz+ (for Run-Ahead with RA=2+)              │
│  RAM:    4 GB                                                       │
│  GPU:    Discrete GPU with 2GB VRAM (for CRT-Royale)                │
│  Disk:   SSD recommended for fast state saves                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
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
│  [ ] Create rustynes-desktop crate structure                        │
│  [ ] Set up Iced application skeleton                               │
│  [ ] Implement custom title bar (Windows/Linux/macOS)               │
│  [ ] Create basic theme system with glass morphism                  │
│  [ ] Implement sidebar navigation                                   │
│  [ ] Set up wgpu render pipeline                                    │
│  [ ] Integrate game viewport with NES framebuffer                   │
│                                                                     │
│  WEEK 2: Core Playback + Latency Foundation                         │
│  ──────────────────────────────────────────                         │
│                                                                     │
│  [ ] Implement ROM loading via file dialog                          │
│  [ ] Create cpal audio pipeline with dynamic rate control           │
│  [ ] Set up input handling (keyboard + JIT polling)                 │
│  [ ] Implement basic play/pause/reset                               │
│  [ ] Add FPS counter and frame timing display                       │
│  [ ] Create quick menu overlay                                      │
│  [ ] Implement save states (UI only)                                │
│  [ ] Implement basic run-ahead (1 frame)                            │
│                                                                     │
│  DELIVERABLE: Playable emulator with basic UI and RA=1              │
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
│  [ ] Implement ROM library browser (Grid/List views)                │
│  [ ] Add box art loading (local + scraping)                         │
│  [ ] Create settings panel UI with all categories                   │
│  [ ] Implement video settings (scale, shaders basic)                │
│  [ ] Implement audio settings (volume, latency)                     │
│  [ ] Implement input settings (keyboard + gamepad mapping)          │
│  [ ] Add gamepad support (gilrs)                                    │
│  [ ] Persist configuration to TOML                                  │
│  [ ] Implement per-game profiles                                    │
│                                                                     │
│  WEEK 4: Visual Polish + Latency System                             │
│  ───────────────────────────────────────                            │
│                                                                     │
│  [ ] Implement CRT shader pipeline (5 presets)                      │
│  [ ] Add animation system                                           │
│  [ ] Create toast notification system                               │
│  [ ] Implement modal dialogs                                        │
│  [ ] Add loading states and progress indicators                     │
│  [ ] Polish transitions and micro-interactions                      │
│  [ ] Implement theme switching (light/dark/retro)                   │
│  [ ] Full run-ahead system (0-4 frames, auto-detect)                │
│  [ ] Frame delay system with auto-tuning                            │
│  [ ] Latency calibration wizard                                     │
│                                                                     │
│  DELIVERABLE: Polished MVP with full latency reduction              │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Phase 3: Advanced Features (Week 5-8)

```
┌─────────────────────────────────────────────────────────────────────┐
│                   PHASE 3: ADVANCED FEATURES                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  WEEK 5-6: HTPC Mode + Enhanced Shaders                             │
│  ──────────────────────────────────────                             │
│                                                                     │
│  [ ] Implement HTPC Controller-First mode                           │
│  [ ] Create Cover Flow view                                         │
│  [ ] Create Virtual Shelf view                                      │
│  [ ] Full CRT shader pipeline (12+ presets)                         │
│  [ ] Phosphor mask simulation (all types)                           │
│  [ ] NTSC composite simulation                                      │
│  [ ] Rolling scan mode for 120Hz+ displays                          │
│  [ ] HD Pack support                                                │
│  [ ] Automatic metadata scraping (IGDB)                             │
│  [ ] Haptic feedback system                                         │
│  [ ] UI sound design system                                         │
│                                                                     │
│  WEEK 7-8: Netplay, Achievements & Plugins                          │
│  ─────────────────────────────────────────                          │
│                                                                     │
│  [ ] Implement netplay lobby UI                                     │
│  [ ] Add session creation/joining flow                              │
│  [ ] Create in-game netplay overlay                                 │
│  [ ] Integrate RetroAchievements login                              │
│  [ ] Implement achievement browser                                  │
│  [ ] Add achievement unlock notifications                           │
│  [ ] Create leaderboard UI                                          │
│  [ ] Implement plugin system architecture                           │
│  [ ] Discord Rich Presence plugin                                   │
│  [ ] Cloud sync plugin (optional)                                   │
│                                                                     │
│  DELIVERABLE: Feature-complete with HTPC and plugins                │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Phase 4: Debug & TAS (Week 9-12)

```
┌─────────────────────────────────────────────────────────────────────┐
│                   PHASE 4: DEBUG & TAS TOOLS                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  WEEK 9-10: Debugger                                                │
│  ───────────────────                                                │
│                                                                     │
│  [ ] Implement debugger view (egui overlay)                         │
│  [ ] Add CPU/PPU/APU state viewers                                  │
│  [ ] Create memory hex editor                                       │
│  [ ] Implement breakpoint system UI                                 │
│  [ ] Add conditional breakpoints                                    │
│  [ ] Memory watch expressions                                       │
│  [ ] Trace logging to file                                          │
│  [ ] Run-ahead frame visualizer                                     │
│                                                                     │
│  WEEK 11-12: TAS & Accessibility                                    │
│  ───────────────────────────────                                    │
│                                                                     │
│  [ ] Add rewind timeline                                            │
│  [ ] Create TAS editor (piano roll)                                 │
│  [ ] Implement movie recording/playback                             │
│  [ ] Add Lua scripting console                                      │
│  [ ] Greenzone system                                               │
│  [ ] Branch management                                              │
│  [ ] Full accessibility audit and fixes                             │
│  [ ] Screen reader testing                                          │
│  [ ] One-handed mode polish                                         │
│                                                                     │
│  DELIVERABLE: Full-featured emulator exceeding Mesen2               │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Appendix A: Shader Examples

### Rolling Scan CRT Simulator (WGSL)

Based on the Blur Busters CRT Simulator technique for high-Hz displays:

```wgsl
// Rolling Scan CRT Simulator
// Achieves CRT-like motion clarity on 120Hz+ LCD/OLED displays
// Based on research from Blur Busters and Timothy Lottes

struct RollingScanUniforms {
    screen_size: vec2<f32>,
    simulated_refresh: f32,    // e.g., 60.0 for NES
    native_refresh: f32,       // e.g., 240.0 for the display
    time: f32,
    phosphor_persistence: f32, // 0.0-1.0, lower = sharper motion
    beam_height: f32,          // Visible portion of rolling scan
};

@group(0) @binding(0) var nes_texture: texture_2d<f32>;
@group(0) @binding(1) var nes_sampler: sampler;
@group(0) @binding(2) var<uniform> u: RollingScanUniforms;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // Calculate rolling scan position
    let frames_per_scan = u.native_refresh / u.simulated_refresh;
    let current_phase = fract(u.time * u.simulated_refresh);

    // Beam position (0.0 = top, 1.0 = bottom)
    let beam_y = current_phase;

    // Distance from current beam position
    let y_dist = abs(uv.y - beam_y);
    let wrapped_dist = min(y_dist, 1.0 - y_dist); // Handle wrap-around

    // Phosphor decay based on distance from beam
    let decay = exp(-wrapped_dist / (u.beam_height * u.phosphor_persistence));

    // Only show pixels that have been "scanned" this frame
    let in_beam = wrapped_dist < u.beam_height;
    let brightness = select(0.0, decay, in_beam);

    // Sample texture
    let color = textureSample(nes_texture, nes_sampler, uv).rgb;

    // Apply rolling scan brightness
    return vec4<f32>(color * brightness, 1.0);
}
```

---

## Appendix B: CLI Interface

RustyNES includes a command-line interface for automation and scripting:

```bash
# Basic usage
rustynes <rom_file>                    # Launch and play ROM
rustynes --headless <rom_file>         # Run without GUI (for testing)

# Configuration
rustynes --config <path>               # Use specific config file
rustynes --profile <name>              # Use named game profile
rustynes --run-ahead <0-4>             # Set run-ahead frames
rustynes --shader <preset>             # Set shader preset

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

---

## Conclusion

This enhanced UI/UX design specification (v2.0.0) provides a comprehensive blueprint for creating a **world-class NES emulator interface** that not only matches but surpasses existing solutions like Mesen, RetroArch, and FCEUX. Key improvements over v1.0.0 include:

**Revolutionary Latency Reduction:**
- Run-Ahead system achieving **lower latency than original NES hardware**
- Frame Delay auto-tuning
- Just-In-Time input polling
- Measurable sub-10ms input latency

**Professional CRT Simulation:**
- 12+ shader presets (CRT-Royale, Lottes, Guest, etc.)
- Accurate phosphor mask simulation
- Rolling scan mode for modern high-Hz displays
- NTSC composite signal simulation

**Living Room Ready:**
- Full HTPC Controller-First mode
- Cover Flow and Virtual Shelf views
- 10-foot UI scaling
- Haptic feedback and UI sound design

**Extensibility:**
- Plugin architecture for shaders, input, metadata, cloud sync
- Discord Rich Presence integration
- Per-game configuration profiles
- CLI for automation

By combining nostalgic aesthetics, modern UX patterns, sub-frame latency, and comprehensive accessibility, RustyNES v2.0 will deliver an experience that's not just functional, but genuinely **transformative** — setting a new standard for what emulator interfaces can achieve.

---

**Document Version:** 2.0.0  
**Author:** RustyNES Team  
**Status:** Enhanced Design Complete, Ready for Implementation
