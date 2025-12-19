# Milestone 10: Advanced Debugger with egui Overlay

**Phase:** 2 (Advanced Features)
**Duration:** Months 10-11 (2 months)
**Status:** Planned
**Target:** November 2026
**Prerequisites:** M6 MVP Complete (Iced GUI established)

---

## Overview

Milestone 10 builds a comprehensive debugging toolset for homebrew development and reverse engineering. This milestone integrates **egui as a debug overlay** within the Iced application, providing immediate-mode tools for CPU debugging, PPU visualization, APU monitoring, and memory editing.

**Architecture:**

- **Main UI:** Iced 0.13+ (retained-mode, production interface)
- **Debug Overlay:** egui 0.28 (immediate-mode, developer tools)
- **Integration:** egui rendered as overlay via wgpu

This hybrid approach leverages Iced's structured state management for the main application while using egui's flexibility for rapid debug tool iteration.

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

## Architecture: Iced + egui Hybrid

### Why egui for Debug Tools?

**egui Strengths (Immediate Mode):**

- Rapid iteration (no state management boilerplate)
- Perfect for transient debug info (registers, memory, waveforms)
- Small window overhead (debug tools don't need Elm architecture)
- Built-in widgets (sliders, toggles, color pickers)

**Iced Strengths (Retained Mode):**

- Main application structure (views, navigation)
- Consistent theming across production UI
- Better for complex state (emulation, settings, library)

### Integration Strategy

```text
┌───────────────────────────────────────────────┐
│  Iced Application (Main)                      │
│  ┌─────────────────────────────────────────┐  │
│  │  Game Viewport (wgpu)                   │  │
│  │                                         │  │
│  │  ┌────────────────────────────────┐     │  │
│  │  │  egui Debug Overlay            │     │  │
│  │  │  • CPU Debugger                │     │  │
│  │  │  • PPU Viewer                  │     │  │
│  │  │  • APU Viewer                  │     │  │
│  │  │  • Memory Editor               │     │  │
│  │  │  • Trace Logger                │     │  │
│  │  └────────────────────────────────┘     │  │
│  └─────────────────────────────────────────┘  │
│                                               │
│  Iced Menus, Settings, Library (Production)   │
└───────────────────────────────────────────────┘
```

### Rendering Pipeline

```rust
// In Iced's view() function
fn view(&self) -> Element<Message> {
    // 1. Render game viewport (wgpu custom widget)
    let game = game_viewport(&self.framebuffer);

    // 2. Render egui overlay (if debug mode enabled)
    let debug_overlay = if self.debug_mode {
        egui_overlay(&mut self.egui_state, &self.console)
    } else {
        None
    };

    // 3. Composite layers
    stack![game, debug_overlay.unwrap_or_default()]
}
```

---

## Technical Details

### egui Integration with Iced

**File:** `crates/rustynes-desktop/src/debug/egui_integration.rs`

```rust
use egui::{Context, RawInput, FullOutput};
use egui_wgpu::Renderer as EguiRenderer;

pub struct EguiDebugOverlay {
    /// egui context (immediate-mode state)
    ctx: Context,

    /// egui renderer (wgpu backend)
    renderer: EguiRenderer,

    /// Window open states
    pub show_cpu: bool,
    pub show_ppu: bool,
    pub show_apu: bool,
    pub show_memory: bool,
    pub show_trace: bool,
}

impl EguiDebugOverlay {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let ctx = Context::default();
        let renderer = EguiRenderer::new(device, format, None, 1);

        Self {
            ctx,
            renderer,
            show_cpu: false,
            show_ppu: false,
            show_apu: false,
            show_memory: false,
            show_trace: false,
        }
    }

    pub fn update(&mut self, console: &Console, input: RawInput) -> FullOutput {
        self.ctx.begin_frame(input);

        // Render debug windows
        if self.show_cpu {
            self.cpu_debugger_window(console);
        }
        if self.show_ppu {
            self.ppu_viewer_window(console);
        }
        if self.show_apu {
            self.apu_viewer_window(console);
        }
        if self.show_memory {
            self.memory_editor_window(console);
        }
        if self.show_trace {
            self.trace_logger_window(console);
        }

        self.ctx.end_frame()
    }

    fn cpu_debugger_window(&mut self, console: &Console) {
        egui::Window::new("CPU Debugger")
            .default_size([400.0, 600.0])
            .show(&self.ctx, |ui| {
                // Disassembly viewer
                ui.heading("Disassembly");
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (addr, instruction) in disassemble_range(console, 20) {
                        let is_current = addr == console.cpu.pc();
                        let text = format!("{:04X}: {}", addr, instruction);

                        if is_current {
                            ui.colored_label(egui::Color32::GREEN, text);
                        } else {
                            ui.label(text);
                        }
                    }
                });

                ui.separator();

                // Registers
                ui.heading("Registers");
                ui.monospace(format!("A:  ${:02X}", console.cpu.a));
                ui.monospace(format!("X:  ${:02X}", console.cpu.x));
                ui.monospace(format!("Y:  ${:02X}", console.cpu.y));
                ui.monospace(format!("SP: ${:02X}", console.cpu.sp));
                ui.monospace(format!("PC: ${:04X}", console.cpu.pc));

                ui.separator();

                // Breakpoints
                ui.heading("Breakpoints");
                // ... breakpoint UI ...

                ui.separator();

                // Controls
                ui.horizontal(|ui| {
                    if ui.button("Step").clicked() {
                        console.step_instruction();
                    }
                    if ui.button("Run").clicked() {
                        console.resume();
                    }
                    if ui.button("Pause").clicked() {
                        console.pause();
                    }
                });
            });
    }

    fn ppu_viewer_window(&mut self, console: &Console) {
        egui::Window::new("PPU Viewer")
            .default_size([600.0, 800.0])
            .show(&self.ctx, |ui| {
                ui.heading("Nametables");

                // Render all 4 nametables as images
                let nametables = console.ppu.render_nametables();
                for (i, nametable_tex) in nametables.iter().enumerate() {
                    ui.label(format!("Nametable {}", i));
                    ui.image(nametable_tex);
                }

                ui.separator();

                ui.heading("Pattern Tables");

                // Render CHR-ROM pattern tables
                let pattern_tables = console.ppu.render_pattern_tables();
                ui.horizontal(|ui| {
                    ui.label("Bank 0");
                    ui.image(&pattern_tables[0]);
                    ui.label("Bank 1");
                    ui.image(&pattern_tables[1]);
                });

                ui.separator();

                ui.heading("Palettes");

                // Render palette swatches
                let palettes = console.ppu.palettes();
                for (i, palette) in palettes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Palette {}: ", i));
                        for color in palette {
                            ui.colored_label(
                                egui::Color32::from_rgb(color.r, color.g, color.b),
                                "█"
                            );
                        }
                    });
                }

                ui.separator();

                ui.heading("OAM (Sprites)");

                // Render sprite attributes
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..64 {
                        let sprite = console.ppu.oam[i * 4..(i + 1) * 4];
                        ui.monospace(format!(
                            "Sprite {:02}: Y={:02X} Tile={:02X} Attr={:02X} X={:02X}",
                            i, sprite[0], sprite[1], sprite[2], sprite[3]
                        ));
                    }
                });
            });
    }

    fn apu_viewer_window(&mut self, console: &Console) {
        egui::Window::new("APU Viewer")
            .default_size([400.0, 500.0])
            .show(&self.ctx, |ui| {
                // Channel waveforms (plot)
                ui.heading("Waveforms");

                // Square 1
                let square1_samples = console.apu.square1.recent_samples();
                egui::plot::Plot::new("square1_plot")
                    .height(60.0)
                    .show(ui, |plot_ui| {
                        plot_ui.line(egui::plot::Line::new(square1_samples));
                    });

                // Square 2
                let square2_samples = console.apu.square2.recent_samples();
                egui::plot::Plot::new("square2_plot")
                    .height(60.0)
                    .show(ui, |plot_ui| {
                        plot_ui.line(egui::plot::Line::new(square2_samples));
                    });

                // ... similar for Triangle, Noise, DMC ...

                ui.separator();

                // Volume meters
                ui.heading("Volume");
                ui.add(egui::Slider::new(&mut console.apu.square1.volume, 0..=15).text("Square 1"));
                ui.add(egui::Slider::new(&mut console.apu.square2.volume, 0..=15).text("Square 2"));
                // ... etc ...
            });
    }

    fn memory_editor_window(&mut self, console: &mut Console) {
        egui::Window::new("Memory Editor")
            .default_size([600.0, 700.0])
            .show(&self.ctx, |ui| {
                ui.heading("Hex Dump");

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for addr in (0x0000..=0xFFFF).step_by(16) {
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{:04X}:", addr));

                            for offset in 0..16 {
                                let byte = console.cpu_read(addr + offset);
                                ui.monospace(format!("{:02X}", byte));
                            }

                            ui.label("|");

                            // ASCII representation
                            for offset in 0..16 {
                                let byte = console.cpu_read(addr + offset);
                                let ch = if byte.is_ascii_graphic() {
                                    byte as char
                                } else {
                                    '.'
                                };
                                ui.monospace(ch.to_string());
                            }
                        });
                    }
                });

                ui.separator();

                // Watchpoints
                ui.heading("Watchpoints");
                // ... watchpoint UI ...
            });
    }

    fn trace_logger_window(&mut self, console: &Console) {
        egui::Window::new("Trace Logger")
            .default_size([700.0, 600.0])
            .show(&self.ctx, |ui| {
                ui.heading("Execution Trace");

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for trace_entry in console.trace_log.recent_entries(100) {
                        ui.monospace(format!(
                            "{:04X}: {} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X}",
                            trace_entry.pc,
                            trace_entry.instruction,
                            trace_entry.a,
                            trace_entry.x,
                            trace_entry.y,
                            trace_entry.p,
                            trace_entry.sp
                        ));
                    }
                });

                ui.separator();

                // Export controls
                ui.horizontal(|ui| {
                    if ui.button("Export Log").clicked() {
                        console.trace_log.export_to_file("trace.log");
                    }
                    if ui.button("Clear").clicked() {
                        console.trace_log.clear();
                    }
                });
            });
    }
}
```

---

## Implementation Plan

### Sprint 1: egui Integration with Iced

**Duration:** 2 weeks

- [ ] Add egui dependencies (egui, egui-wgpu)
- [ ] Create egui overlay widget for Iced
- [ ] Implement egui rendering in wgpu pipeline
- [ ] Test basic egui window display
- [ ] Handle input forwarding to egui

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

- **M6 MVP Complete:** Iced GUI established, wgpu rendering
- **Core Emulation:** Full CPU, PPU, APU implementation

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies.egui]
version = "0.28"
optional = true

