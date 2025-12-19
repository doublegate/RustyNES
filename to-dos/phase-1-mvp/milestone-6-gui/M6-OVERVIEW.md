# Milestone 6: Desktop GUI

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~3-4 weeks (estimated)
**Progress:** 0%

---

## Overview

Milestone 6 will deliver a **cross-platform desktop application** with egui interface, wgpu rendering, audio output, and controller support. This completes the Phase 1 MVP.

### Goals

- ⏳ egui-based user interface
- ⏳ wgpu rendering backend (60 FPS)
- ⏳ Audio output (SDL2 or cpal)
- ⏳ Gamepad support (gilrs)
- ⏳ Keyboard input
- ⏳ Configuration system
- ⏳ File browser for ROMs
- ⏳ Save state hotkeys
- ⏳ Cross-platform (Linux, Windows, macOS)
- ⏳ Zero unsafe code (except FFI)

---

## Sprint Breakdown

### Sprint 1: egui Application Structure ⏳ PENDING

**Duration:** Week 1
**Target Files:** `crates/rustynes-desktop/src/main.rs`, `app.rs`, `ui/`

**Goals:**

- [ ] egui window creation
- [ ] Menu bar (File, Emulation, Settings, Help)
- [ ] File → Open ROM dialog
- [ ] File → Exit
- [ ] Emulation → Reset, Pause/Resume
- [ ] Main viewport for game screen
- [ ] Status bar (FPS, frame time)
- [ ] Keyboard shortcuts

**Outcome:** Basic application shell.

### Sprint 2: wgpu Rendering Backend ⏳ PENDING

**Duration:** Week 1-2
**Target Files:** `crates/rustynes-desktop/src/renderer.rs`

**Goals:**

- [ ] wgpu initialization
- [ ] Texture creation (256×240 NES frame)
- [ ] Texture update from framebuffer
- [ ] Nearest-neighbor scaling (pixel-perfect)
- [ ] Aspect ratio modes (4:3, pixel-perfect, stretch)
- [ ] Vsync toggle
- [ ] 60 FPS rendering
- [ ] Integer scaling option

**Outcome:** Game rendering at 60 FPS.

### Sprint 3: Audio Output ⏳ PENDING

**Duration:** Week 2
**Target Files:** `crates/rustynes-desktop/src/audio.rs`

**Goals:**

- [ ] Audio backend (cpal recommended)
- [ ] Ring buffer for APU samples
- [ ] Resampling (APU → 48 kHz)
- [ ] Audio callback
- [ ] Volume control
- [ ] Mute toggle
- [ ] <20ms latency
- [ ] No crackling/popping

**Outcome:** Clean audio output.

### Sprint 4: Controller Support ⏳ PENDING

**Duration:** Week 3
**Target Files:** `crates/rustynes-desktop/src/input.rs`

**Goals:**

- [ ] Keyboard input (arrow keys, Z/X for A/B)
- [ ] Gamepad detection (gilrs)
- [ ] Gamepad button mapping
- [ ] Controller configuration UI
- [ ] Save controller mappings
- [ ] Player 1/2 selection
- [ ] Hotkey support (F1-F12 for save states)

**Outcome:** Full input support.

### Sprint 5: Configuration & Polish ⏳ PENDING

**Duration:** Week 3-4
**Target Files:** `crates/rustynes-desktop/src/config.rs`, `ui/settings.rs`

**Goals:**

- [ ] Configuration file (TOML)
- [ ] Settings window
  - [ ] Video settings (scale, filter, aspect ratio)
  - [ ] Audio settings (volume, sample rate)
  - [ ] Input settings (keyboard/gamepad mapping)
  - [ ] Paths (ROM directory, save states)
- [ ] Recent ROMs list
- [ ] About window
- [ ] Error dialogs
- [ ] Icon and window title
- [ ] Cross-platform packaging

**Outcome:** Polished desktop application.

---

## Technical Requirements

### egui Application

```rust
use eframe::egui;

struct RustyNesApp {
    console: Option<Console>,
    renderer: Renderer,
    audio: AudioOutput,
    input: InputManager,
    config: Config,
    paused: bool,
}

impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open ROM...").clicked() {
                        self.open_rom_dialog();
                    }
                    if ui.button("Exit").clicked() {
                        std::process::exit(0);
                    }
                });

                ui.menu_button("Emulation", |ui| {
                    if ui.button("Reset").clicked() {
                        if let Some(console) = &mut self.console {
                            console.reset();
                        }
                    }
                    if ui.button(if self.paused { "Resume" } else { "Pause" }).clicked() {
                        self.paused = !self.paused;
                    }
                });

                ui.menu_button("Settings", |ui| {
                    if ui.button("Video...").clicked() {
                        self.show_video_settings = true;
                    }
                    if ui.button("Audio...").clicked() {
                        self.show_audio_settings = true;
                    }
                });
            });
        });

        // Game viewport
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(console) = &mut self.console {
                if !self.paused {
                    console.step_frame();
                    self.audio.queue_samples(console.audio_buffer());
                }

                let framebuffer = console.framebuffer();
                self.renderer.update_texture(framebuffer);
                self.renderer.render(ui);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("No ROM loaded. File → Open ROM to begin.");
                });
            }
        });

        // Status bar
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("FPS: {:.1}", self.fps()));
                ui.separator();
                ui.label(if self.paused { "Paused" } else { "Running" });
            });
        });

        // Request repaint (60 FPS)
        ctx.request_repaint();
    }
}
```

### wgpu Renderer

