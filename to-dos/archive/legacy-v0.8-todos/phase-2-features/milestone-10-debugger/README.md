# Milestone 10: Advanced Debugger with Native egui

**Phase:** 2 (Advanced Features)
**Duration:** Months 10-11 (2 months)
**Status:** Planned
**Target:** November 2026
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Last Updated:** 2025-12-28

---

## Overview

Milestone 10 builds a comprehensive debugging toolset for homebrew development and reverse engineering. Since v0.7.1, RustyNES uses **eframe 0.33 + egui 0.33** as its primary GUI framework, so debug windows are **native egui windows** (not overlays on a separate framework).

**Architecture (v0.7.1+):**

- **Main UI:** eframe 0.33 + egui 0.33 (immediate-mode, unified framework)
- **Debug Windows:** Native `egui::Window` instances
- **Rendering:** OpenGL via glow backend
- **No wgpu:** Using glow instead of wgpu shader pipeline

This unified approach simplifies development - all UI (production and debug) uses the same immediate-mode paradigm with consistent theming and input handling.

---

## Goals

### Core Debug Features

- [ ] **CPU Debugger**
  - Disassembly viewer (6502 instructions)
  - Breakpoints (address, read/write, execution)
  - Single-step execution
  - Register viewer (A, X, Y, SP, PC, status flags)
  - Call stack tracing

- [ ] **PPU Viewer**
  - Nametable display (all 4 nametables)
  - Pattern table display (CHR-ROM/RAM)
  - Palette viewer (8 background + 8 sprite palettes)
  - OAM inspector (64 sprites, attributes)
  - Real-time updates (live visualization)

- [ ] **APU Viewer**
  - Channel waveforms (Square1, Square2, Triangle, Noise, DMC)
  - Volume meters (per-channel)
  - Frequency displays
  - Duty cycle indicators

- [ ] **Memory Viewer/Editor**
  - Hex dump (CPU address space $0000-$FFFF)
  - Binary search functionality
  - Watchpoints (track memory changes)
  - Inline editing with validation

- [ ] **Trace Logger**
  - Execution trace (PC, opcode, operands, registers)
  - Configurable filters (address ranges, opcodes)
  - Export to file (.log, .cdl)
  - Performance-optimized (circular buffer)

- [ ] **Code-Data Logger (CDL)**
  - Distinguish code vs data in ROM
  - Export CDL maps for disassemblers
  - Integrated with trace logger

---

## Architecture: Native egui (Unified Framework)

### Why Native egui? (v0.7.1 Migration)

Since v0.7.1, RustyNES uses eframe + egui as its **only** GUI framework:

**Benefits of Unified Framework:**

- **Simpler Codebase**: No hybrid Iced + egui complexity
- **Consistent Input Handling**: All UI uses egui input system
- **Unified Theming**: Production and debug UI share same visuals
- **No Overlay Rendering**: Debug windows are regular egui windows
- **Immediate Mode Throughout**: Rapid iteration for all UI

**egui 0.33 Features for Debugging:**

- `egui::Window` - Dockable, resizable debug windows
- `egui::ScrollArea` - Efficient scrolling for large data (disassembly, trace logs)
- `egui::Grid` - Memory hex dump with inline editing
- `egui_extras::TableBuilder` - Structured data (breakpoints, sprites)
- `egui::plot::Plot` - APU waveforms, timing graphs

### Application Structure

```text
┌───────────────────────────────────────────────────────────────┐
│  eframe Application (eframe::App trait)                       │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  egui::TopBottomPanel - Menu Bar                        │  │
│  │  (File, Emulation, Video, Audio, Debug, Help)           │  │
│  └─────────────────────────────────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  egui::CentralPanel - Game Viewport                     │  │
│  │  ┌───────────────────────────────────────────────────┐  │  │
│  │  │  egui::Image - NES Framebuffer (256x240 scaled)   │  │  │
│  │  └───────────────────────────────────────────────────┘  │  │
│  │                                                         │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │  │
│  │  │egui::Window │  │egui::Window │  │egui::Window     │  │  │
│  │  │CPU Debugger │  │PPU Viewer   │  │Memory Editor    │  │  │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘  │  │
│  │                                                         │  │
│  │  ┌─────────────┐  ┌─────────────┐                       │  │
│  │  │egui::Window │  │egui::Window │                       │  │
│  │  │APU Viewer   │  │Trace Logger │                       │  │
│  │  └─────────────┘  └─────────────┘                       │  │
│  └─────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────┘
```

