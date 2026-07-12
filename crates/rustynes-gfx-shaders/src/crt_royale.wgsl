// CRT-Royale (single-pass WGSL port) — v2.1.9 "Presentation & Signal" (B6).
//
// A faithful *single-pass* condensation of TroggleMonkey's libretro CRT-Royale
// slang preset. The reference is a multi-pass pipeline (bloom/blur passes +
// scanline + phosphor mask + halation + geometry); this port folds its core
// perceptual model into one fullscreen fragment shader so it slots into the
// existing RustyNES post-pass pipeline (same rect/crop letterbox convention as
// CRT_WGSL). It keeps CRT-Royale's defining pieces:
//
//   * Gaussian scanline beam in gamma-linear space (per-source-row beam with a
//     configurable standard deviation, so bright rows bloom wider than dark
//     rows — the "beam thickness follows luminance" look).
//   * Selectable phosphor mask (0 = aperture grille, 1 = slot mask, 2 = shadow
//     / EEK dot mask) with mask brightness compensation.
//   * Input/output gamma (CRT-Royale decodes to linear, applies the beam, then
//     re-encodes) so the scanline math is photometrically correct.
//   * Optional barrel curvature + a soft vignette at the tube edges.
//
// Uniform layout (16 f32 / 64 bytes), shared by the whole CRT stack:
//   rect  : vec4 letterbox transform (x,y = scale, z,w = offset)
//   crop  : vec4 overscan (x = v-scale, y = v-offset, z = u-scale, w = u-offset)
//   params: vec4 (x = scanline weight 0..1, y = mask strength 0..1,
//                 z = mask type {0,1,2}, w = curvature 0..1)
//   aux   : vec4 (x = beam sigma, y = input gamma, z = output gamma,
//                 w = source rows, default 240)
//
// Presentation only — never touches the emulation core or determinism contract.

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

// Barrel-distort a centred UV by curvature amount `k` (0 = flat).
fn curve(uv: vec2<f32>, k: f32) -> vec2<f32> {
    let cc = uv - vec2<f32>(0.5, 0.5);
    let dist = dot(cc, cc) * k;
    return uv + cc * (1.0 + dist) * dist;
}

// Gaussian beam weight for a source-row distance `d` (in rows) and sigma.
fn beam(d: f32, sigma: f32) -> f32 {
    let s = max(sigma, 0.05);
    return exp(-(d * d) / (2.0 * s * s));
}

// Phosphor mask multiplier for output column/row and mask type.
fn phosphor_mask(px: vec2<f32>, kind: f32, strength: f32) -> vec3<f32> {
    let one = vec3<f32>(1.0, 1.0, 1.0);
    let dim = 1.0 - strength;
    var m = one;
    let col = i32(floor(px.x)) % 3;
    if (kind < 0.5) {
        // Aperture grille: vertical RGB triads.
        m = vec3<f32>(dim, dim, dim);
        if (col == 0) { m.r = 1.0; } else if (col == 1) { m.g = 1.0; } else { m.b = 1.0; }
    } else if (kind < 1.5) {
        // Slot mask: aperture grille offset every other pair of rows.
        let rowpair = i32(floor(px.y / 2.0)) % 2;
        let ccol = (col + rowpair) % 3;
        m = vec3<f32>(dim, dim, dim);
        if (ccol == 0) { m.r = 1.0; } else if (ccol == 1) { m.g = 1.0; } else { m.b = 1.0; }
    } else {
        // Shadow / dot mask: 2x2 dot pattern per channel.
        let cx = i32(floor(px.x)) % 2;
        let cy = i32(floor(px.y)) % 2;
        m = vec3<f32>(dim, dim, dim);
        if (cx == 0 && cy == 0) { m.r = 1.0; }
        else if (cx == 1 && cy == 0) { m.g = 1.0; }
        else { m.b = 1.0; }
    }
    return m;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let scan = u.params.x;
    let mask_amt = u.params.y;
    let mask_kind = u.params.z;
    let curvature = u.params.w;
    let sigma = u.aux.x;
    let gamma_in = max(u.aux.y, 0.1);
    let gamma_out = max(u.aux.z, 0.1);
    let rows = select(240.0, u.aux.w, u.aux.w >= 1.0);

    // Curvature (about the letterboxed picture centre).
    var uv = in.uv;
    if (curvature > 0.001) {
        uv = curve(uv, curvature * 0.25);
    }
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let suv = vec2<f32>(uv.x * u.crop.z + u.crop.w, uv.y * u.crop.x + u.crop.y);

    // Integrate the Gaussian beam over the three nearest source rows in linear
    // light. Sampling each row's colour at the pixel's horizontal position gives
    // vertical beam bloom without a separate blur pass.
    let src_y = suv.y * rows;
    let centre = floor(src_y - 0.5) + 0.5;
    var acc = vec3<f32>(0.0, 0.0, 0.0);
    var wsum = 0.0;
    for (var i = -1; i <= 1; i = i + 1) {
        let ry = centre + f32(i);
        let sample_v = (ry + 0.5) / rows;
        var c = textureSample(nes_tex, nes_smp, vec2<f32>(suv.x, sample_v)).rgb;
        c = pow(c, vec3<f32>(gamma_in)); // to linear
        // Beam brightness scales its own width: brighter rows spread more.
        let lum = dot(c, vec3<f32>(0.299, 0.587, 0.114));
        let w = beam(src_y - ry, sigma * mix(0.6, 1.4, lum)) * mix(1.0, lum + 0.2, scan);
        acc = acc + c * w;
        wsum = wsum + w;
    }
    var rgb = acc / max(wsum, 1e-4);

    // Phosphor mask in linear light, then re-encode.
    let mask = phosphor_mask(in.pos.xy, mask_kind, mask_amt);
    rgb = rgb * mix(vec3<f32>(1.0), mask, mask_amt);
    // Mask + scanline energy loss compensation.
    rgb = rgb * (1.0 + 0.6 * (scan + mask_amt));
    rgb = pow(clamp(rgb, vec3<f32>(0.0), vec3<f32>(4.0)), vec3<f32>(1.0 / gamma_out));

    // Soft edge vignette when curved.
    if (curvature > 0.001) {
        let e = uv - vec2<f32>(0.5, 0.5);
        let v = 1.0 - dot(e, e) * curvature * 0.6;
        rgb = rgb * clamp(v, 0.0, 1.0);
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
