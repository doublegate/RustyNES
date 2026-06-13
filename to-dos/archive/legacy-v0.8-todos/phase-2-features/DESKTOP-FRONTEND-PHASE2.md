# Phase 2: Desktop Frontend Feature Enhancement TODO

**Version:** 2.0.0
**Created:** 2025-12-27
**Last Updated:** 2025-12-28
**Status:** Planning
**Target Completion:** December 2026

---

## Table of Contents

- [Overview](#overview)
- [Technology Stack](#technology-stack)
- [egui 0.33 Features for Phase 2](#egui-033-features-for-phase-2)
- [Prerequisites](#prerequisites)
- [Current Implementation Status](#current-implementation-status)
- [Feature Roadmap](#feature-roadmap)
  - [1. Save States System](#1-save-states-system)
  - [2. Debugger Interface Enhancements](#2-debugger-interface-enhancements)
  - [3. RetroAchievements Integration](#3-retroachievements-integration)
  - [4. GGPO Netplay](#4-ggpo-netplay)
  - [5. TAS Tools](#5-tas-tools)
  - [6. Lua Scripting](#6-lua-scripting)
  - [7. Video Enhancements](#7-video-enhancements)
  - [8. Audio Enhancements](#8-audio-enhancements)
  - [9. Input Enhancements](#9-input-enhancements)
  - [10. Library Management](#10-library-management)
  - [11. Settings and Configuration](#11-settings-and-configuration)
  - [12. Quality of Life](#12-quality-of-life)
- [Technical Architecture](#technical-architecture)
- [Dependencies](#dependencies)
- [Testing Plan](#testing-plan)
- [Timeline Estimates](#timeline-estimates)
- [Risk Assessment](#risk-assessment)
- [References](#references)

---

## Overview

This document tracks all Phase 2 feature enhancement tasks for the RustyNES desktop frontend (`rustynes-desktop`). Phase 2 transforms the functional emulator into a feature-rich platform with advanced capabilities including RetroAchievements, GGPO netplay, TAS tools, Lua scripting, and comprehensive debugging.

### Design Philosophy

1. **Accuracy First**: All features must maintain cycle-accurate emulation
2. **User Experience**: Intuitive UI with minimal learning curve
3. **Performance**: Features should not degrade base emulation performance
4. **Modularity**: Features implemented as optional components where possible
5. **Cross-Platform**: All features must work on Linux, macOS, and Windows

---

## Technology Stack

### Current Stack (v0.7.1+)

| Component | Library | Version | Purpose |
|-----------|---------|---------|---------|
| GUI Framework | eframe + egui | 0.33 | Immediate mode GUI with window management |
| Rendering | glow (via eframe) | 0.16 | OpenGL backend |
| Framebuffer | egui textures | - | NES display via `egui::Image` |
| Audio | cpal | 0.16 | Low-latency audio with buffer underrun reporting |
| Input | gilrs | 0.11 | Gamepad support with hotplug detection |
| File Dialogs | rfd | 0.15 | Native file picker |
| Configuration | ron + serde | 0.12 | Settings persistence |
| Platform Paths | directories | 5.0 | Cross-platform config/data directories |
| CLI | clap | 4.5 | Command-line argument parsing |
| Rust Edition | 2024 | MSRV 1.88 | Latest Rust language features |

### Rendering Architecture

```text
egui 0.33 Immediate Mode Rendering
==================================

   User Input → egui::Context::begin_frame()
                        ↓
   UI Code → egui::Window, egui::CentralPanel, etc.
                        ↓
   egui::Context::end_frame() → egui::FullOutput
                        ↓
   eframe/glow → OpenGL rendering
                        ↓
   Frame displayed on screen
```

**Key Points:**
- **No wgpu**: Using glow (OpenGL) backend for simplicity
- **No pixels crate**: Framebuffer rendered directly as egui texture
- **Immediate Mode**: UI rebuilt every frame (no retained state complexity)

---

## egui 0.33 Features for Phase 2

egui 0.33 provides several features essential for Phase 2 development:

### Core Widgets

| Widget | Phase 2 Usage |
|--------|---------------|
| `egui::Window` | Debug windows, achievement toasts, dialogs |
| `egui::CentralPanel` | Game viewport container |
| `egui::SidePanel` | Debugger panels, script console |
| `egui::TopBottomPanel` | Menu bar, status bar |
| `egui::ScrollArea` | Disassembly, trace logs, memory hex view |
| `egui::Grid` | Memory editor hex dump |
| `egui::TextEdit` | Lua console, search boxes |
| `egui::plot::Plot` | APU waveforms, CPU timing graphs |

### New in egui 0.33

| Feature | Phase 2 Application |
|---------|---------------------|
| **Atoms** | Status indicators, debug labels, frame counters |
| **Plugin trait** | Organize debug windows as plugins |
| **Modal dialogs** | Confirmation dialogs, netplay connection |
| **kittest** | Automated UI testing for debug windows |
| **Improved text editing** | Lua console, memory editor |
| **Better tables** | `egui_extras::TableBuilder` for structured data |

### egui Patterns for Phase 2

```rust
// Achievement toast notification (egui 0.33)
fn show_achievement_toast(ctx: &egui::Context, achievement: &Achievement) {
    egui::Window::new("Achievement Unlocked!")
        .anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.image(&achievement.icon);
                ui.vertical(|ui| {
                    ui.strong(&achievement.title);
                    ui.label(&achievement.description);
                });
            });
        });
}

// Memory hex editor using egui::Grid
fn memory_editor(ctx: &egui::Context, memory: &mut [u8]) {
    egui::Window::new("Memory Editor").show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("hex_grid")
                .num_columns(17) // address + 16 bytes
                .striped(true)
                .show(ui, |ui| {
                    for (addr, chunk) in memory.chunks_mut(16).enumerate() {
                        ui.monospace(format!("{:04X}:", addr * 16));
                        for byte in chunk.iter_mut() {
                            let mut text = format!("{:02X}", *byte);
                            if ui.text_edit_singleline(&mut text).changed() {
                                if let Ok(val) = u8::from_str_radix(&text, 16) {
                                    *byte = val;
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    });
}

// Debug window organization using Plugin pattern (egui 0.33)
trait DebugWindow {
    fn name(&self) -> &'static str;
    fn show(&mut self, ctx: &egui::Context, console: &Console);
}

struct CpuDebugger { /* ... */ }
struct PpuViewer { /* ... */ }
struct ApuViewer { /* ... */ }
struct MemoryEditor { /* ... */ }

impl DebugWindow for CpuDebugger {
    fn name(&self) -> &'static str { "CPU Debugger" }
    fn show(&mut self, ctx: &egui::Context, console: &Console) {
        egui::Window::new(self.name()).show(ctx, |ui| {
            // CPU debugger UI...
        });
    }
}
```

---

## Prerequisites

Before beginning Phase 2 development, the following must be complete:

### Phase 1.5 Stabilization (Required)

- [x] M7: Accuracy Improvements (v0.6.0) - Complete
- [x] M8: Test ROM Validation (v0.7.0) - 100% Blargg pass rate (90/90 tests)
- [x] GUI Migration: eframe/egui rewrite (v0.7.1) - Complete
- [ ] M9: Known Issues Resolution (audio quality, PPU edge cases)
- [ ] M10: Documentation and v1.0-alpha preparation

### Core Requirements

- [x] Cycle-accurate CPU (all 256 opcodes)
- [x] Dot-accurate PPU rendering
- [x] Frame-accurate APU timing
- [x] 5 core mappers (0-4) working
- [x] 100% Blargg test pass rate (90/90 tests)
- [x] eframe 0.33 + egui 0.33 desktop frontend
- [x] cpal 0.16 audio with buffer underrun detection
- [ ] Save state serialization framework
- [ ] Deterministic emulation validation

### Technical Prerequisites

- [x] Rust 2024 Edition stable toolchain (MSRV 1.88)
- [ ] Cross-platform build verification (Linux, macOS, Windows)
- [ ] CI/CD pipeline for automated testing
- [ ] Performance baseline established (target: 120+ FPS)

---

## Current Implementation Status

| Feature | Status | Notes |
|---------|--------|-------|
| ROM Loading | Complete | iNES format, drag-and-drop |
| Display | Complete | 256x240 framebuffer, scaling modes |
| Audio | Complete | cpal ring buffer, volume control |
| Input | Complete | Keyboard + gamepad (gilrs) |
| Menu System | Complete | File, Emulation, Video, Audio, Debug, Help |
| Basic Debug Windows | Partial | CPU/PPU/APU state, memory viewer |
| Configuration | Complete | RON persistence, platform paths |
| Save States | Not Started | Framework defined, not implemented |
| Debugger | Basic | Needs enhancement for Phase 2 |
| RetroAchievements | Not Started | Planned for Phase 2 |
| Netplay | Not Started | Planned for Phase 2 |
| TAS Tools | Not Started | Planned for Phase 2 |
| Lua Scripting | Not Started | Planned for Phase 2 |

---

## Feature Roadmap

### 1. Save States System

**Priority:** Critical (blocks Netplay, TAS, Rewind)
**Estimated Effort:** 2-3 weeks
**Dependencies:** rustynes-core serialization

#### 1.1 Core Serialization

- [ ] Design save state format specification
  - [ ] Version header for forward compatibility
  - [ ] Component checksums for integrity
  - [ ] Compression support (zstd)
- [ ] Implement `Serialize`/`Deserialize` for CPU state
  - [ ] All registers (A, X, Y, SP, PC, P)
  - [ ] Cycle counter
  - [ ] Interrupt state (pending NMI, IRQ)
- [ ] Implement serialization for PPU state
  - [ ] Internal registers (v, t, x, w)
  - [ ] OAM (256 bytes)
  - [ ] Secondary OAM (32 bytes)
  - [ ] Palette RAM (32 bytes)
  - [ ] Framebuffer state
  - [ ] Scanline/dot position
- [ ] Implement serialization for APU state
  - [ ] All channel registers and internal state
  - [ ] Frame sequencer position
  - [ ] DMC sample buffer
  - [ ] Mixer state
- [ ] Implement serialization for Bus state
  - [ ] RAM (2KB)
  - [ ] VRAM/nametables
  - [ ] Controller state
- [ ] Implement serialization for Mapper state
  - [ ] Bank registers
  - [ ] IRQ counters
  - [ ] PRG/CHR RAM

**Technical Notes:**
- Use `bincode` or `postcard` for compact binary serialization
- Target save state size: <64KB for most games
- Reference: `docs/api/SAVE_STATES.md`

#### 1.2 Quick Save/Load

- [ ] Implement quick save (F5 default)
- [ ] Implement quick load (F8 default)
- [ ] Add visual feedback (toast notification)
- [ ] Handle load failures gracefully
- [ ] Verify state integrity on load

#### 1.3 Save State Slots

- [ ] Implement 10 save slots per game
- [ ] Slot selection UI (egui window)
- [ ] Slot preview with metadata
  - [ ] Timestamp
  - [ ] Play time
  - [ ] Thumbnail (downscaled)
- [ ] Auto-save to designated slot
- [ ] Slot management (delete, rename, export)

#### 1.4 State Thumbnails

- [ ] Capture framebuffer on save
- [ ] Downscale to 64x60 or 128x120
- [ ] Store as PNG in state file
- [ ] Display in slot selection UI

#### 1.5 Rewind Feature

- [ ] Ring buffer of recent states (configurable depth)
- [ ] Hold-to-rewind input binding
- [ ] Visual rewind indicator
- [ ] Frame-by-frame stepping during rewind
- [ ] Performance optimization (delta states)

**Acceptance Criteria:**
- [ ] Save/load completes in <50ms
- [ ] Loaded state produces identical output to original
- [ ] Works with all 5 core mappers
- [ ] No memory leaks during rapid save/load

---

### 2. Debugger Interface Enhancements

**Priority:** High
**Estimated Effort:** 4-5 weeks
**Dependencies:** egui overlay system, rustynes-core debug hooks

#### 2.1 CPU Debugger

- [ ] Disassembly view
  - [ ] Show current instruction at PC
  - [ ] Configurable context lines (before/after)
  - [ ] Syntax highlighting
  - [ ] Address labels (NMI, RESET, IRQ vectors)
- [ ] Breakpoint system
  - [ ] Execute breakpoints (by address)
  - [ ] Read/write watchpoints
  - [ ] Conditional breakpoints (register value)
  - [ ] Breakpoint list management
- [ ] Stepping controls
  - [ ] Step Into (single instruction)
  - [ ] Step Over (skip JSR)
  - [ ] Step Out (return from subroutine)
  - [ ] Run to cursor
- [ ] Register editing
  - [ ] Click-to-edit A, X, Y, SP, PC
  - [ ] Flag toggle buttons (N, V, B, D, I, Z, C)
- [ ] Cycle counter display
- [ ] Instruction history/trace

**Technical Notes:**
- Reference: FCEUX debugger for UI patterns
- Use `rustynes-cpu` internal state access
- Consider instruction caching for disassembly performance

#### 2.2 PPU Debugger

- [ ] Pattern table viewer
  - [ ] Both tables (256 tiles each)
  - [ ] Palette selection overlay
  - [ ] Tile click-to-select
  - [ ] Grid toggle
- [ ] Nametable viewer
  - [ ] All 4 nametables (real or mirrored)
  - [ ] Attribute grid overlay
  - [ ] Scroll position indicator
  - [ ] Tile info on hover
- [ ] Sprite/OAM viewer
  - [ ] 64 sprite list with attributes
  - [ ] Visual sprite preview
  - [ ] Highlight on-screen sprites
  - [ ] Sprite 0 indicator
- [ ] Palette viewer
  - [ ] Background palettes (4)
  - [ ] Sprite palettes (4)
  - [ ] Color picker/editor
  - [ ] Hex value display
- [ ] Scanline/dot position display
- [ ] PPU register state (PPUCTRL, PPUMASK, etc.)

#### 2.3 APU Debugger

- [ ] Channel visualization
  - [ ] Square 1 & 2 waveforms
  - [ ] Triangle waveform
  - [ ] Noise visualization
  - [ ] DMC sample buffer
- [ ] Volume meters per channel
- [ ] Frequency display
- [ ] Envelope visualization
- [ ] Length counter state
- [ ] Frame sequencer position
- [ ] Channel mute/solo toggles
- [ ] Mixer output visualization

#### 2.4 Memory Viewer/Editor

- [ ] Hex dump display
  - [ ] Configurable columns (8/16/32)
  - [ ] ASCII sidebar
  - [ ] Address ranges (CPU $0000-$FFFF, PPU $0000-$3FFF)
- [ ] Address navigation
  - [ ] Go to address input
  - [ ] Bookmark addresses
  - [ ] History (back/forward)
- [ ] Search functionality
  - [ ] Byte sequence search
  - [ ] Relative value search (cheat finding)
  - [ ] Previous value comparison
- [ ] Memory editing
  - [ ] Click-to-edit bytes
  - [ ] Paste hex data
  - [ ] Fill region
- [ ] Memory regions highlighting
  - [ ] Zero page
  - [ ] Stack
  - [ ] RAM
  - [ ] PPU registers
  - [ ] APU registers
  - [ ] Mapper registers
  - [ ] PRG ROM

#### 2.5 Trace Logger

- [ ] Instruction trace output
  - [ ] Address, opcode, operands
  - [ ] Register state after execution
  - [ ] Cycle count
- [ ] Configurable logging scope
  - [ ] Start/stop addresses
  - [ ] Instruction count limit
- [ ] Log to file export
- [ ] Real-time display (scrolling)
- [ ] Filter by instruction type
- [ ] nestest.log format compatibility

#### 2.6 Code-Data Logger (CDL)

- [ ] Track code vs data access
- [ ] Visualize coverage (heat map)
- [ ] Export CDL file (fceux format)
- [ ] Import existing CDL
- [ ] Use for informed disassembly

**Acceptance Criteria:**
- [ ] Breakpoints halt execution reliably
- [ ] Memory edits reflect immediately
- [ ] PPU viewers update at frame rate
- [ ] Trace logger handles 60fps without lag
- [ ] All debug windows can be docked/undocked

---

### 3. RetroAchievements Integration

**Priority:** High
**Estimated Effort:** 3-4 weeks
**Dependencies:** rcheevos FFI bindings, rustynes-achievements crate

**Reference:** `to-dos/phase-2-features/milestone-7-achievements/README.md`

#### 3.1 rcheevos FFI Bindings

- [ ] Create `rustynes-achievements` crate
- [ ] rcheevos-sys bindgen setup
- [ ] Safe Rust wrapper API
- [ ] Memory accessor callback implementation
- [ ] Error handling for FFI boundary

**Technical Notes:**
- Reference: RetroArch's rcheevos integration
- Requires C compiler for build
- Link statically for distribution

#### 3.2 Authentication

- [ ] Login dialog (username/password)
- [ ] Token storage (secure, platform-specific)
- [ ] Session management
- [ ] Logout functionality
- [ ] Profile display (username, points, rank)

#### 3.3 Achievement Detection

- [ ] Memory polling at frame end
- [ ] Trigger evaluation
- [ ] Achievement unlock events
- [ ] Progress indicators (measured achievements)
- [ ] Leaderboard tracking

#### 3.4 UI Integration

- [ ] Achievement unlock toast
  - [ ] Icon, title, description
  - [ ] Sound effect (optional)
  - [ ] Duration configurable
- [ ] Achievement list window
  - [ ] Game achievements display
  - [ ] Locked/unlocked state
  - [ ] Progress for measured
  - [ ] Point values
- [ ] Game info panel
  - [ ] Box art display
  - [ ] Developer/publisher
  - [ ] Achievement set info
- [ ] Leaderboard submission UI

#### 3.5 Hardcore Mode

- [ ] Disable save states
- [ ] Disable cheats
- [ ] Disable rewind
- [ ] Disable slow-motion
- [ ] Badge indicator in UI

**Acceptance Criteria:**
- [ ] Achievements unlock correctly in 10+ test games
- [ ] No false positives or negatives
- [ ] <1% performance impact
- [ ] Leaderboard submissions work
- [ ] Login persists across sessions

---

### 4. GGPO Netplay

**Priority:** High
**Estimated Effort:** 5-6 weeks
**Dependencies:** backroll-rs, save state serialization, deterministic emulation

**Reference:** `to-dos/phase-2-features/milestone-8-netplay/README.md`

#### 4.1 backroll-rs Integration

- [ ] Create `rustynes-netplay` crate
- [ ] Implement backroll-rs `GameState` trait
- [ ] Input serialization (compact format)
- [ ] State serialization for rollback
- [ ] Frame advance callback

**Technical Notes:**
- backroll-rs is Rust port of GGPO
- Requires deterministic emulation (byte-perfect replay)
- Target: <5 frame rollback at 100ms latency

#### 4.2 Determinism Validation

- [ ] Input recording/playback test
- [ ] Multi-frame checksum verification
- [ ] Floating-point elimination audit
- [ ] Random state seeding control
- [ ] Side-effect audit (timing, RNG)

#### 4.3 Network Layer

- [ ] UDP socket management
- [ ] NAT traversal (STUN/TURN)
  - [ ] STUN server integration
  - [ ] TURN fallback for strict NAT
- [ ] Hole punching implementation
- [ ] Connection quality monitoring
  - [ ] Ping display
  - [ ] Packet loss indicator
  - [ ] Rollback frame counter

#### 4.4 Lobby System

- [ ] Host game option
- [ ] Join by code/IP
- [ ] Room browser (optional central server)
- [ ] Player ready state
- [ ] Chat functionality
- [ ] Spectator slots

#### 4.5 In-Game UI

- [ ] Connection status overlay
- [ ] Ping display (per player)
- [ ] Rollback frame counter
- [ ] Desync detection warning
- [ ] Disconnect handling
- [ ] Input delay configuration

#### 4.6 Spectator Mode

- [ ] Join as spectator
- [ ] Delayed stream (anti-cheat)
- [ ] Multiple spectator support
- [ ] Spectator chat

**Acceptance Criteria:**
- [ ] 1-2 frame input lag over LAN
- [ ] <5 frame rollback at 100ms ping
- [ ] No desyncs in 30-minute sessions
- [ ] Works behind typical NAT setups
- [ ] Graceful disconnect handling

---

### 5. TAS Tools

**Priority:** Medium-High
**Estimated Effort:** 4-5 weeks
**Dependencies:** Save states, input recording, deterministic emulation

**Reference:** `to-dos/phase-2-features/milestone-9-scripting/README.md` (related)

#### 5.1 FM2 Movie Format

- [ ] FM2 file parsing
  - [ ] Header parsing (ROM info, rerecord count)
  - [ ] Input log parsing
  - [ ] Subtitle support
- [ ] FM2 file writing
- [ ] Movie metadata editing
- [ ] ROM hash verification

**Reference:** `docs/formats/FM2_FORMAT.md`

#### 5.2 Recording Mode

- [ ] Start recording from power-on
- [ ] Start recording from save state
- [ ] Append to existing movie
- [ ] Recording indicator UI
- [ ] Frame counter display

#### 5.3 Playback Mode

- [ ] Load and play FM2 movies
- [ ] Playback controls
  - [ ] Play/Pause
  - [ ] Frame advance
  - [ ] Fast forward
  - [ ] Speed control (0.5x, 2x, 4x)
- [ ] Progress display (current/total frames)
- [ ] Playback complete handling

#### 5.4 Re-recording

- [ ] Take control during playback
- [ ] Rerecord count tracking
- [ ] Branch from any point
- [ ] Truncate future input on edit
- [ ] Undo last input change

#### 5.5 Greenzone (State History)

- [ ] Automatic periodic state saves
- [ ] Configurable interval
- [ ] Memory-efficient delta compression
- [ ] Scrub to any frame
- [ ] Visual timeline UI

#### 5.6 Input Display

- [ ] On-screen input overlay
- [ ] Controller visualization
- [ ] Input history (last N frames)
- [ ] Configurable position/style

**Acceptance Criteria:**
- [ ] FM2 files from FCEUX play correctly
- [ ] Exported FM2 files play in FCEUX
- [ ] Re-recording maintains sync
- [ ] Greenzone scrubbing is <100ms latency
- [ ] TAS playback is deterministic across runs

---

### 6. Lua Scripting

**Priority:** Medium
**Estimated Effort:** 4-5 weeks
**Dependencies:** mlua crate, memory access API

**Reference:** `to-dos/phase-2-features/milestone-9-scripting/README.md`

#### 6.1 mlua Integration

- [ ] mlua 5.4 dependency setup
- [ ] Lua state management
- [ ] Script loading from file
- [ ] Script reload hot-key
- [ ] Error handling with line numbers

#### 6.2 Memory API

- [ ] `memory.readbyte(addr)` - CPU address space
- [ ] `memory.writebyte(addr, value)`
- [ ] `memory.readword(addr)` - little-endian 16-bit
- [ ] `memory.writeword(addr, value)`
- [ ] `memory.readbyterange(addr, length)` - returns table
- [ ] `ppu.readbyte(addr)` - PPU address space
- [ ] `rom.readbyte(addr)` - PRG ROM direct access

#### 6.3 Callback Hooks

- [ ] `emu.frameadvance()` - called each frame
- [ ] `emu.registerbefore(func)` - before frame
- [ ] `emu.registerafter(func)` - after frame
- [ ] `emu.registerexecute(addr, func)` - on CPU execute
- [ ] `emu.registerread(addr, func)` - on memory read
- [ ] `emu.registerwrite(addr, func)` - on memory write
- [ ] `emu.registerscanline(func)` - per scanline
- [ ] Callback removal API

#### 6.4 Input Functions

- [ ] `joypad.read(player)` - get current input
- [ ] `joypad.set(player, buttons)` - override input
- [ ] `input.get()` - raw keyboard state

#### 6.5 Drawing API

- [ ] `gui.text(x, y, message, [color])` - draw text
- [ ] `gui.pixel(x, y, color)` - single pixel
- [ ] `gui.line(x1, y1, x2, y2, color)` - line
- [ ] `gui.box(x1, y1, x2, y2, [fill], [outline])` - rectangle
- [ ] `gui.drawimage(x, y, filename)` - image overlay
- [ ] `gui.transparency(alpha)` - overlay alpha
- [ ] Color format: RGB hex or RGBA table

**Technical Notes:**
- Drawing renders to overlay layer, not NES framebuffer
- Performance target: <5% overhead with typical scripts
- Reference: FCEUX Lua API for compatibility

#### 6.6 Emulation Control

- [ ] `emu.pause()` / `emu.unpause()`
- [ ] `emu.speedmode(mode)` - normal, turbo, max
- [ ] `emu.framecount()` - get current frame
- [ ] `emu.lagcount()` - lagged frames
- [ ] `savestate.save(slot)` / `savestate.load(slot)`
- [ ] `movie.framecount()` - if movie active

#### 6.7 Script Manager UI

- [ ] Script list panel
- [ ] Enable/disable scripts
- [ ] Script output console
- [ ] Recent scripts list
- [ ] Script error display

#### 6.8 Example Scripts

- [ ] Hitbox viewer (SMB, Mega Man)
- [ ] RAM watch display
- [ ] Bot AI example
- [ ] Cheat script template
- [ ] Input display customization

**Acceptance Criteria:**
- [ ] FCEUX-compatible scripts mostly work
- [ ] Memory read/write at 60 Hz stable
- [ ] Drawing primitives render correctly
- [ ] <5% performance overhead typical scripts
- [ ] Error messages are informative

---

### 7. Video Enhancements

**Priority:** Medium
**Estimated Effort:** 2-3 weeks
**Dependencies:** OpenGL shader support via glow

#### 7.1 Shader Effects

- [ ] Shader pipeline setup (glow/OpenGL)
- [ ] Built-in shaders:
  - [ ] CRT curvature
  - [ ] Scanlines
  - [ ] Phosphor glow
  - [ ] NTSC artifact simulation
  - [ ] Color correction (warm/cool)
- [ ] Shader parameter UI (egui sliders)
- [ ] Custom shader loading (.glsl)

**Technical Notes:**
- eframe uses glow (OpenGL) backend, not wgpu
- Shaders written in GLSL, not WGSL
- egui texture rendering with post-processing pipeline
- Reference: RetroArch shader specs for algorithm ideas

#### 7.2 Integer Scaling

- [ ] Exact 2x, 3x, 4x, etc. scaling
- [ ] Black bars for non-integer displays
- [ ] Maintain aspect ratio option
- [ ] Fullscreen integer scaling

#### 7.3 Aspect Ratio Options

- [ ] 8:7 pixel perfect (NES hardware)
- [ ] 4:3 TV aspect ratio
- [ ] 16:9 widescreen stretch
- [ ] Custom aspect ratio input

#### 7.4 Screenshot Capture

- [ ] Capture current frame (PNG)
- [ ] Configurable output directory
- [ ] Filename pattern (game-date-time)
- [ ] Hotkey binding
- [ ] Optional: with overlay graphics
- [ ] Optional: without shader effects

#### 7.5 Video Recording

- [ ] Record gameplay to video file
- [ ] Format options (MP4, WebM)
- [ ] Quality settings
- [ ] Include audio track
- [ ] Start/stop hotkeys
- [ ] Recording indicator

**Acceptance Criteria:**
- [ ] Shaders don't affect emulation performance
- [ ] Integer scaling is pixel-perfect
- [ ] Screenshots are correct size/format
- [ ] Video recording doesn't drop frames

---

### 8. Audio Enhancements

**Priority:** Medium
**Estimated Effort:** 2 weeks
**Dependencies:** cpal 0.16 audio backend

**cpal 0.16 Features Available:**
- Buffer underrun detection and reporting
- Improved device enumeration
- Better error handling for audio device changes
- Enhanced callback timing precision

#### 8.1 Channel Mixer

- [ ] Per-channel volume sliders
  - [ ] Square 1
  - [ ] Square 2
  - [ ] Triangle
  - [ ] Noise
  - [ ] DMC
- [ ] Per-channel mute toggles
- [ ] Master volume control
- [ ] Balance (stereo panning simulation)

#### 8.2 Audio Recording

- [ ] Record audio to WAV file
- [ ] Sample rate options
- [ ] Bit depth options
- [ ] Start/stop hotkeys
- [ ] Sync with video recording

#### 8.3 NSF Player Mode

- [ ] Load NSF files directly
- [ ] Track selection UI
- [ ] Playback controls
- [ ] Visualizer display
- [ ] Playlist support

**Reference:** `docs/formats/NSF_FORMAT.md`

#### 8.4 Audio Visualization

- [ ] Waveform display (oscilloscope)
- [ ] Spectrum analyzer
- [ ] Per-channel visualization
- [ ] Configurable colors/style

#### 8.5 Dynamic Resampling

- [ ] Support non-44.1kHz output devices
- [ ] High-quality resampling (libsamplerate or rubato)
- [ ] Latency adjustment

**Acceptance Criteria:**
- [ ] Mixer changes don't introduce artifacts
- [ ] Audio recording is in sync
- [ ] NSF playback matches native NES
- [ ] Visualization updates smoothly

---

### 9. Input Enhancements

**Priority:** Medium
**Estimated Effort:** 2-3 weeks
**Dependencies:** gilrs gamepad library

#### 9.1 Input Configuration UI

- [ ] Per-player input mapping
- [ ] Keyboard binding dialog
- [ ] Gamepad button mapping
- [ ] Analog stick configuration
  - [ ] Deadzone adjustment
  - [ ] D-pad threshold
- [ ] Input test mode (display pressed buttons)

#### 9.2 Multiple Controller Profiles

- [ ] Save/load input profiles
- [ ] Per-game profile association
- [ ] Profile quick-switch hotkey
- [ ] Import/export profiles

#### 9.3 Turbo Button Support

- [ ] Configurable turbo rate (Hz)
- [ ] Per-button turbo toggle
- [ ] Turbo indicator display
- [ ] Auto-fire patterns

#### 9.4 Input Macros

- [ ] Record button sequences
- [ ] Playback on hotkey
- [ ] Macro editor UI
- [ ] Import/export macros

#### 9.5 Additional Controllers

- [ ] Zapper (light gun) simulation
  - [ ] Mouse-based aiming
  - [ ] Click-to-shoot
- [ ] Arkanoid paddle (mouse horizontal)
- [ ] Power Pad (keyboard grid)

**Acceptance Criteria:**
- [ ] All standard gamepads detected
- [ ] Input latency <1 frame
- [ ] Turbo rate is accurate
- [ ] Zapper works with Duck Hunt

---

### 10. Library Management

**Priority:** Low-Medium
**Estimated Effort:** 2-3 weeks
**Dependencies:** ROM scanning, metadata sources

#### 10.1 ROM Library Scanner

- [ ] Scan directories for ROM files
- [ ] Recursive scanning option
- [ ] File type filtering (.nes, .zip)
- [ ] Scan progress indicator

#### 10.2 Game Database

- [ ] Store scanned ROM metadata
  - [ ] Filename, path, size
  - [ ] CRC32/MD5/SHA1 hashes
  - [ ] iNES header info
- [ ] Database file (SQLite or RON)
- [ ] Incremental updates

#### 10.3 Cover Art

- [ ] Scrape from online sources (libretro DB)
- [ ] Local image support
- [ ] Thumbnail generation
- [ ] Cover art display in library

#### 10.4 Library Browser UI

- [ ] Grid view (cover art)
- [ ] List view (detailed)
- [ ] Sort options (name, date, playtime)
- [ ] Filter by mapper, year, genre
- [ ] Search functionality

#### 10.5 Collections

- [ ] Favorites list
- [ ] Custom collections
- [ ] Recently played
- [ ] Play history with stats

**Acceptance Criteria:**
- [ ] Scans 1000+ ROMs in <10 seconds
- [ ] Database persists across sessions
- [ ] UI responsive with large library
- [ ] Cover art loads asynchronously

---

### 11. Settings and Configuration

**Priority:** Medium
**Estimated Effort:** 1-2 weeks
**Dependencies:** RON configuration system

#### 11.1 Hotkey Configuration

- [ ] Rebindable hotkeys for all actions
- [ ] Hotkey conflict detection
- [ ] Default hotkey reset
- [ ] Hotkey reference display

#### 11.2 Configuration Profiles

- [ ] Multiple named profiles
- [ ] Quick profile switching
- [ ] Profile import/export
- [ ] Reset to defaults

#### 11.3 Per-Game Settings

- [ ] Override global settings per ROM
- [ ] Video settings override
- [ ] Audio settings override
- [ ] Input profile override
- [ ] Settings stored by ROM hash

#### 11.4 Settings Categories

- [ ] Video settings panel
- [ ] Audio settings panel
- [ ] Input settings panel
- [ ] Emulation settings (region, timing)
- [ ] Advanced/developer settings
- [ ] Paths configuration

#### 11.5 Settings Persistence

- [ ] Auto-save on change
- [ ] Settings migration (version upgrades)
- [ ] Backup/restore
- [ ] Factory reset option

**Acceptance Criteria:**
- [ ] All settings persist correctly
- [ ] Per-game overrides work
- [ ] Settings UI is intuitive
- [ ] Migration handles version changes

---

### 12. Quality of Life

**Priority:** Medium
**Estimated Effort:** 2-3 weeks
**Dependencies:** Various

#### 12.1 Speed Controls

- [ ] Fast forward (hold button)
- [ ] Slow motion (0.5x, 0.25x)
- [ ] Frame advance (single frame step)
- [ ] Speed indicator display
- [ ] Turbo mode (uncapped, max speed)

#### 12.2 Game Cheats

- [ ] Game Genie code support
- [ ] Raw address cheat codes
- [ ] Cheat code database
- [ ] Cheat search (RAM compare)
- [ ] Cheat enable/disable toggle

**Reference:** `docs/features/CHEATS.md` (if exists)

#### 12.3 Pause Menu Overlay

- [ ] In-game pause overlay
- [ ] Quick save/load buttons
- [ ] Settings access
- [ ] Exit to menu

#### 12.4 Window Behavior

- [ ] Remember window size/position
- [ ] Multi-monitor support
- [ ] Always-on-top option
- [ ] Borderless fullscreen
- [ ] Alt+Enter fullscreen toggle

#### 12.5 Accessibility

- [ ] Color blind modes
- [ ] High contrast UI theme
- [ ] Font size options
- [ ] Screen reader support (basic)
- [ ] Keyboard-only navigation

#### 12.6 System Tray

- [ ] Minimize to tray (optional)
- [ ] Tray quick actions
- [ ] Notification support

**Acceptance Criteria:**
- [ ] Speed controls don't affect audio pitch
- [ ] Cheats work without save state dependency
- [ ] Window state persists correctly
- [ ] Accessibility modes are usable

---

## Technical Architecture

### eframe/egui Application Model

```text
eframe Application Lifecycle (egui 0.33)
========================================

┌─────────────────────────────────────────────────────────────────┐
│                    eframe::App trait                            │
│                                                                 │
│  fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  1. Handle input (keyboard, gamepad, mouse)             │    │
│  │  2. Update emulation state (run frame if playing)       │    │
│  │  3. Render egui UI:                                     │    │
│  │     - Menu bar (egui::TopBottomPanel)                   │    │
│  │     - Game viewport (egui::CentralPanel + Image)        │    │
│  │     - Debug windows (egui::Window)                      │    │
│  │     - Overlays (input display, achievements)            │    │
│  │  4. Request repaint for next frame                      │    │
│  └─────────────────────────────────────────────────────────┘    │
│                                                                 │
│  Frame timing: eframe handles vsync/60Hz, no manual loop needed│
└─────────────────────────────────────────────────────────────────┘
```

### Proposed Crate Structure

```text
crates/
├── rustynes-core/          # Core emulation (existing)
│   └── save_state.rs       # NEW: serialization framework
├── rustynes-desktop/       # Desktop frontend (eframe 0.33)
│   ├── src/
│   │   ├── main.rs         # Entry point, eframe::run_native()
│   │   ├── app.rs          # eframe::App implementation
│   │   ├── emulator.rs     # Emulator wrapper, frame timing
│   │   ├── gui/            # egui UI components
│   │   │   ├── mod.rs
│   │   │   ├── menu.rs     # egui::menu::bar()
│   │   │   ├── viewport.rs # Game display (egui::Image)
│   │   │   ├── debug/      # Debug windows (egui::Window)
│   │   │   │   ├── mod.rs
│   │   │   │   ├── cpu.rs
│   │   │   │   ├── ppu.rs
│   │   │   │   ├── apu.rs
│   │   │   │   └── memory.rs
│   │   │   ├── dialogs/    # Modal dialogs (egui::Modal)
│   │   │   │   ├── mod.rs
│   │   │   │   ├── achievements.rs
│   │   │   │   ├── netplay.rs
│   │   │   │   └── settings.rs
│   │   │   └── overlays/   # In-game overlays
│   │   │       ├── mod.rs
│   │   │       ├── input_display.rs
│   │   │       ├── lua_drawing.rs
│   │   │       └── notifications.rs
│   │   ├── audio.rs        # cpal 0.16 audio system
│   │   ├── input.rs        # gilrs 0.11 gamepad + keyboard
│   │   ├── config.rs       # ron 0.12 configuration
│   │   └── video/          # Shader/rendering (glow/OpenGL)
│   │       ├── mod.rs
│   │       ├── shaders.rs  # GLSL shader loading
│   │       └── capture.rs  # Screenshot/recording
│   └── Cargo.toml
├── rustynes-achievements/  # NEW: RetroAchievements
│   ├── src/
│   │   ├── lib.rs
│   │   ├── rcheevos.rs     # FFI bindings
│   │   ├── client.rs       # HTTP client
│   │   └── ui.rs           # egui integration (toasts, panels)
│   └── Cargo.toml
├── rustynes-netplay/       # NEW: GGPO netplay
│   ├── src/
│   │   ├── lib.rs
│   │   ├── session.rs      # Game session
│   │   ├── transport.rs    # UDP networking
│   │   └── lobby.rs        # egui lobby dialogs
│   └── Cargo.toml
├── rustynes-tas/           # NEW: TAS tools
│   ├── src/
│   │   ├── lib.rs
│   │   ├── fm2.rs          # FM2 format
│   │   ├── recording.rs    # Recording mode
│   │   └── playback.rs     # Playback mode
│   └── Cargo.toml
└── rustynes-scripting/     # NEW: Lua scripting
    ├── src/
    │   ├── lib.rs
    │   ├── api.rs          # Lua API
    │   └── drawing.rs      # egui overlay drawing
    └── Cargo.toml
```

### Data Flow Diagram

```text
┌────────────────────────────────────────────────────────────────┐
│                 rustynes-desktop (eframe 0.33)                 │
│  ┌─────────┐  ┌──────────┐  ┌────────────┐  ┌───────────────┐  │
│  │  Input  │  │  Audio   │  │   Video    │  │     GUI       │  │
│  │ (gilrs) │  │  (cpal)  │  │   (glow)   │  │   (egui)      │  │
│  └────┬────┘  └────▲─────┘  └─────▲──────┘  └───────┬───────┘  │
│       │            │              │                 │          │
│       ▼            │              │                 ▼          │
│  ┌─────────────────┴──────────────┴─────────────────────────┐  │
│  │              eframe::App::update() loop                  │  │
│  └─────────────────────────────┬────────────────────────────┘  │
└────────────────────────────────┼───────────────────────────────┘
                                 │
         ┌───────────────────────┼───────────────────────┐
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│ rustynes-tas    │    │  rustynes-core  │    │ rustynes-       │
│                 │    │                 │    │ achievements    │
│ ┌─────────────┐ │    │ ┌─────────────┐ │    │                 │
│ │ FM2 Movie   │ │    │ │   Console   │ │    │ ┌─────────────┐ │
│ │ Recording   │◄├───►│ │ CPU PPU APU │ │◄───┤ │  rcheevos   │ │
│ │ Playback    │ │    │ │ Bus Mapper  │ │    │ │  Memory     │ │
│ └─────────────┘ │    │ └─────────────┘ │    │ │  Polling    │ │
└─────────────────┘    │ ┌─────────────┐ │    │ └─────────────┘ │
                       │ │ Save State  │ │    └─────────────────┘
┌─────────────────┐    │ │ Serializer  │ │
│ rustynes-       │    │ └─────────────┘ │    ┌─────────────────┐
│ scripting       │    └─────────────────┘    │ rustynes-       │
│                 │             │             │ netplay         │
│ ┌─────────────┐ │             │             │                 │
│ │ Lua Engine  │◄├─────────────┼─────────────┤ ┌─────────────┐ │
│ │ Memory API  │ │             │             │ │  backroll   │ │
│ │ egui Draw   │ │             │             │ │  Session    │ │
│ └─────────────┘ │             ▼             │ │  Rollback   │ │
└─────────────────┘    ┌─────────────────┐    │ └─────────────┘ │
                       │   State Export  │    └─────────────────┘
                       │   (bincode for  │
                       │   netplay/TAS)  │
                       └─────────────────┘
```

---

## Dependencies

### Current Dependencies (v0.7.1)

| Crate | Version | Purpose |
|-------|---------|---------|
| `eframe` | 0.33 | GUI framework + window + OpenGL |
| `egui` | 0.33 | Immediate mode UI |
| `egui_extras` | 0.33 | Tables, images, utilities |
| `cpal` | 0.16 | Audio with underrun detection |
| `gilrs` | 0.11 | Gamepad with hotplug |
| `rfd` | 0.15 | Native file dialogs |
| `ron` | 0.12 | Configuration format |
| `directories` | 5.0 | Platform paths |
| `clap` | 4.5 | CLI parsing |
| `image` | 0.25 | PNG screenshot/thumbnails |

### Phase 2 Dependencies (Planned)

| Crate | Feature | Version | Purpose |
|-------|---------|---------|---------|
| `backroll` | Netplay | latest | GGPO rollback implementation |
| `mlua` | Scripting | 0.10+ | Lua 5.4 bindings |
| `rcheevos-sys` | Achievements | latest | RetroAchievements FFI |
| `bincode` | Save States | 2.0+ | Fast binary serialization |
| `zstd` | Save States | 0.13+ | State compression |
| `rubato` | Audio | 0.15+ | High-quality resampling |
| `rusqlite` | Library | 0.32+ | Game database (optional) |

### Critical Path

```text
Save States ──┬──► GGPO Netplay
              ├──► TAS Tools
              └──► Rewind Feature

Deterministic ──┬──► GGPO Netplay
Emulation       └──► TAS Playback

egui 0.33 ──┬──► All Debug Windows (native egui::Window)
            ├──► Lua Drawing Overlay (egui painter)
            ├──► Achievement Toasts (egui::Window anchored)
            └──► Netplay Lobby (egui::Modal dialogs)
```

---

## Testing Plan

### Unit Testing

- [ ] Save state serialization round-trip
- [ ] FM2 parsing and generation
- [ ] Lua API function coverage
- [ ] Input mapping logic

### Integration Testing

- [ ] Save/load with all mappers
- [ ] TAS playback determinism
- [ ] Netplay sync verification
- [ ] Achievement detection accuracy

### Manual Testing

- [ ] UI workflow testing
- [ ] Cross-platform verification
- [ ] Gamepad testing (various brands)
- [ ] Performance profiling

### Compatibility Testing

- [ ] FCEUX FM2 movie compatibility
- [ ] RetroAchievements with 10+ games
- [ ] FCEUX Lua script compatibility

---

## Timeline Estimates

### Phase 2 Sprint Schedule

| Sprint | Duration | Focus Areas | Key Deliverables |
|--------|----------|-------------|------------------|
| **S1** | 2 weeks | Save States Core | Serialization, quick save/load |
| **S2** | 2 weeks | Save States Extended | Slots, thumbnails, rewind |
| **S3** | 2 weeks | Debugger: CPU | Disassembly, breakpoints, stepping |
| **S4** | 2 weeks | Debugger: PPU/APU | Viewers, visualization |
| **S5** | 2 weeks | Debugger: Memory | Hex editor, trace logger |
| **S6** | 2 weeks | RetroAchievements | rcheevos integration |
| **S7** | 2 weeks | RetroAchievements UI | Toasts, achievement list |
| **S8** | 3 weeks | Netplay: Core | backroll-rs, determinism |
| **S9** | 2 weeks | Netplay: Networking | UDP, NAT traversal |
| **S10** | 2 weeks | Netplay: UI | Lobby, spectator |
| **S11** | 3 weeks | TAS Tools | FM2, recording, playback |
| **S12** | 3 weeks | Lua Scripting | mlua, API, drawing |

**Total Estimated Duration:** ~27 weeks (6-7 months)

### Milestone Targets

| Milestone | Target Date | Features |
|-----------|-------------|----------|
| **M7 Complete** | February 2026 | Save States, Debugger Phase 1 |
| **M8 Complete** | April 2026 | RetroAchievements |
| **M9 Complete** | July 2026 | GGPO Netplay |
| **M10 Complete** | September 2026 | TAS Tools, Lua Scripting |
| **Phase 2 Complete** | December 2026 | All features integrated |

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| rcheevos FFI complexity | Medium | Medium | Study RetroArch integration, dedicated time |
| Netplay desync issues | High | High | Determinism validation suite, extensive testing |
| Lua performance overhead | Medium | Medium | Profiling, selective callback usage |
| Cross-platform compatibility | Medium | Medium | CI/CD testing on all platforms |
| Scope creep | High | Medium | Strict feature prioritization, MVP focus |
| Dependency breaking changes | Low | High | Pin versions, regular dependency audits |

---

## References

### Internal Documentation

- [ROADMAP.md](/ROADMAP.md) - Project roadmap
- [ARCHITECTURE.md](/ARCHITECTURE.md) - System architecture
- [docs/api/SAVE_STATES.md](/docs/api/SAVE_STATES.md) - Save state spec
- [docs/formats/FM2_FORMAT.md](/docs/formats/FM2_FORMAT.md) - TAS movie format
- [crates/rustynes-desktop/README.md](/crates/rustynes-desktop/README.md) - Desktop frontend docs

### External References

- [RetroAchievements API](https://docs.retroachievements.org/) - Achievement integration
- [rcheevos GitHub](https://github.com/RetroAchievements/rcheevos) - FFI source
- [backroll-rs](https://github.com/HouraiTeahouse/backroll-rs) - GGPO Rust port
- [mlua Documentation](https://docs.rs/mlua) - Lua bindings
- [FCEUX Lua API](https://fceux.com/web/help/Lua.html) - Script compatibility reference
- [FM2 Movie Format](https://fceux.com/web/help/FM2.html) - TAS format spec

### Phase 2 Milestone TODOs

- [Milestone 7: RetroAchievements](milestone-7-achievements/README.md)
- [Milestone 8: Netplay](milestone-8-netplay/README.md)
- [Milestone 9: Scripting](milestone-9-scripting/README.md)
- [Milestone 10: Debugger](milestone-10-debugger/README.md)

---

## Changelog

### v2.0.0 (2025-12-28)

- **MAJOR**: Updated for eframe 0.33 + egui 0.33 framework
- Updated technology stack to reflect current dependencies:
  - eframe/egui 0.33 (was 0.29)
  - cpal 0.16 with buffer underrun detection (was 0.15)
  - ron 0.12 (was 0.8)
  - Rust 2024 Edition, MSRV 1.88
- Added egui 0.33 features section with new capabilities:
  - Atoms for status displays
  - Plugin trait for debug window organization
  - Modal dialogs for netplay/confirmation
  - kittest for UI testing
- Updated rendering architecture (glow/OpenGL, not wgpu)
- Updated shader section (GLSL, not WGSL)
- Added eframe application model documentation
- Updated crate structure to reflect current layout
- Revised dependency table with correct versions

### v1.0.0 (2025-12-27)

- Initial document creation
- Defined 12 feature categories with 400+ tasks
- Established sprint schedule and timeline
- Added technical architecture and data flow diagrams

---

**Document Maintainer:** Claude Code / Development Team
**Last Review:** 2025-12-28
**Next Review:** Upon Phase 1.5 completion (M9-M10)
