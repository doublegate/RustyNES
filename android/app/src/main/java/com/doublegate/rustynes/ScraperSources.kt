package com.doublegate.rustynes

import android.content.Context
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.net.URLEncoder

/**
 * Box-art source orchestration (v1.8.8 "Atlas", Workstream C).
 *
 * Resolves cover art for a game by trying, in order, the sources the user has opted
 * into, and downloading the first hit to the per-game cache (`BoxArt.cacheFile`):
 *
 *  1. **ScreenScraper** — if the user filled in their ScreenScraper account
 *     (ssid + password) in Settings. Richest DB; their personal account / quota.
 *  2. **TheGamesDB** — if the user supplied their own public API key in Settings.
 *  3. **libretro-thumbnails** — always available, no account (see [BoxArt]).
 *
 * All credentials are the **user's own** and live only in their device's settings —
 * RustyNES ships no keys, bundles no art, and only fetches on the user's explicit
 * "Find box art" action. This keeps the no-bundled-content posture while letting power
 * users plug in the better databases.
 */
object ScraperSources {
    /** ScreenScraper is gated off until RustyNES has a registered devid/softname (their
     *  API requires a registered application, not merely a user login). The client is
     *  fully implemented; flip this to true and wire the dev credentials once obtained. */
    const val SS_ENABLED = false

    /**
     * Try each configured source in priority order; return the cached art file or null.
     * MUST be called off the main thread (blocking HTTP).
     */
    fun fetchBoxArt(ctx: Context, settings: AppSettings, sha: String, displayName: String): File? {
        val out = BoxArt.cacheFile(ctx, sha)
        // 1. ScreenScraper (user account). GATED OFF until RustyNES has a registered
        //    ScreenScraper devid/devpassword/softname (their API requires a registered
        //    application, not just a user login). The full client is implemented below
        //    (screenScraperUrl) and the Settings fields are kept; flip SS_ENABLED to
        //    true and supply the dev credentials once registered. Until then the user's
        //    ssid/password are stored but never used, and TheGamesDB + libretro serve.
        if (SS_ENABLED && settings.ssUser.isNotEmpty() && settings.ssPassword.isNotEmpty()) {
            screenScraperUrl(displayName, settings.ssUser, settings.ssPassword)
                ?.let { if (downloadTo(it, out)) return out }
        }
        // 2. TheGamesDB (user API key).
        if (settings.tgdbApiKey.isNotEmpty()) {
            theGamesDbUrl(displayName, settings.tgdbApiKey)
                ?.let { if (downloadTo(it, out)) return out }
        }
        // 3. libretro-thumbnails (always; no account).
        return BoxArt.fetchToCache(ctx, sha, displayName)
    }

    // --- ScreenScraper -------------------------------------------------------

    /** RustyNES's ScreenScraper software identifier (the user's own ssid carries the
     *  quota; this just names the client). systemeid 3 = NES on ScreenScraper. */
    private const val SS_SOFTNAME = "RustyNES"
    private const val SS_SYSTEM_NES = 3

    /**
     * Resolve a ScreenScraper box-2D media URL by game name. Uses the user's ssid +
     * password. Returns null on any error / no match. NOTE: ScreenScraper also wants a
     * registered devid/devpassword for full throughput; without them, anonymous/user
     * calls are rate-limited but still resolve, which is fine for one-off lookups.
     */
    private fun screenScraperUrl(name: String, ssid: String, sspass: String): String? {
        val short = name.substringBeforeLast('.').substringBefore('(').trim()
        val q = "https://api.screenscraper.fr/api2/jeuInfos.php?" +
            "output=json&softname=${enc(SS_SOFTNAME)}" +
            "&ssid=${enc(ssid)}&sspassword=${enc(sspass)}" +
            "&systemeid=$SS_SYSTEM_NES&romtype=rom&romnom=${enc("$short.nes")}"
        val body = httpGet(q) ?: return null
        return runCatching {
            val medias = JSONObject(body)
                .getJSONObject("response")
                .getJSONObject("jeu")
                .getJSONArray("medias")
            for (i in 0 until medias.length()) {
                val m = medias.getJSONObject(i)
                if (m.optString("type") == "box-2D") return@runCatching m.optString("url").ifEmpty { null }
            }
            null
        }.getOrNull()
    }

    // --- TheGamesDB ----------------------------------------------------------

    /**
     * Resolve a TheGamesDB box-art front URL by game name using the user's API key.
     * Searches by name, then reads `include=boxart` -> base_url + the first front
     * boxart filename for the matched game. Returns null on any error / no match.
     */
    private fun theGamesDbUrl(name: String, apiKey: String): String? {
        val short = name.substringBeforeLast('.').substringBefore('(').trim()
        val q = "https://api.thegamesdb.net/v1/Games/ByGameName?" +
            "apikey=${enc(apiKey)}&name=${enc(short)}&filter%5Bplatform%5D=7&include=boxart"
        val body = httpGet(q) ?: return null
        return runCatching {
            val root = JSONObject(body)
            val games = root.getJSONObject("data").getJSONArray("games")
            if (games.length() == 0) return@runCatching null
            val gameId = games.getJSONObject(0).getInt("id")
            val include = root.getJSONObject("include").getJSONObject("boxart")
            val baseUrl = include.getJSONObject("base_url").optString("original")
            val data = include.getJSONObject("data")
            if (!data.has(gameId.toString())) return@runCatching null
            val arts = data.getJSONArray(gameId.toString())
            for (i in 0 until arts.length()) {
                val a = arts.getJSONObject(i)
                if (a.optString("side") == "front" && a.optString("type") == "boxart") {
                    return@runCatching baseUrl + a.optString("filename")
                }
            }
            null
        }.getOrNull()
    }

    // --- shared HTTP ---------------------------------------------------------

    private const val MAX_BYTES = 4 * 1024 * 1024

    private fun enc(s: String) = URLEncoder.encode(s, "UTF-8")

    private fun httpGet(urlStr: String): String? {
        val conn = (URL(urlStr).openConnection() as HttpURLConnection).apply {
            connectTimeout = 8000
            readTimeout = 8000
            setRequestProperty("User-Agent", "RustyNES-Android")
        }
        return try {
            if (conn.responseCode != HttpURLConnection.HTTP_OK) return null
            conn.inputStream.bufferedReader().use { it.readText() }
        } catch (_: Exception) {
            null
        } finally {
            conn.disconnect()
        }
    }

    private fun downloadTo(urlStr: String, out: File): Boolean {
        val conn = (URL(urlStr).openConnection() as HttpURLConnection).apply {
            connectTimeout = 8000
            readTimeout = 8000
            instanceFollowRedirects = true
            setRequestProperty("User-Agent", "RustyNES-Android")
        }
        return try {
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
            if (tmp.length() == 0L) {
                tmp.delete()
                false
            } else {
                // Replace any existing file, then move the temp into place. renameTo
                // fails (returns false) if the destination exists on some filesystems,
                // so delete first and clean up the temp if the move still fails.
                if (out.exists()) out.delete()
                if (!tmp.renameTo(out)) { tmp.delete(); false } else out.length() > 0
            }
        } catch (_: Exception) {
            false
        } finally {
            conn.disconnect()
        }
    }
}
