//! wgpu surface + texture-blit pipeline for the NES framebuffer.
//!
//! The PPU emits a 256x240 RGBA8 framebuffer. Each frame the frontend
//! uploads it to a wgpu texture; a fullscreen-triangle render pass samples
//! that texture with nearest filtering and aspect-ratio-correct letterbox.
//!
//! [`Gfx`] also owns the optional post-process chain layered on that base
//! blit: the single-select `crt` / `ntsc` / `ntsc_bisqwit` filters and the
//! composable `shader_pass::ShaderStack` (CRT / NTSC / upscalers in any order).
//! With none of them active the direct nearest-blit is taken and the output is
//! pixel-identical to a filter-less build. Present-mode selection (fifo /
//! mailbox / immediate, with a non-silent fifo fallback) lives in
//! `select_present_mode`.
//!
//! See `docs/frontend.md` for the render-path architecture.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use winit::window::Window;

/// NES native resolution.
pub const NES_W: u32 = 256;
/// NES native resolution.
pub const NES_H: u32 = 240;

/// Resolve the configured present-mode string against the surface's
/// supported modes.
///
/// Recognized values (case-insensitive): `"fifo"` (vsync; the safe
/// default), `"mailbox"` (triple-buffered, no tearing, no vsync gate),
/// `"immediate"` (uncapped, may tear). When the requested mode is not in
/// `supported`, falls back to `Fifo`, which every wgpu backend is
/// guaranteed to support.
///
/// The native frontend's wall-clock pacer (`App::pace_frames`) is the
/// authoritative timing source; selecting `Mailbox` avoids the
/// double-pacing beat between the NTSC 60.098 Hz pacer and a 60 Hz
/// display's `Fifo` vsync.
/// Returns the effective mode plus whether the request had to fall back.
///
/// The fallback is NOT silent (v2.8.0 Phase 0): on Wayland/GL stacks that
/// lack `Mailbox`, falling back to `Fifo` re-introduces the double-pacing
/// beat the user explicitly configured away — the caller logs it and the
/// settings panel shows a warning so the symptom (periodic ~10 s hitch) is
/// attributable.
fn select_present_mode(pref: &str, supported: &[wgpu::PresentMode]) -> (wgpu::PresentMode, bool) {
    let requested = match pref.to_ascii_lowercase().as_str() {
        "mailbox" => wgpu::PresentMode::Mailbox,
        "immediate" => wgpu::PresentMode::Immediate,
        _ => wgpu::PresentMode::Fifo,
    };
    if supported.contains(&requested) {
        (requested, false)
    } else {
        (
            wgpu::PresentMode::Fifo,
            requested != wgpu::PresentMode::Fifo,
        )
    }
}

/// Errors during graphics init.
#[derive(Debug, thiserror::Error)]
pub enum GfxError {
    /// Failed to create a wgpu surface for the given window.
    #[error("create surface: {0}")]
    Surface(String),
    /// Failed to acquire a wgpu adapter (no compatible GPU).
    #[error("no compatible wgpu adapter")]
    NoAdapter,
    /// Failed to acquire a wgpu device.
    #[error("request device: {0}")]
    Device(String),
}

