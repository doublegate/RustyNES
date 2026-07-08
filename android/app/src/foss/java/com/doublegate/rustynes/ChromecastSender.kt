// FOSS-FLAVOR SOURCE SET (v2.0.1, ADR 0025). No-op stand-in for the Cast-Application-
// Framework `ChromecastSender` (the real one is in `src/play/.../ChromecastSender.kt`).
// Links NO `com.google.android.gms.cast.*`, and there is NO `RustyNesCastOptionsProvider`
// here (the FOSS manifest omits the Cast OPTIONS_PROVIDER meta-data). The CAF spectator-
// mirror is a Play-Services feature, so it is simply absent in FOSS. NOTE: the AOSP
// Presentation-API `CastManager` (Cast.kt) is separate and stays in `src/main`, so
// same-device external-display mirroring still works in FOSS.
package com.doublegate.rustynes

import android.content.Context
import android.view.View

/** No-op FOSS Chromecast sender: never casts; every frame push is dropped. */
@Suppress("UNUSED_PARAMETER")
class ChromecastSender(context: Context) {

    /** Never casting in FOSS. */
    val isCasting: Boolean = false

    fun register() {}

    fun unregister() {}

    fun sendFrame(indexBytes: ByteArray) {}

    /**
     * FOSS stand-in for the CAF `MediaRouteButton`: a blank, inert [View]. The FOSS
     * flavor links no `androidx.mediarouter` / `com.google.android.gms.cast`, and the
     * only caller in `src/main` is gated behind the default-off
     * `BuildConfig.CHROMECAST_ENABLED`, so this view is never actually shown here.
     * Returning a real (empty) `View` keeps the shared `AndroidView` factory's
     * non-null contract without leaking any proprietary Cast type into `src/main`.
     */
    fun mediaRouteButton(context: Context): View = View(context)
}
