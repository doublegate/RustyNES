# Video Filters and Shaders Implementation Guide

Complete reference for implementing CRT simulation, scaling algorithms, and video filters in RustyNES.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Scaling Algorithms](#scaling-algorithms)
4. [CRT Simulation](#crt-simulation)
5. [Shader Pipeline](#shader-pipeline)
6. [Color Correction](#color-correction)
7. [wgpu Integration](#wgpu-integration)
8. [Performance Optimization](#performance-optimization)
9. [Configuration](#configuration)
10. [References](#references)

---

## Overview

The video filter system provides various upscaling and post-processing effects to enhance the NES visual output while maintaining authentic aesthetics.

### Key Features

1. **Scaling Algorithms**: Nearest neighbor, bilinear, HQx, xBRZ
2. **CRT Simulation**: Scanlines, curvature, phosphor glow, aperture grille
3. **Color Correction**: NTSC palette emulation, gamma correction
4. **Shader Pipeline**: GPU-accelerated effects via wgpu
5. **Integer Scaling**: Pixel-perfect scaling options

### Design Goals

- Authentic retro visual reproduction
- High performance GPU acceleration
- Configurable effect intensity
- Support for multiple backends

---

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Video Pipeline                            │
│                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   PPU       │    │   Scale     │    │    CRT          │  │
│  │  Output     │───►│   Filter    │───►│   Shader        │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│     256x240              ↓                    ↓              │
│                    ┌─────────────┐    ┌─────────────────┐   │
│                    │   Color     │    │   Display       │   │
│                    │ Correction  │───►│   Output        │   │
│                    └─────────────┘    └─────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### Core Types

```rust
/// Video filter configuration
#[derive(Clone)]
pub struct VideoConfig {
    /// Scaling algorithm
    pub scale_filter: ScaleFilter,

    /// Scale factor (1-8)
    pub scale_factor: u32,

    /// Enable integer scaling
    pub integer_scaling: bool,

    /// Enable CRT simulation
    pub crt_enabled: bool,

    /// CRT shader settings
    pub crt_config: CrtConfig,

    /// Color correction settings
    pub color_config: ColorConfig,

    /// Enable vsync
    pub vsync: bool,

    /// Target framerate (0 = unlimited)
    pub target_fps: u32,
}

/// Available scaling algorithms
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScaleFilter {
    /// Nearest neighbor (pixelated)
    Nearest,

    /// Bilinear interpolation (smooth)
    Bilinear,

    /// HQ2x algorithm
    Hq2x,

    /// HQ3x algorithm
    Hq3x,

    /// HQ4x algorithm
    Hq4x,

    /// xBRZ 2x
    XbrZ2x,

    /// xBRZ 3x
    XbrZ3x,

    /// xBRZ 4x
    XbrZ4x,

    /// Scale2x/AdvMAME
    Scale2x,

    /// Scale3x/AdvMAME
    Scale3x,
}

/// CRT simulation configuration
#[derive(Clone)]
pub struct CrtConfig {
    /// Scanline intensity (0.0 - 1.0)
    pub scanline_intensity: f32,

    /// Scanline thickness (0.5 - 2.0)
    pub scanline_thickness: f32,

    /// Screen curvature (0.0 - 0.3)
    pub curvature: f32,

    /// Corner rounding (0.0 - 0.1)
    pub corner_radius: f32,

    /// Phosphor mask type
    pub mask_type: PhosphorMask,

    /// Mask intensity (0.0 - 1.0)
    pub mask_intensity: f32,

    /// Bloom/glow amount (0.0 - 1.0)
    pub bloom: f32,

    /// Color fringing (0.0 - 1.0)
    pub color_fringing: f32,

    /// Vignette intensity (0.0 - 1.0)
    pub vignette: f32,

    /// Noise/grain (0.0 - 0.1)
    pub noise: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum PhosphorMask {
    /// No mask
    None,

    /// Aperture grille (vertical lines)
    ApertureGrille,

    /// Shadow mask (triangle pattern)
    ShadowMask,

    /// Slot mask (horizontal slots)
    SlotMask,
}

impl Default for CrtConfig {
    fn default() -> Self {
        Self {
            scanline_intensity: 0.3,
            scanline_thickness: 1.0,
            curvature: 0.05,
            corner_radius: 0.03,
            mask_type: PhosphorMask::ApertureGrille,
            mask_intensity: 0.2,
            bloom: 0.15,
            color_fringing: 0.0,
            vignette: 0.1,
            noise: 0.02,
        }
    }
}

/// Color correction configuration
#[derive(Clone)]
pub struct ColorConfig {
    /// Palette type
    pub palette: PaletteType,

    /// Gamma correction (0.5 - 3.0)
    pub gamma: f32,

    /// Brightness (-0.5 - 0.5)
    pub brightness: f32,

    /// Contrast (0.5 - 2.0)
    pub contrast: f32,

    /// Saturation (0.0 - 2.0)
    pub saturation: f32,

    /// NTSC signal artifacts
    pub ntsc_artifacts: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum PaletteType {
    /// Standard NES palette
    Standard,

    /// NTSC color decode
    Ntsc,

    /// Sony CXA2025AS decoder
    SonyCxa,

    /// NES Classic Mini palette
    NesClassic,

    /// Grayscale
    Grayscale,

    /// Custom palette
    Custom,
}
```

---

## Scaling Algorithms

### Nearest Neighbor

```rust
/// Nearest neighbor scaling (CPU fallback)
pub fn scale_nearest(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
) {
    let x_ratio = src_width as f32 / dst_width as f32;
    let y_ratio = src_height as f32 / dst_height as f32;

    for dst_y in 0..dst_height {
        let src_y = (dst_y as f32 * y_ratio) as u32;

        for dst_x in 0..dst_width {
            let src_x = (dst_x as f32 * x_ratio) as u32;

            let src_idx = ((src_y * src_width + src_x) * 4) as usize;
            let dst_idx = ((dst_y * dst_width + dst_x) * 4) as usize;

            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }
}
```

### HQx Algorithm

```rust
/// HQ2x scaling implementation
pub struct Hq2xScaler {
    yuv_table: [u32; 65536],
    lookup: [u8; 256 * 12],
}

impl Hq2xScaler {
    pub fn new() -> Self {
        let mut scaler = Self {
            yuv_table: [0; 65536],
            lookup: [0; 256 * 12],
        };
        scaler.init_tables();
        scaler
    }

    fn init_tables(&mut self) {
        // Initialize YUV conversion table
        for i in 0..65536u32 {
            let r = ((i >> 11) & 0x1F) as f32 * 255.0 / 31.0;
            let g = ((i >> 5) & 0x3F) as f32 * 255.0 / 63.0;
            let b = (i & 0x1F) as f32 * 255.0 / 31.0;

            let y = (0.299 * r + 0.587 * g + 0.114 * b) as u32;
            let u = ((-0.169 * r - 0.331 * g + 0.5 * b) + 128.0) as u32;
            let v = ((0.5 * r - 0.419 * g - 0.081 * b) + 128.0) as u32;

            self.yuv_table[i as usize] = (y << 16) | (u << 8) | v;
        }

        // Initialize pattern lookup table
        // (This would contain the HQ2x interpolation patterns)
    }

    /// Check if two colors are similar
    fn diff(&self, c1: u32, c2: u32) -> bool {
        let yuv1 = self.yuv_table[(c1 & 0xFFFF) as usize];
        let yuv2 = self.yuv_table[(c2 & 0xFFFF) as usize];

        let y_diff = ((yuv1 >> 16) as i32 - (yuv2 >> 16) as i32).abs();
        let u_diff = (((yuv1 >> 8) & 0xFF) as i32 - ((yuv2 >> 8) & 0xFF) as i32).abs();
        let v_diff = ((yuv1 & 0xFF) as i32 - (yuv2 & 0xFF) as i32).abs();

        y_diff > 48 || u_diff > 7 || v_diff > 6
    }

    /// Scale image using HQ2x
    pub fn scale(
        &self,
        src: &[u32],
        src_width: usize,
        src_height: usize,
        dst: &mut [u32],
    ) {
        let dst_width = src_width * 2;

        for y in 0..src_height {
            for x in 0..src_width {
                // Get 3x3 neighborhood
                let mut w = [0u32; 9];
                self.get_neighborhood(src, src_width, src_height, x, y, &mut w);

                // Determine pattern
                let pattern = self.get_pattern(&w);

                // Apply interpolation
                let dst_x = x * 2;
                let dst_y = y * 2;

                let (p0, p1, p2, p3) = self.interpolate(&w, pattern);

                dst[dst_y * dst_width + dst_x] = p0;
                dst[dst_y * dst_width + dst_x + 1] = p1;
                dst[(dst_y + 1) * dst_width + dst_x] = p2;
                dst[(dst_y + 1) * dst_width + dst_x + 1] = p3;
            }
        }
    }

    fn get_neighborhood(
        &self,
        src: &[u32],
        width: usize,
        height: usize,
        x: usize,
        y: usize,
        w: &mut [u32; 9],
    ) {
        let x_m1 = x.saturating_sub(1);
        let x_p1 = (x + 1).min(width - 1);
        let y_m1 = y.saturating_sub(1);
        let y_p1 = (y + 1).min(height - 1);

        w[0] = src[y_m1 * width + x_m1];
        w[1] = src[y_m1 * width + x];
        w[2] = src[y_m1 * width + x_p1];
        w[3] = src[y * width + x_m1];
        w[4] = src[y * width + x];
        w[5] = src[y * width + x_p1];
        w[6] = src[y_p1 * width + x_m1];
        w[7] = src[y_p1 * width + x];
        w[8] = src[y_p1 * width + x_p1];
    }

    fn get_pattern(&self, w: &[u32; 9]) -> u8 {
        let mut pattern = 0u8;
        let center = w[4];

        if self.diff(center, w[0]) { pattern |= 0x01; }
        if self.diff(center, w[1]) { pattern |= 0x02; }
        if self.diff(center, w[2]) { pattern |= 0x04; }
        if self.diff(center, w[3]) { pattern |= 0x08; }
        if self.diff(center, w[5]) { pattern |= 0x10; }
        if self.diff(center, w[6]) { pattern |= 0x20; }
        if self.diff(center, w[7]) { pattern |= 0x40; }
        if self.diff(center, w[8]) { pattern |= 0x80; }

        pattern
    }

    fn interpolate(&self, w: &[u32; 9], pattern: u8) -> (u32, u32, u32, u32) {
        // Simplified - full implementation would use lookup table
        let c = w[4];
        match pattern {
            0 => (c, c, c, c),
            _ => {
                // Blend with neighbors based on pattern
                (c, c, c, c) // Placeholder
            }
        }
    }
}
```

### xBRZ Algorithm

```rust
/// xBRZ scaling implementation
pub struct XbrzScaler {
    scale_factor: u32,
}

impl XbrzScaler {
    pub fn new(scale_factor: u32) -> Self {
        assert!(scale_factor >= 2 && scale_factor <= 6);
        Self { scale_factor }
    }

    /// Scale image using xBRZ
    pub fn scale(
        &self,
        src: &[u32],
        src_width: usize,
        src_height: usize,
        dst: &mut [u32],
    ) {
        let scale = self.scale_factor as usize;
        let dst_width = src_width * scale;

        for y in 0..src_height {
            for x in 0..src_width {
                // Get 5x5 neighborhood for xBRZ
                let neighborhood = self.get_neighborhood_5x5(
                    src, src_width, src_height, x, y
                );

                // Classify edge pattern
                let blend_info = self.classify_edges(&neighborhood);

                // Scale pixel
                self.scale_pixel(
                    &neighborhood,
                    &blend_info,
                    dst,
                    dst_width,
                    x * scale,
                    y * scale,
                );
            }
        }
    }

    fn get_neighborhood_5x5(
        &self,
        src: &[u32],
        width: usize,
        height: usize,
        x: usize,
        y: usize,
    ) -> [u32; 25] {
        let mut n = [0u32; 25];

        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let sx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
                let sy = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
                let idx = ((dy + 2) * 5 + (dx + 2)) as usize;
                n[idx] = src[sy * width + sx];
            }
        }

        n
    }

    fn classify_edges(&self, n: &[u32; 25]) -> BlendInfo {
        // xBRZ edge classification
        // Returns blend weights for each corner
        BlendInfo::default()
    }

    fn scale_pixel(
        &self,
        n: &[u32; 25],
        blend: &BlendInfo,
        dst: &mut [u32],
        dst_width: usize,
        dst_x: usize,
        dst_y: usize,
    ) {
        let scale = self.scale_factor as usize;
        let center = n[12]; // Center pixel

        for sy in 0..scale {
            for sx in 0..scale {
                // Apply blending based on position and blend_info
                dst[(dst_y + sy) * dst_width + dst_x + sx] = center;
            }
        }
    }
}

#[derive(Default)]
struct BlendInfo {
    top_left: f32,
    top_right: f32,
    bottom_left: f32,
    bottom_right: f32,
}
```

---

## CRT Simulation

### CRT Shader (WGSL)

```wgsl
// CRT simulation shader

struct CrtParams {
    scanline_intensity: f32,
    scanline_thickness: f32,
    curvature: f32,
    corner_radius: f32,
    mask_intensity: f32,
    mask_type: u32,
    bloom: f32,
    vignette: f32,
    screen_size: vec2<f32>,
    texture_size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> params: CrtParams;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

// Apply barrel distortion for screen curvature
fn barrel_distort(coord: vec2<f32>) -> vec2<f32> {
    let center = coord - 0.5;
    let r2 = dot(center, center);
    let distortion = 1.0 + r2 * params.curvature;
    return center * distortion + 0.5;
}

// Calculate scanline intensity
fn scanline(y: f32, intensity: f32) -> f32 {
    let scan_y = y * params.texture_size.y;
    let scanline_pos = fract(scan_y);
    let thickness = params.scanline_thickness * 0.5;

    // Smooth scanline falloff
    let scan = smoothstep(0.0, thickness, scanline_pos) *
               smoothstep(1.0, 1.0 - thickness, scanline_pos);

    return mix(1.0, scan, intensity);
}

// Aperture grille mask (RGB vertical stripes)
fn aperture_grille(x: f32) -> vec3<f32> {
    let pixel = floor(x * params.screen_size.x);
    let subpixel = u32(pixel) % 3u;

    var mask = vec3<f32>(params.mask_intensity);
    mask[subpixel] = 1.0;

    return mask;
}

// Shadow mask (triangular pattern)
fn shadow_mask(coord: vec2<f32>) -> vec3<f32> {
    let pixel = floor(coord * params.screen_size);
    let pattern_x = u32(pixel.x) % 3u;
    let pattern_y = u32(pixel.y) % 2u;
    let offset = pattern_y * 1u;

    var mask = vec3<f32>(params.mask_intensity);
    mask[(pattern_x + offset) % 3u] = 1.0;

    return mask;
}

// Corner darkening
fn corner_mask(coord: vec2<f32>) -> f32 {
    let center = coord - 0.5;
    let corner_dist = length(max(abs(center) - 0.5 + params.corner_radius, vec2<f32>(0.0)));
    return smoothstep(params.corner_radius, 0.0, corner_dist);
}

// Vignette effect
fn vignette(coord: vec2<f32>) -> f32 {
    let center = coord - 0.5;
    let dist = length(center) * 1.414;
    return 1.0 - smoothstep(0.5, 1.0, dist) * params.vignette;
}

// Bloom/glow effect (simplified)
fn bloom(coord: vec2<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let blur_size = 2.0 / params.texture_size;

    // Sample neighboring pixels
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let offset = vec2<f32>(f32(x), f32(y)) * blur_size;
            sum += textureSample(source_texture, source_sampler, coord + offset).rgb;
        }
    }

    return sum / 25.0 * params.bloom;
}

@fragment
fn fs_main(@location(0) tex_coord: vec2<f32>) -> @location(0) vec4<f32> {
    // Apply curvature
    let curved_coord = barrel_distort(tex_coord);

    // Check if outside screen
    if (curved_coord.x < 0.0 || curved_coord.x > 1.0 ||
        curved_coord.y < 0.0 || curved_coord.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // Sample base color
    var color = textureSample(source_texture, source_sampler, curved_coord).rgb;

    // Apply scanlines
    color *= scanline(curved_coord.y, params.scanline_intensity);

    // Apply phosphor mask
    if (params.mask_type == 1u) {
        color *= aperture_grille(curved_coord.x);
    } else if (params.mask_type == 2u) {
        color *= shadow_mask(curved_coord);
    }

    // Add bloom
    color += bloom(curved_coord);

    // Apply vignette
    color *= vignette(curved_coord);

    // Apply corner mask
    color *= corner_mask(curved_coord);

    return vec4<f32>(color, 1.0);
}
```

### CRT Preset Configurations

```rust
/// Preset CRT configurations
pub enum CrtPreset {
    /// Minimal CRT effect
    Subtle,

    /// Balanced CRT look
    Standard,

    /// Strong retro feel
    Authentic,

    /// Over-the-top CRT
    Extreme,

    /// Custom settings
    Custom(CrtConfig),
}

impl CrtPreset {
    pub fn to_config(&self) -> CrtConfig {
        match self {
            CrtPreset::Subtle => CrtConfig {
                scanline_intensity: 0.15,
                scanline_thickness: 1.0,
                curvature: 0.0,
                corner_radius: 0.0,
                mask_type: PhosphorMask::None,
                mask_intensity: 0.0,
                bloom: 0.05,
                color_fringing: 0.0,
                vignette: 0.0,
                noise: 0.0,
            },

            CrtPreset::Standard => CrtConfig {
                scanline_intensity: 0.3,
                scanline_thickness: 1.0,
                curvature: 0.03,
                corner_radius: 0.02,
                mask_type: PhosphorMask::ApertureGrille,
                mask_intensity: 0.15,
                bloom: 0.1,
                color_fringing: 0.0,
                vignette: 0.1,
                noise: 0.01,
            },

            CrtPreset::Authentic => CrtConfig {
                scanline_intensity: 0.5,
                scanline_thickness: 1.2,
                curvature: 0.08,
                corner_radius: 0.05,
                mask_type: PhosphorMask::ShadowMask,
                mask_intensity: 0.25,
                bloom: 0.2,
                color_fringing: 0.1,
                vignette: 0.2,
                noise: 0.03,
            },

            CrtPreset::Extreme => CrtConfig {
                scanline_intensity: 0.7,
                scanline_thickness: 1.5,
                curvature: 0.15,
                corner_radius: 0.08,
                mask_type: PhosphorMask::ShadowMask,
                mask_intensity: 0.4,
                bloom: 0.3,
                color_fringing: 0.2,
                vignette: 0.3,
                noise: 0.05,
            },

            CrtPreset::Custom(config) => config.clone(),
        }
    }
}
```

---

## Shader Pipeline

### Pipeline Setup

```rust
use wgpu::*;

/// Video filter shader pipeline
pub struct FilterPipeline {
    device: Device,
    queue: Queue,
    scale_pipeline: RenderPipeline,
    crt_pipeline: RenderPipeline,
    scale_bind_group: BindGroup,
    crt_bind_group: BindGroup,
    crt_params_buffer: Buffer,
    intermediate_texture: Texture,
}

impl FilterPipeline {
    pub fn new(device: &Device, queue: &Queue, config: &VideoConfig) -> Self {
        // Create shader modules
        let scale_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Scale Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/scale.wgsl").into()),
        });

        let crt_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("CRT Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/crt.wgsl").into()),
        });

        // Create pipelines
        let scale_pipeline = Self::create_scale_pipeline(&device, &scale_shader);
        let crt_pipeline = Self::create_crt_pipeline(&device, &crt_shader);

        // Create intermediate texture for multi-pass rendering
        let intermediate_size = Extent3d {
            width: 256 * config.scale_factor,
            height: 240 * config.scale_factor,
            depth_or_array_layers: 1,
        };

        let intermediate_texture = device.create_texture(&TextureDescriptor {
            label: Some("Intermediate Texture"),
            size: intermediate_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        // Create CRT params buffer
        let crt_params_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("CRT Params"),
            size: std::mem::size_of::<CrtParamsGpu>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ... create bind groups

        Self {
            device: device.clone(),
            queue: queue.clone(),
            scale_pipeline,
            crt_pipeline,
            scale_bind_group: todo!(),
            crt_bind_group: todo!(),
            crt_params_buffer,
            intermediate_texture,
        }
    }

    fn create_scale_pipeline(device: &Device, shader: &ShaderModule) -> RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Scale Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Scale Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    }

    fn create_crt_pipeline(device: &Device, shader: &ShaderModule) -> RenderPipeline {
        // Similar to scale pipeline with CRT-specific settings
        todo!()
    }

    /// Render frame through filter pipeline
    pub fn render(
        &self,
        source: &TextureView,
        target: &TextureView,
        encoder: &mut CommandEncoder,
    ) {
        // Pass 1: Scale
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Scale Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &self.intermediate_texture.create_view(&TextureViewDescriptor::default()),
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.scale_pipeline);
            pass.set_bind_group(0, &self.scale_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 2: CRT
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("CRT Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.crt_pipeline);
            pass.set_bind_group(0, &self.crt_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Update CRT parameters
    pub fn update_crt_params(&self, config: &CrtConfig) {
        let params = CrtParamsGpu {
            scanline_intensity: config.scanline_intensity,
            scanline_thickness: config.scanline_thickness,
            curvature: config.curvature,
            corner_radius: config.corner_radius,
            mask_intensity: config.mask_intensity,
            mask_type: match config.mask_type {
                PhosphorMask::None => 0,
                PhosphorMask::ApertureGrille => 1,
                PhosphorMask::ShadowMask => 2,
                PhosphorMask::SlotMask => 3,
            },
            bloom: config.bloom,
            vignette: config.vignette,
            ..Default::default()
        };

        self.queue.write_buffer(
            &self.crt_params_buffer,
            0,
            bytemuck::cast_slice(&[params]),
        );
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct CrtParamsGpu {
    scanline_intensity: f32,
    scanline_thickness: f32,
    curvature: f32,
    corner_radius: f32,
    mask_intensity: f32,
    mask_type: u32,
    bloom: f32,
    vignette: f32,
    screen_size: [f32; 2],
    texture_size: [f32; 2],
}
```

---

## Color Correction

### NTSC Palette Generation

```rust
/// NTSC palette generator
pub struct NtscPalette {
    colors: [[u8; 3]; 64],
}

impl NtscPalette {
    pub fn generate(
        hue: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        gamma: f32,
    ) -> Self {
        let mut colors = [[0u8; 3]; 64];

        for i in 0..64 {
            let (r, g, b) = Self::decode_ntsc_color(
                i as u8,
                hue,
                saturation,
                brightness,
                contrast,
                gamma,
            );

            colors[i] = [r, g, b];
        }

        Self { colors }
    }

    fn decode_ntsc_color(
        color_index: u8,
        hue_offset: f32,
        saturation: f32,
        brightness: f32,
        contrast: f32,
        gamma: f32,
    ) -> (u8, u8, u8) {
        let luma_index = (color_index & 0x30) >> 4;
        let chroma_index = color_index & 0x0F;

        // Base luma levels
        let luma_base = match luma_index {
            0 => [0.350, 0.518, 0.962, 1.0],
            1 => [0.350, 0.518, 0.962, 1.0],
            2 => [0.350, 0.518, 0.962, 1.0],
            3 => [0.350, 0.518, 0.962, 1.0],
            _ => unreachable!(),
        };

        let luma = luma_base[luma_index as usize];

        // Chroma phase (hue)
        let phase = if chroma_index == 0 || chroma_index == 0x0D {
            0.0
        } else {
            ((chroma_index as f32 - 1.0) * 30.0 + hue_offset).to_radians()
        };

        // Chroma amplitude
        let chroma_amp = if chroma_index == 0 || chroma_index >= 0x0D {
            0.0
        } else {
            saturation * 0.5
        };

        // Decode YIQ to RGB
        let y = luma * contrast + brightness;
        let i = chroma_amp * phase.cos();
        let q = chroma_amp * phase.sin();

        let r = y + 0.956 * i + 0.621 * q;
        let g = y - 0.272 * i - 0.647 * q;
        let b = y - 1.106 * i + 1.703 * q;

        // Apply gamma and clamp
        let r = (r.powf(gamma) * 255.0).clamp(0.0, 255.0) as u8;
        let g = (g.powf(gamma) * 255.0).clamp(0.0, 255.0) as u8;
        let b = (b.powf(gamma) * 255.0).clamp(0.0, 255.0) as u8;

        (r, g, b)
    }

    pub fn lookup(&self, index: u8) -> [u8; 3] {
        self.colors[(index & 0x3F) as usize]
    }
}
```

### Gamma Correction Shader

```wgsl
// Gamma correction and color adjustment shader

struct ColorParams {
    gamma: f32,
    brightness: f32,
    contrast: f32,
    saturation: f32,
};

@group(0) @binding(0) var<uniform> params: ColorParams;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
    let max_c = max(max(rgb.r, rgb.g), rgb.b);
    let min_c = min(min(rgb.r, rgb.g), rgb.b);
    let delta = max_c - min_c;

    var h = 0.0;
    var s = 0.0;
    let l = (max_c + min_c) * 0.5;

    if (delta > 0.0) {
        s = delta / (1.0 - abs(2.0 * l - 1.0));

        if (max_c == rgb.r) {
            h = ((rgb.g - rgb.b) / delta) % 6.0;
        } else if (max_c == rgb.g) {
            h = (rgb.b - rgb.r) / delta + 2.0;
        } else {
            h = (rgb.r - rgb.g) / delta + 4.0;
        }
        h /= 6.0;
    }

    return vec3<f32>(h, s, l);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let c = (1.0 - abs(2.0 * hsl.z - 1.0)) * hsl.y;
    let x = c * (1.0 - abs((hsl.x * 6.0) % 2.0 - 1.0));
    let m = hsl.z - c * 0.5;

    var rgb: vec3<f32>;
    let h_segment = u32(hsl.x * 6.0);

    switch (h_segment) {
        case 0u: { rgb = vec3<f32>(c, x, 0.0); }
        case 1u: { rgb = vec3<f32>(x, c, 0.0); }
        case 2u: { rgb = vec3<f32>(0.0, c, x); }
        case 3u: { rgb = vec3<f32>(0.0, x, c); }
        case 4u: { rgb = vec3<f32>(x, 0.0, c); }
        default: { rgb = vec3<f32>(c, 0.0, x); }
    }

    return rgb + m;
}

@fragment
fn fs_main(@location(0) tex_coord: vec2<f32>) -> @location(0) vec4<f32> {
    var color = textureSample(source_texture, source_sampler, tex_coord).rgb;

    // Apply contrast and brightness
    color = (color - 0.5) * params.contrast + 0.5 + params.brightness;

    // Apply saturation
    let hsl = rgb_to_hsl(color);
    let adjusted_hsl = vec3<f32>(hsl.x, hsl.y * params.saturation, hsl.z);
    color = hsl_to_rgb(adjusted_hsl);

    // Apply gamma
    color = pow(color, vec3<f32>(1.0 / params.gamma));

    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
```

---

## wgpu Integration

### Renderer Implementation

```rust
/// Video renderer with filter support
pub struct VideoRenderer {
    device: Device,
    queue: Queue,
    surface: Surface,
    config: SurfaceConfiguration,

    // NES framebuffer texture
    nes_texture: Texture,
    nes_texture_view: TextureView,

    // Filter pipeline
    filter_pipeline: FilterPipeline,

    // Current configuration
    video_config: VideoConfig,
}

impl VideoRenderer {
    pub async fn new(window: &Window, config: VideoConfig) -> Self {
        let instance = Instance::new(InstanceDescriptor::default());
        let surface = unsafe { instance.create_surface(window) }.unwrap();

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("RustyNES Device"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    memory_hints: MemoryHints::default(),
                },
                None,
            )
            .await
            .unwrap();

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: if config.vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::AutoNoVsync
            },
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        // Create NES framebuffer texture (256x240)
        let nes_texture = device.create_texture(&TextureDescriptor {
            label: Some("NES Framebuffer"),
            size: Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let nes_texture_view = nes_texture.create_view(&TextureViewDescriptor::default());

        let filter_pipeline = FilterPipeline::new(&device, &queue, &config);

        Self {
            device,
            queue,
            surface,
            config: surface_config,
            nes_texture,
            nes_texture_view,
            filter_pipeline,
            video_config: config,
        }
    }

    /// Upload NES framebuffer
    pub fn upload_framebuffer(&self, pixels: &[u8]) {
        self.queue.write_texture(
            ImageCopyTexture {
                texture: &self.nes_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            pixels,
            ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(240),
            },
            Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Render frame with filters
    pub fn render(&self) -> Result<(), SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        self.filter_pipeline.render(&self.nes_texture_view, &view, &mut encoder);

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Update video configuration
    pub fn update_config(&mut self, config: VideoConfig) {
        if config.vsync != self.video_config.vsync {
            self.config.present_mode = if config.vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::AutoNoVsync
            };
            self.surface.configure(&self.device, &self.config);
        }

        self.filter_pipeline.update_crt_params(&config.crt_config);
        self.video_config = config;
    }
}
```

---

## Performance Optimization

### Shader Compilation Caching

```rust
/// Cache compiled shaders
pub struct ShaderCache {
    cache_dir: PathBuf,
}

impl ShaderCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&cache_dir).ok();
        Self { cache_dir }
    }

    pub fn get_or_compile(
        &self,
        device: &Device,
        source: &str,
        label: &str,
    ) -> ShaderModule {
        let hash = Self::hash_shader(source);
        let cache_path = self.cache_dir.join(format!("{}.spirv", hash));

        // Try to load cached SPIR-V
        if let Ok(spirv) = std::fs::read(&cache_path) {
            if let Ok(module) = device.create_shader_module(ShaderModuleDescriptor {
                label: Some(label),
                source: ShaderSource::SpirV(bytemuck::cast_slice(&spirv).into()),
            }) {
                return module;
            }
        }

        // Compile from source
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some(label),
            source: ShaderSource::Wgsl(source.into()),
        });

        // Cache is handled by wgpu/driver

        module
    }

    fn hash_shader(source: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}
```

---

## Configuration

### Settings UI

```rust
/// Video settings UI
pub fn render_video_settings(ui: &mut egui::Ui, config: &mut VideoConfig) {
    ui.heading("Video Settings");

    // Scaling
    ui.separator();
    ui.label("Scaling");

    egui::ComboBox::from_label("Filter")
        .selected_text(format!("{:?}", config.scale_filter))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut config.scale_filter, ScaleFilter::Nearest, "Nearest");
            ui.selectable_value(&mut config.scale_filter, ScaleFilter::Bilinear, "Bilinear");
            ui.selectable_value(&mut config.scale_filter, ScaleFilter::Hq2x, "HQ2x");
            ui.selectable_value(&mut config.scale_filter, ScaleFilter::Hq3x, "HQ3x");
            ui.selectable_value(&mut config.scale_filter, ScaleFilter::XbrZ2x, "xBRZ 2x");
            ui.selectable_value(&mut config.scale_filter, ScaleFilter::XbrZ4x, "xBRZ 4x");
        });

    ui.horizontal(|ui| {
        ui.label("Scale:");
        ui.add(egui::Slider::new(&mut config.scale_factor, 1..=8));
    });

    ui.checkbox(&mut config.integer_scaling, "Integer Scaling");

    // CRT
    ui.separator();
    ui.checkbox(&mut config.crt_enabled, "CRT Simulation");

    if config.crt_enabled {
        ui.indent("crt", |ui| {
            ui.horizontal(|ui| {
                ui.label("Scanlines:");
                ui.add(egui::Slider::new(&mut config.crt_config.scanline_intensity, 0.0..=1.0));
            });

            ui.horizontal(|ui| {
                ui.label("Curvature:");
                ui.add(egui::Slider::new(&mut config.crt_config.curvature, 0.0..=0.2));
            });

            ui.horizontal(|ui| {
                ui.label("Bloom:");
                ui.add(egui::Slider::new(&mut config.crt_config.bloom, 0.0..=0.5));
            });

            egui::ComboBox::from_label("Phosphor Mask")
                .selected_text(format!("{:?}", config.crt_config.mask_type))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut config.crt_config.mask_type, PhosphorMask::None, "None");
                    ui.selectable_value(&mut config.crt_config.mask_type, PhosphorMask::ApertureGrille, "Aperture Grille");
                    ui.selectable_value(&mut config.crt_config.mask_type, PhosphorMask::ShadowMask, "Shadow Mask");
                });
        });
    }

    // Color
    ui.separator();
    ui.label("Color");

    ui.horizontal(|ui| {
        ui.label("Gamma:");
        ui.add(egui::Slider::new(&mut config.color_config.gamma, 0.5..=3.0));
    });

    ui.horizontal(|ui| {
        ui.label("Saturation:");
        ui.add(egui::Slider::new(&mut config.color_config.saturation, 0.0..=2.0));
    });

    // VSync
    ui.separator();
    ui.checkbox(&mut config.vsync, "VSync");
}
```

---

## References

### Related Documentation

- [PPU Specification](../ppu/PPU_2C02_SPECIFICATION.md)
- [Configuration](../api/CONFIGURATION.md)
- [Build Guide](../dev/BUILD.md)

### External Resources

- [CRT-Royale Shader](https://github.com/libretro/slang-shaders/tree/master/crt/shaders/crt-royale)
- [xBRZ Algorithm](https://github.com/Treeki/libxbrz)
- [HQx Documentation](https://en.wikipedia.org/wiki/Hqx)
- [wgpu Documentation](https://wgpu.rs/)

### Source Files

```
crates/rustynes-desktop/src/
├── video/
│   ├── mod.rs           # Module exports
│   ├── renderer.rs      # VideoRenderer implementation
│   ├── pipeline.rs      # FilterPipeline
│   ├── scale.rs         # Scaling algorithms
│   ├── crt.rs           # CRT configuration
│   └── palette.rs       # NTSC palette
├── shaders/
│   ├── scale.wgsl       # Scaling shader
│   ├── crt.wgsl         # CRT simulation shader
│   └── color.wgsl       # Color correction shader
└── ui/
    └── video_settings.rs
```
