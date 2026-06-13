# Sprint M6-S5: Configuration & Polish

**Status:** ⏳ PENDING
**Sprint:** 6.5 (Configuration & Polish)
**Milestone:** M6 (Desktop GUI)
**Estimated Duration:** ~10-12 hours
**Target Files:** `crates/rustynes-desktop/src/config.rs`, `ui/settings.rs`, `ui/dialogs.rs`

---

## Overview

Sprint M6-S5 finalizes the desktop GUI with a comprehensive configuration system, polished user interface elements, and cross-platform packaging. This sprint delivers a complete, user-friendly desktop application that persists settings across sessions and provides professional error handling and dialogs.

**Goals:**

1. Implement TOML-based configuration file with automatic save/load
2. Create comprehensive Settings window with tabbed interface
3. Build Recent ROMs list with quick access
4. Design About window and error dialogs
5. Add application icon and proper window metadata
6. Set up cross-platform packaging scripts

**Dependencies:**

- M6-S1 (egui application structure)
- M6-S2 (wgpu rendering backend)
- M6-S3 (audio output)
- M6-S4 (controller support)

---

## Tasks

### Task 1: Configuration File Format & Persistence

**Duration:** ~2 hours

Implement TOML-based configuration system with automatic persistence.

#### 1.1 Create Config Structure

