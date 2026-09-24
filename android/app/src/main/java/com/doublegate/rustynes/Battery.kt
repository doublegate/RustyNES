package com.doublegate.rustynes

import android.content.Context
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Cartridge battery saves on Android (v2.7.4, frontend audit AND-09 / MOB-05).
 *
 * Until v2.7.4 the app kept no in-game save: the bridge exposed no battery RAM,
 * so a game saved in-game, force-stopped and reopened came back without it,
 * except inside the auto save-state. The bridge now exports and imports it
 * (`NesController.hasBattery` / `batteryRam` / `loadBatteryRam`), and this file
 * keeps it at `filesDir/battery/<rom-sha256>.sav`, keyed by the same hash as the
 * save-state directory. The rules are the desktop's (docs/frontend.md, "Battery
 * saves"), so the two hosts behave alike:
 *
 * - only a cartridge whose header sets the battery bit is persisted;
 * - a `.sav` of the wrong size is never loaded and, that session, never
 *   overwritten: it is more likely another game's or another emulator's file
 *   than a damaged one of ours, and overwriting it would destroy it;
 * - "clean" means equal to the last bytes written; there is no dirty flag;
 * - writes go through [writeAtomic].
 */
object BatteryStore {
    /** The `.sav` for the ROM with SHA-256 [sha]. */
    fun file(ctx: Context, sha: String): File = File(ctx.filesDir, "battery/$sha.sav")
}

/** What [BatterySaver.read] found on disk. */
sealed interface SavedBattery {
    /** No save yet; the game starts from its power-on RAM. */
    data object None : SavedBattery

    /** A save of the cartridge's size, to load before the first frame. */
    class Found(val bytes: ByteArray) : SavedBattery

    /** A file that must not be loaded or overwritten; [reason] is for the user. */
    class Unusable(val reason: String) : SavedBattery
}

/**
 * One cartridge's battery RAM bound to its `.sav` [file]. Pure JVM, so the rules
 * are pinned by `BatterySaverTest` without a device.
 *
 * Thread-safety: the periodic flush runs on `Dispatchers.IO` while the lifecycle
 * flushes run on the main thread, so [flushIfChanged] is synchronized; [tryBegin]
 * keeps the loop from queueing a second periodic flush behind a slow one.
 */
class BatterySaver(private val file: File) {
    private var last: ByteArray? = null

    @Volatile
    private var disabled = false

    private val busy = AtomicBoolean(false)

    /**
     * Read the save, checking its size against the cartridge's ([expected]
     * bytes) BEFORE reading it, so a huge or wrong file is never loaded into
     * memory. An unusable file disables this saver for the session.
     */
    fun read(expected: Int): SavedBattery {
        if (!file.exists()) return SavedBattery.None
        val length = file.length()
        if (length != expected.toLong()) {
            disabled = true
            return SavedBattery.Unusable(
                "The battery save is $length bytes; this game's is $expected. " +
                    "It was left untouched and this session will not be saved.",
            )
        }
        val bytes = runCatching { file.readBytes() }.getOrElse { e ->
            disabled = true
            return SavedBattery.Unusable("The battery save could not be read (${e.message}).")
        }
        if (bytes.size != expected) {
            disabled = true
            return SavedBattery.Unusable("The battery save changed size while it was read.")
        }
        return SavedBattery.Found(bytes)
    }

    /** Stop persisting this session (a save the bridge refused must not be overwritten). */
    fun disable() {
        disabled = true
    }

    /** Record what the cartridge now holds (after a load, or at power-on) as clean. */
    @Synchronized
    fun baseline(current: ByteArray) {
        last = current.copyOf()
    }

    /**
     * Write [current] if it differs from the last write. Returns whether it wrote.
     * A failed write throws and leaves the baseline alone, so the next flush
     * retries; the file keeps its previous contents ([writeAtomic]).
     */
    @Synchronized
    fun flushIfChanged(current: ByteArray): Boolean {
        if (disabled || current.isEmpty()) return false
        if (last?.contentEquals(current) == true) return false
        writeAtomic(file, current)
        last = current.copyOf()
        return true
    }

    /** Claim the periodic-flush slot; false while one is already running. */
    fun tryBegin(): Boolean = busy.compareAndSet(false, true)

    /** Release the periodic-flush slot. */
    fun end() = busy.set(false)
}
