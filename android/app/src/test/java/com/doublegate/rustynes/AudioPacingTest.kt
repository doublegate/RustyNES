package com.doublegate.rustynes

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer

/**
 * v3.6.0 (`T-ANDROID-AUDIO-NONBLOCKING`): the explicit pacing that replaced the
 * blocking `AudioTrack` write, on the JVM. A simulated DAC stands in for the
 * device: these tests pin the arithmetic and the equilibrium, not a real
 * device's drift (that is a run-sheet row).
 */
class AudioPacingTest {
    private val rate = 48_000
    private val frameNanos = 16_639_267L // NTSC, as MainActivity's FRAME_NANOS

    private fun pacer(capacity: Int = 3_200) = AudioPacer(rate, capacity, frameNanos)

    @Test
    fun the_target_is_two_frames_when_the_buffer_allows_it() {
        val p = pacer(3_200) // ~4 NES frames, the app's buffer at 48 kHz
        assertEquals(799, p.framesPerFrame)
        assertEquals(2 * 799, p.targetFrames)
    }

    @Test
    fun a_small_buffer_keeps_room_for_one_frame_and_at_least_one_frame() {
        assertEquals(799, pacer(1_200).targetFrames) // floor: one frame (401 would be the room rule)
        assertEquals(500, pacer(500).targetFrames) // never above the buffer
        assertEquals(2_000 - 799, pacer(2_000).targetFrames)
    }

    @Test
    fun below_the_target_the_loop_does_not_sleep() {
        val p = pacer()
        assertEquals(0, p.delayNanos(queued = 0, headPosition = 10, nowNanos = 0, elapsedNanos = 0, audioDriven = true))
        assertEquals(
            0,
            p.delayNanos(p.targetFrames.toLong(), headPosition = 20, nowNanos = 1, elapsedNanos = 0, audioDriven = true),
        )
    }

    @Test
    fun above_the_target_it_sleeps_for_the_excess() {
        val p = pacer()
        // 480 frames over the target at 48 kHz = 10 ms.
        val d = p.delayNanos(p.targetFrames + 480L, headPosition = 1, nowNanos = 0, elapsedNanos = 0, audioDriven = true)
        assertEquals(10_000_000L, d)
    }

    @Test
    fun one_sleep_is_capped_at_two_frames() {
        val p = pacer(1_000_000)
        val d = p.delayNanos(queued = 900_000, headPosition = 1, nowNanos = 0, elapsedNanos = 0, audioDriven = true)
        assertEquals(p.maxDelayNanos, d)
        assertEquals(2 * frameNanos, p.maxDelayNanos)
    }

    @Test
    fun without_audio_the_wall_clock_paces_as_before() {
        val p = pacer()
        assertEquals(frameNanos - 4_000_000, p.delayNanos(5_000, 1, 0, elapsedNanos = 4_000_000, audioDriven = false))
        assertEquals(0, p.delayNanos(0, 1, 0, elapsedNanos = frameNanos + 1, audioDriven = false))
    }

    /** A head that has not moved for [AudioPacer.stallNanos] with audio
     *  queued is not a clock: the wall clock paces until it moves again. */
    @Test
    fun a_stalled_head_falls_back_to_the_wall_clock() {
        val p = pacer()
        val full = p.targetFrames + 1_000L
        val excess = 1_000L * 1_000_000_000L / rate
        var now = 0L
        assertEquals(excess, p.delayNanos(full, headPosition = 7, nowNanos = now, elapsedNanos = 0, audioDriven = true))
        // The same head for just under the stall window: still trusted.
        now += p.stallNanos - 1
        assertEquals(excess, p.delayNanos(full, headPosition = 7, nowNanos = now, elapsedNanos = 0, audioDriven = true))
        // At the window: not a clock.
        now += 1
        assertEquals(
            frameNanos - 1_000,
            p.delayNanos(full, headPosition = 7, nowNanos = now, elapsedNanos = 1_000, audioDriven = true),
        )
        // It moves again: audio pacing resumes.
        now += 1
        assertEquals(excess, p.delayNanos(full, headPosition = 8, nowNanos = now, elapsedNanos = 0, audioDriven = true))
    }

    /**
     * A HAL moves the head once per mixer period, so a loop refilling a low
     * buffer calls several times between two updates. Below the target that
     * must never read as a stall: the first draft counted calls, slept a whole
     * frame on the third, and let the buffer run dry.
     */
    @Test
    fun a_bursty_head_below_the_target_never_sleeps() {
        val p = pacer()
        var now = 0L
        // 20 calls 10 ms apart, the head unchanged throughout: 200 ms, past the
        // stall window, and below the target every refill still goes at once.
        repeat(20) {
            assertEquals(0, p.delayNanos(p.targetFrames - 10L, headPosition = 960, nowNanos = now, elapsedNanos = 0, audioDriven = true))
            now += 10_000_000
        }
        assertTrue(now > p.stallNanos)
        // Then above the target with the head still unchanged since the first
        // call: only now is it a stall.
        assertEquals(frameNanos, p.delayNanos(p.targetFrames + 10L, 960, now, elapsedNanos = 0, audioDriven = true))
    }

