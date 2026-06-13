# [Milestone 6] Sprint 6.2: wgpu Rendering Backend

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** 1 week (40 hours)
**Sprint:** M6-S2 (wgpu Game Viewport)
**Architecture:** Iced 0.13+ with wgpu custom rendering
**Progress:** 0%

---

## Overview

This sprint implements the **wgpu-based game viewport** within the Iced 0.13+ architecture. Unlike traditional egui approaches, Iced provides a widget-based system with explicit state management (Elm architecture). The viewport will render NES frames at 60 FPS with nearest-neighbor filtering for pixel-perfect accuracy.

### Goals

- ⏳ wgpu custom widget for NES framebuffer (256×240)
- ⏳ Iced `canvas::Program` trait implementation
- ⏳ Texture upload pipeline (RGB888 → GPU)
- ⏳ Nearest-neighbor filtering (pixel-perfect scaling)
- ⏳ Aspect ratio modes (4:3, 8:7, stretch)
- ⏳ Integer scaling support
- ⏳ 60 FPS rendering target
- ⏳ Zero unsafe code
- ⏳ Integration with Iced update loop

### Prerequisites

- ✅ M6-S1 Iced Application Foundation complete
- ✅ Console framebuffer API available (from Phase 1 M1-M5)
- ✅ wgpu 0.18+ and Iced 0.13+ dependencies

---

## Tasks

### Task 1: wgpu Texture Setup (4 hours)

**Files:**
- `crates/rustynes-desktop/src/viewport/mod.rs` (new)
- `crates/rustynes-desktop/src/viewport/texture.rs` (new)

**Objective:** Create NES framebuffer texture and upload pipeline.

#### 1.1 Create Texture Structure

```rust
// viewport/texture.rs
use wgpu::{Device, Queue, Texture, TextureView, TextureFormat};

/// NES framebuffer texture (256×240 RGB888 → RGBA8 GPU)
pub struct NesTexture {
    texture: Texture,
    view: TextureView,
    width: u32,
    height: u32,
}

impl NesTexture {
    pub const NES_WIDTH: u32 = 256;
    pub const NES_HEIGHT: u32 = 240;

    pub fn new(device: &Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("NES Framebuffer"),
            size: wgpu::Extent3d {
                width: Self::NES_WIDTH,
                height: Self::NES_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            texture,
            view,
            width: Self::NES_WIDTH,
            height: Self::NES_HEIGHT,
        }
    }

    /// Upload NES framebuffer (RGB888) to GPU
    pub fn update(&self, queue: &Queue, framebuffer: &[u8]) {
        assert_eq!(
            framebuffer.len(),
            (Self::NES_WIDTH * Self::NES_HEIGHT * 3) as usize,
            "Invalid framebuffer size"
        );

        // Convert RGB888 → RGBA8888
        let rgba = Self::rgb_to_rgba(framebuffer);

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(Self::NES_WIDTH * 4),
                rows_per_image: Some(Self::NES_HEIGHT),
            },
            wgpu::Extent3d {
                width: Self::NES_WIDTH,
                height: Self::NES_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
    }

    fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((Self::NES_WIDTH * Self::NES_HEIGHT * 4) as usize);

        for chunk in rgb.chunks_exact(3) {
            rgba.push(chunk[0]); // R
            rgba.push(chunk[1]); // G
            rgba.push(chunk[2]); // B
            rgba.push(255);      // A
        }

        rgba
    }

    pub fn view(&self) -> &TextureView {
        &self.view
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
```

#### 1.2 Sampler Configuration

```rust
// viewport/texture.rs (continued)
use wgpu::Sampler;

pub struct NesSampler {
    sampler: Sampler,
}

impl NesSampler {
    pub fn new(device: &Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("NES Sampler (Nearest)"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest, // Pixel-perfect
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self { sampler }
    }

    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }
}
```

**Acceptance Criteria:**
- [ ] Texture created with correct dimensions (256×240)
- [ ] RGB → RGBA conversion functional
- [ ] Nearest-neighbor sampler configured
- [ ] Texture upload completes <1ms

---

### Task 2: Iced Custom Widget (6 hours)

