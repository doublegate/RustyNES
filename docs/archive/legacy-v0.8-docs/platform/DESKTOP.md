# RustyNES Desktop Platform Specification

**Document Version:** 1.2.0
**Last Updated:** 2025-12-19
**Milestone:** M6 - Desktop GUI (Phase 1 MVP)
**Sprint Status:** Sprint 5 Complete (Polish & Release)

---

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Framework Stack](#framework-stack)
- [Application Structure](#application-structure)
- [Rendering Pipeline](#rendering-pipeline)
- [Audio System](#audio-system)
- [Input Handling](#input-handling)
- [Settings Persistence](#settings-persistence)
- [Run-Ahead System](#run-ahead-system)
- [UI Components](#ui-components)
- [Design System](#design-system)
- [Performance Targets](#performance-targets)
- [Implementation Phases](#implementation-phases)
- [Sprint 4 Implementation](#sprint-4-implementation)
- [Sprint 5 Implementation](#sprint-5-implementation)
- [UI/UX Design v2 Implementation](#uiux-design-v2-implementation)

---

## Overview

The RustyNES desktop application provides a native, high-performance graphical interface for NES emulation on Windows, macOS, and Linux. Built with Iced 0.13+ for the primary UI framework and wgpu for GPU-accelerated rendering, it delivers modern UX patterns while maintaining the nostalgic aesthetics of classic console gaming.

### Design Philosophy

**Nostalgic Futurism**: Blend retro NES aesthetics with modern UI/UX patterns. The interface should feel simultaneously familiar to classic gamers and polished for contemporary users.

### Key Features (MVP - Milestone 6)

- **Iced 0.13+ UI Framework**: Elm architecture with type-safe state management
- **wgpu GPU Rendering**: Hardware-accelerated NES framebuffer display with integer scaling
- **cpal Audio Engine**: Cross-platform, low-latency audio (<20ms)
- **gilrs Gamepad Support**: Universal controller compatibility
- **Run-Ahead Latency Reduction**: Basic RA=1 implementation for MVP (RA=0-4 in Phase 2)
- **ROM Library Management**: Scan, organize, and launch games
- **TOML Settings Persistence**: Cross-platform configuration storage
- **Glass Morphism Styling**: Modern visual design with backdrop blur effects

---

## Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    DESKTOP APPLICATION                      │
│                  (rustynes-desktop crate)                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              ICED APPLICATION LAYER                   │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  RustyNes (Application State)                   │  │  │
│  │  │  • Model: Console, Settings, LibraryState       │  │  │
│  │  │  • Update: Message → State Transitions          │  │  │
│  │  │  • View: State → Element Tree                   │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  │                                                       │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  Views (Welcome, Library, Playing, Settings)    │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
│                           ↕                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              RENDERING SUBSYSTEM                      │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  wgpu Renderer                                  │  │  │
│  │  │  • NES Framebuffer → Texture (256×240)          │  │  │
│  │  │  • Integer Scaling Pipeline                     │  │  │
│  │  │  • Aspect Ratio Preservation (8:7 PAR)          │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
│                           ↕                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              AUDIO SUBSYSTEM                          │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  cpal Audio Thread                              │  │  │
│  │  │  • Ring Buffer (4096 samples)                   │  │  │
│  │  │  • Sample Rate Conversion (APU → 48kHz)         │  │  │
│  │  │  • Latency Target: <20ms                        │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
│                           ↕                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              INPUT SUBSYSTEM                          │  │
│  │  ┌─────────────────────────────────────────────────┐  │  │
│  │  │  gilrs Gamepad Manager                          │  │  │
│  │  │  • Controller Mapping (Xbox, PS, Switch)        │  │  │
│  │  │  • Keyboard Fallback                            │  │  │
│  │  │  • Hotkey Bindings                              │  │  │
│  │  └─────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────┘  │
│                           ↕                                 │
│  ┌───────────────────────────────────────────────────────┐  │
│  │           RUSTYNES-CORE (Emulation Engine)            │  │
│  │  Console → CPU, PPU, APU, Mappers, Bus                │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Elm Architecture (Iced 0.13+)

Iced follows The Elm Architecture, a functional pattern with unidirectional data flow:

```
┌──────────┐
│  Model   │  ← Application state (RustyNes struct)
└──────────┘
     ↓
┌──────────┐
│   View   │  ← Pure function: Model → Element<Message>
└──────────┘
     ↓
┌──────────┐
│   User   │  ← User interaction generates Message
└──────────┘
     ↓
┌──────────┐
│  Update  │  ← Message → Model transformation
└──────────┘
     ↓
   (loop)
```

**Benefits**:
- Type-safe state management (no runtime state errors)
- Predictable state transitions (all changes through `update()`)
- Time-travel debugging (record/replay message sequences)
- Testability (pure view functions, deterministic updates)

---

## Framework Stack

### Core Dependencies

```toml
[dependencies]
# UI Framework
iced = { version = "0.13", features = ["wgpu", "tokio", "canvas", "image"] }
iced_wgpu = "0.13"
iced_native = "0.13"

# Graphics
wgpu = "22.0"
bytemuck = { version = "1.14", features = ["derive"] }

# Audio
cpal = "0.15"
rubato = "0.15"  # Sample rate conversion

# Input
gilrs = "0.11"

# Serialization
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# File Dialogs
rfd = "0.15"

# Utilities
directories = "5.0"
log = "0.4"
env_logger = "0.11"
thiserror = "1.0"

# Internal crates
rustynes-core = { path = "../rustynes-core" }
```

### Platform-Specific Dependencies

```toml
[target.'cfg(target_os = "windows")'.dependencies]
winapi = { version = "0.3", features = ["winuser"] }

[target.'cfg(target_os = "macos")'.dependencies]
cocoa = "0.25"
objc = "0.2"

[target.'cfg(target_os = "linux")'.dependencies]
x11 = { version = "2.21", features = ["xlib"] }
```

---

## Application Structure

### Main Application State (Model)

```rust
/// Root application state following Elm architecture
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

    /// Run-ahead engine (MVP: RA=1 fixed)
    run_ahead: RunAheadManager,

    // ═══════════════════════════════════════════════════════════
    // UI STATE
    // ═══════════════════════════════════════════════════════════

    /// Theme configuration
    theme: Theme,

    /// Window state
    window: WindowState,

    /// Current modal (if any)
    modal: Option<Modal>,

    /// Toast notifications
    toasts: Vec<Toast>,

    // ═══════════════════════════════════════════════════════════
    // SUBSYSTEMS
    // ═══════════════════════════════════════════════════════════

    /// ROM library
    library: LibraryState,

    /// Settings
    settings: Settings,

    /// Audio state
    audio: AudioState,

    /// Input state
    input: InputState,

    /// Rendering state
    rendering: RenderingState,
}

/// All possible views (screens)
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
}

/// Settings tabs
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsTab {
    Video,
    Audio,
    Input,
    Paths,
    Advanced,
}

/// Emulation state
#[derive(Debug, Clone, PartialEq)]
pub enum EmulationState {
    Idle,
    Running,
    Paused,
    Error(String),
}
```

### Message Types (Events)

```rust
/// All application messages (events)
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

    // ═══════════════════════════════════════════════════════════
    // SETTINGS
    // ═══════════════════════════════════════════════════════════
    UpdateSetting(SettingKey, SettingValue),
    SaveSettings,

    // ═══════════════════════════════════════════════════════════
    // LIBRARY
    // ═══════════════════════════════════════════════════════════
    ScanRomDirectory(PathBuf),
    ScanComplete(Vec<RomEntry>),
    SearchLibrary(String),
    SortLibrary(SortOrder),

    // ═══════════════════════════════════════════════════════════
    // UI
    // ═══════════════════════════════════════════════════════════
    ShowToast(Toast),
    DismissToast(ToastId),
    ShowModal(Modal),
    DismissModal,
    ThemeChanged(ThemeVariant),

    // ═══════════════════════════════════════════════════════════
    // SYSTEM
    // ═══════════════════════════════════════════════════════════
    Tick(Instant),
    Exit,
}
```

### Update Function (State Transitions)

```rust
impl Application for RustyNes {
    type Message = Message;
    type Executor = executor::Default;
    type Flags = ();
    type Theme = Theme;

    fn new(_flags: Self::Flags) -> (Self, Command<Message>) {
        // Load settings from disk
        let settings = Settings::load().unwrap_or_default();
        let theme = Theme::from_variant(settings.theme_variant);

        (
            Self {
                view: View::Welcome,
                console: None,
                emulation: EmulationState::Idle,
                run_ahead: RunAheadManager::new(),
                theme,
                window: WindowState::default(),
                modal: None,
                toasts: Vec::new(),
                library: LibraryState::new(),
                settings,
                audio: AudioState::new(),
                input: InputState::new(),
                rendering: RenderingState::new(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        match &self.view {
            View::Welcome => "RustyNES - Welcome".to_string(),
            View::Library => "RustyNES - Library".to_string(),
            View::Playing => {
                if let Some(console) = &self.console {
                    format!("RustyNES - {}", console.rom_title())
                } else {
                    "RustyNES - Playing".to_string()
                }
            }
            View::Settings(_) => "RustyNES - Settings".to_string(),
        }
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::NavigateTo(view) => {
                self.view = view;
                Command::none()
            }

            Message::LoadRom(path) => {
                // Async ROM loading
                Command::perform(
                    async move {
                        let rom_data = tokio::fs::read(&path).await?;
                        Console::new(&rom_data)
                    },
                    Message::RomLoaded,
                )
            }

            Message::RomLoaded(result) => {
                match result {
                    Ok(console) => {
                        self.console = Some(console);
                        self.emulation = EmulationState::Running;
                        self.view = View::Playing;
                        self.show_toast(Toast::success("ROM loaded successfully"));
                    }
                    Err(err) => {
                        self.emulation = EmulationState::Error(err.to_string());
                        self.show_toast(Toast::error(&format!("Failed to load ROM: {}", err)));
                    }
                }
                Command::none()
            }

            Message::Play => {
                self.emulation = EmulationState::Running;
                Command::none()
            }

            Message::Pause => {
                self.emulation = EmulationState::Paused;
                Command::none()
            }

            Message::Tick(now) => {
                if self.emulation == EmulationState::Running {
                    if let Some(console) = &mut self.console {
                        // Step one frame
                        let framebuffer = console.step_frame();

                        // Update rendering state
                        self.rendering.update_framebuffer(framebuffer);

                        // Process audio samples
                        let samples = console.apu_mut().take_samples();
                        self.audio.push_samples(samples);

                        // Run-ahead processing (MVP: RA=1)
                        if self.settings.run_ahead_enabled {
                            self.run_ahead.process(console, &self.input.current_state);
                        }
                    }
                }
                Command::none()
            }

            // ... other message handlers

            _ => Command::none(),
        }
    }

    fn view(&self) -> Element<Message> {
        match &self.view {
            View::Welcome => view_welcome(self),
            View::Library => view_library(self),
            View::Playing => view_playing(self),
            View::Settings(tab) => view_settings(self, *tab),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // Tick every frame for emulation loop
        if self.emulation == EmulationState::Running {
            time::every(Duration::from_millis(16))
                .map(|_| Message::Tick(Instant::now()))
        } else {
            Subscription::none()
        }
    }
}
```

---

## Rendering Pipeline

### wgpu NES Framebuffer Renderer

The NES outputs a 256×240 pixel framebuffer at 60.0988 Hz. The rendering pipeline handles:
- Framebuffer upload to GPU texture
- Integer scaling (2× minimum, 3×, 4×, 5×, 6× based on window size)
- Aspect ratio correction (8:7 pixel aspect ratio)
- Center alignment with letterboxing

```rust
pub struct NesRenderer {
    /// wgpu device
    device: wgpu::Device,

    /// Command queue
    queue: wgpu::Queue,

    /// NES framebuffer texture (256×240 RGB)
    framebuffer_texture: wgpu::Texture,

    /// Render pipeline
    pipeline: wgpu::RenderPipeline,

    /// Bind group for framebuffer texture
    bind_group: wgpu::BindGroup,

    /// Vertex buffer (fullscreen quad)
    vertex_buffer: wgpu::Buffer,

    /// Current scale factor (2×, 3×, 4×, etc.)
    scale: u32,
}

impl NesRenderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // Create 256×240 texture
        let framebuffer_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("NES Framebuffer"),
            size: wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Create render pipeline for nearest-neighbor scaling
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("NES Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/nes.wgsl").into()),
        });

        // ... pipeline creation

        Self {
            device,
            queue,
            framebuffer_texture,
            pipeline,
            bind_group,
            vertex_buffer,
            scale: 3,
        }
    }

    /// Update framebuffer texture from NES output
    pub fn update_framebuffer(&mut self, framebuffer: &[u8]) {
        // Convert indexed color (256×240 palette indices) to RGBA8
        let rgba_data = self.convert_to_rgba(framebuffer);

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.framebuffer_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_data,
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

    /// Calculate optimal integer scale for window size
    pub fn calculate_scale(&self, window_width: u32, window_height: u32) -> u32 {
        // NES resolution with 8:7 PAR correction
        const NES_WIDTH_CORRECTED: u32 = 292; // 256 * 8/7 ≈ 292
        const NES_HEIGHT: u32 = 240;

        let max_scale_x = window_width / NES_WIDTH_CORRECTED;
        let max_scale_y = window_height / NES_HEIGHT;

        max_scale_x.min(max_scale_y).max(2) // Minimum 2× scaling
    }

    fn convert_to_rgba(&self, framebuffer: &[u8]) -> Vec<u8> {
        // NES palette (NTSC 2C02)
        const PALETTE: [[u8; 3]; 64] = [
            // ... 64 RGB triplets for NES palette
        ];

        framebuffer.iter()
            .flat_map(|&index| {
                let rgb = PALETTE[index as usize % 64];
                [rgb[0], rgb[1], rgb[2], 255]
            })
            .collect()
    }
}
```

### wgpu Shader (nes.wgsl)

```wgsl
// Nearest-neighbor scaling shader for pixel-perfect NES rendering

@group(0) @binding(0) var nes_texture: texture_2d<f32>;
@group(0) @binding(1) var nes_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Fullscreen triangle
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );

    let position = positions[vertex_index];
    let uv = position * 0.5 + 0.5;

    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Nearest-neighbor sampling for pixel-perfect scaling
    return textureSample(nes_texture, nes_sampler, input.uv);
}
```

---

## Audio System

### cpal Audio Thread

The audio subsystem runs on a dedicated thread with a lock-free ring buffer to prevent audio crackling.

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;

pub struct AudioSystem {
    /// Sample rate (48000 Hz)
    sample_rate: u32,

    /// Ring buffer (producer side)
    producer: HeapProducer<f32>,

    /// Audio stream
    stream: cpal::Stream,

    /// Sample rate converter (APU native ~1.789 MHz → 48 kHz)
    resampler: Option<rubato::FastFixedIn<f32>>,
}

impl AudioSystem {
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;

        // Create ring buffer (4096 samples = ~85ms at 48kHz)
        let ring = HeapRb::<f32>::new(4096);
        let (producer, mut consumer) = ring.split();

        // Build audio stream
        let stream = device.build_output_stream(
            &config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // Fill output buffer from ring buffer
                for sample in output.iter_mut() {
                    *sample = consumer.pop().unwrap_or(0.0);
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self {
            sample_rate,
            producer,
            stream,
            resampler: None,
        })
    }

    /// Push APU samples (called from emulation thread)
    pub fn push_samples(&mut self, samples: &[f32]) {
        // Resample if needed
        let resampled = if let Some(resampler) = &mut self.resampler {
            resampler.process(&[samples], None).unwrap()[0].clone()
        } else {
            samples.to_vec()
        };

        // Push to ring buffer (drop if full to prevent blocking)
        for &sample in &resampled {
            let _ = self.producer.push(sample);
        }
    }

    /// Get current latency estimate
    pub fn latency_ms(&self) -> f32 {
        let buffered = self.producer.len() as f32;
        (buffered / self.sample_rate as f32) * 1000.0
    }
}
```

---

## Input Handling

### gilrs Gamepad Manager

```rust
use gilrs::{Gilrs, Event, EventType, Button, Axis};

pub struct InputManager {
    /// gilrs context
    gilrs: Gilrs,

    /// Connected gamepads
    gamepads: HashMap<GamepadId, Gamepad>,

    /// Current controller state (NES format)
    controller1: ControllerState,
    controller2: ControllerState,

    /// Keyboard mapping
    keyboard_map: KeyboardMapping,
}

/// NES controller state (8 bits)
#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl InputManager {
    pub fn new() -> Self {
        let gilrs = Gilrs::new().expect("Failed to initialize gilrs");

        Self {
            gilrs,
            gamepads: HashMap::new(),
            controller1: ControllerState::default(),
            controller2: ControllerState::default(),
            keyboard_map: KeyboardMapping::default(),
        }
    }

    /// Poll for input events
    pub fn poll(&mut self) -> Vec<Message> {
        let mut messages = Vec::new();

        // Process gamepad events
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            match event {
                EventType::Connected => {
                    messages.push(Message::GamepadConnected(id));
                }
                EventType::Disconnected => {
                    messages.push(Message::GamepadDisconnected(id));
                }
                EventType::ButtonPressed(button, _) => {
                    self.handle_button(button, true);
                }
                EventType::ButtonReleased(button, _) => {
                    self.handle_button(button, false);
                }
                _ => {}
            }
        }

        messages
    }

    fn handle_button(&mut self, button: Button, pressed: bool) {
        match button {
            Button::South => self.controller1.b = pressed,      // A (Xbox A / PS X)
            Button::East => self.controller1.a = pressed,       // B (Xbox B / PS Circle)
            Button::Select => self.controller1.select = pressed,
            Button::Start => self.controller1.start = pressed,
            Button::DPadUp => self.controller1.up = pressed,
            Button::DPadDown => self.controller1.down = pressed,
            Button::DPadLeft => self.controller1.left = pressed,
            Button::DPadRight => self.controller1.right = pressed,
            _ => {}
        }
    }

    /// Get current controller state as byte (for NES bus)
    pub fn controller1_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.controller1.a { byte |= 0x01; }
        if self.controller1.b { byte |= 0x02; }
        if self.controller1.select { byte |= 0x04; }
        if self.controller1.start { byte |= 0x08; }
        if self.controller1.up { byte |= 0x10; }
        if self.controller1.down { byte |= 0x20; }
        if self.controller1.left { byte |= 0x40; }
        if self.controller1.right { byte |= 0x80; }
        byte
    }
}
```

---

## Settings Persistence

### TOML Configuration

Settings are stored in platform-specific directories using the `directories` crate.

**File Location:**
- Linux: `~/.config/rustynes/settings.toml`
- macOS: `~/Library/Application Support/rustynes/settings.toml`
- Windows: `%APPDATA%\rustynes\settings.toml`

```rust
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Video settings
    pub video: VideoSettings,

    /// Audio settings
    pub audio: AudioSettings,

    /// Input settings
    pub input: InputSettings,

    /// Path settings
    pub paths: PathSettings,

    /// Advanced settings
    pub advanced: AdvancedSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSettings {
    /// Integer scale factor (2×, 3×, 4×, 5×, 6×)
    pub scale: u32,

    /// Fullscreen mode
    pub fullscreen: bool,

    /// VSync enabled
    pub vsync: bool,

    /// Frame skip (0 = no skip)
    pub frame_skip: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    /// Master volume (0.0 - 1.0)
    pub volume: f32,

    /// Sample rate (48000 Hz recommended)
    pub sample_rate: u32,

    /// Buffer size (lower = less latency, higher = less crackling)
    pub buffer_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSettings {
    /// Run-ahead enabled (MVP: RA=1 fixed)
    pub run_ahead_enabled: bool,

    /// Run-ahead frames (MVP: 1, Phase 2: 0-4)
    pub run_ahead_frames: u8,
}

impl Settings {
    /// Load settings from disk
    pub fn load() -> Result<Self, SettingsError> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            let toml_str = std::fs::read_to_string(&config_path)?;
            Ok(toml::from_str(&toml_str)?)
        } else {
            Ok(Self::default())
        }
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<(), SettingsError> {
        let config_path = Self::config_path()?;

        // Create parent directories if needed
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let toml_str = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, toml_str)?;

        Ok(())
    }

    fn config_path() -> Result<PathBuf, SettingsError> {
        let proj_dirs = ProjectDirs::from("com", "RustyNES", "RustyNES")
            .ok_or(SettingsError::NoConfigDirectory)?;

        Ok(proj_dirs.config_dir().join("settings.toml"))
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            video: VideoSettings {
                scale: 3,
                fullscreen: false,
                vsync: true,
                frame_skip: 0,
            },
            audio: AudioSettings {
                volume: 1.0,
                sample_rate: 48000,
                buffer_size: 4096,
            },
            input: InputSettings::default(),
            paths: PathSettings::default(),
            advanced: AdvancedSettings {
                run_ahead_enabled: false,
                run_ahead_frames: 1,
            },
        }
    }
}
```

---

## Run-Ahead System

### MVP Implementation (RA=1)

Run-ahead reduces input latency by speculatively running the emulation one frame ahead with the assumption that input won't change. If input does change, the state is rolled back.

```rust
pub struct RunAheadManager {
    /// Run-ahead enabled
    enabled: bool,

    /// Number of frames to run ahead (MVP: fixed at 1)
    frames: u8,

    /// Saved state before speculative execution
    saved_state: Option<SaveState>,

    /// Previous input state
    prev_input: ControllerState,

    /// Performance metrics
    metrics: RunAheadMetrics,
}

#[derive(Debug, Clone)]
pub struct RunAheadMetrics {
    /// Successful predictions (input didn't change)
    pub hits: u64,

    /// Failed predictions (had to rollback)
    pub misses: u64,

    /// Average time to save state (µs)
    pub avg_save_time_us: f32,

    /// Average time to restore state (µs)
    pub avg_restore_time_us: f32,
}

impl RunAheadManager {
    pub fn new() -> Self {
        Self {
            enabled: false,
            frames: 1,
            saved_state: None,
            prev_input: ControllerState::default(),
            metrics: RunAheadMetrics::default(),
        }
    }

    /// Process run-ahead for current frame
    pub fn process(&mut self, console: &mut Console, current_input: &ControllerState) {
        if !self.enabled {
            return;
        }

        let start = Instant::now();

        // Save current state
        let state = console.save_state();
        let save_time = start.elapsed().as_micros() as f32;

        // Check if input changed
        if current_input != &self.prev_input {
            // Input changed - restore previous state
            if let Some(saved) = &self.saved_state {
                let restore_start = Instant::now();
                console.load_state(saved);
                let restore_time = restore_start.elapsed().as_micros() as f32;

                self.metrics.misses += 1;
                self.metrics.avg_restore_time_us =
                    (self.metrics.avg_restore_time_us * 0.95) + (restore_time * 0.05);
            }
        } else {
            // Input unchanged - prediction hit
            self.metrics.hits += 1;
        }

        // Run ahead 1 frame with current input
        console.step_frame();

        // Save state for next frame
        self.saved_state = Some(state);
        self.prev_input = *current_input;

        self.metrics.avg_save_time_us =
            (self.metrics.avg_save_time_us * 0.95) + (save_time * 0.05);
    }

    /// Get hit rate (successful predictions)
    pub fn hit_rate(&self) -> f32 {
        let total = self.metrics.hits + self.metrics.misses;
        if total == 0 {
            0.0
        } else {
            self.metrics.hits as f32 / total as f32
        }
    }

    /// Get latency reduction estimate (ms)
    pub fn latency_reduction_ms(&self) -> f32 {
        if !self.enabled {
            return 0.0;
        }

        // Each frame of run-ahead reduces latency by ~16.67ms (60 Hz)
        (self.frames as f32) * 16.67 * self.hit_rate()
    }
}
```

---

## UI Components

### Custom Iced Widgets

```rust
/// Custom NES viewport widget (displays framebuffer with integer scaling)
pub struct NesViewport {
    framebuffer: Vec<u8>,
    scale: u32,
}

impl<Message> Widget<Message, Renderer> for NesViewport {
    fn width(&self) -> Length {
        Length::Fixed((256 * self.scale) as f32)
    }

    fn height(&self) -> Length {
        Length::Fixed((240 * self.scale) as f32)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: Cursor,
        viewport: &Rectangle,
    ) {
        // Render framebuffer texture
        renderer.draw_nes_framebuffer(
            &self.framebuffer,
            layout.bounds(),
            self.scale,
        );
    }
}
```

---

## Design System

### Color Palette (Nostalgic Futurism)

```rust
pub struct ColorPalette {
    /// Console Black - Primary background
    pub console_black: Color,  // #1A1A2E

    /// Deep Navy - Secondary background
    pub deep_navy: Color,      // #16213E

    /// NES Blue - Accent color
    pub nes_blue: Color,       // #0F3460

    /// Power Red - Primary action color
    pub power_red: Color,      // #E94560

    /// Coral Accent - Secondary action color
    pub coral_accent: Color,   // #FF6B6B
}

impl ColorPalette {
    pub const CONSOLE_BLACK: Color = Color::from_rgb(0.102, 0.102, 0.180);
    pub const DEEP_NAVY: Color = Color::from_rgb(0.086, 0.129, 0.243);
    pub const NES_BLUE: Color = Color::from_rgb(0.059, 0.204, 0.376);
    pub const POWER_RED: Color = Color::from_rgb(0.914, 0.271, 0.376);
    pub const CORAL_ACCENT: Color = Color::from_rgb(1.0, 0.420, 0.420);
}
```

### Typography

```rust
pub const FONT_UI: &str = "JetBrains Mono";
pub const FONT_HEADER: &str = "Press Start 2P";
pub const FONT_BODY: &str = "Inter";

pub const FONT_SIZE_SMALL: u16 = 12;
pub const FONT_SIZE_MEDIUM: u16 = 14;
pub const FONT_SIZE_LARGE: u16 = 16;
pub const FONT_SIZE_HEADER: u16 = 20;
```

### Glass Morphism Styling

```rust
pub fn glass_container() -> Container<'static, Message> {
    Container::new(content)
        .style(|theme| {
            container::Appearance {
                background: Some(Background::Color(
                    Color::from_rgba(0.102, 0.102, 0.180, 0.7)
                )),
                border: Border {
                    color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(20)
}
```

---

## Performance Targets

### Milestone 6 MVP Goals

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Frame Rate** | 60 FPS (60.0988 Hz exact) | Stable 16.67ms per frame |
| **Audio Latency** | <20ms | cpal buffer size + resampling |
| **Input Latency** | <16.67ms base, <8ms with RA=1 | gilrs poll rate + run-ahead |
| **Memory Usage** | <100MB | RSS during active gameplay |
| **CPU Usage** | <15% single core | On modern CPU (Ryzen/Intel 10th gen) |
| **Startup Time** | <2 seconds | Cold start to welcome screen |
| **ROM Load Time** | <500ms | Average for 512KB ROM |

### Benchmarking

```rust
// Criterion benchmarks
#[bench]
fn bench_frame_rendering(b: &mut Bencher) {
    let mut renderer = NesRenderer::new();
    let framebuffer = vec![0u8; 256 * 240];

    b.iter(|| {
        renderer.update_framebuffer(&framebuffer);
        renderer.render();
    });
}

#[bench]
fn bench_save_state(b: &mut Bencher) {
    let console = Console::new(&ROM_DATA).unwrap();

    b.iter(|| {
        console.save_state()
    });
}
```

---

## Implementation Phases

### Sprint 1: Iced Application Foundation (Week 1)

**Deliverables:**
- [ ] Create `rustynes-desktop` crate structure
- [ ] Implement `RustyNes` application state
- [ ] Build `Message` enum and `update()` function
- [ ] Create Welcome view with ROM file picker
- [ ] Basic window creation and event loop
- [ ] Theme system (Dark mode only for MVP)

**Files to create:**
- `crates/rustynes-desktop/src/main.rs`
- `crates/rustynes-desktop/src/app.rs`
- `crates/rustynes-desktop/src/views/mod.rs`
- `crates/rustynes-desktop/src/views/welcome.rs`
- `crates/rustynes-desktop/src/theme.rs`
- `crates/rustynes-desktop/Cargo.toml`

### Sprint 2: wgpu Rendering (Week 2)

**Deliverables:**
- [ ] wgpu NES framebuffer renderer
- [ ] Integer scaling pipeline
- [ ] Aspect ratio correction (8:7 PAR)
- [ ] Playing view with NES viewport
- [ ] 60 FPS rendering loop
- [ ] Basic performance metrics overlay

**Files to create:**
- `crates/rustynes-desktop/src/rendering/mod.rs`
- `crates/rustynes-desktop/src/rendering/nes_renderer.rs`
- `crates/rustynes-desktop/src/rendering/shaders/nes.wgsl`
- `crates/rustynes-desktop/src/views/playing.rs`
- `crates/rustynes-desktop/src/widgets/nes_viewport.rs`

### Sprint 3: Input + ROM Library (Week 3)

**Deliverables:**
- [ ] gilrs gamepad support
- [ ] Keyboard input handling
- [ ] Controller mapping UI
- [ ] ROM library scanner
- [ ] Library view with ROM list
- [ ] ROM metadata extraction (iNES header)

**Files to create:**
- `crates/rustynes-desktop/src/input/mod.rs`
- `crates/rustynes-desktop/src/input/gamepad.rs`
- `crates/rustynes-desktop/src/input/keyboard.rs`
- `crates/rustynes-desktop/src/library/mod.rs`
- `crates/rustynes-desktop/src/library/scanner.rs`
- `crates/rustynes-desktop/src/views/library.rs`

### Sprint 4: Settings Persistence (Week 4)

**Deliverables:**
- [ ] Settings data structures
- [ ] TOML serialization/deserialization
- [ ] Settings view (Video, Audio, Input, Paths, Advanced)
- [ ] Cross-platform config directory handling
- [ ] Save/load settings on app start/exit

**Files to create:**
- `crates/rustynes-desktop/src/settings/mod.rs`
- `crates/rustynes-desktop/src/settings/storage.rs`
- `crates/rustynes-desktop/src/views/settings.rs`

### Sprint 5: Polish + Run-Ahead (Variable)

**Deliverables:**
- [ ] Run-ahead manager (RA=1 fixed for MVP)
- [ ] Performance metrics tracking
- [ ] Toast notifications system
- [ ] Modal dialogs
- [ ] Error handling and user feedback
- [ ] Hotkey system
- [ ] Save state quick save/load (F5/F6)

**Files to create:**
- `crates/rustynes-desktop/src/run_ahead.rs`
- `crates/rustynes-desktop/src/widgets/toast.rs`
- `crates/rustynes-desktop/src/widgets/modal.rs`
- `crates/rustynes-desktop/src/hotkeys.rs`

---

## Sprint 4 Implementation

### Overview

Sprint 4 (Settings & Persistence) has been fully implemented, providing a comprehensive configuration system with TOML persistence and a tabbed settings UI following the Elm architecture pattern.

### Implemented Files

```
crates/rustynes-desktop/
├── src/
│   ├── config/
│   │   ├── mod.rs           # Module exports (AppConfig, settings types)
│   │   └── settings.rs      # Complete settings data structures
│   ├── views/
│   │   └── settings.rs      # Tabbed settings UI implementation
│   ├── app.rs               # Updated with settings integration
│   ├── message.rs           # All settings-related message variants
│   └── main.rs              # Window size persistence on startup
└── Cargo.toml               # Added dependencies: toml, directories, thiserror
```

### Configuration System (`config/settings.rs`)

The configuration system implements a hierarchical settings structure with TOML serialization:

```rust
/// Main application configuration (TOML serializable)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub emulation: EmulationConfig,  // Speed, region, rewind settings
    pub video: VideoConfig,          // Scaling, VSync, CRT shader, overscan
    pub audio: AudioConfig,          // Output, sample rate, volume, buffer
    pub input: InputConfig,          // Keyboard mappings, gamepad deadzone
    pub app: ApplicationConfig,      // Recent ROMs, window state
}
```

#### Emulation Settings

```rust
pub struct EmulationConfig {
    pub speed: f32,                  // 1.0 = normal (0.25-3.0 range)
    pub region: Region,              // NTSC (60.0988 Hz) or PAL (50.0070 Hz)
    pub rewind_enabled: bool,        // Rewind feature toggle
    pub rewind_buffer_size: usize,   // Frames (default: 600 = 10 seconds)
}
```

#### Video Settings

```rust
pub struct VideoConfig {
    pub scaling_mode: ScalingMode,   // AspectRatio4x3, PixelPerfect, Integer, Stretch
    pub vsync: bool,                 // VSync toggle
    pub crt_shader: bool,            // CRT effect enable
    pub crt_preset: CrtPreset,       // None, Subtle, Moderate, Authentic, Custom
    pub overscan: OverscanConfig,    // Top, bottom, left, right crop (0-16px)
}
```

#### Audio Settings

```rust
pub struct AudioConfig {
    pub enabled: bool,               // Audio output toggle
    pub sample_rate: u32,            // 44100, 48000, or 96000 Hz
    pub volume: f32,                 // Master volume (0.0-1.0)
    pub buffer_size: u32,            // 512, 1024, 2048, or 4096 samples
}
```

#### Input Settings

```rust
pub struct InputConfig {
    pub keyboard_p1: KeyboardMapping, // Player 1 keyboard bindings
    pub keyboard_p2: KeyboardMapping, // Player 2 keyboard bindings
    pub gamepad_deadzone: f32,        // Analog stick deadzone (0.0-0.5)
}

pub struct KeyboardMapping {
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub a: String,
    pub b: String,
    pub select: String,
    pub start: String,
}
```

### Persistence Implementation

**Config File Location (Platform-Specific):**

- Linux: `~/.config/rustynes/RustyNES/config.toml`
- macOS: `~/Library/Application Support/com.rustynes.RustyNES/config.toml`
- Windows: `%APPDATA%\rustynes\RustyNES\config\config.toml`

**Key Features:**

1. **Automatic Loading**: Config loads on application startup via `AppConfig::load()`
2. **Auto-Save on Exit**: Drop trait implementation saves config when application closes
3. **Validation**: Loaded configs are validated for valid ranges (speed > 0, volume 0-1, etc.)
4. **Default Creation**: Missing config file triggers creation with sensible defaults
5. **Recent ROMs Tracking**: Maintains deduplicated list of last 10 played ROMs

```rust
impl AppConfig {
    pub fn load() -> Result<Self, ConfigError>;
    pub fn save(&self) -> Result<(), ConfigError>;
    pub fn add_recent_rom(&mut self, path: PathBuf);
    pub fn clear_recent_roms(&mut self);
}
```

### Settings UI (`views/settings.rs`)

The settings view implements a tabbed interface with four main categories:

#### Tab Structure

```
┌──────────────────────────────────────────────────────────────────┐
│  [Emulation]  [Video]  [Audio]  [Input]                          │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│  (Tab content with sliders, checkboxes, pick_lists)              │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│  [Reset to Defaults]                              [Close]        │
└──────────────────────────────────────────────────────────────────┘
```

#### Emulation Tab Controls

| Control | Widget | Range/Values |
|---------|--------|--------------|
| Speed | Slider | 0.25x - 3.0x (step 0.25) |
| Region | PickList | NTSC, PAL |
| Enable Rewind | Checkbox | On/Off |
| Buffer Size | Slider (conditional) | 60-3600 frames (step 60) |

#### Video Tab Controls

| Control | Widget | Range/Values |
|---------|--------|--------------|
| Scaling Mode | PickList | AspectRatio4x3, PixelPerfect, IntegerScaling, Stretch |
| VSync | Checkbox | On/Off |
| CRT Shader | Checkbox | On/Off |
| CRT Preset | PickList (conditional) | Subtle, Moderate, Authentic |
| Overscan (T/B/L/R) | Sliders | 0-16 pixels each |

#### Audio Tab Controls

| Control | Widget | Range/Values |
|---------|--------|--------------|
| Audio Output | Checkbox | On/Off |
| Sample Rate | PickList | 44100, 48000, 96000 Hz |
| Master Volume | Slider | 0-100% |
| Buffer Size | PickList | 512, 1024, 2048, 4096 samples |

#### Input Tab Controls

| Control | Widget | Purpose |
|---------|--------|---------|
| Player 1 Keys | Button Grid | 8 buttons for key remapping |
| Player 2 Keys | Button Grid | 8 buttons for key remapping |
| Analog Deadzone | Slider | 0.0-0.5 (step 0.05) |

### Message Types for Settings

All settings-related messages follow the Elm architecture pattern:

```rust
pub enum Message {
    // Navigation
    OpenSettings,
    CloseSettings,
    SelectSettingsTab(SettingsTab),

    // Emulation settings
    UpdateEmulationSpeed(f32),
    UpdateRegion(Region),
    ToggleRewind(bool),
    UpdateRewindBufferSize(usize),

    // Video settings
    UpdateScalingMode(ScalingMode),
    ToggleVSync(bool),
    ToggleCrtShader(bool),
    UpdateCrtPreset(CrtPreset),
    UpdateOverscanTop(u32),
    UpdateOverscanBottom(u32),
    UpdateOverscanLeft(u32),
    UpdateOverscanRight(u32),

    // Audio settings
    ToggleAudio(bool),
    UpdateSampleRate(u32),
    UpdateVolume(f32),
    UpdateBufferSize(u32),

    // Input settings
    UpdateGamepadDeadzone(f32),
    RemapKey { player: u8, button: String },

    // Persistence
    SaveConfig,
    ConfigSaved(Result<(), String>),
    LoadConfig,
    ConfigLoaded(Result<(), String>),
    ResetSettingsToDefaults,

    // Window events
    WindowResized(f32, f32),
}
```

### Window Geometry Persistence

Window size is persisted across sessions:

1. **Resize Event Subscription**: `iced::event::listen()` captures window resize events
2. **State Update**: Window dimensions stored in `ApplicationConfig`
3. **Startup Restoration**: `main.rs` applies saved dimensions to window settings

```rust
// In app.rs - subscription
fn subscription(&self) -> Subscription<Message> {
    iced::event::listen().map(|event| {
        if let iced::Event::Window(iced::window::Event::Resized(size)) = event {
            Message::WindowResized(size.width, size.height)
        } else {
            Message::None
        }
    })
}

// In main.rs - apply saved dimensions
iced::application(...)
    .window_size((config.app.window_width as f32, config.app.window_height as f32))
```

### About Dialog

The About dialog provides application information and quick links:

| Element | Content |
|---------|---------|
| Title | "RustyNES" with version |
| Description | Accurate NES emulator |
| Links | GitHub repository, Documentation |
| Actions | Open URL (browser launch) |

### Test Coverage

The settings module includes comprehensive unit tests:

- `test_default_config` - Validates default values
- `test_config_serialization` - TOML round-trip serialization
- `test_validation` - Range and constraint validation
- `test_recent_roms` - Recent ROM list management
- `test_recent_roms_limit` - 10-entry limit enforcement

**Total Tests Passing:** 28

---

## Sprint 5 Implementation

### Overview

Sprint 5 (Polish & Release) has been fully implemented, providing application polish with icon, themes, loading infrastructure, performance metrics, and run-ahead stub for Phase 2.

### Implemented Files

```
crates/rustynes-desktop/
├── src/
│   ├── loading.rs              # Loading state management and UI
│   ├── metrics.rs              # Performance metrics tracking and overlay
│   ├── runahead.rs             # Run-ahead manager stub (Phase 2)
│   ├── theme.rs                # Enhanced with multiple theme variants
│   ├── main.rs                 # Application icon creation
│   ├── app.rs                  # Updated with metrics, themes, loading state
│   ├── message.rs              # UpdateTheme, ToggleMetrics message variants
│   ├── config/settings.rs      # Theme field added to ApplicationConfig
│   ├── views/
│   │   ├── settings.rs         # Theme selector UI
│   │   ├── playing.rs          # Metrics overlay integration
│   │   └── welcome.rs          # Updated for RustyPalette
└── Cargo.toml                  # Added dependency: image
```

### Application Icon (`main.rs`)

The application now includes a 256x256 gradient icon using RustyNES brand colors:

```rust
fn create_icon() -> Option<window::Icon> {
    use image::{ImageBuffer, Rgba};
    const SIZE: u32 = 256;

    // Gradient: Power Red (#E94560) → NES Blue (#0F3460)
    // Linear interpolation for smooth transition
    // Returns iced::window::Icon via from_rgba()
}
```

**Icon Features:**
- 256x256 RGBA buffer with gradient fill
- Power Red (#E94560) at top, NES Blue (#0F3460) at bottom
- Proper icon conversion using `image` crate
- Cross-platform window icon support

### Theme System (`theme.rs`)

Enhanced theme system with multiple variants and custom color palettes:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeVariant {
    #[default]
    Dark,        // Nostalgic Futurism (default)
    Light,       // Light mode variant
    Nord,        // Nord color scheme
    GruvboxDark, // Gruvbox Dark variant
}
```

**Theme Features:**
- Four built-in themes using Iced's built-in theme system
- Custom `RustyPalette` struct for brand-specific colors
- Persistent theme selection via ApplicationConfig
- Theme selector in settings UI (pick_list widget)

**RustyPalette (Brand Colors):**
- Background: Console Black (#1C1E21)
- Surface: Deep Navy (#262A2E)
- Accent: NES Blue (#0F3460)
- Primary: Power Red (#E94560)
- Success/Danger/Text/TextDim colors for all UI states

### Loading State System (`loading.rs`)

Infrastructure for future loading screen UI:

```rust
#[derive(Debug, Clone, Default)]
pub enum LoadingState {
    #[default]
    None,
    LoadingRom { path: PathBuf, progress: f32 },
    InitializingEmulator { progress: f32 },
}
```

**Loading Features:**
- State enum for tracking loading operations
- Progress tracking (0.0 to 1.0)
- Loading screen UI with progress bar (infrastructure ready)
- Will be activated when ROM loading UI is implemented

**Loading Screen UI:**
- Centered layout with RustyNES branding
- Progress bar (400px wide)
- Percentage display (0-100%)
- Message display (filename or "Initializing emulator")

### Performance Metrics System (`metrics.rs`)

Comprehensive performance tracking and overlay:

```rust
pub struct PerformanceMetrics {
    fps: f32,               // Frames per second
    frame_time_ms: f32,     // Frame time in milliseconds
    input_latency_ms: f32,  // Input latency (estimated)
    runahead_overhead_us: u64,  // Run-ahead overhead
    audio_buffer_fill: f32, // Audio buffer percentage
    frame_times: VecDeque<f32>,  // 60-frame history
    last_frame: Instant,
}
```

**Metrics Features:**
- FPS tracking with 60-frame rolling average
- Color-coded FPS display (Green: 58+, Yellow: 50-58, Red: <50)
- Frame time, input latency, run-ahead overhead tracking
- Audio buffer fill percentage monitoring
- F3 hotkey to toggle overlay visibility

**Metrics Overlay UI:**
- Semi-transparent black background (70% opacity)
- Top-left corner positioning with 10px padding
- 1px border with rounded corners (4px radius)
- Displays all metrics in 14pt font
- Help text: "F3: Toggle Overlay" (12pt, dimmed)

### Run-Ahead Manager Stub (`runahead.rs`)

Stub implementation for Phase 2 latency reduction system:

```rust
pub struct RunAheadManager {
    enabled: bool,      // Always false in MVP
    frames: u8,         // Always 0 in MVP
    overhead_us: u64,   // Always 0 in MVP
}
```

**Run-Ahead Stub Features:**
- Complete API surface for Phase 2 implementation
- Comprehensive documentation explaining run-ahead technique
- No-op methods (all return default/disabled values)
- Getter/setter methods with proper signatures
- Default trait implementation

**Phase 2 Implementation Plan (Documented in Module):**
- Fast save state serialization (bincode, <1ms)
- Configurable RA frames (0-4)
- Dual-instance mode for pristine audio
- Auto-detection of optimal RA per game
- JIT input polling
- Per-game profile database

### Message Handlers (`message.rs`, `app.rs`)

New message variants and handlers for Sprint 5 features:

**UpdateTheme:**
```rust
Message::UpdateTheme(theme) => {
    self.config.app.theme = theme;
    if let Err(e) = self.config.save() {
        error!("Failed to save theme preference: {}", e);
    }
    Task::none()
}
```

**ToggleMetrics:**
```rust
Message::ToggleMetrics => {
    self.show_metrics = !self.show_metrics;
    info!("Metrics overlay: {}", if self.show_metrics { "shown" } else { "hidden" });
    Task::none()
}
```

**Keyboard Subscription (F3 for Metrics):**
```rust
keyboard::on_key_press(|key, modifiers| match key {
    keyboard::Key::Named(keyboard::key::Named::F3)
        if modifiers.is_empty() => Some(Message::ToggleMetrics),
    // ... other hotkeys
})
```

### Settings UI Integration (`views/settings.rs`)

Theme selector added to settings panel:

```rust
let theme_selector = container(
    row![
        text("Theme:").width(Length::Fixed(80.0)),
        pick_list(
            ThemeVariant::all(),
            Some(config.app.theme),
            Message::UpdateTheme
        )
        .width(Length::Fixed(150.0))
    ]
    .spacing(10)
    .align_y(iced::alignment::Vertical::Center)
)
.padding(10);
```

**Theme Selector Features:**
- Pick list widget with all theme variants
- 80px label width for alignment
- 150px dropdown width
- Live theme switching (no restart required)
- Persistent theme preference (TOML config)

### Playing View Integration (`views/playing.rs`)

Metrics overlay rendering in playing view:

```rust
if model.show_metrics() {
    let metrics_overlay = container(model.metrics().view(true))
        .padding(10);
    stack![base, metrics_overlay].into()
} else {
    base
}
```

**Overlay Features:**
- Layered above game viewport using `stack!` widget
- Top-left positioning with padding
- Non-intrusive during gameplay
- Toggle visibility with F3 key

### Configuration Persistence (`config/settings.rs`)

Theme field added to ApplicationConfig:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationConfig {
    pub theme: crate::theme::ThemeVariant,
    pub window_width: u32,
    pub window_height: u32,
    // ... other app settings
}
```

**Default Theme:**
```rust
impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            theme: crate::theme::ThemeVariant::Dark,
            // ... other defaults
        }
    }
}
```

### Quality Assurance

**Code Quality:**
- Zero clippy warnings (`cargo clippy -p rustynes-desktop -- -D warnings`)
- All 28 tests passing
- Proper `#[allow(dead_code)]` annotations for MVP infrastructure
- Formatted with `cargo fmt`

**Clippy Fixes Applied:**
- `#[allow(dead_code)]` for stub infrastructure (loading, runahead, metrics getters)
- `#[allow(clippy::unused_self)]` for stub methods (will mutate in Phase 2)
- `#[derive(Default)]` with `#[default]` attribute for ThemeVariant
- Fixed `items_after_statements` by moving `use` to top of function

**Test Coverage:**
- Settings module: 5 tests (config, serialization, validation, recent ROMs)
- Input module: 8 tests (keyboard mapping, gamepad, controller state)
- Library module: 7 tests (scanner, state, search)
- Viewport module: 5 tests (scaling, texture, dimensions)
- Total: 28 tests passing

### Sprint 5 Deliverables Status

| Feature | Status | Notes |
|---------|--------|-------|
| Application Icon | Complete | 256x256 gradient icon |
| Theme System | Complete | 4 variants (Dark, Light, Nord, Gruvbox) |
| Theme Selector UI | Complete | Settings panel integration |
| Loading State | Complete | Infrastructure ready, UI deferred |
| Performance Metrics | Complete | Full tracking + overlay |
| Metrics Overlay | Complete | F3 toggle, top-left positioning |
| Run-Ahead Manager | Stub | Phase 2 implementation deferred |
| Code Quality | Complete | Zero clippy warnings, 28 tests pass |
| Documentation | Complete | DESKTOP.md updated, inline docs |

### Deferred to Phase 2

The following features from Sprint 5 planning were deferred to Phase 2:

- **Full Run-Ahead Implementation**: Stub-only in MVP, full RA=1-4 in Phase 2
- **Loading Screen UI**: State infrastructure complete, UI will activate when ROM loading is async
- **Toast Notifications**: Deferred to Phase 2 error handling sprint
- **Modal Dialogs**: Deferred to Phase 2 UI enhancement sprint
- **Save State Hotkeys**: Requires save state implementation (Phase 2)

### Files Modified Summary

| File | Changes | Lines Added |
|------|---------|-------------|
| `main.rs` | Icon creation, module declarations | +50 |
| `theme.rs` | Multiple variants, RustyPalette | +100 |
| `loading.rs` | New file: loading states, UI | +82 |
| `metrics.rs` | New file: metrics tracking, overlay | +163 |
| `runahead.rs` | New file: stub implementation | +169 |
| `app.rs` | Fields, handlers, subscriptions | +30 |
| `message.rs` | UpdateTheme, ToggleMetrics | +2 |
| `config/settings.rs` | Theme field, defaults | +5 |
| `views/settings.rs` | Theme selector UI | +15 |
| `views/playing.rs` | Metrics overlay rendering | +10 |
| `views/welcome.rs` | RustyPalette update | +2 |

**Total Lines Added:** ~628 lines of production code + comprehensive inline documentation

---

## UI/UX Design v2 Implementation

This section documents how the [UI/UX Design v2 specification](../../ref-docs/RustyNES-UI_UX-Design-v2.md) is being implemented in the desktop application.

### Color System Implementation

The UI/UX v2 color system is applied as follows:

#### Primary Palette (Current Status)

| Color | Hex | Usage | Status |
|-------|-----|-------|--------|
| Console Black | `#1A1A2E` | Primary background | Planned (Theme) |
| Deep Navy | `#16213E` | Secondary background | Planned (Theme) |
| NES Blue | `#0F3460` | Accent color | Planned (Theme) |
| Power Red | `#E94560` | Primary action | Planned (Theme) |
| Coral Accent | `#FF6B6B` | Secondary action | Planned (Theme) |

#### Current Implementation

The MVP uses Iced's built-in theme system with customization planned for Sprint 5:

```rust
// Current: Iced built-in themes
fn theme(&self) -> Theme {
    Theme::Dark // Uses Iced's default dark theme
}

// Planned: Custom RustyNES theme
fn theme(&self) -> Theme {
    Theme::custom(
        "RustyNES".to_string(),
        Palette {
            background: Color::from_rgb(0.102, 0.102, 0.180), // #1A1A2E
            text: Color::from_rgb(0.973, 0.973, 0.949),       // #F8F8F2
            primary: Color::from_rgb(0.914, 0.271, 0.376),    // #E94560
            success: Color::from_rgb(0.133, 0.773, 0.369),    // #22C55E
            danger: Color::from_rgb(0.937, 0.267, 0.267),     // #EF4444
        },
    )
}
```

### Typography Implementation

#### Current Font Usage

| Category | v2 Spec | Current | Notes |
|----------|---------|---------|-------|
| UI Text | JetBrains Mono | System Default | Font loading in Sprint 5 |
| Headers | Press Start 2P | System Default | Pixel font integration planned |
| Body | Inter | System Default | Bundled font planned |

#### Font Size Scale

The 8px base grid from v2 is prepared for implementation:

```rust
// Planned font size constants
pub const FONT_SIZE_XS: u16 = 10;    // Tooltips, timestamps
pub const FONT_SIZE_SM: u16 = 12;    // Labels, secondary text
pub const FONT_SIZE_BASE: u16 = 14;  // Body text, menus
pub const FONT_SIZE_MD: u16 = 16;    // Button labels
pub const FONT_SIZE_LG: u16 = 20;    // Section headers
pub const FONT_SIZE_XL: u16 = 24;    // View titles
pub const FONT_SIZE_2XL: u16 = 32;   // Hero text
```

### Glass Morphism Styling

The v2 specification defines glass morphism effects. Current implementation uses Iced containers with planned enhancements:

```rust
// v2 Spec: Glass Morphism
// - Background: rgba(26, 26, 46, 0.7)
// - Backdrop-filter: blur(20px) saturate(180%)
// - Border: 1px solid rgba(255, 255, 255, 0.1)
// - Shadow: 0 8px 32px rgba(0, 0, 0, 0.3)

// Current: Basic container styling
container(content)
    .width(Length::Fixed(700.0))
    .height(Length::Fixed(600.0))
    .center_x(Length::Fill)
    .center_y(Length::Fill)

// Planned: Custom container theme with glass effect
fn glass_container() -> container::Style {
    container::Style {
        background: Some(Background::Color(
            Color::from_rgba(0.102, 0.102, 0.180, 0.7)
        )),
        border: Border {
            color: Color::from_rgba(1.0, 1.0, 1.0, 0.1),
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 32.0,
        },
        ..Default::default()
    }
}
```

### Layout Grid System

The settings UI follows the v2 layout principles:

| Principle | Implementation |
|-----------|----------------|
| Fixed panel width | 700px settings panel |
| Consistent spacing | 10px tab spacing, 15px element spacing |
| Label alignment | 150px label width for consistent alignment |
| Padding | 20px container padding, 10px button padding |

### Component Styling

#### Settings Tabs

```rust
// Tab button styling follows v2 guidelines
fn tab_button(selected: SettingsTab, tab: SettingsTab) -> Button<'static, Message> {
    let style = if selected == tab {
        iced::widget::button::primary   // Highlighted tab
    } else {
        iced::widget::button::secondary // Inactive tab
    };

    button(text(tab.to_string()))
        .style(style)
        .on_press(Message::SelectSettingsTab(tab))
}
```

#### Slider Controls

Following v2's emphasis on visual feedback:

```rust
// Volume slider with percentage display
row![
    text("Master Volume:").width(Length::Fixed(150.0)),
    slider(0.0..=1.0, config.audio.volume, Message::UpdateVolume)
        .step(0.01),
    text(format!("{:.0}%", config.audio.volume * 100.0))
        .width(Length::Fixed(60.0)),
]
```

### Latency Settings (v2 Enhancement)

The v2 specification introduces advanced latency controls. Current status:

| Feature | v2 Spec | Sprint 4 | Future Sprint |
|---------|---------|----------|---------------|
| Run-Ahead Frames | 1-4 configurable | Rewind system | Sprint 5 |
| Frame Delay | 0-15 frames | Not implemented | Phase 2 |
| Auto-detection | Per-game profiles | Not implemented | Phase 2 |
| Latency Display | Real-time overlay | Not implemented | Sprint 5 |

### Responsive Design

Window resize handling enables responsive layouts:

```rust
// Window resize event tracking
Message::WindowResized(width, height) => {
    self.config.app.window_width = width as u32;
    self.config.app.window_height = height as u32;
    Command::none()
}
```

### Accessibility Considerations

Per v2 accessibility requirements:

| Requirement | Status | Notes |
|-------------|--------|-------|
| Keyboard navigation | Partial | Tab navigation via Iced |
| Screen reader support | Planned | egui accessibility features |
| High contrast mode | Planned | Theme variant |
| Font scaling | Planned | User preference setting |

### Implementation Roadmap

| Sprint | v2 Features to Implement |
|--------|--------------------------|
| Sprint 5 | Custom theme colors, basic CRT shader, performance overlay |
| Phase 2 | Run-ahead UI, latency calibration wizard, glass morphism effects |
| Phase 3 | HTPC mode, Cover Flow, controller-first navigation |

---

## Related Documentation

- [ARCHITECTURE.md](../../ARCHITECTURE.md) - Overall system architecture
- [UI/UX Design v2](../../ref-docs/RustyNES-UI_UX-Design-v2.md) - Complete UI/UX specification
- [M6-OVERVIEW.md](../../to-dos/phase-1-mvp/milestone-6-gui/M6-OVERVIEW.md) - Milestone 6 overview
- [M6-S1-iced-application.md](../../to-dos/phase-1-mvp/milestone-6-gui/M6-S1-iced-application.md) - Sprint 1 guide
- [M6-S2-wgpu-rendering.md](../../to-dos/phase-1-mvp/milestone-6-gui/M6-S2-wgpu-rendering.md) - Sprint 2 guide
- [M6-S3-input-library.md](../../to-dos/phase-1-mvp/milestone-6-gui/M6-S3-input-library.md) - Sprint 3 guide
- [M6-S4-settings-persistence.md](../../to-dos/phase-1-mvp/milestone-6-gui/M6-S4-settings-persistence.md) - Sprint 4 guide
- [M6-S5-polish-runahead.md](../../to-dos/phase-1-mvp/milestone-6-gui/M6-S5-polish-runahead.md) - Sprint 5 guide

---

<!-- End of Desktop Platform Specification -->
