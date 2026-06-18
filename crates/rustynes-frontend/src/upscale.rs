#![allow(clippy::too_many_arguments, clippy::doc_markdown)]

//! Pixel-art upscaler post-passes — wgsl (v1.6.0 "Studio" I2): hqNx / xBRZ-style.
//!
//! Two edge-directed pixel-art filters as RGBA [`crate::shader_pass::ShaderStack`]
//! passes. Both sample the already-rendered framebuffer and smooth hard diagonal
//! edges (the staircase artifacts of nearest-neighbour scaling) while preserving
//! flat areas — the look of the hqNx (Maxim Stepin) and xBRZ (Zenju) families.
//!
//! Because the ShaderStack ping-pongs at the fixed NES resolution (256x240), a
//! true integer-NxN scaler can't widen the buffer; instead each pass runs the
//! filter's **edge-detection + directional-blend kernel** at the sample point,
//! producing the same smoothing the integer scalers are prized for, then the
//! final letterbox blit scales to the window. This is the standard single-pass
//! GPU adaptation (cf. the LibRetro `hqx`/`xbr` "fast" shader variants) and
//! keeps the wasm/WebGL path happy (no storage buffers, no value-array dynamic
//! indexing).
//!
//! **Output-only**: these post-passes never touch the core, the index
//! framebuffer, or determinism — they only run when the user adds them to the
//! stack, so the byte-identical default and AccuracyCoin both hold.
//!
//! References: hqx (Maxim Stepin, LGPL — algorithm only, independent WGSL),
//! xBR/xBRZ (Hyllian / Zenju, public algorithm). These are independent WGSL
//! adaptations of the published edge-blend models, not ports of GPL/LGPL code.

/// hqNx ("hq") edge-directed smoothing pass parameters.
///
/// The blend is driven by the luma-difference threshold baked into the kernel,
/// plus a single `strength` knob that scales the diagonal blend back toward
/// nearest-neighbour.
pub const HQX_PARAMS: &str = "// #pragma parameter strength \"Blend strength\" 1.0 0.0 1.0 0.05\n";

/// xBRZ-style edge-directed smoothing pass with a `strength` knob.
pub const XBRZ_PARAMS: &str = "// #pragma parameter strength \"Blend strength\" 1.0 0.0 1.0 0.05\n";

/// hqNx `#pragma parameter` declarations.
#[must_use]
pub const fn hqx_params() -> &'static str {
    HQX_PARAMS
}

/// xBRZ `#pragma parameter` declarations.
#[must_use]
pub const fn xbrz_params() -> &'static str {
    XBRZ_PARAMS
}

/// Shared YUV-distance helper + fullscreen-triangle vertex stage WGSL prelude.
/// Concatenated in front of each filter body so both share the boilerplate.
const PRELUDE: &str = r"
struct Uniforms {
    rect: vec4<f32>,
    crop: vec4<f32>,
    params: vec4<f32>, // x = strength
    aux: vec4<f32>,
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

// Perceptual distance between two colours (weighted YUV — the hqx/xbr metric).
fn cdist(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = a - b;
    let y = dot(d, vec3<f32>(0.299, 0.587, 0.114));
    let uu = dot(d, vec3<f32>(-0.169, -0.331, 0.5));
    let vv = dot(d, vec3<f32>(0.5, -0.419, -0.081));
    return abs(y) * 48.0 + abs(uu) * 7.0 + abs(vv) * 6.0;
}
";

/// hqNx-style fragment body. Examines the 3x3 neighbourhood of the source texel
/// and, where a diagonal edge is detected (the two corner-adjacent neighbours
/// differ strongly from the centre), blends the sub-pixel toward the dominant
/// edge colour — the hqx interpolation rule, simplified to a single pass.
const HQX_BODY: &str = r"
const THRESH: f32 = 24.0;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let suv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);
    let strength = u.params.x;
    let ts = vec2<f32>(1.0 / 256.0, 1.0 / 240.0);

    // Centre + 4-connected neighbours.
    let c = textureSample(nes_tex, nes_smp, suv).rgb;
    let n = textureSample(nes_tex, nes_smp, suv + vec2<f32>(0.0, -ts.y)).rgb;
    let s = textureSample(nes_tex, nes_smp, suv + vec2<f32>(0.0,  ts.y)).rgb;
    let w = textureSample(nes_tex, nes_smp, suv + vec2<f32>(-ts.x, 0.0)).rgb;
    let e = textureSample(nes_tex, nes_smp, suv + vec2<f32>( ts.x, 0.0)).rgb;

    // Sub-pixel position inside the source texel (0..1).
    let f = fract(suv / ts);

    // Edge-directed blend: in each quadrant, if the two bordering neighbours
    // match each other but differ from the centre, the corner is an edge — pull
    // the colour toward the neighbour pair (rounds the staircase).
    var outc = c;
    // Pick the relevant horizontal/vertical neighbour for this sub-pixel.
    let h = select(w, e, f.x > 0.5);
    let v = select(n, s, f.y > 0.5);
    if (cdist(h, v) < THRESH && cdist(c, h) > THRESH) {
        // Distance from the texel centre toward this corner (0 at centre .. 1).
        let corner = clamp((abs(f.x - 0.5) + abs(f.y - 0.5)), 0.0, 1.0);
        let blended = mix(c, 0.5 * (h + v), corner);
        outc = mix(c, blended, clamp(strength, 0.0, 1.0));
    }
    return vec4<f32>(outc, 1.0);
}
";

