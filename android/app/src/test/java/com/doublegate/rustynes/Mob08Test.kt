package com.doublegate.rustynes

import android.view.KeyEvent
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * v3.7.0 (MOB-08): the decisions behind the Kotlin TODOs v3.7.0 fixed. The sites
 * recorded rather than fixed (clip audio, the list-detail rail, the cloud-save
 * merge, Tink, the rest of the i18n sweep) are listed with their reasons in
 * docs/android.md.
 */
class Mob08Test {
    // --- States.kt: live save-state thumbnails ----------------------------------

    @Test
    fun the_thumbnail_samples_each_cells_centre() {
        val w = 256
        val h = 240
        val frame = IntArray(w * h) { i -> (0xFF shl 24) or ((i % w) shl 8) or (i / w) }
        val t = Thumbs.downscale(frame, w, h)!!
        assertEquals(Thumbs.W * Thumbs.H, t.size)
        // Output (x, y) samples source (4x + 2, 4y + 2).
        for ((x, y) in listOf(0 to 0, 63 to 59, 10 to 20, 31 to 7)) {
            assertEquals("($x,$y)", frame[(4 * y + 2) * w + 4 * x + 2], t[y * Thumbs.W + x])
        }
    }

    @Test
    fun a_full_size_downscale_is_the_identity_and_a_bad_buffer_is_refused() {
        val frame = IntArray(256 * 240) { it * 7 }
        assertArrayEquals(frame, Thumbs.downscale(frame, 256, 240, 256, 240))
        assertNull("no frame yet", Thumbs.downscale(IntArray(0), 256, 240))
        assertNull("an HD-pack sized frame", Thumbs.downscale(IntArray(512 * 480), 256, 240))
    }

    // --- Library.kt: the tile menu on a d-pad-only TV ---------------------------

    @Test
    fun menu_y_and_a_long_centre_press_open_the_tile_menu() {
        assertTrue(TileKeys.opensMenu(KeyEvent.KEYCODE_MENU, false))
        assertTrue(TileKeys.opensMenu(KeyEvent.KEYCODE_BUTTON_Y, false))
        for (k in listOf(KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER)) {
            assertTrue("long $k", TileKeys.opensMenu(k, true))
            assertFalse("a short $k press stays play", TileKeys.opensMenu(k, false))
        }
        for (k in listOf(KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_BACK)) {
            assertFalse("$k", TileKeys.opensMenu(k, true))
        }
    }

    // --- LibraryImport.kt: the folder walk --------------------------------------

    /** The walk was unbounded; it now stops below MAX_DEPTH and never repeats a folder. */
    @Test
    fun the_import_walk_is_bounded_and_never_repeats_a_folder() {
        val seen = HashSet<String>()
        assertTrue(ImportWalk.shouldScan(0, "root", seen))
        assertFalse("a folder already scanned", ImportWalk.shouldScan(1, "root", seen))
        assertTrue(ImportWalk.shouldScan(ImportWalk.MAX_DEPTH, "deep", seen))
        assertFalse("past the bound", ImportWalk.shouldScan(ImportWalk.MAX_DEPTH + 1, "deeper", seen))
        // A chain of folders, each the child of the last, is scanned to the bound.
        val chain = HashSet<String>()
        var depth = 0
        while (ImportWalk.shouldScan(depth, "d$depth", chain)) depth++
        assertEquals(ImportWalk.MAX_DEPTH + 1, depth)
    }

    // --- BoxArt.kt: the libretro fallback (the stale TODO) ----------------------

    @Test
    fun box_art_candidates_are_full_then_short_name() {
        val base = "https://raw.githubusercontent.com/libretro-thumbnails/" +
            "Nintendo_-_Nintendo_Entertainment_System/master/Named_Boxarts/"
        assertEquals(
            listOf(base + "Mega%20Man%202%20(USA).png", base + "Mega%20Man%202.png"),
            BoxArt.candidateUrls("Mega Man 2 (USA).nes"),
        )
        assertEquals(listOf(base + "Tetris.png"), BoxArt.candidateUrls("Tetris.nes"))
        // The same strings the bridge's `box_art_candidate_urls` test pins for iOS.
        assertEquals(
            base + "Mario%20_%20Yoshi_%20A_B%20(Japan).png",
            BoxArt.candidateUrls("Mario & Yoshi: A/B (Japan).zip")[0],
        )
        assertEquals(
            base + "Kirby's%20Adventure,%20Pok%C3%A9mon%20(USA).png",
            BoxArt.candidateUrls("Kirby's Adventure, Pokémon (USA).nes")[0],
        )
    }
}
