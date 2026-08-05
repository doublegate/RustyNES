//! v2.3.0 "Datum II" — true multi-viewport tool-window detach.
//!
//! This module lets a tool panel (PPU viewer, Cheats, Audio Mixer, …) leave the
//! main window and live in its **own real OS window** — the fix for the Windows-10
//! "every tool window is trapped inside the main window" report. It replaces the
//! v2.2.9 stopgap, where `egui`'s `show_viewport_immediate` merely *embedded* the
//! panel inside the single main viewport (`RustyNES`'s `egui_winit` integration was
//! single-viewport, so the immediate viewport never became a separate window).
//!
//! ## Why a separate module instead of egui's native multi-viewport
//!
//! egui *does* have a native multi-viewport path (`set_embed_viewports(false)` +
//! `Context::set_immediate_viewport_renderer`). But an *immediate* viewport is
//! rendered by a re-entrant callback that runs **inside** the parent's render pass
//! and must create a winit window on the spot — which needs `&ActiveEventLoop`.
//! That reference is only valid during event dispatch, so `eframe` erases its
//! lifetime into a `'static` thread-local behind `unsafe`. A *deferred* viewport
//! avoids that, but its callback is `Fn(&Context) + Send + Sync + 'static`, so it
//! cannot borrow the live `&mut Nes` a debugger panel needs.
//!
//! `RustyNES` deliberately avoids both. Each detached window here owns its own egui
//! [`egui::Context`] / [`egui_winit::State`] / [`egui_wgpu::Renderer`] /
//! [`wgpu::Surface`] and is rendered on **its own** `RedrawRequested`, in its own
//! stack frame, where the panel's borrows (`&mut Nes`, panel state) are freshly
//! re-acquired by re-running the panel dispatch under a thread-local render-target
//! filter (see [`crate::debugger::DebuggerOverlay::render_detached_body`]). Nothing is stashed
//! across frames, so **no `unsafe` and no lifetime erasure are required**.
//!
//! ## GPU resource sharing
//!
//! Every detached window shares the main [`crate::gfx::Gfx`]'s one
//! [`wgpu::Instance`] / [`wgpu::Adapter`] / [`wgpu::Device`] / [`wgpu::Queue`]
//! (retained on `Gfx` for exactly this purpose). Sharing the device is not just an
//! optimization: an `egui_wgpu::Renderer`'s textures and buffers must live on the
//! same device its render pass targets, so a second device would force
//! cross-device texture copies egui does not perform. Only the per-window surface
//! and egui state are new.
//!
//! Native-only: wasm is single-canvas and keeps every tool panel docked, so this
//! whole module is `#[cfg(not(target_arch = "wasm32"))]` and never compiled there.

use std::collections::HashMap;
use std::sync::Arc;

use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

/// One detached tool panel, hosted in its own OS window with a private egui stack.
///
/// The `window` `Arc` keeps the winit window (and therefore the surface's backing
/// handle) alive for the surface's whole lifetime — which is why
/// [`crate::gfx::Gfx::create_detached_surface`] can hand back a `'static` surface.
struct DetachedWindow {
    /// The OS window. `Arc` because the surface borrows its display/window handle
    /// for `'static`; we hold the only other reference.
    window: Arc<Window>,
    /// This window's own swapchain surface (created from the shared instance).
    surface: wgpu::Surface<'static>,
    /// Live surface configuration (tracks size for reconfigure on resize).
    config: wgpu::SurfaceConfiguration,
    /// This window's own egui frontend state (input + platform output).
    state: egui_winit::State,
    /// This window's own egui render pipeline (wgpu-backed), targeting `config.format`.
    renderer: egui_wgpu::Renderer,
    /// The stable panel id this window hosts (e.g. `"ppu"`); the render-target key.
    panel_id: &'static str,
    /// How often this panel needs to repaint on the emulator's frame clock (v2.3.0).
    refresh: crate::debugger::DetachedRefresh,
}

