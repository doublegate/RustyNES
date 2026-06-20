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
//! The CRT / scanline post-pass WGSL is SHARED with the desktop frontend (the
//! `rustynes-gfx-shaders` crate), so the on-screen filter look matches across
//! platforms; the `params` uniform selects None / Scanlines / CRT (None = a plain
//! letterboxed blit). The heavier NTSC passes (LMP88959 / Bisqwit, which need the
//! palette-index texture) are a follow-up.

use ndk::native_window::NativeWindow;
use raw_window_handle::{AndroidDisplayHandle, HasWindowHandle, RawDisplayHandle};

/// NES framebuffer dimensions (must match `rustynes_mobile::{FRAME_WIDTH,_HEIGHT}`).
const NES_W: u32 = 256;
const NES_H: u32 = 240;
/// NES pixel aspect ratio (8:7) → the displayed image is wider than 256:240.
const PAR_W: f32 = NES_W as f32 * 8.0 / 7.0;
const IMG_ASPECT: f32 = PAR_W / NES_H as f32;

/// Uniform for the shared CRT/scanline shader (`rustynes_gfx_shaders::CRT_WGSL`):
/// `rect` letterbox (x,y = scale of the surface; z,w = extra centre offset, 0
/// here — the shader centres), `crop` overscan (none), and `params` (x = scanline
/// intensity, y = aperture-mask intensity) selected by the active filter. With
/// `params = (0,0)` the shader is a plain letterboxed blit (the "None" filter).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4],
    crop: [f32; 4],
    // CRT: (scanline, mask, _, _). NTSC: (saturation, sharpness, tint, phase).
    params: [f32; 4],
    // NTSC only: x = PAL mode. Padding for the CRT shader (which reads 12 floats).
    aux: [f32; 4],
}

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
    /// CRT/scanline pipeline (filters None / Scanlines / CRT, selected by `params`).
    pipeline: wgpu::RenderPipeline,
    /// LMP88959 NTSC pipeline (filter NTSC).
    ntsc_pipeline: wgpu::RenderPipeline,
    /// Active video filter: 0 = none, 1 = scanlines, 2 = CRT, 3 = NTSC.
    filter: u8,
    /// The shader `params` for the active filter (meaning is filter-specific:
    /// Scanlines = [scan]; CRT = [scan, mask]; NTSC = [sat, sharp, tint, phase]).
    params: [f32; 4],
    /// Reused framebuffer staging buffer — the JNI copies each frame's bytes in
    /// place (`get_byte_array_region`) instead of allocating a fresh `Vec` per frame.
    frame_buf: Vec<u8>,
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
                    // The CRT shader reads the uniform in BOTH stages (vertex uses
                    // `rect` for the letterbox; fragment uses `crop`/`params`).
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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

        // The CRT/scanline WGSL is shared with the desktop frontend.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crt/blit shader"),
            source: wgpu::ShaderSource::Wgsl(rustynes_gfx_shaders::CRT_WGSL.into()),
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

        // The LMP88959 NTSC pipeline (also shared with the desktop), same bind-group
        // layout (texture + sampler + the 16-float uniform).
        let ntsc_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ntsc shader"),
            source: wgpu::ShaderSource::Wgsl(rustynes_gfx_shaders::NTSC_LMP_WGSL.into()),
        });
        let ntsc_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ntsc pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &ntsc_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ntsc_shader,
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
            ntsc_pipeline,
            filter: 0,
            params: [0.0; 4],
            frame_buf: vec![0u8; (NES_W * NES_H * 4) as usize],
            _window: window,
        };
        gfx.write_uniforms();
        Ok(gfx)
    }

    /// Reconfigure the surface for a new size and recompute the letterbox.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.write_uniforms();
    }

    /// Set the active video filter (0 = none, 1 = scanlines, 2 = CRT, 3 = NTSC) and
    /// its shader `params` (filter-specific — see the field doc; supplied by the
    /// Settings sliders), then rewrite the uniform.
    pub fn set_filter(&mut self, filter: u8, params: [f32; 4]) {
        self.filter = filter;
        self.params = params;
        self.write_uniforms();
    }

    fn write_uniforms(&self) {
        let sw = self.config.width as f32;
        let sh = self.config.height as f32;
        let screen_aspect = sw / sh;
        let (sx, sy) = if screen_aspect > IMG_ASPECT {
            (IMG_ASPECT / screen_aspect, 1.0) // pillarbox
        } else {
            (1.0, screen_aspect / IMG_ASPECT) // letterbox
        };
        // The shader `params` come straight from the caller (the per-filter sliders);
        // `aux.x` is NTSC's PAL flag (off). None just leaves params at (0,0,0,0).
        let u = Uniforms {
            rect: [sx, sy, 0.0, 0.0],
            crop: [1.0, 0.0, 1.0, 0.0],
            params: self.params,
            aux: [0.0, 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&u));
    }

    /// Upload one 256×240 RGBA8 frame and present it. `fb` must be
    /// `NES_W*NES_H*4` bytes; tolerates a transient `Lost`/`Outdated` surface by
    /// reconfiguring and skipping the frame.
    /// The reused frame staging buffer; the JNI copies the Java `byte[]` into this
    /// (`get_byte_array_region`) so no `Vec` is allocated per frame. Length is fixed
    /// at `NES_W*NES_H*4`.
    pub fn frame_buf_mut(&mut self) -> &mut [u8] {
        &mut self.frame_buf
    }

    pub fn render(&mut self) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.nes_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.frame_buf,
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
            let pipeline = if self.filter == 3 {
                &self.ntsc_pipeline
            } else {
                &self.pipeline
            };
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}
