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
//! The four picture knobs (Contrast / Saturation / Brightness / Hue) are live
//! per-frame uniforms (see [`NtscKnobs`]); the Y/I/Q filter widths stay fixed at
//! 12. They all default to Bisqwit's neutral values ([`NtscKnobs::DEFAULT`] —
//! contrast / saturation / brightness / hue = 0), at which the in-shader matrix
//! evaluates bit-identically to the old baked constants, so the default build is
//! byte-for-byte unchanged. The artifacts come from the algorithm, not the
//! tunables.

use wgpu::util::DeviceExt;

/// The four live picture knobs for the Bisqwit NTSC decode.
///
/// Promoted from baked WGSL constants to a per-frame uniform (v1.2.0 C1). The
/// YIQ→RGB matrix is linear in `contrast` and in `contrast * saturation`,
/// `brightness` is an additive luma term, and `hue` rotates the demodulated
/// (I, Q) vector — so they decode losslessly in the shader each frame. At
/// [`Self::DEFAULT`] (all four 0) the matrix matches the previous hardcoded
/// coefficients exactly and `brightness`/`hue` are no-ops, so the output is
/// byte-identical to before C1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NtscKnobs {
    /// Contrast offset. Picture contrast factor is `(contrast + 1)^2`.
    pub contrast: f32,
    /// Saturation offset. Chroma gain factor is `(saturation + 1)^2`.
    pub saturation: f32,
    /// Additive luma (brightness) offset, in the same integer signal-sum units
    /// the decoder accumulates (0 = no change).
    pub brightness: f32,
    /// Hue rotation in degrees, applied as a rotation of the demodulated
    /// (I, Q) vector (0 = no change).
    pub hue: f32,
}

impl NtscKnobs {
    /// Bisqwit's neutral defaults (all 0). The decode at these values is
    /// byte-identical to the pre-C1 baked constants.
    pub const DEFAULT: Self = Self {
        contrast: 0.0,
        saturation: 0.0,
        brightness: 0.0,
        hue: 0.0,
    };
}

impl Default for NtscKnobs {
    fn default() -> Self {
        Self::DEFAULT
    }
}

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

// --- YIQ matrix coefficients (Bisqwit) ---------------------------------------
//
// The integer YIQ→RGB matrix is computed in the shader each frame from the live
// [`NtscKnobs`]: the luma gain is `contrast_factor / FW`, and each chroma term
// is `contrast_factor * <tiny coeff> * saturation_factor / FW`, where
// `contrast_factor = (contrast + 1)^2 * 167941` and
// `saturation_factor = (saturation + 1)^2 * 144044` (matching Mesen's
// `OnBeforeApplyFilter`). The base scalars 167941 / 144044 and the seven tiny
// chroma coefficients are baked into the WGSL below; promoting only the knobs to
// the uniform keeps the default decode (contrast = saturation = 0) bit-identical
// to the previous hardcoded `Y/IR/QR/...` constants. `FILTER_WIDTH` is the fixed
// y/i/q window (yWidth = iWidth = qWidth = 12).