    @Test
    fun queued_frames_wrap_at_32_bits() {
        val p = pacer()
        assertEquals(100, p.queuedFrames(writtenFrames = 1_100, headPosition = 1_000))
        // The head is an unsigned 32-bit count the platform hands over as an Int.
        val written = 0x1_0000_0010L // 16 frames past the wrap
        assertEquals(32, p.queuedFrames(written, headPosition = -16)) // 0xFFFF_FFF0
    }

    /**
     * The equilibrium against a simulated DAC: a loop that emulates one frame
     * (799 or 800 samples, 60.0988 Hz), writes non-blocking, and sleeps what the
     * pacer says, while the DAC drains at exactly 48 kHz. The fill must settle
     * near the target and never underrun or overflow, and the loop must run at
     * the DAC's rate (samples produced == samples played, within the buffer).
     *
     * The sleep is modelled as the loop takes it: `delay(ms)`, truncated to whole
     * milliseconds, and then overslept by up to 2 ms of scheduler jitter (a fixed
     * seed, so the run is reproducible). The first two seconds are warm-up: the
     * track starts empty, so its first frames necessarily run dry.
     */
    @Test
    fun the_loop_settles_at_the_target_against_a_simulated_dac() {
        val capacity = 3_200
        val p = pacer(capacity)
        val q = AudioQueue(capacity * FLOAT_MONO_FRAME_BYTES)
        var nowNs = 0L
        var played = 0L // frames the DAC consumed
        var written = 0L // frames the track accepted
        var produced = 0L
        var underruns = 0
        var minFill = Long.MAX_VALUE
        var maxFill = 0L
        val samplesPerSecond = rate.toDouble()
        val nesHz = 1_000_000_000.0 / frameNanos
        var acc = 0.0
        var warm = false
        val jitter = java.util.Random(0x5EED)
        // The head the app sees moves once per 20 ms mixer period (960 frames),
        // as a HAL reports it; the DAC itself drains continuously.
        val period = 960L
        fun visibleHead() = (played / period) * period
        fun advanceTo(t: Long) {
            val target = (t * samplesPerSecond / 1e9).toLong()
            val want = target - played
            val avail = written - played
            if (want > avail && warm) underruns++
            played += minOf(want, avail)
            nowNs = t
        }
        repeat(60 * 30) { i ->
            val start = nowNs
            acc += samplesPerSecond / nesHz
            val n = acc.toInt()
            acc -= n
            produced += n
            val bytes = ByteArray(n * FLOAT_MONO_FRAME_BYTES)
            val emulate = 3_000_000L // 3 ms of emulation per frame
            advanceTo(nowNs + emulate)
            written += q.offer(bytes) { b ->
                val room = ((capacity - (written - played)) * FLOAT_MONO_FRAME_BYTES).toInt()
                val take = minOf(room, b.remaining()).coerceAtLeast(0)
                b.position(b.position() + take)
                take
            } / FLOAT_MONO_FRAME_BYTES
            val queued = p.queuedFrames(written, visibleHead().toInt())
            warm = i >= 120
            if (warm) {
                minFill = minOf(minFill, queued)
                maxFill = maxOf(maxFill, queued)
            }
            val d = p.delayNanos(queued, visibleHead().toInt(), nowNs, nowNs - start, audioDriven = true)
            val ms = d / 1_000_000
            val slept = if (ms > 0) ms * 1_000_000 + jitter.nextInt(2_000_001) else 0L
            advanceTo(nowNs + slept)
        }
        assertEquals("underruns after warm-up", 0, underruns)
        assertTrue("fill dropped to $minFill", minFill >= p.framesPerFrame / 2)
        assertTrue("fill reached $maxFill of $capacity", maxFill <= capacity)
        assertEquals("nothing dropped", 0, q.droppedBytes)
        // Produced and played agree to within what the track and queue hold.
        assertTrue(produced - played <= capacity + q.pendingBytes / FLOAT_MONO_FRAME_BYTES)
    }

    // --- AudioQueue -------------------------------------------------------

    private fun frames(vararg v: Int): ByteArray {
        val b = ByteBuffer.allocate(v.size * 4).order(java.nio.ByteOrder.LITTLE_ENDIAN)
        v.forEach { b.putFloat(it.toFloat()) }
        return b.array()
    }

    /** A sink that takes at most [limit] bytes per call and records them. */
    private class Sink(var limit: Int) {
        val got = java.io.ByteArrayOutputStream()
        fun take(b: ByteBuffer): Int {
            val n = minOf(limit, b.remaining())
            val tmp = ByteArray(n)
            b.get(tmp)
            got.write(tmp)
            return n
        }
    }

    @Test
    fun a_partial_write_keeps_the_rest_and_sends_it_first() {
        val q = AudioQueue(64)
        val s = Sink(8)
        assertEquals(8, q.offer(frames(1, 2, 3, 4), s::take))
        assertEquals(8, q.pendingBytes)
        s.limit = 1_000
        assertEquals(8 + 8, q.offer(frames(5, 6), s::take))
        assertEquals(0, q.pendingBytes)
        assertArrayEquals(frames(1, 2, 3, 4, 5, 6), s.got.toByteArray())
    }