### Debug Window Implementation

```rust
// In eframe::App::update() function (egui 0.33)
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // 1. Menu bar
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Debug", |ui| {
                if ui.button("CPU Debugger").clicked() {
                    self.show_cpu_debugger = true;
                }
                if ui.button("PPU Viewer").clicked() {
                    self.show_ppu_viewer = true;
                }
                // ... more debug windows
            });
        });
    });

    // 2. Game viewport (central panel)
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.image(&self.framebuffer_texture);
    });

    // 3. Debug windows (native egui::Window)
    if self.show_cpu_debugger {
        self.cpu_debugger_window(ctx);
    }
    if self.show_ppu_viewer {
        self.ppu_viewer_window(ctx);
    }
    if self.show_apu_viewer {
        self.apu_viewer_window(ctx);
    }
    if self.show_memory_editor {
        self.memory_editor_window(ctx);
    }
    if self.show_trace_logger {
        self.trace_logger_window(ctx);
    }
}
```

---

## Technical Details

### Debug Window Manager

**File:** `crates/rustynes-desktop/src/gui/debug/mod.rs`

The debug system uses a trait-based plugin architecture to organize windows:

```rust
use egui::Context;
use rustynes_core::Console;

/// Trait for all debug windows (egui 0.33)
pub trait DebugWindow {
    /// Window name (for menu and title bar)
    fn name(&self) -> &'static str;

    /// Render the debug window
    fn show(&mut self, ctx: &Context, console: &Console, open: &mut bool);
}

/// Debug window manager
pub struct DebugWindows {
    pub cpu_debugger: CpuDebugger,
    pub ppu_viewer: PpuViewer,
    pub apu_viewer: ApuViewer,
    pub memory_editor: MemoryEditor,
    pub trace_logger: TraceLogger,

    // Window open states
    pub show_cpu: bool,
    pub show_ppu: bool,
    pub show_apu: bool,
    pub show_memory: bool,
    pub show_trace: bool,
}

impl DebugWindows {
    pub fn new() -> Self {
        Self {
            cpu_debugger: CpuDebugger::new(),
            ppu_viewer: PpuViewer::new(),
            apu_viewer: ApuViewer::new(),
            memory_editor: MemoryEditor::new(),
            trace_logger: TraceLogger::new(),
            show_cpu: false,
            show_ppu: false,
            show_apu: false,
            show_memory: false,
            show_trace: false,
        }
    }

    /// Render all enabled debug windows
    pub fn show(&mut self, ctx: &Context, console: &Console) {
        if self.show_cpu {
            self.cpu_debugger.show(ctx, console, &mut self.show_cpu);
        }
        if self.show_ppu {
            self.ppu_viewer.show(ctx, console, &mut self.show_ppu);
        }
        if self.show_apu {
            self.apu_viewer.show(ctx, console, &mut self.show_apu);
        }
        if self.show_memory {
            self.memory_editor.show(ctx, console, &mut self.show_memory);
        }
        if self.show_trace {
            self.trace_logger.show(ctx, console, &mut self.show_trace);
        }
    }
}
```

### CPU Debugger Window

**File:** `crates/rustynes-desktop/src/gui/debug/cpu.rs`

