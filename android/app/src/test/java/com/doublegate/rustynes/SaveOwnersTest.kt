package com.doublegate.rustynes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * v3.2.0: the fence that decides which controller may write a SHA-keyed FDS
 * save. Review on #604 found three ways a write went to the wrong place or
 * was lost:
 *
 * - the game loop paired the controller it captured with the ROM identity it
 *   read later, across a ROM switch, sending the old disk to the new game's
 *   file (Cursor);
 * - a flush queued by the outgoing controller ran after the incoming one had
 *   written the same file, on reopening the same disk (CodeRabbit);
 * - the first fix claimed ownership later, on an I/O thread, so a close or a
 *   pause before the claim was refused and never retried, and two quick
 *   switches could install a retired controller as owner (Cursor, on the fix).
 *
 * Ownership therefore moves synchronously ([SaveOwners.transfer]), and the
 * outgoing controller keeps a RETIRING right to its key until the new owner
 * first writes it.
 */
class SaveOwnersTest {
    private val a = "a".repeat(64)
    private val b = "b".repeat(64)

    /** A controller that was never given a key cannot write it. */
    @Test
    fun only_the_owner_may_write() {
        val owners = SaveOwners<Any>()
        val ctrl = Any()
        assertFalse(owners.write(a, ctrl) { error("must not run") })
        owners.transfer(null, null, a, ctrl)
        assertTrue(owners.write(a, ctrl) {})
    }

    /** Cursor's first case: the old controller paired with the NEW game's key. */
    @Test
    fun an_outgoing_controller_cannot_write_the_incoming_key() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        owners.transfer(null, null, a, old)
        owners.transfer(a, old, b, new)
        assertFalse(owners.write(b, old) { error("old image into the new game's file") })
        assertTrue(owners.write(b, new) {})
    }

    /** The outgoing controller's final flush lands on its own key after the switch. */
    @Test
    fun the_outgoing_final_flush_is_allowed_on_its_own_key() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        owners.transfer(null, null, a, old)
        owners.transfer(a, old, b, new)
        assertTrue(owners.write(a, old) {})
        owners.finish(a, old)
        assertFalse(owners.write(a, old) { error("finished: nothing may write for it") })
    }

    /**
     * CodeRabbit's case: the same disk reopened. The old controller may write
     * until the new one does, and never after, so an older image can never
     * replace a newer one.
     */
    @Test
    fun reopening_the_same_key_never_writes_old_over_new() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        val log = mutableListOf<String>()
        owners.transfer(null, null, a, old)
        owners.transfer(a, old, a, new)
        assertTrue(owners.write(a, old) { log += "old final" })
        assertTrue(owners.write(a, new) { log += "new" })
        assertFalse(owners.write(a, old) { log += "old stale" })
        assertEquals(listOf("old final", "new"), log)
    }

    /** Cursor's case on the first fix: a close straight after a load still writes. */
    @Test
    fun a_close_straight_after_a_load_writes() {
        val owners = SaveOwners<Any>()
        val ctrl = Any()
        owners.transfer(null, null, a, ctrl)
        var wrote = false
        owners.close(a, ctrl) { wrote = true }
        assertTrue(wrote)
        assertFalse(owners.write(a, ctrl) { error("closed") })
    }

    /** Cursor's other case: two quick switches leave the LIVE controller owning the key. */
    @Test
    fun two_quick_switches_leave_the_live_controller_owning() {
        val owners = SaveOwners<Any>()
        val first = Any()
        val second = Any()
        val live = Any()
        owners.transfer(null, null, a, first)
        owners.transfer(a, first, a, second)
        owners.transfer(a, second, a, live)
        assertTrue(owners.write(a, live) {})
        assertFalse(owners.write(a, first) { error("long retired") })
        assertFalse(owners.write(a, second) { error("the live owner has written") })
    }

    /** A close by a controller that no longer owns the key changes nothing. */
    @Test
    fun a_stale_close_is_ignored() {
        val owners = SaveOwners<Any>()
        val old = Any()
        val new = Any()
        owners.transfer(null, null, a, old)
        owners.transfer(a, old, a, new)
        owners.close(a, old) {}
        assertTrue(owners.write(a, new) {})
    }
}
