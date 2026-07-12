#![allow(clippy::too_many_arguments, clippy::doc_markdown)]

//! CRT / scanline post-process filter — wgsl post-pass (v1.1.0 beta.1, T-110-A2).
//!
//! A lightweight CRT look applied as a single fullscreen pass (same shape as the
//! [`crate::ntsc`] filter — it letterboxes the 256x240 NES texture into the
//! window and writes the surface directly, mutually exclusive with NTSC for the
//! MVP). The effect has two tunable parts, both driven from a uniform:
//!
//! 1. **Scanlines**: each NES source row (`uv.y * 240`) gets a parabolic
//!    brightness profile — bright at the row centre, dark at the gaps between
//!    rows — scaled by `scanline` intensity (0 = off). Computed in source-row
//!    space so it looks right at any window size.
//! 2. **Aperture mask**: a subtle RGB phosphor-grille pattern keyed off the
//!    output column (`pos.x % 3`), scaled by `mask` intensity, with a small
//!    brightness compensation so the picture does not get too dark.
//!
//! Not a curvature/bloom-heavy shader — a clean, cheap scanline+grille that fits
//! the existing pipeline. Reference: `ref-proj/tetanes` CRT-EasyMode (LibRetro).
//!
//! Performance: 1 texture tap per surface pixel (cheaper than NTSC's 7).

use wgpu::util::DeviceExt;

// v1.8.4: the CRT/scanline WGSL now lives in the shared `rustynes-gfx-shaders`
// crate so the Android wgpu renderer reuses the exact same source (no copy-paste
// drift). The body is byte-identical to the inline version it replaces.
pub(crate) const SHADER_SRC: &str = rustynes_gfx_shaders::CRT_WGSL;

/// v1.2.0 C2 — the CRT knobs exposed for the composable shader stack.
///
/// These RetroArch-style `#pragma parameter` header comments declare the two CRT
/// knobs (`scanline`, `mask`) so [`crate::shader_pass::parse_pragma_parameters`]
/// can drive generic egui sliders. The leading `//` keeps the lines valid WGSL
/// comments. The stack reuses the full `SHADER_SRC` body via its own generic
/// pipeline; the standalone [`CrtFilter`] below is untouched, so the legacy
/// single-select CRT path stays byte-identical.
pub const STACK_SHADER_SRC: &str = concat!(
    "// #pragma parameter scanline \"Scanline intensity\" 0.5 0.0 1.0 0.05\n",
    "// #pragma parameter mask \"Aperture mask\" 0.1 0.0 0.5 0.01\n",
);

/// The `#pragma parameter` header lines the CRT stack pass exposes.
///
/// Kept separate from the body so the parser sees only the declarations.
#[must_use]
pub const fn stack_shader_src() -> &'static str {
    STACK_SHADER_SRC
}

/// CRT / scanline post-process filter.
pub struct CrtFilter {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    /// Bind group over the NES texture, built once in [`Self::new`] (the texture
    /// is created once per `Gfx` and never replaced).
    bind_group: wgpu::BindGroup,
    /// Scanline intensity (0.0 = off .. 1.0 = strong), written into the uniform.
    scanline: f32,
    /// Aperture-mask intensity (fixed-subtle for the MVP).
    mask: f32,
}

impl CrtFilter {
    /// Build the pipeline + the bind group over `nes_texture` (the source the
    /// filter samples; owned by [`crate::gfx::Gfx`] and stable for its lifetime).
    /// `scanline` is the initial scanline intensity (clamped to `0.0..=1.0`).
    #[must_use]
    #[allow(clippy::too_many_lines)] // wgpu pipeline init is naturally verbose.
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        nes_texture: &wgpu::Texture,
        scanline: f32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crt-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crt-bgl"),
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
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
            label: Some("crt-pipeline-layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crt-pipeline"),
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
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("crt-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let scanline = scanline.clamp(0.0, 1.0);
        let mask = 0.10; // subtle fixed grille for the MVP.
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("crt-uniforms"),
            // rect (identity letterbox) + crop (none) + params (scanline, mask).
            contents: bytemuck::cast_slice(&[
                1.0f32, 1.0, 0.0, 0.0, // rect
                1.0, 0.0, 1.0, 0.0, // crop (v-scale, v-off, u-scale, u-off)
                scanline, mask, 0.0, 0.0, // params
            ]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let in_view = nes_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("crt-bg"),
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
            scanline,
            mask,
        }
    }

    /// Update the scanline intensity live (clamped to `0.0..=1.0`). The new value
    /// lands in the uniform on the next [`Self::render`].
    pub const fn set_scanline(&mut self, scanline: f32) {
        self.scanline = scanline.clamp(0.0, 1.0);
    }

    /// Render the CRT filter into `out_view`, sampling the NES texture it was
    /// constructed over. Letterboxes + applies 8:7 pixel-aspect / overscan crop
    /// to (`width`, `height`) exactly like `Gfx::render`'s main blit (shared
    /// `gfx::letterbox_uniform`).
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        par_correction: bool,
        overscan: crate::config::Overscan,
    ) {
        // rect (4) + crop (4) from the shared helper, then the CRT params (4).
        let lb = crate::gfx::letterbox_uniform(width, height, par_correction, overscan);
        let uniform = [
            lb[0],
            lb[1],
            lb[2],
            lb[3],
            lb[4],
            lb[5],
            lb[6],
            lb[7],
            self.scanline,
            self.mask,
            0.0,
            0.0,
        ];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&uniform));

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("crt-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bind_group, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    /// The embedded CRT WGSL must parse AND validate — the same front-end +
    /// validator wgpu runs at `create_shader_module` (guards the dynamic-array
    /// and binding-visibility bug classes a runtime crash would otherwise hide).
    #[test]
    fn shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(super::SHADER_SRC).expect("CRT WGSL must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("CRT WGSL must validate");
    }

    /// The v2.1.9 marquee CRT stack (B6) + the raw NTSC signal-decode pass (P4)
    /// must ALL parse and validate under the exact naga front-end + validator
    /// wgpu runs at `create_shader_module`. This is the gate that proves the new
    /// WGSL files are real, compilable shaders — not just string constants — and
    /// guards the dynamic-array / binding-visibility bug classes a runtime crash
    /// would otherwise hide. Runs the same validation the base CRT test does,
    /// once per shader in the stack registry.
    #[test]
    fn crt_stack_shaders_parse_and_validate() {
        for shader in rustynes_gfx_shaders::CrtStackShader::ALL {
            let src = shader.wgsl();
            let module = naga::front::wgsl::parse_str(src)
                .unwrap_or_else(|e| panic!("{} WGSL must parse: {e:?}", shader.display_name()));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{} WGSL must validate: {e:?}", shader.display_name()));
        }
    }
}