```rust
use egui::{Context, Window, ScrollArea, Color32};

pub struct CpuDebugger {
    breakpoints: Vec<u16>,
    new_breakpoint_addr: String,
}

impl CpuDebugger {
    pub fn new() -> Self {
        Self {
            breakpoints: Vec::new(),
            new_breakpoint_addr: String::new(),
        }
    }
}

impl DebugWindow for CpuDebugger {
    fn name(&self) -> &'static str { "CPU Debugger" }

    fn show(&mut self, ctx: &Context, console: &Console, open: &mut bool) {
        Window::new(self.name())
            .open(open)
            .default_size([400.0, 600.0])
            .show(ctx, |ui| {
                // Disassembly viewer
                ui.heading("Disassembly");
                ui.separator();

                ScrollArea::vertical().id_salt("disasm").show(ui, |ui| {
                    for (addr, instruction) in disassemble_range(console, 20) {
                        let is_current = addr == console.cpu.pc();
                        let text = format!("{:04X}: {}", addr, instruction);

                        if is_current {
                            ui.colored_label(Color32::GREEN, text);
                        } else {
                            ui.label(text);
                        }
                    }
                });

                ui.separator();

                // Registers (using egui::Grid for alignment)
                ui.heading("Registers");
                egui::Grid::new("cpu_regs").show(ui, |ui| {
                    ui.monospace("A:"); ui.monospace(format!("${:02X}", console.cpu.a)); ui.end_row();
                    ui.monospace("X:"); ui.monospace(format!("${:02X}", console.cpu.x)); ui.end_row();
                    ui.monospace("Y:"); ui.monospace(format!("${:02X}", console.cpu.y)); ui.end_row();
                    ui.monospace("SP:"); ui.monospace(format!("${:02X}", console.cpu.sp)); ui.end_row();
                    ui.monospace("PC:"); ui.monospace(format!("${:04X}", console.cpu.pc)); ui.end_row();
                });

                // Status flags (N V - B D I Z C)
                ui.separator();
                ui.heading("Status Flags");
                ui.horizontal(|ui| {
                    let p = console.cpu.p;
                    let flag = |ui: &mut egui::Ui, name: &str, set: bool| {
                        if set {
                            ui.colored_label(Color32::GREEN, name);
                        } else {
                            ui.colored_label(Color32::GRAY, name);
                        }
                    };
                    flag(ui, "N", p & 0x80 != 0);
                    flag(ui, "V", p & 0x40 != 0);
                    ui.label("-");
                    flag(ui, "B", p & 0x10 != 0);
                    flag(ui, "D", p & 0x08 != 0);
                    flag(ui, "I", p & 0x04 != 0);
                    flag(ui, "Z", p & 0x02 != 0);
                    flag(ui, "C", p & 0x01 != 0);
                });

                ui.separator();

                // Breakpoints with TableBuilder (egui_extras)
                ui.heading("Breakpoints");
                // ... breakpoint UI ...

                ui.separator();

                // Controls
                ui.horizontal(|ui| {
                    if ui.button("Step").clicked() {
                        // Send step message to emulator
                    }
                    if ui.button("Run").clicked() {
                        // Send resume message
                    }
                    if ui.button("Pause").clicked() {
                        // Send pause message
                    }
                });
            });
    }
}
```

### Memory Editor with Grid

**File:** `crates/rustynes-desktop/src/gui/debug/memory.rs`

