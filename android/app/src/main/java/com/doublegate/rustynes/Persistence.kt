package com.doublegate.rustynes

import android.content.Context
import androidx.core.util.AtomicFile
import java.io.File
import java.io.OutputStream
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

/**
 * Write a file so that a kill or power loss mid-write leaves either the old
 * contents or the new, never a truncated mix (v2.7.4, frontend audit AND-05).
 *
 * Every store here used `File.writeBytes` / `writeText`, which truncate the file
 * and then write it, with no temporary copy and no fsync: a low-memory kill or a
 * power loss inside that window destroyed the previous save of that slot, the
 * progress sidecar, or the recents list. `AtomicFile` writes a side file, fsyncs
 * it in [AtomicFile.finishWrite], and renames it over the original; on any
 * failure [AtomicFile.failWrite] discards the side file and the original stays.
 * The androidx class is used rather than `android.util.AtomicFile` because it is
 * plain Java, so the JVM unit tests exercise the real thing.
 */
fun writeAtomic(file: File, write: (OutputStream) -> Unit) {
    file.parentFile?.mkdirs()
    val atomic = AtomicFile(file)
    val out = atomic.startWrite()
    try {
        write(out)
        atomic.finishWrite(out)
    } catch (e: Throwable) {
        atomic.failWrite(out)
        throw e
    }
}

/** [writeAtomic] for a whole byte array. */
fun writeAtomic(file: File, bytes: ByteArray) = writeAtomic(file) { it.write(bytes) }

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
        writeAtomic(file(ctx), updated.joinToString("\n") { "${it.uri}\t${it.name}" }.toByteArray())
    }

    fun forget(ctx: Context, uri: String) {
        writeAtomic(
            file(ctx),
            recents(ctx).filterNot { it.uri == uri }
                .joinToString("\n") { "${it.uri}\t${it.name}" }.toByteArray(),
        )
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
        if (blob.isNotEmpty()) writeAtomic(file(ctx, sha), blob)
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
        writeAtomic(slotFile(ctx, sha, slot), blob)
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

    /** The `THM`-style PNG thumbnail beside a save slot (v1.8.8, Workstream C). */
    fun thumbFile(ctx: Context, sha: String, slot: String): File =
        File(dir(ctx, sha), "$slot.thm.png")

    /** Save a small PNG thumbnail for a slot from raw ARGB pixels (the framebuffer
     *  packed by the emulation loop). Best-effort: a failure to encode is swallowed
     *  (the thumbnail is purely cosmetic; the `.rns` is the real save). */
    fun saveThumb(ctx: Context, sha: String, slot: String, pixels: IntArray, w: Int, h: Int) {
        runCatching {
            val bmp = android.graphics.Bitmap.createBitmap(w, h, android.graphics.Bitmap.Config.ARGB_8888)
            try {
                bmp.setPixels(pixels, 0, w, 0, 0, w, h)
                val out = thumbFile(ctx, sha, slot)
                // The `states/<rom-sha256>` parent may not exist yet (e.g. thumbnail
                // saved before any `.rns`) -> ensure it before opening the stream.
                out.parentFile?.mkdirs()
                out.outputStream().use {
                    bmp.compress(android.graphics.Bitmap.CompressFormat.PNG, 90, it)
                }
            } finally {
                // Recycle on every path so an exception in setPixels/compress/IO can't
                // leak the bitmap.
                bmp.recycle()
            }
        }
    }
}
