#![allow(clippy::too_many_arguments, clippy::doc_markdown)]

//! Composable post-process shader stack (v1.2.0 C2, GeraNES `ShaderPass`-inspired).
//!
//! This module turns the frontend's single-select post-process filter (the
//! mutually-exclusive CRT / NTSC / composite-rt chain in [`crate::gfx`]) into a
//! *composable* stack: an ordered list of enabled passes, each rendered by
//! ping-ponging between two NES-resolution intermediate render targets, with the
//! final pass blitting to the swapchain.
//!
//! ## The load-bearing invariant
//!
//! An **empty (or all-disabled) stack falls through to the existing direct blit**
//! — pixel-identical to the pre-C2 output. The ping-pong render-target path
//! engages only when [`ShaderStackConfig::has_enabled_passes`] is `true`. The default
//! config deserializes to an empty stack (`#[serde(default)]`), so a stock build
//! / stock config is byte-for-byte unchanged. See [`crate::gfx::Gfx`].
//!
//! ## Special case: the Bisqwit composite-rt pass
//!
//! The true-composite NES_NTSC pass ([`crate::ntsc_bisqwit`]) consumes the
//! `R16Uint` palette-index texture, not the RGBA framebuffer. It therefore can
//! only be the **first** pass in the stack (it has no RGBA input). The stack
//! enforces this: a `composite-rt` pass anywhere other than position 0 is
//! ignored. Because that filter is already wired (with its live `NtscKnobs`)
//! through the legacy [`crate::gfx::Gfx`] path, the stack treats it as a marker
//! that defers to the existing wiring rather than re-implementing it here.
//!
//! ## Tunable parameters
//!
//! Each built-in shader declares its knobs with RetroArch-style header lines:
//!
//! ```text
//! // #pragma parameter <name> "<label>" <default> <min> <max> <step>
//! ```
//!
//! [`parse_pragma_parameters`] parses them (mirroring GeraNES'
//! `parseShaderParameters` in `ShaderWindowUI.inl`) to drive generic egui
//! sliders, and the per-pass parameter overrides persist in the config.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One tunable parameter declared by a shader via a `#pragma parameter` header.
///
/// Mirrors GeraNES' `ShaderPass::Parameter` (RetroArch's parameter convention):
/// `#pragma parameter <name> "<label>" <default> <min> <max> <step>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderParam {
    /// Uniform/identifier name (the persistence key).
    pub name: String,
    /// Human-readable slider label.
    pub label: String,
    /// Default value.
    pub default: f32,
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
    /// Slider step (informational; egui sliders are continuous).
    pub step: f32,
}

/// Parse the `#pragma parameter` declarations out of a shader source string.
///
/// Recognizes lines of the form (the leading `//` is optional, matching how the
/// built-in WGSL embeds them as comments so the WGSL still validates):
///
/// ```text
/// #pragma parameter NAME "Label text" 1.0 0.0 2.0 0.05
/// ```
///
/// `min`/`max`/`step` are optional and default to `0.0` / `1.0` / `0.01` when
/// absent. Malformed lines are skipped. This mirrors GeraNES'
/// `parseShaderParameters` (`GeraNESApp.ShaderWindowUI.inl`).
#[must_use]
pub fn parse_pragma_parameters(src: &str) -> Vec<ShaderParam> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let line = raw.trim().trim_start_matches("//").trim();
        let Some(rest) = line.strip_prefix("#pragma parameter") else {
            continue;
        };
        let rest = rest.trim();
        // name is the first whitespace-delimited token.
        let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if name_end == 0 {
            continue;
        }
        let name = rest[..name_end].to_string();
        let after_name = rest[name_end..].trim_start();
        // label is a "double-quoted" string (fall back to the next token).
        let (label, after_label) = if let Some(stripped) = after_name.strip_prefix('"') {
            match stripped.find('"') {
                Some(close) => (
                    stripped[..close].to_string(),
                    stripped[close + 1..].trim_start(),
                ),
                None => continue,
            }
        } else {
            let end = after_name
                .find(char::is_whitespace)
                .unwrap_or(after_name.len());
            (
                after_name[..end].to_string(),
                after_name[end..].trim_start(),
            )
        };
        // Remaining numeric tokens: default [min [max [step]]].
        let nums: Vec<f32> = after_label
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .collect();
        let default = nums.first().copied().unwrap_or(0.0);
        let min = nums.get(1).copied().unwrap_or(0.0);
        let max = nums.get(2).copied().unwrap_or(1.0);
        let step = nums.get(3).copied().unwrap_or(0.01);
        let label = if label.is_empty() {
            name_label_fallback(&name)
        } else {
            label
        };
        out.push(ShaderParam {
            name,
            label,
            default,
            min,
            max,
            step,
        });
    }
    out
}

