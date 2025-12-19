# [Milestone 6] Sprint 6.2: wgpu Rendering Backend

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~1-2 weeks
**Assignee:** Claude Code / Developer
**Sprint:** M6-S2 (GUI - Graphics Rendering)
**Progress:** 0%

---

## Overview

This sprint implements the **wgpu rendering backend** for displaying NES frames in the egui application. This includes texture creation, framebuffer upload, nearest-neighbor scaling, and achieving 60 FPS rendering performance.

### Goals

- ⏳ wgpu initialization with egui integration
- ⏳ Texture creation (256×240 NES resolution)
- ⏳ Framebuffer upload from Console
- ⏳ Nearest-neighbor scaling (pixel-perfect)
- ⏳ Aspect ratio modes (4:3, pixel-perfect, stretch)
- ⏳ VSync toggle
- ⏳ 60 FPS rendering
- ⏳ Integer scaling option
- ⏳ Zero unsafe code

### Prerequisites

- ✅ M6-S1 egui Application Structure complete
- ✅ Console framebuffer API available

---

## Tasks

### Task 1: wgpu Setup (3 hours)

**File:** `crates/rustynes-desktop/src/renderer.rs`

**Objective:** Initialize wgpu rendering context and integrate with egui.

#### Subtasks

1. Add wgpu dependencies to Cargo.toml
2. Create `Renderer` struct
3. Initialize wgpu device, queue, surface
4. Integrate with egui via `egui_wgpu_backend`
5. Handle device selection (prefer high-performance GPU)

**Acceptance Criteria:**

- [ ] wgpu initializes successfully
- [ ] Works on all platforms (Vulkan/Metal/DX12)
- [ ] Fallback to software renderer if no GPU
- [ ] egui integration functional

**Dependencies:**

```toml
# Add to crates/rustynes-desktop/Cargo.toml

[dependencies]
wgpu = "0.18"
egui-wgpu = "0.24"  # egui wgpu backend
pollster = "0.3"    # Block on async operations
```

**Implementation:**

```rust
use wgpu::{Device, Queue, Texture, TextureView, Sampler, BindGroup};
use std::sync::Arc;

/// wgpu rendering backend for NES frames
pub struct Renderer {
    device: Arc<Device>,
    queue: Arc<Queue>,

    /// NES framebuffer texture (256×240 RGB)
    texture: Texture,
    texture_view: TextureView,

    /// Nearest-neighbor sampler (for pixel-perfect scaling)
    sampler: Sampler,

    /// Bind group for texture + sampler
    bind_group: BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,

    /// Render pipeline
    pipeline: wgpu::RenderPipeline,

    /// Scaling options
    scaling_mode: ScalingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalingMode {
    /// 4:3 aspect ratio (classic CRT)
    AspectRatio4x3,

    /// Pixel-perfect (256×240 native)
    PixelPerfect,

    /// Integer scaling (2x, 3x, 4x, etc.)
    IntegerScaling,

    /// Stretch to fill window
    Stretch,
}

impl Renderer {
    /// Create new renderer with wgpu context
    pub fn new(device: Arc<Device>, queue: Arc<Queue>) -> Self {
        // Create NES framebuffer texture (256×240 RGB)
        let texture = Self::create_framebuffer_texture(&device);
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Create nearest-neighbor sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("NES Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,  // Pixel-perfect scaling
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("NES Bind Group Layout"),
            entries: &[
                // Texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("NES Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // Create render pipeline (shaders in Task 2)
        let pipeline = Self::create_render_pipeline(&device, &bind_group_layout);

        Self {
            device,
            queue,
            texture,
            texture_view,
            sampler,
            bind_group,
            bind_group_layout,
            pipeline,
            scaling_mode: ScalingMode::AspectRatio4x3,
        }
    }

    fn create_framebuffer_texture(device: &Device) -> Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("NES Framebuffer"),
            size: wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,  // RGBA8 (convert RGB to RGBA)
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn create_render_pipeline(device: &Device, bind_group_layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
        // Shader code in Task 2
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("NES Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/nes.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("NES Pipeline Layout"),
            bind_group_layouts: &[bind_group_layout],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("NES Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Bgra8Unorm,  // egui surface format
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        })
    }

    pub fn set_scaling_mode(&mut self, mode: ScalingMode) {
        self.scaling_mode = mode;
    }

    pub fn scaling_mode(&self) -> ScalingMode {
        self.scaling_mode
    }
}
```

