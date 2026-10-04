package com.doublegate.rustynes

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File

/**
 * v2.9.9 (re-audit NF-21): the one-time move of every per-game store from the
 * whole file's SHA-256 (the key until v2.9.9) to the core's ROM identity
 * (`NesController.romIdentity`). The rules: move only into an empty key, never
 * remove the old copy before the new one is written and read back equal, and
 * leave both keys alone when the new one is already taken.
 */
class RomKeyMigrationTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val legacy = "a".repeat(64)
    private val identity = "b".repeat(64)

    private fun file(path: String) = File(tmp.root, path)

    private fun put(path: String, bytes: ByteArray) {
        val f = file(path)
        f.parentFile!!.mkdirs()
        f.writeBytes(bytes)
    }

    /** Every store an old key had arrives under the new key, and the old one goes. */
    @Test
    fun every_store_moves_to_the_identity() {
        put("battery/$legacy.sav", byteArrayOf(1, 2, 3))
        put("states/$legacy/auto.rns", byteArrayOf(4))
        put("states/$legacy/1.rns", byteArrayOf(5))
        put("states/$legacy/1.thm.png", byteArrayOf(6))
        put("ra-progress/$legacy.rap", byteArrayOf(7))
        put("boxart/$legacy.png", byteArrayOf(8))
        put(
            "game_config.json",
            JSONObject().put(legacy, JSONObject().put("filter", 3)).toString().toByteArray(),
        )
        put(
            "library.json",
            JSONArray().put(
                GameEntry(sha = legacy, name = "Game", uri = "content://x", favorite = true).toJson(),
            ).toString().toByteArray(),
        )

        val moved = RomKeyMigration.migrate(tmp.root, legacy, identity)

        assertEquals(8, moved)
        assertArrayEquals(byteArrayOf(1, 2, 3), file("battery/$identity.sav").readBytes())
        assertArrayEquals(byteArrayOf(4), file("states/$identity/auto.rns").readBytes())
        assertArrayEquals(byteArrayOf(5), file("states/$identity/1.rns").readBytes())
        assertArrayEquals(byteArrayOf(6), file("states/$identity/1.thm.png").readBytes())
        assertArrayEquals(byteArrayOf(7), file("ra-progress/$identity.rap").readBytes())
        assertArrayEquals(byteArrayOf(8), file("boxart/$identity.png").readBytes())
        assertFalse(file("battery/$legacy.sav").exists())
        assertFalse("the emptied state directory is removed", file("states/$legacy").exists())
        assertFalse(file("ra-progress/$legacy.rap").exists())
        assertFalse(file("boxart/$legacy.png").exists())

        val config = JSONObject(file("game_config.json").readText())
        assertFalse(config.has(legacy))
        assertEquals(3, config.getJSONObject(identity).getInt("filter"))

        val lib = JSONArray(file("library.json").readText())
        assertEquals(1, lib.length())
        val entry = GameEntry.fromJson(lib.getJSONObject(0))
        assertEquals(identity, entry.sha)
        assertTrue("user-owned fields survive", entry.favorite)
    }

    /** A second run, or a run with nothing under the old key, does nothing. */
    @Test
    fun migration_is_one_time() {
        put("battery/$legacy.sav", byteArrayOf(1))
        assertEquals(1, RomKeyMigration.migrate(tmp.root, legacy, identity))
        assertEquals(0, RomKeyMigration.migrate(tmp.root, legacy, identity))
        assertArrayEquals(byteArrayOf(1), file("battery/$identity.sav").readBytes())
    }

    /** A store already under the new key is never overwritten, and the old copy stays. */
    @Test
    fun an_existing_identity_store_is_left_alone() {
        put("battery/$legacy.sav", byteArrayOf(1))
        put("battery/$identity.sav", byteArrayOf(2))
        put("states/$legacy/1.rns", byteArrayOf(3))
        put("states/$legacy/2.rns", byteArrayOf(4))
        put("states/$identity/1.rns", byteArrayOf(5))
        put(
            "game_config.json",
            JSONObject()
                .put(legacy, JSONObject().put("filter", 1))
                .put(identity, JSONObject().put("filter", 2))
                .toString().toByteArray(),
        )
        put(
            "library.json",
            JSONArray()
                .put(GameEntry(sha = legacy, name = "Old", uri = "").toJson())
                .put(GameEntry(sha = identity, name = "New", uri = "").toJson())
                .toString().toByteArray(),
        )

        // Only the free slot moves.
        assertEquals(1, RomKeyMigration.migrate(tmp.root, legacy, identity))

        assertArrayEquals(byteArrayOf(2), file("battery/$identity.sav").readBytes())
        assertArrayEquals("the old save is kept", byteArrayOf(1), file("battery/$legacy.sav").readBytes())
        assertArrayEquals(byteArrayOf(5), file("states/$identity/1.rns").readBytes())
        assertArrayEquals(byteArrayOf(3), file("states/$legacy/1.rns").readBytes())
        assertArrayEquals(byteArrayOf(4), file("states/$identity/2.rns").readBytes())
        assertFalse(file("states/$legacy/2.rns").exists())
        val config = JSONObject(file("game_config.json").readText())
        assertEquals(1, config.getJSONObject(legacy).getInt("filter"))
        assertEquals(2, config.getJSONObject(identity).getInt("filter"))
        assertEquals(2, JSONArray(file("library.json").readText()).length())
    }

    /** Equal keys (an FDS disk, an NSF, a UNIF board, opened unzipped) touch nothing. */
    @Test
    fun equal_keys_are_a_no_op() {
        put("battery/$legacy.sav", byteArrayOf(1))
        assertEquals(0, RomKeyMigration.migrate(tmp.root, legacy, legacy))
        assertArrayEquals(byteArrayOf(1), file("battery/$legacy.sav").readBytes())
    }

    /**
     * A copy that cannot be written leaves the only copy where it was: the new key's
     * parent is a FILE, so the write fails, and the old save must still be there.
     */
    @Test
    fun a_failed_copy_keeps_the_original() {
        put("states/$legacy/auto.rns", byteArrayOf(9))
        put("states/$identity", byteArrayOf(0)) // a file where the directory would go
        assertEquals(0, RomKeyMigration.migrate(tmp.root, legacy, identity))
        assertArrayEquals(byteArrayOf(9), file("states/$legacy/auto.rns").readBytes())
    }
}