[dependencies.egui-wgpu]
version = "0.28"
optional = true

[features]
default = []
debug-overlay = ["egui", "egui-wgpu"]
```

---

## Related Documentation

- [M6-S1-iced-application.md](../../phase-1-mvp/milestone-6-gui/M6-S1-iced-application.md) - Iced architecture
- [M6-S2-wgpu-rendering.md](../../phase-1-mvp/milestone-6-gui/M6-S2-wgpu-rendering.md) - wgpu integration
- [M6-PLANNING-CHANGES.md](../../phase-1-mvp/milestone-6-gui/M6-PLANNING-CHANGES.md) - Iced + egui hybrid justification

---

## Success Criteria

1. egui integrated as overlay within Iced application
2. All debug windows functional (CPU, PPU, APU, Memory, Trace)
3. Breakpoints, stepping, and watchpoints work reliably
4. Real-time PPU visualization at 60 FPS
5. Trace logger captures execution efficiently
6. CDL maps export for disassemblers
7. Useful for homebrew debugging (verified by testers)
8. Zero performance impact when debug overlay disabled
9. M10 milestone marked as ✅ COMPLETE

---

## Supplementary: egui Patterns and Best Practices

### Keyboard Shortcuts for Debug Overlay

Recommended keyboard shortcuts for toggling debug windows:

```rust
// In Iced's update() function
impl RustyNes {
    fn handle_debug_shortcuts(&mut self, key: iced::keyboard::Key) {
        use iced::keyboard::Key;

        match key {
            // F12: Toggle entire debug overlay
            Key::F12 => self.debug_overlay.toggle_all(),

            // Ctrl+D: Toggle CPU debugger
            Key::D if self.modifiers.control() => {
                self.debug_overlay.show_cpu = !self.debug_overlay.show_cpu;
            }

            // Ctrl+P: Toggle PPU viewer
            Key::P if self.modifiers.control() => {
                self.debug_overlay.show_ppu = !self.debug_overlay.show_ppu;
            }

            // Ctrl+M: Toggle memory editor
            Key::M if self.modifiers.control() => {
                self.debug_overlay.show_memory = !self.debug_overlay.show_memory;
            }

            // Ctrl+T: Toggle trace logger
            Key::T if self.modifiers.control() => {
                self.debug_overlay.show_trace = !self.debug_overlay.show_trace;
            }

            _ => {}
        }
    }
}
```

### Debug Overlay Performance Monitoring

Add FPS counter to debug overlay for performance monitoring:

```rust
impl EguiDebugOverlay {
    /// Render performance stats window
    fn performance_stats_window(&mut self) {
        egui::Window::new("Performance")
            .default_size([300.0, 200.0])
            .show(&self.ctx, |ui| {
                ui.heading("Debug Overlay Stats");

                // FPS counter
                ui.label(format!("FPS: {:.1}", self.fps_counter.fps()));
                ui.label(format!("Frame Time: {:.2}ms", self.fps_counter.frame_time_ms()));

                ui.separator();

                // Emulation stats
                ui.heading("Emulation Stats");
                ui.label(format!("CPU Cycles: {}", self.cpu_cycles));
                ui.label(format!("PPU Dots: {}", self.ppu_dots));

                ui.separator();

                // Memory usage
                ui.heading("Memory Usage");
                ui.label(format!("Overlay: {:.1} MB", self.memory_usage_mb()));
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

### egui Input Handling Best Practices

Forward input events from Iced to egui overlay:

```rust
impl RustyNes {
    fn forward_input_to_egui(&mut self, event: &iced::Event) -> bool {
        // Convert Iced event to egui RawInput
        match event {
            iced::Event::Mouse(mouse_event) => {
                // Convert mouse position, buttons, scroll
                self.egui_input.events.push(convert_mouse_event(mouse_event));
                true
            }

            iced::Event::Keyboard(key_event) => {
                // Convert keyboard input
                self.egui_input.events.push(convert_key_event(key_event));
                true
            }

            _ => false
        }
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

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M6 MVP Complete
**Next Milestone:** M11 (Advanced CRT Shaders)
