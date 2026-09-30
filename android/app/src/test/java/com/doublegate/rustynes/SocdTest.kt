package com.doublegate.rustynes

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

/**
 * v2.9.2 (audit AUD-09): the on-screen pad's neutral SOCD cleaning, on the JVM.
 * Two fingers on the virtual D-pad could send Up + Down (or Left + Right) to the
 * console, which a real NES pad never does.
 */
class SocdTest {
    private val vertical = NesBit.UP or NesBit.DOWN
    private val horizontal = NesBit.LEFT or NesBit.RIGHT

    @Test
    fun opposing_directions_cancel_per_axis() {
        assertEquals(0, socdNeutral(vertical))
        assertEquals(0, socdNeutral(horizontal))
        assertEquals(NesBit.UP, socdNeutral(NesBit.UP or horizontal))
        assertEquals(NesBit.A, socdNeutral(NesBit.A or vertical or horizontal))
    }

    @Test
    fun unopposed_masks_pass_through() {
        for (m in 0..0xFF) {
            if (m and vertical == vertical || m and horizontal == horizontal) continue
            assertEquals("mask $m", m, socdNeutral(m))
        }
    }

    /** v2.9.7 "Tandem" (plan item 8): with "Cancel opposite directions" off, every
     *  mask -- opposing pairs included -- reaches the console untouched. */
    @Test
    fun turned_off_every_mask_passes_through() {
        for (m in 0..0xFF) {
            assertEquals("mask $m", m, socdNeutral(m, enabled = false))
        }
        assertEquals(vertical or horizontal, socdNeutral(vertical or horizontal, enabled = false))
    }

    /** The default is on: callers that pass no setting keep the v2.9.2 behaviour. */
    @Test
    fun default_is_on() {
        for (m in 0..0xFF) {
            assertEquals("mask $m", socdNeutral(m, enabled = true), socdNeutral(m))
        }
        assertEquals(0, socdNeutral(vertical or horizontal))
    }

    @Test
    fun opposites_never_survive_and_buttons_never_change() {
        val buttons = NesBit.A or NesBit.B or NesBit.SELECT or NesBit.START
        for (m in 0..0xFF) {
            val c = socdNeutral(m)
            assertFalse("mask $m", c and vertical == vertical)
            assertFalse("mask $m", c and horizontal == horizontal)
            assertEquals("mask $m", m and buttons, c and buttons)
        }
    }
}
