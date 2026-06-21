package com.doublegate.rustynes

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * The box-art game library (v1.8.8 "Atlas", Workstream C).
 *
 * Grows the bare [RomLibrary] recents list into a real library: per-ROM entries
 * keyed by **ROM SHA-256** (the same key the bridge, save-states, and RA progress
 * use) carrying a display name, the persistable SAF content URI, the mapper/region
 * from the bridge `RomInfo`, a last-played timestamp, a favorite flag, an optional
 * user-supplied box-art URI, and a folder/collection tag.
 *
 * It is persisted as JSON in `filesDir/library.json` (mirroring the [GameConfig] /
 * recents pattern). Box art is **user-supplied / locally-derived only** — no bundled
 * or network-fetched art (the no-bundled-content posture). Presentation-only, so the
 * determinism contract is untouched.
 */

/** One game in the library. [sha] is the lowercase-hex ROM SHA-256 (stable key). */
data class GameEntry(
    val sha: String,
    val name: String,
    /** Persistable SAF content URI to (re-)open the ROM, or "" if it is unknown
     *  (e.g. a debug autoload that was never a SAF document). */
    val uri: String,
    /** iNES/NES 2.0 mapper number, or -1 if unknown (entry predates a load). */
    val mapper: Int = -1,
    /** Region label ("NTSC" / "PAL" / "Dendy"), or "" if unknown. */
    val region: String = "",
    /** Last-played epoch millis (0 = never played since being added). */
    val lastPlayed: Long = 0L,
    /** Whether the user starred this game. */
    val favorite: Boolean = false,
    /** Optional user-picked box-art content URI ("" = none, show a placeholder). */
    val boxArtUri: String = "",
    /** A folder / collection tag ("" = uncategorized / All only). */
    val folder: String = "",
) {
    fun toJson(): JSONObject = JSONObject().apply {
        put("sha", sha)
        put("name", name)
        put("uri", uri)
        put("mapper", mapper)
        put("region", region)
        put("lastPlayed", lastPlayed)
        put("favorite", favorite)
        put("boxArt", boxArtUri)
        put("folder", folder)
    }

    companion object {
        fun fromJson(o: JSONObject): GameEntry = GameEntry(
            sha = o.optString("sha"),
            name = o.optString("name"),
            uri = o.optString("uri"),
            mapper = o.optInt("mapper", -1),
            region = o.optString("region"),
            lastPlayed = o.optLong("lastPlayed", 0L),
            favorite = o.optBoolean("favorite", false),
            boxArtUri = o.optString("boxArt"),
            folder = o.optString("folder"),
        )
    }
}

/** How the library grid is ordered. */
enum class LibrarySort { RECENT, NAME, FAVORITE }

/**
 * The on-disk game library: a JSON array of [GameEntry] at `filesDir/library.json`,
 * keyed (and de-duplicated) by ROM SHA-256.
 */
object GameLibrary {
    private fun file(ctx: Context) = File(ctx.filesDir, "library.json")

    /** True once the one-time recents -> library migration has run. */
    private fun migratedFile(ctx: Context) = File(ctx.filesDir, ".library_migrated")

    /** Read every entry (unordered). Tolerant of a missing / corrupt file. */
    fun all(ctx: Context): List<GameEntry> {
        ensureMigrated(ctx)
        val f = file(ctx)
        if (!f.exists()) return emptyList()
        return runCatching {
            val arr = JSONArray(f.readText())
            (0 until arr.length()).map { GameEntry.fromJson(arr.getJSONObject(it)) }
        }.getOrDefault(emptyList())
    }

    /** The single entry for [sha], or null. */
    fun get(ctx: Context, sha: String): GameEntry? = all(ctx).firstOrNull { it.sha == sha }

    private fun writeAll(ctx: Context, entries: List<GameEntry>) {
        val arr = JSONArray()
        entries.forEach { arr.put(it.toJson()) }
        runCatching { file(ctx).writeText(arr.toString()) }
    }

    /**
     * Insert or update a game (matched by [GameEntry.sha]). When an entry already
     * exists, name/uri/mapper/region/lastPlayed are refreshed from [entry] while the
     * user-owned fields (favorite, boxArt, folder) are preserved unless [entry]
     * carries non-default values for them.
     */
    fun upsert(ctx: Context, entry: GameEntry) {
        val existing = all(ctx).toMutableList()
        val idx = existing.indexOfFirst { it.sha == entry.sha }
        if (idx >= 0) {
            val prev = existing[idx]
            existing[idx] = prev.copy(
                name = entry.name.ifEmpty { prev.name },
                uri = entry.uri.ifEmpty { prev.uri },
                mapper = if (entry.mapper >= 0) entry.mapper else prev.mapper,
                region = entry.region.ifEmpty { prev.region },
                lastPlayed = maxOf(entry.lastPlayed, prev.lastPlayed),
                favorite = entry.favorite || prev.favorite,
                boxArtUri = entry.boxArtUri.ifEmpty { prev.boxArtUri },
                folder = entry.folder.ifEmpty { prev.folder },
            )
        } else {
            existing.add(entry)
        }
        writeAll(ctx, existing)
    }