/// Owns every detached tool window and the panel-id ⇄ window-id mapping.
///
/// Lives on `App` (native only). The main event loop routes window events here by
/// [`WindowId`], reconciles the open set against the debugger's "detached panels"
/// set each iteration, and drives one render per window per frame.
#[derive(Default)]
pub struct DetachedManager {
    /// Every live detached window, keyed by its OS window id.
    windows: HashMap<WindowId, DetachedWindow>,
    /// Reverse index: panel id → its window id (for reconcile / close-by-panel).
    by_panel: HashMap<&'static str, WindowId>,
    /// Monotonic tick advanced once per produced emulator frame; drives the
    /// `Throttled` refresh tier's ~10 Hz cadence (v2.3.0).
    throttle_tick: u32,
}

/// The egui output of one detached window's `run_ui` pass.
///
/// Carried from [`DetachedManager::render_ui`] (which needs the emulator lock,
/// because the panel body borrows `&mut Nes`) to [`DetachedManager::present`] (which
/// does NOT — it is pure GPU work + a vsync-blocking present). This lets `App` drop the
/// emulator lock BEFORE the blocking present, so N detached windows never serialize
/// the emulation thread behind N vsync waits (the "slows to a crawl" bug).
pub struct PreparedDetachedFrame {
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
}

impl DetachedManager {
    /// True if `id` is one of our detached windows (so the main event handler must
    /// route the event here instead of treating it as the main window).
    #[must_use]
    pub fn contains_window(&self, id: WindowId) -> bool {
        self.windows.contains_key(&id)
    }

    /// True if `panel` currently has an open detached window.
    #[must_use]
    pub fn has_panel(&self, panel: &'static str) -> bool {
        self.by_panel.contains_key(panel)
    }