    /** Fresh audio waits behind what is pending, even when the track takes
     *  some of the pending bytes and then stops: it must not jump the queue. */
    @Test
    fun fresh_audio_never_overtakes_pending_audio() {
        val q = AudioQueue(64)
        val s = Sink(0)
        q.offer(frames(1, 2), s::take) // both pending
        s.limit = 4 // the track takes one frame per call
        assertEquals(4, q.offer(frames(3), s::take)) // frame 1 only; 2 and 3 wait
        assertEquals(8, q.pendingBytes)
        s.limit = 1_000
        q.offer(ByteArray(0), s::take)
        assertArrayEquals(frames(1, 2, 3), s.got.toByteArray())
    }

    @Test
    fun a_full_track_takes_nothing_and_loses_nothing_within_capacity() {
        val q = AudioQueue(64)
        val s = Sink(0)
        assertEquals(0, q.offer(frames(1, 2), s::take))
        assertEquals(0, q.offer(frames(3), s::take))
        assertEquals(12, q.pendingBytes)
        s.limit = 1_000
        q.offer(ByteArray(0), s::take)
        assertArrayEquals(frames(1, 2, 3), s.got.toByteArray())
        assertEquals(0, q.droppedBytes)
    }

    @Test
    fun overflow_drops_the_oldest_whole_frames() {
        val q = AudioQueue(12) // three frames
        val s = Sink(0)
        q.offer(frames(1, 2), s::take)
        q.offer(frames(3, 4), s::take) // one frame too many: frame 1 goes
        assertEquals(12, q.pendingBytes)
        assertEquals(4, q.droppedBytes)
        s.limit = 1_000
        q.offer(ByteArray(0), s::take)
        assertArrayEquals(frames(2, 3, 4), s.got.toByteArray())
    }

    @Test
    fun a_chunk_larger_than_the_queue_keeps_its_newest_frames() {
        val q = AudioQueue(8)
        val s = Sink(0)
        q.offer(frames(1, 2, 3, 4), s::take)
        assertEquals(8, q.pendingBytes)
        assertEquals(8, q.droppedBytes)
        s.limit = 1_000
        q.offer(ByteArray(0), s::take)
        assertArrayEquals(frames(3, 4), s.got.toByteArray())
    }

    @Test
    fun a_sink_error_takes_nothing_and_is_counted() {
        val q = AudioQueue(64)
        assertEquals(0, q.offer(frames(1)) { -3 })
        assertEquals(1, q.sinkErrors)
    }

    @Test
    fun a_trailing_partial_sample_is_ignored() {
        val q = AudioQueue(64)
        val s = Sink(1_000)
        assertEquals(4, q.offer(frames(1) + byteArrayOf(9, 9), s::take))
        assertArrayEquals(frames(1), s.got.toByteArray())
    }

    @Test
    fun clear_forgets_pending() {
        val q = AudioQueue(64)
        q.offer(frames(1, 2)) { 0 }
        q.clear()
        assertEquals(0, q.pendingBytes)
    }

    // --- the MainActivity wiring (source checks; Compose and AudioTrack do not run on the JVM)

    private fun mainActivity(): String =
        listOf("src/main", "app/src/main", "android/app/src/main")
            .map { java.io.File(it, "java/com/doublegate/rustynes/MainActivity.kt") }
            .first { it.isFile }
            .readText()
            .replace("\r\n", "\n") // a CRLF checkout

    @Test
    fun the_track_is_written_non_blocking_through_the_queue_and_paced_by_the_pacer() {
        val src = mainActivity()
        assertFalse("a blocking write is back", src.contains("AudioTrack.WRITE_BLOCKING"))
        assertTrue(src.contains("track.write(buf, buf.remaining(), AudioTrack.WRITE_NON_BLOCKING)"))
        assertTrue(src.contains("val took = queue.offer(bytes)"))
        assertTrue(src.contains("val remainingMs = audio.paceDelayNanos(System.nanoTime() - start) / 1_000_000"))
    }

    @Test
    fun the_loop_plays_only_while_the_activity_is_started() {
        val src = mainActivity()
        assertTrue(src.contains("if (ctrl == null || !audioMayPlay(emulator.visible, running)) {"))
        val stop = src.substringAfter("override fun onStop() {").substringBefore("\n    }\n")
        assertTrue(stop, stop.contains("emulator.visible = false"))
        val start = src.substringAfter("override fun onStart() {").substringBefore("\n    }\n")
        assertTrue(start, start.contains("emulator.visible = true"))
    }

    // --- audioMayPlay -----------------------------------------------------

    @Test
    fun audio_plays_only_while_visible_and_running() {
        assertTrue(audioMayPlay(visible = true, running = true))
        assertFalse(audioMayPlay(visible = false, running = true))
        assertFalse(audioMayPlay(visible = true, running = false))
        assertFalse(audioMayPlay(visible = false, running = false))
    }
}
