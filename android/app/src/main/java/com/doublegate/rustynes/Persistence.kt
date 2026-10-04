package com.doublegate.rustynes

import android.content.Context
import androidx.core.util.AtomicFile
import org.json.JSONArray
import org.json.JSONObject
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

/**
 * Lowercase hex SHA-256 of the bytes as given. Until v2.9.9 this was every per-game
 * store's key (the whole file, header and any `.zip` container included); it is now
 * the legacy key [RomKeyMigration] moves stores from. The key is the core's
 * identity, `NesController.romIdentity()` / `romIdentityOfFile`.
 */
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

/**
 * The Famicom Disk System BIOS the user chose (v2.9.7 "Tandem", plan item 6).
 *
 * FDS disks boot only with `disksys.rom`, an 8 KiB Nintendo BIOS the app never
 * ships. The first time a disk fails to load with `MissingFdsBios`, the app asks
 * for the file once and keeps a private copy in `filesDir/fds/disksys.rom`; every
 * later load hands it to `NesController.newWithFdsBios`. The bridge validates the
 * size too -- this check only avoids storing a file that could never work.
 */
object FdsBios {
    /** The exact size of the FDS BIOS. */
    const val SIZE = 8 * 1024

    private fun file(ctx: Context) = File(File(ctx.filesDir, "fds"), "disksys.rom")

    /** The stored BIOS, or null when none has been chosen (or it is unreadable). */
    fun load(ctx: Context): ByteArray? =
        runCatching { file(ctx).takeIf { it.isFile }?.readBytes() }.getOrNull()
            ?.takeIf { it.size == SIZE }

    /** Store [bytes] as the BIOS. Returns false (and stores nothing) unless 8 KiB. */
    fun save(ctx: Context, bytes: ByteArray): Boolean {
        if (bytes.size != SIZE) return false
        writeAtomic(file(ctx), bytes)
        return true
    }
}

/**
 * v2.9.9 (re-audit NF-21) — the one-time move of a game's stores from the key the
 * app used until v2.9.9 to the core's ROM identity.
 *
 * Until v2.9.9 every per-game store here was keyed by [sha256Hex] of the bytes as
 * read: the whole file, header included, and for a `.zip` the ARCHIVE. v2.9.8 moved
 * the core's identity (`Nes::rom_sha256`, the key the desktop gives slots, `.sav`
 * and cheats, and what movies and netplay record) to the bytes after the 16-byte
 * iNES header, of the unpacked image. So on Android two dumps differing only in
 * their header did not share saves, a re-zipped ROM lost them, and no key matched
 * the desktop's. The app now keys by `NesController.romIdentity()` and, the first
 * time a game is opened under it, moves what the old key held.
 *
 * The rules, pinned by `RomKeyMigrationTest`:
 *
 * - a store moves only into an EMPTY new key; one already there is never
 *   overwritten, and the old copy is then left where it was (an orphan, not a loss);
 * - a file is copied with [writeAtomic], read back and compared, and only then is
 *   the original deleted, so a failure at any point leaves the user's copy;
 * - a store counts as moved only once its original is gone; a run that copied a
 *   file but could not delete the original finishes the move on the next run,
 *   which finds the identical copy already in place (every open runs this, with
 *   the legacy key recomputed from the file, so a partial run is always retried);
 * - equal keys (an FDS disk, an NSF or a UNIF board opened unzipped, whose
 *   identity IS the whole image) touch nothing, and a second run finds nothing.
 *
 * Moved: `battery/<k>.sav`, every file in `states/<k>/` (auto-resume, the slots and
 * their thumbnails), `ra-progress/<k>.rap`, `boxart/<k>.png`, the `game_config.json`
 * entry and the `library.json` entry (its user fields included). NOT moved: Play
 * Games cloud snapshots (`rns.<k>.<slot>`, remote), which the next push re-creates
 * under the new key.
 */
