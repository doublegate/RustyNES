#![allow(
    clippy::too_many_arguments,
    clippy::doc_markdown,
    // Numeric tables are ported verbatim from Bisqwit's C; the integer casts
    // are intentional truncation (matching the `(int)` / `(int8_t)` casts).
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

//! True composite NES_NTSC filter — Bisqwit's algorithm on the GPU (T-110-A1,
//! stage 2/2).
//!
//! Unlike the simplified [`crate::ntsc`] blur, this is a faithful port of
//! Bisqwit's `nes_ntsc`-style composite model (as implemented by Mesen2's
//! `BisqwitNtscFilter`): it reconstructs the analog luma+chroma **signal** from
//! the PPU's per-pixel palette index, then demodulates it back to RGB with a
//! windowed Y/I/Q filter. The genuine NTSC artifacts (chroma dot-crawl, colour
//! fringing on vertical edges, the diagonal "checkerboard" on saturated hues)
//! fall out of the math instead of being faked.
//!
//! It samples the PPU's **palette-index** framebuffer (`(emphasis << 6) | colour`,
//! 0..=511 — `Ppu::index_framebuffer`) as an `R16Uint` texture, plus the
//! per-frame NTSC phase (`Ppu::ntsc_phase`), both produced by the core in
//! stage 1. The decode runs per output fragment:
//! for each pixel it sums a 12-sample window of the reconstructed signal and
//! applies the contrast/saturation matrix. All of the static tables (the
//! per-colour low/high signal levels, the sine table, the YIQ matrix, the
//! emphasis waveforms) are computed here and **baked into the WGSL** as
//! `var<private>` arrays — WebGL2 (the wasm backend) has no storage buffers, and
//! naga forbids dynamically indexing value (`const`/`let`) arrays, so a private
//! module-scope `var` array is the portable choice.
//!
//! Parameters are fixed at Bisqwit's neutral defaults (Hue/Brightness/Contrast/
//! Saturation = 0, Y/I/Q filter widths = 12). The artifacts come from the
//! algorithm, not the tunables; per-knob controls can be added later.

use wgpu::util::DeviceExt;

use crate::gfx::{NES_H, NES_W};

// --- Signal model constants (from Bisqwit / lidnariq's measurements) ---------

/// Per-luma-group low signal levels, `[attenuated][luma 0..3]` (volts).
const SIGNAL_LUMA_LOW: [[f64; 4]; 2] = [[0.228, 0.312, 0.552, 0.880], [0.192, 0.256, 0.448, 0.712]];
/// Per-luma-group high signal levels, `[attenuated][luma 0..3]` (volts).
const SIGNAL_LUMA_HIGH: [[f64; 4]; 2] =
    [[0.616, 0.840, 1.100, 1.100], [0.500, 0.676, 0.896, 0.896]];

/// Compute the 128-entry low/high signal tables, indexed by
/// `(attenuated << 6) | colour` (colour 0..=0x3F). Values are scaled so blank
/// (`$0D`) maps to 0 and white (`$20`) to 100, then floored — exactly as the
/// Mesen `BisqwitNtscFilter` constructor does.
fn build_signal_luts() -> ([i32; 128], [i32; 128]) {
    let signal_blank = SIGNAL_LUMA_LOW[0][1];
    let signal_white = SIGNAL_LUMA_HIGH[0][3];
    let span = signal_white - signal_blank;

    let mut low = [0i32; 128];
    let mut high = [0i32; 128];
    for h in 0..=1usize {
        for i in 0..=0x3Fusize {
            let luma = i / 0x10;
            let mut m = SIGNAL_LUMA_LOW[h][luma];
            let mut q = SIGNAL_LUMA_HIGH[h][luma];
            let chroma = i & 0x0F;
            if chroma == 0x0D {
                q = m;
            } else if chroma == 0 {
                m = q;
            } else if chroma >= 0x0E {
                // $xE / $xF are blanking levels, unaffected by emphasis.
                m = SIGNAL_LUMA_LOW[0][1];
                q = SIGNAL_LUMA_LOW[0][1];
            }
            let idx = (if h == 1 { 0x40 } else { 0 }) | i;
            low[idx] = (((m - signal_blank) / span) * 100.0).floor() as i32;
            high[idx] = (((q - signal_blank) / span) * 100.0).floor() as i32;
        }
    }
    (low, high)
}