**File:** `crates/rustynes-desktop/src/config.rs`

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Emulation settings
    pub emulation: EmulationConfig,

    /// Video/rendering settings
    pub video: VideoConfig,

    /// Audio settings
    pub audio: AudioConfig,

    /// Input mappings
    pub input: InputConfig,

    /// Advanced settings
    pub advanced: AdvancedConfig,

    /// Application settings
    pub app: ApplicationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulationConfig {
    /// Emulation speed multiplier (1.0 = 60 FPS)
    pub speed: f32,

    /// Region (NTSC/PAL)
    pub region: Region,

    /// Accuracy level
    pub accuracy: AccuracyLevel,

    /// Emulate DMC/controller conflicts
    pub dmc_conflicts: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Region {
    NTSC, // 60.0988 Hz
    PAL,  // 50.0070 Hz
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AccuracyLevel {
    CycleAccurate,
    ScanlineAccurate,
    FrameAccurate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    /// Display scale factor
    pub scale: u32,

    /// Scaling filter
    pub filter: ScalingFilter,

    /// Aspect ratio mode
    pub aspect_ratio: AspectRatio,

    /// VSync enabled
    pub vsync: bool,

    /// Palette selection
    pub palette: Palette,

    /// Sprite limit (8 per scanline)
    pub sprite_limit: bool,

    /// Overscan cropping (pixels)
    pub overscan: OverscanConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ScalingFilter {
    Nearest,
    Linear,
    CrtShader,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AspectRatio {
    PixelPerfect,  // 256x240 (8:7 PAR)
    Standard,      // 4:3
    Stretch,       // Fill window
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Palette {
    Default,
    FCEUX,
    Smooth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverscanConfig {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Audio enabled
    pub enabled: bool,

    /// Sample rate (Hz)
    pub sample_rate: u32,

    /// Master volume (0.0-1.0)
    pub volume: f32,

    /// Low-pass filter cutoff (Hz)
    pub lowpass_filter: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// Player 1 keyboard mapping
    pub keyboard_p1: KeyboardMapping,

    /// Player 2 keyboard mapping
    pub keyboard_p2: KeyboardMapping,

    /// Gamepad mappings
    pub gamepad_mappings: Vec<GamepadMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardMapping {
    pub a: String,
    pub b: String,
    pub select: String,
    pub start: String,
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamepadMapping {
    pub device_name: String,
    pub a: u32,
    pub b: u32,
    pub select: u32,
    pub start: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedConfig {
    /// Rewind buffer enabled
    pub rewind_enabled: bool,

    /// Rewind buffer size (frames)
    pub rewind_buffer_size: usize,

    /// Compress save states
    pub compress_savestates: bool,

    /// Fast-forward speed cap (0 = unlimited)
    pub fast_forward_cap: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationConfig {
    /// Recent ROMs list (max 10)
    pub recent_roms: Vec<PathBuf>,

    /// Default ROM directory
    pub rom_directory: Option<PathBuf>,

    /// Save states directory
    pub savestate_directory: Option<PathBuf>,

    /// Window width
    pub window_width: u32,

    /// Window height
    pub window_height: u32,

    /// Fullscreen mode
    pub fullscreen: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            emulation: EmulationConfig {
                speed: 1.0,
                region: Region::NTSC,
                accuracy: AccuracyLevel::CycleAccurate,
                dmc_conflicts: true,
            },
            video: VideoConfig {
                scale: 3,
                filter: ScalingFilter::Nearest,
                aspect_ratio: AspectRatio::PixelPerfect,
                vsync: true,
                palette: Palette::Default,
                sprite_limit: true,
                overscan: OverscanConfig {
                    top: 0,
                    bottom: 0,
                    left: 0,
                    right: 0,
                },
            },
            audio: AudioConfig {
                enabled: true,
                sample_rate: 48000,
                volume: 0.5,
                lowpass_filter: Some(14000.0),
            },
            input: InputConfig {
                keyboard_p1: KeyboardMapping {
                    a: "Z".to_string(),
                    b: "X".to_string(),
                    select: "RShift".to_string(),
                    start: "Return".to_string(),
                    up: "Up".to_string(),
                    down: "Down".to_string(),
                    left: "Left".to_string(),
                    right: "Right".to_string(),
                },
                keyboard_p2: KeyboardMapping {
                    a: "Numpad1".to_string(),
                    b: "Numpad2".to_string(),
                    select: "Numpad3".to_string(),
                    start: "Numpad4".to_string(),
                    up: "Numpad8".to_string(),
                    down: "Numpad5".to_string(),
                    left: "Numpad4".to_string(),
                    right: "Numpad6".to_string(),
                },
                gamepad_mappings: Vec::new(),
            },
            advanced: AdvancedConfig {
                rewind_enabled: false,
                rewind_buffer_size: 600, // 10 seconds at 60 FPS
                compress_savestates: true,
                fast_forward_cap: 300, // 5x speed
            },
            app: ApplicationConfig {
                recent_roms: Vec::new(),
                rom_directory: None,
                savestate_directory: None,
                window_width: 768,  // 256 * 3
                window_height: 720, // 240 * 3
                fullscreen: false,
            },
        }
    }
}
```

#### 1.2 Implement Save/Load Functions

```rust
impl AppConfig {
    /// Get default config file path
    pub fn default_path() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            config_dir.join("rustynes").join("config.toml")
        } else {
            PathBuf::from("config.toml")
        }
    }

    /// Load configuration from file
    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::default_path();

        if !path.exists() {
            // Create default config
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let contents = std::fs::read_to_string(&path)
            .map_err(|e| ConfigError::IoError(e))?;

        let config: AppConfig = toml::from_str(&contents)
            .map_err(|e| ConfigError::ParseError(e))?;

        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::default_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::IoError(e))?;
        }

        let toml = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::SerializeError(e))?;

        std::fs::write(&path, toml)
            .map_err(|e| ConfigError::IoError(e))?;

        Ok(())
    }

    /// Add ROM to recent list
    pub fn add_recent_rom(&mut self, path: PathBuf) {
        // Remove if already exists
        self.app.recent_roms.retain(|p| p != &path);

        // Add to front
        self.app.recent_roms.insert(0, path);

        // Limit to 10 entries
        self.app.recent_roms.truncate(10);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Serialize error: {0}")]
    SerializeError(#[from] toml::ser::Error),
}
```

#### 1.3 Integrate with Application State

**File:** `crates/rustynes-desktop/src/app.rs`

```rust
impl RustyNesApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        // Load configuration
        let config = AppConfig::load().unwrap_or_default();

        // Apply window size from config
        let mut app = Self {
            config,
            // ... other fields
        };

        // Auto-save config periodically
        app.setup_autosave();

        app
    }

    fn setup_autosave(&mut self) {
        // Save config every 30 seconds
        // (implementation depends on async/threading strategy)
    }
}

impl Drop for RustyNesApp {
    fn drop(&mut self) {
        // Save config on exit
        let _ = self.config.save();
    }
}
```

---

### Task 2: Settings Window UI

**Duration:** ~3-4 hours

Create comprehensive Settings window with tabbed interface.

#### 2.1 Settings Window Structure

**File:** `crates/rustynes-desktop/src/ui/settings.rs`

```rust
use eframe::egui;
use super::AppConfig;

pub struct SettingsWindow {
    open: bool,
    selected_tab: SettingsTab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsTab {
    Video,
    Audio,
    Input,
    Advanced,
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self {
            open: false,
            selected_tab: SettingsTab::Video,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, config: &mut AppConfig) {
        if !self.open {
            return;
        }

        egui::Window::new("Settings")
            .open(&mut self.open)
            .default_size([600.0, 400.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Tab buttons
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Video, "Video");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Audio, "Audio");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Input, "Input");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Advanced, "Advanced");
                });

                ui.separator();

                // Tab content
                match self.selected_tab {
                    SettingsTab::Video => self.show_video_settings(ui, &mut config.video),
                    SettingsTab::Audio => self.show_audio_settings(ui, &mut config.audio),
                    SettingsTab::Input => self.show_input_settings(ui, &mut config.input),
                    SettingsTab::Advanced => self.show_advanced_settings(ui, &mut config.advanced),
                }

                ui.separator();

                // Bottom buttons
                ui.horizontal(|ui| {
                    if ui.button("Reset to Defaults").clicked() {
                        *config = AppConfig::default();
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.open = false;
                        }
                    });
                });
            });
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}
```

#### 2.2 Video Settings Tab

```rust
impl SettingsWindow {
    fn show_video_settings(&mut self, ui: &mut egui::Ui, config: &mut VideoConfig) {
        egui::Grid::new("video_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                // Scale
                ui.label("Scale:");
                ui.add(egui::Slider::new(&mut config.scale, 1..=6).text("x"));
                ui.end_row();

                // Filter
                ui.label("Scaling Filter:");
                egui::ComboBox::from_id_source("filter")
                    .selected_text(format!("{:?}", config.filter))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.filter, ScalingFilter::Nearest, "Nearest (Sharp)");
                        ui.selectable_value(&mut config.filter, ScalingFilter::Linear, "Linear (Smooth)");
                        ui.selectable_value(&mut config.filter, ScalingFilter::CrtShader, "CRT Shader");
                    });
                ui.end_row();

                // Aspect Ratio
                ui.label("Aspect Ratio:");
                egui::ComboBox::from_id_source("aspect")
                    .selected_text(format!("{:?}", config.aspect_ratio))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.aspect_ratio, AspectRatio::PixelPerfect, "Pixel Perfect (8:7)");
                        ui.selectable_value(&mut config.aspect_ratio, AspectRatio::Standard, "Standard (4:3)");
                        ui.selectable_value(&mut config.aspect_ratio, AspectRatio::Stretch, "Stretch");
                    });
                ui.end_row();

                // VSync
                ui.label("VSync:");
                ui.checkbox(&mut config.vsync, "Enabled");
                ui.end_row();

                // Palette
                ui.label("Palette:");
                egui::ComboBox::from_id_source("palette")
                    .selected_text(format!("{:?}", config.palette))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.palette, Palette::Default, "Default");
                        ui.selectable_value(&mut config.palette, Palette::FCEUX, "FCEUX");
                        ui.selectable_value(&mut config.palette, Palette::Smooth, "Smooth");
                    });
                ui.end_row();

                // Sprite Limit
                ui.label("Sprite Limit:");
                ui.checkbox(&mut config.sprite_limit, "8 per scanline (authentic)");
                ui.end_row();
            });

        ui.separator();
        ui.heading("Overscan Cropping");

        egui::Grid::new("overscan_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Top:");
                ui.add(egui::Slider::new(&mut config.overscan.top, 0..=16).text("px"));
                ui.end_row();

                ui.label("Bottom:");
                ui.add(egui::Slider::new(&mut config.overscan.bottom, 0..=16).text("px"));
                ui.end_row();

                ui.label("Left:");
                ui.add(egui::Slider::new(&mut config.overscan.left, 0..=16).text("px"));
                ui.end_row();

                ui.label("Right:");
                ui.add(egui::Slider::new(&mut config.overscan.right, 0..=16).text("px"));
                ui.end_row();
            });
    }
}
```

#### 2.3 Audio Settings Tab

```rust
impl SettingsWindow {
    fn show_audio_settings(&mut self, ui: &mut egui::Ui, config: &mut AudioConfig) {
        egui::Grid::new("audio_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                // Enabled
                ui.label("Audio Output:");
                ui.checkbox(&mut config.enabled, "Enabled");
                ui.end_row();

                // Sample Rate
                ui.label("Sample Rate:");
                egui::ComboBox::from_id_source("sample_rate")
                    .selected_text(format!("{} Hz", config.sample_rate))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.sample_rate, 44100, "44100 Hz");
                        ui.selectable_value(&mut config.sample_rate, 48000, "48000 Hz");
                        ui.selectable_value(&mut config.sample_rate, 96000, "96000 Hz");
                    });
                ui.end_row();

                // Volume
                ui.label("Master Volume:");
                ui.add(egui::Slider::new(&mut config.volume, 0.0..=1.0)
                    .text(format!("{:.0}%", config.volume * 100.0)));
                ui.end_row();

                // Low-pass Filter
                ui.label("Low-Pass Filter:");
                let mut enabled = config.lowpass_filter.is_some();
                ui.checkbox(&mut enabled, "Enabled");
                if enabled && config.lowpass_filter.is_none() {
                    config.lowpass_filter = Some(14000.0);
                } else if !enabled {
                    config.lowpass_filter = None;
                }
                ui.end_row();

                if let Some(cutoff) = &mut config.lowpass_filter {
                    ui.label("  Cutoff Frequency:");
                    ui.add(egui::Slider::new(cutoff, 1000.0..=20000.0)
                        .logarithmic(true)
                        .text("Hz"));
                    ui.end_row();
                }
            });
    }
}
```

#### 2.4 Input Settings Tab

```rust
impl SettingsWindow {
    fn show_input_settings(&mut self, ui: &mut egui::Ui, config: &mut InputConfig) {
        ui.heading("Player 1 Keyboard Mapping");

        egui::Grid::new("p1_keyboard")
            .num_columns(2)
            .show(ui, |ui| {
                Self::key_mapping_row(ui, "A:", &mut config.keyboard_p1.a);
                Self::key_mapping_row(ui, "B:", &mut config.keyboard_p1.b);
                Self::key_mapping_row(ui, "Select:", &mut config.keyboard_p1.select);
                Self::key_mapping_row(ui, "Start:", &mut config.keyboard_p1.start);
                Self::key_mapping_row(ui, "Up:", &mut config.keyboard_p1.up);
                Self::key_mapping_row(ui, "Down:", &mut config.keyboard_p1.down);
                Self::key_mapping_row(ui, "Left:", &mut config.keyboard_p1.left);
                Self::key_mapping_row(ui, "Right:", &mut config.keyboard_p1.right);
            });

        ui.separator();
        ui.heading("Player 2 Keyboard Mapping");

        egui::Grid::new("p2_keyboard")
            .num_columns(2)
            .show(ui, |ui| {
                Self::key_mapping_row(ui, "A:", &mut config.keyboard_p2.a);
                Self::key_mapping_row(ui, "B:", &mut config.keyboard_p2.b);
                Self::key_mapping_row(ui, "Select:", &mut config.keyboard_p2.select);
                Self::key_mapping_row(ui, "Start:", &mut config.keyboard_p2.start);
                Self::key_mapping_row(ui, "Up:", &mut config.keyboard_p2.up);
                Self::key_mapping_row(ui, "Down:", &mut config.keyboard_p2.down);
                Self::key_mapping_row(ui, "Left:", &mut config.keyboard_p2.left);
                Self::key_mapping_row(ui, "Right:", &mut config.keyboard_p2.right);
            });

        ui.separator();

        if ui.button("Reset to Defaults").clicked() {
            config.keyboard_p1 = KeyboardMapping::default();
            config.keyboard_p2 = KeyboardMapping::default();
        }
    }