/// xBRZ-style fragment body. Like hqNx but with the xBR diagonal-dominance test:
/// it compares the two diagonals through the texel and blends along whichever
/// diagonal is the *weaker* edge (the xBR "blend the smoother diagonal" rule),
/// giving the rounder corners xBRZ is known for.
const XBRZ_BODY: &str = r"
const THRESH: f32 = 20.0;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let suv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);
    let strength = u.params.x;
    let ts = vec2<f32>(1.0 / 256.0, 1.0 / 240.0);

    let c  = textureSample(nes_tex, nes_smp, suv).rgb;
    let nw = textureSample(nes_tex, nes_smp, suv + vec2<f32>(-ts.x, -ts.y)).rgb;
    let ne = textureSample(nes_tex, nes_smp, suv + vec2<f32>( ts.x, -ts.y)).rgb;
    let sw = textureSample(nes_tex, nes_smp, suv + vec2<f32>(-ts.x,  ts.y)).rgb;
    let se = textureSample(nes_tex, nes_smp, suv + vec2<f32>( ts.x,  ts.y)).rgb;

    let f = fract(suv / ts);

    // The two diagonals' edge energy. xBR blends along the diagonal whose
    // endpoints differ LESS (the continuous edge) when it beats the other.
    let d_main = cdist(nw, se);   // top-left .. bottom-right
    let d_anti = cdist(ne, sw);   // top-right .. bottom-left

    var outc = c;
    // Which diagonal does this sub-pixel sit nearest?
    let on_main = (f.x - 0.5) * (f.y - 0.5) > 0.0; // same-sign => main diagonal half
    let chosen = select(0.5 * (ne + sw), 0.5 * (nw + se), on_main);
    let d_this = select(d_anti, d_main, on_main);
    let d_other = select(d_main, d_anti, on_main);
    if (d_this < THRESH && d_other > d_this + THRESH && cdist(c, chosen) > THRESH) {
        let corner = clamp(abs(f.x - 0.5) + abs(f.y - 0.5), 0.0, 1.0);
        let blended = mix(c, chosen, corner);
        outc = mix(c, blended, clamp(strength, 0.0, 1.0));
    }
    return vec4<f32>(outc, 1.0);
}
";

/// Full hqNx WGSL (prelude + body).
#[must_use]
pub fn hqx_shader_src() -> String {
    format!("{PRELUDE}{HQX_BODY}")
}

/// Full xBRZ WGSL (prelude + body).
#[must_use]
pub fn xbrz_shader_src() -> String {
    format!("{PRELUDE}{XBRZ_BODY}")
}

#[cfg(test)]
mod tests {
    fn validate(src: &str, what: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{what} WGSL must parse: {e:?}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{what} WGSL must validate: {e:?}"));
    }

    #[test]
    fn hqx_shader_parses_and_validates() {
        validate(&super::hqx_shader_src(), "hqNx");
    }

    #[test]
    fn xbrz_shader_parses_and_validates() {
        validate(&super::xbrz_shader_src(), "xBRZ");
    }

    #[test]
    fn both_declare_strength() {
        for src in [super::HQX_PARAMS, super::XBRZ_PARAMS] {
            let params = crate::shader_pass::parse_pragma_parameters(src);
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "strength");
        }
    }
}
