#![allow(clippy::doc_markdown)]

//! Constrained RetroArch `.slangp` / `.cgp` preset importer (v1.6.0 "Studio" I3).
//!
//! ADR 0013 originally **rejected** a full `.slangp` importer: a complete
//! GLSL/Slang -> WGSL translation layer is a large, fragile surface with poor
//! payoff for a built-in-shader emulator. v1.6.0 revisits that with a deliberately
//! *constrained* scope:
//!
//! 1. **Parse** the preset (the RetroArch `shaderN = path` / `parameters = …` /
//!    `<param> = <value>` key/value format used by both `.slangp` and `.cgp`).
//! 2. **Map** each referenced pass onto a built-in [`crate::shader_pass`] pass by
//!    recognizing the *well-known shader filename stems* (e.g. a `crt-*` pass ->
//!    the built-in CRT pass, an `ntsc`/`composite` pass -> the LMP88959 pass, an
//!    `hqx`/`hq2x` pass -> the hqNx pass, an `xbr`/`xbrz` pass -> the xBRZ pass)
//!    and carry over any matching parameter overrides.
//! 3. **Honestly reject** anything it can't translate: a pass whose filename is
//!    not recognized becomes an [`ImportedPass::Unsupported`] entry (it is NOT
//!    silently dropped, and it does NOT produce a broken stack). The caller shows
//!    the unsupported list to the user.
//!
//! This is **not** a GLSL->WGSL transpiler. It does not read or compile the
//! referenced shader files; it recognizes the *intent* of common community
//! presets by name and re-expresses them with RustyNES's curated built-in WGSL
//! passes. That is the honest, tractable subset — full source translation
//! remains out of scope (as ADR 0013 §"Out of scope" states), and the
//! [`ImportResult::unsupported`] count makes the limit visible.
//!
//! Pure data transform (no GPU, no I/O beyond the caller handing us the file
//! text), so it is fully unit-testable and wasm-safe.

use crate::shader_pass::{ShaderPassDesc, ShaderStackConfig};

/// One pass resolved from a preset entry.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportedPass {
    /// Successfully mapped to a built-in pass (ready to push into a stack).
    Mapped(ShaderPassDesc),
    /// Recognized as a pass entry, but no built-in equivalent — reported, not
    /// dropped. Carries the original shader path for the UI message.
    Unsupported {
        /// The `shaderN` path from the preset (verbatim).
        path: String,
        /// A short human reason (e.g. "no built-in equivalent").
        reason: String,
    },
}

/// The result of importing a preset: the translatable passes (as a ready stack)
/// plus the honest list of passes that could not be translated.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImportResult {
    /// The stack assembled from the mapped passes (may be empty if nothing
    /// translated). `composite`-style passes are placed appropriately — the LMP
    /// pass is RGBA so ordering is preserved as-authored.
    pub stack: ShaderStackConfig,
    /// Passes that were recognized as entries but had no built-in equivalent.
    pub unsupported: Vec<ImportedPass>,
}

impl ImportResult {
    /// `true` when at least one preset pass mapped to a built-in.
    #[must_use]
    pub const fn any_mapped(&self) -> bool {
        !self.stack.passes.is_empty()
    }

    /// Number of preset passes that could not be translated.
    #[must_use]
    pub const fn unsupported_count(&self) -> usize {
        self.unsupported.len()
    }
}

/// A parsed key/value line from a preset (INI-ish, `key = value`, `#` comments,
/// optional surrounding quotes on the value).
fn parse_kv(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return None;
    }
    let (k, v) = line.split_once('=')?;
    let v = v.trim().trim_matches('"').trim();
    Some((k.trim().to_ascii_lowercase(), v.to_string()))
}

/// The lowercase filename stem of a shader path (`shaders/crt/crt-geom.slang`
/// -> `crt-geom`).
fn shader_stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file.split('.').next().unwrap_or(file);
    stem.to_ascii_lowercase()
}

/// The outcome of classifying a preset pass by its filename stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StemMap {
    /// Maps onto the named built-in pass.
    Builtin(&'static str),
    /// A pure pass-through stage (`stock` / `passthrough` / `pixellate`): it
    /// contributes nothing visual, so it is silently **skipped** — not reported
    /// as unsupported (there is nothing missing to report).
    Skip,
    /// No built-in equivalent and not a pass-through: recorded as
    /// [`ImportedPass::Unsupported`] so the limit is visible.
    Unsupported,
}