    fn key_mapping_row(ui: &mut egui::Ui, label: &str, key: &mut String) {
        ui.label(label);
        ui.text_edit_singleline(key);
        ui.end_row();
    }
}
```

#### 2.5 Advanced Settings Tab

```rust
impl SettingsWindow {
    fn show_advanced_settings(&mut self, ui: &mut egui::Ui, config: &mut AdvancedConfig) {
        egui::Grid::new("advanced_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                // Rewind
                ui.label("Rewind:");
                ui.checkbox(&mut config.rewind_enabled, "Enabled");
                ui.end_row();

                if config.rewind_enabled {
                    ui.label("  Buffer Size:");
                    ui.add(egui::Slider::new(&mut config.rewind_buffer_size, 60..=3600)
                        .text(format!("{:.1}s", config.rewind_buffer_size as f32 / 60.0)));
                    ui.end_row();
                }

                // Save State Compression
                ui.label("Save State Compression:");
                ui.checkbox(&mut config.compress_savestates, "Enabled");
                ui.end_row();

                // Fast-Forward Cap
                ui.label("Fast-Forward Cap:");
                if config.fast_forward_cap == 0 {
                    if ui.button("Unlimited").clicked() {
                        config.fast_forward_cap = 300;
                    }
                } else {
                    ui.add(egui::Slider::new(&mut config.fast_forward_cap, 120..=600)
                        .text(format!("{}x", config.fast_forward_cap / 60)));
                }
                ui.end_row();
            });

