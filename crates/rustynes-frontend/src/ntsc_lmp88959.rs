#![allow(clippy::too_many_arguments, clippy::doc_markdown)]

//! LMP88959-style composite NTSC/PAL filter — wgsl post-pass (v1.6.0 "Studio" I1).
//!
//! A self-contained composite-NTSC look modelled on EMMIR's well-known
//! `NTSC-CRT` / `LMP88959` algorithm (a single-pass encode-then-decode of the
//! RGB image through a simulated composite signal). Unlike the Bisqwit
//! [`crate::ntsc_bisqwit`] filter — which consumes the `R16Uint` palette-index
//! texture and must be the *first* pass — this one is a pure **RGBA post-pass**:
//! it samples the already-rendered framebuffer, so it composes anywhere in the
//! [`crate::shader_pass::ShaderStack`] (e.g. after an upscaler, before a CRT).
//!
//! ## Algorithm (per output texel)
//!
//! 1. **Encode**: walk a short horizontal window of source texels, convert each
//!    to YIQ, and modulate `I`/`Q` onto a simulated colour subcarrier whose
//!    phase advances with the source column (and, for PAL, alternates the `Q`
//!    sign every other line). This yields a 1-D composite sample per tap.
//! 2. **Luma**: a box low-pass over the composite window recovers `Y`.
//! 3. **Chroma**: quadrature-demodulate the windowed composite against the same
//!    subcarrier to recover `I`/`Q`, low-passed by the window width.
//! 4. **Decode**: YIQ -> RGB, with `saturation` / `tint` / `sharpness` knobs.
//!
//! The visible result is authentic composite artifacting — chroma bleed, dot
//! crawl, and the colour-fringing on hard luma edges — without touching the
//! palette pipeline. It is **output-only** (post-framebuffer); the core, the
//! index framebuffer, and determinism are untouched, so AccuracyCoin and the
//! byte-identical default both hold (the pass only runs when the user adds it).
//!
//! Reference: EMMIR (LMP88959), `NTSC-CRT` (<https://github.com/LMP88959/NTSC-CRT>),
//! public-domain. This is an independent WGSL adaptation of the published
//! encode/decode model, not a line-by-line port.
//!
//! ## Knobs (`#pragma parameter`)
//!
//! - `saturation` — chroma gain (0 = monochrome .. 2 = oversaturated).
//! - `sharpness` — luma window width (0 = soft/blurry .. 1 = sharp).
//! - `tint` — hue rotation in the IQ plane (radians-ish, -0.5 .. 0.5).
//! - `phase` — base subcarrier phase offset (0 .. 1 turn) — shifts the
//!   artifact pattern (akin to the Bisqwit `videoPhase`).
//! - `pal` — 0 = NTSC, 1 = PAL (alternate the Q sign each line, which cancels
//!   hue error at the cost of "Hanover bars" softening).

/// The `#pragma parameter` header lines this pass exposes.
///
/// Parsed by [`crate::shader_pass::parse_pragma_parameters`] and kept separate
/// from the body so the parser sees only the declarations. The parameters are
/// forwarded to the shader uniform in this same declaration order (`params.x..w`,
/// then `aux.x`).
pub const STACK_SHADER_PARAMS: &str = concat!(
    "// #pragma parameter saturation \"Saturation\" 1.0 0.0 2.0 0.05\n",
    "// #pragma parameter sharpness \"Sharpness\" 0.5 0.0 1.0 0.05\n",
    "// #pragma parameter tint \"Tint\" 0.0 -0.5 0.5 0.01\n",
    "// #pragma parameter phase \"Phase\" 0.0 0.0 1.0 0.01\n",
    "// #pragma parameter pal \"PAL mode\" 0.0 0.0 1.0 1.0\n",
);

/// The `#pragma parameter` declarations this pass exposes.
#[must_use]
pub const fn stack_shader_params() -> &'static str {
    STACK_SHADER_PARAMS
}