```rust
use egui::{Context, Window, ScrollArea, Grid, TextEdit};

pub struct MemoryEditor {
    goto_address: String,
    current_address: u16,
}

impl MemoryEditor {
    pub fn new() -> Self {
        Self {
            goto_address: String::from("0000"),
            current_address: 0,
        }
    }
}

impl DebugWindow for MemoryEditor {
    fn name(&self) -> &'static str { "Memory Editor" }

    fn show(&mut self, ctx: &Context, console: &Console, open: &mut bool) {
        Window::new(self.name())
            .open(open)
            .default_size([600.0, 700.0])
            .show(ctx, |ui| {
                // Go to address
                ui.horizontal(|ui| {
                    ui.label("Go to:");
                    let response = TextEdit::singleline(&mut self.goto_address)
                        .desired_width(60.0)
                        .font(egui::TextStyle::Monospace)
                        .show(ui);
                    if response.response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        if let Ok(addr) = u16::from_str_radix(&self.goto_address, 16) {
                            self.current_address = addr & 0xFFF0; // Align to 16-byte boundary
                        }
                    }
                });

                ui.separator();

                // Hex dump using Grid
                ScrollArea::vertical()
                    .id_salt("hex_scroll")
                    .show(ui, |ui| {
                        Grid::new("hex_grid")
                            .num_columns(18) // addr + 16 bytes + ASCII
                            .striped(true)
                            .min_col_width(0.0)
                            .show(ui, |ui| {
                                for row in 0..32 {
                                    let addr = self.current_address.wrapping_add((row * 16) as u16);
                                    ui.monospace(format!("{:04X}:", addr));

                                    for col in 0..16 {
                                        let byte_addr = addr.wrapping_add(col);
                                        let byte = console.cpu_read(byte_addr);
                                        ui.monospace(format!("{:02X}", byte));
                                    }

                                    // ASCII column
                                    ui.label("|");
                                    let ascii: String = (0..16)
                                        .map(|col| {
                                            let byte = console.cpu_read(addr.wrapping_add(col));
                                            if byte.is_ascii_graphic() {
                                                byte as char
                                            } else {
                                                '.'
                                            }
                                        })
                                        .collect();
                                    ui.monospace(ascii);

                                    ui.end_row();
                                }
                            });
                    });

                ui.separator();

                // Watchpoints
                ui.heading("Watchpoints");
                // ... watchpoint UI ...
            });
    }
}
```

### APU Waveform Viewer with Plot

**File:** `crates/rustynes-desktop/src/gui/debug/apu.rs`

```rust
use egui::{Context, Window};
use egui_plot::{Plot, Line, PlotPoints};

pub struct ApuViewer {
    square1_samples: Vec<f64>,
    square2_samples: Vec<f64>,
    triangle_samples: Vec<f64>,
    noise_samples: Vec<f64>,
}

impl DebugWindow for ApuViewer {
    fn name(&self) -> &'static str { "APU Viewer" }

    fn show(&mut self, ctx: &Context, console: &Console, open: &mut bool) {
        Window::new(self.name())
            .open(open)
            .default_size([400.0, 500.0])
            .show(ctx, |ui| {
                ui.heading("Waveforms");

                // Square 1 waveform plot
                ui.label("Square 1");
                let points: PlotPoints = self.square1_samples
                    .iter()
                    .enumerate()
                    .map(|(i, &y)| [i as f64, y])
                    .collect();
                Plot::new("square1")
                    .height(60.0)
                    .show_axes([false, true])
                    .show(ui, |plot_ui| {
                        plot_ui.line(Line::new(points));
                    });

                // Square 2 waveform plot
                ui.label("Square 2");
                // ... similar ...

                // Triangle waveform plot
                ui.label("Triangle");
                // ... similar ...

                ui.separator();

                // Channel status (using Atoms pattern from egui 0.33)
                ui.heading("Channel Status");
                egui::Grid::new("channel_status").show(ui, |ui| {
                    ui.label("Channel");
                    ui.label("Enabled");
                    ui.label("Volume");
                    ui.label("Frequency");
                    ui.end_row();

                    // Square 1
                    ui.label("Square 1");
                    ui.label(if console.apu.square1_enabled { "ON" } else { "OFF" });
                    ui.label(format!("{}", console.apu.square1_volume));
                    ui.label(format!("{} Hz", console.apu.square1_freq));
                    ui.end_row();

                    // ... more channels ...
                });
            });
    }
}
```

---

## Implementation Plan

### Sprint 1: Debug Window Framework

**Duration:** 2 weeks

- [ ] Create `DebugWindow` trait for consistent interface
- [ ] Implement `DebugWindows` manager struct
- [ ] Add debug menu entries to menu bar
- [ ] Implement keyboard shortcuts (F12 toggle all, Ctrl+D/P/M/T individual)
- [ ] Test window open/close state persistence

### Sprint 2: CPU Debugger

**Duration:** 2 weeks

- [ ] Disassembly viewer (6502 instruction decoder)
- [ ] Breakpoint system (address, read/write, execution)
- [ ] Single-step execution
- [ ] Register viewer (A, X, Y, SP, PC, status flags)
- [ ] Call stack tracing

