package com.doublegate.rustynes

import android.content.Context
import android.os.SystemClock
import android.util.Base64
import com.google.android.gms.cast.framework.CastContext
import com.google.android.gms.cast.framework.CastOptions
import com.google.android.gms.cast.framework.CastSession
import com.google.android.gms.cast.framework.OptionsProvider
import com.google.android.gms.cast.framework.SessionManagerListener
import com.google.android.gms.cast.framework.SessionProvider
import org.json.JSONObject

/**
 * Experimental Chromecast (Cast Application Framework) SPECTATOR mirror (v1.8.7,
 * #38). PREPPED BEHIND A DEFAULT-OFF FLAG — nothing here runs (and no Cast UI is
 * shown) unless [BuildConfig.CHROMECAST_ENABLED] is true, which it is not until the
 * maintainer does the deferred ops (Cast Developer Console account + a registered
 * Receiver App ID + HTTPS hosting of the Web Receiver under android/cast-receiver/).
 *
 * Why this is a "spectator" cast, not the primary one:
 *  - Google removed the Cast Remote Display API (~2019). True Chromecast now needs
 *    an Android Sender (this class) plus a custom Web Receiver (HTML/JS the
 *    maintainer hosts over HTTPS and registers for an App ID).
 *  - Frames travel as Cast custom messages (the "urn:x-cast:" channel). The 256x240
 *    palette-index byte-plane is ~61 KB, which fits the Cast custom-message 64 KB
 *    cap, but the round trip is multi-hundred-ms — fine for a TV spectator view,
 *    NOT a play surface. We throttle to ~20-30fps to stay within message budget.
 *  - The low-latency cast path (the Presentation API in [CastManager]/Cast.kt) is
 *    UNCHANGED and remains the primary "Cast to TV". This is purely additive.
 *
 * Custom message namespace (must match the Web Receiver's CustomMessageListener):
 * "urn:x-cast:com.doublegate.rustynes.fb".
 */
object ChromecastConstants {
    /**
     * PLACEHOLDER Receiver App ID. The maintainer registers a real one at the Cast
     * Developer Console (https://cast.google.com/publish) — a "Custom Receiver"
     * pointing at the HTTPS-hosted android/cast-receiver/index.html — and pastes the
     * 8-hex-digit ID here. "RUSTYNES0" is intentionally not a valid registered ID,
     * so discovery returns nothing until that is done (belt-and-suspenders on top of
     * the default-off CHROMECAST_ENABLED flag).
     */
    const val APP_ID: String = "RUSTYNES0"

    /** Custom-message channel both the sender and Web Receiver use for frame data. */
    const val FRAME_NAMESPACE: String = "urn:x-cast:com.doublegate.rustynes.fb"

    /** Cast custom-message hard cap. A frame's base64 JSON must stay under this. */
    const val MESSAGE_CAP_BYTES: Int = 64 * 1024

    /** Spectator-mirror frame budget (~25fps): one frame every 40 ms. */
    const val MIN_FRAME_INTERVAL_MS: Long = 40L

    /** Native NES framebuffer dimensions (the sender's input plane). */
    const val NES_WIDTH: Int = 256
    const val NES_HEIGHT: Int = 240

    /** Cast plane dimensions: 2x down-sampled so the base64 message fits 64 KB. The
     *  Web Receiver upscales this to the TV with nearest-neighbour. */
    const val CAST_WIDTH: Int = 128
    const val CAST_HEIGHT: Int = 120
}

/**
 * The CAF [OptionsProvider] the manifest's OPTIONS_PROVIDER_CLASS_NAME meta-data
 * points at. The SDK instantiates this only when CastContext is initialized, which
 * happens exclusively behind the default-off [BuildConfig.CHROMECAST_ENABLED] gate.
 */
class RustyNesCastOptionsProvider : OptionsProvider {
    override fun getCastOptions(context: Context): CastOptions =
        CastOptions.Builder()
            // Maintainer: register a real App ID at the Cast Developer Console and
            // replace ChromecastConstants.APP_ID with it.
            .setReceiverApplicationId(ChromecastConstants.APP_ID)
            .build()

    override fun getAdditionalSessionProviders(context: Context): MutableList<SessionProvider>? =
        null
}

/**
 * Wraps the CAF SessionManager and streams palette-index frames to the connected
 * Web Receiver as throttled, 64 KB-capped custom messages.
 *
 * Every method is a cheap no-op when the flag is off or no Cast session is active,
 * so [MainActivity]'s emulation loop can call [sendFrame] unconditionally (it is
 * still wrapped in `if (BuildConfig.CHROMECAST_ENABLED)` there for compile-time
 * stripping in default builds).
 */
class ChromecastSender(context: Context) {
    private val appContext = context.applicationContext

    /** Null when the flag is off (so we never touch CastContext in default builds). */
    private val castContext: CastContext? =
        if (BuildConfig.CHROMECAST_ENABLED) {
            runCatching { CastContext.getSharedInstance(appContext) }.getOrNull()
        } else {
            null
        }

