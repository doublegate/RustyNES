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

    /** The latest frame, consumed atomically by the render thread (`getAndSet(null)`)
     *  so a frame is never re-rendered and there's no torn read. */
    private val latestFrame = java.util.concurrent.atomic.AtomicReference<ByteArray?>(null)

    /** The active filter + its params as one immutable value, swapped atomically —
     *  no race between the UI thread (setter) and the render thread (reader). */
    private class FilterState(val filter: Int, val params: FloatArray)
    private val filterState = java.util.concurrent.atomic.AtomicReference(FilterState(0, FloatArray(4)))

    /** The latest palette-index frame + NTSC phase for the Bisqwit pass, consumed
     *  atomically; null unless that filter is active. */
    private class IndexFrame(val idx: ByteArray, val phase: Int)
    private val latestIndex = java.util.concurrent.atomic.AtomicReference<IndexFrame?>(null)

    @Volatile
    private var running = false
    private var thread: Thread? = null

    init {
        holder.addCallback(this)
    }

    /** Hand the render thread the latest RGBA frame (called from the emu loop). */
    fun submitFrame(fb: ByteArray) {
        latestFrame.set(fb)
    }

    /** Hand the render thread the latest palette-index frame + NTSC phase (only
     *  called while the Bisqwit filter is active). */
    fun submitIndexFrame(idx: ByteArray, phase: Int) {
        latestIndex.set(IndexFrame(idx, phase))
    }

    /** Set the video filter (0 none / 1 scanlines / 2 CRT / 3 NTSC) and its four
     *  shader params; applied on the render thread before the next frame. */
    fun setFilter(f: Int, params: FloatArray) {
        filterState.set(FilterState(f, params))
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (!NativeRenderer.ensureLoaded()) return
        synchronized(lock) { surfaceGone = false }
        // Only start a render thread if the previous one has actually exited — guards
        // the (rare) case where a prior `surfaceDestroyed` join timed out with the old
        // thread still alive (e.g. blocked in a long present), so we never run two.
        if (thread?.isAlive != true) {
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
        // Only clear the reference if it actually exited; if the join timed out (the
        // thread is still alive), keep it so `surfaceCreated` won't start a second.
        if (t?.isAlive != true) thread = null
    }

    private fun renderLoop() {
        var handle = 0L
        var appliedFilter: FilterState? = null
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
                        NativeRenderer.nativeInitSurface(surface!!, w, h)
                    } else {
                        NativeRenderer.nativeResize(handle, w, h)
                        handle
                    }
                    appliedFilter = null // force a re-apply on (re)create
                }
                // Apply the filter when it changes (identity compare on the atomic
                // value) or after a surface (re)create.
                val fs = filterState.get()
                if (handle != 0L && fs !== appliedFilter) {
                    val p = fs.params
                    NativeRenderer.nativeSetFilter(handle, fs.filter, p[0], p[1], p[2], p[3])
                    appliedFilter = fs
                }
                // Render only a NEW frame (atomic consume); idle otherwise. The wgpu
                // Fifo present blocks to vsync, so a present paces the thread.
                val fb = latestFrame.getAndSet(null)
                if (handle != 0L && fb != null) {
                    // Upload the Bisqwit index frame (if any) before presenting.
                    latestIndex.getAndSet(null)?.let {
                        NativeRenderer.nativeSetIndexFrame(handle, it.idx, it.phase)
                    }
                    NativeRenderer.nativeRender(handle, fb)
                } else {
                    Thread.sleep(2)
                }
            }
        } finally {
            if (handle != 0L) NativeRenderer.nativeDestroy(handle)
        }
    }
}