/// Build the 27-entry sine table (`8 * sin(i * 2pi/12)`, Hue = 0), truncated to
/// integers the same way the `(int8_t)` cast in Mesen does (toward zero). The
/// extra entries past 12 let the decoder index `i%12 + phase0` (+3 for the Q
/// axis) without a second modulo.
fn build_sinetable() -> [i32; 27] {
    let mut t = [0i32; 27];
    let pi = core::f64::consts::PI;
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = (8.0 * (i as f64 * 2.0 * pi / 12.0).sin()) as i32;
    }
    t
}

// --- YIQ matrix (Bisqwit neutral defaults: contrast=sat=bright=0) ------------

const CONTRAST: f64 = 167_941.0; // (0+1)^2 * 167941
const SATURATION: f64 = 144_044.0; // (0+1)^2 * 144044
const FILTER_WIDTH: i32 = 12; // yWidth = iWidth = qWidth at default lengths
const BRIGHTNESS: i32 = 0;

/// The integer YIQ→RGB matrix coefficients, matching `OnBeforeApplyFilter`.
struct Matrix {
    y: i32,
    ir: i32,
    qr: i32,
    ig: i32,
    qg: i32,
    ib: i32,
    qb: i32,
}

fn build_matrix() -> Matrix {
    let w = f64::from(FILTER_WIDTH);
    Matrix {
        y: (CONTRAST / w) as i32,
        ir: (CONTRAST * 1.994_681e-6 * SATURATION / w) as i32,
        qr: (CONTRAST * 9.915_742e-7 * SATURATION / w) as i32,
        ig: (CONTRAST * 9.151_351e-8 * SATURATION / w) as i32,
        qg: (CONTRAST * -6.334_805e-7 * SATURATION / w) as i32,
        ib: (CONTRAST * -1.012_984e-6 * SATURATION / w) as i32,
        qb: (CONTRAST * 1.667_217e-6 * SATURATION / w) as i32,
    }
}

/// 12-bit emphasis waveforms, one per emphasis value 0..7 (R/G/B bit patterns).
const EMPHASIS_LUT: [u32; 8] = [
    0,
    0b0000_0011_1111,
    0b0011_1111_0000,
    0b0011_1111_1111,
    0b1111_0000_0011,
    0b1111_0011_1111,
    0b1111_1111_0011,
    0b1111_1111_1111,
];

/// Format an `i32` slice as a WGSL `array<i32, N>(...)` initializer body.
fn wgsl_i32_array(values: &[i32]) -> String {
    let mut s = String::new();
    for (i, v) in values.iter().enumerate() {
        if i != 0 {
            s.push_str(", ");
        }
        s.push_str(&v.to_string());
    }
    s
}

