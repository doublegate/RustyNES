// PLAY-FLAVOR SOURCE SET (v2.0.1, ADR 0025). The real in-app-update + in-app-review
// manager (`com.google.android.play:app-update` / `review`). The `foss` twin in
// `src/foss/.../PlayUpdates.kt` is a no-op with the same public surface — a sideload /
// F-Droid build self-updates through its store, so there is nothing to do there.
package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import android.util.Log
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.IntentSenderRequest
import com.google.android.play.core.appupdate.AppUpdateManager
import com.google.android.play.core.appupdate.AppUpdateManagerFactory
import com.google.android.play.core.appupdate.AppUpdateOptions
import com.google.android.play.core.install.InstallStateUpdatedListener
import com.google.android.play.core.install.model.AppUpdateType
import com.google.android.play.core.install.model.InstallStatus
import com.google.android.play.core.install.model.UpdateAvailability
import com.google.android.play.core.review.ReviewManagerFactory

/**
 * In-app updates (FLEXIBLE) + in-app review (v1.8.8 "Atlas", Workstream L).
 *
 * Both need NO Cloud project and no server: the Play Core libraries talk to the
 * installed Play Store directly, and **no-op gracefully on a sideloaded / non-Play
 * install** (the update check just reports nothing available; the review flow quietly
 * does nothing). They are therefore safe to call unconditionally — we still gate on
 * [BuildConfig.PLAY_BUILD] for clarity / to skip the work on dev builds, mirroring the
 * rest of the Play wiring.
 *
 * FLEXIBLE update: the user keeps using the app while the update downloads in the
 * background, then a snackbar offers to restart-and-install (Immediate is reserved for
 * a critical fix and is not used here). In-app review is triggered sparingly after a
 * satisfying session (no CTA button; the API enforces its own quota, so an over-eager
 * call simply no-ops).
 */
class PlayUpdatesManager(context: Context) {
    private val appContext = context.applicationContext

    private val updateManager: AppUpdateManager? =
        if (BuildConfig.PLAY_BUILD) {
            runCatching { AppUpdateManagerFactory.create(appContext) }.getOrNull()
        } else {
            null
        }

    /** Fired when a flexible update has finished downloading and is ready to install —
     *  the UI shows a "Restart to update" snackbar that calls [completeFlexibleUpdate]. */
    @Volatile
    var onUpdateDownloaded: (() -> Unit)? = null

    private val installListener = InstallStateUpdatedListener { state ->
        if (state.installStatus() == InstallStatus.DOWNLOADED) {
            onUpdateDownloaded?.invoke()
        }
    }

    /** Guards against registering [installListener] more than once (each
     *  [checkForFlexibleUpdate] would otherwise add a duplicate / leak the callback). */
    @Volatile
    private var listenerRegistered = false

    /**
     * Check Play for an available update and, if a FLEXIBLE update is allowed, launch
     * the flexible flow via [launcher]. Call at a sensible point (e.g. first foreground
     * after launch). No-op on sideload / when Play has nothing.
     */
    fun checkForFlexibleUpdate(launcher: ActivityResultLauncher<IntentSenderRequest>) {
        val mgr = updateManager ?: return
        // Register at most once — repeated calls would otherwise stack duplicate
        // listeners / leak callbacks.
        if (!listenerRegistered) {
            mgr.registerListener(installListener)
            listenerRegistered = true
        }
        mgr.appUpdateInfo.addOnSuccessListener { info ->
            val available = info.updateAvailability() == UpdateAvailability.UPDATE_AVAILABLE
            if (available && info.isUpdateTypeAllowed(AppUpdateType.FLEXIBLE)) {
                runCatching {
                    mgr.startUpdateFlowForResult(
                        info,
                        launcher,
                        AppUpdateOptions.newBuilder(AppUpdateType.FLEXIBLE).build(),
                    )
                }
            }
        }.addOnFailureListener { Log.i("RustyNES", "Update check: none") }
    }

    /** Re-check on resume for a flexible update that already finished downloading (so a
     *  download that completed while backgrounded still offers the install). */
    fun resumeStalledUpdate() {
        val mgr = updateManager ?: return
        mgr.appUpdateInfo.addOnSuccessListener { info ->
            if (info.installStatus() == InstallStatus.DOWNLOADED) onUpdateDownloaded?.invoke()
        }
    }

    /** Finish a downloaded flexible update (restarts the app to install). */
    fun completeFlexibleUpdate() {
        updateManager?.completeUpdate()
    }

    /** Detach the install listener (call from onDestroy). */
    fun release() {
        if (listenerRegistered) {
            updateManager?.unregisterListener(installListener)
            listenerRegistered = false
        }
    }

    /**
     * Request the in-app review flow (after a satisfying session). The API quietly
     * enforces its own frequency quota — over-eager calls just no-op — so there is no
     * CTA and no guarantee a dialog shows. No-op on sideload.
     */
    fun maybeRequestReview(activity: Activity) {
        if (!BuildConfig.PLAY_BUILD) return
        val manager = runCatching { ReviewManagerFactory.create(appContext) }.getOrNull() ?: return
        manager.requestReviewFlow().addOnCompleteListener { task ->
            if (task.isSuccessful) {
                runCatching { manager.launchReviewFlow(activity, task.result) }
            }
        }
    }
}
