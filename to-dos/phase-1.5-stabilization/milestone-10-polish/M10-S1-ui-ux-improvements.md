# M10 Sprint 1: UI/UX Improvements

**Sprint:** M10-S1 (UI/UX Improvements)
**Phase:** 1.5 (Stabilization & Accuracy)
**Duration:** ~2-3 days
**Status:** COMPLETE
**Prerequisites:** M10-S0 Complete
**Completed:** 2025-12-28
**Updated:** 2025-12-28

---

## Overview

Polish the desktop GUI with responsive design, theme support, improved settings organization, and visual feedback to create an intuitive and professional user experience. Leverage new egui 0.33 features (Atoms, Modal dialogs, Plugin trait) for enhanced UI components.

## Current Implementation (Post M10-S0)

The desktop frontend uses eframe 0.33 + egui 0.33 with the following existing features:

**Completed (v0.7.1 + M10-S0):**
- [x] eframe 0.33 window management with OpenGL (glow) backend
- [x] egui 0.33 immediate mode GUI
- [x] Menu bar (File, Emulation, Video, Audio, Debug, Help)
- [x] Native file dialogs via rfd 0.15
- [x] Configuration persistence with ron 0.12 format
- [x] Basic scaling modes: PixelPerfect, FitWindow, Integer
- [x] Keyboard input handling via egui
- [x] Gamepad support via gilrs 0.11
- [x] Rust 2024 Edition with modern patterns
- [x] MSRV 1.88

**New egui 0.33 Features Available:**
- **Atoms:** Indivisible UI building blocks for status displays
- **Modal Dialogs:** Native `egui::Modal` for alerts, confirmations, first-run
- **Plugin Trait:** Cleaner debug window organization
- **Popup Rewrite:** Improved menu close-on-click behavior
- **egui_kittest:** UI automation testing framework
- **Crisper Text:** Enhanced font rendering (default 13.0pt)
- **`viewport_rect`/`content_rect`:** Replaces deprecated `screen_rect`

**Location:** `crates/rustynes-desktop/src/`

## Objectives

- [x] Implement responsive layout using `viewport_rect`/`content_rect` (adapt to window size)
- [x] Add theme support (light/dark mode via egui Visuals)
- [x] Improve settings UI (organized tabs, intuitive controls)
- [x] Add visual feedback using Atoms and Spinner (loading states, status displays)
- [x] Implement Modal dialogs for error handling and first-run experience
- [x] Polish animations and transitions (status message fade)
- [x] Improve accessibility (keyboard navigation, keyboard shortcuts)
- [ ] Evaluate egui_kittest for UI automation testing (deferred to future sprint)

## Tasks

### Task 1: Responsive Layout (egui 0.33)
- [x] Implement window size constraints (min 800x600, max 4K)
- [x] Use `ctx.viewport_rect()` and `ctx.content_rect()` for layout calculations (replaces deprecated `screen_rect`)
- [x] Adapt UI elements to window size (scale fonts, spacing)
- [x] Test with different aspect ratios (4:3, 16:9, 21:9)
- [x] Handle window resize events (smooth transitions)
- [x] Optimize for common resolutions (1080p, 1440p, 4K)

### Task 2: Theme Support
- [x] Implement theme system (light/dark mode)
- [x] Design light theme colors (background, text, accents)
- [x] Design dark theme colors (background, text, accents)
- [x] Add theme switcher in settings (dropdown or toggle)
- [x] Persist theme preference (save in config file)
- [x] Support system theme detection (follow OS preference)

### Task 3: Settings Organization
- [x] Organize settings into tabs (Video, Audio, Input, Advanced)
- [x] Video tab: Resolution, scale, filters, vsync, fullscreen
- [x] Audio tab: Volume, sample rate, buffer size, channels
- [x] Input tab: Keyboard mapping, controller mapping, auto-detect
- [x] Advanced tab: Debug options, logging, performance metrics
- [x] Add tooltips for complex settings

### Task 4: Visual Feedback (egui 0.33 Atoms)
- [x] Use Atoms for status bar displays (FPS, audio state, ROM info)
- [x] Add loading spinner via `egui::Spinner` (ROM loading, save state loading)
- [ ] Add progress bar (long operations, ROM scanning) - deferred
- [x] Add status messages with Atoms (bottom status bar: "ROM loaded", "Save state created")
- [x] Add error indicators (red text, error icons via Atoms)
- [x] Add success indicators (green text, checkmarks via Atoms)
- [x] Add hover effects (buttons, tabs, menu items)