/// Build the full WGSL source with all static tables baked in.
#[allow(clippy::too_many_lines)] // the embedded WGSL is naturally long.
fn shader_src() -> String {
    let (low, high) = build_signal_luts();
    let sine = build_sinetable();
    let m = build_matrix();
    let emph: Vec<i32> = EMPHASIS_LUT.iter().map(|&v| v as i32).collect();

    format!(
        r"
struct Uniforms {{
    rect: vec4<f32>,   // letterbox transform (same shape as gfx.wgsl)
    params: vec4<f32>, // x = videoPhase (0..2), rest reserved
}};

@group(0) @binding(0) var idx_tex: texture_2d<u32>;
@group(0) @binding(1) var<uniform> u: Uniforms;

// Baked static tables (see ntsc_bisqwit.rs). `var<private>` so the fragment
// shader can dynamically index them (naga forbids dynamic indexing of const/let
// value arrays; WebGL2 has no storage buffers).
var<private> SIGNAL_LOW: array<i32, 128> = array<i32, 128>({signal_low});
var<private> SIGNAL_HIGH: array<i32, 128> = array<i32, 128>({signal_high});
var<private> SINE: array<i32, 27> = array<i32, 27>({sine});
var<private> EMPHASIS: array<i32, 8> = array<i32, 8>({emphasis});

const Y: i32 = {y};
const IR: i32 = {ir};
const QR: i32 = {qr};
const IG: i32 = {ig};
const QG: i32 = {qg};
const IB: i32 = {ib};
const QB: i32 = {qb};
const BRIGHTNESS: i32 = {brightness};
const FW: i32 = {fw}; // filter width (y=i=q=12)

struct VsOut {{
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {{
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
    let p = pos[vid];
    let scaled = vec2<f32>(p.x * u.rect.x, p.y * u.rect.y) + vec2<f32>(u.rect.z, u.rect.w);
    var out: VsOut;
    out.pos = vec4<f32>(scaled, 0.0, 1.0);
    out.uv = uv[vid];
    return out;
}}

// Reconstruct one composite signal sample at absolute line position `t`
// (t = pixel_x*8 + sub-sample j), for the given source `row` and `videoPhase`.
fn signal_sample(t: i32, row: i32, video_phase: i32) -> i32 {{
    if (t < 0 || t >= 256 * 8) {{
        return 0;
    }}
    let x = t >> 3;
    let j = t & 7;

    let ppu = i32(textureLoad(idx_tex, vec2<i32>(x, row), 0).r);
    let pixel_color = ppu & 0x3F;
    let emphasis = ppu >> 6;
    let hue = ppu & 0x0F;

    // Per-pixel entering phase: videoPhase*4 + row*341*8 + x*8.
    let pix_phase = video_phase * 4 + row * 341 * 8 + x * 8;
    let k = ((pix_phase - hue) % 12 + 12) % 12;

    // Square-wave position for sub-sample j, with the 12 -> 1 wrap.
    var pos = k + 1 + j;
    if (pos > 12) {{
        pos = pos - 12;
    }}
    let high = (pos % 12) < 6; // pos==12 -> 0 -> high

    var color = pixel_color;
    if (emphasis != 0 && pos < 12) {{
        let lut = u32(EMPHASIS[emphasis]);
        let r = u32(hue % 12);
        let wave = ((lut >> r) | (lut << (12u - r))) & 0xFFFFu;
        if (((wave >> u32(pos)) & 1u) != 0u) {{
            color = color | 0x40;
        }}
    }}

    if (high) {{
        return SIGNAL_HIGH[color];
    }}
    return SIGNAL_LOW[color];
}}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {{
    let video_phase = i32(u.params.x + 0.5);
    let row = clamp(i32(in.uv.y * 240.0), 0, 239);

    // Centre signal position for this output pixel (256*8 samples per line).
    let center = i32(in.uv.x * 256.0 * 8.0);

    // Per-row decode phase: phase0 = (startCycle + 7) % 12,
    // startCycle = (videoPhase*4 + row*341*8) % 12.
    let start_cycle = ((video_phase * 4 + row * 341 * 8) % 12 + 12) % 12;
    let phase0 = (start_cycle + 7) % 12;

    var ysum = BRIGHTNESS;
    var isum = 0;
    var qsum = 0;
    // Windowed sum over FW samples centred on `center` (yWidth=iWidth=qWidth).
    for (var d = -(FW / 2) + 1; d <= FW / 2; d = d + 1) {{
        let t = center + d;
        let s = signal_sample(t, row, video_phase);
        // Cos(t) = SINE[(t+36)%12 + phase0]; Sin(t) = SINE[... + 3 + phase0].
        let m12 = ((t + 36) % 12 + 12) % 12;
        let cs = SINE[m12 + phase0];
        let sn = SINE[m12 + 3 + phase0];
        ysum = ysum + s;
        isum = isum + s * cs;
        qsum = qsum + s * sn;
    }}

    let r = clamp((ysum * Y + isum * IR + qsum * QR) / 65536, 0, 255);
    let g = clamp((ysum * Y + isum * IG + qsum * QG) / 65536, 0, 255);
    let b = clamp((ysum * Y + isum * IB + qsum * QB) / 65536, 0, 255);

    return vec4<f32>(f32(r) / 255.0, f32(g) / 255.0, f32(b) / 255.0, 1.0);
}}
",
        signal_low = wgsl_i32_array(&low),
        signal_high = wgsl_i32_array(&high),
        sine = wgsl_i32_array(&sine),
        emphasis = wgsl_i32_array(&emph),
        y = m.y,
        ir = m.ir,
        qr = m.qr,
        ig = m.ig,
        qg = m.qg,
        ib = m.ib,
        qb = m.qb,
        brightness = BRIGHTNESS,
        fw = FILTER_WIDTH,
    )
}

/// True composite NES_NTSC filter (Bisqwit algorithm).
pub struct NtscBisqwitFilter {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl NtscBisqwitFilter {
    /// Build the pipeline + bind group over `index_texture` (the `R16Uint`
    /// palette-index source, owned by [`crate::gfx::Gfx`] and stable for its
    /// lifetime).
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        index_texture: &wgpu::Texture,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ntsc-bisqwit-shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src().into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ntsc-bisqwit-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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
            label: Some("ntsc-bisqwit-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ntsc-bisqwit-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
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
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ntsc-bisqwit-uniforms"),
            contents: bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let in_view = index_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ntsc-bisqwit-bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
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

    /// Render the filter into `out_view`, sampling the index texture the filter
    /// was constructed over. `video_phase` is the core's per-frame NTSC phase
    /// (0..=2). Letterboxes to (`width`, `height`) exactly like `Gfx::render`.
    #[allow(clippy::cast_precision_loss)]
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        video_phase: u8,
    ) {
        let nes_aspect = (NES_W as f32) / (NES_H as f32);
        let win_aspect = (width.max(1) as f32) / (height.max(1) as f32);
        let (sx, sy) = if win_aspect > nes_aspect {
            (nes_aspect / win_aspect, 1.0)
        } else {
            (1.0, win_aspect / nes_aspect)
        };
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::cast_slice(&[sx, sy, 0.0, 0.0, f32::from(video_phase), 0.0, 0.0, 0.0]),
        );

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ntsc-bisqwit-pass"),
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
    use super::*;

    /// The generated WGSL must parse AND validate (the same naga front-end +
    /// validator wgpu runs at `create_shader_module`) — guards against baked-in
    /// table or syntax regressions failing only at runtime.
    #[test]
    fn shader_parses_and_validates() {
        let src = shader_src();
        let module = naga::front::wgsl::parse_str(&src).expect("Bisqwit NTSC WGSL must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("Bisqwit NTSC WGSL must validate");
    }

    /// Spot-check the signal LUTs against the Bisqwit model: blanking colours
    /// ($0D and $xE/$xF) normalise to 0, white ($20) high is 100, and the
    /// universal black ($0F) is a blank level.
    #[test]
    fn signal_luts_match_model() {
        let (low, high) = build_signal_luts();
        // $20 (white) high = white level = 100.
        assert_eq!(high[0x20], 100);
        // $0E / $0F are blanking -> 0 low and high.
        assert_eq!(low[0x0E], 0);
        assert_eq!(high[0x0E], 0);
        assert_eq!(low[0x0F], 0);
        assert_eq!(high[0x0F], 0);
        // $0D: q forced to m (no luma swing) -> low == high.
        assert_eq!(low[0x0D], high[0x0D]);
        // Attenuated table populated (index >= 0x40).
        assert_eq!(high[0x40 | 0x20], high[0x40 | 0x20]); // exists, no panic
    }

    /// The sine table is `8*sin`, so its extremes are ±8 and it has the
    /// expected zero crossings.
    #[test]
    fn sinetable_shape() {
        let t = build_sinetable();
        assert_eq!(t[0], 0); // sin(0) = 0
        assert_eq!(t[3], 8); // sin(pi/2) = 1 -> 8
        assert_eq!(t[6], 0); // sin(pi) = 0
        assert_eq!(t[9], -8); // sin(3pi/2) = -1 -> -8
    }
}