/// Title-case-ish fallback label when a `#pragma parameter` omits the quoted text.
fn name_label_fallback(name: &str) -> String {
    name.replace('_', " ")
}

/// A built-in shader pass kind. The id strings are the stable config keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinPass {
    /// CRT / scanline pass (the [`crate::crt`] shader, parameterized).
    Crt,
    /// Simplified Blargg-style NTSC blur (the [`crate::ntsc`] shader).
    Ntsc,
    /// True-composite NES_NTSC (Bisqwit). Special-cased: must be first; the
    /// legacy [`crate::gfx::Gfx`] wiring renders it (with its live knobs).
    CompositeRt,
}

impl BuiltinPass {
    /// Resolve a config id string to a built-in pass kind.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "crt" => Some(Self::Crt),
            "ntsc" => Some(Self::Ntsc),
            "composite-rt" => Some(Self::CompositeRt),
            _ => None,
        }
    }

    /// The stable config id for this pass.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Crt => "crt",
            Self::Ntsc => "ntsc",
            Self::CompositeRt => "composite-rt",
        }
    }

    /// Human-readable label for the stack editor.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Crt => "CRT / scanlines",
            Self::Ntsc => "NTSC (simplified blur)",
            Self::CompositeRt => "NTSC composite (Bisqwit)",
        }
    }

    /// True when this pass samples the `R16Uint` palette-index texture rather
    /// than the RGBA framebuffer (so it must be the first pass).
    #[must_use]
    pub const fn is_index_source(self) -> bool {
        matches!(self, Self::CompositeRt)
    }

    /// The ordered list of built-in passes a user can add to the stack.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[Self::Crt, Self::Ntsc, Self::CompositeRt]
    }

    /// The `#pragma parameter` declarations this pass exposes (parsed from the
    /// pass's WGSL, so the source is the single source of truth).
    #[must_use]
    pub fn params(self) -> Vec<ShaderParam> {
        match self {
            Self::Crt => parse_pragma_parameters(crate::crt::stack_shader_src()),
            // The simplified NTSC blur and the Bisqwit composite-rt pass expose
            // no stack-tunable knobs here (the latter keeps its own dedicated
            // NtscKnobs UI in the legacy path).
            Self::Ntsc | Self::CompositeRt => Vec::new(),
        }
    }
}

/// One pass in the persisted shader stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderPassDesc {
    /// The built-in pass id (see [`BuiltinPass::id`]). Unknown ids are tolerated
    /// (and ignored at render time) so a config from a newer build still loads.
    pub id: String,
    /// Whether this pass is enabled. A disabled pass is skipped (it does not
    /// occupy a ping-pong slot), so a stack of all-disabled passes is equivalent
    /// to an empty stack — the byte-identical fall-through still applies.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Per-parameter overrides (keyed by `#pragma parameter` name). Missing keys
    /// use the shader's declared default. `BTreeMap` for stable serialization.
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
}

const fn default_enabled() -> bool {
    true
}