**Files:**
- `crates/rustynes-desktop/src/viewport/widget.rs` (new)
- `crates/rustynes-desktop/src/viewport/primitive.rs` (new)

**Objective:** Create Iced custom widget for rendering NES viewport.

#### 2.1 Viewport Widget Structure

```rust
// viewport/widget.rs
use iced::widget::{canvas, Canvas};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size};
use std::sync::Arc;

pub struct GameViewport {
    framebuffer: Arc<Vec<u8>>, // Shared with emulator thread
    scaling: ScalingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalingMode {
    /// 4:3 aspect ratio (classic CRT)
    AspectRatio4x3,
    /// 8:7 pixel aspect ratio (authentic NES)
    PixelPerfect,
    /// Integer scaling (2x, 3x, 4x, etc.)
    IntegerScaling,
    /// Stretch to fill
    Stretch,
}

impl GameViewport {
    pub fn new(framebuffer: Arc<Vec<u8>>) -> Self {
        Self {
            framebuffer,
            scaling: ScalingMode::PixelPerfect,
        }
    }

    pub fn scaling(mut self, mode: ScalingMode) -> Self {
        self.scaling = mode;
        self
    }
}

impl<Message> canvas::Program<Message> for GameViewport {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        // Rendering implementation in Task 2.2
        vec![]
    }
}

// Convert to Iced Element
impl<'a, Message: 'a> From<GameViewport> for Element<'a, Message> {
    fn from(viewport: GameViewport) -> Self {
        Canvas::new(viewport)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
```

#### 2.2 Custom Primitive for wgpu Rendering

**Note:** Iced 0.13+ requires custom primitives for low-level wgpu access. This is more complex than egui's `PaintCallback`.

```rust
// viewport/primitive.rs
use iced_graphics::Primitive;
use iced_wgpu::primitive::Custom;
use std::sync::Arc;

pub struct NesViewportPrimitive {
    pub framebuffer: Arc<Vec<u8>>,
    pub bounds: iced::Rectangle,
    pub scaling: super::ScalingMode,
}

impl NesViewportPrimitive {
    pub fn new(
        framebuffer: Arc<Vec<u8>>,
        bounds: iced::Rectangle,
        scaling: super::ScalingMode,
    ) -> Primitive {
        Primitive::Custom(Box::new(Self {
            framebuffer,
            bounds,
            scaling,
        }))
    }
}

impl Custom for NesViewportPrimitive {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        storage: &mut iced_wgpu::primitive::Storage,
    ) {
        // Upload framebuffer to GPU texture
        // Store pipeline state in storage
        // Implementation in Task 3
    }

    fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        storage: &iced_wgpu::primitive::Storage,
    ) {
        // Render NES texture to target
        // Implementation in Task 3
    }
}
```

**Acceptance Criteria:**
- [ ] Canvas widget integrates with Iced element tree
- [ ] Custom primitive compiles and registers
- [ ] Widget responds to layout changes
- [ ] No panics in draw loop

---

### Task 3: Render Pipeline (8 hours)

**Files:**
- `crates/rustynes-desktop/src/viewport/pipeline.rs` (new)
- `crates/rustynes-desktop/src/viewport/shaders.wgsl` (new)

**Objective:** Implement wgpu render pipeline for NES texture → screen.

#### 3.1 Shader (WGSL)

```wgsl
// viewport/shaders.wgsl
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@group(0) @binding(0)
var nes_texture: texture_2d<f32>;

@group(0) @binding(1)
var nes_sampler: sampler;

// Fullscreen triangle vertex shader
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;

    // Fullscreen triangle trick (3 vertices cover entire screen)
    let x = f32((vertex_index << 1u) & 2u) - 1.0;
    let y = f32(vertex_index & 2u) - 1.0;

    output.position = vec4<f32>(x, -y, 0.0, 1.0);
    output.tex_coords = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);

    return output;
}

// Fragment shader with nearest-neighbor sampling
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(nes_texture, nes_sampler, input.tex_coords);
}
```

#### 3.2 Pipeline Setup