    /// Number of open detached windows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_panel.len()
    }

    /// True when no tool panel is detached (the overwhelmingly common case, and the
    /// cheapest possible reconcile fast-path).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_panel.is_empty()
    }

    /// True when every open window's panel is still in `desired` — an
    /// allocation-free membership check used by `App`'s per-iteration reconcile
    /// fast-path. Combined with an equal length this proves the two sets match.
    #[must_use]
    pub fn all_panels_in(&self, desired: &std::collections::HashSet<&'static str>) -> bool {
        self.by_panel.keys().all(|id| desired.contains(id))
    }

    /// The panel id hosted by window `id`, if it is one of ours.
    #[must_use]
    pub fn panel_of(&self, id: WindowId) -> Option<&'static str> {
        self.windows.get(&id).map(|w| w.panel_id)
    }

    /// The set of panel ids that currently have a detached window (owned copy so
    /// the caller can mutate `self` while iterating).
    #[must_use]
    pub fn detached_ids(&self) -> Vec<&'static str> {
        self.by_panel.keys().copied().collect()
    }

    /// Advance the frame tick and repaint each detached window according to its
    /// [`crate::debugger::DetachedRefresh`] tier: `Live` panels every produced frame
    /// (~60 Hz), `Throttled` panels at ~10 Hz, and `OnInteraction` panels never here
    /// (they repaint only when the user interacts, via the egui repaint flag). Called
    /// once per produced emulator frame. This is the "smart update rate" pass that
    /// keeps a wall of static detached panels from each costing a lock hold + GPU
    /// frame every 16 ms.
    pub fn request_redraw_tick(&mut self) {
        // ~10 Hz throttle at the NTSC ~60 Hz frame rate (every 6th produced frame).
        const THROTTLE_DIV: u32 = 6;
        self.throttle_tick = self.throttle_tick.wrapping_add(1);
        let throttled_now = self.throttle_tick.is_multiple_of(THROTTLE_DIV);
        for w in self.windows.values() {
            let repaint = match w.refresh {
                crate::debugger::DetachedRefresh::Live => true,
                crate::debugger::DetachedRefresh::Throttled => throttled_now,
                crate::debugger::DetachedRefresh::OnInteraction => false,
            };
            if repaint {
                w.window.request_redraw();
            }
        }
    }

    /// Create a new detached OS window hosting `panel_id`.
    ///
    /// Builds the winit window from `event_loop`, a swapchain surface from the
    /// shared [`crate::gfx::Gfx`] instance/adapter/device, and a fresh egui
    /// `Context`/`State`/`Renderer` mirroring [`crate::debugger::DebuggerOverlay`]'s
    /// setup (Font Awesome installed, root viewport, matching renderer options).
    ///
    /// # Errors
    /// Returns a message if the OS refuses the window or the surface can't be made.
    pub fn create(
        &mut self,
        event_loop: &ActiveEventLoop,
        gfx: &crate::gfx::Gfx,
        panel_id: &'static str,
        title: &str,
        size: (u32, u32),
        refresh: crate::debugger::DetachedRefresh,
    ) -> Result<(), String> {
        if self.by_panel.contains_key(panel_id) {
            return Ok(()); // already open — idempotent
        }
        let attrs = WindowAttributes::default()
            .with_title(title)
            .with_inner_size(LogicalSize::new(size.0.max(1), size.1.max(1)));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| format!("create detached window: {e}"))?,
        );
        let (surface, config) = gfx
            .create_detached_surface(window.clone())
            .map_err(|e| format!("create detached surface: {e}"))?;

        // Mirror DebuggerOverlay::new so the detached panel is themed/scaled and
        // renders identically to its docked form.
        let ctx = egui::Context::default();
        crate::icons::install(&ctx);
        let viewport_id = ctx.viewport_id();
        let state = egui_winit::State::new(ctx, viewport_id, &*window, None, None, None);
        let renderer = egui_wgpu::Renderer::new(
            &gfx.device,
            config.format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                dithering: false,
                predictable_texture_filtering: false,
            },
        );

        let id = window.id();
        window.request_redraw();
        self.windows.insert(
            id,
            DetachedWindow {
                window,
                surface,
                config,
                state,
                renderer,
                panel_id,
                refresh,
            },
        );
        self.by_panel.insert(panel_id, id);
        Ok(())
    }

    /// Close the detached window `id` (drops its window, surface, and egui state).
    pub fn close(&mut self, id: WindowId) {
        if let Some(w) = self.windows.remove(&id) {
            self.by_panel.remove(w.panel_id);
        }
    }

    /// Close the detached window hosting `panel` (used by reconcile when the panel
    /// was reattached from inside the main window).
    pub fn close_by_panel(&mut self, panel: &'static str) {
        if let Some(id) = self.by_panel.remove(panel) {
            self.windows.remove(&id);
        }
    }

    /// Feed a window event to the hosting window's egui integration (pointer /
    /// keyboard / focus) and report whether egui now wants a repaint. The caller
    /// uses that to `request_redraw` ONLY on interaction, so an idle detached window
    /// does not spin (the per-frame live refresh is driven separately, once per
    /// produced emulator frame).
    pub fn on_window_event(&mut self, id: WindowId, event: &WindowEvent) -> bool {
        if let Some(w) = self.windows.get_mut(&id) {
            w.state.on_window_event(&w.window, event).repaint
        } else {
            false
        }
    }

    /// Request a repaint of one detached window (used after an interaction the egui
    /// integration flagged as repaint-worthy).
    pub fn request_redraw(&self, id: WindowId) {
        if let Some(w) = self.windows.get(&id) {
            w.window.request_redraw();
        }
    }

    /// Reconfigure window `id`'s surface after an OS resize.
    pub fn resize(&mut self, id: WindowId, width: u32, height: u32, device: &wgpu::Device) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.config.width = width.max(1);
            w.config.height = height.max(1);
            w.surface.configure(device, &w.config);
        }
    }

    /// Phase 1 of a detached window's frame: run its egui UI. This is the ONLY part
    /// that needs the emulator lock (the `build` panel body borrows `&mut Nes`), so
    /// the caller holds the lock across just this call and drops it before
    /// [`Self::present`]. Returns the tessellation input, or `None` if the window is
    /// gone.
    pub fn render_ui(
        &mut self,
        id: WindowId,
        build: impl FnMut(&mut egui::Ui),
    ) -> Option<PreparedDetachedFrame> {
        let w = self.windows.get_mut(&id)?;
        let raw = w.state.take_egui_input(&w.window);
        let ctx = w.state.egui_ctx().clone();
        let output = ctx.run_ui(raw, build);
        w.state
            .handle_platform_output(&w.window, output.platform_output);

        // Honour egui's own repaint request. `run_ui` reports, per viewport, how
        // long it is willing to wait before the next frame; a ZERO delay means it
        // wants another frame immediately — a hover highlight or active-widget
        // colour mid-transition, a spinner, a text cursor blink.
        //
        // This matters most for the `OnInteraction` refresh tier, which is the one
        // tier that never re-arms from the emulator frame clock. Without this, an
        // animation that egui started would stop partway and only resume on the
        // next user input, leaving the widget stuck in a half-lit state.
        if output
            .viewport_output
            .values()
            .any(|v| v.repaint_delay.is_zero())
        {
            w.window.request_redraw();
        }

        Some(PreparedDetachedFrame {
            shapes: output.shapes,
            textures_delta: output.textures_delta,
        })
    }

    /// Phase 2 of a detached window's frame: tessellate + paint + present. Does NOT
    /// touch the emulator, so the caller MUST drop the emulator lock before calling
    /// this — the `frame.present()` here blocks on vsync (Fifo), and holding the
    /// lock across it is what starved the emulation thread.
    ///
    /// The color attachment is **cleared** (not `Load`ed): a detached window has no
    /// NES framebuffer underneath, so the panel draws onto a solid backdrop.
    pub fn present(
        &mut self,
        id: WindowId,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared: PreparedDetachedFrame,
    ) {
        let Some(w) = self.windows.get_mut(&id) else {
            return;
        };
        let ctx = w.state.egui_ctx().clone();

        // Apply egui's texture delta BEFORE anything that can bail out.
        //
        // The delta is ONE-SHOT: egui hands over each created/updated texture
        // exactly once and then forgets it. Skipping a frame's `set` list (as an
        // early `return` on a lost swapchain would) means the font atlas and any
        // newly-created image never reach the renderer, and every later frame
        // draws with a texture id the renderer has never seen — the window stays
        // blank or garbled for the rest of its life, long after the transient
        // surface error cleared. Uploading first costs nothing on the happy path
        // and makes a skipped present merely a dropped frame.
        for (tid, image) in prepared.textures_delta.set {
            w.renderer.update_texture(device, queue, tid, &image);
        }

        // Acquire the swapchain image (wgpu 29 `CurrentSurfaceTexture` enum), with
        // the same reconfigure-on-lost / skip-otherwise policy as `Gfx`. Frees are
        // deferred to the end so a bail-out cannot drop a texture the next frame
        // still references.
        let frame = match w.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                w.surface.configure(device, &w.config);
                Self::free_textures(&mut w.renderer, prepared.textures_delta.free);
                return;
            }
            _ => {
                Self::free_textures(&mut w.renderer, prepared.textures_delta.free);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("detached-egui-encoder"),
        });

        let pixels_per_point = ctx.pixels_per_point();
        let clipped = ctx.tessellate(prepared.shapes, pixels_per_point);
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [w.config.width.max(1), w.config.height.max(1)],
            pixels_per_point,
        };
        w.renderer
            .update_buffers(device, queue, &mut encoder, &clipped, &screen_desc);
        {
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("detached-egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.05,
                                g: 0.05,
                                b: 0.05,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            w.renderer.render(&mut rp, &clipped, &screen_desc);
        }
        Self::free_textures(&mut w.renderer, prepared.textures_delta.free);
        queue.submit(Some(encoder.finish()));
        frame.present();
    }

    /// Release the textures egui retired this frame.
    ///
    /// Factored out so every exit path from [`Self::present`] — including the
    /// swapchain bail-outs — runs it exactly once. Leaking these would grow GPU
    /// memory for the window's lifetime.
    fn free_textures(renderer: &mut egui_wgpu::Renderer, free: Vec<egui::TextureId>) {
        for tid in free {
            renderer.free_texture(&tid);
        }
    }
}
