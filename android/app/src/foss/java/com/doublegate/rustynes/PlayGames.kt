// FOSS-FLAVOR SOURCE SET (v2.0.1, ADR 0025). No-op stand-in for the Play-Games-Services
// `PlayGamesManager` (the real one is in `src/play/.../PlayGames.kt`). Links NO
// `com.google.*`. The shared `PgsIds` object lives in `src/main/.../PlayFacadeShared.kt`,
// so `MainActivity`'s `unlock(PgsIds.ACH_…)` calls still resolve — they are simply
// swallowed here. Public surface is identical to the `play` twin.
package com.doublegate.rustynes

import android.app.Activity
import android.content.Context

/** No-op FOSS Play-Games manager: never signed in, every achievement/leaderboard call
 *  dropped. There is no Google account surface in the FOSS build. */
@Suppress("UNUSED_PARAMETER")
class PlayGamesManager(context: Context) {

    /** Always false: no PGS sign-in in FOSS. Read by the cloud-save layer + Settings. */
    var isSignedIn: Boolean = false
        private set

    fun attachActivity(activity: Activity?) {}

    fun initialize() {}

    fun ensureSignedIn(onResult: (Boolean) -> Unit = {}) {
        onResult(false)
    }

    fun unlock(achievementId: String) {}

    fun increment(achievementId: String, delta: Int) {}

    fun submitScore(leaderboardId: String, score: Long) {}
}