```rust
// viewport/pipeline.rs
use wgpu::{BindGroup, BindGroupLayout, Device, RenderPipeline, TextureFormat};

pub struct NesRenderPipeline {
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
}

impl NesRenderPipeline {
    pub fn new(device: &Device, target_format: TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("NES Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });

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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("NES Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                    format: target_format,
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
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    pub fn create_bind_group(
        &self,
        device: &Device,
        texture_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("NES Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        bind_group: &BindGroup,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("NES Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1); // Fullscreen triangle
    }
}
```

**Acceptance Criteria:**
- [ ] Shader compiles without errors
- [ ] Pipeline created successfully
- [ ] Bind group setup functional
- [ ] Render pass executes at 60 FPS

---

### Task 4: Scaling Modes (4 hours)

**Files:**
- `crates/rustynes-desktop/src/viewport/scaling.rs` (new)

**Objective:** Implement aspect ratio and scaling logic.

```rust
// viewport/scaling.rs
use iced::{Rectangle, Size};

pub fn calculate_viewport(
    window_size: Size,
    mode: super::ScalingMode,
) -> Rectangle {
    const NES_WIDTH: f32 = 256.0;
    const NES_HEIGHT: f32 = 240.0;

    match mode {
        super::ScalingMode::AspectRatio4x3 => {
            // Maintain 4:3 aspect ratio
            let aspect = 4.0 / 3.0;
            let (width, height) = if window_size.width / window_size.height > aspect {
                (window_size.height * aspect, window_size.height)
            } else {
                (window_size.width, window_size.width / aspect)
            };

            Rectangle {
                x: (window_size.width - width) / 2.0,
                y: (window_size.height - height) / 2.0,
                width,
                height,
            }
        }

        super::ScalingMode::PixelPerfect => {
            // 8:7 pixel aspect ratio (authentic NES)
            let pixel_aspect = 8.0 / 7.0;
            let aspect = (NES_WIDTH / NES_HEIGHT) * pixel_aspect;

            let (width, height) = if window_size.width / window_size.height > aspect {
                (window_size.height * aspect, window_size.height)
            } else {
                (window_size.width, window_size.width / aspect)
            };

            Rectangle {
                x: (window_size.width - width) / 2.0,
                y: (window_size.height - height) / 2.0,
                width,
                height,
            }
        }

        super::ScalingMode::IntegerScaling => {
            // Integer multiples (2x, 3x, 4x, etc.)
            let max_scale_x = (window_size.width / NES_WIDTH).floor();
            let max_scale_y = (window_size.height / NES_HEIGHT).floor();
            let scale = max_scale_x.min(max_scale_y).max(1.0);

            let width = NES_WIDTH * scale;
            let height = NES_HEIGHT * scale;

            Rectangle {
                x: (window_size.width - width) / 2.0,
                y: (window_size.height - height) / 2.0,
                width,
                height,
            }
        }

        super::ScalingMode::Stretch => {
            // Fill entire window
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: window_size.width,
                height: window_size.height,
            }
        }
    }
}
```

**Acceptance Criteria:**
- [ ] 4:3 aspect ratio correct
- [ ] Pixel-perfect (8:7) aspect ratio correct
- [ ] Integer scaling works (2x, 3x, 4x)
- [ ] Stretch mode fills window
- [ ] Viewport centered correctly

---

### Task 5: Integration with Iced Application (6 hours)

**Files:**
- `crates/rustynes-desktop/src/main.rs`
- `crates/rustynes-desktop/src/lib.rs`
- `crates/rustynes-desktop/src/views/gameplay.rs`

**Objective:** Wire viewport into Iced Model-Update-View loop.

#### 5.1 Update Model

```rust
// lib.rs (Model)
use std::sync::Arc;

pub struct RustyNesModel {
    // ... existing fields from M6-S1 ...

    /// Shared framebuffer (updated by emulator thread)
    pub framebuffer: Arc<Vec<u8>>,

    /// Viewport scaling mode
    pub scaling_mode: viewport::ScalingMode,
}

impl Default for RustyNesModel {
    fn default() -> Self {
        Self {
            framebuffer: Arc::new(vec![0u8; 256 * 240 * 3]),
            scaling_mode: viewport::ScalingMode::PixelPerfect,
            // ... other defaults ...
        }
    }
}
```

