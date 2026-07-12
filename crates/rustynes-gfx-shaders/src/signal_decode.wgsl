// Raw NTSC signal-decode post-pass — v2.1.9 "Presentation & Signal" (P4).
//
// The companion shader for `rustynes-ppu::raw_signal`. Unlike the LMP composite
// filter (which re-encodes an already-decoded RGB framebuffer to a 1-D signal
// and back — losing everything the per-color palette already threw away), this
// pass reconstructs the 2C02's ACTUAL two-level chroma square wave from the
// palette-INDEX framebuffer, then demodulates it. Because it works from the
// signal the chip really emits, it reproduces the signal-domain artifacts a
// per-color decode structurally cannot: composite colour bleed between adjacent
// pixels, dot crawl, and the "waterfall"/dither transparency effects (Kirby's
// Adventure waterfalls, the 240p test suite colour-bleed screens).
//
// Input: an index texture (`texture_2d<u32>`, binding 0) where each texel packs
//   bits 0..5  = the 6-bit NES palette index (0..=63)
//   bits 6..8  = the 3-bit emphasis state (0..=7)
// exactly as the PPU emits per dot (the same source the Bisqwit pass consumes).
//
// Model (matches raw_signal.rs, the Bisqwit `nes_ntsc` generator):
//   * 8 signal sub-samples per source pixel; subcarrier phase advances 8 units
//     per pixel over a 12-unit (one-subcarrier) wheel, so `InColorPhase` both
//     positions the hue and produces per-line dot crawl from `params.x`.
//   * two voltage LEVELS per luma; three emphasis bits attenuate by 0.746 on
//     the phases overlapping their primary.
//   * a windowed quadrature demod recovers Y/I/Q, then a standard YIQ->RGB.
//
// Uniform layout (16 f32 / 64 bytes):
//   rect, crop as in CRT_WGSL.
//   params: (x = video phase / line offset, y = saturation, z = sharpness 0..1,
//            w = source rows, default 240)
//   knobs : (x = brightness, y = contrast, z = hue radians, w unused)
//
// Presentation only — reads the index framebuffer, never the core state.

struct Uniforms {
    rect: vec4<f32>,
    crop: vec4<f32>,
    params: vec4<f32>,
    knobs: vec4<f32>,
};

@group(0) @binding(0) var idx_tex: texture_2d<u32>;
@group(0) @binding(1) var<uniform> u: Uniforms;

// LEVELS baked as var<private> so the fragment shader can dynamically index them
// (naga forbids dynamic indexing of const/let value arrays; matches bisqwit.wgsl).
var<private> LEVELS: array<f32, 8> = array<f32, 8>(
    0.350, 0.518, 0.962, 1.550,
    1.094, 1.506, 1.962, 1.962,
);
const BLACK: f32 = 0.518;
const WHITE: f32 = 1.962;
const ATTEN: f32 = 0.746;
const TAU: f32 = 6.28318530717959;

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

fn in_color_phase(color: i32, phase: i32) -> bool {
    return ((color + phase) % 12) < 6;
}

// Non-negative modulo. WGSL `%` takes the sign of the dividend, so a filter tap
// sampling off the LEFT edge (negative absolute sub-sample position) would give
// a negative `phase`, flipping `in_color_phase` / emphasis decisions and making
// the reconstructed signal inconsistent with the (edge-clamped) texel load — a
// visible edge artifact. Wrapping into [0, n) keeps the subcarrier phase
// continuous across the edge and consistent with the (also-continuous) demod
// reference, so edge pixels reconstruct correctly.
fn pmod(x: i32, n: i32) -> i32 {
    return ((x % n) + n) % n;
}

// Reconstruct one normalized composite sample for source column `col` at
// sub-sample `sub` (0..8), given the packed index texel and the line phase.
fn composite_at(col: i32, sub: i32, row: i32, line_phase: i32) -> f32 {
    let dim = textureDimensions(idx_tex);
    let cx = clamp(col, 0, i32(dim.x) - 1);
    let cy = clamp(row, 0, i32(dim.y) - 1);
    let packed = i32(textureLoad(idx_tex, vec2<i32>(cx, cy), 0).r);
    let index = packed & 0x3F;
    let emphasis = (packed >> 6) & 0x7;

    let color = index & 0x0F;
    var level = 1;
    if (color < 0x0E) {
        level = (index >> 4) & 0x3;
    }
    // Absolute subcarrier phase (8 phase units per pixel + per-line offset),
    // wrapped non-negative so off-left-edge taps (negative `col`/`sub`) don't
    // flip the in-phase/emphasis decisions (see `pmod`).
    let phase = pmod(col * 8 + sub + line_phase, 12);
    let is_high = in_color_phase(color, phase) || color == 0x00;
    var lo_i = level;
    if (color == 0x00) { lo_i = level + 4; }
    var hi_i = level;
    if (color < 0x0D) { hi_i = level + 4; }
    var wave = LEVELS[lo_i];
    if (is_high) { wave = LEVELS[hi_i]; }

    let emph = ((emphasis & 1) != 0 && in_color_phase(0, phase))
        || ((emphasis & 2) != 0 && in_color_phase(4, phase))
        || ((emphasis & 4) != 0 && in_color_phase(8, phase));
    if (emph) { wave = wave * ATTEN; }
    return (wave - BLACK) / (WHITE - BLACK);
}