const FILTER_WIDTH: i32 = 12;

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
pub(crate) fn shader_src() -> String {
    let (low, high) = build_signal_luts();
    let sine = build_sinetable();
    let emph: Vec<i32> = EMPHASIS_LUT.iter().map(|&v| v as i32).collect();

    format!(
        r"
struct Uniforms {{
    rect: vec4<f32>,   // letterbox transform (same shape + math as gfx.wgsl)
    crop: vec4<f32>,   // overscan crop: x=v-scale, y=v-offset, z=u-scale, w=u-offset
    params: vec4<f32>, // x = videoPhase (0..2), rest reserved
    knobs: vec4<f32>,  // x = contrast, y = saturation, z = brightness, w = hue (degrees)
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

// Base YIQ matrix scalars (Bisqwit / Mesen). The live contrast / saturation
// knobs scale these per frame; at knob = 0 the integer matrix below equals the
// old baked Y/IR/QR/... constants exactly (verified in f32).
const CONTRAST_BASE: f32 = 167941.0;
const SATURATION_BASE: f32 = 144044.0;
const IR_C: f32 = 1.994681e-6;
const QR_C: f32 = 9.915742e-7;
const IG_C: f32 = 9.151351e-8;
const QG_C: f32 = -6.334805e-7;
const IB_C: f32 = -1.012984e-6;
const QB_C: f32 = 1.667217e-6;
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
    // Fullscreen triangle; letterbox in UV space (not by scaling position) so
    // the bars clip to black in fs (no ClampToEdge edge-smear).
    var out: VsOut;
    out.pos = vec4<f32>(pos[vid], 0.0, 1.0);
    out.uv = (uv[vid] - vec2<f32>(0.5, 0.5) - vec2<f32>(u.rect.z, u.rect.w))
        / vec2<f32>(u.rect.x, u.rect.y) + vec2<f32>(0.5, 0.5);
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
    let emphasis = (ppu >> 6) & 7;
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
    // Letterbox bars -> black.
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {{
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }}
    // Overscan crop: remap the visible V (crop.xy) and U (crop.zw) ranges.
    let suv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);

    let video_phase = i32(u.params.x + 0.5);
    let row = clamp(i32(suv.y * 240.0), 0, 239);

    // Centre signal position for this output pixel (256*8 samples per line).
    let center = i32(suv.x * 256.0 * 8.0);

    // Per-row decode phase: phase0 = (startCycle + 7) % 12,
    // startCycle = (videoPhase*4 + row*341*8) % 12.
    let start_cycle = ((video_phase * 4 + row * 341 * 8) % 12 + 12) % 12;
    let phase0 = (start_cycle + 7) % 12;

    // Live picture knobs (C1): brightness is the additive luma seed, contrast +
    // saturation scale the YIQ matrix, hue rotates the demodulated (I, Q) vector.
    // At knobs = 0 this reproduces the pre-C1 constants byte-for-byte.
    var ysum = i32(u.knobs.z);
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

    // Hue rotation of the (I, Q) vector by `hue` degrees (identity at 0). Done in
    // f32 then truncated back to integer so the matrix multiply below is unchanged.
    // Gated on `hue != 0` so the default (hue==0, byte-identical) path skips the
    // per-fragment cos()/sin() trig entirely. `hue` is a uniform, so this branch
    // is coherent across the workgroup (cheap), and the hue==0 result is identical
    // to multiplying by the identity rotation (no float round-trip applied).
    if u.knobs.w != 0.0 {{
        let hue_rad = u.knobs.w * 0.017453292; // pi/180
        let hc = cos(hue_rad);
        let hs = sin(hue_rad);
        let isum_r = i32(f32(isum) * hc - f32(qsum) * hs);
        let qsum_r = i32(f32(isum) * hs + f32(qsum) * hc);
        isum = isum_r;
        qsum = qsum_r;
    }}

    // Build the integer YIQ->RGB matrix from the live contrast / saturation.
    let cf = (u.knobs.x + 1.0) * (u.knobs.x + 1.0) * CONTRAST_BASE;
    let sf = (u.knobs.y + 1.0) * (u.knobs.y + 1.0) * SATURATION_BASE;
    let wf = f32(FW);
    let my = i32(cf / wf);
    let ir = i32(cf * IR_C * sf / wf);
    let qr = i32(cf * QR_C * sf / wf);
    let ig = i32(cf * IG_C * sf / wf);
    let qg = i32(cf * QG_C * sf / wf);
    let ib = i32(cf * IB_C * sf / wf);
    let qb = i32(cf * QB_C * sf / wf);

    let r = clamp((ysum * my + isum * ir + qsum * qr) / 65536, 0, 255);
    let g = clamp((ysum * my + isum * ig + qsum * qg) / 65536, 0, 255);
    let b = clamp((ysum * my + isum * ib + qsum * qb) / 65536, 0, 255);

    return vec4<f32>(f32(r) / 255.0, f32(g) / 255.0, f32(b) / 255.0, 1.0);
}}
",
        signal_low = wgsl_i32_array(&low),
        signal_high = wgsl_i32_array(&high),
        sine = wgsl_i32_array(&sine),
        emphasis = wgsl_i32_array(&emph),
        fw = FILTER_WIDTH,
    )
}

