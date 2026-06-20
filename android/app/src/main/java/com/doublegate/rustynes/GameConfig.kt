package com.doublegate.rustynes

import android.content.Context
import org.json.JSONObject
import java.io.File

/**
 * Per-game settings remembered by ROM SHA-256 (v1.8.5) — a small JSON map in
 * `filesDir` (`game_config.json`). Currently the preferred video filter, so each
 * game reopens with the look you last chose for it; a game with no entry uses the
 * global default. Presentation-only, so determinism is untouched.
 */
object GameConfig {
    private fun file(context: Context) = File(context.filesDir, "game_config.json")

    private fun readAll(context: Context): JSONObject =
        runCatching { JSONObject(file(context).readText()) }.getOrDefault(JSONObject())

    /** The remembered video-filter ordinal for [sha], or null if none is stored. */
    fun filter(context: Context, sha: String): Int? {
        val o = readAll(context).optJSONObject(sha) ?: return null
        return if (o.has("filter")) o.getInt("filter") else null
    }

    /** Remember [filter] (a [VideoFilter] ordinal) as [sha]'s preferred filter. */
    fun setFilter(context: Context, sha: String, filter: Int) {
        val all = readAll(context)
        val o = all.optJSONObject(sha) ?: JSONObject()
        o.put("filter", filter)
        all.put(sha, o)
        runCatching { file(context).writeText(all.toString()) }
    }
}