/// Outcome of trying to acquire + present a swapchain frame.
///
/// wgpu 29 replaced the `Result<SurfaceTexture, SurfaceError>` returned by
/// `get_current_texture` with the [`wgpu::CurrentSurfaceTexture`] enum. This
/// preserves the two behaviours the frontend cares about: `Reconfigure`
/// (surface lost / outdated — the caller resizes to rebuild the swapchain)
/// versus a skip-this-frame status the caller just logs.
#[derive(Debug, thiserror::Error)]
pub enum PresentError {
    /// The surface was lost or outdated; the caller should reconfigure
    /// (resize) the surface and try again next frame.
    #[error("surface lost/outdated; reconfiguring")]
    Reconfigure,
    /// A transient acquisition status (timeout / occluded / validation); the
    /// frame is skipped. Carries a static label for logging.
    #[error("surface unavailable: {0}")]
    Other(&'static str),
}

const SHADER_SRC: &str = r"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Letterbox transform pushed via a tiny uniform buffer.
struct Uniforms {
    // x,y = scale (0..1 of clip space); z,w = offset.
    rect: vec4<f32>,
    // v1.0.0 overscan crop: x = vertical scale, y = vertical offset (both in
    // texture-V space). Default (1.0, 0.0) samples the full framebuffer; when
    // overscan is hidden, (224/240, 8/240) crops the top + bottom 8 scanlines.
    // v1.5.0 D2: z = horizontal scale, w = horizontal offset (texture-U space),
    // for per-side overscan. Default (1.0, 0.0) leaves the U sample unchanged.
    crop: vec4<f32>,
};

@group(0) @binding(0) var nes_tex: texture_2d<f32>;
@group(0) @binding(1) var nes_smp: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Fullscreen triangle covering [-1,1]^2 with [0,1]^2 UVs.
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>( 0.0,  2.0),
        vec2<f32>( 0.0,  0.0),
        vec2<f32>( 2.0,  0.0),
    );
    var out: VsOut;
    // The triangle always covers the WHOLE surface (NO position scaling); the
    // letterbox is applied in UV space and the out-of-image bars are clipped to
    // black in the fragment shader. Scaling the oversized triangle's POSITION
    // instead (the previous approach) leaves its far vertex covering the bottom
    // (and, in fullscreen, the right) bar, which then samples clamped edge
    // texels -> the garbage-at-the-bottom / fullscreen edge-smear.
    // rect.xy = the image's fraction of the surface (<=1); rect.zw = offset.
    out.pos = vec4<f32>(pos[vid], 0.0, 1.0);
    out.uv = (uv[vid] - vec2<f32>(0.5, 0.5) - vec2<f32>(u.rect.z, u.rect.w))
        / vec2<f32>(u.rect.x, u.rect.y) + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Outside the letterboxed image is a black bar (the pass clears to black,
    // but the fullscreen triangle also rasterizes the bars, so clip here so a
    // ClampToEdge sampler can't smear the edge texels across them).
    if (in.uv.x < 0.0 || in.uv.x > 1.0 || in.uv.y < 0.0 || in.uv.y > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // v1.0.0 overscan crop: remap the visible V range onto the inner texture
    // rows. Default crop (1.0, 0.0) leaves the sample point unchanged.
    // v1.5.0 D2: the same remap on U (crop.zw) for per-side horizontal crop.
    var sample_uv = vec2<f32>(in.uv.x * u.crop.z + u.crop.w, in.uv.y * u.crop.x + u.crop.y);
    return textureSample(nes_tex, nes_smp, sample_uv);
}
";

/// v2.8.0 Phase 0 (`gpu-timing` feature) — whole-encoder GPU pass timer.
///
/// Two timestamps bracket the frame's command encoder (NES blit / NTSC +
/// egui overlay); the delta, scaled by the queue's timestamp period, is the
/// GPU cost of one presented frame. Readback is asynchronous through a
/// 3-deep ring of mappable buffers so the render loop never blocks on the
/// GPU; the most recent resolved value is published through an atomic the
/// Performance panel reads (typically 1-3 frames stale — fine for
/// attribution).
#[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
struct GpuTimer {
    query_set: wgpu::QuerySet,
    resolve_buf: wgpu::Buffer,
    // `Arc` because each readback buffer is moved into a `'static` `map_async`
    // callback that outlives the frame; wgpu's `Buffer` is not `Clone`.
    read_bufs: Vec<Arc<wgpu::Buffer>>,
    /// Which `read_bufs` slot this frame copies into.
    cursor: usize,
    /// Slots with an outstanding `map_async` (cleared by the map callback,
    /// so a slot is only ever remapped after its previous read completed).
    in_flight: Vec<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Nanoseconds per timestamp tick (`Queue::get_timestamp_period`).
    period_ns: f32,
    /// Latest measured GPU pass time, f32 ms as bits (lock-free publish
    /// from the map callback).
    last_ms_bits: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
impl GpuTimer {
    const RING: usize = 3;

    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("nes-gpu-timer"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nes-gpu-timer-resolve"),
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read_bufs = (0..Self::RING)
            .map(|i| {
                Arc::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(&format!("nes-gpu-timer-read-{i}")),
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }))
            })
            .collect();
        Self {
            query_set,
            resolve_buf,
            read_bufs,
            cursor: 0,
            in_flight: (0..Self::RING)
                .map(|_| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))
                .collect(),
            period_ns: queue.get_timestamp_period(),
            last_ms_bits: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    /// Bracket-start: write timestamp 0 into the encoder.
    fn begin(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.write_timestamp(&self.query_set, 0);
    }

    /// Bracket-end: write timestamp 1, resolve both into the resolve buffer
    /// and copy into this frame's readback slot (skipped while that slot's
    /// previous map is still in flight).
    fn end(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.write_timestamp(&self.query_set, 1);
        if self.in_flight[self.cursor].load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        encoder.resolve_query_set(&self.query_set, 0..2, &self.resolve_buf, 0);
        encoder.copy_buffer_to_buffer(&self.resolve_buf, 0, &self.read_bufs[self.cursor], 0, 16);
    }

    /// After submit: kick the async map for this frame's slot and advance.
    /// The callback (fired during wgpu's queue maintenance on subsequent
    /// frames) publishes the measured ms and releases the slot.
    fn after_submit(&mut self) {
        let slot = self.cursor;
        if !self.in_flight[slot].load(std::sync::atomic::Ordering::Acquire) {
            let buf = self.read_bufs[slot].clone();
            let bits = std::sync::Arc::clone(&self.last_ms_bits);
            let flag = std::sync::Arc::clone(&self.in_flight[slot]);
            let period = self.period_ns;
            flag.store(true, std::sync::atomic::Ordering::Release);
            let buf_for_cb = buf.clone();
            buf.slice(..).map_async(wgpu::MapMode::Read, move |res| {
                if res.is_ok() {
                    let data = buf_for_cb.slice(..).get_mapped_range();
                    let t0 = u64::from_le_bytes(data[0..8].try_into().expect("8 bytes"));
                    let t1 = u64::from_le_bytes(data[8..16].try_into().expect("8 bytes"));
                    drop(data);
                    buf_for_cb.unmap();
                    #[allow(clippy::cast_precision_loss)] // sub-frame deltas fit f32.
                    let ms = (t1.saturating_sub(t0)) as f32 * period / 1_000_000.0;
                    bits.store(ms.to_bits(), std::sync::atomic::Ordering::Relaxed);
                }
                // Release the slot only after the read completed (or failed)
                // so it is never remapped while still mapped.
                flag.store(false, std::sync::atomic::Ordering::Release);
            });
        }
        self.cursor = (self.cursor + 1) % Self::RING;
    }

    fn last_ms(&self) -> Option<f32> {
        let bits = self.last_ms_bits.load(std::sync::atomic::Ordering::Relaxed);
        (bits != 0).then(|| f32::from_bits(bits))
    }
}

/// Owns the wgpu surface, device, queue, NES texture, render pipeline.
pub struct Gfx {
    /// Reference-counted handle to the underlying winit window. Held to
    /// keep the surface valid for as long as we render.
    pub window: Arc<Window>,
    /// Surface configuration (width + height, current present mode, ...).
    surface: wgpu::Surface<'static>,
    /// Wgpu device — pub so the debugger overlay + ntsc filter can share
    /// resources with the same `&Device`.
    pub device: wgpu::Device,
    /// Wgpu queue.
    pub queue: wgpu::Queue,
    /// Current surface configuration. Pub so the frontend can read
    /// `width`/`height`/`format`.
    pub config: wgpu::SurfaceConfiguration,
    nes_texture: wgpu::Texture,
    /// v1.1.0 beta.1 (T-110-A1) — `R16Uint` palette-index source for the true
    /// composite `NES_NTSC` filter; uploaded only while `ntsc_bisqwit` is active.
    index_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    /// Optional NTSC filter (T-53-008) — when present, the PPU framebuffer
    /// is first composited through this filter, then the letterbox blit
    /// samples the filter's output texture.
    ntsc: Option<crate::ntsc::NtscFilter>,
    /// v1.1.0 beta.1 (T-110-A1) — optional true composite `NES_NTSC` filter
    /// (Bisqwit algorithm). Samples `index_texture`. Render priority sits below
    /// CRT and above the simplified `ntsc` blur.
    ntsc_bisqwit: Option<crate::ntsc_bisqwit::NtscBisqwitFilter>,
    /// v1.1.0 beta.1 — optional CRT / scanline post-pass. Mutually exclusive with
    /// `ntsc` at render time (CRT takes priority when both are set).
    crt: Option<crate::crt::CrtFilter>,
    /// v1.2.0 C2 — optional composable shader stack (ping-pong RT executor). When
    /// `Some`, it OWNS the post-process path and the legacy single-select
    /// `crt`/`ntsc`/`ntsc_bisqwit` chain below is bypassed. `None` (the default,
    /// for an empty config stack) keeps the EXACT pre-C2 chain, so the default
    /// presented image is byte-identical. Built/cleared via
    /// [`Self::apply_shader_stack`].
    shader_stack: Option<crate::shader_pass::ShaderStack>,
    /// v1.2.0 C2 — the live NTSC knobs forwarded to a leading composite-rt stack
    /// pass (kept in sync with the legacy `ntsc_bisqwit` path's knobs).
    stack_ntsc_knobs: crate::ntsc_bisqwit::NtscKnobs,
    /// Whether the configured present mode was unsupported and the surface
    /// silently runs `Fifo` instead (surfaced in the settings panel so the
    /// resulting pacer-vs-vsync beat is attributable).
    present_mode_fell_back: bool,
    /// Present modes the surface supports (captured at init), so the
    /// pacing-matrix mode switches can validate live reconfigurations.
    supported_present_modes: Vec<wgpu::PresentMode>,
    /// v2.8.0 Phase 0 (`gpu-timing`) — whole-encoder GPU pass timer. `None`
    /// when the adapter lacks `TIMESTAMP_QUERY`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
    gpu_timer: Option<GpuTimer>,
    /// v1.0.0 — apply the NES's native 8:7 pixel aspect ratio when `true`
    /// (so the 256x240 framebuffer is stretched to a 4:3-ish display shape);
    /// `false` keeps the 1:1 (square-pixel) 256:240 aspect. Drives the
    /// letterbox transform.
    par_correction: bool,
    /// v1.0.0 — crop the top + bottom 8 NES scanlines (CRT overscan) when
    /// `true`. Drives the overscan `crop` half of the blit uniform; default
    /// `false` = the full 256x240 framebuffer (byte-identical presentation).
    ///
    /// v1.5.0 D2 — this legacy toggle is folded into [`Self::overscan`] (it is
    /// equivalent to an 8 px top + 8 px bottom crop) by the `effective_overscan`
    /// helper; the field is retained so the existing `set_hide_overscan` API +
    /// callers keep working.
    hide_overscan: bool,
    /// v1.5.0 "Lens" Workstream D2 — per-side overscan crop (in NES pixels). All
    /// zero (default) = the full framebuffer; combined with [`Self::hide_overscan`]
    /// by the `effective_overscan` helper. Presentation-only; byte-identical at
    /// zero.
    overscan: crate::config::Overscan,
    /// v1.2.0 beta.2 (Workstream C3) — optional HD-pack blit resources. `None`
    /// (the default, and the only state when no pack is loaded) means the
    /// presentation path is byte-identical to the stock build. When `Some`, the
    /// HD compositor's upscaled RGBA buffer is uploaded to `texture` and blitted
    /// (letterboxed) through the same nearest-sampling pipeline as the NES
    /// framebuffer. Lazily (re)built when the HD buffer dimensions change.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    hd: Option<HdBlit>,
}