```rust
pub struct Renderer {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    width: u32,
    height: u32,
}

impl Renderer {
    pub fn update_texture(&mut self, framebuffer: &[u8]) {
        // Update wgpu texture with NES framebuffer (256×240 RGB)
        let queue = /* ... */;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            framebuffer,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256 * 3),
                rows_per_image: Some(240),
            },
            wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn render(&self, ui: &mut egui::Ui) {
        // Render texture to egui viewport
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(self.width as f32, self.height as f32),
            egui::Sense::hover(),
        );

        // Use egui_wgpu_backend to render texture
    }
}
```

### Audio Output

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct AudioOutput {
    _stream: cpal::Stream,
    producer: ringbuf::Producer<f32>,
}

impl AudioOutput {
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(AudioError::NoDevice)?;
        let config = device.default_output_config()?;

        let (mut producer, mut consumer) = ringbuf::RingBuffer::<f32>::new(4096).split();

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data {
                    *sample = consumer.pop().unwrap_or(0.0);
                }
            },
            |err| eprintln!("Audio error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self { _stream: stream, producer })
    }

    pub fn queue_samples(&mut self, samples: &[f32]) {
        for &sample in samples {
            let _ = self.producer.push(sample);  // Drop if full
        }
    }
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Loads ROMs via file dialog
- [ ] Renders at 60 FPS (16.67ms frame time)
- [ ] Audio plays without crackling
- [ ] Keyboard controls work
- [ ] Gamepad detected and usable
- [ ] Can reset emulation
- [ ] Can pause/resume
- [ ] Settings persist across sessions
- [ ] Cross-platform builds work

### User Experience

- [ ] Intuitive menus
- [ ] Responsive UI (no lag)
- [ ] Clear error messages
- [ ] Keyboard shortcuts work
- [ ] Window resizes correctly
- [ ] Fullscreen mode works
- [ ] Recent ROMs accessible

### Quality

- [ ] No crashes on invalid ROMs
- [ ] No audio crackling
- [ ] Smooth scrolling in menus
- [ ] No visual tearing
- [ ] Clean shutdown

---

## Code Structure

```text
crates/rustynes-desktop/
├── src/
│   ├── main.rs          # Entry point
│   ├── app.rs           # Main application struct
│   ├── renderer.rs      # wgpu rendering
│   ├── audio.rs         # Audio output
│   ├── input.rs         # Input handling
│   ├── config.rs        # Configuration management
│   └── ui/
│       ├── mod.rs
│       ├── menu_bar.rs
│       ├── settings.rs
│       └── dialogs.rs
├── assets/
│   └── icon.png         # Application icon
└── Cargo.toml
```

**Estimated Total:** ~2,000-2,500 lines of code

---

## Dependencies

### External Crates

- **eframe** - egui application framework
- **egui** - Immediate mode GUI
- **egui_wgpu_backend** - wgpu rendering for egui
- **wgpu** - Graphics API
- **cpal** - Cross-platform audio library
- **gilrs** - Gamepad input
- **rfd** - Native file dialogs
- **serde** / **toml** - Configuration serialization
- **ringbuf** - Lock-free ring buffer

### Internal Dependencies

- rustynes-core

---

## Testing Strategy

### Manual Testing

- [ ] Load 10 different ROMs
- [ ] Test keyboard controls
- [ ] Test gamepad controls
- [ ] Test settings persistence
- [ ] Test on Linux, Windows, macOS
- [ ] Test fullscreen mode
- [ ] Test save states

### Integration Tests

- [ ] ROM loading
- [ ] Audio callback
- [ ] Input injection
- [ ] Configuration save/load

---

## Performance Targets

- **Frame Rate:** 60 FPS (16.67ms)
- **Frame Time:** <16ms emulation + rendering
- **Audio Latency:** <20ms
- **Memory:** <100 MB
- **Startup Time:** <500ms

---

## Challenges & Risks

| Challenge | Risk | Mitigation |
|-----------|------|------------|
| Audio latency/crackling | Medium | Use cpal, tune ring buffer size |
| Cross-platform builds | Low | CI matrix testing |
| Gamepad compatibility | Low | Test with common controllers |
| Performance on low-end hardware | Low | Profile and optimize hot paths |

---

## Platform-Specific Notes

### Linux

- Dependencies: libxcb, libasound, libudev
- Package: AppImage or .deb
- Test on Ubuntu 22.04+, Fedora 38+

### Windows

- Dependencies: None (static linking)
- Package: .exe installer or portable .zip
- Test on Windows 10+

### macOS

- Dependencies: None
- Package: .dmg or .app bundle
- Code signing required for distribution
- Test on macOS 12+

---

## Related Documentation

- [Desktop Frontend](../../../docs/platform/DESKTOP.md)
- [Build Instructions](../../../docs/dev/BUILD.md)
- [Configuration](../../../docs/api/CONFIGURATION.md)

---

## Next Steps

### Pre-Sprint Preparation

1. **Set Up Crate**
   - Create rustynes-desktop/Cargo.toml
   - Add all dependencies
   - Create basic window

2. **Research**
   - Study egui examples
   - Review wgpu texture updates
   - Test cpal audio on all platforms

3. **Design**
   - Mockup UI layout
   - Plan settings structure
   - Design keyboard/gamepad mappings

### Sprint 1 Kickoff

- Create egui window
- Implement menu bar
- Add file dialog
- Create main viewport
- Load and display ROM

---

**Milestone Status:** ⏳ PENDING
**Blocked By:** M5 ⏳ (needs rustynes-core)
**Deliverable:** Phase 1 MVP Complete!