fn yiq2rgb(y: f32, i: f32, q: f32) -> vec3<f32> {
    let r = y + 0.9563 * i + 0.6210 * q;
    let g = y - 0.2721 * i - 0.6474 * q;
    let b = y - 1.1070 * i + 1.7046 * q;
    return vec3<f32>(r, g, b);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let suv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);
    let dim = textureDimensions(idx_tex);
    let rows = select(240.0, u.params.w, u.params.w >= 1.0);

    let sat = u.params.y;
    let sharp = clamp(u.params.z, 0.0, 1.0);
    let brightness = u.knobs.x;
    let contrast = u.knobs.y;
    let hue = u.knobs.z;

    let row = i32(floor(suv.y * rows));
    // Per-line phase offset gives the 3-phase dot crawl (params.x is the frame
    // videoPhase; the row parity contributes the per-line shift). Wrapped
    // non-negative for the same edge-consistency reason as `composite_at`.
    let line_phase = pmod(i32(round(u.params.x)) + row * 8, 12);

    // Centre position in absolute sub-samples: 8 per pixel.
    let fcol = suv.x * f32(dim.x);
    let centre = fcol * 8.0;

    // Window half-width in sub-samples; sharper => narrower (less bleed).
    let half = i32(round(mix(24.0, 8.0, sharp)));

    // Rotating-oscillator demod. The reference subcarrier phase advances by a
    // CONSTANT `TAU/12` per tap (each tap steps the absolute sub-sample by 1), so
    // rather than a `sin()`/`cos()` per tap (~49 transcendental pairs/pixel — a
    // real fullscreen-pass cost) we evaluate the pair ONCE at the window start
    // and rotate it forward with the angle-addition identity:
    //   cos(p+d) = cos p cos d - sin p sin d
    //   sin(p+d) = sin p cos d + cos p sin d
    // costing 3 transcendentals/pixel total (the start pair + the fixed
    // `cos d`/`sin d`). f32 drift over the ~49-step recurrence is ~1e-5 and this
    // is an opt-in display pass (default-off, never on the deterministic path),
    // so the output stays visually identical to the per-tap form. (Mirrors the
    // Bisqwit pass's avoidance of per-tap transcendentals.)
    let dph = TAU / 12.0;
    let cd = cos(dph);
    let sd = sin(dph);
    let start_ph = TAU * ((centre - f32(half)) / 12.0);
    var cs = cos(start_ph);
    var sn = sin(start_ph);

    var y_acc = 0.0;
    var i_acc = 0.0;
    var q_acc = 0.0;
    var w_sum = 0.0;
    for (var s = -half; s <= half; s = s + 1) {
        let abs_sub = centre + f32(s);
        let col = i32(floor(abs_sub / 8.0));
        let sub = i32(floor(abs_sub)) % 8;
        let comp = composite_at(col, sub, row, line_phase);
        // Triangular low-pass window.
        let w = 1.0 - abs(f32(s)) / (f32(half) + 1.0);
        y_acc = y_acc + comp * w;
        i_acc = i_acc + comp * cs * w * 2.0;
        q_acc = q_acc + comp * sn * w * 2.0;
        w_sum = w_sum + w;
        // Advance the oscillator by one tap (dph) for the next iteration.
        let ncs = cs * cd - sn * sd;
        let nsn = sn * cd + cs * sd;
        cs = ncs;
        sn = nsn;
    }
    let inv = 1.0 / max(w_sum, 1e-4);
    var y = y_acc * inv;
    var i = i_acc * inv * sat;
    var q = q_acc * inv * sat;

    // Hue rotate.
    let ct = cos(hue);
    let st = sin(hue);
    let i2 = i * ct - q * st;
    let q2 = i * st + q * ct;

    // Brightness / contrast on luma.
    y = (y - 0.5) * contrast + 0.5 + (brightness - 1.0);

    let rgb = yiq2rgb(y, i2, q2);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
