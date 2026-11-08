package com.doublegate.rustynes

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * v3.7.0 (D24 row 3, `T-RA-HARDCORE-RESUME`): the pure decisions behind the
 * equalizer, the Game Genie picks and the hardcore auto-resume notice, on the JVM.
 * The filter, the catalog and the correction are the bridge's, tested on the host by
 * `cargo test -p rustynes-mobile` (its `extras` module).
 */
class MobileExtrasTest {
    // --- T-RA-HARDCORE-RESUME: the decision path ---------------------------------

    /** Hardcore with RA enabled never loads the on-background state, and says why. */
    @Test
    fun hardcore_skips_the_resume_and_shows_the_notice() {
        val d = AutoResume.decide(hasSlot = true, raEnabled = true, hardcore = true)
        assertEquals(AutoResume.SKIP_HARDCORE, d)
        assertTrue(d.showsNotice)
    }

    /** Casual RA, RA off, or RA off with a stale hardcore switch: the state loads. */
    @Test
    fun outside_hardcore_the_state_loads_silently() {
        for ((ra, hc) in listOf(true to false, false to false, false to true)) {
            val d = AutoResume.decide(hasSlot = true, raEnabled = ra, hardcore = hc)
            assertEquals("ra=$ra hardcore=$hc", AutoResume.LOAD, d)
            assertFalse(d.showsNotice)
        }
    }

    /** Without a saved state there is nothing to refuse, so no notice either way. */
    @Test
    fun no_slot_is_a_plain_power_on_without_a_notice() {
        for (ra in listOf(true, false)) for (hc in listOf(true, false)) {
            val d = AutoResume.decide(hasSlot = false, raEnabled = ra, hardcore = hc)
            assertEquals(AutoResume.NONE, d)
            assertFalse(d.showsNotice)
        }
    }

    /** A load the bridge itself refused for hardcore gets the notice; any other failure none. */
    @Test
    fun a_bridge_refusal_for_hardcore_also_gets_the_notice() {
        assertEquals(AutoResume.SKIP_HARDCORE, AutoResume.afterRefusal(true))
        assertTrue(AutoResume.afterRefusal(true).showsNotice)
        assertEquals(AutoResume.NONE, AutoResume.afterRefusal(false))
        assertFalse(AutoResume.afterRefusal(false).showsNotice)
    }

    // --- the equalizer's stored gains -------------------------------------------

    @Test
    fun eq_gains_round_trip() {
        // -12.0 .. +10.8 dB, all inside the slider range (decode clamps to +/-12).
        val g = FloatArray(EqPrefs.BANDS) { (it - 10) * 1.2f }
        assertArrayEquals(g, EqPrefs.decode(EqPrefs.encode(g)), 0f)
    }

    /** Absent, wrong-length or unparsable preferences are the flat bank (EQ off). */
    @Test
    fun a_bad_stored_preference_is_flat() {
        for (bad in listOf(null, "", "1,2,3", (0 until 21).joinToString(","), "a" + ",0".repeat(19))) {
            val g = EqPrefs.decode(bad)
            assertEquals(EqPrefs.BANDS, g.size)
            assertTrue("$bad", EqPrefs.isFlat(g))
        }
    }

    /** Out-of-range values clamp to the slider range; non-finite ones become 0 dB. */
    @Test
    fun stored_gains_are_clamped_and_never_nan() {
        val stored = (listOf("40", "-40", "NaN", "Infinity") + List(16) { "0" }).joinToString(",")
        val g = EqPrefs.decode(stored)
        assertEquals(12f, g[0], 0f)
        assertEquals(-12f, g[1], 0f)
        assertEquals(0f, g[2], 0f)
        assertEquals(0f, g[3], 0f)
        assertTrue(g.all { it.isFinite() })
    }

    @Test
    fun band_labels() {
        assertEquals("25 Hz", EqPrefs.bandLabel(25f))
        assertEquals("630 Hz", EqPrefs.bandLabel(630f))
        assertEquals("1 kHz", EqPrefs.bandLabel(1000f))
        assertEquals("1.6 kHz", EqPrefs.bandLabel(1600f))
        assertEquals("12.5 kHz", EqPrefs.bandLabel(12500f))
    }

    // --- the per-game Game Genie picks ------------------------------------------

    @Test
    fun picks_round_trip_and_drop_junk() {
        val set = setOf("SXIOPO", "AEKPTZGA")
        assertEquals(set, CheatPicks.decode(CheatPicks.encode(set)))
        assertEquals(setOf("SXIOPO"), CheatPicks.decode("sxiopo,,X,TOOLONGCODE,12345678, SXIOPO "))
        assertTrue(CheatPicks.decode(null).isEmpty())
    }

    @Test
    fun toggling_a_pick() {
        val a = CheatPicks.toggle(emptySet(), "sxiopo", true)
        assertEquals(setOf("SXIOPO"), a)
        assertEquals(emptySet<String>(), CheatPicks.toggle(a, "SXIOPO", false))
    }
}