### Sprint 3: PPU Viewer

**Duration:** 2 weeks

- [ ] Nametable renderer (all 4 nametables)
- [ ] Pattern table renderer (CHR-ROM/RAM visualization)
- [ ] Palette viewer (8 BG + 8 sprite palettes)
- [ ] OAM inspector (64 sprites, attributes)
- [ ] Real-time update system

### Sprint 4: APU Viewer & Memory Editor

**Duration:** 2 weeks

- [ ] APU waveform plots (egui::plot)
- [ ] Volume meters (per-channel)
- [ ] Frequency displays
- [ ] Memory hex dump viewer
- [ ] Watchpoint system
- [ ] Inline memory editing

### Sprint 5: Trace Logger & CDL

**Duration:** 1 week

- [ ] Execution trace capture (circular buffer)
- [ ] Configurable filters
- [ ] Export to .log files
- [ ] CDL map generation
- [ ] Performance optimization

---

## Acceptance Criteria

### Functionality

- [ ] All debug windows functional (CPU, PPU, APU, Memory, Trace)
- [ ] Breakpoints work reliably (hit detection, resume)
- [ ] PPU viewer updates in real-time (60 FPS)
- [ ] Trace logger captures execution without crashing
- [ ] Memory editor allows inline editing
- [ ] CDL maps export correctly

### Performance

- [ ] Debug overlay <2ms render time
- [ ] No impact on emulation when overlay hidden
- [ ] Trace logger <5% CPU overhead (when enabled)
- [ ] PPU viewer updates at 60 FPS

### User Experience

- [ ] egui windows dockable and resizable
- [ ] Clear visual indication of current PC in disassembly
- [ ] PPU viewer shows live updates
- [ ] Easy to toggle debug overlay (F12 key)

---

## Dependencies

### Prerequisites

- **Phase 1.5 Complete:** eframe 0.33 + egui 0.33 desktop frontend established
- **Core Emulation:** Full CPU, PPU, APU implementation

### Current Technology Stack (v0.7.1+)

The debug windows leverage dependencies already in place:

```toml
# crates/rustynes-desktop/Cargo.toml (already present in v0.7.1)

# GUI framework - eframe provides egui + window management + OpenGL rendering
eframe = { version = "0.33", default-features = false, features = ["default_fonts", "glow", "wayland", "x11"] }
egui = "0.33"
egui_extras = { version = "0.33", features = ["image"] }

# For APU waveform plots (optional, add if not present)
# Note: egui_plot moved to separate crate in egui 0.33
# egui_plot = "0.33"
```

### Additional Debug Features (to add)

```toml
# Optional additions for enhanced debugging

[dependencies]
# Plot widget for APU waveforms (if not using egui's built-in)
egui_plot = "0.33"

[features]
default = []
debug-windows = []  # Enable debug window compilation
```

---

## Related Documentation

- [DESKTOP-FRONTEND-PHASE2.md](../DESKTOP-FRONTEND-PHASE2.md) - Phase 2 desktop frontend enhancements
- [PHASE-2-OVERVIEW.md](../PHASE-2-OVERVIEW.md) - Phase 2 overview with egui 0.33 technology stack
- [crates/rustynes-desktop/README.md](../../../crates/rustynes-desktop/README.md) - Desktop crate architecture

---

## Success Criteria

1. All debug windows implemented as native egui::Window instances
2. DebugWindow trait provides consistent interface for all debug tools
3. All debug windows functional (CPU, PPU, APU, Memory, Trace)
4. Breakpoints, stepping, and watchpoints work reliably
5. Real-time PPU visualization at 60 FPS
6. Trace logger captures execution efficiently
7. CDL maps export for disassemblers
8. Useful for homebrew debugging (verified by testers)
9. Minimal performance impact when debug windows closed
10. M10 milestone marked as COMPLETE

---

## Supplementary: egui Patterns and Best Practices

### Keyboard Shortcuts for Debug Windows