impl ShaderPassDesc {
    /// Construct an enabled pass with default parameters.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            enabled: true,
            params: BTreeMap::new(),
        }
    }

    /// Resolve the built-in pass kind, if the id is recognized.
    #[must_use]
    pub fn builtin(&self) -> Option<BuiltinPass> {
        BuiltinPass::from_id(&self.id)
    }

    /// The effective value of parameter `name`: the override if present (clamped
    /// to the declared range), else the shader's declared default.
    #[must_use]
    pub fn param_value(&self, p: &ShaderParam) -> f32 {
        self.params
            .get(&p.name)
            .copied()
            .map_or(p.default, |v| v.clamp(p.min, p.max))
    }
}

/// The persisted composable shader stack (`[graphics] shader_stack`).
///
/// `#[serde(default)]` on the field in `GraphicsConfig` plus the empty default
/// here mean a pre-C2 config (no `shader_stack` key) deserializes to an empty
/// stack — i.e. the byte-identical direct-blit path.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShaderStackConfig {
    /// Ordered passes (top to bottom = first to last applied).
    #[serde(default)]
    pub passes: Vec<ShaderPassDesc>,
}

impl ShaderStackConfig {
    /// `true` when at least one pass is enabled (so the ping-pong path engages).
    /// An empty stack, or a stack whose passes are all disabled, returns `false`
    /// and the caller MUST take the unchanged direct-blit path.
    #[must_use]
    pub fn has_enabled_passes(&self) -> bool {
        self.passes
            .iter()
            .any(|p| p.enabled && p.builtin().is_some())
    }

    /// The enabled, recognized passes in render order, with the
    /// `composite-rt`-must-be-first rule enforced (a composite-rt pass at any
    /// position other than 0 is dropped).
    #[must_use]
    pub fn effective_passes(&self) -> Vec<&ShaderPassDesc> {
        self.passes
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                if !p.enabled {
                    return false;
                }
                match p.builtin() {
                    None => false,
                    Some(b) if b.is_index_source() => *i == 0,
                    Some(_) => true,
                }
            })
            .map(|(_, p)| p)
            .collect()
    }
}

/// A named, persisted bank of saved stacks (`[graphics] shader_presets`).
///
/// `BTreeMap` for a deterministic on-disk ordering. Empty by default, so a
/// pre-C2 config is byte-identical.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ShaderPresetBank {
    /// preset name -> stack.
    #[serde(default)]
    pub presets: BTreeMap<String, ShaderStackConfig>,
}

impl ShaderPresetBank {
    /// The built-in CRT preset bank, merged into a user bank on first run (only
    /// when the user has no preset of the same name — never clobbering a user
    /// edit). Each reuses the existing [`crate::crt`] shader at varying knobs.
    #[must_use]
    pub fn builtins() -> Vec<(String, ShaderStackConfig)> {
        let crt = |scanline: f32, mask: f32| {
            let mut params = BTreeMap::new();
            params.insert("scanline".to_string(), scanline);
            params.insert("mask".to_string(), mask);
            ShaderStackConfig {
                passes: vec![ShaderPassDesc {
                    id: "crt".to_string(),
                    enabled: true,
                    params,
                }],
            }
        };
        vec![
            ("CRT - Sharp".to_string(), crt(0.25, 0.05)),
            ("CRT - Classic".to_string(), crt(0.5, 0.10)),
            ("CRT - Heavy Aperture".to_string(), crt(0.8, 0.25)),
        ]
    }
}

// =============================================================================
// Runtime: the ping-pong render-target executor.
// =============================================================================

use wgpu::util::DeviceExt;

use crate::gfx::{NES_H, NES_W};

/// One compiled pass in the live stack (pipeline + uniform + bind group).
struct CompiledPass {
    kind: BuiltinPass,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Resolved parameter values, in [`BuiltinPass::params`] declaration order.
    param_values: Vec<f32>,
    /// `true` for the final pass (blits to the swapchain with letterbox); the
    /// rest render NES-res -> NES-res with an identity transform.
    is_final: bool,
}

