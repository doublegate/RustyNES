package com.doublegate.rustynes

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.MediaMuxer
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import java.io.OutputStream

/**
 * v1.8.8 "Atlas" (Workstream F): screenshot + gameplay-clip capture and share.
 *
 * The capture surface is the **gameplay framebuffer** the emulation loop already
 * packs each frame — a 256x240 (or the upscaled HD-pack size) ARGB [Bitmap] with
 * NO UI chrome, system bars, or ROM content beyond the live picture the user is
 * playing. Capture taps a *copy* of that bitmap off the loop's critical path, so
 * the emulation timing / determinism contract is untouched.
 *
 * - **Screenshot** -> a PNG in shared storage `Pictures/RustyNES` via [MediaStore]
 *   (scoped-storage-correct on API 29+; gated to API 29+ — the app's minSdk is 26,
 *   but the legacy `WRITE_EXTERNAL_STORAGE` pre-29 path is intentionally not taken,
 *   keeping the no-broad-permission posture). Returns the inserted content [Uri].
 * - **Clip** -> an H.264 MP4 in shared storage `Movies/RustyNES`, encoded from a
 *   rolling ring buffer of the last N seconds of gameplay frames with
 *   [MediaCodec] + [MediaMuxer] (gameplay-only, video-only for this first version;
 *   muxing the APU PCM is a TODO below). Encoding runs entirely off the emulation
 *   loop (the ring buffer holds cheap bitmap copies; encode happens on Stop).
 *
 * Both then offer an `ACTION_SEND` share-sheet. Presentation-only — the core and
 * the `.rns`/AccuracyCoin contracts are untouched.
 */
object Capture {

    /** Shared-storage album sub-directory for both screenshots and clips. */
    private const val ALBUM = "RustyNES"

    /** Whether shared-storage capture is available (scoped MediaStore needs API 29+). */
    val supported: Boolean
        get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q

    // ---- Screenshot -------------------------------------------------------