---

### Task 2: Shader Implementation (2 hours)

**File:** `crates/rustynes-desktop/src/shaders/nes.wgsl`

**Objective:** Create WGSL shader for rendering NES framebuffer.

#### Subtasks

1. Vertex shader (fullscreen triangle)
2. Fragment shader (texture sampling)
3. Proper coordinate mapping

**Acceptance Criteria:**

- [ ] Shader compiles on all platforms
- [ ] Texture renders correctly
- [ ] Nearest-neighbor filtering works

**Implementation:**

```wgsl
// Vertex shader output / Fragment shader input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

// Bind group 0: Texture + Sampler
@group(0) @binding(0)
var nes_texture: texture_2d<f32>;

@group(0) @binding(1)
var nes_sampler: sampler;

// Vertex shader: Fullscreen triangle
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;

    // Fullscreen triangle trick
    // Generates 3 vertices that cover the entire screen
    let x = f32((vertex_index << 1u) & 2u) - 1.0;
    let y = f32(vertex_index & 2u) - 1.0;

    output.position = vec4<f32>(x, -y, 0.0, 1.0);
    output.tex_coords = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);

    return output;
}

// Fragment shader: Sample NES texture
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(nes_texture, nes_sampler, input.tex_coords);
}
```

---

### Task 3: Framebuffer Upload (2 hours)

**File:** `crates/rustynes-desktop/src/renderer.rs` (continued)

**Objective:** Upload NES framebuffer to GPU texture every frame.

#### Subtasks

1. Convert RGB888 to RGBA8888 (add alpha channel)
2. Upload to texture via `queue.write_texture()`
3. Handle frame timing (upload once per emulated frame)

**Acceptance Criteria:**

- [ ] Framebuffer uploads correctly
- [ ] No tearing or artifacts
- [ ] Efficient (minimal copying)

**Implementation:**

```rust
impl Renderer {
    /// Update texture with new NES framebuffer
    ///
    /// # Arguments
    ///
    /// * `framebuffer` - RGB888 data (256×240 × 3 bytes = 184,320 bytes)
    pub fn update_texture(&self, framebuffer: &[u8]) {
        assert_eq!(framebuffer.len(), 256 * 240 * 3, "Invalid framebuffer size");

        // Convert RGB to RGBA (NES framebuffer is RGB, texture is RGBA)
        let rgba_data = Self::rgb_to_rgba(framebuffer);

        // Upload to GPU texture
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),  // 256 pixels × 4 bytes (RGBA)
                rows_per_image: Some(240),
            },
            wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Convert RGB888 to RGBA8888
    fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(256 * 240 * 4);

        for chunk in rgb.chunks_exact(3) {
            rgba.push(chunk[0]);  // R
            rgba.push(chunk[1]);  // G
            rgba.push(chunk[2]);  // B
            rgba.push(255);       // A (opaque)
        }

        rgba
    }

    /// Render NES frame to egui viewport
    pub fn render(&self, ui: &mut egui::Ui, window_size: egui::Vec2) {
        // Calculate viewport rectangle based on scaling mode
        let rect = self.calculate_viewport(window_size);

        // Render texture to viewport
        // (egui_wgpu integration handles actual rendering)
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);

        // TODO: Use egui_wgpu to render texture
        // This requires custom egui::PaintCallback
    }

    fn calculate_viewport(&self, window_size: egui::Vec2) -> egui::Rect {
        match self.scaling_mode {
            ScalingMode::AspectRatio4x3 => {
                // Maintain 4:3 aspect ratio
                let aspect = 4.0 / 3.0;
                let (width, height) = if window_size.x / window_size.y > aspect {
                    (window_size.y * aspect, window_size.y)
                } else {
                    (window_size.x, window_size.x / aspect)
                };

                egui::Rect::from_center_size(
                    egui::pos2(window_size.x / 2.0, window_size.y / 2.0),
                    egui::vec2(width, height),
                )
            }

            ScalingMode::PixelPerfect => {
                // Native 256×240, centered
                egui::Rect::from_center_size(
                    egui::pos2(window_size.x / 2.0, window_size.y / 2.0),
                    egui::vec2(256.0, 240.0),
                )
            }

            ScalingMode::IntegerScaling => {
                // Integer multiples (2x, 3x, 4x, etc.)
                let max_scale_x = (window_size.x / 256.0).floor() as u32;
                let max_scale_y = (window_size.y / 240.0).floor() as u32;
                let scale = max_scale_x.min(max_scale_y).max(1) as f32;

                let width = 256.0 * scale;
                let height = 240.0 * scale;

                egui::Rect::from_center_size(
                    egui::pos2(window_size.x / 2.0, window_size.y / 2.0),
                    egui::vec2(width, height),
                )
            }

            ScalingMode::Stretch => {
                // Fill entire window
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), window_size)
            }
        }
    }
}
```

