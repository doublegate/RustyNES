package com.doublegate.rustynes

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.random.Random

/**
 * v3.7.0 (`T-ANDROID-GPU-FRAMESKIP`): skipping the per-frame RGBA -> ARGB pack on the
 * GPU path must not change a single pixel any reader sees. The reference is the
 * v3.6.2 loop, which packed every frame unconditionally; at every point a reader
 * looks (a screenshot after any frame, every frame of a clip or of the external
 * display) the deferred [CpuFrame] must hold the same pixels. No throughput claim
 * is made here: that needs a device (mobile run sheet).
 */
class CpuFrameTest {
    private val n = 256 * 240

    private fun frame(rng: Random) = ByteArray(n * 4).also { rng.nextBytes(it) }

    /** The v3.6.2 behaviour: pack every frame. */
    private fun eagerPack(rgba: ByteArray) = IntArray(n).also { packRgbaToArgb(rgba, it) }

    /** A screenshot taken after ANY frame of a GPU-path run (no clip, no external
     *  display, so nothing forced a pack) sees exactly that frame. */
    @Test
    fun a_screenshot_sees_the_frame_the_always_pack_loop_showed() {
        val rng = Random(0x5EED)
        val cpu = CpuFrame(n)
        repeat(40) { f ->
            val rgba = frame(rng)
            val eager = CpuFrame.eagerEveryFrame(gpuPath = true, clipRecording = false, externalDisplay = false)
            assertFalse(eager)
            cpu.publish(rgba, eager)
            assertTrue("frame $f is deferred", cpu.isDeferred)
            // Screenshot on some frames only, as a player would.
            if (rng.nextInt(4) == 0) {
                assertTrue(cpu.materialize())
                assertArrayEquals("screenshot after frame $f", eagerPack(rgba), cpu.pixels)
                assertFalse(cpu.materialize())
                assertArrayEquals(eagerPack(rgba), cpu.pixels)
            }
        }
    }

    /** While a clip records (or the external display mirrors), every frame is packed
     *  as it is published: the clip's frames are the old loop's, frame for frame. */
    @Test
    fun a_clip_and_the_external_display_get_every_frame() {
        for ((clip, ext) in listOf(true to false, false to true, true to true)) {
            val rng = Random(42)
            val cpu = CpuFrame(n)
            repeat(20) { f ->
                val rgba = frame(rng)
                cpu.publish(rgba, CpuFrame.eagerEveryFrame(gpuPath = true, clipRecording = clip, externalDisplay = ext))
                assertFalse(cpu.isDeferred)
                assertArrayEquals("clip=$clip ext=$ext frame $f", eagerPack(rgba), cpu.pixels)
            }
        }
    }

    /** The Bitmap path is the picture itself: always packed. */
    @Test
    fun the_bitmap_path_always_packs() {
        for (clip in listOf(true, false)) for (ext in listOf(true, false)) {
            assertTrue(CpuFrame.eagerEveryFrame(gpuPath = false, clipRecording = clip, externalDisplay = ext))
        }
    }

    /** Switching a clip on mid-run: the first eager frame is current at once, and the
     *  deferred frames before it never leak into later reads. */
    @Test
    fun turning_a_clip_on_mid_run_is_current_immediately() {
        val rng = Random(7)
        val cpu = CpuFrame(n)
        repeat(5) { cpu.publish(frame(rng), eager = false) }
        val first = frame(rng)
        cpu.publish(first, eager = true)
        assertArrayEquals(eagerPack(first), cpu.pixels)
        assertFalse("nothing older is waiting", cpu.materialize())
        assertArrayEquals(eagerPack(first), cpu.pixels)
    }

    /** A direct write (the netplay index path) drops a remembered frame, so a later
     *  screenshot cannot replace it with a stale one. */
    @Test
    fun an_overwrite_drops_a_remembered_frame() {
        val rng = Random(9)
        val cpu = CpuFrame(n)
        cpu.publish(frame(rng), eager = false)
        cpu.overwrite { it.fill(0x11223344) }
        assertFalse(cpu.isDeferred)
        assertFalse(cpu.materialize())
        assertTrue(cpu.pixels.all { it == 0x11223344 })
    }

    @Test
    fun the_pack_is_rgba_to_opaque_argb() {
        val rgba = ByteArray(n * 4)
        rgba[0] = 0x12; rgba[1] = 0x34; rgba[2] = 0x56; rgba[3] = 0x00
        rgba[4] = 0xFF.toByte(); rgba[5] = 0x80.toByte(); rgba[6] = 0x01; rgba[7] = 0x7F
        val out = eagerPack(rgba)
        assertEquals(0xFF123456.toInt(), out[0])
        assertEquals(0xFFFF8001.toInt(), out[1])
    }

    /** v3.7.0 (#646 review): a screenshot's Bitmap update on the main thread and the
     *  loop's publish on `Dispatchers.Default` must not interleave. [CpuFrame.materializeInto]
     *  hands [CpuFrame.pixels] to the copy while holding the monitor, so a copy taken
     *  while another thread publishes uniform black and white frames is never torn. */
    @Test
    fun a_copy_through_materialize_into_is_never_torn_by_a_concurrent_publish() {
        val cpu = CpuFrame(n)
        val black = ByteArray(n * 4) { if (it % 4 == 3) -1 else 0 }
        val white = ByteArray(n * 4) { -1 }
        val stop = java.util.concurrent.atomic.AtomicBoolean(false)
        val loop = Thread {
            var k = 0
            while (!stop.get()) cpu.publish(if (k++ % 2 == 0) black else white, eager = true)
        }
        loop.start()
        val copy = IntArray(n)
        var torn = 0
        try {
            repeat(3000) {
                cpu.materializeInto(always = true) { System.arraycopy(it, 0, copy, 0, n) }
                if (copy.any { px -> px != copy[0] }) torn++
            }
        } finally {
            stop.set(true)
            loop.join()
        }
        assertEquals("copies torn by a concurrent publish", 0, torn)
        // And without `always`, nothing pending means nothing to copy.
        var ran = false
        assertFalse(cpu.materializeInto(always = false) { ran = true })
        assertFalse(ran)
        cpu.publish(white, eager = false)
        assertTrue(cpu.materializeInto(always = false) { ran = true })
        assertTrue(ran)
    }
}
