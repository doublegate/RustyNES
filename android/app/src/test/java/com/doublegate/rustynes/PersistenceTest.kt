package com.doublegate.rustynes

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.IOException

/**
 * v2.7.4 (frontend audit AND-05 / AND-09): the persistence rules, on the JVM.
 * The app had no unit tests before this release; these pin the two things a
 * device test cannot reach reliably -- an interrupted write, and a save file of
 * the wrong size.
 */
class PersistenceTest {
    @get:Rule
    val tmp = TemporaryFolder()

    /** A write interrupted part-way leaves the previous contents, not a mix. */
    @Test
    fun an_interrupted_write_keeps_the_previous_contents() {
        val f = tmp.root.resolve("states/abc/1.rns")
        writeAtomic(f, byteArrayOf(1, 2, 3, 4))
        try {
            writeAtomic(f) { out ->
                out.write(byteArrayOf(9, 9))
                throw IOException("killed mid-write")
            }
        } catch (_: IOException) {
            // expected
        }
        assertArrayEquals(byteArrayOf(1, 2, 3, 4), f.readBytes())
        // No side file is left behind to be mistaken for a save.
        assertEquals(listOf("1.rns"), f.parentFile!!.list()!!.sorted())
    }

    /** A completed write replaces the contents and creates missing directories. */
    @Test
    fun a_completed_write_replaces_the_contents() {
        val f = tmp.root.resolve("new/dir/recents.tsv")
        writeAtomic(f, "a".toByteArray())
        writeAtomic(f, "bb".toByteArray())
        assertEquals("bb", f.readText())
    }

    private fun saver() = BatterySaver(tmp.root.resolve("battery/rom.sav"))

    /** No file: nothing to load, and the first change is written. */
    @Test
    fun a_new_battery_save_is_written_on_the_first_change() {
        val s = saver()
        assertTrue(s.read(8) is SavedBattery.None)
        s.baseline(ByteArray(8))
        assertFalse("power-on RAM is not a save", s.flushIfChanged(ByteArray(8)))
        assertTrue(s.flushIfChanged(byteArrayOf(0xA5.toByte(), 0, 0, 0, 0, 0, 0, 0)))
        val back = saver().read(8)
        assertTrue(back is SavedBattery.Found)
        assertEquals(0xA5.toByte(), (back as SavedBattery.Found).bytes[0])
    }

    /** An unchanged save is not rewritten. */
    @Test
    fun an_unchanged_battery_save_is_not_rewritten() {
        val s = saver()
        val bytes = byteArrayOf(1, 2, 3, 4, 5, 6, 7, 8)
        assertTrue(s.flushIfChanged(bytes))
        assertFalse(s.flushIfChanged(bytes.copyOf()))
    }

    /** A save of the wrong size is refused and never overwritten that session. */
    @Test
    fun a_battery_save_of_the_wrong_size_is_left_untouched() {
        val f = tmp.root.resolve("battery/rom.sav")
        f.parentFile!!.mkdirs()
        f.writeBytes(ByteArray(32) { 7 })
        val s = BatterySaver(f)
        assertTrue(s.read(8) is SavedBattery.Unusable)
        assertFalse(s.flushIfChanged(ByteArray(8) { 1 }))
        assertArrayEquals(ByteArray(32) { 7 }, f.readBytes())
    }

    /**
     * The size is checked from metadata BEFORE the file is read: a save that is
     * huge or never ends must not be pulled into memory first. `/dev/zero`
     * reports length 0 and never ends, so reading it first runs the heap out.
     * (Linux / macOS only; the test is skipped where `/dev/zero` is absent.)
     */
    @Test
    fun a_wrong_size_battery_save_is_refused_without_reading_it() {
        val zero = java.io.File("/dev/zero")
        assumeTrue(zero.exists())
        val link = tmp.root.resolve("battery/rom.sav")
        link.parentFile!!.mkdirs()
        java.nio.file.Files.createSymbolicLink(link.toPath(), zero.toPath())
        val saved = BatterySaver(link).read(8)
        assertTrue(saved is SavedBattery.Unusable)
        // Refused by the metadata check. Reading first also ends in `Unusable` --
        // `read` catches the OutOfMemoryError the read provokes -- so the reason
        // is what tells the two apart, and the spike is what the check prevents.
        assertTrue(
            (saved as SavedBattery.Unusable).reason,
            saved.reason.startsWith("The battery save is 0 bytes"),
        )
    }

    /** A saver the bridge refused (disabled) never writes. */
    @Test
    fun a_disabled_battery_saver_never_writes() {
        val s = saver()
        s.disable()
        assertFalse(s.flushIfChanged(ByteArray(8) { 1 }))
        assertFalse(tmp.root.resolve("battery/rom.sav").exists())
    }
}
