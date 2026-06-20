package com.doublegate.rustynes

import android.view.Surface

/**
 * JNI binding to the `rustynes-android` native wgpu renderer (v1.8.4, Workstream B).
 *
 * The four entry points mirror a [android.view.SurfaceHolder.Callback] lifecycle:
 * create the wgpu surface from a [Surface], resize it, render one RGBA frame, and
 * destroy it. The returned `Long` is an opaque handle (a boxed native renderer
 * pointer); `0` means initialization failed (the caller should fall back to the
 * Compose `Bitmap` path).
 *
 * Presentation only — the native side never emulates, so determinism is untouched.
 */
object NativeRenderer {
    @Volatile
    private var loaded = false

    /** Load `librustynes_android.so` once; returns false if it isn't present
     *  (e.g. an ABI without the native renderer) so callers can fall back. */
    fun ensureLoaded(): Boolean {
        if (loaded) return true
        return synchronized(this) {
            if (!loaded) {
                runCatching { System.loadLibrary("rustynes_android") }
                    .onSuccess { loaded = true }
                    .onFailure { android.util.Log.w("RustyNES", "native renderer unavailable: ${it.message}") }
            }
            loaded
        }
    }

    /** Create the renderer for `surface` at `width`×`height`; 0 on failure. */
    external fun nativeInitSurface(surface: Surface, width: Int, height: Int): Long

    /** Reconfigure for a new surface size. */
    external fun nativeResize(handle: Long, width: Int, height: Int)

    /** Upload + present one 256×240 RGBA8 frame (`fb` = 245_760 bytes). */
    external fun nativeRender(handle: Long, fb: ByteArray)

    /** Set the video filter: 0 = none, 1 = scanlines, 2 = CRT. */
    external fun nativeSetFilter(handle: Long, filter: Int)

    /** Drop the renderer (releases the wgpu surface before the ANativeWindow). */
    external fun nativeDestroy(handle: Long)
}