        ui.separator();

        ui.colored_label(
            egui::Color32::YELLOW,
            "⚠ Advanced settings may impact performance or accuracy."
        );
    }
}
```

---

### Task 3: Recent ROMs List

**Duration:** ~1-2 hours

Implement Recent ROMs menu with quick access.

#### 3.1 Recent ROMs Menu

**File:** `crates/rustynes-desktop/src/ui/menu_bar.rs`

```rust
impl RustyNesApp {
    pub fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open ROM...").clicked() {
                    self.open_rom_dialog();
                    ui.close_menu();
                }

                // Recent ROMs submenu
                ui.menu_button("Recent ROMs", |ui| {
                    if self.config.app.recent_roms.is_empty() {
                        ui.label("(No recent ROMs)");
                    } else {
                        for (i, rom_path) in self.config.app.recent_roms.clone().iter().enumerate() {
                            let label = rom_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Unknown");

                            let shortcut = if i < 9 {
                                format!("  Ctrl+{}", i + 1)
                            } else {
                                String::new()
                            };

                            if ui.button(format!("{}{}", label, shortcut)).clicked() {
                                self.load_rom(rom_path.clone());
                                ui.close_menu();
                            }
                        }

                        ui.separator();

                        if ui.button("Clear Recent List").clicked() {
                            self.config.app.recent_roms.clear();
                            let _ = self.config.save();
                            ui.close_menu();
                        }
                    }
                });

                ui.separator();

                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            // ... other menus
        });
    }
}
```

#### 3.2 Keyboard Shortcuts for Recent ROMs

```rust
impl RustyNesApp {
    pub fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+1 through Ctrl+9 for recent ROMs
        for i in 0..9 {
            let key = match i {
                0 => egui::Key::Num1,
                1 => egui::Key::Num2,
                2 => egui::Key::Num3,
                3 => egui::Key::Num4,
                4 => egui::Key::Num5,
                5 => egui::Key::Num6,
                6 => egui::Key::Num7,
                7 => egui::Key::Num8,
                8 => egui::Key::Num9,
                _ => continue,
            };

            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(key)) {
                if let Some(rom_path) = self.config.app.recent_roms.get(i).cloned() {
                    self.load_rom(rom_path);
                }
            }
        }
    }
}
```

---

### Task 4: About Window & Error Dialogs

**Duration:** ~2 hours

Create About window and comprehensive error dialog system.

#### 4.1 About Window

**File:** `crates/rustynes-desktop/src/ui/dialogs.rs`

```rust
use eframe::egui;