### Task 4b: Modal Dialogs (egui 0.33)
- [x] Implement first-run welcome modal using `egui::Modal`
- [x] Add error modal for ROM loading failures
- [x] Add confirmation modal for destructive actions (reset, close without save)
- [x] Add about/help modal with version info
- [x] Test modal interactions (Esc to close, click outside behavior)

### Task 5: Animations & Transitions
- [x] Smooth fade in/out transitions (dialogs, modals)
- [x] Button press animations (scale, color change) - via egui defaults
- [x] Tab switching animations (slide, fade) - via egui defaults
- [x] Menu open/close animations (expand, collapse) - via egui defaults
- [x] Loading spinner animation (rotate, pulse)
- [x] Ensure animations are performant (60 FPS)

### Task 6: Accessibility
- [x] Add keyboard navigation (Tab, Enter, Arrow keys) - via egui defaults
- [x] Add keyboard shortcuts (Ctrl+O: Open, Ctrl+R: Reset, Ctrl+P: Pause, Ctrl+Q: Quit, Ctrl+,: Settings)
- [ ] Add screen reader support (ARIA labels, accessible descriptions) - deferred
- [ ] Add high contrast mode (for low vision users) - deferred
- [ ] Test with assistive technologies - deferred
- [x] Document keyboard shortcuts (in-app help, user guide)

### Task 7: UI Testing (egui_kittest)
- [ ] Evaluate egui_kittest for automated UI testing - deferred to future sprint
- [ ] Create snapshot tests for main UI states - deferred
- [ ] Test menu navigation programmatically - deferred
- [ ] Test settings dialog interactions - deferred
- [ ] Add UI regression tests to CI pipeline - deferred

## Design Mockups

### Light Theme

```
┌────────────────────────────────────────────────┐
│ RustyNES                        [_] [□] [X]    │
├────────────────────────────────────────────────┤
│ File  Emulation  View  Help                    │
├────────────────────────────────────────────────┤
│                                                 │
│                                                 │
│               [NES Screen Area]                │
│               (256x240 scaled)                 │
│                                                 │
│                                                 │
├────────────────────────────────────────────────┤
│ Status: ROM loaded | FPS: 60 | Audio: 48kHz   │
└────────────────────────────────────────────────┘
```

### Dark Theme

```
┌────────────────────────────────────────────────┐
│ RustyNES                        [_] [□] [X]    │
├────────────────────────────────────────────────┤
│ File  Emulation  View  Help                    │
├────────────────────────────────────────────────┤
│                                                 │
│                                                 │
│               [NES Screen Area]                │
│               (256x240 scaled)                 │
│                                                 │
│                                                 │
├────────────────────────────────────────────────┤
│ Status: ROM loaded | FPS: 60 | Audio: 48kHz   │
└────────────────────────────────────────────────┘
```

### Settings Dialog

```
┌─────────── Settings ────────────┐
│ [Video] [Audio] [Input] [Advanced]│
│                                   │
│ Video Settings:                  │
│                                   │
│ Scale: [2x ▼]                    │
│ Filter: [None ▼]                 │
│ VSync: [✓] Enable                │
│ Fullscreen: [ ] Enable           │
│                                   │
│          [Apply] [Cancel]        │
└──────────────────────────────────┘
```

## Implementation Details

### Responsive Layout (egui 0.33)

