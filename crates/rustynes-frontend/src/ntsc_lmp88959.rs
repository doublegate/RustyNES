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
// v1.8.4: the NTSC WGSL is shared with the Android renderer via
// `rustynes-gfx-shaders` (byte-identical to the inline version it replaces).
pub(crate) const SHADER_SRC: &str = rustynes_gfx_shaders::NTSC_LMP_WGSL;

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