object RomKeyMigration {
    /**
     * Move every store keyed by [legacy] under [filesDir] to [identity]. Returns how
     * many stores moved (a file, or a JSON entry). Never throws for a store that
     * cannot be moved; that store stays under [legacy].
     */
    fun migrate(filesDir: File, legacy: String, identity: String): Int {
        if (legacy.isEmpty() || identity.isEmpty() || legacy == identity) return 0
        var moved = 0
        moved += moveFile(File(filesDir, "battery/$legacy.sav"), File(filesDir, "battery/$identity.sav"))
        moved += moveDir(File(filesDir, "states/$legacy"), File(filesDir, "states/$identity"))
        moved += moveFile(
            File(filesDir, "ra-progress/$legacy.rap"),
            File(filesDir, "ra-progress/$identity.rap"),
        )
        moved += moveFile(File(filesDir, "boxart/$legacy.png"), File(filesDir, "boxart/$identity.png"))
        moved += moveConfigKey(File(filesDir, "game_config.json"), legacy, identity)
        moved += moveLibraryEntry(File(filesDir, "library.json"), legacy, identity)
        return moved
    }

    /**
     * Copy [src] to an absent [dst], verify it, then delete [src]. 1 only when [src]
     * is gone and [dst] holds its bytes; 0 for anything else, [src] then staying.
     *
     * A [dst] that already holds exactly [src]'s bytes is a move an earlier run left
     * half done: the copy was written and verified but `File.delete` failed (its
     * result used to be ignored, so that run still counted the file as moved). The
     * leftover is a duplicate, not the only copy, so this run deletes it and counts
     * the move then. Without that step the second run saw [dst] taken and left the
     * duplicate forever. A [dst] holding DIFFERENT bytes is the never-overwrite rule:
     * both stay. Every path is therefore safe to repeat on every launch.
     */
    private fun moveFile(src: File, dst: File): Int {
        if (!src.isFile) return 0
        return runCatching {
            val bytes = src.readBytes()
            if (dst.exists()) {
                if (!dst.isFile || !dst.readBytes().contentEquals(bytes)) return 0
            } else {
                writeAtomic(dst, bytes)
                check(dst.readBytes().contentEquals(bytes)) { "copy of $src differs" }
            }
            if (src.delete()) 1 else 0
        }.getOrDefault(0)
    }

    /**
     * [moveFile] for each file in [src]; removes [src] once it is empty. Not
     * `deleteRecursively`: a file kept by the never-overwrite rule, or one whose
     * move failed, is still the user's only copy, so the directory goes only when
     * nothing is left in it (a failed removal of the empty directory is retried by
     * the next run, which finds it empty again).
     */
    private fun moveDir(src: File, dst: File): Int {
        val files = src.listFiles()?.filter { it.isFile } ?: return 0
        val moved = files.sumOf { moveFile(it, File(dst, it.name)) }
        if (src.listFiles()?.isEmpty() == true) src.delete()
        return moved
    }

    /** Re-key the `game_config.json` object entry, if the new key is free. */
    private fun moveConfigKey(file: File, legacy: String, identity: String): Int {
        if (!file.isFile) return 0
        return runCatching {
            val all = JSONObject(file.readText())
            if (!all.has(legacy) || all.has(identity)) return 0
            all.put(identity, all.get(legacy))
            all.remove(legacy)
            writeAtomic(file, all.toString().toByteArray())
            1
        }.getOrDefault(0)
    }

    /** Re-key the `library.json` entry, keeping its user fields, if the new key is free. */
    private fun moveLibraryEntry(file: File, legacy: String, identity: String): Int {
        if (!file.isFile) return 0
        return runCatching {
            val arr = JSONArray(file.readText())
            val entries = (0 until arr.length()).map { GameEntry.fromJson(arr.getJSONObject(it)) }
            val idx = entries.indexOfFirst { it.sha == legacy }
            if (idx < 0 || entries.any { it.sha == identity }) return 0
            val out = JSONArray()
            entries.forEachIndexed { i, e -> out.put((if (i == idx) e.copy(sha = identity) else e).toJson()) }
            writeAtomic(file, out.toString().toByteArray())
            1
        }.getOrDefault(0)
    }
}
