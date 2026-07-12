// crt-guest-advanced / guest-dr-venom (single-pass WGSL port) — v2.1.9 (B6).
//
// A single-pass condensation of guest.r's crt-guest-advanced / guest-dr-venom
// libretro slang shaders. Those are a large multi-pass stack (linearize, two
// blur passes for glow + halation, the scanline/mask pass, an AfterGlow pass);
// this port keeps guest's characteristic look in one fragment shader:
//
//   * A sharp horizontal beam profile (guest's "beam shape" — a configurable
//     scan-width with a controllable inner/outer falloff) rather than the pure
//     Gaussian of CRT-Royale, giving the crisper guest-dr-venom scanlines.
//   * Halation / glow: a cheap 5-tap neighbourhood bloom mixed back in linear
//     light (`aux.z` glow amount), approximating guest's separate blur passes.
//   * Selectable mask (aperture / slot / shadow) shared with the CRT stack.
//   * Barrel curvature.
//
// Uniform layout (16 f32 / 64 bytes) matches the shared CRT-stack block:
//   rect, crop as in CRT_WGSL.
//   params: (x = scanline weight, y = mask strength, z = mask type, w = curvature)
//   aux   : (x = beam width 0..1, y = source rows, z = glow amount 0..1,
//            w = sharpness 0..1)
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

// guest's beam: a power-shaped scanline. `d` is the fractional distance from
// the row centre (-0.5..0.5); `width` widens the bright core, `sharp` steepens
// the edge falloff.
fn guest_beam(d: f32, width: f32, sharp: f32) -> f32 {
    let x = abs(d) / max(width, 0.05);
    let p = mix(1.5, 4.0, clamp(sharp, 0.0, 1.0));
    return clamp(1.0 - pow(clamp(x, 0.0, 1.0), p), 0.0, 1.0);
}

fn phosphor_mask(px: vec2<f32>, kind: f32, strength: f32) -> vec3<f32> {
    let dim = 1.0 - strength;
    var m = vec3<f32>(dim, dim, dim);
    let col = i32(floor(px.x)) % 3;
    if (kind < 0.5) {
        if (col == 0) { m.r = 1.0; } else if (col == 1) { m.g = 1.0; } else { m.b = 1.0; }
    } else if (kind < 1.5) {
        let rowpair = i32(floor(px.y / 2.0)) % 2;
        let ccol = (col + rowpair) % 3;
        if (ccol == 0) { m.r = 1.0; } else if (ccol == 1) { m.g = 1.0; } else { m.b = 1.0; }
    } else {
        let cx = i32(floor(px.x)) % 2;
        let cy = i32(floor(px.y)) % 2;
        if (cx == 0 && cy == 0) { m.r = 1.0; }
        else if (cx == 1 && cy == 0) { m.g = 1.0; }
        else { m.b = 1.0; }
    }
    return m;
}

// 5-tap halation glow around `suv` in linear light.
fn glow(suv: vec2<f32>) -> vec3<f32> {
    let tx = 1.0 / 256.0;
    let ty = 1.0 / 240.0;
    var acc = vec3<f32>(0.0, 0.0, 0.0);
    acc = acc + textureSample(nes_tex, nes_smp, suv).rgb * 0.4;
    acc = acc + textureSample(nes_tex, nes_smp, suv + vec2<f32>( 2.0 * tx, 0.0)).rgb * 0.15;
    acc = acc + textureSample(nes_tex, nes_smp, suv + vec2<f32>(-2.0 * tx, 0.0)).rgb * 0.15;
    acc = acc + textureSample(nes_tex, nes_smp, suv + vec2<f32>(0.0,  2.0 * ty)).rgb * 0.15;
    acc = acc + textureSample(nes_tex, nes_smp, suv + vec2<f32>(0.0, -2.0 * ty)).rgb * 0.15;
    return acc;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let scan = u.params.x;
    let mask_amt = u.params.y;
    let mask_kind = u.params.z;
    let curvature = u.params.w;
    let width = mix(0.25, 0.8, u.aux.x);
    let rows = select(240.0, u.aux.y, u.aux.y >= 1.0);
    let glow_amt = u.aux.z;
    let sharp = u.aux.w;

    var uv = in.uv;
    if (curvature > 0.001) {
        uv = curve(uv, curvature * 0.25);
    }
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let suv = vec2<f32>(uv.x * u.crop.z + u.crop.w, uv.y * u.crop.x + u.crop.y);

    var rgb = textureSample(nes_tex, nes_smp, suv).rgb;
    rgb = pow(rgb, vec3<f32>(2.2)); // to linear

    // Scanline beam in source-row space.
    let src_y = suv.y * rows;
    let d = fract(src_y) - 0.5;
    let b = guest_beam(d, width, sharp);
    rgb = rgb * mix(1.0, b, scan);

    // Halation glow mixed additively in linear light.
    if (glow_amt > 0.001) {
        let g = pow(glow(suv), vec3<f32>(2.2));
        rgb = rgb + g * glow_amt * 0.5;
    }

    // Mask.
    let mask = phosphor_mask(in.pos.xy, mask_kind, mask_amt);
    rgb = rgb * mix(vec3<f32>(1.0), mask, mask_amt);
    rgb = rgb * (1.0 + 0.5 * (scan + mask_amt));

    rgb = pow(clamp(rgb, vec3<f32>(0.0), vec3<f32>(4.0)), vec3<f32>(1.0 / 2.2));
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
