// Sony Megatron (single-pass WGSL port) — v2.1.9 "Presentation & Signal" (B6).
//
// A port of MajorPainInTheCactus's "Sony Megatron Colour Video Monitor" slang
// shader. Megatron's defining idea is a physically-scaled phosphor subpixel
// model driven for HDR displays: it lights individual R/G/B phosphors within a
// selectable mask and scales brightness to an absolute nits target so an HDR
// swapchain reproduces CRT peak brightness. WGSL/wgpu here targets an SDR
// swapchain by default, so this port keeps Megatron's *structure* — per-subpixel
// phosphor lighting, mask selection, gamma-correct scanline beam, and an
// exposed peak/paper-white ratio — but tone-maps the result back into [0,1]
// with a Reinhard curve (`aux.w` controls the HDR headroom the tone-map
// assumes). On an HDR path a host can skip the final tone-map and scale by the
// nits target instead; the hook (`hdr` in aux.z) is left in the uniform.
//
// Uniform layout (16 f32 / 64 bytes), shared CRT-stack block. The aux knobs are
// ordered so the composable-stack `#pragma parameter` sliders fill them
// contiguously and the rarely-touched source-row count lands last (0 -> the 240
// default via `select`):
//   rect, crop as in CRT_WGSL.
//   params: (x = scanline weight, y = mask strength, z = mask type, w = curvature)
//   aux   : (x = beam sigma, y = hdr headroom / peak ratio (default 4.0),
//            z = hdr flag {0,1}, w = source rows, default 240)
//
// Presentation only — never touches the core or the determinism contract.

struct Uniforms {
    rect: vec4<f32>,
    crop: vec4<f32>,
    params: vec4<f32>,
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

fn curve(uv: vec2<f32>, k: f32) -> vec2<f32> {
    let cc = uv - vec2<f32>(0.5, 0.5);
    let dist = dot(cc, cc) * k;
    return uv + cc * (1.0 + dist) * dist;
}

fn beam(d: f32, sigma: f32) -> f32 {
    let s = max(sigma, 0.05);
    return exp(-(d * d) / (2.0 * s * s));
}

// Megatron lights each phosphor subpixel individually: return a per-channel
// weight for the output pixel's position within the selected mask cell.
fn subpixel(px: vec2<f32>, kind: f32) -> vec3<f32> {
    let col = i32(floor(px.x)) % 3;
    if (kind < 0.5) {
        // Aperture grille: one lit phosphor column per triad.
        if (col == 0) { return vec3<f32>(1.0, 0.0, 0.0); }
        if (col == 1) { return vec3<f32>(0.0, 1.0, 0.0); }
        return vec3<f32>(0.0, 0.0, 1.0);
    } else if (kind < 1.5) {
        // Slot mask: staggered every two rows.
        let rowpair = i32(floor(px.y / 2.0)) % 2;
        let ccol = (col + rowpair) % 3;
        if (ccol == 0) { return vec3<f32>(1.0, 0.0, 0.0); }
        if (ccol == 1) { return vec3<f32>(0.0, 1.0, 0.0); }
        return vec3<f32>(0.0, 0.0, 1.0);
    }
    // Shadow / dot mask.
    let cx = i32(floor(px.x)) % 2;
    let cy = i32(floor(px.y)) % 2;
    if (cx == 0 && cy == 0) { return vec3<f32>(1.0, 0.0, 0.0); }
    if (cx == 1 && cy == 0) { return vec3<f32>(0.0, 1.0, 0.0); }
    return vec3<f32>(0.0, 0.0, 1.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let scan = u.params.x;
    let mask_amt = u.params.y;
    let mask_kind = u.params.z;
    let curvature = u.params.w;
    let sigma = u.aux.x;
    let headroom = max(u.aux.y, 1.0);
    let rows = select(240.0, u.aux.w, u.aux.w >= 1.0);

    var uv = in.uv;
    if (curvature > 0.001) {
        uv = curve(uv, curvature * 0.25);
    }
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let suv = vec2<f32>(uv.x * u.crop.z + u.crop.w, uv.y * u.crop.x + u.crop.y);

    var rgb = textureSample(nes_tex, nes_smp, suv).rgb;
    rgb = pow(rgb, vec3<f32>(2.4)); // to linear (Megatron uses ~2.4 EOTF)

    // Gaussian beam in source-row space, scaled by luminance (thicker beam for
    // brighter content — Megatron's beam-dynamics knob, simplified).
    let src_y = suv.y * rows;
    let d = fract(src_y) - 0.5;
    let lum = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let b = beam(d, sigma * mix(0.7, 1.3, lum));
    rgb = rgb * mix(1.0, b, scan);

    // Per-subpixel phosphor lighting: push brightness into headroom so the lit
    // phosphor reaches peak while unlit ones go dark (energy is concentrated,
    // then tone-mapped back for SDR).
    let sp = subpixel(in.pos.xy, mask_kind);
    let lit = mix(vec3<f32>(1.0), sp, mask_amt);
    rgb = rgb * lit * mix(1.0, headroom, mask_amt);

    // Reinhard tone-map back to SDR (skipped on a real HDR path).
    rgb = rgb / (rgb + vec3<f32>(1.0)) * (1.0 + 1.0 / headroom);
    rgb = pow(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.4));
    return vec4<f32>(rgb, 1.0);
}
