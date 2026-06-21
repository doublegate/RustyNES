package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import android.util.Log
import com.google.android.gms.games.AuthenticationResult
import com.google.android.gms.games.PlayGames
import com.google.android.gms.games.PlayGamesSdk
import java.lang.ref.WeakReference

/**
 * Play Games Services v2 (PGS) integration — sign-in + achievements + leaderboards
 * (v1.8.8 "Atlas", Workstream E), plus the account anchor that the cloud-save
 * Snapshots layer ([CloudSaveManager], Workstream D) rides on.
 *
 * PREPPED BEHIND A DEFAULT-OFF FLAG — exactly like [ChromecastSender]. Nothing here
 * touches the Play Games SDK unless [BuildConfig.PGS_ENABLED] is true, which it is
 * not until the maintainer does the deferred ops:
 *  - create the linked **Play Games project** in the Play Console (a `game_ids`
 *    resource / OAuth2 client tied to the app's signing cert),
 *  - drop the generated `games-ids.xml` / `@string/game_services_project_id` in,
 *  - create the real **achievement IDs** + the **leaderboard ID** (see [PgsIds]) and
 *    paste them over the placeholders below.
 * With the flag off, [PlayGamesManager] is a cheap no-op shell: `ensureSignedIn`
 * returns immediately, every `unlock`/`increment`/`submitScore` is dropped.
 *
 * DISTINCT FROM RETROACHIEVEMENTS. RetroAchievements (`rustynes-ra`, v1.8.6) is the
 * per-GAME retro-achievement community service (hardcore mode, `.rap` sidecars). PGS
 * achievements here are **app-level platform milestones** surfaced in the Google Play
 * Games app (first ROM loaded, first save-state, first netplay session, turbo usage).
 * The two coexist and never collide — they are different services with different IDs,
 * different UIs, and different triggers.
 */

/**
 * PGS achievement + leaderboard IDs (Workstream E).
 *
 * These are **PLACEHOLDERS**. The maintainer creates the achievements + leaderboard in
 * the Play Console (Play Games Services -> Achievements / Leaderboards), which mints
 * the real opaque IDs (e.g. "CgkI...EAIQAQ"), then replaces the strings below. The
 * placeholders are intentionally NOT valid PGS IDs, so even if the flag were flipped
 * on without the Console setup, the SDK would just reject the call (belt-and-suspenders
 * on top of the default-off PGS_ENABLED flag).
 *
 * Kept deliberately small + app-flavored (NOT per-game). Each maps to a natural
 * trigger point wired in MainActivity / the emulation loop.
 */
object PgsIds {
    /** Unlocked the first time any ROM boots. */
    const val ACH_FIRST_ROM: String = "achievement_first_rom_loaded"

    /** Unlocked the first time the user writes a save-state slot. */
    const val ACH_FIRST_SAVE_STATE: String = "achievement_first_save_state"

    /** Unlocked the first time a netplay session connects. */
    const val ACH_FIRST_NETPLAY: String = "achievement_first_netplay"

    /** Incremental: accumulated frames run with fast-forward (turbo) engaged. The
     *  Console-side step target is 100 (so this fires at ~100 turbo frames ≈ a few
     *  seconds of fast-forward). We post deltas via [PlayGamesManager.increment]. */
    const val ACH_TURBO_100: String = "achievement_turbo_100_frames"

    /** Unlocked the first time a cloud save syncs (Workstream D ties in here). */
    const val ACH_FIRST_CLOUD_SYNC: String = "achievement_first_cloud_sync"

    /** A single, minimal leaderboard: total play time in seconds. (Leaderboards are
     *  thin for an emulator; this one is enough to exercise the LeaderboardsClient and
     *  is harmless if the maintainer chooses not to publish it.) */
    const val LB_TOTAL_PLAY_SECONDS: String = "leaderboard_total_play_seconds"
}