pub struct AboutWindow {
    open: bool,
}

impl AboutWindow {
    pub fn new() -> Self {
        Self { open: false }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }

        egui::Window::new("About RustyNES")
            .open(&mut self.open)
            .resizable(false)
            .default_size([400.0, 300.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("RustyNES");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(10.0);

                    ui.label("A high-accuracy NES emulator written in Rust");
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        ui.label("GitHub:");
                        if ui.link("https://github.com/doublegate/RustyNES").clicked() {
                            let _ = open::that("https://github.com/doublegate/RustyNES");
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();

                    ui.label("Copyright © 2025");
                    ui.label("Licensed under MIT / Apache-2.0");
                    ui.add_space(10.0);

                    ui.collapsing("Credits", |ui| {
                        ui.label("• egui - Emil Ernerfeldt");
                        ui.label("• wgpu - gfx-rs developers");
                        ui.label("• cpal - RustAudio developers");
                        ui.label("• NESdev community");
                    });

                    ui.add_space(10.0);

                    if ui.button("Close").clicked() {
                        self.open = false;
                    }
                });
            });
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }
}
```

#### 4.2 Error Dialog System

```rust
pub struct ErrorDialog {
    message: Option<String>,
    details: Option<String>,
}

impl ErrorDialog {
    pub fn new() -> Self {
        Self {
            message: None,
            details: None,
        }
    }