```rust
// crates/rustynes-desktop/src/app.rs
impl eframe::App for NesApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| { /* ... */ });
                ui.menu_button("Emulation", |ui| { /* ... */ });
            });
        });

        // Bottom status bar with Atoms for status displays
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_message);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("FPS: {:.1}", self.fps));
                });
            });
        });

        // Central panel (emulator screen) - responsive sizing
        egui::CentralPanel::default().show(ctx, |ui| {
            let available_size = ui.available_size();

            // Calculate scaled size maintaining aspect ratio
            let (width, height) = self.calculate_scaled_size(available_size);

            if let Some(texture) = &self.nes_texture {
                let image = egui::Image::new(texture)
                    .fit_to_exact_size(egui::vec2(width, height));
                ui.centered_and_justified(|ui| {
                    ui.add(image);
                });
            }
        });
    }
}

// Window size constraints (egui 0.33)
fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_min_inner_size([800.0, 600.0])
            .with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };
    // Note: eframe 0.33 returns Result
    eframe::run_native("RustyNES", options, Box::new(|cc| Ok(Box::new(NesApp::new(cc)))))
        .expect("Failed to run eframe");
}

// Get viewport dimensions (egui 0.33 - replaces deprecated screen_rect)
fn get_viewport_size(ctx: &egui::Context) -> egui::Vec2 {
    // Use viewport_rect instead of deprecated screen_rect
    ctx.input(|i| i.viewport().outer_rect_pixels)
        .map(|r| egui::vec2(r.width() as f32, r.height() as f32))
        .unwrap_or(egui::vec2(800.0, 600.0))
}
```

### Theme Support (egui)

```rust
// crates/rustynes-desktop/src/config.rs
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AppTheme {
    Light,
    Dark,
    System, // Follow OS preference
}

impl Default for AppTheme {
    fn default() -> Self {
        AppTheme::Dark
    }
}

// crates/rustynes-desktop/src/app.rs
impl NesApp {
    fn apply_theme(&self, ctx: &egui::Context) {
        match self.config.video.theme {
            AppTheme::Light => {
                ctx.set_visuals(egui::Visuals::light());
            }
            AppTheme::Dark => {
                ctx.set_visuals(egui::Visuals::dark());
            }
            AppTheme::System => {
                // Follow system theme (eframe detects automatically)
                // Or check manually via dark_mode detection
            }
        }
    }

    fn render_theme_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Theme:");
            egui::ComboBox::from_id_salt("theme_selector")
                .selected_text(format!("{:?}", self.config.video.theme))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.config.video.theme, AppTheme::Light, "Light");
                    ui.selectable_value(&mut self.config.video.theme, AppTheme::Dark, "Dark");
                    ui.selectable_value(&mut self.config.video.theme, AppTheme::System, "System");
                });
        });
    }
}
```

### Loading Spinner (egui 0.33)

```rust
fn render_loading_overlay(&self, ctx: &egui::Context) {
    if self.loading {
        egui::Area::new(egui::Id::new("loading_overlay"))
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                // Use content_rect (egui 0.33) instead of deprecated screen_rect
                let content_rect = ui.ctx().available_rect();

                // Semi-transparent background
                ui.painter().rect_filled(
                    content_rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
                );

                // Centered loading indicator
                egui::Area::new(egui::Id::new("loading_content"))
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add(egui::Spinner::new().size(50.0));
                            ui.add_space(16.0);
                            ui.label(
                                egui::RichText::new("Loading ROM...")
                                    .size(18.0)
                                    .color(egui::Color32::WHITE)
                            );
                        });
                    });
            });
    }
}
```

### Modal Dialogs (egui 0.33)

```rust
// First-run welcome modal using egui::Modal (new in 0.30+)
fn render_welcome_modal(&mut self, ctx: &egui::Context) {
    if self.show_welcome {
        egui::Modal::new("welcome_modal".into()).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Welcome to RustyNES!");
            ui.add_space(8.0);
            ui.label("A high-accuracy NES emulator written in Rust.");
            ui.add_space(16.0);

            ui.label("Quick Start:");
            ui.label("1. Press Ctrl+O to open a ROM file");
            ui.label("2. Use Arrow keys + Z/X for controls");
            ui.label("3. Press F11 for fullscreen");

            ui.add_space(16.0);
            if ui.button("Get Started").clicked() {
                self.show_welcome = false;
                self.config.first_run = false;
                self.save_config();
            }
        });
    }
}

// Error modal for ROM loading failures
fn render_error_modal(&mut self, ctx: &egui::Context) {
    if let Some(error_msg) = &self.error_message {
        let error_msg = error_msg.clone();
        egui::Modal::new("error_modal".into()).show(ctx, |ui| {
            ui.set_width(350.0);
            ui.heading(egui::RichText::new("Error").color(egui::Color32::RED));
            ui.add_space(8.0);
            ui.label(&error_msg);
            ui.add_space(16.0);
            if ui.button("OK").clicked() {
                self.error_message = None;
            }
        });
    }
}

// Confirmation modal for destructive actions
fn render_confirm_modal(&mut self, ctx: &egui::Context) {
    if self.confirm_action.is_some() {
        egui::Modal::new("confirm_modal".into()).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading("Confirm Action");
            ui.add_space(8.0);
            ui.label("Are you sure you want to proceed?");
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    self.execute_confirm_action();
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_action = None;
                }
            });
        });
    }
}
```

