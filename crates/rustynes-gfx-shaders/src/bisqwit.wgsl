
struct Uniforms {
    rect: vec4<f32>,   // letterbox transform (same shape + math as gfx.wgsl)
    crop: vec4<f32>,   // overscan crop: x=v-scale, y=v-offset, z=u-scale, w=u-offset
    params: vec4<f32>, // x = videoPhase (0..2), rest reserved
    knobs: vec4<f32>,  // x = contrast, y = saturation, z = brightness, w = hue (degrees)
};

@group(0) @binding(0) var idx_tex: texture_2d<u32>;
@group(0) @binding(1) var<uniform> u: Uniforms;

// Baked static tables (see ntsc_bisqwit.rs). `var<private>` so the fragment
// shader can dynamically index them (naga forbids dynamic indexing of const/let
// value arrays; WebGL2 has no storage buffers).
var<private> SIGNAL_LOW: array<i32, 128> = array<i32, 128>(38, -11, -11, -11, -11, -11, -11, -11, -11, -11, -11, -11, -11, -11, 0, 0, 67, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 100, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 0, 0, 100, 72, 72, 72, 72, 72, 72, 72, 72, 72, 72, 72, 72, 72, 0, 0, 23, -16, -16, -16, -16, -16, -16, -16, -16, -16, -16, -16, -16, -16, 0, 0, 46, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, -8, 0, 0, 74, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 17, 0, 0, 74, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 50, 0, 0);
var<private> SIGNAL_HIGH: array<i32, 128> = array<i32, 128>(38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, -11, 0, 0, 67, 67, 67, 67, 67, 67, 67, 67, 67, 67, 67, 67, 67, 0, 0, 0, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 30, 0, 0, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 72, 0, 0, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, -16, 0, 0, 46, 46, 46, 46, 46, 46, 46, 46, 46, 46, 46, 46, 46, -8, 0, 0, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 17, 0, 0, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 74, 50, 0, 0);
var<private> SINE: array<i32, 27> = array<i32, 27>(0, 3, 6, 8, 6, 3, 0, -3, -6, -8, -6, -4, 0, 4, 6, 8, 6, 3, 0, -3, -6, -8, -6, -4, 0, 4, 6);
var<private> EMPHASIS: array<i32, 8> = array<i32, 8>(0, 63, 1008, 1023, 3843, 3903, 4083, 4095);

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
const FW: i32 = 12; // filter width (y=i=q=12)

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
    // Fullscreen triangle; letterbox in UV space (not by scaling position) so
    // the bars clip to black in fs (no ClampToEdge edge-smear).
    var out: VsOut;
    out.pos = vec4<f32>(pos[vid], 0.0, 1.0);
    out.uv = (uv[vid] - vec2<f32>(0.5, 0.5) - vec2<f32>(u.rect.z, u.rect.w))
        / vec2<f32>(u.rect.x, u.rect.y) + vec2<f32>(0.5, 0.5);
    return out;
}

// Reconstruct one composite signal sample at absolute line position `t`
// (t = pixel_x*8 + sub-sample j), for the given source `row` and `videoPhase`.
fn signal_sample(t: i32, row: i32, video_phase: i32) -> i32 {
    if (t < 0 || t >= 256 * 8) {
        return 0;
    }
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
    if (pos > 12) {
        pos = pos - 12;
    }
    let high = (pos % 12) < 6; // pos==12 -> 0 -> high

    var color = pixel_color;
    if (emphasis != 0 && pos < 12) {
        let lut = u32(EMPHASIS[emphasis]);
        let r = u32(hue % 12);
        let wave = ((lut >> r) | (lut << (12u - r))) & 0xFFFFu;
        if (((wave >> u32(pos)) & 1u) != 0u) {
            color = color | 0x40;
        }
    }

    if (high) {
        return SIGNAL_HIGH[color];
    }
    return SIGNAL_LOW[color];
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Letterbox bars -> black.
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
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
    for (var d = -(FW / 2) + 1; d <= FW / 2; d = d + 1) {
        let t = center + d;
        let s = signal_sample(t, row, video_phase);
        // Cos(t) = SINE[(t+36)%12 + phase0]; Sin(t) = SINE[... + 3 + phase0].
        let m12 = ((t + 36) % 12 + 12) % 12;
        let cs = SINE[m12 + phase0];
        let sn = SINE[m12 + 3 + phase0];
        ysum = ysum + s;
        isum = isum + s * cs;
        qsum = qsum + s * sn;
    }

    // Hue rotation of the (I, Q) vector by `hue` degrees (identity at 0). Done in
    // f32 then truncated back to integer so the matrix multiply below is unchanged.
    // Gated on `hue != 0` so the default (hue==0, byte-identical) path skips the
    // per-fragment cos()/sin() trig entirely. `hue` is a uniform, so this branch
    // is coherent across the workgroup (cheap), and the hue==0 result is identical
    // to multiplying by the identity rotation (no float round-trip applied).
    if u.knobs.w != 0.0 {
        let hue_rad = u.knobs.w * 0.017453292; // pi/180
        let hc = cos(hue_rad);
        let hs = sin(hue_rad);
        let isum_r = i32(f32(isum) * hc - f32(qsum) * hs);
        let qsum_r = i32(f32(isum) * hs + f32(qsum) * hc);
        isum = isum_r;
        qsum = qsum_r;
    }

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
}