    pub fn show_error(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
        self.details = None;
    }

    pub fn show_error_with_details(&mut self, message: impl Into<String>, details: impl Into<String>) {
        self.message = Some(message.into());
        self.details = Some(details.into());
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if self.message.is_none() {
            return;
        }

        egui::Window::new("Error")
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⚠").size(32.0).color(egui::Color32::RED));
                    ui.vertical(|ui| {
                        if let Some(msg) = &self.message {
                            ui.label(msg);
                        }
                    });
                });

                if let Some(details) = &self.details {
                    ui.collapsing("Details", |ui| {
                        ui.monospace(details);
                    });
                }

                ui.separator();

                if ui.button("OK").clicked() {
                    self.message = None;
                    self.details = None;
                }
            });
    }
}
```

#### 4.3 Integrate Error Handling

**File:** `crates/rustynes-desktop/src/app.rs`

```rust
impl RustyNesApp {
    fn open_rom_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("NES ROM", &["nes"])
            .pick_file()
        {
            self.load_rom(path);
        }
    }

    fn load_rom(&mut self, path: PathBuf) {
        match self.try_load_rom(&path) {
            Ok(()) => {
                self.config.add_recent_rom(path);
                let _ = self.config.save();
            }
            Err(e) => {
                self.error_dialog.show_error_with_details(
                    format!("Failed to load ROM: {}", path.display()),
                    format!("{:?}", e),
                );
            }
        }
    }

    fn try_load_rom(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let rom_data = std::fs::read(path)?;
        let rom = Rom::from_bytes(&rom_data)?;

        self.console = Some(Console::new(rom, self.config.clone())?);

        Ok(())
    }
}
```

---

### Task 5: Application Metadata (Icon, Window Title)

**Duration:** ~1 hour

Add application icon and proper window metadata.

#### 5.1 Set Window Title and Icon

**File:** `crates/rustynes-desktop/src/main.rs`

```rust
use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RustyNES - NES Emulator")
            .with_inner_size([768.0, 720.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "RustyNES",
        options,
        Box::new(|cc| Box::new(RustyNesApp::new(cc))),
    )
}

fn load_icon() -> egui::IconData {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(icon_bytes)
        .expect("Failed to load icon")
        .to_rgba8();

    let (width, height) = image.dimensions();

    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}
```

#### 5.2 Dynamic Window Title

```rust
impl RustyNesApp {
    fn update_window_title(&self, ctx: &egui::Context) {
        let title = if let Some(rom_name) = self.current_rom_name() {
            format!("RustyNES - {}", rom_name)
        } else {
            "RustyNES - NES Emulator".to_string()
        };

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn current_rom_name(&self) -> Option<String> {
        self.current_rom_path.as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }
}
```

#### 5.3 Create Icon Asset

Create a 256x256 PNG icon at `crates/rustynes-desktop/assets/icon.png`. For now, a simple placeholder can be used (NES controller silhouette or "RN" monogram).

---

### Task 6: Cross-Platform Packaging

**Duration:** ~2-3 hours

Set up cross-platform packaging scripts.

#### 6.1 Linux Packaging (AppImage)

**File:** `scripts/package-linux.sh`

```bash
#!/bin/bash
set -e

VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="rustynes-desktop") | .version')

echo "Building RustyNES v${VERSION} for Linux..."

# Build release binary
cargo build --release -p rustynes-desktop

# Create AppDir structure
mkdir -p AppDir/usr/bin
mkdir -p AppDir/usr/share/applications
mkdir -p AppDir/usr/share/icons/hicolor/256x256/apps

# Copy binary
cp target/release/rustynes-desktop AppDir/usr/bin/rustynes

# Create .desktop file
cat > AppDir/usr/share/applications/rustynes.desktop << EOF
[Desktop Entry]
Name=RustyNES
Exec=rustynes
Icon=rustynes
Type=Application
Categories=Game;Emulator;
EOF

# Copy icon
cp crates/rustynes-desktop/assets/icon.png AppDir/usr/share/icons/hicolor/256x256/apps/rustynes.png

