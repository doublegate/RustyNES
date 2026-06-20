package com.doublegate.rustynes

import android.app.Presentation
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.hardware.display.DisplayManager
import android.os.Bundle
import android.view.Display
import android.view.Gravity
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/**
 * Casts the GAMEPLAY ONLY to an external display (item 1).
 *
 * Uses Android's [Presentation] API on a [DisplayManager] presentation-category
 * display (HDMI, Chromecast/Miracast, a wireless display, or the "Simulate
 * secondary displays" developer option). The phone keeps the on-screen controller;
 * the TV shows just the NES picture. This is NOT screen mirroring — it's a second,
 * independent surface that only ever receives the framebuffer.
 *
 * Note: Google removed the Cast Remote Display API (~2019) and Android has no
 * native AirPlay, so the Presentation API is the supported route for sending a
 * dedicated game surface to a connected/cast display (ADR: v1.8.3 casting).
 */
class CastManager(private val context: Context) {
    private val appContext = context.applicationContext
    private val dm = appContext.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager

    /** True while at least one presentation-capable external display is connected. */
    var available by mutableStateOf(false)
        private set

    /** True while we're actively presenting the gameplay to that display. */
    var casting by mutableStateOf(false)
        private set

    /** Human-readable name of the display we're casting to (for the UI). */
    var displayName by mutableStateOf<String?>(null)
        private set

    private var presentation: GamePresentation? = null

    private val listener = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) = refresh()
        override fun onDisplayRemoved(displayId: Int) {
            // If the display we were casting to vanished, tear down cleanly.
            if (presentation?.display?.displayId == displayId) stop()
            refresh()
        }
        override fun onDisplayChanged(displayId: Int) = refresh()
    }

    fun register() {
        dm.registerDisplayListener(listener, null)
        refresh()
    }

    fun unregister() {
        runCatching { dm.unregisterDisplayListener(listener) }
        stop()
    }

    private fun presentationDisplay(): Display? =
        dm.getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION).firstOrNull()

    private fun refresh() {
        val d = presentationDisplay()
        available = d != null
        if (d == null) stop()
    }

    /** Toggle casting on/off (no-op if no external display is connected). */
    fun toggle() {
        if (casting) stop() else start()
    }

    private fun start() {
        val d = presentationDisplay() ?: return
        // A Presentation is a Dialog, so it needs a UI (Activity) context with a
        // valid window token — the application context throws BadTokenException.
        val p = GamePresentation(context, d)
        // Keep our state in sync if the system dismisses it (e.g. the display
        // disconnects, or another presentation takes the display).
        p.setOnDismissListener {
            if (presentation === p) {
                presentation = null
                casting = false
                displayName = null
            }
        }
        runCatching { p.show() }
            .onSuccess {
                presentation = p
                casting = true
                displayName = d.name
            }
            .onFailure { presentation = null }
    }

    fun stop() {
        presentation?.let { runCatching { it.dismiss() } }
        presentation = null
        casting = false
        displayName = null
    }

    /**
     * Push the latest framebuffer to the TV. Must be called on the main thread
     * (the emulation loop already publishes the Compose frame from the main
     * dispatcher, so this rides along with it). Cheap no-op when not casting.
     */
    fun pushFrame(bitmap: Bitmap) {
        presentation?.update(bitmap)
    }
}

/** A black, letterboxed full-screen surface on the external display that shows the
 *  NES picture and nothing else. */
private class GamePresentation(context: Context, display: Display) :
    Presentation(context, display) {

    private lateinit var image: ImageView
    private var boundBitmap: Bitmap? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = FrameLayout(context).apply { setBackgroundColor(Color.BLACK) }
        image = ImageView(context).apply {
            // FIT_CENTER letterboxes the 8:7-ish NES picture on a 16:9 TV.
            scaleType = ImageView.ScaleType.FIT_CENTER
        }
        root.addView(
            image,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
                Gravity.CENTER,
            ),
        )
        setContentView(root)
    }

    fun update(bitmap: Bitmap) {
        if (!::image.isInitialized) return
        // The emulation loop reuses one mutable Bitmap, so bind it once and then
        // just invalidate each frame (no per-frame drawable re-wrap on the UI thread).
        if (boundBitmap !== bitmap) {
            image.setImageBitmap(bitmap)
            boundBitmap = bitmap
        }
        image.invalidate()
    }
}
