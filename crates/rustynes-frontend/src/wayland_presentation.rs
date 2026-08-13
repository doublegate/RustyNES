// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.3.3 — sourcing the display refresh from the Wayland compositor.
//!
//! # Why this module exists
//!
//! Display-synchronised pacing needs one number: how long a display refresh
//! lasts. [`crate::refresh_probe::best_divisor`] turns that into "produce one
//! emulated frame every `N` refreshes", and everything downstream — the
//! phase/rate split, the divisor gating, the sustained-miss fallback — is
//! already built and verified. The whole of `docs/performance.md` v2.3.3 F2
//! rests on obtaining a trustworthy refresh figure.
//!
//! The obvious source is the windowing API, and when it answers it is the
//! right answer: `winit`'s `Monitor::refresh_rate_millihertz()` is exact and a
//! measurement could only degrade it. **But it frequently does not answer.**
//! On the KDE Wayland session this was investigated against, the compositor
//! advertises no `wl_output` global at all (65 globals, none an output), so
//! `Window::current_monitor()` returns `None` for the entire session — not
//! merely during startup. There is nothing to wait for and nothing to retry.
//!
//! # Why measuring redraw intervals was abandoned
//!
//! The first answer was empirical: force Fifo, drive continuous redraws, and
//! take the median interval ([`crate::refresh_probe`]). That estimator is
//! sound; its *input* is not. **A redraw interval measures the application,
//! not the display, and the two diverge precisely when it matters** — on a
//! heavy commercial ROM the probe returned 20.032 Hz on a 119.991 Hz panel,
//! because the app was taking ~14 ms per frame and the GPU had not yet left
//! its idle clock. The measurement is accurate only on the ROMs that never
//! needed it. No retry schedule fixes a signal that is measuring the wrong
//! thing, which is why three successive attempts to rescue it were discarded.
//!
//! # What this module does instead
//!
//! `wp_presentation` is a **stable** Wayland protocol
//! (`protocols/stable/presentation-time/presentation-time.xml`) and *is*
//! advertised by this compositor. Its `presented` event carries a `refresh`
//! argument: the compositor's own prediction, in nanoseconds, of when the next
//! output refresh occurs. That is the refresh period, stated by the authority
//! that owns it — not inferred from the client's own frame cadence, and not
//! dependent on a `wl_output` global existing. It is exactly the number the
//! investigation needed, and it is free: one request per present.
//!
//! # Interoperating with winit's connection
//!
//! Presentation feedback attaches to a specific `wl_surface`, and the surface
//! that matters is winit's. Wayland objects belong to the connection that
//! created them, so this cannot open its own connection and must instead
//! attach to winit's `wl_display`, reconstructing the surface as a proxy on
//! it. That is what `Backend::from_foreign_display` and `ObjectId::from_ptr`
//! are for — the documented libwayland-interop path — and it is the only
//! `unsafe` here. Both pointers come from `raw-window-handle`, and the
//! `Arc<Window>` that owns them is held for the lifetime of this struct so
//! neither can dangle.
//!
//! Events are collected on a **separate event queue** created on that shared
//! connection. libwayland routes each object's events to the queue it was
//! created on, so winit's socket reads deliver our feedback events into our
//! queue, and [`PresentationClock::poll`](crate::wayland_presentation::PresentationClock::poll)
//! drains them with a non-blocking
//! `dispatch_pending`. Nothing here ever blocks on the socket: a roundtrip
//! from inside winit's own dispatch is the one way this could hang the app,
//! and the few frames of extra latency avoided by one are worth nothing.
//!
//! # Scope
//!
//! Output-only and best-effort. Every failure — not Wayland, no
//! `wp_presentation` global, a mismatched surface interface, a compositor that
//! reports `refresh = 0` (variable-refresh outputs are required to) — returns
//! `None` and leaves the caller on exactly the path it was already taking.
//! The emulation core is not involved at any point.

use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::presentation_time::client::wp_presentation::{self, WpPresentation};
use wayland_protocols::wp::presentation_time::client::wp_presentation_feedback::{
    self, WpPresentationFeedback,
};
use winit::window::Window;

/// Refresh reports required before an estimate is offered.
///
/// Far fewer than the redraw probe's 80 because these are not measurements:
/// each is the compositor stating the period outright, so the sample set exists
/// to reject an output that is *changing* (a window dragged between panels, a
/// mode switch mid-startup) rather than to average away noise. At 120 Hz this
/// is a fifth of a second.
const PRESENTATION_SAMPLES: usize = 24;