/// The fragment+vertex WGSL for the LMP88959-style composite pass.
///
/// Uniform layout matches the generic [`crate::shader_pass`] 16-float buffer:
/// `rect`(4) + `crop`(4) + `params`(4) + `aux`(4). This pass reads:
/// `params.x = saturation`, `params.y = sharpness`, `params.z = tint`,
/// `params.w = phase`, `aux.x = pal`.
pub(crate) const SHADER_SRC: &str = r"
struct Uniforms {
    rect: vec4<f32>,   // letterbox transform (same shape + math as gfx.wgsl)
    crop: vec4<f32>,   // overscan crop: x=v-scale, y=v-offset, z=u-scale, w=u-offset
    params: vec4<f32>, // x=saturation, y=sharpness, z=tint, w=phase
    aux: vec4<f32>,    // x=pal mode (0/1), y,z,w unused
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

// RGB -> YIQ (NTSC FCC matrix).
fn rgb2yiq(c: vec3<f32>) -> vec3<f32> {
    let y = dot(c, vec3<f32>(0.299, 0.587, 0.114));
    let i = dot(c, vec3<f32>(0.595716, -0.274453, -0.321263));
    let q = dot(c, vec3<f32>(0.211456, -0.522591, 0.311135));
    return vec3<f32>(y, i, q);
}

// YIQ -> RGB.
fn yiq2rgb(c: vec3<f32>) -> vec3<f32> {
    let r = c.x + 0.9563 * c.y + 0.6210 * c.z;
    let g = c.x - 0.2721 * c.y - 0.6474 * c.z;
    let b = c.x - 1.1070 * c.y + 1.7046 * c.z;
    return vec3<f32>(r, g, b);
}

const PI: f32 = 3.14159265358979;
const TAU: f32 = 6.28318530717959;
// Composite samples per source pixel (the subcarrier resolution of the model).
const SUB: i32 = 4;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Letterbox bars -> black.
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // Overscan crop.
    let suv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);

    let sat = u.params.x;
    let sharp = u.params.y;
    let tint = u.params.z;
    let phase0 = u.params.w;
    let pal = u.aux.x;

    let tx_w = 256.0;
    let tx_h = 240.0;
    let texel = 1.0 / tx_w;
    // Source column + row (integer-ish), used to phase-lock the subcarrier so the
    // artifact pattern is stable in image space (this is what produces dot crawl).
    let col = suv.x * tx_w;
    let row = floor(suv.y * tx_h);
    // PAL alternates the Q (V) phase every other line.
    let pal_flip = select(1.0, -1.0, pal > 0.5 && (fract(row * 0.5) > 0.25));

    // Window half-width in subcarrier samples; sharper => narrower window.
    // Range maps sharpness 0..1 -> half-window 8..2.
    let half = i32(round(mix(8.0, 2.0, clamp(sharp, 0.0, 1.0))));

    var y_acc = 0.0;
    var i_acc = 0.0;
    var q_acc = 0.0;
    var w_sum = 0.0;
    // Walk the composite window. For each subcarrier sample we re-encode the
    // (interpolated) source texel to a 1-D composite value, then demodulate.
    for (var s = -half; s <= half; s = s + 1) {
        let fs = f32(s);
        let samp_col = col + fs / f32(SUB);
        let uvx = samp_col * texel;
        let src = textureSample(nes_tex, nes_smp, vec2<f32>(uvx, suv.y)).rgb;
        let yiq = rgb2yiq(src);
        // Subcarrier phase at this composite sample.
        let ph = TAU * (samp_col / f32(SUB) + phase0) ;
        let cs = cos(ph);
        let sn = sin(ph) * pal_flip;
        // Encode: composite = Y + I*cos + Q*sin.
        let comp = yiq.x + yiq.y * cs + yiq.z * sn;
        // Triangular window weight (low-pass).
        let w = 1.0 - abs(fs) / (f32(half) + 1.0);
        y_acc = y_acc + comp * w;
        // Quadrature demod for chroma (x2 recovers amplitude).
        i_acc = i_acc + comp * cs * w * 2.0;
        q_acc = q_acc + comp * sn * w * 2.0;
        w_sum = w_sum + w;
    }
    let inv = 1.0 / max(w_sum, 1e-4);
    var y = y_acc * inv;
    var i = i_acc * inv;
    var q = q_acc * inv;

    // Tint = rotate the IQ vector; saturation = scale it.
    let ct = cos(tint);
    let st = sin(tint);
    let i2 = (i * ct - q * st) * sat;
    let q2 = (i * st + q * ct) * sat;

    let rgb = yiq2rgb(vec3<f32>(y, i2, q2));
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
";

#[cfg(test)]
mod tests {
    /// The embedded WGSL must parse AND validate (the same front-end + validator
    /// wgpu runs at `create_shader_module`), guarding the dynamic-array and
    /// binding-visibility bug classes that would otherwise only surface at
    /// runtime.
    #[test]
    fn shader_parses_and_validates() {
        let module =
            naga::front::wgsl::parse_str(super::SHADER_SRC).expect("LMP88959 WGSL must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("LMP88959 WGSL must validate");
    }

    /// The `#pragma parameter` declarations parse into the five expected knobs.
    #[test]
    fn params_declared() {
        let params = crate::shader_pass::parse_pragma_parameters(super::STACK_SHADER_PARAMS);
        let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["saturation", "sharpness", "tint", "phase", "pal"]);
    }
}
