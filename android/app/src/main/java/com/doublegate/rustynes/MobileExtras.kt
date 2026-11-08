package com.doublegate.rustynes

import android.content.Context

/**
 * v3.7.0 "the mobile extras" (D24, `T-MOBILE-EXTRAS` row 3, and the carry-overs
 * `T-MOBILE-GAME-CORRECTIONS` / `T-RA-HARDCORE-RESUME`): the pure decisions behind
 * the 20-band equalizer, the Game Genie code database, the per-game picture
 * corrections and the hardcore auto-resume notice.
 *
 * Everything here is plain Kotlin with no Android or UniFFI type, so the JVM unit
 * tests (`app/src/test`) reach it without a device or the native library. The
 * filter, the catalog and the correction itself live in the Rust bridge
 * (`rustynes-mobile`'s `extras` module), shared with iOS and compiled from the
 * desktop's own sources; this file only stores the player's choices and decides
 * what the app does with them.
 */

/** The equalizer's stored gains: one dB value per band, as a comma-separated list. */
object EqPrefs {
    /** Bands in the bridge's EQ (`equalizerBandFrequencies().size`). */
    const val BANDS = 20

    /** The slider range; the bridge clamps to the same. */
    const val LIMIT_DB = 12f

    /** All bands at 0 dB: the EQ off, bit-identical audio. */
    fun flat(): FloatArray = FloatArray(BANDS)

    /** Serialize [gains] for SharedPreferences. */
    fun encode(gains: FloatArray): String = gains.joinToString(",")

    /**
     * Parse a stored value. Anything that is not exactly [BANDS] numbers (absent,
     * from an older or corrupt preference) is the flat bank, and every value is
     * clamped to +/-[LIMIT_DB] with a non-finite one taken as 0, so a bad stored
     * preference can never reach the audio as a NaN.
     */
    fun decode(stored: String?): FloatArray {
        val parts = stored?.split(',') ?: return flat()
        if (parts.size != BANDS) return flat()
        val out = FloatArray(BANDS)
        for ((i, p) in parts.withIndex()) {
            val v = p.trim().toFloatOrNull() ?: return flat()
            out[i] = if (v.isFinite()) v.coerceIn(-LIMIT_DB, LIMIT_DB) else 0f
        }
        return out
    }

    /** Whether [gains] is the flat bank (the EQ is off). */
    fun isFlat(gains: FloatArray): Boolean = gains.all { it == 0f }

    /** A slider label: "25 Hz", "1.6 kHz". */
    fun bandLabel(hz: Float): String =
        if (hz >= 1000f) {
            val k = hz / 1000f
            if (k == k.toInt().toFloat()) "${k.toInt()} kHz" else "${"%.1f".format(java.util.Locale.ROOT, k)} kHz"
        } else {
            "${hz.toInt()} Hz"
        }
}

/**
 * The Game Genie codes a player picked from the database (or typed), kept per game
 * so they come back when the game is opened again. Stored as a sorted,
 * comma-separated list of canonical codes (Game Genie codes are letters only, so a
 * comma never occurs in one).
 */
object CheatPicks {
    private val CODE = Regex("^[A-Z]{6}([A-Z]{2})?$")

    /** Serialize a set of codes. */
    fun encode(codes: Set<String>): String = codes.sorted().joinToString(",")

    /**
     * Parse a stored list, keeping only well-formed codes (upper-cased). The bridge
     * validates each again on `cheatAddGenie`, so this is a filter for junk, not
     * the validator.
     */
    fun decode(stored: String?): Set<String> =
        stored.orEmpty().split(',').map { it.trim().uppercase() }.filter { CODE.matches(it) }.toSortedSet()

    /** The new set after toggling [code] on or off in [current]. */
    fun toggle(current: Set<String>, code: String, on: Boolean): Set<String> {
        val c = code.trim().uppercase()
        return if (on) current + c else current - c
    }
}

/**
 * `T-RA-HARDCORE-RESUME` (maintainer, 2026-11-08: keep the refusal). What to do with
 * the on-background save state when a game is opened.
 *
 * RetroAchievements hardcore forbids loading a state, and the bridge refuses
 * `loadState` while a hardcore session is active. On Android the session is
 * created after the game is prepared, so the bridge alone would let the resume
 * through and then power-cycle the console when hardcore takes effect: the game
 * restarted either way, with nothing on screen saying why. The app now decides
 * before it loads anything, and says so in one line.
 */
enum class AutoResume {
    /** No on-background state exists for this game: a plain power-on. */
    NONE,

    /** Load the on-background state. */
    LOAD,

    /** Hardcore is on: start from power-on and say so in one line. */
    SKIP_HARDCORE,
    ;

    companion object {
        /**
         * The decision for a game being opened. [hardcore] counts only when
         * RetroAchievements is enabled ([raEnabled]): the hardcore switch of a
         * disabled RA integration changes nothing.
         */
        fun decide(hasSlot: Boolean, raEnabled: Boolean, hardcore: Boolean): AutoResume = when {
            !hasSlot -> NONE
            raEnabled && hardcore -> SKIP_HARDCORE
            else -> LOAD
        }

        /**
         * The same decision when the load itself was refused: a hardcore refusal from
         * the bridge (`MobileException.HardcoreBlocked`, seen as [refusedForHardcore])
         * gets the notice, any other failure (a state from another version) none.
         */
        fun afterRefusal(refusedForHardcore: Boolean): AutoResume = if (refusedForHardcore) SKIP_HARDCORE else NONE
    }

    /** Whether this decision puts the one-line notice (`R.string.notice_hardcore_resume`) on the status line. */
    val showsNotice: Boolean get() = this == SKIP_HARDCORE
}

/**
 * The per-game store for [CheatPicks]: SharedPreferences `cheats`, one key per ROM
 * identity (the same SHA-256 every other per-game store uses).
 */
object CheatStore {
    private fun prefs(context: Context) = context.getSharedPreferences("cheats", Context.MODE_PRIVATE)

    /** The codes picked for the game [sha]. */
    fun picks(context: Context, sha: String): Set<String> = CheatPicks.decode(prefs(context).getString(sha, null))

    /** Replace the codes picked for the game [sha]; an empty set removes the key. */
    fun save(context: Context, sha: String, codes: Set<String>) {
        val e = prefs(context).edit()
        if (codes.isEmpty()) e.remove(sha) else e.putString(sha, CheatPicks.encode(codes))
        e.apply()
    }
}