/**
 * Owns PGS sign-in + the achievement/leaderboard surface. Created once (application
 * Context). Every method is a no-op when [BuildConfig.PGS_ENABLED] is false.
 *
 * Sign-in: PGS v2 **auto-signs-in** at app launch — calling [PlayGamesSdk.initialize]
 * once is enough for the SDK to attempt a silent sign-in. [ensureSignedIn] checks the
 * authenticated state and (only if needed) requests an interactive sign-in. The
 * [isSignedIn] flag drives the cloud-save layer and the Settings status line.
 */
class PlayGamesManager(context: Context) {
    private val appContext = context.applicationContext

    /** The current foreground Activity — the PGS v2 client factories
     *  (`PlayGames.getXxxClient`) require an Activity, not a Context. Held weakly +
     *  refreshed by the owning Activity so we never leak a destroyed Activity. */
    private var activityRef: WeakReference<Activity> = WeakReference(null)

    /** True once PGS reports the user is authenticated. Read by [CloudSaveManager] to
     *  gate Snapshot calls and by Settings for the status line. */
    @Volatile
    var isSignedIn: Boolean = false
        private set

    /** Bind/refresh the foreground Activity (call from the owner's onResume; clear in
     *  onDestroy with null). The PGS clients are obtained per-call from it. */
    fun attachActivity(activity: Activity?) {
        activityRef = WeakReference(activity)
    }

    private fun activity(): Activity? = activityRef.get()

    /** Initialize the PGS SDK once (triggers the v2 auto-sign-in). No-op when off. */
    fun initialize() {
        if (!BuildConfig.PGS_ENABLED) return
        runCatching { PlayGamesSdk.initialize(appContext) }
            .onFailure { Log.w("RustyNES", "PGS init failed", it) }
    }

    /**
     * Ensure the user is signed in, requesting an interactive sign-in only if the
     * silent auto-sign-in didn't already authenticate. [onResult] reports the final
     * state. No-op (reports false) when the flag is off or no Activity is bound.
     */
    fun ensureSignedIn(onResult: (Boolean) -> Unit = {}) {
        if (!BuildConfig.PGS_ENABLED) { onResult(false); return }
        val act = activity() ?: run { isSignedIn = false; onResult(false); return }
        val client = runCatching { PlayGames.getGamesSignInClient(act) }.getOrNull()
        if (client == null) { isSignedIn = false; onResult(false); return }
        client.isAuthenticated.addOnSuccessListener { result: AuthenticationResult ->
            if (result.isAuthenticated) {
                isSignedIn = true
                onResult(true)
            } else {
                client.signIn().addOnCompleteListener { task ->
                    isSignedIn = task.isSuccessful && (task.result?.isAuthenticated == true)
                    onResult(isSignedIn)
                }
            }
        }.addOnFailureListener {
            isSignedIn = false
            onResult(false)
        }
    }

    /** Unlock a (non-incremental) achievement by its [PgsIds] id. Idempotent server-side. */
    fun unlock(achievementId: String) {
        if (!BuildConfig.PGS_ENABLED || !isSignedIn) return
        val act = activity() ?: return
        runCatching { PlayGames.getAchievementsClient(act).unlock(achievementId) }
    }

    /** Post a [delta]-step increment toward an incremental achievement. PGS clamps at
     *  the Console-defined step target and auto-unlocks on reaching it. */
    fun increment(achievementId: String, delta: Int) {
        if (!BuildConfig.PGS_ENABLED || !isSignedIn || delta <= 0) return
        val act = activity() ?: return
        runCatching { PlayGames.getAchievementsClient(act).increment(achievementId, delta) }
    }

    /** Submit a score to a leaderboard. Used for the total-play-time leaderboard. */
    fun submitScore(leaderboardId: String, score: Long) {
        if (!BuildConfig.PGS_ENABLED || !isSignedIn || score < 0) return
        val act = activity() ?: return
        runCatching { PlayGames.getLeaderboardsClient(act).submitScore(leaderboardId, score) }
    }

    /** The Snapshots client (used by [CloudSaveManager]); null without a bound Activity. */
    internal fun snapshotsClientOrNull() =
        activity()?.let { runCatching { PlayGames.getSnapshotsClient(it) }.getOrNull() }
}