/// v1.2.0 C3 — the HD-pack blit texture + its bind group, sized to the
/// compositor's `scale*256 x scale*240` output.
#[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
struct HdBlit {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    width: u32,
    height: u32,
}

impl Gfx {
    /// Initialize wgpu against `window`.
    ///
    /// v1.3.0 Sprint 1.4 — this is now `async`. The adapter/device
    /// requests are awaited rather than `pollster::block_on`'d so the
    /// same code path works on wasm32 (where blocking the browser
    /// event loop is impossible). Native callers wrap this in
    /// `pollster::block_on(Gfx::new(window))`; the wasm32 path drives
    /// it via `wasm_bindgen_futures::spawn_local` and delivers the
    /// resulting `Gfx` back to the winit `App` through an
    /// `EventLoopProxy<Gfx>` user event (see `app.rs`).
    #[allow(clippy::too_many_lines)] // wgpu init is naturally verbose; splitting hurts readability.
    pub async fn new(
        window: Arc<Window>,
        present_mode_pref: &str,
        max_frame_latency: u32,
        par_correction: bool,
        hide_overscan: bool,
    ) -> Result<Self, GfxError> {
        let size = window.inner_size();

        // On wasm32 request WebGPU primary with a WebGL2 fallback (the
        // latter needs the `webgl` cargo feature on wgpu). On native,
        // the default backend set (Vulkan/Metal/DX12/GL) is right.
        #[cfg(target_arch = "wasm32")]
        let instance = {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
            wgpu::Instance::new(desc)
        };
        #[cfg(not(target_arch = "wasm32"))]
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| GfxError::Surface(e.to_string()))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| GfxError::NoAdapter)?;

        // wasm32 (esp. WebGL2) needs downlevel_webgl2_defaults so the
        // requested limits don't exceed what the backend exposes.
        #[cfg(target_arch = "wasm32")]
        let required_limits =
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        #[cfg(not(target_arch = "wasm32"))]
        let required_limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());

        // v2.8.0 Phase 0 — opt-in GPU pass timing (`gpu-timing` feature):
        // request the timestamp features the adapter offers so the render
        // encoder can be bracketed with timestamps. `GpuTimer` writes its
        // timestamps via `CommandEncoder::write_timestamp`, which needs
        // BOTH `TIMESTAMP_QUERY` *and* `TIMESTAMP_QUERY_INSIDE_ENCODERS` —
        // requesting only the former (as before) made wgpu validation abort
        // at the first `write_timestamp` on any device that didn't already
        // have the inside-encoders feature enabled. We request whichever of
        // the two the adapter supports and only arm the timer below when both
        // were actually granted. Default builds request no extra features
        // (byte-for-byte unchanged device).
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
        let required_features = adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "gpu-timing")))]
        let required_features = wgpu::Features::empty();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rustynes-device"),
                required_features,
                required_limits,
                memory_hints: wgpu::MemoryHints::Performance,
                // wgpu 25 moved the API-trace path into the descriptor.
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            })
            .await
            .map_err(|e| GfxError::Device(e.to_string()))?;

        let surface_caps = surface.get_capabilities(&adapter);

        // Color-space handling differs by backend. On native (Vulkan /
        // Metal / DX12) we render through an sRGB surface and store the
        // PPU framebuffer in an sRGB texture: the sampler's sRGB->linear
        // decode and the surface's linear->sRGB encode round-trip to
        // identity, so the NES palette bytes reach the screen untouched.
        //
        // On the WebGL2 backend (wgpu `Backend::Gl`, the GitHub-Pages
        // fallback when WebGPU is absent) that round-trip is NOT identity.
        // wgpu-hal's GL surface cannot present to a real sRGB default
        // framebuffer, so when the surface format `is_srgb()` it renders
        // into an intermediate `SRGB8_ALPHA8` texture and runs an EXTRA
        // explicit `linear_to_srgb` encode at present time
        // (`wgpu-hal-22.0.0/src/gles/web.rs::present` +
        // `gles/shaders/srgb_present.frag`). Combined with GL's automatic
        // sRGB framebuffer encoding on the intermediate write, the encode
        // count no longer matches the decode count and the palette comes
        // out wrong (washed out / too dark). The canvas-2D embed path
        // (`wasm.rs`) has correct colors precisely because it does ZERO
        // conversion — it `put_image_data`s the raw RGBA8 bytes, which are
        // already in the display (sRGB) domain.
        //
        // Fix: on the GL backend, keep EVERYTHING in the UNORM domain
        // (non-sRGB surface + non-sRGB NES texture). With a plain
        // pass-through blit shader (no manual color math) this performs
        // zero color conversion anywhere, so the PPU bytes reach the
        // canvas untouched — byte-for-byte the same result the canvas-2D
        // path produces. Native is unaffected (it never hits this branch),
        // so the 60-ROM oracle + sacred trio stay pixel-identical.
        let is_gl_backend = adapter.get_info().backend == wgpu::Backend::Gl;
        let format = if is_gl_backend {
            // Prefer a non-sRGB UNORM format; fall back to the first
            // advertised format if (unexpectedly) none is non-sRGB.
            surface_caps
                .formats
                .iter()
                .copied()
                .find(|f| !f.is_srgb())
                .unwrap_or(surface_caps.formats[0])
        } else {
            surface_caps
                .formats
                .iter()
                .copied()
                .find(wgpu::TextureFormat::is_srgb)
                .unwrap_or(surface_caps.formats[0])
        };
        // The NES framebuffer texture's sRGB-ness MUST match the surface's
        // so the sample-decode / write-encode pair round-trips to identity
        // (sRGB pair on native, UNORM pair on WebGL2). A mismatch would
        // leave a single uncompensated conversion and tint the whole image.
        let nes_texture_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        // Pick the present mode from the user config, validating it against
        // what the surface actually supports. The native frontend paces
        // frames on a wall clock (NTSC 60.098 Hz) in `App::pace_frames`;
        // when the surface ALSO vsync-gates with `Fifo` on a 60.000 Hz
        // display the two clocks beat against each other and drop/double
        // one frame every ~10 s — visible as periodic stutter. Honoring a
        // non-`Fifo` mode (`Mailbox` / `Immediate`) lets the wall-clock
        // pacer be the single source of timing truth and removes the beat.
        // `Fifo` is guaranteed supported by every backend, so it is the
        // safe fallback when the requested mode is unavailable.
        let (present_mode, present_mode_fell_back) =
            select_present_mode(present_mode_pref, &surface_caps.present_modes);
        if present_mode_fell_back {
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!(
                "rustynes: requested present mode \"{present_mode_pref}\" is not \
                 supported by this surface (backend {:?}); falling back to Fifo (vsync). \
                 The wall-clock pacer and the display's vsync will beat against each \
                 other — expect a periodic hitch every ~10 s on a 60 Hz panel.",
                adapter.get_info().backend
            );
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // v2.8.0 Phase 2 — configurable swapchain depth (`[graphics]
            // max_frame_latency`): 1 = lowest display latency, 2 = slack.
            desired_maximum_frame_latency: max_frame_latency.clamp(1, 2),
        };
        surface.configure(&device, &config);

        // NES framebuffer texture.
        let nes_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nes-fb"),
            size: wgpu::Extent3d {
                width: NES_W,
                height: NES_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: nes_texture_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let nes_view = nes_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // v1.1.0 beta.1 (T-110-A1) — the palette-index source for the true
        // composite `NES_NTSC` filter. `R16Uint` (one (emph<<6)|colour value per
        // pixel); read via textureLoad in the Bisqwit shader. Only uploaded when
        // that filter is active, so it costs nothing otherwise.
        let index_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nes-index-fb"),
            size: wgpu::Extent3d {
                width: NES_W,
                height: NES_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nes-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Uniforms (letterbox transform + overscan crop).
        let initial = letterbox_uniform(
            size.width,
            size.height,
            par_correction,
            effective_overscan(hide_overscan, crate::config::Overscan::default()),
        );
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("nes-letterbox"),
            contents: bytemuck::cast_slice(&initial),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nes-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    // v1.0.0 — the fragment shader now also reads the uniform
                    // (the overscan crop), so it is visible to both stages.
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
            label: Some("nes-bg"),
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
                    resource: uniforms.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nes-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nes-pipeline-layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nes-pipeline"),
            layout: Some(&pipeline_layout),
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

        // v2.8.0 Phase 0 — arm the GPU pass timer only when the feature is on
        // and the device actually got BOTH timestamp features the timer's
        // encoder-level `write_timestamp` calls require. If the adapter offers
        // `TIMESTAMP_QUERY` but not `TIMESTAMP_QUERY_INSIDE_ENCODERS`, the timer
        // stays disarmed (gpu_ms reads "-") instead of aborting at submit.
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
        let gpu_timer = device
            .features()
            .contains(
                wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            )
            .then(|| GpuTimer::new(&device, &queue));

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            nes_texture,
            index_texture,
            bind_group,
            uniforms,
            pipeline,
            ntsc: None,
            ntsc_bisqwit: None,
            crt: None,
            shader_stack: None,
            stack_ntsc_knobs: crate::ntsc_bisqwit::NtscKnobs::DEFAULT,
            present_mode_fell_back,
            supported_present_modes: surface_caps.present_modes,
            #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
            gpu_timer,
            par_correction,
            hide_overscan,
            overscan: crate::config::Overscan::default(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd: None,
        })
    }

    /// v2.8.0 Phase 2 — live present-mode switch for the pacing matrix
    /// (e.g. entering display-sync needs `Fifo`; leaving it restores the
    /// configured preference). Returns `false` (and leaves the surface
    /// unchanged) when the mode is unsupported.
    pub fn set_present_mode(&mut self, mode: wgpu::PresentMode) -> bool {
        if !self.supported_present_modes.contains(&mode) {
            return false;
        }
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.surface.configure(&self.device, &self.config);
        }
        true
    }

    /// Resolve + apply the config preference string (`"Mailbox"` / `"Fifo"`
    /// / `"Immediate"`), with the usual Fifo fallback. Used when the pacing
    /// matrix leaves a mode that had forced `Fifo`.
    pub fn apply_present_mode_pref(&mut self, pref: &str) {
        let (mode, _fell_back) = select_present_mode(pref, &self.supported_present_modes);
        let _ = self.set_present_mode(mode);
    }

    /// Return the surface format selected at init time.
    #[must_use]
    pub const fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// The present mode the surface is actually running (may differ from the
    /// configured preference — see [`Self::present_mode_fell_back`]).
    #[must_use]
    pub const fn effective_present_mode(&self) -> wgpu::PresentMode {
        self.config.present_mode
    }

    /// True when the configured present mode was unsupported by the surface
    /// and the swapchain runs `Fifo` (vsync) instead. Surfaced in the
    /// settings panel: in this state the wall-clock pacer and the display's
    /// vsync double-gate and beat against each other.
    #[must_use]
    pub const fn present_mode_fell_back(&self) -> bool {
        self.present_mode_fell_back
    }

    /// Enable the NTSC filter as a wgsl post-pass. The first PPU
    /// framebuffer of the next frame will route through the filter.
    pub fn enable_ntsc(&mut self) {
        if self.ntsc.is_none() {
            self.ntsc = Some(crate::ntsc::NtscFilter::new(
                &self.device,
                self.config.format,
                &self.nes_texture,
            ));
        }
    }

    /// Disable the NTSC filter (skip the post-pass).
    #[allow(dead_code)]
    pub fn disable_ntsc(&mut self) {
        self.ntsc = None;
    }

    /// Enable the true composite `NES_NTSC` filter (Bisqwit algorithm, T-110-A1).
    /// Samples the `R16Uint` index texture; the caller must supply the index
    /// framebuffer + phase to [`Self::render_with_overlay`] while it is active.
    pub fn enable_ntsc_bisqwit(&mut self) {
        if self.ntsc_bisqwit.is_none() {
            self.ntsc_bisqwit = Some(crate::ntsc_bisqwit::NtscBisqwitFilter::new(
                &self.device,
                self.config.format,
                &self.index_texture,
            ));
        }
    }

    /// Disable the true composite `NES_NTSC` filter.
    pub fn disable_ntsc_bisqwit(&mut self) {
        self.ntsc_bisqwit = None;
    }

    /// Whether the true composite `NES_NTSC` filter is active (so the caller knows
    /// to snapshot + supply the index framebuffer this frame).
    #[must_use]
    pub const fn ntsc_bisqwit_active(&self) -> bool {
        self.ntsc_bisqwit.is_some()
    }

    /// v1.2.0 C1 — push the live Bisqwit NTSC picture knobs (contrast /
    /// saturation / brightness / hue) into the filter (no-op when it is
    /// disabled). At [`crate::ntsc_bisqwit::NtscKnobs::DEFAULT`] the decode is
    /// byte-identical to the pre-C1 filter.
    pub const fn set_ntsc_bisqwit_knobs(&mut self, knobs: crate::ntsc_bisqwit::NtscKnobs) {
        if let Some(filter) = self.ntsc_bisqwit.as_mut() {
            filter.set_knobs(knobs);
        }
    }

    /// Enable the CRT / scanline post-pass with the given scanline intensity
    /// (`0.0..=1.0`). If the filter is already enabled this retunes its scanline
    /// intensity to `scanline` (equivalent to [`Self::set_crt_scanline`]). CRT
    /// and NTSC are mutually exclusive at render time.
    pub fn enable_crt(&mut self, scanline: f32) {
        if self.crt.is_none() {
            self.crt = Some(crate::crt::CrtFilter::new(
                &self.device,
                self.config.format,
                &self.nes_texture,
                scanline,
            ));
        } else if let Some(crt) = self.crt.as_mut() {
            crt.set_scanline(scanline);
        }
    }

    /// Disable the CRT filter (skip the post-pass).
    #[allow(dead_code)]
    pub fn disable_crt(&mut self) {
        self.crt = None;
    }

    /// Update the CRT scanline intensity live (no-op when CRT is disabled).
    pub const fn set_crt_scanline(&mut self, scanline: f32) {
        if let Some(crt) = self.crt.as_mut() {
            crt.set_scanline(scanline);
        }
    }

    /// v1.2.0 C2 — (re)build the composable shader stack from `cfg`.
    ///
    /// When `cfg` has at least one enabled, recognized pass, the ping-pong stack
    /// takes over the post-process path (and the legacy single-select filters
    /// are turned off so the two systems never both render). When `cfg` is empty
    /// (or all-disabled / unknown), the stack is cleared and the renderer falls
    /// back to the EXACT legacy chain — the byte-identical default. Returns
    /// `true` when the stack is now active.
    pub fn apply_shader_stack(&mut self, cfg: &crate::shader_pass::ShaderStackConfig) -> bool {
        if cfg.has_enabled_passes() {
            self.shader_stack = crate::shader_pass::ShaderStack::new(
                &self.device,
                self.config.format,
                &self.nes_texture,
                &self.index_texture,
                cfg,
            );
            if self.shader_stack.is_some() {
                // The stack owns the post-process path; drop the legacy filters
                // so they cannot also draw.
                self.ntsc = None;
                self.ntsc_bisqwit = None;
                self.crt = None;
                return true;
            }
        }
        // Empty / all-disabled / failed-compile -> direct-blit fall-through.
        self.shader_stack = None;
        false
    }

    /// Whether the composable shader stack currently owns the post-process path.
    #[must_use]
    pub const fn shader_stack_active(&self) -> bool {
        self.shader_stack.is_some()
    }

    /// Whether the active stack's FIRST pass samples the palette-index texture
    /// (i.e. a leading composite-rt pass), so the caller knows to snapshot +
    /// supply the index framebuffer this frame — same contract as
    /// [`Self::ntsc_bisqwit_active`].
    #[must_use]
    pub fn shader_stack_needs_index(&self) -> bool {
        self.shader_stack
            .as_ref()
            .is_some_and(crate::shader_pass::ShaderStack::needs_index_source)
    }

    /// True when the active shader stack has a pass that consumes the live NES
    /// colour phase (composite-rt or LMP88959), so the caller must snapshot
    /// `ntsc_phase()` even when the index framebuffer itself is not needed.
    #[must_use]
    pub fn shader_stack_needs_phase(&self) -> bool {
        self.shader_stack
            .as_ref()
            .is_some_and(crate::shader_pass::ShaderStack::needs_phase)
    }

    /// v1.2.0 C2 — push the live Bisqwit NTSC knobs forwarded to a leading
    /// composite-rt stack pass (kept in sync with the legacy filter's knobs).
    pub const fn set_stack_ntsc_knobs(&mut self, knobs: crate::ntsc_bisqwit::NtscKnobs) {
        self.stack_ntsc_knobs = knobs;
    }

    /// Resize the surface (triggered on `WindowEvent::Resized`).
    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::cast_slice(&letterbox_uniform(
                w,
                h,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
            )),
        );
    }

    /// v1.0.0 — enable / disable the NES native 8:7 pixel-aspect-ratio
    /// correction and rewrite the letterbox uniform so the change takes effect
    /// on the next present. A no-op when the value is unchanged.
    pub fn set_pixel_aspect(&mut self, on: bool) {
        if self.par_correction == on {
            return;
        }
        self.par_correction = on;
        self.rewrite_blit_uniform();
    }

    /// v1.0.0 — crop / un-crop the top + bottom 8 NES overscan scanlines and
    /// rewrite the blit uniform so the change takes effect on the next
    /// present. A no-op when unchanged. Presentation-layer only — the
    /// framebuffer / core output is untouched.
    pub fn set_hide_overscan(&mut self, on: bool) {
        if self.hide_overscan == on {
            return;
        }
        self.hide_overscan = on;
        self.rewrite_blit_uniform();
    }

    /// v1.5.0 "Lens" Workstream D2 — set the per-side overscan crop (in NES
    /// pixels) and rewrite the blit uniform so the change is live on the next
    /// present. A no-op when unchanged. Presentation-layer only — the
    /// framebuffer / core output is untouched. Combined with the legacy
    /// [`Self::set_hide_overscan`] toggle by the `effective_overscan` helper.
    pub fn set_overscan(&mut self, overscan: crate::config::Overscan) {
        let overscan = overscan.clamped();
        if self.overscan == overscan {
            return;
        }
        self.overscan = overscan;
        self.rewrite_blit_uniform();
    }

    /// v1.0.0 — rewrite the full blit uniform (letterbox + overscan crop) from
    /// the current surface size + flags.
    fn rewrite_blit_uniform(&self) {
        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::cast_slice(&letterbox_uniform(
                self.config.width,
                self.config.height,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
            )),
        );
    }

    /// Upload the NES framebuffer (RGBA8, 256*240*4 bytes) to the texture
    /// and present a frame.
    #[allow(clippy::needless_pass_by_ref_mut)] // matches `render_with_overlay`.
    pub fn render(&mut self, framebuffer: &[u8]) -> Result<(), PresentError> {
        // No index framebuffer and phase 0: this simple present path is used when
        // no phase-consuming shader is active (the composite passes take the
        // `render_with_overlay` path with a live phase).
        self.render_with_overlay(framebuffer, None, 0, |_, _, _, _, _| {})
    }

    /// Upload the framebuffer and present a frame; between the letterbox
    /// pass and `present`, invoke `overlay` so the debugger can draw into
    /// the same surface view.
    #[allow(clippy::needless_pass_by_ref_mut)] // signature parity with `render`.
    #[allow(clippy::too_many_lines)] // upload + filter-priority branch + present.
    pub fn render_with_overlay<F>(
        &mut self,
        framebuffer: &[u8],
        index: Option<&[u16]>,
        video_phase: u8,
        overlay: F,
    ) -> Result<(), PresentError>
    where
        F: FnOnce(
            &wgpu::Device,
            &wgpu::Queue,
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
            (u32, u32),
        ),
    {
        // v1.7.1 — `debug_assert` keeps dev builds honest, but a release build
        // must NEVER hand a mismatched-length slice to `write_texture`: wgpu
        // validation aborts the process (SIGABRT) when the source buffer is
        // smaller than the copy region. The ROM-close path (`emu.nes = None`)
        // presents an empty/zeroed staging slice, so guard the upload and skip
        // it on a size mismatch — the texture keeps its previously-uploaded
        // contents and we still present (a blank/idle frame, never a crash).
        debug_assert_eq!(framebuffer.len(), (NES_W * NES_H * 4) as usize);
        let fb_ok = framebuffer.len() == (NES_W * NES_H * 4) as usize;
        // v1.1.0 beta.1 (T-110-A1) — upload the palette-index framebuffer for
        // the true composite `NES_NTSC` filter, only when it is active and the
        // caller supplied a correctly-sized snapshot. `R16Uint` = 2 bytes/texel.
        // v2.1.2 F2.2 — `video_phase` (the live NES 3-frame colour phase) is now
        // decoupled from the index framebuffer: composite-rt needs BOTH the index
        // texture and the phase, but LMP88959 needs the phase only (it samples
        // RGBA). The caller snapshots the phase whenever any phase-consuming pass
        // is active; here we upload the index texture only when a pass samples it.
        let wants_index = self.ntsc_bisqwit.is_some() || self.shader_stack_needs_index();
        if wants_index
            && let Some(idx) = index
            && idx.len() == (NES_W * NES_H) as usize
        {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.index_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(idx),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(NES_W * 2),
                    rows_per_image: Some(NES_H),
                },
                wgpu::Extent3d {
                    width: NES_W,
                    height: NES_H,
                    depth_or_array_layers: 1,
                },
            );
        }
        if fb_ok {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.nes_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                framebuffer,
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
        }

        // wgpu 29: `get_current_texture` returns the `CurrentSurfaceTexture`
        // enum instead of a `Result`. Map it to the same behaviour as before:
        // use the texture from `Success`/`Suboptimal`, reconfigure on
        // `Lost`/`Outdated`, and skip the frame for any other status.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                return Err(PresentError::Reconfigure);
            }
            wgpu::CurrentSurfaceTexture::Timeout => return Err(PresentError::Other("timeout")),
            wgpu::CurrentSurfaceTexture::Occluded => return Err(PresentError::Other("occluded")),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(PresentError::Other("validation"));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nes-encoder"),
            });
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
        if let Some(t) = &self.gpu_timer {
            t.begin(&mut encoder);
        }

        // Pass 1: optional NTSC post-pass writes into an intermediate
        // texture; the letterbox bind group already references the NES
        // framebuffer view, so for v0 we just sample directly from there.
        // The NTSC filter renders into its own RT and the letterbox would
        // sample from that RT; wiring that requires re-creating the bind
        // group with the new SRV. We keep the public API simple: when
        // NTSC is enabled we run it as a pre-pass into the NES texture
        // itself via a ping-pong. For v0 we run the filter inline as a
        // sample-time effect (the wgsl applies horizontal blur + scanline
        // darkening to the input texel; the result goes straight to the
        // surface).
        // v1.2.0 C2 — when the composable shader stack is active it OWNS the
        // post-process path (ping-pong RTs -> final letterboxed blit). When it
        // is `None` (the empty-config default) the renderer falls through to the
        // EXACT pre-C2 single-select chain below — byte-identical default image.
        if let Some(stack) = &self.shader_stack {
            stack.render(
                &self.queue,
                &mut encoder,
                &view,
                self.config.width,
                self.config.height,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
                video_phase,
                self.stack_ntsc_knobs,
            );
        } else if let Some(filter) = &self.crt {
            filter.render(
                &self.queue,
                &mut encoder,
                &view,
                self.config.width,
                self.config.height,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
            );
        } else if let Some(filter) = &self.ntsc_bisqwit {
            filter.render(
                &self.queue,
                &mut encoder,
                &view,
                self.config.width,
                self.config.height,
                video_phase,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
            );
        } else if let Some(filter) = &self.ntsc {
            filter.render(
                &self.queue,
                &mut encoder,
                &view,
                self.config.width,
                self.config.height,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
            );
        } else {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nes-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &self.bind_group, &[]);
            rp.draw(0..3, 0..1);
        }

        // Overlay pass — egui draws on top.
        overlay(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            (self.config.width, self.config.height),
        );

        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
        if let Some(t) = &mut self.gpu_timer {
            t.end(&mut encoder);
        }
        self.queue.submit(Some(encoder.finish()));
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
        if let Some(t) = &mut self.gpu_timer {
            t.after_submit();
        }
        frame.present();
        Ok(())
    }

    /// v1.2.0 beta.2 (Workstream C3) — present a pre-composited HD-pack RGBA
    /// buffer (`hd_w x hd_h`, RGBA8) with the overlay, letterboxed through the
    /// same nearest pipeline as the NES framebuffer. The HD path bypasses the
    /// post-process filter chain (the compositor already produced final RGBA);
    /// it exists only while a pack is loaded, so the default presentation is
    /// byte-identical.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub fn render_hd_with_overlay<F>(
        &mut self,
        hd_rgba: &[u8],
        hd_w: u32,
        hd_h: u32,
        overlay: F,
    ) -> Result<(), PresentError>
    where
        F: FnOnce(
            &wgpu::Device,
            &wgpu::Queue,
            &mut wgpu::CommandEncoder,
            &wgpu::TextureView,
            (u32, u32),
        ),
    {
        // v1.7.1 — same size guard as `render_with_overlay`: never feed a
        // mismatched-length slice to `write_texture` in a release build (it
        // aborts the process via wgpu validation). Skip the upload on mismatch
        // and present with the texture's previous contents.
        debug_assert_eq!(hd_rgba.len(), (hd_w * hd_h * 4) as usize);
        let hd_ok = hd_rgba.len() == (hd_w * hd_h * 4) as usize;
        self.ensure_hd_blit(hd_w, hd_h);
        let hd = self.hd.as_ref().expect("ensure_hd_blit built it");
        if hd_ok {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &hd.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                hd_rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(hd_w * 4),
                    rows_per_image: Some(hd_h),
                },
                wgpu::Extent3d {
                    width: hd_w,
                    height: hd_h,
                    depth_or_array_layers: 1,
                },
            );
        }
        // The letterbox transform is in normalized UV space, so it is
        // resolution-independent; the HD compositor already applied (or skipped)
        // overscan at NES resolution, so HD blit uses no extra crop.
        self.queue.write_buffer(
            &hd.uniforms,
            0,
            bytemuck::cast_slice(&letterbox_uniform(
                self.config.width,
                self.config.height,
                self.par_correction,
                effective_overscan(self.hide_overscan, self.overscan),
            )),
        );

        // wgpu 29: `get_current_texture` returns the `CurrentSurfaceTexture`
        // enum instead of a `Result`. Map it to the same behaviour as before:
        // use the texture from `Success`/`Suboptimal`, reconfigure on
        // `Lost`/`Outdated`, and skip the frame for any other status.
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                return Err(PresentError::Reconfigure);
            }
            wgpu::CurrentSurfaceTexture::Timeout => return Err(PresentError::Other("timeout")),
            wgpu::CurrentSurfaceTexture::Occluded => return Err(PresentError::Other("occluded")),
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(PresentError::Other("validation"));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nes-hd-encoder"),
            });
        {
            let hd = self.hd.as_ref().expect("built above");
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nes-hd-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &hd.bind_group, &[]);
            rp.draw(0..3, 0..1);
        }
        overlay(
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            (self.config.width, self.config.height),
        );
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// (Re)build the HD blit texture + bind group when absent or resized.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn ensure_hd_blit(&mut self, w: u32, h: u32) {
        if self
            .hd
            .as_ref()
            .is_some_and(|hd| hd.width == w && hd.height == h)
        {
            return;
        }
        let format = self.nes_texture.format();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nes-hd-texture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nes-hd-nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let uniforms = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("nes-hd-letterbox"),
                contents: bytemuck::cast_slice(&letterbox_uniform(
                    self.config.width,
                    self.config.height,
                    self.par_correction,
                    effective_overscan(self.hide_overscan, self.overscan),
                )),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let bgl = self.pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nes-hd-bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        });
        self.hd = Some(HdBlit {
            texture,
            bind_group,
            uniforms,
            width: w,
            height: h,
        });
    }

    /// v2.8.0 Phase 0 (`gpu-timing`) — the most recently resolved GPU pass
    /// time in milliseconds (1-3 frames stale), or `None` when the feature
    /// is off / the adapter lacks timestamp queries / nothing resolved yet.
    #[must_use]
    // Not const: the feature-on body reads an atomic. The feature-off body
    // degenerates to `None`, which trips missing_const_for_fn there only.
    #[allow(clippy::missing_const_for_fn)]
    pub fn last_gpu_pass_ms(&self) -> Option<f32> {
        #[cfg(all(not(target_arch = "wasm32"), feature = "gpu-timing"))]
        {
            self.gpu_timer.as_ref().and_then(GpuTimer::last_ms)
        }
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "gpu-timing")))]
        {
            None
        }
    }
}

