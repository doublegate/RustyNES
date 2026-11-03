package com.doublegate.rustynes

import java.nio.ByteBuffer
import java.nio.ByteOrder

/*
 * v3.6.0 (`T-ANDROID-AUDIO-NONBLOCKING`, audit AND-R2-01): the pacing that the
 * blocking `AudioTrack` write used to provide implicitly, written down.
 *
 * Until v3.6.0 the emulation loop wrote each frame's samples with
 * `WRITE_BLOCKING`, and the block WAS the pacing: when the track's ring was
 * full the write waited for the DAC, so the loop ran at the audio device's
 * clock. That is a sound design and the maintainer kept it in v3.2.0 for a good
 * reason: the replacement drafted then (`write(byte[], ..., WRITE_NON_BLOCKING)`)
 * used an overload meant for 8/16-bit PCM on a PCM_FLOAT track, dropped partial
 * writes, and removed the clock without putting anything in its place
 * (`c04da0ca`). It also has two costs worth removing, which is why the ticket
 * stayed open:
 *
 * - the block happens on a `Dispatchers.Default` worker, a pool sized to the CPU
 *   count and shared with every other coroutine in the process;
 * - it cannot be cancelled: a block in progress outlives the coroutine's
 *   cancellation, and on Android 17 a silenced track (background audio
 *   hardening, see [audioMayPlay]) is documented to fail "silently", which does
 *   not say whether a full ring still blocks.
 *
 * The non-blocking path keeps the audio clock and makes it explicit. Each
 * iteration hands the track what it will take ([AudioQueue], keeping what it
 * will not), then the loop sleeps -- a cancellable coroutine `delay`, off the
 * worker -- for as long as the track holds more than [AudioPacer.targetFrames].
 * That is exactly the blocking write's equilibrium, with the waiting moved from
 * inside `write` to a measured `delay`, and with a floor and a ceiling the
 * blocking write never had:
 *
 * - below the target the loop does not sleep at all, so a ring that is running
 *   dry is refilled at once (the blocking write would also have returned at
 *   once; a sleep there is an underrun);
 * - above it the sleep is capped at [AudioPacer.maxDelayNanos], and a play head
 *   that has not moved for [AudioPacer.stallNanos] with audio queued drops the
 *   loop back to the wall-clock floor instead of trusting a clock that is not
 *   running. The stall is measured in time, not in calls: a HAL advances the
 *   head once per mixer period (a few to tens of milliseconds), so a loop
 *   refilling a low buffer can call several times between two updates, and a
 *   call count read that as a stall and slept a whole frame while the buffer
 *   ran dry (caught in review of the first draft, before it was committed).
 *
 * Everything here is plain JVM code (no `android.*`), so `AudioPacingTest` runs
 * it on the host; the `AudioTrack` glue in `MainActivity.kt`'s `AudioPlayer`
 * only feeds it numbers.
 *
 * NOT verified on a device. The JVM tests pin the arithmetic; the drift
 * behaviour against a real DAC (the reason the maintainer wanted a device
 * before this changed) is a row in `docs/mobile-v2.9.3-run-sheet.md`.
 */

/** Bytes per mono `PCM_FLOAT` frame. */
const val FLOAT_MONO_FRAME_BYTES = 4

/**
 * Explicit pacing for the non-blocking write.
 *
 * @param sampleRate the track's rate, frames per second.
 * @param capacityFrames the track's buffer, in frames (`AudioTrack.bufferSizeInFrames`).
 * @param frameNanos one emulated frame's wall-clock period: the floor when no
 *   audio drives the loop (silence, mute, lost focus, a stalled head).
 */
