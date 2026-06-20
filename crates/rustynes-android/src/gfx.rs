//! Android wgpu render path (v1.8.4, Workstream B).
//!
//! A self-contained wgpu blit that draws the 256×240 RGBA `NES` framebuffer onto
//! the `SurfaceView`'s `ANativeWindow` with **8:7-PAR letterboxing**, off the UI
//! thread. This is the native, GPU-accelerated replacement for the Compose
//! `Bitmap` blit on API ≥ 33; the `Bitmap` + AGSL path stays as the fallback.
//!
//! Presentation only — no emulation happens here, so the determinism contract is
//! untouched. The surface can be lost + recreated (rotate / background / fold)
//! while the core keeps running headless; `render` tolerates `Lost`/`Outdated` by
//! reconfiguring.
//!
//! The richer desktop shader stack (LMP88959 NTSC / CRT / hqNx-xBRZ) is reused in
//! a follow-up increment by lifting those WGSL passes into a shared, winit-free
//! core; this first pass establishes the SurfaceView → wgpu pipeline with the
//! base PAR/overscan blit.

use ndk::native_window::NativeWindow;
use raw_window_handle::{AndroidDisplayHandle, HasWindowHandle, RawDisplayHandle};

/// NES framebuffer dimensions (must match `rustynes_mobile::{FRAME_WIDTH,_HEIGHT}`).
const NES_W: u32 = 256;
const NES_H: u32 = 240;
/// NES pixel aspect ratio (8:7) → the displayed image is wider than 256:240.
const PAR_W: f32 = NES_W as f32 * 8.0 / 7.0;
const IMG_ASPECT: f32 = PAR_W / NES_H as f32;

/// Letterbox transform handed to the shader: the image occupies `scale` of the
/// surface, centered at `offset` (both in [0,1] screen space).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    scale: [f32; 2],
    offset: [f32; 2],
}

const SHADER_SRC: &str = r#"
struct Uniforms { scale: vec2<f32>, offset: vec2<f32> };
@group(0) @binding(0) var nes_tex: texture_2d<f32>;
@group(0) @binding(1) var nes_smp: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let xy = p[vi];
    var o: VsOut;
    o.pos = vec4<f32>(xy, 0.0, 1.0);
    // Screen UV in [0,1] with y pointing down.
    o.uv = vec2<f32>((xy.x + 1.0) * 0.5, (1.0 - xy.y) * 0.5);
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let t = (i.uv - u.offset) / u.scale;
    if (t.x < 0.0 || t.x > 1.0 || t.y < 0.0 || t.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return textureSample(nes_tex, nes_smp, t);
}
"#;

/// Owns the wgpu device/surface/pipeline + the `NES` texture for one
/// `SurfaceView`. Declared field order keeps `surface` dropping before
/// `_window`, so the wgpu surface releases before its `ANativeWindow` is freed.
pub struct AndroidGfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    nes_texture: wgpu::Texture,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    _window: NativeWindow,
}

impl AndroidGfx {
    /// Build the renderer for `window` at the surface size `width`×`height`.
    pub fn new(window: NativeWindow, width: u32, height: u32) -> Result<Self, String> {
        pollster::block_on(Self::new_async(window, width, height))
    }

    async fn new_async(window: NativeWindow, width: u32, height: u32) -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        // SAFETY: `window` is a live `ANativeWindow` obtained from the
        // `SurfaceHolder`'s `Surface`; it is stored in `self` and outlives the
        // surface (field drop order), so the handle stays valid for the surface's
        // whole lifetime.
        let raw_window = window
            .window_handle()
            .map_err(|e| format!("window handle: {e}"))?
            .as_raw();
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
            raw_window_handle: raw_window,
        };
        let surface = unsafe { instance.create_surface_unsafe(target) }
            .map_err(|e| format!("create_surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("request_adapter: {e}"))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rustynes-android device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            })
            .await
            .map_err(|e| format!("request_device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        // Match the NES texture format to the surface format so the sRGB round-trip
        // is identity (the framebuffer is already in the final colour space).
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let nes_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nes framebuffer"),
            size: wgpu::Extent3d {
                width: NES_W,
                height: NES_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let nes_view = nes_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nes sampler (nearest)"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("letterbox uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit bgl"),
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit bind group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&nes_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buf.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit pipeline"),
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
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let gfx = Self {
            surface,
            device,
            queue,
            config,
            nes_texture,
            uniform_buf,
            bind_group,
            pipeline,
            _window: window,
        };
        gfx.write_letterbox();
        Ok(gfx)
    }

    /// Reconfigure the surface for a new size and recompute the letterbox.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.write_letterbox();
    }

    fn write_letterbox(&self) {
        let sw = self.config.width as f32;
        let sh = self.config.height as f32;
        let screen_aspect = sw / sh;
        let (sx, sy) = if screen_aspect > IMG_ASPECT {
            (IMG_ASPECT / screen_aspect, 1.0) // pillarbox
        } else {
            (1.0, screen_aspect / IMG_ASPECT) // letterbox
        };
        let u = Uniforms {
            scale: [sx, sy],
            offset: [(1.0 - sx) * 0.5, (1.0 - sy) * 0.5],
        };
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
    }

    /// Upload one 256×240 RGBA8 frame and present it. `fb` must be
    /// `NES_W*NES_H*4` bytes; tolerates a transient `Lost`/`Outdated` surface by
    /// reconfiguring and skipping the frame.
    pub fn render(&mut self, fb: &[u8]) {
        if fb.len() != (NES_W * NES_H * 4) as usize {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.nes_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            fb,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(NES_W * 4),
                rows_per_image: Some(NES_H),
            },
            wgpu::Extent3d {
                width: NES_W,
                height: NES_H,
                depth_or_array_layers: 1,
            },
        );

        // This wgpu returns the `CurrentSurfaceTexture` enum, not a `Result`.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("blit encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}