# Download appimagetool if not present
if [ ! -f appimagetool-x86_64.AppImage ]; then
    wget https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x appimagetool-x86_64.AppImage
fi

# Build AppImage
./appimagetool-x86_64.AppImage AppDir RustyNES-${VERSION}-x86_64.AppImage

echo "AppImage created: RustyNES-${VERSION}-x86_64.AppImage"
```

#### 6.2 Windows Packaging (Installer)

**File:** `scripts/package-windows.ps1`

```powershell
# Build release
cargo build --release -p rustynes-desktop

# Get version from Cargo.toml
$version = (Select-String -Path "Cargo.toml" -Pattern 'version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value

Write-Host "Packaging RustyNES v$version for Windows..."

# Create distribution directory
New-Item -ItemType Directory -Force -Path "dist/rustynes-$version-windows"

# Copy executable
Copy-Item "target/release/rustynes-desktop.exe" "dist/rustynes-$version-windows/RustyNES.exe"

# Copy README and LICENSE
Copy-Item "README.md" "dist/rustynes-$version-windows/"
Copy-Item "LICENSE-MIT" "dist/rustynes-$version-windows/"
Copy-Item "LICENSE-APACHE" "dist/rustynes-$version-windows/"

# Create ZIP archive
Compress-Archive -Path "dist/rustynes-$version-windows/*" -DestinationPath "RustyNES-$version-windows-x64.zip" -Force

Write-Host "Package created: RustyNES-$version-windows-x64.zip"
```

#### 6.3 macOS Packaging (.app Bundle)

**File:** `scripts/package-macos.sh`

```bash
#!/bin/bash
set -e

VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="rustynes-desktop") | .version')

echo "Building RustyNES v${VERSION} for macOS..."

# Build release binary
cargo build --release -p rustynes-desktop

# Create .app bundle structure
mkdir -p "RustyNES.app/Contents/MacOS"
mkdir -p "RustyNES.app/Contents/Resources"

# Copy binary
cp target/release/rustynes-desktop "RustyNES.app/Contents/MacOS/RustyNES"

# Create Info.plist
cat > "RustyNES.app/Contents/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>RustyNES</string>
    <key>CFBundleDisplayName</key>
    <string>RustyNES</string>
    <key>CFBundleIdentifier</key>
    <string>com.doublegate.rustynes</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>RustyNES</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
</dict>
</plist>
EOF

# Copy icon (convert PNG to ICNS)
# Requires iconutil: sips -s format icns crates/rustynes-desktop/assets/icon.png --out RustyNES.app/Contents/Resources/icon.icns

# Create DMG
hdiutil create -volname "RustyNES ${VERSION}" -srcfolder RustyNES.app -ov -format UDZO "RustyNES-${VERSION}-macOS.dmg"

echo "DMG created: RustyNES-${VERSION}-macOS.dmg"
```

#### 6.4 Update Cargo.toml Metadata

**File:** `crates/rustynes-desktop/Cargo.toml`

```toml
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
rustynes-core = { path = "../rustynes-core" }
eframe = "0.24"
egui = "0.24"
egui_wgpu_backend = "0.24"
wgpu = "0.18"
cpal = "0.15"
gilrs = "0.10"
rfd = "0.12"
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
ringbuf = "0.3"
dirs = "5.0"
thiserror = "1.0"
image = { version = "0.24", features = ["png"] }
open = "5.0"

[build-dependencies]
embed-resource = "2.1"  # Windows icon embedding

