// FOSS-FLAVOR SOURCE SET (v2.0.1, ADR 0025). No-op stand-in for the in-app-update /
// in-app-review `PlayUpdatesManager` (the real one is in `src/play/.../PlayUpdates.kt`).
// Links NO `com.google.android.play.core.*`. A sideload / F-Droid build updates through
// its own store (F-Droid client, GitHub release), so there is nothing to do here; the
// review prompt is likewise a Play-only affordance. `ActivityResultLauncher` /
// `IntentSenderRequest` are AndroidX (not Google Play), so the parameter type is shared.
package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.IntentSenderRequest

/** No-op FOSS updates/review manager: no in-app update, no review prompt. */
@Suppress("UNUSED_PARAMETER")
class PlayUpdatesManager(context: Context) {

    /** Callback the shell sets to be told a flexible update finished downloading — never
     *  invoked in FOSS (no in-app updates). Kept for API parity with the `play` twin. */
    var onUpdateDownloaded: (() -> Unit)? = null

    fun checkForFlexibleUpdate(launcher: ActivityResultLauncher<IntentSenderRequest>) {}

    fun resumeStalledUpdate() {}

    fun completeFlexibleUpdate() {}

    fun release() {}

    fun maybeRequestReview(activity: Activity) {}
}
