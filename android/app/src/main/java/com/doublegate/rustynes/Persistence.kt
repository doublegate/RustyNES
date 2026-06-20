package com.doublegate.rustynes

import android.content.Context
import java.io.File
import java.security.MessageDigest

/**
 * On-device persistence for RustyNES (v1.8.0 Workstream E).
 *
 * Two stores, both rooted in the app-private `filesDir` (the Android analogue of
 * the desktop `ProjectDirs`): a recent-ROMs list keyed by persistable SAF content
 * URIs, and save-states keyed by ROM SHA-256 + slot — the same `<rom-sha256>/`
 * layout the desktop host uses, so the `.rns` blobs stay byte-identical and a
 * state is portable across devices.
 */

/** A recently-opened ROM: a persistable SAF content URI + its display name. */
data class RecentRom(val uri: String, val name: String)

/** Lowercase hex SHA-256 of the ROM bytes — the per-ROM save-state directory key. */
fun sha256Hex(bytes: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

/** The recent-ROMs list, persisted as tab-separated `uri\tname` lines (newest first). */
object RomLibrary {
    private const val MAX = 12
    private fun file(ctx: Context) = File(ctx.filesDir, "recents.tsv")

    fun recents(ctx: Context): List<RecentRom> {
        val f = file(ctx)
        if (!f.exists()) return emptyList()
        return f.readLines().mapNotNull { line ->
            val tab = line.indexOf('\t')
            if (tab <= 0) null else RecentRom(line.substring(0, tab), line.substring(tab + 1))
        }
    }

    /** Record (or promote) a ROM at the front of the list, de-duplicated by URI. */
    fun remember(ctx: Context, uri: String, name: String) {
        val updated = (listOf(RecentRom(uri, name)) + recents(ctx).filterNot { it.uri == uri }).take(MAX)
        file(ctx).writeText(updated.joinToString("\n") { "${it.uri}\t${it.name}" })
    }

    fun forget(ctx: Context, uri: String) {
        file(ctx).writeText(recents(ctx).filterNot { it.uri == uri }.joinToString("\n") { "${it.uri}\t${it.name}" })
    }

    /** Clear the entire recently-played list. */
    fun clear(ctx: Context) {
        file(ctx).delete()
    }
}

/**
 * RetroAchievements per-game progress sidecars (v1.8.6), stored at
 * `filesDir/ra-progress/<rom-sha256>.rap`. The RA session serializes its runtime
 * progress here on background / ROM unload and re-applies it on the next
 * `raLoadGame` of the same ROM, so unlock progress survives across launches.
 */
object RaProgressStore {
    private fun dir(ctx: Context) = File(ctx.filesDir, "ra-progress").apply { mkdirs() }

    private fun file(ctx: Context, sha: String) = File(dir(ctx), "$sha.rap")

    /** The saved progress sidecar for a ROM, or an empty array if none exists. */
    fun load(ctx: Context, sha: String): ByteArray {
        val f = file(ctx, sha)
        return if (f.exists()) f.readBytes() else ByteArray(0)
    }

    /** Persist the progress sidecar for a ROM (a no-op for an empty blob). */
    fun save(ctx: Context, sha: String, blob: ByteArray) {
        if (blob.isNotEmpty()) file(ctx, sha).writeBytes(blob)
    }
}

/**
 * Save-state slots, stored at `filesDir/states/<rom-sha256>/<slot>.rns`. The
 * `auto` slot is written on background and auto-loaded on the next open of the
 * same ROM (resume-where-you-left-off); numbered slots are explicit user saves.
 */
object SaveStateStore {
    const val AUTO_SLOT = "auto"

    /** The explicit, user-facing save slots (the manager UI, v1.8.3). */
    val USER_SLOTS = listOf("1", "2", "3", "4")

    private fun dir(ctx: Context, sha: String) =
        File(ctx.filesDir, "states/$sha").apply { mkdirs() }

    private fun slotFile(ctx: Context, sha: String, slot: String) =
        File(dir(ctx, sha), "$slot.rns")

    fun save(ctx: Context, sha: String, slot: String, blob: ByteArray) {
        slotFile(ctx, sha, slot).writeBytes(blob)
    }

    fun load(ctx: Context, sha: String, slot: String): ByteArray? {
        val f = slotFile(ctx, sha, slot)
        return if (f.exists()) f.readBytes() else null
    }

    fun exists(ctx: Context, sha: String, slot: String): Boolean =
        slotFile(ctx, sha, slot).exists()

    /** Last-write epoch millis for a slot, or 0 if it is empty. */
    fun lastModified(ctx: Context, sha: String, slot: String): Long {
        val f = slotFile(ctx, sha, slot)
        return if (f.exists()) f.lastModified() else 0L
    }

    fun delete(ctx: Context, sha: String, slot: String): Boolean =
        slotFile(ctx, sha, slot).delete()
}
