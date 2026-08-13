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
    /// v2.3.3 — captured SCANOUT reports, when scanout tracing is on.
    ///
    /// Empty and never appended to unless [`PresentationClock::trace_scanout`]
    /// was set, so the shipped path allocates nothing here.
    scanouts: Vec<Scanout>,
    /// Whether to append to [`Self::scanouts`] at all.
    trace_scanout: bool,
    /// Presentation feedback that came back `discarded` — the frame was
    /// composited but never reached the screen. Counted rather than recorded,
    /// because there is no timestamp to record.
    discarded: u64,
    /// The POSIX clock id the compositor stamps presentations with.
    clock_id: Option<u32>,
}

/// v2.3.3 — one compositor-reported scanout.
///
/// **Why this is the only honest display-cadence measurement.** Everything else
/// in this frontend timestamps `present()` *returning*, which under
/// triple-buffered Fifo is when an image was QUEUED, not when it was scanned
/// out — the two differ by an unbounded amount and burst differently (measured:
/// 31.8% of present returns under 1 ms apart, only 1.9% near one refresh
/// period). A cadence computed from those timestamps counts queue slots, not
/// refreshes on screen, which invalidated the v2.3.3 F10 raggedness result.
///
/// `wp_presentation` reports the actual scanout instant, from the compositor,
/// in the compositor's own clock domain. That is the number that answers "what
/// did the display show, and when".
#[derive(Debug, Clone, Copy)]
pub struct Scanout {
    /// Presentation time in nanoseconds, in the clock domain named by the
    /// protocol's `clock_id` event (see [`PresentationClock::clock_id`]).
    /// Assembled from the protocol's split `tv_sec_hi` / `tv_sec_lo` /
    /// `tv_nsec` triple.
    pub t_ns: u64,
    /// The compositor's own refresh estimate for this presentation, in
    /// nanoseconds; `0` means "no constant refresh rate".
    pub refresh_ns: u32,
    /// Monotonically increasing per-output sequence counter (`seq_hi`/`seq_lo`
    /// joined), when the compositor supplies one. A gap of more than 1 between
    /// consecutive reports is a MISSED refresh, stated by the compositor rather
    /// than inferred from timing — the single most direct evidence available
    /// for the judder question.
    pub seq: u64,
    /// Raw protocol flags (`VSYNC` / `HW_CLOCK` / `HW_COMPLETION` /
    /// `ZERO_COPY`), kept as bits so the log records what the compositor said
    /// rather than this module's interpretation of it.
    pub flags: u32,
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
        state: &mut Self,
        _proxy: &WpPresentation,
        event: wp_presentation::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The only event is `clock_id`, naming the POSIX clock the presentation
        // timestamps live in. Irrelevant while only `refresh` was read; now
        // that scanout timestamps are recorded it is essential — a timestamp
        // without its clock domain cannot be compared to anything, and
        // recording the id lets the analysis say so rather than assume
        // `CLOCK_MONOTONIC` (1) and silently mis-align if it is not.
        if let wp_presentation::Event::ClockId { clk_id } = event {
            state.clock_id = Some(clk_id);
        }
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
        // `sync_output` (which output it landed on) carries no period.
        // `discarded` means the frame never reached the screen: no timestamp
        // exists to record, but the COUNT is meaningful — a discarded frame is
        // work that was composited and thrown away, invisible in every
        // present-side metric. Both are destructors handled by the protocol
        // layer.
        if matches!(event, wp_presentation_feedback::Event::Discarded) {
            state.discarded = state.discarded.saturating_add(1);
            return;
        }
        if let wp_presentation_feedback::Event::Presented {
            tv_sec_hi,
            tv_sec_lo,
            tv_nsec,
            refresh,
            seq_hi,
            seq_lo,
            flags,
            ..
        } = event
        {
            if state.trace_scanout {
                // The protocol splits a 64-bit seconds count across two u32s
                // (the interface predates 64-bit scalars). Join, then fold in
                // the nanosecond remainder — in nanoseconds throughout, since
                // that is the resolution the compositor reports and any
                // conversion to f64 here would quantise it.
                let secs = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
                let t_ns = secs
                    .saturating_mul(1_000_000_000)
                    .saturating_add(u64::from(tv_nsec));
                state.scanouts.push(Scanout {
                    t_ns,
                    refresh_ns: refresh,
                    seq: (u64::from(seq_hi) << 32) | u64::from(seq_lo),
                    flags: flags.into(),
                });
            }
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
        // v2.3.3 — settling stops the refresh estimator, but scanout tracing
        // needs a report for EVERY present, so it keeps the requests flowing.
        // Deliberately gated: one feedback object per present for a whole
        // session is a real (if small) cost, and the shipped path has no use
        // for it once the refresh is known.
        if self.settled && !self.state.trace_scanout {
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

    /// v2.3.3 — start recording compositor-reported scanout instants.
    ///
    /// Off by default. When on, [`Self::request_feedback`] keeps issuing after
    /// the refresh estimate settles and every `presented` report is buffered
    /// for [`Self::drain_scanouts`]. See [`Scanout`] for why present-return
    /// timestamps cannot answer the same question.
    pub const fn enable_scanout_trace(&mut self) {
        self.state.trace_scanout = true;
    }

    /// Take the buffered scanout reports, leaving tracing on and the buffer
    /// empty. `Vec` take — O(1); the caller writes them outside any lock.
    pub fn drain_scanouts(&mut self) -> Vec<Scanout> {
        std::mem::take(&mut self.state.scanouts)
    }

    /// Frames the compositor reported as `discarded` — composited but never
    /// scanned out. Cumulative.
    #[must_use]
    pub const fn discarded(&self) -> u64 {
        self.state.discarded
    }

    /// The POSIX clock id the compositor stamps presentations with, once it has
    /// said. `1` is `CLOCK_MONOTONIC`; anything else means the timestamps are
    /// NOT comparable to `Instant` and the analysis must say so rather than
    /// quietly mix domains.
    #[must_use]
    pub const fn clock_id(&self) -> Option<u32> {
        self.state.clock_id
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
        // v2.3.3 — dispatch FIRST, and unconditionally when scanout tracing is
        // on. This used to early-return on `settled`, which was correct while
        // the refresh estimate was the only output: no dispatch, no work. With
        // tracing on that return would strand every `presented` event in the
        // queue forever, so the feature would silently record nothing while
        // appearing to be enabled. Settling must terminate the ESTIMATE, not
        // the event pump.
        if self.settled && !self.state.trace_scanout {
            return None;
        }
        // Non-blocking: processes only what winit's socket reads have already
        // routed into this queue.
        let _ = self.queue.dispatch_pending(&mut self.state);
        if self.settled {
            return None;
        }
        let hz = crate::refresh_probe::estimate_hz_from_intervals(
            &self.state.samples_ms,
            PRESENTATION_SAMPLES,
        )?;
        self.settled = true;
        Some(hz)
    }
}