/// Classify a shader filename stem, recognizing the common community-preset
/// naming conventions.
fn map_stem_to_builtin(stem: &str) -> StemMap {
    // Order matters: check the more specific tokens first.
    if stem.contains("xbrz") || stem.contains("xbr") {
        StemMap::Builtin("xbrz")
    } else if stem.contains("hqx") || stem.contains("hq2x") || stem.contains("hq4x") {
        StemMap::Builtin("hqx")
    } else if stem.contains("ntsc") || stem.contains("composite") || stem.contains("lmp") {
        // RGBA composite -> the LMP88959 pass (NOT the index-only Bisqwit pass,
        // which can't slot mid-stack).
        StemMap::Builtin("lmp88959")
    } else if stem.contains("crt")
        || stem.contains("scanline")
        || stem.contains("aperture")
        || stem.contains("geom")
    {
        StemMap::Builtin("crt")
    } else if stem == "stock" || stem == "passthrough" || stem.contains("pixellate") {
        // A pure pass-through stage: skip it (it adds nothing, and it is not a
        // capability we're missing, so it must NOT count as unsupported).
        StemMap::Skip
    } else {
        StemMap::Unsupported
    }
}

/// Translate a preset's recognized parameter overrides onto the built-in pass's
/// own knob names. Only a small, hand-picked set of common RetroArch parameter
/// names are carried over; unknown ones are ignored (the built-in default
/// applies).
fn carry_params(builtin_id: &str, params: &[(String, f32)], desc: &mut ShaderPassDesc) {
    for (k, v) in params {
        let k = k.as_str();
        match builtin_id {
            "crt" => match k {
                "scanline" | "scanline_weight" | "scanlinestrength" | "spike" => {
                    desc.params
                        .insert("scanline".to_string(), v.clamp(0.0, 1.0));
                }
                "mask" | "maskstrength" | "dotmask" | "shadowmask" => {
                    desc.params.insert("mask".to_string(), v.clamp(0.0, 0.5));
                }
                _ => {}
            },
            "lmp88959" => match k {
                "saturation" | "ntsc_sat" | "cs" => {
                    desc.params
                        .insert("saturation".to_string(), v.clamp(0.0, 2.0));
                }
                "sharpness" | "ntsc_sharp" | "sharp" => {
                    desc.params
                        .insert("sharpness".to_string(), v.clamp(0.0, 1.0));
                }
                "tint" | "hue" => {
                    desc.params.insert("tint".to_string(), v.clamp(-0.5, 0.5));
                }
                "pal" => {
                    desc.params
                        .insert("pal".to_string(), if *v > 0.5 { 1.0 } else { 0.0 });
                }
                _ => {}
            },
            "hqx" | "xbrz" if k == "strength" || k == "blend" || k.contains("weight") => {
                desc.params
                    .insert("strength".to_string(), v.clamp(0.0, 1.0));
            }
            _ => {}
        }
    }
}