/// Hard cap on retained samples so a clock left polling cannot grow without
/// bound if the estimator keeps refusing.
const MAX_SAMPLES: usize = 240;

/// Protocol version bound from the registry.
///
/// Version 1 already carries `refresh` on the `presented` event, which is the
/// only field consumed here; binding higher would add nothing and narrow the
/// set of compositors that match.
const PRESENTATION_VERSION: u32 = 1;

/// Dispatch target for the presentation-time objects.
///
/// Separate from [`PresentationClock`] because `EventQueue::dispatch_pending`
/// takes the state by `&mut`, so it cannot live inside the struct that owns
/// the queue.
#[derive(Debug, Default)]
struct State {
    /// The bound global, once the registry has advertised it.
    presentation: Option<WpPresentation>,
    /// Reported refresh periods, in milliseconds.
    samples_ms: Vec<f64>,
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == WpPresentation::interface().name
            && state.presentation.is_none()
        {
            state.presentation = Some(registry.bind::<WpPresentation, _, _>(
                name,
                version.min(PRESENTATION_VERSION),
                qh,
                (),
            ));
        }
    }
}

impl Dispatch<WpPresentation, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpPresentation,
        _event: wp_presentation::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The only event is `clock_id`, which names the clock domain the
        // presentation timestamps live in. Those timestamps are never read
        // here — only the `refresh` period is — so the domain is irrelevant.
    }
}

impl Dispatch<WpPresentationFeedback, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &WpPresentationFeedback,
        event: wp_presentation_feedback::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // `discarded` (the frame never reached the screen) and `sync_output`
        // (which output it landed on) carry no period, so only `presented`
        // is of interest. Both are destructors handled by the protocol layer.
        if let wp_presentation_feedback::Event::Presented { refresh, .. } = event {
            // The spec requires `refresh = 0` for an output with no constant
            // refresh rate. That is a legitimate answer meaning "there is no
            // period to lock to", so it is dropped rather than recorded — and
            // if every report is zero the estimator simply never fires and the
            // session stays wall-clock paced, which is correct for that panel.
            if refresh > 0 {
                // Evict the OLDEST rather than drop the newest. Dropping the
                // newest bounded memory but made the sample set unrecoverable:
                // if the first `MAX_SAMPLES` reports straddled an output change
                // — a window dragged between panels, a mode switch during
                // startup — the quorum could never clear, `settled` stayed
                // false, and `request_feedback` then created one feedback
                // object per present for the rest of the session with no
                // possibility of ever producing an estimate. A sliding window
                // is self-healing at the same bounded cost.
                if state.samples_ms.len() >= MAX_SAMPLES {
                    state.samples_ms.remove(0);
                }
                state.samples_ms.push(f64::from(refresh) / 1_000_000.0);
            }
        }
    }
}

/// A refresh-period source backed by the compositor's own presentation
/// feedback.
///
/// Construct once per surface, call [`request_feedback`](Self::request_feedback)
/// on each present and [`poll`](Self::poll) alongside it; `poll` yields the
/// refresh in Hz exactly once, when enough consistent reports have arrived.
#[derive(Debug)]
pub struct PresentationClock {
    /// Our view of winit's connection. Owns nothing: dropping it does not
    /// disconnect the display.
    conn: Connection,
    /// Our private queue on that connection.
    queue: EventQueue<State>,
    /// Handle used to assign newly-created objects to `queue`.
    qh: QueueHandle<State>,
    /// winit's surface, reconstructed as a proxy usable as a request argument.
    surface: WlSurface,
    /// Dispatch state, held here so `poll` can lend it to the queue.
    state: State,
    /// Set once an estimate has been reported, after which this stops issuing
    /// requests and stops answering. One number is all the caller needs, and a
    /// clock that kept re-reporting could oscillate the pacing regime — the
    /// failure mode that made the previous attempt worse than doing nothing.
    settled: bool,
    /// Keeps winit's window — and therefore the `wl_display` and `wl_surface`
    /// the raw pointers above were taken from — alive for at least as long as
    /// this struct. This is the invariant the `unsafe` in
    /// [`new`](Self::new) relies on, so the field is load-bearing despite
    /// never being read.
    ///
    /// DECLARED LAST ON PURPOSE. Rust drops struct fields in declaration
    /// order, so this releases the `Arc<Window>` only after `conn`, `queue`
    /// and `surface` — every field backed by a pointer into winit's Wayland
    /// state — are already gone. Declared first (as it was), teardown could
    /// drop the last window reference and then run foreign-backed destructors
    /// against a freed `wl_display`. Moving this field up reopens that hole.
    _window: Arc<Window>,
}