/// The live, compiled shader stack.
///
/// Owns the two ping-pong intermediate RTs and the per-pass pipelines. Rebuilt
/// from a [`ShaderStackConfig`] whenever the config changes (cheap — only a few
/// passes). When the stack has no enabled passes this is never constructed and
/// [`crate::gfx::Gfx`] takes the unchanged direct-blit path.
pub struct ShaderStack {
    /// Two NES-resolution RGBA intermediates for ping-ponging. Kept alive so the
    /// derived `target_views` (and the bind groups referencing them) stay valid.
    _targets: [wgpu::Texture; 2],
    target_views: [wgpu::TextureView; 2],
    /// The compiled passes in render order.
    passes: Vec<CompiledPass>,
    /// Linear sampler shared by every RGBA-sampling pass. Kept alive for the
    /// bind groups that reference it.
    _sampler: wgpu::Sampler,
    /// `true` when the first pass samples the `R16Uint` palette-index texture
    /// (a leading composite-rt pass), so the caller must supply the index FB.
    needs_index: bool,
}

impl ShaderStack {
    /// Build the live stack from `cfg` (which MUST satisfy
    /// [`ShaderStackConfig::has_enabled_passes`]). Returns `None` when no pass
    /// could be compiled (e.g. every id was unknown), so the caller falls back
    /// to the direct blit.
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        nes_texture: &wgpu::Texture,
        index_texture: &wgpu::Texture,
        cfg: &ShaderStackConfig,
    ) -> Option<Self> {
        let effective = cfg.effective_passes();
        if effective.is_empty() {
            return None;
        }

        // Intermediate RTs share the NES texture's format so the sRGB decode /
        // encode round-trips to identity (same rule the direct blit relies on).
        let rt_format = nes_texture.format();
        let make_rt = || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shader-stack-rt"),
                size: wgpu::Extent3d {
                    width: NES_W,
                    height: NES_H,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: rt_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        };
        let targets = [make_rt(), make_rt()];
        let target_views = [
            targets[0].create_view(&wgpu::TextureViewDescriptor::default()),
            targets[1].create_view(&wgpu::TextureViewDescriptor::default()),
        ];

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader-stack-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let nes_view = nes_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let index_view = index_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let last = effective.len() - 1;
        let mut passes = Vec::with_capacity(effective.len());
        for (i, desc) in effective.iter().enumerate() {
            let kind = desc
                .builtin()
                .expect("effective_passes filters to builtins");
            let is_final = i == last;
            // Input for pass i: pass 0 reads the source (nes/index texture);
            // later passes ping-pong the intermediate written by pass i-1.
            let input_index_src = kind.is_index_source();
            let rgba_input_view: &wgpu::TextureView = if i == 0 {
                &nes_view
            } else {
                // Pass i-1 wrote into target[(i-1) % 2].
                &target_views[(i - 1) % 2]
            };
            let params = kind.params();
            let param_values: Vec<f32> = params.iter().map(|p| desc.param_value(p)).collect();
            let compiled = compile_pass(
                device,
                kind,
                if is_final { surface_format } else { rt_format },
                if input_index_src {
                    &index_view
                } else {
                    rgba_input_view
                },
                &sampler,
                input_index_src,
                &param_values,
                is_final,
            );
            passes.push(compiled);
        }

        let needs_index = effective
            .first()
            .and_then(|d| d.builtin())
            .is_some_and(BuiltinPass::is_index_source);

        Some(Self {
            _targets: targets,
            target_views,
            passes,
            _sampler: sampler,
            needs_index,
        })
    }

    /// Whether the first pass samples the palette-index texture (composite-rt).
    #[must_use]
    pub const fn needs_index_source(&self) -> bool {
        self.needs_index
    }

    /// Render every pass, ping-ponging the intermediates; the final pass blits
    /// to `out_view` (the swapchain) with the letterbox + overscan crop. The
    /// `video_phase` + knobs are forwarded to a leading composite-rt pass.
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        par_correction: bool,
        overscan: crate::config::Overscan,
        video_phase: u8,
        ntsc_knobs: crate::ntsc_bisqwit::NtscKnobs,
    ) {
        // Identity transform for the NES-res -> NES-res intermediate passes (full
        // [0,1] UV, no crop). Only the FINAL pass letterboxes to the swapchain.
        // crop = (scale_v=1, offset_v=0, scale_u=1, offset_u=0) leaves UV intact.
        const IDENTITY: [f32; 8] = [1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let lb = crate::gfx::letterbox_uniform(width, height, par_correction, overscan);

        for (i, pass) in self.passes.iter().enumerate() {
            let transform = if pass.is_final { lb } else { IDENTITY };
            // Build the 16-float uniform: rect(4) + crop(4) + params(4) + knobs(4).
            let mut u = [0.0f32; 16];
            u[..8].copy_from_slice(&transform);
            match pass.kind {
                BuiltinPass::Crt => {
                    // params.x = scanline, params.y = mask (declaration order).
                    u[8] = pass.param_values.first().copied().unwrap_or(0.5);
                    u[9] = pass.param_values.get(1).copied().unwrap_or(0.1);
                }
                BuiltinPass::CompositeRt => {
                    // params.x = videoPhase; knobs = contrast/sat/bright/hue.
                    u[8] = f32::from(video_phase);
                    u[12] = ntsc_knobs.contrast;
                    u[13] = ntsc_knobs.saturation;
                    u[14] = ntsc_knobs.brightness;
                    u[15] = ntsc_knobs.hue;
                }
                BuiltinPass::Ntsc => {}
            }
            queue.write_buffer(&pass.uniforms, 0, bytemuck::cast_slice(&u));

            let target: &wgpu::TextureView = if pass.is_final {
                out_view
            } else {
                &self.target_views[i % 2]
            };
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shader-stack-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&pass.pipeline);
            rp.set_bind_group(0, &pass.bind_group, &[]);
            rp.draw(0..3, 0..1);
        }
    }
}