/// Compute a letterbox transform mapping the NES 256x240 framebuffer into
/// a window of (`width` x `height`), preserving the chosen display aspect.
/// When `par_8_7` is `true` the NES's native 8:7 pixel-aspect correction is
/// applied (target aspect `(256 * 8 / 7) / 240`); otherwise the square-pixel
/// 256:240 aspect is used. Returns `[scale_x, scale_y, offset_x, offset_y]`
/// in NDC.
///
/// Retained as the reference for the letterbox tests; the live path uses
/// [`letterbox_uniform`] (which folds in the overscan crop).
#[cfg(test)]
#[allow(clippy::cast_precision_loss)] // window dims fit comfortably in f32 mantissa.
fn letterbox(width: u32, height: u32, par_8_7: bool) -> [f32; 4] {
    let win_aspect = width as f32 / height.max(1) as f32;
    let nes_aspect = if par_8_7 {
        (NES_W as f32 * 8.0 / 7.0) / NES_H as f32
    } else {
        NES_W as f32 / NES_H as f32
    };
    let (sx, sy) = if win_aspect > nes_aspect {
        // Window wider than NES: letterbox vertically (full height, narrowed width).
        (nes_aspect / win_aspect, 1.0)
    } else {
        (1.0, win_aspect / nes_aspect)
    };
    [sx, sy, 0.0, 0.0]
}

