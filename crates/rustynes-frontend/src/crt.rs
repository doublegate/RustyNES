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

pub(crate) const SHADER_SRC: &str = r"
struct Uniforms {
    rect: vec4<f32>,   // letterbox transform (same shape + math as gfx.wgsl)
    crop: vec4<f32>,   // overscan crop: x = vertical scale, y = vertical offset
    params: vec4<f32>, // x = scanline intensity, y = mask intensity, z,w unused
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
    // Fullscreen triangle; letterbox in UV space (not by scaling position), so
    // the bars clip to black in fs (no ClampToEdge edge-smear).
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
    var rgb = textureSample(nes_tex, nes_smp, suv).rgb;

    let scan_amt = u.params.x;
    let mask_amt = u.params.y;

    // Scanlines in NES source-row space (240 rows). Parabolic profile: 1.0 at the
    // row centre, (1 - scan_amt) at the row boundary.
    let src_y = suv.y * 240.0;
    let d = fract(src_y) - 0.5;
    let scan = (1.0 - scan_amt) + scan_amt * (1.0 - 4.0 * d * d);
    rgb = rgb * scan;

    // Aperture grille: tint output columns in an R/G/B triad. Each channel is
    // attenuated on the two columns where it is not the dominant phosphor.
    let col = i32(floor(in.pos.x)) % 3;
    var mask = vec3<f32>(1.0 - mask_amt, 1.0 - mask_amt, 1.0 - mask_amt);
    if (col == 0) {
        mask.r = 1.0;
    } else if (col == 1) {
        mask.g = 1.0;
    } else {
        mask.b = 1.0;
    }
    rgb = rgb * mask;

    // Brightness compensation: scanlines + mask remove energy; add a little back
    // so a mid-strength CRT does not look washed-out dark.
    let comp = 1.0 + 0.5 * (scan_amt + mask_amt);
    rgb = rgb * comp;

    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
";

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
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
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
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("crt-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let scanline = scanline.clamp(0.0, 1.0);
        let mask = 0.10; // subtle fixed grille for the MVP.
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("crt-uniforms"),
            // rect (identity letterbox) + crop (none) + params (scanline, mask).
            contents: bytemuck::cast_slice(&[
                1.0f32, 1.0, 0.0, 0.0, // rect
                1.0, 0.0, 0.0, 0.0, // crop
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
        hide_overscan: bool,
    ) {
        // rect (4) + crop (4) from the shared helper, then the CRT params (4).
        let lb = crate::gfx::letterbox_uniform(width, height, par_correction, hide_overscan);
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
}
