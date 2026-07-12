//! Shared WGSL presentation-shader sources for the RustyNES wgpu render path.
//!
//! These `pub const` strings are the single source of truth for the presentation
//! shaders used by BOTH the desktop frontend (`rustynes-frontend`) and the Android
//! wgpu renderer (`rustynes-android`), so the on-screen look matches across
//! platforms without copy-paste drift.
//!
//! Presentation only — nothing here touches the emulation core or the determinism
//! contract. The shaders are deliberately self-contained (one uniform block, one
//! input texture + sampler) so any wgpu host can build a pipeline over them.

#![no_std]
// The docs are full of graphics acronyms (WGSL, NES, CRT, RGB, PAR) and the mixed-
// case crate name; backticking each would hurt readability, so allow doc_markdown
// here (the desktop crt.rs takes the same exemption).
#![allow(clippy::doc_markdown)]

/// CRT / scanline post-process WGSL (a single fullscreen pass).
///
/// Letterboxes the 256x240 NES texture into the surface (UV-space, clipping the
/// bars to black) and applies, from the `params` uniform:
/// 1. **Scanlines** — a parabolic brightness profile per NES source row
///    (`params.x` = intensity, 0 = off), so it looks right at any output size.
/// 2. **Aperture mask** — a subtle RGB phosphor grille keyed off the output column
///    (`params.y` = intensity), with a small brightness compensation.
///
/// Uniform layout (12 `f32`): `rect` (letterbox: x,y = scale, z,w = offset),
/// `crop` (overscan: x = v-scale, y = v-offset, z = u-scale, w = u-offset),
/// `params` (x = scanline, y = mask, z,w unused). Setting `params` to (0,0) and
/// `crop` to (1,0,1,0) yields a plain letterboxed blit.
pub const CRT_WGSL: &str = r"
struct Uniforms {
    rect: vec4<f32>,   // letterbox transform (same shape + math as gfx.wgsl)
    crop: vec4<f32>,   // overscan crop: x=v-scale, y=v-offset, z=u-scale, w=u-offset
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
    // Overscan crop: remap the visible V (crop.xy) and U (crop.zw) ranges.
    let suv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);
    var rgb = textureSample(nes_tex, nes_smp, suv).rgb;

    let scan_amt = u.params.x;
    let mask_amt = u.params.y;

    // Scanlines in source-row space. The row count is params.z (so the host can
    // expose a 'number of scanlines' control); fall back to the NES's 240 rows when
    // unset (params.z < 1, e.g. the desktop, which leaves it 0 -> unchanged).
    // Parabolic profile: 1.0 at the row centre, (1 - scan_amt) at the row boundary.
    let rows = select(240.0, u.params.z, u.params.z >= 1.0);
    let src_y = suv.y * rows;
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

/// LMP88959 NTSC / PAL composite filter (a single RGBA post-pass).
///
/// Simulates the NES composite signal in YIQ space (subcarrier phase-locked
/// to image columns for stable dot-crawl) with `params` = (saturation,
/// sharpness, tint, phase) and `aux.x` = PAL mode. Same `rect`/`crop`
/// letterbox convention as [`CRT_WGSL`]; reused by both the desktop frontend
/// and the Android wgpu renderer.
pub const NTSC_LMP_WGSL: &str = r"
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

/// The Bisqwit composite NES NTSC post-pass (a faithful port of Bisqwit's
/// `nes_ntsc` signal model).
///
/// Unlike CRT/LMP it samples the **palette-index** framebuffer as an `R16Uint`
/// texture (`@group(0) @binding(0) idx_tex`), not the RGBA, plus the per-frame NTSC
/// phase + picture knobs in a 64-byte uniform
/// (`rect`/`crop`/`params[videoPhase]`/`knobs[contrast,sat,bright,hue]`). All the
/// static tables are baked in. Generated verbatim from the desktop's
/// `ntsc_bisqwit::shader_src()`; a drift test in `rustynes-frontend` asserts the two
/// stay identical, so editing the model there regenerates this file.
pub const BISQWIT_WGSL: &str = include_str!("bisqwit.wgsl");

// v2.1.9 "Presentation & Signal": the marquee CRT shader stack (B6) + the raw
// NTSC signal-decode pass (P4) live in their own module + WGSL files so they add
// atop the ladder above without disturbing it.
mod crt_stack;
pub use crt_stack::{
    CRT_GUEST_WGSL, CRT_ROYALE_WGSL, CrtStackShader, MEGATRON_WGSL, SIGNAL_DECODE_WGSL,
};