---

### Task 4: egui Integration (3 hours)

**File:** `crates/rustynes-desktop/src/app.rs`

**Objective:** Integrate wgpu renderer with egui application.

#### Subtasks

1. Initialize renderer in app creation
2. Call `update_texture()` each frame
3. Render to egui viewport via custom paint callback
4. Handle window resize

**Acceptance Criteria:**

- [ ] Texture renders in egui viewport
- [ ] Frame updates smoothly
- [ ] Resizing works correctly

**Implementation:**

```rust
use crate::renderer::{Renderer, ScalingMode};

pub struct RustyNesApp {
    // ... existing fields ...

    /// wgpu renderer
    renderer: Renderer,
}

impl RustyNesApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Extract wgpu context from eframe
        let wgpu_render_state = cc.wgpu_render_state.as_ref()
            .expect("eframe must be configured with wgpu backend");

        let device = wgpu_render_state.device.clone();
        let queue = wgpu_render_state.queue.clone();

        // Create renderer
        let renderer = Renderer::new(device, queue);

        Self {
            console: None,
            state: EmulationState::NoRom,
            fps_counter: FpsCounter::new(),
            rom_path: None,
            last_frame: Instant::now(),
            renderer,
        }
    }

    fn render_game_viewport(&mut self, ui: &mut egui::Ui) {
        if let Some(console) = &self.console {
            // Get framebuffer from console
            let framebuffer = console.framebuffer();

            // Upload to GPU
            self.renderer.update_texture(framebuffer);

            // Render to viewport
            let window_size = ui.available_size();
            let viewport_rect = self.renderer.calculate_viewport(window_size);

            // Custom paint callback for wgpu rendering
            let callback = egui::PaintCallback {
                rect: viewport_rect,
                callback: std::sync::Arc::new(egui_wgpu::CallbackFn::new(move |_info, render_pass| {
                    // Render NES texture
                    // This executes on GPU thread
                    // TODO: Implement render pass
                })),
            };

            ui.painter().add(callback);
        }
    }
}
```

---

### Task 5: Video Settings (2 hours)

**File:** `crates/rustynes-desktop/src/ui/video_settings.rs`

**Objective:** Add video settings UI for scaling mode, vsync, etc.

#### Subtasks

1. Create video settings window
2. Scaling mode selection (4:3, pixel-perfect, integer, stretch)
3. VSync toggle
4. Show current resolution

**Acceptance Criteria:**

- [ ] Settings window opens from menu
- [ ] Changes apply immediately
- [ ] Settings persist (Sprint 5)

**Implementation:**