    /**
     * Save [bitmap] (a gameplay-frame copy) as a PNG in `Pictures/RustyNES`.
     * Returns the inserted content [Uri], or null on failure / unsupported API.
     * Best-effort + self-contained: callers run it off the main thread.
     */
    fun saveScreenshot(context: Context, bitmap: Bitmap): Uri? {
        if (!supported) return null
        val name = "RustyNES_${timestamp()}.png"
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, name)
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(
                MediaStore.Images.Media.RELATIVE_PATH,
                "${Environment.DIRECTORY_PICTURES}/$ALBUM",
            )
            put(MediaStore.Images.Media.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
            ?: return null
        return runCatching {
            resolver.openOutputStream(uri)?.use { out: OutputStream ->
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
            } ?: throw java.io.IOException("can't open screenshot output stream")
            values.clear()
            values.put(MediaStore.Images.Media.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            uri
        }.getOrElse {
            // Roll back the half-written pending entry on failure.
            runCatching { resolver.delete(uri, null, null) }
            null
        }
    }

    // ---- Clip recording (MediaCodec MP4) ----------------------------------

    /** ~30 fps so the encoder keeps up comfortably while gameplay runs at ~60 Hz. */
    private const val CLIP_FPS = 30

    /**
     * A bounded ring buffer of recent gameplay frames. The emulation loop pushes a
     * cheap [Bitmap] copy each presented frame (capped to [CLIP_FPS] cadence so the
     * buffer covers ~[seconds] of play in [capacity] frames). On Stop the frames are
     * encoded to an MP4 — so the loop never touches the codec on its hot path.
     *
     * Thread model: the loop appends from the UI thread (where it publishes frames);
     * [drain] is called once, on Stop, from a background coroutine. The list is only
     * mutated under `this` so a concurrent drain can't tear.
     */
    class ClipBuffer(val width: Int, val height: Int, seconds: Int = 30) {
        val capacity: Int = CLIP_FPS * seconds
        private val frames = ArrayDeque<Bitmap>(capacity)
        // A small pool of reusable, pre-sized bitmaps reclaimed when a frame is evicted
        // from the (full) ring. At steady state the ring is full, so every new frame
        // pulls a recycled buffer from the pool and overwrites its pixels in place —
        // ZERO per-frame allocation (vs. the old src.copy() that allocated ~900 bitmaps /
        // ~216 MB over a 30 s clip). Only the initial fill (capacity frames) allocates.
        private val pool = ArrayDeque<Bitmap>(2)
        // Scratch pixel row used to copy frame contents without allocating a Bitmap.
        private val scratch = IntArray(width * height)
        // Throttle to CLIP_FPS: skip frames so a 60 Hz loop records every other one.
        private var accum = 0
        private val step = 2 // ~60 Hz loop -> ~30 fps clip.

        /** Offer a presented gameplay frame; snapshots it into a (pooled) bitmap on the
         *  throttle beat without allocating once the ring is full. */
        @Synchronized
        fun offer(src: Bitmap) {
            accum += 1
            if (accum % step != 0) return
            // Reclaim the oldest frame's bitmap into the pool when the ring is full, so
            // we reuse its buffer instead of allocating a new one.
            if (frames.size >= capacity) {
                val evicted = frames.removeFirst()
                pool.addLast(evicted)
            }
            // Pull a reusable bitmap from the pool, or allocate one only if the pool is
            // empty (i.e. during the initial fill). The dest is always width x height.
            val dest = pool.removeFirstOrNull()?.takeIf { !it.isRecycled }
                ?: Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            // Copy the source pixels into the reused buffer (handles the in-place loop
            // bitmap correctly: we snapshot now). If the source size differs (shouldn't
            // for a fixed framebuffer), fall back to a fresh copy so output isn't wrong.
            if (src.width == width && src.height == height) {
                src.getPixels(scratch, 0, width, 0, 0, width, height)
                dest.setPixels(scratch, 0, width, 0, 0, width, height)
                frames.addLast(dest)
            } else {
                // Size mismatch: don't reuse the pooled buffer; recycle it back and take
                // a true copy so the recorded frame stays correct.
                if (dest.width == width && dest.height == height) pool.addLast(dest)
                val copy = src.copy(Bitmap.Config.ARGB_8888, false) ?: return
                frames.addLast(copy)
            }
        }

        /** Snapshot + clear the retained frames (caller owns + recycles them). */
        @Synchronized
        fun drain(): List<Bitmap> {
            val out = frames.toList()
            frames.clear()
            // The pooled spares aren't handed out; recycle them here.
            pool.forEach { it.recycle() }
            pool.clear()
            return out
        }

        @Synchronized
        fun clear() {
            frames.forEach { it.recycle() }
            frames.clear()
            pool.forEach { it.recycle() }
            pool.clear()
        }
    }

    /**
     * Encode [frames] (gameplay bitmaps, all [width]x[height]) into an H.264 MP4 in
     * `Movies/RustyNES` and return the content [Uri]. Video-only for this first
     * version. Runs off the emulation loop (call from a background coroutine).
     *
     * TODO(v1.8.x WS F follow-up): mux the APU PCM. The bridge exposes the f32
     * stream via `NesController.drainAudioBytes`; a parallel audio ring captured in
     * lockstep with the video ring, AAC-encoded through a second MediaCodec track,
     * would give a sound clip. Deferred to keep this increment video-only + simple.
     */
    fun encodeClip(context: Context, frames: List<Bitmap>, width: Int, height: Int): Uri? {
        if (!supported || frames.isEmpty()) {
            frames.forEach { runCatching { it.recycle() } }
            return null
        }
        // MediaCodec wants even dimensions for H.264; the NES 256x240 already is.
        val w = width and 1.inv()
        val h = height and 1.inv()
        val name = "RustyNES_${timestamp()}.mp4"
        val values = ContentValues().apply {
            put(MediaStore.Video.Media.DISPLAY_NAME, name)
            put(MediaStore.Video.Media.MIME_TYPE, "video/mp4")
            put(
                MediaStore.Video.Media.RELATIVE_PATH,
                "${Environment.DIRECTORY_MOVIES}/$ALBUM",
            )
            put(MediaStore.Video.Media.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val uri = resolver.insert(MediaStore.Video.Media.EXTERNAL_CONTENT_URI, values)
            ?: run {
                frames.forEach { runCatching { it.recycle() } }
                return null
            }
        return runCatching {
            resolver.openFileDescriptor(uri, "rw")!!.use { pfd ->
                encodeH264(pfd.fileDescriptor.let { it }, frames, w, h, pfd)
            }
            values.clear()
            values.put(MediaStore.Video.Media.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
            uri
        }.getOrElse {
            runCatching { resolver.delete(uri, null, null) }
            null
        }.also {
            frames.forEach { f -> runCatching { f.recycle() } }
        }
    }

    /**
     * Drive a synchronous (non-Surface) H.264 encode of [frames] into the MP4 muxer
     * backed by [pfd]. Uses the YUV420 (COLOR_FormatYUV420Flexible) input path: each
     * ARGB frame is converted to I420 and fed to the codec; output packets are
     * written to the [MediaMuxer]. Kept deliberately simple + dependency-free.
     */
    private fun encodeH264(
        @Suppress("UNUSED_PARAMETER") fd: java.io.FileDescriptor,
        frames: List<Bitmap>,
        w: Int,
        h: Int,
        pfd: android.os.ParcelFileDescriptor,
    ) {
        val mime = MediaFormat.MIMETYPE_VIDEO_AVC
        val format = MediaFormat.createVideoFormat(mime, w, h).apply {
            setInteger(
                MediaFormat.KEY_COLOR_FORMAT,
                MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible,
            )
            setInteger(MediaFormat.KEY_BIT_RATE, 4_000_000)
            setInteger(MediaFormat.KEY_FRAME_RATE, CLIP_FPS)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
        }
        val codec = MediaCodec.createEncoderByType(mime)
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        codec.start()

        val muxer = MediaMuxer(pfd.fileDescriptor, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
        var trackIndex = -1
        var muxerStarted = false
        val bufferInfo = MediaCodec.BufferInfo()
        val frameDurationUs = 1_000_000L / CLIP_FPS
        val yuv = ByteArray(w * h * 3 / 2)
        val argb = IntArray(w * h)

        fun drainOutput(endOfStream: Boolean) {
            while (true) {
                val outIndex = codec.dequeueOutputBuffer(bufferInfo, if (endOfStream) 10_000 else 0)
                if (outIndex == MediaCodec.INFO_TRY_AGAIN_LATER) {
                    if (!endOfStream) break
                } else if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    trackIndex = muxer.addTrack(codec.outputFormat)
                    muxer.start()
                    muxerStarted = true
                } else if (outIndex >= 0) {
                    val encoded = codec.getOutputBuffer(outIndex)
                    if (encoded != null && bufferInfo.size > 0 && muxerStarted &&
                        (bufferInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG) == 0
                    ) {
                        encoded.position(bufferInfo.offset)
                        encoded.limit(bufferInfo.offset + bufferInfo.size)
                        muxer.writeSampleData(trackIndex, encoded, bufferInfo)
                    }
                    codec.releaseOutputBuffer(outIndex, false)
                    if ((bufferInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM) != 0) break
                }
            }
        }

        try {
            for ((i, frame) in frames.withIndex()) {
                // Scale into the encode size if needed (HD-pack frames are larger),
                // then read ARGB -> I420.
                val scaled = if (frame.width == w && frame.height == h) {
                    frame
                } else {
                    Bitmap.createScaledBitmap(frame, w, h, false)
                }
                scaled.getPixels(argb, 0, w, 0, 0, w, h)
                if (scaled !== frame) scaled.recycle()
                argbToI420(argb, yuv, w, h)

                var inIndex = codec.dequeueInputBuffer(10_000)
                while (inIndex < 0) {
                    drainOutput(false)
                    inIndex = codec.dequeueInputBuffer(10_000)
                }
                val inBuf = codec.getInputBuffer(inIndex)!!
                inBuf.clear()
                inBuf.put(yuv)
                codec.queueInputBuffer(inIndex, 0, yuv.size, i * frameDurationUs, 0)
                drainOutput(false)
            }
            // Signal end-of-stream and flush the encoder.
            val inIndex = run {
                var idx = codec.dequeueInputBuffer(10_000)
                while (idx < 0) { drainOutput(false); idx = codec.dequeueInputBuffer(10_000) }
                idx
            }
            codec.queueInputBuffer(
                inIndex, 0, 0, frames.size * frameDurationUs,
                MediaCodec.BUFFER_FLAG_END_OF_STREAM,
            )
            drainOutput(true)
        } finally {
            runCatching { codec.stop() }
            runCatching { codec.release() }
            if (muxerStarted) runCatching { muxer.stop() }
            runCatching { muxer.release() }
        }
    }

    /** ARGB_8888 (already 0xFFRRGGBB) -> planar I420 (YUV420). BT.601 full-range. */
    private fun argbToI420(argb: IntArray, out: ByteArray, w: Int, h: Int) {
        val frameSize = w * h
        var yIndex = 0
        var uvIndex = frameSize
        var i = 0
        for (y in 0 until h) {
            for (x in 0 until w) {
                val c = argb[i]
                val r = (c shr 16) and 0xFF
                val g = (c shr 8) and 0xFF
                val b = c and 0xFF
                val yy = ((66 * r + 129 * g + 25 * b + 128) shr 8) + 16
                out[yIndex++] = yy.coerceIn(0, 255).toByte()
                // 4:2:0 subsample: one chroma pair per 2x2 block.
                if (y % 2 == 0 && x % 2 == 0) {
                    val u = ((-38 * r - 74 * g + 112 * b + 128) shr 8) + 128
                    val v = ((112 * r - 94 * g - 18 * b + 128) shr 8) + 128
                    out[uvIndex++] = u.coerceIn(0, 255).toByte()
                    out[uvIndex++] = v.coerceIn(0, 255).toByte()
                }
                i++
            }
        }
    }

    // ---- Share ------------------------------------------------------------

    /** Fire the system share-sheet for a saved screenshot ([image]=true) or clip. */
    fun share(context: Context, uri: Uri, image: Boolean) {
        val send = Intent(Intent.ACTION_SEND).apply {
            type = if (image) "image/png" else "video/mp4"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val title = if (image) "Share screenshot" else "Share clip"
        context.startActivity(
            Intent.createChooser(send, title).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    private fun timestamp(): String =
        java.text.SimpleDateFormat("yyyyMMdd_HHmmss", java.util.Locale.US)
            .format(java.util.Date())
}