    /** The session we're streaming to, set/cleared by the listener. */
    @Volatile
    private var session: CastSession? = null

    private var lastSentMs: Long = 0L

    private val sessionListener = object : SessionManagerListener<CastSession> {
        override fun onSessionStarted(s: CastSession, sessionId: String) {
            session = s
        }

        override fun onSessionResumed(s: CastSession, wasSuspended: Boolean) {
            session = s
        }

        override fun onSessionEnded(s: CastSession, error: Int) {
            if (session === s) session = null
        }

        override fun onSessionSuspended(s: CastSession, reason: Int) {
            if (session === s) session = null
        }

        override fun onSessionStarting(s: CastSession) {}
        override fun onSessionStartFailed(s: CastSession, error: Int) {}
        override fun onSessionEnding(s: CastSession) {}
        override fun onSessionResuming(s: CastSession, sessionId: String) {}
        override fun onSessionResumeFailed(s: CastSession, error: Int) {}
    }

    /** True once a real session is connected (drives the "Casting…" label). */
    val isCasting: Boolean
        get() = session?.isConnected == true

    /** Register for session callbacks. No-op unless the flag is on. */
    fun register() {
        val ctx = castContext ?: return
        ctx.sessionManager.addSessionManagerListener(sessionListener, CastSession::class.java)
        // Pick up an already-running session (e.g. started from the system Cast UI).
        session = ctx.sessionManager.currentCastSession
    }

    /** Unregister. Safe to call when never registered. */
    fun unregister() {
        val ctx = castContext ?: return
        runCatching {
            ctx.sessionManager.removeSessionManagerListener(sessionListener, CastSession::class.java)
        }
        session = null
    }

    /**
     * Send one palette-index frame to the Web Receiver, throttled to the spectator
     * frame budget. [indexBytes] is the core's little-endian u16 index plane
     * (256x240 * 2 = ~123 KB raw); we base64 it into a small JSON envelope. If the
     * resulting message would exceed the 64 KB Cast cap we skip the frame (the
     * receiver simply keeps the previous one — a dropped spectator frame is benign).
     *
     * Cheap no-op when the flag is off or no session is connected.
     */
    fun sendFrame(indexBytes: ByteArray) {
        if (!BuildConfig.CHROMECAST_ENABLED) return
        val s = session ?: return
        if (!s.isConnected) return

        val now = SystemClock.uptimeMillis()
        if (now - lastSentMs < ChromecastConstants.MIN_FRAME_INTERVAL_MS) return

        // Size budget: the full 256x240 6-bit colour plane is 61,440 bytes, which
        // base64-encodes to ~81,920 chars — OVER the 64 KB Cast custom-message cap.
        // So we down-sample 2x to a 128x120 6-bit colour plane (15,360 bytes ->
        // ~20,480 base64), comfortably under cap. The Web Receiver upscales it with
        // nearest-neighbour to the TV — a faithful spectator picture. Emphasis (the
        // high byte of each u16 index) is dropped; it's rare and subtle.
        val colorPlane = toHalfResColorPlane(indexBytes)
        val b64 = Base64.encodeToString(colorPlane, Base64.NO_WRAP)
        val payload = JSONObject()
            .put("w", ChromecastConstants.CAST_WIDTH)
            .put("h", ChromecastConstants.CAST_HEIGHT)
            // "i6" = 6-bit palette-colour index plane, one byte per pixel.
            .put("fmt", "i6")
            .put("data", b64)
            .toString()

        // Belt-and-suspenders: respect the hard cap; skip rather than risk an
        // SDK-side rejection. (The 128x120 plane is always well under it.)
        if (payload.toByteArray(Charsets.UTF_8).size > ChromecastConstants.MESSAGE_CAP_BYTES) {
            return
        }

        runCatching { s.sendMessage(ChromecastConstants.FRAME_NAMESPACE, payload) }
        lastSentMs = now
    }

    /**
     * Down-sample the little-endian u16 index plane (256x240, 2 bytes/pixel) to a
     * 128x120, 1-byte-per-pixel 6-bit colour-index plane (every other pixel of every
     * other row), so the base64 message fits under the 64 KB Cast cap. Emphasis (the
     * high index byte) is dropped — fine for a spectator mirror.
     */
    private fun toHalfResColorPlane(indexBytes: ByteArray): ByteArray {
        val outW = ChromecastConstants.CAST_WIDTH
        val outH = ChromecastConstants.CAST_HEIGHT
        val srcW = ChromecastConstants.NES_WIDTH
        val out = ByteArray(outW * outH)
        var o = 0
        var y = 0
        while (y < outH) {
            // Source row 2*y; each pixel is 2 bytes, so the row's byte offset is
            // (2*y) * srcW * 2; the low colour byte of src pixel 2*x is at +(2*x)*2.
            val rowBase = (2 * y) * srcW * 2
            var x = 0
            while (x < outW) {
                val src = rowBase + (2 * x) * 2 // low byte of the u16 index
                out[o] = (indexBytes[src].toInt() and 0x3F).toByte()
                o += 1
                x += 1
            }
            y += 1
        }
        return out
    }
}