[package.metadata.bundle]
name = "RustyNES"
identifier = "com.doublegate.rustynes"
icon = ["assets/icon.png"]
```

---

## Acceptance Criteria

### Functionality

- [ ] Configuration file loads/saves automatically at `~/.config/rustynes/config.toml` (Linux) or equivalent
- [ ] Settings window displays all configuration options
- [ ] All settings persist across application restarts
- [ ] Recent ROMs list shows last 10 opened files
- [ ] Recent ROMs accessible via Ctrl+1 through Ctrl+9
- [ ] About window displays version, license, and credits
- [ ] Error dialogs show user-friendly messages with technical details
- [ ] Application icon displays in taskbar/dock
- [ ] Window title shows current ROM name
- [ ] Cross-platform packages build successfully

### User Experience

- [ ] Settings window has intuitive tabbed layout
- [ ] All controls have clear labels and tooltips
- [ ] Reset to Defaults button works for all settings
- [ ] Settings changes apply immediately (no restart required)
- [ ] Recent ROMs show filename only (not full path)
- [ ] Error dialogs are modal and non-intrusive
- [ ] About window opens from Help menu
- [ ] Configuration file is human-readable TOML

### Quality

- [ ] No crashes on invalid configuration files (fallback to defaults)
- [ ] Configuration validation prevents invalid values
- [ ] Settings window closes when pressing Escape
- [ ] All keyboard shortcuts documented in UI
- [ ] Platform-specific paths used correctly (XDG on Linux, AppData on Windows, etc.)
- [ ] Zero unsafe code in configuration/UI modules
- [ ] No dependencies with known security vulnerabilities

---

## Dependencies

### Prerequisites

- M6-S1: egui application structure (menu bar, status bar)
- M6-S2: wgpu rendering backend (video settings apply to Renderer)
- M6-S3: audio output (audio settings apply to AudioOutput)
- M6-S4: controller support (input settings apply to InputManager)

### Crate Dependencies

```toml
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
dirs = "5.0"       # Cross-platform config directories
thiserror = "1.0"
image = "0.24"     # Icon loading
open = "5.0"       # Open URLs in browser
```

### Optional Dependencies

```bash
# Linux AppImage packaging
sudo apt install libfuse2 wget jq

# macOS DMG packaging
# Requires Xcode Command Line Tools (iconutil, hdiutil)

# Windows installer (optional)
# Requires Inno Setup or WiX Toolset
```

---

## Related Documentation

- [Configuration API](../../../docs/api/CONFIGURATION.md)
- [Desktop Frontend](../../../docs/platform/DESKTOP.md)
- [Build Instructions](../../../docs/dev/BUILD.md)

---

## Technical Notes

### Configuration File Location

**Platform-specific paths** (via `dirs` crate):

- **Linux**: `~/.config/rustynes/config.toml`
- **macOS**: `~/Library/Application Support/rustynes/config.toml`
- **Windows**: `%APPDATA%\rustynes\config.toml`

**Fallback**: If `dirs::config_dir()` fails, use `./config.toml` in current directory.

### Settings Persistence Strategy

1. **Automatic save on exit**: `Drop` trait implementation
2. **Periodic autosave**: Every 30 seconds (if modified)
3. **Immediate save**: After opening ROM (updates recent list)

### TOML Validation

Use `serde` default values and `#[serde(default)]` to handle missing keys gracefully. If parsing fails entirely, log error and use `AppConfig::default()`.

### Recent ROMs List

- **Max size**: 10 entries
- **Deduplication**: Remove existing entry before adding to front
- **Display**: Show filename only (use `Path::file_name()`)
- **Keyboard shortcuts**: Ctrl+1 through Ctrl+9 (limit to 9 entries)

### Error Dialog Design

- **Modal**: Block interaction until dismissed
- **User-friendly message**: Non-technical explanation
- **Collapsible details**: Technical error info (Debug format)
- **Icon**: Red warning symbol (⚠)
- **Auto-focus OK button**: For quick dismissal

### Cross-Platform Packaging

| Platform | Format | Tool | Size (est.) |
|----------|--------|------|-------------|
| Linux | AppImage | appimagetool | ~15 MB |
| Windows | ZIP | PowerShell | ~5 MB |
| macOS | DMG | hdiutil | ~10 MB |

**Note**: Release builds use `--release` for optimizations. Consider `strip` for smaller binaries.

---

## Performance Targets

- **Config load time**: <10ms
- **Config save time**: <50ms
- **Settings window render**: <1ms per frame
- **Recent ROMs menu**: <1ms to populate
- **Error dialog render**: <1ms per frame

---

## Success Criteria

1. Configuration system tested on Linux, Windows, and macOS
2. All settings persist correctly across restarts
3. Recent ROMs list functions with keyboard shortcuts
4. Settings window has no UI glitches or rendering issues
5. Error dialogs tested with various error scenarios
6. Application icon displays correctly on all platforms
7. Cross-platform packages build without errors
8. Zero clippy warnings (`clippy::pedantic`)
9. All acceptance criteria met
10. M6-S5 sprint marked as ✅ COMPLETE

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1, M6-S2, M6-S3, M6-S4
**Next Sprint:** None (M6 Complete → Phase 1 MVP Complete!)