#### 5.2 Update Gameplay View

```rust
// views/gameplay.rs
use iced::{Element, Length};
use crate::viewport::GameViewport;

pub fn view<'a>(model: &'a RustyNesModel) -> Element<'a, Message> {
    let viewport = GameViewport::new(Arc::clone(&model.framebuffer))
        .scaling(model.scaling_mode);

    iced::widget::container(viewport)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x()
        .center_y()
        .into()
}
```

#### 5.3 Handle Framebuffer Updates

```rust
// lib.rs (Update)
impl RustyNesModel {
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::EmulatorTick => {
                // Emulator thread updates framebuffer via Arc
                // Iced runtime automatically redraws
                iced::Task::none()
            }
            Message::SetScalingMode(mode) => {
                self.scaling_mode = mode;
                iced::Task::none()
            }
            // ... other messages ...
        }
    }
}
```

**Acceptance Criteria:**
- [ ] Viewport widget renders in Gameplay view
- [ ] Framebuffer updates trigger redraws
- [ ] Scaling mode changes apply immediately
- [ ] No performance degradation from Elm architecture

---

### Task 6: Performance Optimization (4 hours)

**Files:**
- `crates/rustynes-desktop/src/viewport/cache.rs` (new)

**Objective:** Optimize texture uploads and rendering for 60 FPS.

#### 6.1 Frame Caching

```rust
// viewport/cache.rs
use std::sync::Arc;

pub struct FramebufferCache {
    last_framebuffer: Vec<u8>,
    dirty: bool,
}

impl FramebufferCache {
    pub fn new() -> Self {
        Self {
            last_framebuffer: vec![0u8; 256 * 240 * 3],
            dirty: true,
        }
    }

    /// Check if framebuffer changed (avoid redundant uploads)
    pub fn check_dirty(&mut self, current: &[u8]) -> bool {
        if current != self.last_framebuffer.as_slice() {
            self.last_framebuffer.copy_from_slice(current);
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
```

#### 6.2 Reuse RGBA Buffer

```rust
// viewport/texture.rs (optimization)
impl NesTexture {
    // Pre-allocated RGBA buffer (avoid allocations)
    rgba_buffer: Vec<u8>,

    pub fn new(device: &Device) -> Self {
        // ... existing code ...

        let rgba_buffer = vec![0u8; (Self::NES_WIDTH * Self::NES_HEIGHT * 4) as usize];

        Self {
            texture,
            view,
            width: Self::NES_WIDTH,
            height: Self::NES_HEIGHT,
            rgba_buffer,
        }
    }

    pub fn update_optimized(&mut self, queue: &Queue, framebuffer: &[u8]) {
        // Convert RGB → RGBA in-place (no allocations)
        for (i, chunk) in framebuffer.chunks_exact(3).enumerate() {
            let offset = i * 4;
            self.rgba_buffer[offset] = chunk[0];
            self.rgba_buffer[offset + 1] = chunk[1];
            self.rgba_buffer[offset + 2] = chunk[2];
            self.rgba_buffer[offset + 3] = 255;
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba_buffer,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(Self::NES_WIDTH * 4),
                rows_per_image: Some(Self::NES_HEIGHT),
            },
            wgpu::Extent3d {
                width: Self::NES_WIDTH,
                height: Self::NES_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
    }
}
```

**Acceptance Criteria:**
- [ ] Frame time <16ms (60 FPS)
- [ ] Texture upload <1ms
- [ ] No allocations in hot path
- [ ] CPU usage <10% for rendering

---

### Task 7: Video Settings UI (4 hours)

**Files:**
- `crates/rustynes-desktop/src/views/settings.rs`

**Objective:** Add video settings to Settings view.