impl PresentationClock {
    /// Attach to `window`'s compositor, or return `None` if that is not
    /// possible.
    ///
    /// `None` is the expected result on X11, Windows and macOS, and on any
    /// Wayland compositor without `wp_presentation`. It is not an error and is
    /// not reported as one: the caller carries on with the declared refresh (or
    /// without one), exactly as before this module existed.
    #[must_use]
    pub fn new(window: &Arc<Window>) -> Option<Self> {
        // Both handles must be Wayland; anything else exits here, before any
        // pointer is touched.
        let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
            return None;
        };
        let RawWindowHandle::Wayland(surface) = window.window_handle().ok()?.as_raw() else {
            return None;
        };

        // SAFETY: `display.display` is the `wl_display` winit is connected to,
        // and `surface.surface` the `wl_surface` backing this window, both
        // supplied by `raw-window-handle` from winit's live Wayland backend.
        // The `Arc<Window>` that owns them is cloned into `_window` below —
        // declared LAST in the struct so it outlives every foreign-backed
        // field during drop — so neither pointer can be freed while it is in
        // use. `from_foreign_display` explicitly does not take
        // ownership of the display — it will not disconnect it on drop — and
        // `ObjectId::from_ptr` validates the proxy's interface against
        // `WlSurface`, returning `Err` rather than mis-typing a foreign object.
        #[allow(unsafe_code)]
        let (conn, surface_id) = unsafe {
            let backend = Backend::from_foreign_display(display.display.as_ptr().cast());
            let conn = Connection::from_backend(backend);
            let id = ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast());
            (conn, id)
        };
        let surface = WlSurface::from_id(&conn, surface_id.ok()?).ok()?;

        let queue = conn.new_event_queue::<State>();
        let qh = queue.handle();
        // Ask for the globals. Deliberately no roundtrip: this runs inside
        // winit's own dispatch, where a blocking socket read is the one way to
        // deadlock the event loop. The registry answer arrives on a later
        // `poll`, costing a few frames of latency and no risk.
        let _registry = conn.display().get_registry(&qh, ());
        conn.flush().ok()?;

        Some(Self {
            conn,
            queue,
            qh,
            surface,
            state: State::default(),
            settled: false,
            // Last, matching the declaration order the drop safety relies on.
            _window: Arc::clone(window),
        })
    }

    /// Ask the compositor to report on the frame about to be presented.
    ///
    /// Call once per present. A no-op before the global has been bound and
    /// after an estimate has settled, so the steady-state cost is nil.
    pub fn request_feedback(&mut self) {
        if self.settled {
            return;
        }
        let Some(presentation) = self.state.presentation.as_ref() else {
            return;
        };
        // The feedback object is a one-shot: the protocol destroys it when it
        // delivers `presented` or `discarded`, so there is nothing to retain.
        let _feedback = presentation.feedback(&self.surface, &self.qh, ());
        let _ = self.conn.flush();
    }

    /// Drain any feedback that has arrived and, once the reports agree, return
    /// the display refresh in Hz.
    ///
    /// Returns `Some` **exactly once per session**; every later call returns
    /// `None`. The caller should treat that one value as terminal.
    ///
    /// `#[must_use]` because that single `Some` is the entire output of the
    /// module: dropping it silently forfeits the refresh figure for the rest of
    /// the session. The off-Wayland stub carries the attribute too, so the
    /// warning fires on the platform where the value actually exists rather
    /// than only where the method is unreachable.
    #[must_use]
    pub fn poll(&mut self) -> Option<f64> {
        if self.settled {
            return None;
        }
        // Non-blocking: processes only what winit's socket reads have already
        // routed into this queue.
        let _ = self.queue.dispatch_pending(&mut self.state);
        let hz = crate::refresh_probe::estimate_hz_from_intervals(
            &self.state.samples_ms,
            PRESENTATION_SAMPLES,
        )?;
        self.settled = true;
        Some(hz)
    }
}