    /** Stamp [sha] as just played (now), creating nothing if it is unknown. */
    fun touch(ctx: Context, sha: String) {
        val entries = all(ctx).toMutableList()
        val idx = entries.indexOfFirst { it.sha == sha }
        if (idx >= 0) {
            entries[idx] = entries[idx].copy(lastPlayed = System.currentTimeMillis())
            writeAll(ctx, entries)
        }
    }

    fun setFavorite(ctx: Context, sha: String, fav: Boolean) =
        mutate(ctx, sha) { it.copy(favorite = fav) }

    fun setBoxArt(ctx: Context, sha: String, uri: String) =
        mutate(ctx, sha) { it.copy(boxArtUri = uri) }

    fun setFolder(ctx: Context, sha: String, folder: String) =
        mutate(ctx, sha) { it.copy(folder = folder) }

    /** Remove a game from the library (does not delete the ROM or its states). */
    fun remove(ctx: Context, sha: String) {
        writeAll(ctx, all(ctx).filterNot { it.sha == sha })
    }

    private inline fun mutate(ctx: Context, sha: String, transform: (GameEntry) -> GameEntry) {
        val entries = all(ctx).toMutableList()
        val idx = entries.indexOfFirst { it.sha == sha }
        if (idx >= 0) {
            entries[idx] = transform(entries[idx])
            writeAll(ctx, entries)
        }
    }

    /** The distinct, sorted set of non-empty folder tags currently in use. */
    fun folders(ctx: Context): List<String> =
        all(ctx).mapNotNull { it.folder.takeIf { f -> f.isNotEmpty() } }.distinct().sorted()

    /**
     * The library filtered + sorted for display. [folder] of null = All; "" never
     * matches (folders are non-empty tags). [favoritesOnly] gates to starred games;
     * [query] is a case-insensitive substring match on the name.
     */
    fun view(
        ctx: Context,
        folder: String? = null,
        favoritesOnly: Boolean = false,
        query: String = "",
        sort: LibrarySort = LibrarySort.RECENT,
    ): List<GameEntry> {
        val q = query.trim().lowercase()
        var list = all(ctx).asSequence()
        if (favoritesOnly) list = list.filter { it.favorite }
        if (folder != null) list = list.filter { it.folder == folder }
        if (q.isNotEmpty()) list = list.filter { it.name.lowercase().contains(q) }
        val comparator = when (sort) {
            LibrarySort.NAME -> compareBy<GameEntry> { it.name.lowercase() }
            LibrarySort.FAVORITE ->
                compareByDescending<GameEntry> { it.favorite }.thenByDescending { it.lastPlayed }
            LibrarySort.RECENT -> compareByDescending<GameEntry> { it.lastPlayed }.thenBy { it.name.lowercase() }
        }
        return list.sortedWith(comparator).toList()
    }

    /**
     * One-time migration: fold any existing [RomLibrary] recents (the v1.8.0..1.8.7
     * TSV list) into the library so an upgrade keeps the user's games. Each recent
     * becomes an entry keyed by a synthetic SHA derived from its URI (the real ROM
     * SHA is filled in the first time the game is opened via [upsert]); favorite /
     * boxArt / folder default empty. Idempotent (guarded by a marker file).
     */
    private fun ensureMigrated(ctx: Context) {
        if (migratedFile(ctx).exists()) return
        // Only migrate if the library doesn't already exist (a fresh install with no
        // recents writes the marker and does nothing).
        if (!file(ctx).exists()) {
            val recents = RomLibrary.recents(ctx)
            if (recents.isNotEmpty()) {
                val now = System.currentTimeMillis()
                // Preserve recents order as descending lastPlayed so RECENT sort keeps it.
                val migrated = recents.mapIndexed { i, r ->
                    GameEntry(
                        sha = "uri:" + r.uri.hashCode().toUInt().toString(16),
                        name = r.name,
                        uri = r.uri,
                        lastPlayed = now - i,
                    )
                }
                val arr = JSONArray()
                migrated.forEach { arr.put(it.toJson()) }
                runCatching { file(ctx).writeText(arr.toString()) }
            }
        }
        runCatching { migratedFile(ctx).writeText("1") }
    }
}
