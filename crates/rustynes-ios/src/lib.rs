//! `rustynes-ios` — the iOS / iPadOS platform host for `RustyNES`.
//!
//! ## What this crate is (and is not)
//!
//! The **typed control surface** over the core (load ROM, set input, run frame,
//! save/load state, movies, HD-pack, RA, netplay) is generated for **Swift** by
//! `UniFFI` from [`rustynes_mobile`] — the `SwiftUI` app drives the emulator
//! through that generated `NesController` class directly, exactly as the Android
//! Compose shell drives the generated Kotlin `NesController`. **One core-binding
//! layer, two platform shims.** This crate adds **only the thin, hot,
//! hand-rolled glue `UniFFI` cannot express**, reached over a small hand-written
//! C ABI the Swift bridging header declares:
//!
//! 1. handing a `CAMetalLayer` (an `MTKView`'s backing layer, created Swift-side)
//!    to `wgpu` so the existing shader / PAR / overscan render pipeline draws the
//!    `NES` image with an 8:7-PAR letterbox blit (Workstream B), and
//! 2. a `cpal` `CoreAudio` output sink fed by a lock-free ring (Workstream C);
//!    `AVAudioSession` category / activation / interruption handling stays
//!    Swift-side, cpal only owns the output stream.
//!
//! Everything iOS-specific is gated behind `#[cfg(target_os = "ios")]`, so on a
//! host build (`cargo build --workspace`) this crate is a near-empty shell that
//! exists only to be linted by host CI. The real static archive is produced by
//! `scripts/build-ios-xcframework.sh` against the `aarch64-apple-ios` /
//! `aarch64-apple-ios-sim` targets, packaged into an `.xcframework`, and linked
//! into the `SwiftUI` app. A Rust `staticlib` bundles all its rlib dependencies, so
//! this one archive carries `rustynes-mobile` + the byte-identical
//! `rustynes-core` as well.
//!
//! ## Determinism
//!
//! No emulation happens here — only presentation and the audio sink. The Metal
//! surface can be lost and recreated (scene background / lock / Stage-Manager
//! resize) while the core keeps running headless across the gap; no frame is
//! re-emulated, so the determinism contract is untouched (presentation-only,
//! exactly like the desktop occlusion watchdog and the Android `SurfaceView`
//! lifecycle). The iOS app consumes the same `Nes` snapshot/output the oracle
//! validates, so desktop <-> Android <-> iOS save portability and (once it lands)
//! netplay cross-play stay valid.

// This is a platform-host crate: the C-ABI render/audio seam requires
// `#[unsafe(no_mangle)]`/`extern "C"` symbols and raw `CAMetalLayer` pointers, so
// it carries `unsafe` (each site documented with a `// SAFETY:` note) — the same
// exemption `rustynes-cheevos`, `rustynes-frontend`, and `rustynes-android` take.
// Done here rather than in `Cargo.toml` so the crate keeps the workspace clippy
// (pedantic/nursery) gates while overriding only `unsafe_code = "warn"`.
#![allow(unsafe_code)]

/// The native core version string, surfaced to the `SwiftUI` About screen. Host-
/// safe; re-exported so a C-ABI getter and the host build share one source.
#[must_use]
pub fn core_version() -> String {
    rustynes_mobile::core_version()
}

/// The iOS wgpu->Metal render path (Workstream B). iOS-only; pulls in wgpu + the
/// `CAMetalLayer` handoff, so it never touches the host shell build.
#[cfg(target_os = "ios")]
mod gfx_metal;

/// The iOS cpal CoreAudio sink (Workstream C). iOS-only.
#[cfg(target_os = "ios")]
mod audio;

/// The C-ABI seam the SwiftUI bridging header declares and calls. iOS-only.
#[cfg(target_os = "ios")]
mod ffi;
