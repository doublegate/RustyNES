package com.doublegate.rustynes

import android.view.KeyEvent

/**
 * v3.7.0 (MOB-08, `T-MOBILE-EXTRAS` row 4): the pure decisions behind the Kotlin
 * TODOs v3.7.0 fixed, kept free of Compose and of the bridge so the JVM tests
 * (`Mob08Test`) reach them. The nine sites and what became of each are listed in
 * docs/android.md ("The mobile extras").
 */

/** Save-state slot thumbnails (`States.kt`): the live frame, shrunk. */
object Thumbs {
    /** Thumbnail size: a quarter of the NES picture each way. */
    const val W = 64
    const val H = 60

    /**
     * Nearest-neighbour downscale of an ARGB frame [srcW] x [srcH] to [outW] x
     * [outH], sampling each output pixel's centre. Returns null for a buffer that
     * is not `srcW * srcH` long (no frame yet, or an HD-pack size).
     */
    fun downscale(argb: IntArray, srcW: Int, srcH: Int, outW: Int = W, outH: Int = H): IntArray? {
        if (srcW <= 0 || srcH <= 0 || outW <= 0 || outH <= 0 || argb.size != srcW * srcH) return null
        val out = IntArray(outW * outH)
        for (y in 0 until outH) {
            val sy = ((2 * y + 1) * srcH) / (2 * outH)
            for (x in 0 until outW) {
                val sx = ((2 * x + 1) * srcW) / (2 * outW)
                out[y * outW + x] = argb[sy * srcW + sx]
            }
        }
        return out
    }
}

/** The library tile's context menu on a d-pad-only TV (`Library.kt`), which has no touch long-press. */
object TileKeys {
    /**
     * Whether a key opens the tile's menu: MENU, a gamepad's Y button, or a long
     * press of the d-pad centre / Enter (a short press stays "play").
     */
    fun opensMenu(keyCode: Int, isLongPress: Boolean): Boolean = when (keyCode) {
        KeyEvent.KEYCODE_MENU, KeyEvent.KEYCODE_BUTTON_Y -> true
        KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> isLongPress
        else -> false
    }
}

/**
 * The folder import's walk (`LibraryImport.kt`). Its comment said "one level of
 * subfolders", but the scan recursed into every subfolder with no bound; it now
 * descends at most [MAX_DEPTH] levels below the chosen folder and never enters a
 * folder twice (a provider that reports a folder under two parents cannot loop it).
 */
object ImportWalk {
    /** Levels below the chosen folder that are scanned (the root is depth 0). */
    const val MAX_DEPTH = 8

    /** Whether to scan the folder [docId] found at [depth]; records it in [seen]. */
    fun shouldScan(depth: Int, docId: String, seen: MutableSet<String>): Boolean =
        depth <= MAX_DEPTH && seen.add(docId)
}