class AudioPacer(
    private val sampleRate: Int,
    capacityFrames: Int,
    private val frameNanos: Long,
) {
    init {
        require(sampleRate > 0) { "sampleRate $sampleRate" }
        require(capacityFrames > 0) { "capacityFrames $capacityFrames" }
        require(frameNanos > 0) { "frameNanos $frameNanos" }
    }

    /** One emulated frame of audio, in frames, rounded up. */
    val framesPerFrame: Int = ((sampleRate.toLong() * frameNanos + 999_999_999L) / 1_000_000_000L).toInt()

    /**
     * The fill the loop holds the track at. Two emulated frames of audio when
     * the buffer has room for them and one more (so a frame's write still fits
     * on top of the target), otherwise one frame less than the buffer, and
     * never under one frame: below that a single late wakeup is an underrun.
     */
    val targetFrames: Int = minOf(2 * framesPerFrame, capacityFrames - framesPerFrame)
        .coerceAtLeast(minOf(framesPerFrame, capacityFrames))

    /** The longest one sleep may be: two emulated frames. */
    val maxDelayNanos: Long = 2 * frameNanos

    /** How long an unmoving head with audio queued must last to count as
     *  stopped: six emulated frames (about 100 ms), several mixer periods. */
    val stallNanos: Long = STALL_FRAMES * frameNanos

    private var lastHead: Long = -1
    private var lastHeadMoveNanos = 0L

    /**
     * Frames written to the track and not yet played. [writtenFrames] counts
     * every frame the track accepted since it was created; [headPosition] is
     * `AudioTrack.playbackHeadPosition`, which the platform documents as an
     * unsigned 32-bit count that wraps (about 24.8 hours at 48 kHz), so both
     * are compared modulo 2^32.
     */
    fun queuedFrames(writtenFrames: Long, headPosition: Int): Long =
        (writtenFrames - (headPosition.toLong() and U32)) and U32

    /**
     * How long the loop sleeps before the next frame.
     *
     * @param queued [queuedFrames] after this frame's write.
     * @param headPosition the play head read with it (for stall detection).
     * @param nowNanos a monotonic clock (`System.nanoTime()`), for the stall.
     * @param elapsedNanos time this iteration has already spent.
     * @param audioDriven whether this frame wrote audio. When it did not, the
     *   loop paces on the wall clock alone, as it always has for silent games.
     */
    fun delayNanos(
        queued: Long,
        headPosition: Int,
        nowNanos: Long,
        elapsedNanos: Long,
        audioDriven: Boolean,
    ): Long {
        val head = headPosition.toLong() and U32
        // The head moved (or nothing waits, or this is the first call): the
        // clock is running as far as anyone can tell.
        if (head != lastHead || queued <= 0 || lastHead < 0) {
            lastHead = head
            lastHeadMoveNanos = nowNanos
        }
        if (!audioDriven) return wallClock(elapsedNanos)
        val excess = queued - targetFrames
        // Below the target: refill now, whatever the head is doing.
        if (excess <= 0) return 0
        // A head that has not moved for `stallNanos` while audio waits is not a
        // clock (a paused, silenced or dead track): the wall clock until it moves.
        if (nowNanos - lastHeadMoveNanos >= stallNanos) return wallClock(elapsedNanos)
        return (excess * 1_000_000_000L / sampleRate).coerceAtMost(maxDelayNanos)
    }

    private fun wallClock(elapsedNanos: Long): Long = (frameNanos - elapsedNanos).coerceAtLeast(0)

    companion object {
        /** Emulated frames an unmoving head must last to count as a stall. */
        const val STALL_FRAMES = 6
        private const val U32 = 0xFFFF_FFFFL
    }
}

/**
 * The bytes the track did not take yet, in order.
 *
 * A non-blocking write may accept part of a frame, or nothing. The v3.2.0 draft
 * dropped the rest, which is a click every time the ring filled; this keeps it
 * and offers it first next time. It is bounded by [capacityBytes]: if the track
 * stops taking audio altogether, the OLDEST bytes go, so what eventually plays is
 * the newest sound rather than a growing backlog of stale sound. With
 * [AudioPacer] holding the track below its capacity this only happens when the
 * track has stopped (counted in [droppedBytes]).
 *
 * @param capacityBytes the most it holds; rounded down to a whole frame.
 */
class AudioQueue(capacityBytes: Int) {
    private val pending: ByteBuffer =
        ByteBuffer.allocate((capacityBytes / FLOAT_MONO_FRAME_BYTES).coerceAtLeast(1) * FLOAT_MONO_FRAME_BYTES)
            .order(ByteOrder.LITTLE_ENDIAN)
            .also { it.flip() } // empty, in read mode

