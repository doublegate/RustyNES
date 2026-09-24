package com.doublegate.rustynes

import android.content.Context
import android.os.Build
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.locks.LockSupport

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
 *
 * v2.7.4 (frontend audit AND-01, AND-08) — each render thread is a [Worker] with
 * its OWN stop token. Before, one shared `running` flag served every thread, and
 * `surfaceCreated` refused to start a new one while a previous thread was still
 * alive (a `surfaceDestroyed` join that timed out on a slow present): the view then
 * stayed black until the next background/foreground cycle. Now a new surface always
 * gets a new worker; the new worker joins its predecessor before its first native
 * call, so there is still only ever one native user of the renderer (the thread
 * invariant `rustynes-android`'s JNI module documents), and only the CURRENT worker
 * may consume a pending surface, so a retiring one cannot steal it. Idle waits park
 * the thread instead of sleeping 2 ms in a loop; every producer unparks it.
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

    /** A render thread and its own stop token (see the class docs). */
    private class Worker {
        val running = AtomicBoolean(true)
        lateinit var thread: Thread
    }

    /** The current render worker; written only on the UI thread, under [lock]. */
    @Volatile
    private var worker: Worker? = null

    /** Wake the render thread early (a producer changed something it acts on). */
    private fun wake() {
        worker?.thread?.let(LockSupport::unpark)
    }

    init {
        holder.addCallback(this)
    }

    /** Hand the render thread the latest RGBA frame (called from the emu loop). */
    fun submitFrame(fb: ByteArray) {
        latestFrame.set(fb)
        wake()
    }

    /** Hand the render thread the latest palette-index frame + NTSC phase (only
     *  called while the Bisqwit filter is active). */
    fun submitIndexFrame(idx: ByteArray, phase: Int) {
        latestIndex.set(IndexFrame(idx, phase))
        wake()
    }

    /** Set the video filter (0 none / 1 scanlines / 2 CRT / 3 NTSC / 4 Bisqwit) and
     *  its four shader params; applied on the render thread before the next frame. */
    fun setFilter(f: Int, params: FloatArray) {
        filterState.set(FilterState(f, params))
        wake()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (!NativeRenderer.ensureLoaded()) return
        val next = Worker()
        val prev: Worker?
        synchronized(lock) {
            surfaceGone = false
            prev = worker
            // Retire the previous worker (if it outlived its surface) before the
            // new one can be current, so it can no longer consume surface state.
            prev?.running?.set(false)
            // The thread is assigned BEFORE the worker is published: `wake()`
            // reads `worker?.thread` from the emulation thread, and reading an
            // unassigned `lateinit` there throws.
            next.thread = Thread({ renderLoop(next, prev) }, "nes-gl")
            worker = next
        }
        prev?.thread?.let(LockSupport::unpark)
        next.thread.start()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        synchronized(lock) {
            pendingSurface = holder.surface
            pendingWidth = width
            pendingHeight = height
            sizeDirty = true
            surfaceGone = false
        }
        wake()
        // v1.8.8 "Atlas" (Workstream J): declare the exact NES NTSC frame rate so the
        // system picks/keeps the closest display mode and paces correctly. Without this,
        // Android 15+ defaults games to 60 Hz, but the NES master clock yields ~60.0988
        // fps — the ~0.1 Hz mismatch causes a periodic dropped/duplicated frame (judder).
        // FRAME_RATE_COMPATIBILITY_DEFAULT lets the system honor it without forcing a
        // mode switch on panels that can't hit it exactly. API 30+ only (no-op below).
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            runCatching {
                holder.surface.setFrameRate(
                    60.0988f,
                    Surface.FRAME_RATE_COMPATIBILITY_DEFAULT,
                )
            }
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        val w: Worker?
        synchronized(lock) {
            surfaceGone = true
            w = worker
            w?.running?.set(false)
        }
        // Wait (bounded) for the render thread to release the native surface -- the
        // Surface handed to wgpu is invalid the moment this returns. If the join
        // times out (a present stuck on the dying surface), the thread finishes on
        // its own: its token is already off, and the next worker joins it before
        // touching native code.
        w?.thread?.let {
            LockSupport.unpark(it)
            runCatching { it.join(800) }
        }
    }

    private fun renderLoop(me: Worker, prev: Worker?) {
        // One native user at a time: let a predecessor that outlived its surface
        // finish (and destroy its handle) before this thread creates its own.
        prev?.thread?.let { runCatching { it.join() } }
        var handle = 0L
        var appliedFilter: FilterState? = null
        try {
            while (me.running.get()) {
                var surface: Surface? = null
                var w = 0
                var h = 0
                var resize = false
                var gone: Boolean
                synchronized(lock) {
                    // Only the current worker may consume surface state; a retired
                    // one (its token cleared under this same lock) stops here.
                    if (!me.running.get()) return
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
                        NativeRenderer.nativeInitSurface(surface, w, h)
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
                    // Park until a producer wakes us (a frame, a filter, a surface
                    // change); the timeout is only a safety net. Replaces a 2 ms
                    // sleep-poll -- up to 500 wakeups a second while idle (AND-08).
                    LockSupport.parkNanos(this, IDLE_PARK_NANOS)
                }
            }
        } finally {
            if (handle != 0L) NativeRenderer.nativeDestroy(handle)
        }
    }

    private companion object {
        /** Idle safety-net wake: one display frame. */
        const val IDLE_PARK_NANOS = 16_666_667L
    }
}
