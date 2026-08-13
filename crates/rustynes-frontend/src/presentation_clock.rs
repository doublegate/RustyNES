// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.3.3 — the platform-neutral face of the compositor refresh source.
//!
//! [`crate::wayland_presentation`] can only exist where winit compiles its
//! Wayland backend. Rather than repeat that (long) target predicate at every
//! use site in `app.rs` — four of them, and each one a place a future platform
//! could be forgotten — this module absorbs it, and the rest of the frontend
//! sees a type that is always present on native.
//!
//! # Where the predicate is written
//!
//! Not, strictly, in one place — Rust has no stable `cfg` alias, so it appears
//! four times and they must be changed together:
//!
//! 1. here, positively, gating the re-export;
//! 2. here, negated, gating the stub;
//! 3. `lib.rs`, gating `pub mod wayland_presentation`;
//! 4. `Cargo.toml`, as the `[target.'cfg(...)'.dependencies]` expression.
//!
//! This module is nonetheless the authoritative statement, and the others
//! point back at it. The saving is real but narrower than "stated once": the
//! four sites are fixed, whereas use sites in `app.rs` would grow with every
//! caller. Drift also fails LOUDLY rather than silently — the Rust copies
//! disagreeing yields either a duplicate `PresentationClock` definition or
//! none, and the Cargo copy disagreeing yields a missing-crate error.
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