```rust
use eframe::egui;
use crate::app::RustyNesApp;
use crate::renderer::ScalingMode;

impl RustyNesApp {
    pub fn show_video_settings(&mut self, ctx: &egui::Context) {
        egui::Window::new("Video Settings")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Scaling Mode");

                ui.horizontal(|ui| {
                    if ui.radio(self.renderer.scaling_mode() == ScalingMode::AspectRatio4x3, "4:3 Aspect Ratio").clicked() {
                        self.renderer.set_scaling_mode(ScalingMode::AspectRatio4x3);
                    }
                });

                ui.horizontal(|ui| {
                    if ui.radio(self.renderer.scaling_mode() == ScalingMode::PixelPerfect, "Pixel Perfect (256×240)").clicked() {
                        self.renderer.set_scaling_mode(ScalingMode::PixelPerfect);
                    }
                });

                ui.horizontal(|ui| {
                    if ui.radio(self.renderer.scaling_mode() == ScalingMode::IntegerScaling, "Integer Scaling (2x, 3x, 4x...)").clicked() {
                        self.renderer.set_scaling_mode(ScalingMode::IntegerScaling);
                    }
                });

                ui.horizontal(|ui| {
                    if ui.radio(self.renderer.scaling_mode() == ScalingMode::Stretch, "Stretch to Fill").clicked() {
                        self.renderer.set_scaling_mode(ScalingMode::Stretch);
                    }
                });

                ui.separator();

                ui.heading("Performance");

                // VSync toggle (requires eframe configuration)
                ui.label("VSync: Enabled (configure in main.rs)");

                ui.separator();

                // Current resolution display
                if let Some(_console) = &self.console {
                    ui.label("Native Resolution: 256 × 240");
                    ui.label(format!("FPS: {:.1}", self.fps_counter.fps()));
                }
            });
    }
}
```

---

### Task 6: Performance Optimization (2 hours)

**File:** `crates/rustynes-desktop/src/renderer.rs` (optimization pass)

**Objective:** Ensure 60 FPS rendering performance.

#### Subtasks

1. Profile texture upload performance
2. Minimize allocations (reuse RGBA buffer)
3. Batch rendering calls
4. Benchmark on low-end hardware

**Acceptance Criteria:**

- [ ] Consistent 60 FPS on target hardware
- [ ] Frame time <16ms
- [ ] No stutter or jank

**Optimization:**

```rust
impl Renderer {
    // Cache RGBA buffer to avoid allocations
    rgba_buffer: Vec<u8>,

    pub fn new(device: Arc<Device>, queue: Arc<Queue>) -> Self {
        // ... existing code ...

        // Pre-allocate RGBA buffer
        let rgba_buffer = vec![0u8; 256 * 240 * 4];

        Self {
            // ... existing fields ...
            rgba_buffer,
        }
    }

    /// Update texture (optimized, no allocations)
    pub fn update_texture(&mut self, framebuffer: &[u8]) {
        assert_eq!(framebuffer.len(), 256 * 240 * 3);

        // Convert RGB to RGBA in-place (reuse buffer)
        for (i, chunk) in framebuffer.chunks_exact(3).enumerate() {
            let offset = i * 4;
            self.rgba_buffer[offset] = chunk[0];      // R
            self.rgba_buffer[offset + 1] = chunk[1];  // G
            self.rgba_buffer[offset + 2] = chunk[2];  // B
            self.rgba_buffer[offset + 3] = 255;       // A
        }

        // Upload to GPU
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba_buffer,
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
}
```

---

## Acceptance Criteria

### Functionality

- [ ] wgpu initializes on all platforms
- [ ] NES frames render correctly
- [ ] Nearest-neighbor filtering (pixel-perfect)
- [ ] All scaling modes work (4:3, pixel-perfect, integer, stretch)
- [ ] 60 FPS rendering
- [ ] VSync works
- [ ] Resizing smooth

### Quality

- [ ] No tearing or artifacts
- [ ] Crisp pixel art rendering
- [ ] Consistent frame timing
- [ ] Zero unsafe code

---

## Dependencies

### External Crates

```toml
wgpu = "0.18"
egui-wgpu = "0.24"
pollster = "0.3"
```

---

## Related Documentation

- [M6-S1-egui-application.md](M6-S1-egui-application.md) - Application structure
- [M6-OVERVIEW.md](M6-OVERVIEW.md) - Milestone overview

---

## Performance Targets

- **Frame Time:** <16ms (60 FPS)
- **Texture Upload:** <1ms
- **Render Pass:** <5ms
- **Total Frame Budget:** 16.67ms (60 Hz)

---

## Success Criteria

- [ ] All tasks complete
- [ ] 60 FPS on target hardware
- [ ] Pixel-perfect rendering
- [ ] All scaling modes work
- [ ] Video settings functional
- [ ] Ready for audio integration (Sprint 3)

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1 (Application shell)
**Next Sprint:** [M6-S3 Audio Output](M6-S3-audio-output.md)