/// v1.0.0 — number of overscan scanlines cropped from the TOP and the BOTTOM
/// when the legacy `hide_overscan` toggle is on (the CRT-cropped region).
/// 8 + 8 = 16, leaving the inner 256x224 visible.
const OVERSCAN_CROP: u8 = 8;

/// v1.5.0 D2 — fold the legacy `hide_overscan` toggle and the per-side
/// [`crate::config::Overscan`] into one effective per-side crop. The toggle is
/// equivalent to `top = bottom = 8`; the per-side values add on top of it (so a
/// user who keeps the toggle on AND nudges a slider gets the sum). Clamped so
/// the visible region never collapses.
#[must_use]
pub(crate) fn effective_overscan(
    hide_overscan: bool,
    per_side: crate::config::Overscan,
) -> crate::config::Overscan {
    let base = if hide_overscan { OVERSCAN_CROP } else { 0 };
    crate::config::Overscan {
        top: per_side.top.saturating_add(base),
        bottom: per_side.bottom.saturating_add(base),
        left: per_side.left,
        right: per_side.right,
    }
    .clamped()
}

/// v1.0.0 — build the full 8-float blit uniform: the letterbox `rect`
/// (`[sx, sy, ox, oy]`, computed against the VISIBLE NES width/height so the
/// cropped image keeps a correct aspect) followed by the overscan `crop`
/// (`[scale_v, offset_v, scale_u, offset_u]`). With a zero crop the `crop`
/// half is `(1, 0, 1, 0)` and the letterbox matches `letterbox(..)` exactly —
/// the default presentation is byte-identical.
///
/// v1.5.0 D2 — generalized from the binary top/bottom-8 crop to a per-side
/// crop (top/right/bottom/left in NES pixels).
#[allow(clippy::cast_precision_loss)] // window / NES dims fit in f32.
pub(crate) fn letterbox_uniform(
    width: u32,
    height: u32,
    par_8_7: bool,
    overscan: crate::config::Overscan,
) -> [f32; 8] {
    let os = overscan.clamped();
    let crop_v = u32::from(os.top) + u32::from(os.bottom);
    let crop_h = u32::from(os.left) + u32::from(os.right);
    let visible_h = NES_H.saturating_sub(crop_v).max(1);
    let visible_w = NES_W.saturating_sub(crop_h).max(1);
    let win_aspect = width as f32 / height.max(1) as f32;
    // Aspect of the VISIBLE image (square-pixel or 8:7-corrected width over
    // the visible height).
    let img_w = if par_8_7 {
        visible_w as f32 * 8.0 / 7.0
    } else {
        visible_w as f32
    };
    let nes_aspect = img_w / visible_h as f32;
    let (sx, sy) = if win_aspect > nes_aspect {
        (nes_aspect / win_aspect, 1.0)
    } else {
        (1.0, win_aspect / nes_aspect)
    };
    // V crop: scale the [0,1] sample range to the visible rows and offset to
    // the top kept row. U crop is the same on the horizontal axis.
    let crop_scale_v = visible_h as f32 / NES_H as f32;
    let crop_offset_v = f32::from(os.top) / NES_H as f32;
    let crop_scale_u = visible_w as f32 / NES_W as f32;
    let crop_offset_u = f32::from(os.left) / NES_W as f32;
    [
        sx,
        sy,
        0.0,
        0.0,
        crop_scale_v,
        crop_offset_v,
        crop_scale_u,
        crop_offset_u,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_identity_when_window_matches_nes_aspect() {
        // 256x240 -> aspect 1.0666...
        let t = letterbox(NES_W, NES_H, false);
        assert!((t[0] - 1.0).abs() < 1e-5);
        assert!((t[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn letterbox_wide_window_has_horizontal_bars() {
        // Very wide -> width should be scaled down; height kept at 1.
        let t = letterbox(NES_W * 4, NES_H, false);
        assert!(t[0] < 1.0);
        assert!((t[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn letterbox_par_8_7_widens_the_image() {
        // At a square-pixel (256:240) window, enabling 8:7 correction makes the
        // target aspect wider than the window, so the image is letterboxed
        // VERTICALLY (full width, reduced height) rather than rendered 1:1.
        let t = letterbox(NES_W, NES_H, true);
        assert!((t[0] - 1.0).abs() < 1e-5, "full width: {}", t[0]);
        assert!(t[1] < 1.0, "height scaled down for 8:7: {}", t[1]);
    }

    #[test]
    fn letterbox_tall_window_has_vertical_bars() {
        let t = letterbox(NES_W, NES_H * 4, false);
        assert!((t[0] - 1.0).abs() < 1e-5);
        assert!(t[1] < 1.0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // exact zeros for the unused offset fields.
    fn letterbox_uniform_default_matches_letterbox_and_no_crop() {
        // v1.0.0 — with overscan OFF, the rect must equal the legacy letterbox
        // and the crop must be the identity (1.0, 0.0): the default
        // presentation is byte-identical.
        for &(w, h, par) in &[
            (NES_W, NES_H, false),
            (NES_W * 4, NES_H, false),
            (NES_W, NES_H * 4, false),
            (NES_W, NES_H, true),
        ] {
            let base = letterbox(w, h, par);
            let u = letterbox_uniform(w, h, par, crate::config::Overscan::default());
            assert!((u[0] - base[0]).abs() < 1e-6, "sx for {w}x{h} par={par}");
            assert!((u[1] - base[1]).abs() < 1e-6, "sy for {w}x{h} par={par}");
            assert_eq!(u[2], 0.0);
            assert_eq!(u[3], 0.0);
            // crop = identity (v-scale=1, v-off=0, u-scale=1, u-off=0).
            assert!((u[4] - 1.0).abs() < 1e-6, "crop v-scale");
            assert_eq!(u[5], 0.0, "crop v-offset");
            assert!((u[6] - 1.0).abs() < 1e-6, "crop u-scale");
            assert_eq!(u[7], 0.0, "crop u-offset");
        }
    }

    #[test]
    fn letterbox_uniform_overscan_crops_inner_224_rows() {
        // v1.0.0 — with the legacy hide-overscan toggle the crop samples rows
        // [8/240, 232/240]: scale = 224/240, offset = 8/240. The new per-side
        // path expresses that as top = bottom = 8 (via `effective_overscan`).
        let os = effective_overscan(true, crate::config::Overscan::default());
        let u = letterbox_uniform(NES_W, NES_H, false, os);
        assert!((u[4] - 224.0 / 240.0).abs() < 1e-6, "crop v-scale {}", u[4]);
        assert!((u[5] - 8.0 / 240.0).abs() < 1e-6, "crop v-offset {}", u[5]);
        // No horizontal crop -> U identity.
        assert!((u[6] - 1.0).abs() < 1e-6, "crop u-scale {}", u[6]);
        assert!(u[7].abs() < 1e-6, "crop u-offset {}", u[7]);
        // The visible image aspect is now 256:224 (taller-pixel), so at a
        // 256x240 window the letterbox is non-identity (the image gains a
        // vertical bar OR widens — either way the height scale changes).
        assert!(u[0] <= 1.0 && u[1] <= 1.0);
    }

    #[test]
    fn letterbox_uniform_per_side_crop_remaps_u_and_v() {
        // v1.5.0 D2 — a per-side crop (top=16, left=32) maps the V/U sample
        // ranges to the inner rect: V scale=(240-16)/240, offset=16/240; U
        // scale=(256-32)/256, offset=32/256 (no right/bottom here).
        let os = crate::config::Overscan {
            top: 16,
            bottom: 0,
            left: 32,
            right: 0,
        };
        let u = letterbox_uniform(NES_W, NES_H, false, os);
        assert!((u[4] - 224.0 / 240.0).abs() < 1e-6, "v-scale {}", u[4]);
        assert!((u[5] - 16.0 / 240.0).abs() < 1e-6, "v-offset {}", u[5]);
        assert!((u[6] - 224.0 / 256.0).abs() < 1e-6, "u-scale {}", u[6]);
        assert!((u[7] - 32.0 / 256.0).abs() < 1e-6, "u-offset {}", u[7]);
    }

    /// The embedded blit WGSL must parse + validate (the same checks wgpu runs
    /// at `create_shader_module`), so a shader regression fails CI not at runtime.
    #[test]
    fn shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(super::SHADER_SRC).expect("gfx WGSL must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("gfx WGSL must validate");
    }
}