/// Import a RetroArch `.slangp` / `.cgp` preset from its text contents.
///
/// The format is shared between the two extensions for the keys we read
/// (`shaders = N`, `shaderN = path`, and a flat list of `param = value`
/// overrides). Returns the translatable [`ShaderStackConfig`] plus the honest
/// list of [`ImportedPass::Unsupported`] passes.
///
/// # Errors
///
/// Returns `Err` with a human message when the preset declares no `shaderN`
/// entries at all (i.e. it is not a recognizable RetroArch preset).
pub fn import_preset(text: &str) -> Result<ImportResult, String> {
    let mut shader_paths: Vec<(usize, String)> = Vec::new();
    let mut params: Vec<(String, f32)> = Vec::new();

    for line in text.lines() {
        let Some((k, v)) = parse_kv(line) else {
            continue;
        };
        if let Some(rest) = k.strip_prefix("shader") {
            // `shader0`, `shader1`, … = path. (`shaders = N` has no digit suffix
            // beyond the count; skip the bare `shaders` count key.)
            if let Ok(idx) = rest.parse::<usize>() {
                shader_paths.push((idx, v));
                continue;
            }
        }
        // Any other `key = number` is treated as a candidate parameter override.
        if let Ok(num) = v.parse::<f32>() {
            params.push((k, num));
        }
    }

    if shader_paths.is_empty() {
        return Err(
            "no `shaderN = …` entries found — not a recognizable RetroArch preset".to_string(),
        );
    }
    shader_paths.sort_by_key(|(i, _)| *i);

    let mut result = ImportResult::default();
    for (_, path) in shader_paths {
        let stem = shader_stem(&path);
        match map_stem_to_builtin(&stem) {
            StemMap::Builtin(id) => {
                let mut desc = ShaderPassDesc::new(id);
                carry_params(id, &params, &mut desc);
                result.stack.passes.push(desc);
            }
            // Pure pass-through: contributes nothing and is not a missing
            // capability, so drop it silently (not counted as unsupported).
            StemMap::Skip => {}
            StemMap::Unsupported => {
                result.unsupported.push(ImportedPass::Unsupported {
                    path,
                    reason: "no built-in equivalent (source translation is out of scope)"
                        .to_string(),
                });
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_crt_preset() {
        let text = "\
            shaders = 1\n\
            shader0 = shaders/crt/crt-geom.slang\n\
            scanline = 0.6\n\
            mask = 0.2\n";
        let r = import_preset(text).unwrap();
        assert_eq!(r.stack.passes.len(), 1);
        assert_eq!(r.stack.passes[0].id, "crt");
        assert!((r.stack.passes[0].params["scanline"] - 0.6).abs() < 1e-6);
        assert!((r.stack.passes[0].params["mask"] - 0.2).abs() < 1e-6);
        assert_eq!(r.unsupported_count(), 0);
        assert!(r.any_mapped());
    }

    #[test]
    fn maps_multipass_ntsc_then_crt() {
        let text = "\
            shaders = 2\n\
            shader0 = shaders/ntsc/ntsc-composite.slang\n\
            shader1 = shaders/crt/crt-easymode.slang\n\
            saturation = 1.3\n";
        let r = import_preset(text).unwrap();
        assert_eq!(r.stack.passes.len(), 2);
        assert_eq!(r.stack.passes[0].id, "lmp88959");
        assert_eq!(r.stack.passes[1].id, "crt");
        // Order from the preset is preserved.
        assert!((r.stack.passes[0].params["saturation"] - 1.3).abs() < 1e-6);
    }

    #[test]
    fn maps_upscalers() {
        let text = "shader0 = hqx/hq4x.slang\nshader1 = xbr/xbrz-freescale.slang\n";
        let r = import_preset(text).unwrap();
        assert_eq!(r.stack.passes[0].id, "hqx");
        assert_eq!(r.stack.passes[1].id, "xbrz");
    }

    #[test]
    fn reports_unsupported_honestly() {
        let text = "\
            shaders = 2\n\
            shader0 = shaders/crt/crt-royale.slang\n\
            shader1 = shaders/anti-aliasing/advanced-aa.slang\n";
        let r = import_preset(text).unwrap();
        // crt-royale maps (crt token); advanced-aa has no equivalent.
        assert_eq!(r.stack.passes.len(), 1);
        assert_eq!(r.stack.passes[0].id, "crt");
        assert_eq!(r.unsupported_count(), 1);
        match &r.unsupported[0] {
            ImportedPass::Unsupported { path, .. } => {
                assert!(path.contains("advanced-aa"));
            }
            ImportedPass::Mapped(_) => panic!("expected unsupported"),
        }
    }

    #[test]
    fn cgp_format_is_same_keys() {
        // `.cgp` uses the same shaderN keys; quotes around the value are stripped.
        let text = "shaders = 1\nshader0 = \"crt-aperture.cg\"\n";
        let r = import_preset(text).unwrap();
        assert_eq!(r.stack.passes[0].id, "crt");
    }

    #[test]
    fn empty_preset_is_an_error() {
        assert!(import_preset("# just a comment\nfoo = 1\n").is_err());
    }

    #[test]
    fn stock_passthrough_is_skipped_not_errored() {
        let text = "shader0 = stock.slang\nshader1 = crt-geom.slang\n";
        let r = import_preset(text).unwrap();
        // stock is a pass-through: skipped entirely (NOT counted as unsupported),
        // and crt maps. This is the contract `map_stem_to_builtin` promises.
        assert_eq!(r.stack.passes.len(), 1);
        assert_eq!(r.stack.passes[0].id, "crt");
        assert_eq!(
            r.unsupported_count(),
            0,
            "a pure pass-through must skip, not report as unsupported"
        );
    }

    #[test]
    fn passthrough_and_pixellate_skip_without_unsupported() {
        let text = "shader0 = passthrough.slang\nshader1 = pixellate.slang\n";
        let r = import_preset(text).unwrap();
        // Both are pass-through stages: nothing mapped, nothing unsupported.
        assert_eq!(r.stack.passes.len(), 0);
        assert_eq!(r.unsupported_count(), 0);
        assert!(!r.any_mapped());
    }
}