Recommended keyboard shortcuts for toggling debug windows (egui 0.33):

```rust
// In eframe::App::update() function
impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts for debug windows
        ctx.input(|i| {
            // F12: Toggle all debug windows
            if i.key_pressed(egui::Key::F12) {
                self.debug_windows.toggle_all();
            }

            // Ctrl+D: Toggle CPU debugger
            if i.modifiers.ctrl && i.key_pressed(egui::Key::D) {
                self.debug_windows.show_cpu = !self.debug_windows.show_cpu;
            }

            // Ctrl+P: Toggle PPU viewer
            if i.modifiers.ctrl && i.key_pressed(egui::Key::P) {
                self.debug_windows.show_ppu = !self.debug_windows.show_ppu;
            }

            // Ctrl+M: Toggle memory editor
            if i.modifiers.ctrl && i.key_pressed(egui::Key::M) {
                self.debug_windows.show_memory = !self.debug_windows.show_memory;
            }

            // Ctrl+T: Toggle trace logger
            if i.modifiers.ctrl && i.key_pressed(egui::Key::T) {
                self.debug_windows.show_trace = !self.debug_windows.show_trace;
            }
        });

        // ... rest of update()
    }
}
```

### Performance Stats Window

Add FPS counter and emulation stats window (egui 0.33):

```rust
pub struct PerformanceStats {
    fps_counter: FpsCounter,
    cpu_cycles: u64,
    ppu_dots: u64,
}

impl DebugWindow for PerformanceStats {
    fn name(&self) -> &'static str { "Performance" }

    fn show(&mut self, ctx: &Context, console: &Console, open: &mut bool) {
        egui::Window::new(self.name())
            .open(open)
            .default_size([300.0, 200.0])
            .show(ctx, |ui| {
                ui.heading("Frame Stats");
                ui.label(format!("FPS: {:.1}", self.fps_counter.fps()));
                ui.label(format!("Frame Time: {:.2}ms", self.fps_counter.frame_time_ms()));

                ui.separator();

                ui.heading("Emulation Stats");
                ui.label(format!("CPU Cycles: {}", console.total_cpu_cycles()));
                ui.label(format!("PPU Dots: {}", console.total_ppu_dots()));
                ui.label(format!("Frame: {}", console.frame_count()));
            });
    }
}

/// FPS counter for debug overlay
struct FpsCounter {
    frame_times: Vec<std::time::Instant>,
    window: usize,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            frame_times: Vec::with_capacity(60),
            window: 60,
        }
    }

    fn tick(&mut self) {
        let now = std::time::Instant::now();
        self.frame_times.push(now);

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
        let fps = self.fps();
        if fps > 0.0 {
            1000.0 / fps
        } else {
            0.0
        }
    }
}
```

### egui Input Handling (Unified Framework)

With eframe + egui, input handling is unified - no event forwarding needed:

```rust
impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // egui handles all input automatically via eframe integration
        // Check if egui wants keyboard/mouse focus for debug windows
        let egui_wants_input = ctx.wants_keyboard_input() || ctx.wants_pointer_input();

        // Only forward input to emulator if egui doesn't want it
        if !egui_wants_input {
            ctx.input(|i| {
                // Handle game input
                for key in &i.keys_down {
                    self.handle_game_key(*key, true);
                }
            });
        }

        // Debug windows receive input naturally through egui
        self.debug_windows.show(ctx, &self.console);
    }
}
```

### egui Immediate Mode Best Practices

**Memory Management:**

- Minimize allocations in egui update loop
- Use `egui::Id` for persistent widget state
- Cache expensive computations between frames

**Performance:**

- Only render visible debug windows
- Use `ScrollArea::auto_shrink()` for large lists
- Limit update frequency for expensive visualizations (PPU viewer)

**State Management:**

- Keep egui state minimal (window open/close flags)
- Store debug data in Console, not egui context
- Use message passing for emulation control (breakpoints, stepping)

---

**Last Updated:** 2025-12-28
**Milestone Status:** PLANNED
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Next Milestone:** Phase 2 completion and v1.0 release preparation
