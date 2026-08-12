// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.3.3 — the platform-neutral face of the compositor refresh source.
//!
//! [`crate::wayland_presentation`] can only exist where winit compiles its
//! Wayland backend. Rather than repeat that (long) target predicate at every
//! use site in `app.rs` — four of them, and each one a place a future platform
//! could be forgotten — the whole of it is stated **once**, here, and the rest
//! of the frontend sees a type that is always present on native.
//!
//! On a Wayland-capable target this re-exports the real implementation. On
//! Windows, macOS, iOS, Android and Redox it is a stub whose constructor
//! returns `None`, which is exactly what the real one does off Wayland — so
//! the caller has a single code path and no `cfg` of its own. Those platforms
//! report a refresh through the windowing API anyway and never reach this
//! fallback.

/// The predicate below is winit's own Wayland-backend `cfg`, verbatim. Keeping
/// it identical is what guarantees there is a `wl_surface` to attach to
/// wherever the real implementation is compiled.
#[cfg(all(
    unix,
    not(any(
        target_os = "redox",
        target_family = "wasm",
        target_os = "android",
        target_os = "ios",
        target_os = "macos"
    ))
))]
pub use crate::wayland_presentation::PresentationClock;

/// Stub for platforms with no Wayland surface. Constructing one always fails,
/// so the pacing code takes the declared-refresh path unchanged.
#[cfg(not(all(
    unix,
    not(any(
        target_os = "redox",
        target_family = "wasm",
        target_os = "android",
        target_os = "ios",
        target_os = "macos"
    ))
)))]
#[derive(Debug)]
pub struct PresentationClock {
    /// Uninhabited in practice — [`PresentationClock::new`] never returns a
    /// value — but a field keeps the type from being constructible elsewhere
    /// by mistake.
    _never: core::convert::Infallible,
}

#[cfg(not(all(
    unix,
    not(any(
        target_os = "redox",
        target_family = "wasm",
        target_os = "android",
        target_os = "ios",
        target_os = "macos"
    ))
)))]
impl PresentationClock {
    /// Always `None`: there is no Wayland compositor to ask.
    #[must_use]
    pub const fn new(_window: &std::sync::Arc<winit::window::Window>) -> Option<Self> {
        None
    }

    /// Unreachable — no value of this type can exist.
    pub const fn request_feedback(&mut self) {}

    /// Unreachable — no value of this type can exist.
    #[must_use]
    pub const fn poll(&mut self) -> Option<f64> {
        None
    }
}
