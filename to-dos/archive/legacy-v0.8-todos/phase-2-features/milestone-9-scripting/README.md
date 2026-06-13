# Milestone 9: Lua Scripting

**Phase:** 2 (Advanced Features)
**Duration:** Months 9-10
**Status:** Planned
**Target:** October 2026
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Last Updated:** 2025-12-28

---

## Overview

Integrate mlua 5.4 for runtime scripting. This milestone enables users to create custom tools, bots, visualizations, and gameplay modifications through Lua scripts. The GUI uses native egui drawing API for overlays and egui::Window for script console.

## Goals

- [ ] mlua 5.4 integration
- [ ] Memory read/write API
- [ ] Callback hooks (frame, scanline, instruction)
- [ ] Input injection
- [ ] GUI overlay support (egui painter API)
- [ ] Script console (egui::Window)
- [ ] Example scripts (hitbox viewer, bot AI)

## UI Integration (egui 0.33)

### Script Console Window

Interactive Lua console using egui::Window:

```rust
use egui::{Context, Window, ScrollArea, TextEdit, Color32, FontId, TextStyle};

pub struct ScriptConsole {
    input: String,
    output_log: Vec<ConsoleEntry>,
    history: Vec<String>,
    history_index: usize,
    script_running: bool,
}

pub struct ConsoleEntry {
    text: String,
    entry_type: ConsoleEntryType,
}

pub enum ConsoleEntryType {
    Input,
    Output,
    Error,
    Info,
}

impl ScriptConsole {
    pub fn show(&mut self, ctx: &Context, lua_state: &mut LuaState, open: &mut bool) {
        Window::new("Lua Console")
            .open(open)
            .default_size([600.0, 400.0])
            .show(ctx, |ui| {
                // Toolbar
                ui.horizontal(|ui| {
                    if ui.button("Load Script...").clicked() {
                        // Open file dialog
                    }
                    if ui.button("Save Output").clicked() {
                        // Save log to file
                    }
                    if ui.button("Clear").clicked() {
                        self.output_log.clear();
                    }

                    ui.separator();

                    // Running indicator
                    if self.script_running {
                        ui.colored_label(Color32::GREEN, "Script Running");
                        if ui.button("Stop").clicked() {
                            lua_state.stop();
                            self.script_running = false;
                        }
                    }
                });

                ui.separator();

                // Output log with scroll
                ScrollArea::vertical()
                    .id_salt("console_output")
                    .stick_to_bottom(true)
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for entry in &self.output_log {
                            let color = match entry.entry_type {
                                ConsoleEntryType::Input => Color32::LIGHT_BLUE,
                                ConsoleEntryType::Output => Color32::WHITE,
                                ConsoleEntryType::Error => Color32::RED,
                                ConsoleEntryType::Info => Color32::YELLOW,
                            };
                            ui.colored_label(color, &entry.text);
                        }
                    });

                ui.separator();

                // Input line
                ui.horizontal(|ui| {
                    ui.label(">");
                    let response = TextEdit::singleline(&mut self.input)
                        .font(TextStyle::Monospace)
                        .desired_width(ui.available_width() - 60.0)
                        .show(ui);

                    // Handle Enter key
                    if response.response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        self.execute_input(lua_state);
                    }

                    // Handle history navigation
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        self.navigate_history(-1);
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        self.navigate_history(1);
                    }

                    if ui.button("Run").clicked() {
                        self.execute_input(lua_state);
                    }
                });
            });
    }

    fn execute_input(&mut self, lua_state: &mut LuaState) {
        if self.input.is_empty() {
            return;
        }

        // Add to history
        self.history.push(self.input.clone());
        self.history_index = self.history.len();

        // Log input
        self.output_log.push(ConsoleEntry {
            text: format!("> {}", self.input),
            entry_type: ConsoleEntryType::Input,
        });

        // Execute
        match lua_state.execute(&self.input) {
            Ok(result) => {
                if !result.is_empty() {
                    self.output_log.push(ConsoleEntry {
                        text: result,
                        entry_type: ConsoleEntryType::Output,
                    });
                }
            }
            Err(err) => {
                self.output_log.push(ConsoleEntry {
                    text: format!("Error: {}", err),
                    entry_type: ConsoleEntryType::Error,
                });
            }
        }

        self.input.clear();
    }

    fn navigate_history(&mut self, delta: isize) {
        if self.history.is_empty() {
            return;
        }
        let new_index = (self.history_index as isize + delta)
            .clamp(0, self.history.len() as isize) as usize;
        self.history_index = new_index;
        if new_index < self.history.len() {
            self.input = self.history[new_index].clone();
        }
    }
}
```

### Script Overlay Drawing

Lua scripts can draw using egui's painter API:

```rust
use egui::{Context, Painter, Pos2, Rect, Color32, Stroke, FontId};

/// Lua drawing context passed to scripts
pub struct LuaDrawContext {
    painter: Painter,
    game_rect: Rect,
    scale: f32,
}

impl LuaDrawContext {
    /// Draw a rectangle (Lua: gui.drawRect(x, y, w, h, color))
    pub fn draw_rect(&self, x: f32, y: f32, w: f32, h: f32, color: Color32) {
        let rect = Rect::from_min_size(
            self.game_to_screen(x, y),
            egui::vec2(w * self.scale, h * self.scale),
        );
        self.painter.rect_stroke(rect, 0.0, Stroke::new(1.0, color));
    }

    /// Draw a filled rectangle (Lua: gui.fillRect(x, y, w, h, color))
    pub fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, color: Color32) {
        let rect = Rect::from_min_size(
            self.game_to_screen(x, y),
            egui::vec2(w * self.scale, h * self.scale),
        );
        self.painter.rect_filled(rect, 0.0, color);
    }

    /// Draw text (Lua: gui.text(x, y, text, color))
    pub fn draw_text(&self, x: f32, y: f32, text: &str, color: Color32) {
        let pos = self.game_to_screen(x, y);
        self.painter.text(
            pos,
            egui::Align2::LEFT_TOP,
            text,
            FontId::monospace(10.0 * self.scale),
            color,
        );
    }

    /// Draw a line (Lua: gui.drawLine(x1, y1, x2, y2, color))
    pub fn draw_line(&self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color32) {
        let p1 = self.game_to_screen(x1, y1);
        let p2 = self.game_to_screen(x2, y2);
        self.painter.line_segment([p1, p2], Stroke::new(1.0, color));
    }

    /// Draw a circle (Lua: gui.drawCircle(cx, cy, r, color))
    pub fn draw_circle(&self, cx: f32, cy: f32, r: f32, color: Color32) {
        let center = self.game_to_screen(cx, cy);
        self.painter.circle_stroke(center, r * self.scale, Stroke::new(1.0, color));
    }

    /// Convert game coordinates (256x240) to screen coordinates
    fn game_to_screen(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(
            self.game_rect.min.x + x * self.scale,
            self.game_rect.min.y + y * self.scale,
        )
    }
}
```

### Integration with eframe::App

```rust
impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Menu bar
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Tools", |ui| {
                    if ui.button("Lua Console").clicked() {
                        self.show_script_console = true;
                    }
                });
            });
        });

        // Game viewport with script overlay
        egui::CentralPanel::default().show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(
                ui.available_size(),
                egui::Sense::hover(),
            );

            // Draw game framebuffer
            let game_rect = response.rect;
            // ... render game ...

            // Execute Lua overlay drawing
            if self.lua_state.has_draw_callback() {
                let scale = game_rect.width() / 256.0;
                let draw_ctx = LuaDrawContext {
                    painter,
                    game_rect,
                    scale,
                };
                self.lua_state.call_draw_callback(&draw_ctx);
            }
        });

        // Lua console window
        if self.show_script_console {
            self.script_console.show(ctx, &mut self.lua_state, &mut self.show_script_console);
        }
    }
}
```

### Lua API Reference

```lua
-- Memory API
memory.readByte(addr)       -- Read byte from CPU memory
memory.readWord(addr)       -- Read 16-bit word
memory.writeByte(addr, val) -- Write byte to CPU memory
memory.readRange(addr, len) -- Read range of bytes

-- Register API
emu.getRegA()     -- Get accumulator
emu.getRegX()     -- Get X register
emu.getRegY()     -- Get Y register
emu.getRegPC()    -- Get program counter
emu.getRegSP()    -- Get stack pointer
emu.getRegP()     -- Get status flags

-- Input API
input.get(player)           -- Get current input state
input.set(player, buttons)  -- Override input

-- Callback API
emu.registerFrame(function() ... end)      -- Called each frame
emu.registerScanline(line, function() ... end)
emu.registerExec(addr, function() ... end) -- Breakpoint callback
emu.registerRead(addr, function() ... end)
emu.registerWrite(addr, function() ... end)

-- Drawing API (egui painter)
gui.drawRect(x, y, w, h, color)
gui.fillRect(x, y, w, h, color)
gui.drawLine(x1, y1, x2, y2, color)
gui.drawCircle(cx, cy, r, color)
gui.text(x, y, text, color)

-- Utility
print(...)         -- Print to console
emu.frameCount()   -- Get current frame number
emu.pause()        -- Pause emulation
emu.unpause()      -- Resume emulation
```

## Acceptance Criteria

- [ ] Can read/write RAM from Lua
- [ ] Frame callbacks work at 60 Hz
- [ ] Drawing primitives render correctly via egui painter
- [ ] <5% performance overhead
- [ ] Script console supports history navigation
- [ ] Scripts can be loaded from file
- [ ] Example scripts work (hitbox viewer, bot AI)

## Dependencies

- Core emulation stable
- Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)

### Crate Dependencies

```toml
# Integration into rustynes-core and rustynes-desktop

[dependencies]
mlua = { version = "0.10", features = ["lua54", "vendored", "async"] }
```

---

## Example Scripts

### Hitbox Viewer (hitboxes.lua)

```lua
-- Display sprite hitboxes for debugging

function onFrame()
    -- Read OAM for sprite positions
    for i = 0, 63 do
        local base = 0x200 + (i * 4)
        local y = memory.readByte(base)
        local tile = memory.readByte(base + 1)
        local attr = memory.readByte(base + 2)
        local x = memory.readByte(base + 3)

        -- Skip hidden sprites
        if y < 0xEF then
            -- Draw hitbox
            gui.drawRect(x, y, 8, 8, 0xFF00FF00) -- Green
        end
    end
end

emu.registerFrame(onFrame)
print("Hitbox viewer loaded")
```

### Input Display (input_display.lua)

```lua
-- Show controller input on screen

local buttons = {"A", "B", "Sel", "Sta", "U", "D", "L", "R"}

function onFrame()
    local state = input.get(1)
    local x, y = 10, 220

    for i, name in ipairs(buttons) do
        local pressed = (state & (1 << (i-1))) ~= 0
        local color = pressed and 0xFFFFFFFF or 0xFF444444
        gui.text(x + (i-1) * 16, y, name, color)
    end
end

emu.registerFrame(onFrame)
print("Input display loaded")
```

---

## Future Planning

*Detailed tasks to be created when milestone begins.*

---

**Last Updated:** 2025-12-28
**Milestone Status:** PLANNED
**Prerequisites:** Phase 1.5 Complete (eframe 0.33 + egui 0.33 frontend)
**Next Milestone:** M10 (Advanced Debugger)
