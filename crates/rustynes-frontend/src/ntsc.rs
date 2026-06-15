#![allow(clippy::too_many_arguments, clippy::doc_markdown)]

//! Simplified Blargg-style NTSC filter — wgsl post-pass (T-53-008).
//!
//! This is a *simplified* composite NTSC look:
//!
//! 1. For each output pixel, sample 5 input texels in a horizontal window
//!    and apply weights `[0.10, 0.20, 0.40, 0.20, 0.10]` (low-pass blur,
//!    emulating chroma bleed).
//! 2. Multiply by a scanline mask: every other vertical pixel pair is
//!    dimmed by 15%.
//! 3. Pixel-edge chroma "fringing": if neighboring texels have a large
//!    luma differential, tint the boundary toward orange/teal (very
//!    coarse Blargg trick).
//!
//! Not a bit-exact port of `nes_ntsc`. Marked `ntsc-simple` in the config
//! to set expectations. A full NES_NTSC port is a v1.1 follow-up.
//!
//! Performance: 5 texture taps per surface pixel; on a 768x720 window
//! that's ~2.8M taps/frame, well below GPU memory-bandwidth limits.

use wgpu::util::DeviceExt;

const SHADER_SRC: &str = r"
struct Uniforms {
    // Letterbox transform (same shape + math as gfx.wgsl's blit): rect.xy =
    // the image's fraction of the surface, rect.zw = offset.
    rect: vec4<f32>,
    // Overscan crop: x = vertical scale, y = vertical offset (texture-V space);
    // (1.0, 0.0) = full frame.
    crop: vec4<f32>,
};

@group(0) @binding(0) var nes_tex: texture_2d<f32>;
@group(0) @binding(1) var nes_smp: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Fullscreen triangle; the letterbox is applied in UV space (NOT by scaling
    // the position), so the out-of-image bars clip to black in fs and a
    // ClampToEdge sampler can't smear edge texels across them.
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vid], 0.0, 1.0);
    out.uv = (uv[vid] - vec2<f32>(0.5, 0.5) - vec2<f32>(u.rect.z, u.rect.w))
        / vec2<f32>(u.rect.x, u.rect.y) + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Letterbox bars -> black.
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // Overscan crop: remap the visible V range onto the inner rows.
    let suv = vec2<f32>(in.uv.x, in.uv.y * u.crop.x + u.crop.y);
    let tx_size = vec2<f32>(256.0, 240.0);
    let texel = 1.0 / tx_size.x;
    // 5-tap horizontal box blur in luma; same blur applied per channel.
    // NOTE: these are var (not let) so the loop can index them with the dynamic
    // counter i. WGSL/naga only permits dynamic indexing of arrays in
    // addressable space (a var), not value arrays (a let) -- a let here is a
    // shader-validation error (may only be indexed by a constant).
    var weights = array<f32, 5>(0.10, 0.20, 0.40, 0.20, 0.10);
    var offsets = array<f32, 5>(-2.0, -1.0, 0.0, 1.0, 2.0);
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < 5; i = i + 1) {
        let uv = vec2<f32>(suv.x + offsets[i] * texel, suv.y);
        let s = textureSample(nes_tex, nes_smp, uv).rgb;
        acc = acc + s * weights[i];
    }
    // Scanline: dim every other line by 15% (in source-row space).
    let scanline_y = floor(suv.y * tx_size.y);
    let dim = select(1.0, 0.85, fract(scanline_y * 0.5) > 0.49);
    acc = acc * dim;
    // Subtle chroma fringe: nudge red high / blue low along strong
    // horizontal gradients (cheap proxy for composite artifacting).
    let left = textureSample(nes_tex, nes_smp, vec2<f32>(suv.x - texel, suv.y)).rgb;
    let right = textureSample(nes_tex, nes_smp, vec2<f32>(suv.x + texel, suv.y)).rgb;
    let dluma = dot(right - left, vec3<f32>(0.299, 0.587, 0.114));
    let fringe = clamp(dluma * 0.20, -0.10, 0.10);
    acc.r = acc.r + fringe;
    acc.b = acc.b - fringe;
    return vec4<f32>(acc, 1.0);
}
";

/// Simplified Blargg-style NTSC filter.
pub struct NtscFilter {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    /// v2.8.0 Phase 4 — the bind group over the NES texture, built once in
    /// [`Self::new`] (the texture is created once per `Gfx` and never
    /// replaced) instead of re-created every frame. The bind-group layout +
    /// sampler are construction-time temporaries now (the bind group keeps
    /// them alive internally).
    bind_group: wgpu::BindGroup,
}

impl NtscFilter {
    /// Build the pipeline + the bind group over `nes_texture` (the source
    /// the filter samples; owned by [`crate::gfx::Gfx`] and stable for the
    /// `Gfx`'s lifetime).
    #[must_use]
    #[allow(clippy::too_many_lines)] // wgpu pipeline init is naturally verbose.
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        nes_texture: &wgpu::Texture,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ntsc-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ntsc-bgl"),
            entries: &[
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
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ntsc-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ntsc-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ntsc-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ntsc-uniforms"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        // v2.8.0 Phase 4 — build the bind group once (the NES texture is
        // stable for the Gfx lifetime); the old per-frame
        // view-and-bind-group churn is gone.
        let in_view = nes_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ntsc-bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        });
        Self {
            pipeline,
            uniforms,
            bind_group,
        }
    }

    /// Render the filter into `out_view`, sampling from the NES texture the
    /// filter was constructed over.
    ///
    /// Letterboxes + applies 8:7 pixel-aspect / overscan crop to (`width`,
    /// `height`) exactly like `Gfx::render`'s main blit (shared
    /// `gfx::letterbox_uniform`), so the filtered output keeps the same
    /// aspect + black bars instead of the old position-scaled edge-smear.
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        par_correction: bool,
        hide_overscan: bool,
    ) {
        let uniform = crate::gfx::letterbox_uniform(width, height, par_correction, hide_overscan);
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&uniform));

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ntsc-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out_view,
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
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind_group, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    /// The embedded NTSC WGSL must parse AND validate — the same front-end +
    /// validator wgpu runs at `create_shader_module`. This guards the exact bug
    /// class that crashed the app on enabling the filter: dynamically indexing a
    /// `let` (value) array, which naga rejects ("may only be indexed by a
    /// constant"). A shader regression now fails CI instead of aborting at
    /// runtime.
    #[test]
    fn shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(super::SHADER_SRC).expect("NTSC WGSL must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("NTSC WGSL must validate");
    }
}
