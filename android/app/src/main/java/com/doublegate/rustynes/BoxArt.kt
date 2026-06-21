package com.doublegate.rustynes

import android.content.Context
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder

/**
 * Box-art auto-match (v1.8.8 "Atlas", Workstream C).
 *
 * Resolves cover art for a game from the community **libretro-thumbnails** library
 * (the same no-intro-named PNG corpus RetroArch uses) by deriving candidate
 * filenames from the ROM's display name. This is **user-triggered, opt-in fetching**
 * of art from a public library — RustyNES still bundles no art and ships no ROMs, so
 * the no-bundled-content posture holds; the network call only happens when the user
 * asks (a "Find box art" action with a preview before it is applied).
 *
 * Matching mirrors libretro's documented rules: the playlist display name with the
 * invalid characters ``& * / : ` < > ? \ |`` replaced by `_`, with a "short name"
 * fallback (the text before the first parenthesis — drops the `(USA)` / `(Rev 1)`
 * region/version tags). Downloaded art is cached to `filesDir/boxart/<sha>.png` and
 * the entry then references it as a `file://` URI (Coil loads it offline thereafter).
 *
 * Source: https://github.com/libretro-thumbnails/Nintendo_-_Nintendo_Entertainment_System
 * (Named_Boxarts). ScreenScraper / TheGamesDB are richer but need an account/API key,
 * so they are noted as a future fallback (TODO) rather than wired here.
 */
object BoxArt {
    /** The libretro NES Named_Boxarts raw base (master branch). */
    private const val BASE =
        "https://raw.githubusercontent.com/libretro-thumbnails/" +
            "Nintendo_-_Nintendo_Entertainment_System/master/Named_Boxarts/"

    /** libretro's invalid filename characters, each replaced by `_`. */
    private val INVALID = Regex("[&*/:`<>?\\\\|]")

    private fun dir(ctx: Context) = File(ctx.filesDir, "boxart").apply { mkdirs() }

    /** The cached art file for a game (may not exist yet). */
    fun cacheFile(ctx: Context, sha: String) = File(dir(ctx), "$sha.png")

    /**
     * Sanitize a display name to a libretro thumbnail base name (no extension): drop
     * any file extension, replace the invalid characters with `_`, and collapse
     * surrounding whitespace. e.g. `"Mega Man 2 (USA).nes"` -> `"Mega Man 2 (USA)"`.
     */
    fun sanitize(displayName: String): String {
        val noExt = displayName.substringBeforeLast('.', displayName).trim()
        return INVALID.replace(noExt, "_").trim()
    }

    /** The "short name": everything before the first '(' (drops region/version tags). */
    private fun shortName(sanitized: String): String =
        sanitized.substringBefore('(').trim()

    /**
     * The ordered candidate raw URLs to try for [displayName]: the full sanitized
     * name first, then the short name (region/version-tag-stripped). Each is
     * percent-encoded for the path (spaces -> %20, parentheses kept readable).
     */
    fun candidateUrls(displayName: String): List<String> {
        val full = sanitize(displayName)
        val short = shortName(full)
        return buildList {
            add(BASE + encodePath("$full.png"))
            if (short.isNotEmpty() && short != full) add(BASE + encodePath("$short.png"))
        }.distinct()
    }

    /** Percent-encode a filename for a raw GitHub path (spaces + unicode), keeping
     *  the common readable separators that GitHub serves verbatim. */
    private fun encodePath(name: String): String =
        URLEncoder.encode(name, "UTF-8")
            .replace("+", "%20")
            .replace("%28", "(")
            .replace("%29", ")")
            .replace("%2C", ",")
            .replace("%27", "'")
            .replace("%21", "!")

    /**
     * Try each candidate URL until one returns 200, download it to the per-game cache
     * file, and return that file. Returns null if nothing matched or on any network
     * error. Caller MUST run this off the main thread (blocking HTTP). The fetch is
     * size-capped (2 MiB) so a wrong/huge response can't OOM.
     */
    fun fetchToCache(ctx: Context, sha: String, displayName: String): File? {
        val out = cacheFile(ctx, sha)
        for (url in candidateUrls(displayName)) {
            val ok = runCatching { download(url, out) }.getOrDefault(false)
            if (ok) return out
        }
        return null
    }

    private const val MAX_BYTES = 2 * 1024 * 1024

    private fun download(urlStr: String, out: File): Boolean {
        val conn = (URL(urlStr).openConnection() as HttpURLConnection).apply {
            connectTimeout = 8000
            readTimeout = 8000
            instanceFollowRedirects = true
            setRequestProperty("User-Agent", "RustyNES-Android")
        }
        try {
            if (conn.responseCode != HttpURLConnection.HTTP_OK) return false
            val tmp = File(out.parentFile, out.name + ".tmp")
            conn.inputStream.use { input ->
                tmp.outputStream().use { o ->
                    val buf = ByteArray(16 * 1024)
                    var total = 0
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        total += n
                        if (total > MAX_BYTES) { tmp.delete(); return false }
                        o.write(buf, 0, n)
                    }
                }
            }
            if (tmp.length() == 0L) { tmp.delete(); return false }
            tmp.renameTo(out)
            return out.exists() && out.length() > 0
        } finally {
            conn.disconnect()
        }
    }
}