### Settings Window with Tabs (egui)

```rust
fn render_settings_window(&mut self, ctx: &egui::Context) {
    if self.show_settings {
        egui::Window::new("Settings")
            .open(&mut self.show_settings)
            .resizable(true)
            .default_size([500.0, 400.0])
            .show(ctx, |ui| {
                // Tab bar
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Video, "Video");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Audio, "Audio");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Input, "Input");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Advanced, "Advanced");
                });

                ui.separator();

                // Tab content
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match self.settings_tab {
                        SettingsTab::Video => self.render_video_settings(ui),
                        SettingsTab::Audio => self.render_audio_settings(ui),
                        SettingsTab::Input => self.render_input_settings(ui),
                        SettingsTab::Advanced => self.render_advanced_settings(ui),
                    }
                });

                ui.separator();

                // Action buttons
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        self.apply_settings();
                    }
                    if ui.button("Reset to Defaults").clicked() {
                        self.config = Config::default();
                    }
                });
            });
    }
}

fn render_video_settings(&mut self, ui: &mut egui::Ui) {
    ui.heading("Video Settings");
    ui.add_space(8.0);

    // Scale factor
    ui.horizontal(|ui| {
        ui.label("Scale:");
        egui::ComboBox::from_id_salt("scale")
            .selected_text(format!("{}x", self.config.video.scale))
            .show_ui(ui, |ui| {
                for scale in 1..=4 {
                    ui.selectable_value(&mut self.config.video.scale, scale, format!("{}x", scale));
                }
            });
    });

    // Scaling mode
    ui.horizontal(|ui| {
        ui.label("Scaling Mode:");
        egui::ComboBox::from_id_salt("scaling_mode")
            .selected_text(format!("{:?}", self.config.video.scaling_mode))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.config.video.scaling_mode, ScalingMode::PixelPerfect, "Pixel Perfect (8:7 PAR)");
                ui.selectable_value(&mut self.config.video.scaling_mode, ScalingMode::FitWindow, "Fit Window");
                ui.selectable_value(&mut self.config.video.scaling_mode, ScalingMode::Integer, "Integer Scaling");
            });
    });

    // VSync
    ui.checkbox(&mut self.config.video.vsync, "Enable VSync");

    // Theme
    self.render_theme_selector(ui);
}

fn render_audio_settings(&mut self, ui: &mut egui::Ui) {
    ui.heading("Audio Settings");
    ui.add_space(8.0);

    // Volume slider
    ui.horizontal(|ui| {
        ui.label("Volume:");
        ui.add(egui::Slider::new(&mut self.config.audio.volume, 0.0..=1.0)
            .show_value(true)
            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
    });

    // Mute checkbox
    ui.checkbox(&mut self.config.audio.muted, "Mute Audio");

    // Sample rate
    ui.horizontal(|ui| {
        ui.label("Sample Rate:");
        egui::ComboBox::from_id_salt("sample_rate")
            .selected_text(format!("{} Hz", self.config.audio.sample_rate))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.config.audio.sample_rate, 44100, "44100 Hz");
                ui.selectable_value(&mut self.config.audio.sample_rate, 48000, "48000 Hz");
            });
    });

    // Buffer size
    ui.horizontal(|ui| {
        ui.label("Buffer Size:");
        egui::ComboBox::from_id_salt("buffer_size")
            .selected_text(format!("{} samples", self.config.audio.buffer_size))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.config.audio.buffer_size, 1024, "1024 (Low latency)");
                ui.selectable_value(&mut self.config.audio.buffer_size, 2048, "2048 (Balanced)");
                ui.selectable_value(&mut self.config.audio.buffer_size, 4096, "4096 (High stability)");
            });
    });
}
```

### Visual Feedback Patterns (egui)