/// True composite NES_NTSC filter (Bisqwit algorithm).
pub struct NtscBisqwitFilter {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Live picture knobs, written into the per-frame uniform on each
    /// [`Self::render`]. Default = byte-identical to the pre-C1 decode.
    knobs: NtscKnobs,
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
            // The shared, byte-identical WGSL (also used by the Android renderer).
            // `shader_src()` remains the generator/source of truth; a drift test
            // (below) asserts the two are equal.
            source: wgpu::ShaderSource::Wgsl(rustynes_gfx_shaders::BISQWIT_WGSL.into()),
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
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ntsc-bisqwit-pipeline"),
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
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ntsc-bisqwit-uniforms"),
            // rect (identity) + crop (none) + params (videoPhase=0) + knobs (0).
            contents: bytemuck::cast_slice(&[
                1.0f32, 1.0, 0.0, 0.0, // rect
                1.0, 0.0, 1.0, 0.0, // crop (v-scale, v-off, u-scale, u-off)
                0.0, 0.0, 0.0, 0.0, // params
                0.0, 0.0, 0.0, 0.0, // knobs (contrast, saturation, brightness, hue)
            ]),
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
            knobs: NtscKnobs::DEFAULT,
        }
    }

    /// Update the live picture knobs (contrast / saturation / brightness / hue).
    /// Takes effect on the next [`Self::render`]; at [`NtscKnobs::DEFAULT`] the
    /// decode is byte-identical to the pre-C1 filter.
    pub const fn set_knobs(&mut self, knobs: NtscKnobs) {
        self.knobs = knobs;
    }

    /// Render the filter into `out_view`, sampling the index texture the filter
    /// was constructed over. `video_phase` is the core's per-frame NTSC phase
    /// (0..=2). Letterboxes + applies 8:7 pixel-aspect / overscan crop to
    /// (`width`, `height`) exactly like `Gfx::render`'s main blit (shared
    /// `gfx::letterbox_uniform`).
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        video_phase: u8,
        par_correction: bool,
        overscan: crate::config::Overscan,
    ) {
        // rect (4) + crop (4) from the shared helper, then params (videoPhase) +
        // knobs (contrast, saturation, brightness, hue).
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
            f32::from(video_phase),
            0.0,
            0.0,
            0.0,
            self.knobs.contrast,
            self.knobs.saturation,
            self.knobs.brightness,
            self.knobs.hue,
        ];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&uniform));

        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ntsc-bisqwit-pass"),
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
    use super::*;

    /// The shared `rustynes_gfx_shaders::BISQWIT_WGSL` const (used at runtime by
    /// both the desktop filter and the Android renderer) must stay byte-identical to
    /// this crate's generator. If the Bisqwit model here changes, regenerate the
    /// committed `crates/rustynes-gfx-shaders/src/bisqwit.wgsl`.
    #[test]
    fn shared_bisqwit_wgsl_matches_generator() {
        // Normalize line endings before comparing: a Windows checkout (autocrlf) can
        // hand the committed `.wgsl` CRLF while the generator emits LF — the WGSL is
        // identical either way. `.gitattributes` pins the file to LF, but normalize
        // here too so the drift test can never flake on line endings.
        assert_eq!(
            shader_src().replace("\r\n", "\n"),
            rustynes_gfx_shaders::BISQWIT_WGSL.replace("\r\n", "\n"),
        );
    }

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
        // Attenuated white ($20 in the h=1 / emphasised half) is dimmer than
        // unattenuated white but still a positive high level.
        let att_white = high[0x40 | 0x20];
        assert!(
            att_white > 0 && att_white < high[0x20],
            "attenuated < full white"
        );
    }

    /// C1 byte-identity guard: the integer YIQ matrix the shader computes from
    /// the live knobs at [`NtscKnobs::DEFAULT`] must equal the pre-C1 baked
    /// constants exactly (the in-shader math is f32; this mirrors it in f32). If
    /// this drifts, the default `composite-rt` output is no longer byte-identical.
    #[test]
    #[allow(clippy::float_cmp)]
    fn default_knobs_match_legacy_matrix() {
        // The contrast / saturation factors at knob = 0 are the legacy bases.
        let cf: f32 = (0.0_f32 + 1.0) * (0.0_f32 + 1.0) * 167_941.0;
        let sf: f32 = (0.0_f32 + 1.0) * (0.0_f32 + 1.0) * 144_044.0;
        let w = FILTER_WIDTH as f32;
        // Mirror the WGSL matrix build (f32, truncating `i32(...)` casts).
        let y = (cf / w) as i32;
        let ir = (cf * 1.994_681e-6 * sf / w) as i32;
        let qr = (cf * 9.915_742e-7 * sf / w) as i32;
        let ig = (cf * 9.151_351e-8 * sf / w) as i32;
        let qg = (cf * -6.334_805e-7 * sf / w) as i32;
        let ib = (cf * -1.012_984e-6 * sf / w) as i32;
        let qb = (cf * 1.667_217e-6 * sf / w) as i32;
        // The exact constants the pre-C1 shader baked in.
        assert_eq!(y, 13_995);
        assert_eq!(ir, 4_021);
        assert_eq!(qr, 1_998);
        assert_eq!(ig, 184);
        assert_eq!(qg, -1_277);
        assert_eq!(ib, -2_042);
        assert_eq!(qb, 3_360);
        // Brightness / hue are no-ops at the default.
        assert_eq!(NtscKnobs::DEFAULT.brightness, 0.0);
        assert_eq!(NtscKnobs::DEFAULT.hue, 0.0);
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
