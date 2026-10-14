package com.doublegate.rustynes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * v3.2.0: the fence that decides which controller may write a SHA-keyed FDS
 * save. Review on #604 found two ways an OUTGOING controller's disk image
 * reached a file it no longer owned: the game loop paired the controller it
 * captured with the ROM identity it read later, across a ROM switch (Cursor),
 * and a flush queued before the switch ran after the new controller had
 * written the same file, on reopening the same disk (CodeRabbit). Both are
 * writes from a controller that is not the file's current owner.
 */
class SaveOwnersTest {
    private val a = "a".repeat(64)
    private val b = "b".repeat(64)

    /** The first load claims its key; a controller that never claimed is refused. */
    @Test
    fun only_the_claimant_may_write() {
        val owners = SaveOwners<Any>()
        val ctrl = Any()
        assertFalse(owners.write(a, ctrl) { error("must not run") })
        owners.handOver(null, null, a, ctrl) { error("nothing outgoing") }
        assertTrue(owners.write(a, ctrl) {})
    }

    /** Cursor's case: the old controller paired with the NEW game's key. */
    @Test
    fun an_outgoing_controller_cannot_write_the_incoming_key() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        owners.handOver(null, null, a, old) {}
        owners.handOver(a, old, b, new) {}
        assertFalse(owners.write(b, old) { error("old image into the new game's file") })
        assertFalse(owners.write(a, old) { error("old no longer owns its key either") })
        assertTrue(owners.write(b, new) {})
    }

    /**
     * CodeRabbit's case: the same disk reopened. The outgoing final flush runs
     * inside the handover, before the claim, and a flush the old controller
     * queued earlier is refused once the new one owns the key, so it can never
     * land after the new controller's write.
     */
    @Test
    fun reopening_the_same_key_orders_the_final_flush_first() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        val log = mutableListOf<String>()
        owners.handOver(null, null, a, old) {}
        owners.handOver(a, old, a, new) { log += "old final" }
        assertTrue(owners.write(a, new) { log += "new" })
        assertFalse(owners.write(a, old) { log += "old stale" })
        assertEquals(listOf("old final", "new"), log)
    }

    /** Closing a ROM releases the key, so nothing writes it until the next claim. */
    @Test
    fun release_ends_ownership() {
        val owners = SaveOwners<Any>()
        val ctrl = Any()
        owners.handOver(null, null, a, ctrl) {}
        owners.release(a, ctrl)
        assertFalse(owners.write(a, ctrl) { error("released") })
    }

    /** A release by a controller that no longer owns the key leaves the owner alone. */
    @Test
    fun a_stale_release_is_ignored() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        owners.handOver(null, null, a, old) {}
        owners.handOver(a, old, a, new) {}
        owners.release(a, old)
        assertTrue(owners.write(a, new) {})
    }
}
