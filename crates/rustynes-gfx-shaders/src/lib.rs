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
