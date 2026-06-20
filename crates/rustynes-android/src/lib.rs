//! `rustynes-android` — the Android platform host for `RustyNES`.
//!
//! ## What this crate is (and is not)
//!
//! The **typed control surface** over the core (load ROM, set input, run frame,
//! save/load state) is generated for Kotlin by `UniFFI` from
//! [`rustynes_mobile`] — the Compose shell drives the emulator through that
//! generated `NesController` class directly. This crate adds **only the thin,
//! hot, hand-rolled glue `UniFFI` cannot express**:
//!
//! 1. handing a native surface handle (`ANativeWindow`, obtained from an
//!    `android.view.Surface`) to `wgpu` so the existing shader/PAR/overscan
//!    render pipeline draws the `NES` image onto a `SurfaceView` (Workstream B),
//!    and
//! 2. the audio sink lifecycle (Workstream C).
//!
//! Everything Android-specific is gated behind `#[cfg(target_os = "android")]`,
//! so on a host build (`cargo build --workspace`) this crate is a near-empty
//! shell that exists only to be linted by host CI. The real `.so` is produced
//! by `cargo-ndk` against the `aarch64-linux-android` / `x86_64-linux-android`
//! targets and loaded by the JVM via `System.loadLibrary("rustynes_android")`.
//!
//! ## Determinism
//!
//! No emulation happens here — only presentation and the audio sink. The render
//! surface can be lost and recreated (rotate/background/lock) while the core
//! keeps running headless across the gap; no frame is re-emulated, so the
//! determinism contract is untouched (presentation-only, exactly like the
//! desktop occlusion watchdog).

// This is a platform-host crate: the `android_main` entry point and the JNI
// surface/audio glue require `#[no_mangle]`/`extern` symbols and raw NDK
// handles, so it carries `unsafe` (each site documented with a `// SAFETY:`
// note) — the same exemption `rustynes-cheevos` and `rustynes-frontend` take.
// Done here rather than in `Cargo.toml` so the crate keeps the workspace clippy
// (pedantic/nursery) gates while overriding only `unsafe_code = "warn"`.
#![allow(unsafe_code)]

/// The native core version string, surfaced to the shell's About screen. Host-
/// safe; re-exported so a JNI getter and the spike can share one source.
#[must_use]
pub fn core_version() -> String {
    rustynes_mobile::core_version()
}

#[cfg(target_os = "android")]
mod android {
    //! Android-only entry points: logcat init, the JNI surface/audio seam, and
    //! the beta.1 winit+wgpu+egui spike's `android_main`.

    use android_activity::AndroidApp;

    /// Initialise logcat routing for the `log` facade. Idempotent — safe to call
    /// from both `android_main` (spike) and `JNI_OnLoad` (Compose host).
    fn init_logging() {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("RustyNES"),
        );
    }

    /// beta.1 spike entry point — Option (a): compile the existing
    /// winit + wgpu + egui frontend `App` to Android via `android-activity`,
    /// proving the byte-identical core renders and sounds on real ARM. winit
    /// 0.30's `resumed`/`suspended` already models the surface-loss contract
    /// (Workstream B). The shippable app graduates (a) -> (c) by lifting the
    /// wgpu surface + core loop into the JNI seam below and replacing the egui
    /// chrome with the Jetpack Compose shell.
    ///
    /// # Safety
    /// Called by the `game-activity` glue as the process entry point.
    // SAFETY: `android_main` is the well-known symbol the `game-activity`
    // native glue resolves and invokes exactly once on the dedicated app
    // thread; `#[no_mangle]` exports it under that fixed name. No Rust caller
    // ever references it, so there is no duplicate-symbol hazard within the
    // crate, and the `AndroidApp` handed in is owned for the call's duration.
    #[unsafe(no_mangle)]
    fn android_main(app: AndroidApp) {
        init_logging();
        log::info!(
            "RustyNES android host starting (core {})",
            super::core_version()
        );
        // The winit event loop reusing `rustynes-frontend`'s `App` is wired in
        // beta.1's spike build; the shippable Option (c) path drives the core
        // through the JNI seam (`jni_glue`) instead. See `docs/android.md`.
        let _ = app;
    }

    /// The JNI surface/audio seam (Workstream B/C). The Compose shell calls into
    /// these from the `SurfaceHolder.Callback` (surface lifecycle) and the audio
    /// focus listener; they hand the `ANativeWindow` to wgpu and own the native
    /// emulation thread. Implemented incrementally across beta.2 (surface) and
    /// beta.3 (audio); the module is the stable binding seam the Kotlin side
    /// links against.
    mod jni_glue {
        // Populated in beta.2/beta.3. Kept as a named module so the Kotlin
        // `external fun` declarations have a stable symbol home.
    }
}