/// Compile one stack pass. RGBA passes (CRT / NTSC) sample a float texture with
/// a filtering sampler; the index pass (composite-rt) samples the `R16Uint`
/// texture with no sampler (textureLoad), mirroring `NtscBisqwitFilter`.
#[allow(clippy::too_many_lines)]
fn compile_pass(
    device: &wgpu::Device,
    kind: BuiltinPass,
    target_format: wgpu::TextureFormat,
    input_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    index_source: bool,
    param_values: &[f32],
    is_final: bool,
) -> CompiledPass {
    // The fragment WGSL for this pass. CRT / NTSC reuse their existing bodies;
    // composite-rt reuses the Bisqwit generator. The uniform buffer is always
    // 16 floats (rect+crop+params+knobs); each shader declares only the prefix
    // it needs, and a larger backing buffer is valid for a smaller binding.
    let src: std::borrow::Cow<'static, str> = match kind {
        BuiltinPass::Crt => crate::crt::SHADER_SRC.into(),
        BuiltinPass::Ntsc => crate::ntsc::SHADER_SRC.into(),
        BuiltinPass::CompositeRt => crate::ntsc_bisqwit::shader_src().into(),
    };
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shader-stack-shader"),
        source: wgpu::ShaderSource::Wgsl(src),
    });

    // Bind-group layout: index passes have (idx_tex, uniform); RGBA passes have
    // (tex, sampler, uniform) — matching each shader's declared bindings.
    let bgl = if index_source {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader-stack-bgl-index"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        })
    } else {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shader-stack-bgl-rgba"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        })
    };

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shader-stack-pipeline-layout"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shader-stack-pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    // 16-float uniform backing buffer (rect+crop+params+knobs).
    let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("shader-stack-uniforms"),
        contents: bytemuck::cast_slice(&[0.0f32; 16]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = if index_source {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader-stack-bg-index"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        })
    } else {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shader-stack-bg-rgba"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        })
    };

    CompiledPass {
        kind,
        pipeline,
        uniforms,
        bind_group,
        param_values: param_values.to_vec(),
        is_final,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_retroarch_pragma_parameter() {
        let src = "// #pragma parameter SCANLINE \"Scanline Strength\" 0.5 0.0 1.0 0.05\n\
                   #pragma parameter MASK \"Mask\" 0.1 0.0 0.5 0.01\n";
        let params = parse_pragma_parameters(src);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "SCANLINE");
        assert_eq!(params[0].label, "Scanline Strength");
        assert!((params[0].default - 0.5).abs() < 1e-6);
        assert!((params[0].min - 0.0).abs() < 1e-6);
        assert!((params[0].max - 1.0).abs() < 1e-6);
        assert!((params[0].step - 0.05).abs() < 1e-6);
        assert_eq!(params[1].name, "MASK");
    }

    #[test]
    fn unquoted_label_and_missing_numbers_default() {
        let params = parse_pragma_parameters("#pragma parameter gain 1.0\n");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "gain");
        // unquoted label -> the second token is the label; "1.0" is consumed as
        // the label, leaving no numbers -> all numeric defaults.
        assert_eq!(params[0].label, "1.0");
        assert!((params[0].min - 0.0).abs() < 1e-6);
        assert!((params[0].max - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ignores_non_pragma_lines() {
        assert!(parse_pragma_parameters("let x = 1.0;\n// a comment\n").is_empty());
    }

    #[test]
    fn empty_stack_takes_direct_blit_path() {
        // The load-bearing invariant at the config level.
        let cfg = ShaderStackConfig::default();
        assert!(!cfg.has_enabled_passes());
        assert!(cfg.effective_passes().is_empty());
    }

    #[test]
    fn all_disabled_stack_is_equivalent_to_empty() {
        let cfg = ShaderStackConfig {
            passes: vec![ShaderPassDesc {
                id: "crt".into(),
                enabled: false,
                params: BTreeMap::new(),
            }],
        };
        assert!(!cfg.has_enabled_passes());
    }

    #[test]
    fn composite_rt_only_allowed_first() {
        let cfg = ShaderStackConfig {
            passes: vec![
                ShaderPassDesc::new("crt"),
                ShaderPassDesc::new("composite-rt"),
            ],
        };
        // composite-rt is not first -> dropped; only crt survives.
        let eff = cfg.effective_passes();
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].id, "crt");

        let cfg2 = ShaderStackConfig {
            passes: vec![
                ShaderPassDesc::new("composite-rt"),
                ShaderPassDesc::new("crt"),
            ],
        };
        let eff2 = cfg2.effective_passes();
        assert_eq!(eff2.len(), 2);
        assert_eq!(eff2[0].id, "composite-rt");
    }

    #[test]
    fn unknown_pass_ids_are_ignored() {
        let cfg = ShaderStackConfig {
            passes: vec![ShaderPassDesc::new("from-a-newer-build")],
        };
        assert!(!cfg.has_enabled_passes());
        assert!(cfg.effective_passes().is_empty());
    }

    #[test]
    fn config_round_trips_through_toml() {
        let cfg = ShaderStackConfig {
            passes: vec![{
                let mut p = ShaderPassDesc::new("crt");
                p.params.insert("scanline".into(), 0.7);
                p
            }],
        };
        let s = toml::to_string(&cfg).unwrap();
        let back: ShaderStackConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn crt_pass_exposes_parsed_params() {
        // The CRT pass advertises its #pragma parameter knobs (parsed from WGSL).
        let params = BuiltinPass::Crt.params();
        assert!(params.iter().any(|p| p.name == "scanline"));
        assert!(params.iter().any(|p| p.name == "mask"));
    }

    #[test]
    fn builtin_presets_are_crt_stacks() {
        let presets = ShaderPresetBank::builtins();
        assert!(!presets.is_empty());
        for (_, stack) in presets {
            assert!(stack.has_enabled_passes());
            assert_eq!(stack.passes[0].id, "crt");
        }
    }
}
