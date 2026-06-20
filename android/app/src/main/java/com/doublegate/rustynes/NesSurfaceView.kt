package com.doublegate.rustynes

import android.content.Context
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView

/**
 * A [SurfaceView] that draws the NES picture via the native wgpu renderer
 * ([NativeRenderer]) — the v1.8.4 GPU render path (Workstream B), an alternative
 * to the Compose `Bitmap` blit.
 *
 * ALL native calls (init / resize / render / destroy) run on one dedicated render
 * thread, so the non-thread-safe wgpu objects are never touched concurrently. The
 * UI-thread [SurfaceHolder.Callback] only posts intent (surface available / size /
 * gone); the render thread acts on it. On `surfaceDestroyed` we block until the
 * native surface is torn down, because the [Surface] is invalid once that returns
 * — this is the surface-loss lifecycle the emulator keeps running headless across.
 *
 * The emulator loop feeds frames via [submitFrame] (the raw 256×240 RGBA bytes
 * from `NesController.runFrame()`); the render thread presents the latest one each
 * vsync (wgpu Fifo paces it). Presentation only: determinism is untouched.
 */
class NesSurfaceView(context: Context) : SurfaceView(context), SurfaceHolder.Callback {
    private val lock = Any()
    private var pendingSurface: Surface? = null
    private var pendingWidth = 0
    private var pendingHeight = 0
    private var sizeDirty = false
    private var surfaceGone = false

    @Volatile
    private var latestFrame: ByteArray? = null

    @Volatile
    private var filter = 0

    @Volatile
    private var filterDirty = false

    @Volatile
    private var running = false
    private var thread: Thread? = null

    init {
        holder.addCallback(this)
    }

    /** Hand the render thread the latest RGBA frame (called from the emu loop). */
    fun submitFrame(fb: ByteArray) {
        latestFrame = fb
    }

    /** Set the video filter (0 = none, 1 = scanlines, 2 = CRT); applied on the
     *  render thread before the next frame. */
    fun setFilter(f: Int) {
        filter = f
        filterDirty = true
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (!NativeRenderer.ensureLoaded()) return
        synchronized(lock) { surfaceGone = false }
        if (thread == null) {
            running = true
            thread = Thread(::renderLoop, "nes-gl").apply { start() }
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        synchronized(lock) {
            pendingSurface = holder.surface
            pendingWidth = width
            pendingHeight = height
            sizeDirty = true
            surfaceGone = false
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        synchronized(lock) { surfaceGone = true }
        // Block until the render thread has released the native surface — the
        // Surface handed to wgpu is invalid the moment this returns.
        val t = thread
        running = false
        runCatching { t?.join(800) }
        thread = null
    }

    private fun renderLoop() {
        var handle = 0L
        try {
            while (running) {
                var surface: Surface? = null
                var w = 0
                var h = 0
                var resize = false
                var gone: Boolean
                synchronized(lock) {
                    gone = surfaceGone
                    if (sizeDirty) {
                        surface = pendingSurface
                        w = pendingWidth
                        h = pendingHeight
                        sizeDirty = false
                        resize = true
                    }
                }
                if (gone) break
                if (resize && surface != null) {
                    handle = if (handle == 0L) {
                        val newHandle = NativeRenderer.nativeInitSurface(surface!!, w, h)
                        if (newHandle != 0L) {
                            NativeRenderer.nativeSetFilter(newHandle, filter)
                            filterDirty = false
                        }
                        newHandle
                    } else {
                        NativeRenderer.nativeResize(handle, w, h)
                        handle
                    }
                }
                if (filterDirty && handle != 0L) {
                    NativeRenderer.nativeSetFilter(handle, filter)
                    filterDirty = false
                }
                val fb = latestFrame
                if (handle != 0L && fb != null) {
                    // wgpu Fifo present blocks to vsync, so this paces the thread.
                    NativeRenderer.nativeRender(handle, fb)
                } else {
                    Thread.sleep(4)
                }
            }
        } finally {
            if (handle != 0L) NativeRenderer.nativeDestroy(handle)
        }
    }
}