    /** Bytes waiting for the track. */
    val pendingBytes: Int get() = pending.remaining()

    /** Bytes discarded because [capacityBytes] was exceeded. */
    var droppedBytes: Long = 0
        private set

    /** Errors the sink returned (a negative count). */
    var sinkErrors: Int = 0
        private set

    /**
     * Offer one frame's [bytes] (little-endian f32). [sink] is the
     * non-blocking write: it is given a buffer positioned at what to write and
     * returns how many bytes it took (0 when full, negative on error). Returns
     * the bytes the sink took in total this call (always a whole number of
     * frames, since the sink is given whole frames and a partial frame stays
     * pending).
     */
    fun offer(bytes: ByteArray, sink: (ByteBuffer) -> Int): Int {
        var taken = 0
        // What was left last time goes first, in order.
        if (pending.hasRemaining()) {
            val n = drain(pending, sink)
            if (n < 0) return taken
            taken += n
        }
        val usable = bytes.size - bytes.size % FLOAT_MONO_FRAME_BYTES
        if (usable == 0) return taken
        val fresh = ByteBuffer.wrap(bytes, 0, usable).order(ByteOrder.LITTLE_ENDIAN)
        if (!pending.hasRemaining()) {
            val n = drain(fresh, sink)
            if (n < 0) return taken
            taken += n
        }
        if (fresh.hasRemaining()) keep(fresh)
        return taken
    }

    /** Forget everything pending (a pause, a flush, a release). */
    fun clear() {
        pending.clear()
        pending.flip()
    }

    /**
     * Hand [buf] to [sink] once; advance [buf] by what it took (whatever the sink
     * did to the position). A take that is not a whole frame keeps the partial
     * frame's remainder, which the next call delivers first.
     */
    private fun drain(buf: ByteBuffer, sink: (ByteBuffer) -> Int): Int {
        val start = buf.position()
        val n = sink(buf)
        if (n < 0) {
            sinkErrors++
            buf.position(start)
            return n
        }
        val took = minOf(n, buf.limit() - start)
        buf.position(start + took)
        return took
    }

    /** Append [fresh]'s remainder to the pending bytes, dropping the oldest on overflow. */
    private fun keep(fresh: ByteBuffer) {
        val cap = pending.capacity()
        val incoming = fresh.remaining()
        if (incoming >= cap) {
            // Only the newest `cap` bytes of this frame survive; all older audio goes.
            droppedBytes += pending.remaining() + (incoming - cap)
            fresh.position(fresh.limit() - cap)
            pending.clear()
            pending.put(fresh)
            pending.flip()
            return
        }
        val overflow = pending.remaining() + incoming - cap
        if (overflow > 0) {
            // Drop whole frames from the front so the stream stays frame-aligned.
            val drop = minOf(
                ((overflow + FLOAT_MONO_FRAME_BYTES - 1) / FLOAT_MONO_FRAME_BYTES) * FLOAT_MONO_FRAME_BYTES,
                pending.remaining(),
            )
            pending.position(pending.position() + drop)
            droppedBytes += drop
        }
        pending.compact() // write mode, the kept bytes moved to the front
        pending.put(fresh)
        pending.flip()
    }
}

/**
 * v3.6.0 (`T-ANDROID-API37`): may the app touch audio now?
 *
 * Android 17 silences playback and volume calls, and fails focus requests, for
 * an app with no visible activity and no qualifying foreground service --
 * "silently", without an exception (developer.android.com, "Background audio
 * hardening"). RustyNES has no foreground service and wants none: it plays only
 * while it is on screen, picture-in-picture included (a PiP activity is visible
 * and started). v2.7.4 (AND-03) already paused emulation in `onStop`; this
 * makes the rule the audio side's own, so no path that unpauses while the
 * activity is stopped can start a track or request focus there: a focus
 * request refused in the background left `focusMuted` set, and the game silent,
 * after it came back.
 *
 * @param visible the activity is started (between `onStart` and `onStop`).
 * @param running a game is loaded and not paused.
 */
fun audioMayPlay(visible: Boolean, running: Boolean): Boolean = visible && running
