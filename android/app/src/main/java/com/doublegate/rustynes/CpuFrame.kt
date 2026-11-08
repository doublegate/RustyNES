package com.doublegate.rustynes

/**
 * v3.7.0 (`T-ANDROID-GPU-FRAMESKIP`, audit AND-02's deferred half): the CPU copy of
 * the picture, packed from the core's RGBA8 frame into ARGB_8888 only when something
 * reads it.
 *
 * On the GPU render path (`NesSurfaceView`, opt-in) the raw RGBA frame goes straight
 * to wgpu, and the 61,440-pixel RGBA -> ARGB pack the Bitmap path needs was still
 * done every frame, because three things read the CPU copy: a screenshot, a clip
 * being recorded, and the external display. A v3.2.0 draft skipped the pack on the
 * GPU path and left all three showing a stale frame. This class keeps them correct:
 *
 * - a consumer that reads EVERY frame (the Bitmap path itself, a clip recording, an
 *   external display) makes the loop publish eagerly, exactly as before;
 * - otherwise the frame is only remembered ([publish] with `eager = false`, a
 *   reference to the frame `runFrame` already allocated), and a one-off reader
 *   (the screenshot) calls [materialize] first, which packs the remembered frame.
 *
 * So every read sees the frame the old always-pack loop would have shown at that
 * moment; `CpuFrameTest` checks that, byte for byte, over random frame sequences.
 * Whether skipping the pack is measurably faster is a device question (it is not
 * claimed here; see the mobile run sheet).
 *
 * Thread-safety: the loop publishes from `Dispatchers.Default` while a screenshot
 * may materialize on the main thread, so both are `@Synchronized`.
 */
class CpuFrame(pixelCount: Int) {
    /** The packed ARGB_8888 picture, current after an eager [publish] or a [materialize]. */
    val pixels = IntArray(pixelCount)

    /** The newest frame not yet packed into [pixels], or null when [pixels] is current. */
    private var pending: ByteArray? = null

    /** Publish this frame's RGBA8 bytes: packed now when [eager], else remembered. */
    @Synchronized
    fun publish(rgba: ByteArray, eager: Boolean) {
        if (eager) {
            packRgbaToArgb(rgba, pixels)
            pending = null
        } else {
            pending = rgba
        }
    }

    /** Pack a remembered frame into [pixels]. Returns whether [pixels] changed. */
    @Synchronized
    fun materialize(): Boolean {
        val p = pending ?: return false
        packRgbaToArgb(p, pixels)
        pending = null
        return true
    }

    /** Write [pixels] directly (the netplay index path, the HD-pack fallback),
     *  dropping any remembered frame so a later [materialize] cannot overwrite it. */
    @Synchronized
    fun overwrite(fill: (IntArray) -> Unit) {
        pending = null
        fill(pixels)
    }

    /** Whether a frame is waiting to be packed. */
    @get:Synchronized
    val isDeferred: Boolean get() = pending != null

    companion object {
        /**
         * Whether the loop must pack every frame: always on the Bitmap path (it IS
         * the picture), and on the GPU path while a clip records or the external
         * display mirrors the game (both read every frame).
         */
        fun eagerEveryFrame(gpuPath: Boolean, clipRecording: Boolean, externalDisplay: Boolean): Boolean =
            !gpuPath || clipRecording || externalDisplay
    }
}

/** Convert the core's RGBA8 framebuffer into packed ARGB_8888 pixels. */
internal fun packRgbaToArgb(rgba: ByteArray, out: IntArray) {
    var i = 0
    var p = 0
    while (p < out.size) {
        val r = rgba[i].toInt() and 0xFF
        val g = rgba[i + 1].toInt() and 0xFF
        val b = rgba[i + 2].toInt() and 0xFF
        out[p] = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
        i += 4
        p += 1
    }
}
