package com.doublegate.rustynes

import android.content.Context
import android.content.Intent

/**
 * v1.8.8 "Atlas" (Workstream H): deep-link contract shared by the platform surfaces
 * — the Quick Settings tile, the static app shortcuts, and the home-screen widget.
 *
 * Each surface starts [MainActivity] with [EXTRA_ACTION] set to one of the action
 * constants; the activity (whose launchMode is `singleTop`) reads it in `onCreate`
 * / `onNewIntent` and the Compose shell reacts (resume the last game, open the SAF
 * picker, or show the library). The "last game" is the library's most-recently
 * played entry — the single source of truth for all three surfaces.
 */
object DeepLink {
    const val EXTRA_ACTION = "com.doublegate.rustynes.extra.ACTION"

    /** Resume the last-played game (no-op if the library is empty). */
    const val ACTION_RESUME = "resume"

    /** Open the SAF document picker to load a new ROM. */
    const val ACTION_OPEN = "open"

    /** Show the box-art library (idle screen). */
    const val ACTION_LIBRARY = "library"

    /** Build an [Intent] that launches the app to [action]. */
    fun intent(context: Context, action: String): Intent =
        Intent(context, MainActivity::class.java).apply {
            this.action = Intent.ACTION_MAIN
            putExtra(EXTRA_ACTION, action)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

    /** The library's most-recently-played game, or null when nothing's been played. */
    fun lastPlayed(context: Context): GameEntry? =
        GameLibrary.view(context, sort = LibrarySort.RECENT).firstOrNull { it.lastPlayed > 0L }
}
