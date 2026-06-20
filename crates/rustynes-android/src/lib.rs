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

/// The Android wgpu render path (Workstream B). Android-only; pulls in wgpu + the
/// NDK, so it never touches the host shell build.
#[cfg(target_os = "android")]
mod gfx;

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
        //! The Kotlin `NativeRenderer` object's `external fun`s land here as the
        //! `SurfaceView`'s `SurfaceHolder.Callback` drives the surface lifecycle:
        //! init (Surface → `ANativeWindow` → wgpu), resize, render one frame, and
        //! destroy. The handle is a boxed [`AndroidGfx`] pointer as a `jlong`.

        use crate::gfx::AndroidGfx;
        use jni::JNIEnv;
        use jni::objects::{JByteArray, JObject};
        use jni::sys::{jfloat, jint, jlong};
        use ndk::native_window::NativeWindow;

        /// `NativeRenderer.nativeInitSurface(surface, w, h): Long` — returns an
        /// opaque renderer handle, or 0 on failure.
        ///
        /// # Safety
        /// Invoked by the JVM; `surface` is a live `android.view.Surface`.
        #[unsafe(no_mangle)]
        pub extern "C" fn Java_com_doublegate_rustynes_NativeRenderer_nativeInitSurface(
            env: JNIEnv,
            _this: JObject,
            surface: JObject,
            width: jint,
            height: jint,
        ) -> jlong {
            // SAFETY: `surface` is a valid Surface jobject for this call;
            // `ANativeWindow_fromSurface` returns a new owned reference that
            // `NativeWindow` takes ownership of (released on drop).
            let window = unsafe {
                NativeWindow::from_surface(env.get_raw().cast(), surface.as_raw().cast())
            };
            let Some(window) = window else {
                log::error!("nativeInitSurface: ANativeWindow_fromSurface returned null");
                return 0;
            };
            match AndroidGfx::new(window, width.max(0) as u32, height.max(0) as u32) {
                Ok(gfx) => Box::into_raw(Box::new(gfx)) as jlong,
                Err(e) => {
                    log::error!("nativeInitSurface failed: {e}");
                    0
                }
            }
        }

        /// `NativeRenderer.nativeResize(handle, w, h)`.
        ///
        /// # Safety
        /// `handle` must be a live value returned by `nativeInitSurface`.
        #[unsafe(no_mangle)]
        pub extern "C" fn Java_com_doublegate_rustynes_NativeRenderer_nativeResize(
            _env: JNIEnv,
            _this: JObject,
            handle: jlong,
            width: jint,
            height: jint,
        ) {
            if handle == 0 {
                return;
            }
            // SAFETY: `handle` is a live `Box<AndroidGfx>` pointer (the Kotlin side
            // only calls this between init and destroy on the render thread).
            let gfx = unsafe { &mut *(handle as *mut AndroidGfx) };
            gfx.resize(width.max(0) as u32, height.max(0) as u32);
        }

        /// `NativeRenderer.nativeRender(handle, fb)` — upload + present one 256×240
        /// RGBA frame.
        ///
        /// # Safety
        /// `handle` must be live; `fb` a Java `byte[]` of the framebuffer.
        #[unsafe(no_mangle)]
        pub extern "C" fn Java_com_doublegate_rustynes_NativeRenderer_nativeRender(
            env: JNIEnv,
            _this: JObject,
            handle: jlong,
            fb: JByteArray,
        ) {
            if handle == 0 {
                return;
            }
            let Ok(bytes) = env.convert_byte_array(&fb) else {
                return;
            };
            // SAFETY: live handle (see `nativeResize`).
            let gfx = unsafe { &mut *(handle as *mut AndroidGfx) };
            gfx.render(&bytes);
        }

        /// `NativeRenderer.nativeSetFilter(handle, filter, p0..p3)` — 0 none /
        /// 1 scanlines / 2 CRT / 3 NTSC, plus the shader `params` (filter-specific:
        /// Scanlines = [intensity, _, rows]; CRT = [intensity, mask, rows];
        /// NTSC = [saturation, sharpness, tint, phase]).
        ///
        /// # Safety
        /// `handle` must be a live value returned by `nativeInitSurface`.
        #[unsafe(no_mangle)]
        pub extern "C" fn Java_com_doublegate_rustynes_NativeRenderer_nativeSetFilter(
            _env: JNIEnv,
            _this: JObject,
            handle: jlong,
            filter: jint,
            p0: jfloat,
            p1: jfloat,
            p2: jfloat,
            p3: jfloat,
        ) {
            if handle == 0 {
                return;
            }
            // SAFETY: live handle (see `nativeResize`).
            let gfx = unsafe { &mut *(handle as *mut AndroidGfx) };
            gfx.set_filter(filter.max(0) as u8, [p0, p1, p2, p3]);
        }

        /// `NativeRenderer.nativeDestroy(handle)` — drop the renderer (releases the
        /// wgpu surface before the `ANativeWindow`).
        ///
        /// # Safety
        /// `handle` must be a live value from `nativeInitSurface`, not used after.
        #[unsafe(no_mangle)]
        pub extern "C" fn Java_com_doublegate_rustynes_NativeRenderer_nativeDestroy(
            _env: JNIEnv,
            _this: JObject,
            handle: jlong,
        ) {
            if handle == 0 {
                return;
            }
            // SAFETY: reclaim the `Box` created in `nativeInitSurface`; the Kotlin
            // side nulls its handle immediately after this call.
            drop(unsafe { Box::from_raw(handle as *mut AndroidGfx) });
        }
    }
}