```rust
// Progress bar for long operations
fn render_progress(&self, ui: &mut egui::Ui, progress: f32, label: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::ProgressBar::new(progress).show_percentage());
    });
}

// Status message with auto-fade
struct StatusMessage {
    text: String,
    color: egui::Color32,
    created_at: std::time::Instant,
    duration: std::time::Duration,
}

fn render_status_bar(&mut self, ui: &mut egui::Ui) {
    if let Some(ref status) = self.status_message {
        let elapsed = status.created_at.elapsed();
        if elapsed < status.duration {
            // Fade out effect
            let alpha = 1.0 - (elapsed.as_secs_f32() / status.duration.as_secs_f32());
            let color = egui::Color32::from_rgba_unmultiplied(
                status.color.r(),
                status.color.g(),
                status.color.b(),
                (alpha * 255.0) as u8,
            );
            ui.colored_label(color, &status.text);
        } else {
            self.status_message = None;
        }
    }
}

// Hover tooltips
fn render_setting_with_tooltip(&mut self, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut self.config.video.vsync, "VSync")
            .on_hover_text("Synchronize frame rate with monitor refresh rate to prevent screen tearing");
    });
}

// Success/Error indicators
fn show_notification(&mut self, message: &str, is_error: bool) {
    self.status_message = Some(StatusMessage {
        text: message.to_string(),
        color: if is_error {
            egui::Color32::from_rgb(255, 100, 100)
        } else {
            egui::Color32::from_rgb(100, 200, 100)
        },
        created_at: std::time::Instant::now(),
        duration: std::time::Duration::from_secs(3),
    });
}
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| **Ctrl+O** | Open ROM |
| **Ctrl+R** | Reset (Soft) |
| **Ctrl+Shift+R** | Reset (Hard) |
| **Ctrl+S** | Save State |
| **Ctrl+L** | Load State |
| **F11** | Toggle Fullscreen |
| **Ctrl+Q** | Quit |
| **Ctrl+,** | Settings |
| **Ctrl+P** | Pause/Resume |
| **Esc** | Close Dialog/Exit Fullscreen |

## User Experience Testing

| Scenario | Expected Behavior |
|----------|-------------------|
| **First Launch** | Welcome screen, prompt to load ROM |
| **Load ROM** | Loading spinner, then emulator screen |
| **Window Resize** | Smooth scaling, maintain aspect ratio |
| **Theme Switch** | Instant theme change, persist preference |
| **Settings Change** | Apply immediately or on dialog close |
| **Error (Invalid ROM)** | Clear error message, recovery options |
| **Keyboard Navigation** | Tab through UI, Enter to activate |
| **Controller Hotplug** | Detect and notify user |

## Acceptance Criteria

- [ ] Responsive layout implemented using `viewport_rect`/`content_rect` (800x600 to 4K)
- [ ] Theme support working (light/dark mode via egui Visuals)
- [ ] Settings organized into tabs
- [ ] Visual feedback using Atoms and Spinner for status displays
- [ ] Modal dialogs for errors, confirmations, first-run experience
- [ ] Smooth animations and transitions
- [ ] Keyboard navigation and shortcuts working
- [ ] Accessibility features implemented
- [ ] Tested on Linux, macOS, Windows
- [ ] User testing feedback incorporated
- [ ] Optional: egui_kittest UI automation tests

## Version Target

v0.9.0 / v1.0.0-alpha.1

---

## References

### egui 0.33 Documentation

- [egui::Modal](https://docs.rs/egui/0.33/egui/containers/modal/index.html) - Modal dialog support
- [egui_kittest](https://docs.rs/egui_kittest/) - UI automation testing
- [egui Visuals](https://docs.rs/egui/0.33/egui/style/struct.Visuals.html) - Theme customization
- [egui Migration Guide](https://github.com/emilk/egui/releases) - API changes

### Key API Changes (egui 0.29 -> 0.33)

| Old API | New API | Notes |
|---------|---------|-------|
| `ctx.screen_rect()` | `ctx.available_rect()` or `viewport_rect` | Deprecated |
| Manual modals | `egui::Modal::new().show()` | New in 0.30 |
| `on_begin_pass/on_end_pass` | Plugin trait | New in 0.33 |
| Default text 12.5pt | Default text 13.0pt | Visual change |
| Menu stays open | Menu closes on click | Behavioral change |

---

**Status:** Pending
**Blocks:** M10-S2 (Documentation)
**Last Updated:** 2025-12-28