```rust
// views/settings.rs
use iced::{Element, widget::{column, row, text, pick_list}};
use crate::viewport::ScalingMode;

pub fn video_tab<'a>(model: &'a RustyNesModel) -> Element<'a, Message> {
    column![
        text("Video Settings").size(24),

        row![
            text("Scaling Mode:"),
            pick_list(
                &[
                    ScalingMode::PixelPerfect,
                    ScalingMode::AspectRatio4x3,
                    ScalingMode::IntegerScaling,
                    ScalingMode::Stretch,
                ],
                Some(model.scaling_mode),
                Message::SetScalingMode
            )
        ].spacing(10),

        text("Pixel Perfect: 8:7 PAR (authentic)"),
        text("4:3 Aspect: CRT television"),
        text("Integer Scaling: Sharp pixels (2x, 3x, 4x)"),
        text("Stretch: Fill window"),
    ]
    .spacing(20)
    .into()
}

impl std::fmt::Display for ScalingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PixelPerfect => write!(f, "Pixel Perfect (8:7)"),
            Self::AspectRatio4x3 => write!(f, "4:3 Aspect Ratio"),
            Self::IntegerScaling => write!(f, "Integer Scaling"),
            Self::Stretch => write!(f, "Stretch to Fill"),
        }
    }
}
```

**Acceptance Criteria:**
- [ ] Video settings accessible from Settings view
- [ ] Scaling mode changes apply immediately
- [ ] UI explains each scaling mode
- [ ] Keyboard shortcut (Ctrl+V) opens video settings

---

## Acceptance Criteria

### Functionality

- [ ] NES frames render correctly at 256×240
- [ ] Nearest-neighbor filtering (pixel-perfect)
- [ ] All scaling modes work (4:3, 8:7, integer, stretch)
- [ ] 60 FPS rendering consistent
- [ ] Texture uploads efficient (<1ms)
- [ ] Viewport integrates with Iced layout system
- [ ] Video settings UI functional

### Quality

- [ ] No tearing or artifacts
- [ ] Crisp pixel art rendering
- [ ] Frame timing consistent
- [ ] Zero unsafe code
- [ ] No Clippy warnings (`clippy::pedantic`)

### Performance

- [ ] Frame time: <16ms (60 FPS)
- [ ] Texture upload: <1ms
- [ ] Render pass: <5ms
- [ ] CPU usage: <10%

---

## Dependencies

### External Crates

```toml
[dependencies]
wgpu = "0.18"
iced = { version = "0.13", features = ["wgpu", "canvas"] }
iced_wgpu = "0.13"
```

---

## Related Documentation

- [M6-S1-iced-application.md](M6-S1-iced-application.md) - Iced application foundation
- [M6-OVERVIEW.md](M6-OVERVIEW.md) - Milestone overview
- [Iced Canvas Documentation](https://docs.rs/iced/latest/iced/widget/canvas/index.html)

---

## Technical Notes

### Iced vs egui Architecture

**Key Differences:**

| Aspect | egui (Old) | Iced 0.13+ (New) |
|--------|------------|------------------|
| Rendering | Immediate mode | Retained mode |
| State | Mutable refs | Elm architecture |
| Custom GPU | `PaintCallback` | `canvas::Program` + primitives |
| Complexity | Low | Medium-High |

**Trade-offs:**

- **Iced Pros:** Better state management, cleaner architecture, proper MVC
- **Iced Cons:** More boilerplate, custom primitives complex
- **Decision:** Worth it for 8+ views and advanced UI needs

### wgpu Integration Challenges

1. **Custom Primitives:** Iced doesn't provide direct wgpu access like egui. Must implement `Custom` trait.
2. **Lifetime Management:** Texture/pipeline must live in persistent storage.
3. **Frame Synchronization:** Elm architecture requires Arc/Mutex for emulator thread updates.

### Performance Targets

- **16.67ms budget (60 Hz):**
  - Texture upload: 1ms
  - Render pass: 5ms
  - Iced layout: 5ms
  - Reserve: 5.67ms

---

## Success Criteria

- [ ] All tasks complete
- [ ] 60 FPS rendering verified
- [ ] Pixel-perfect scaling tested
- [ ] All scaling modes functional
- [ ] Video settings tested
- [ ] Zero unsafe code confirmed
- [ ] Ready for audio integration (M6-S3)

---

**Sprint Status:** ⏳ PENDING
**Blocked By:** M6-S1 (Iced Application Foundation)
**Next Sprint:** [M6-S3 Input + Library](M6-S3-input-library.md)
