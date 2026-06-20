package com.doublegate.rustynes

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.view.KeyEvent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.OutlinedButton
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import uniffi.rustynes_mobile.NesButton
import uniffi.rustynes_mobile.NesController

/**
 * RustyNES Android — first-boot Compose shell (v1.8.0 "Android", beta.1).
 *
 * This shell drives the byte-identical core entirely through the UniFFI-generated
 * [NesController] control surface: it loads a ROM from the Storage Access
 * Framework picker, runs the emulation on a background coroutine, blits each RGBA
 * frame to a [Bitmap], and routes on-screen + hardware-gamepad input into the
 * single late-latched controller mask. The wgpu/shader render path and the AAudio
 * sink land in beta.2/beta.3 via the `rustynes-android` JNI seam; nothing here
 * touches the determinism contract — input converges on one mask per port,
 * exactly as the desktop and wasm hosts do.
 */
class MainActivity : ComponentActivity() {

    /** Holds the live controller so hardware key events (dispatched to the
     *  Activity, not Compose) can reach the same instance the UI drives. */
    private val emulator = EmulatorHandle()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        hideSystemBars()
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = Color.Black) {
                    EmulatorScreen(emulator)
                }
            }
        }
    }

    // Re-assert immersive mode whenever the window regains focus: the system
    // shows the bars on focus loss (dialogs, the SAF picker, fold/unfold), so
    // without this they'd stay visible and overlay the on-screen controls.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    // Hide the status + navigation bars (and the large-screen taskbar) in sticky
    // immersive mode: they stay hidden during play and only reappear transiently
    // on an edge swipe, then auto-hide — so they never sit on top of the gameplay
    // buttons. Applied identically on the cover and inner displays.
    private fun hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            hide(WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    // Save-on-background: write the `auto` save-state for the current ROM so the
    // next open of it resumes where the player left off. The bridge's internal
    // lock serialises this against the emulation thread, so the snapshot is
    // consistent; it is a quick in-memory encode, fine to do on the main thread.
    override fun onPause() {
        super.onPause()
        val ctrl = emulator.controller
        val sha = emulator.romSha
        if (ctrl != null && sha != null) {
            runCatching { SaveStateStore.save(this, sha, SaveStateStore.AUTO_SLOT, ctrl.saveState()) }
        }
    }

    // Hardware gamepad / keyboard: Android dispatches KeyEvents to the Activity.
    // Map them onto P1 and feed the same controller the touch overlay drives.
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean =
        emulator.onKey(keyCode, true) || super.onKeyDown(keyCode, event)

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean =
        emulator.onKey(keyCode, false) || super.onKeyUp(keyCode, event)
}

/**
 * Thin holder around the optional [NesController] plus the hardware-key mapping.
 * Compose owns the per-frame [ImageBitmap]; this owns the controller lifetime so
 * the Activity's key handlers and the Compose UI share one instance.
 */
class EmulatorHandle {
    var controller: NesController? = null

    /** Lowercase hex SHA-256 of the loaded ROM — the save-state directory key. */
    var romSha: String? = null

    /** Emulation paused (the loop idles, no frames advance). Read by the loop. */
    @Volatile
    var paused: Boolean = false

    /** Fast-forward: drop the frame-pace delay + audio so the core runs ahead. */
    @Volatile
    var turbo: Boolean = false

    /** Mute the audio sink (the core still produces samples; they're discarded). */
    @Volatile
    var muted: Boolean = false

    /** Returns true if the key was consumed (a mapped NES button). */
    fun onKey(keyCode: Int, pressed: Boolean): Boolean {
        val button = keyToButton(keyCode) ?: return false
        controller?.setButton(0u, button, pressed)
        return true
    }

    private fun keyToButton(keyCode: Int): NesButton? = when (keyCode) {
        KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_DPAD_CENTER -> NesButton.A
        KeyEvent.KEYCODE_BUTTON_B -> NesButton.B
        KeyEvent.KEYCODE_BUTTON_START, KeyEvent.KEYCODE_ENTER -> NesButton.START
        KeyEvent.KEYCODE_BUTTON_SELECT, KeyEvent.KEYCODE_SPACE -> NesButton.SELECT
        KeyEvent.KEYCODE_DPAD_UP -> NesButton.UP
        KeyEvent.KEYCODE_DPAD_DOWN -> NesButton.DOWN
        KeyEvent.KEYCODE_DPAD_LEFT -> NesButton.LEFT
        KeyEvent.KEYCODE_DPAD_RIGHT -> NesButton.RIGHT
        else -> null
    }
}

/**
 * Load a ROM into [emulator] from raw bytes: build the controller, key it by
 * SHA-256, auto-resume the on-background save-state for that ROM if present, and
 * — when the bytes came from a SAF [uri] — take a persistable read grant + record
 * it in the recent-ROMs list so it survives reboot. Returns a status line. May
 * throw if the bytes are not a valid ROM (callers wrap in `runCatching`).
 */
private fun loadRom(
    context: Context,
    emulator: EmulatorHandle,
    bytes: ByteArray,
    uri: Uri?,
    name: String?,
): String {
    val ctrl = NesController(bytes, 48_000u)
    val sha = sha256Hex(bytes)
    emulator.controller = ctrl
    emulator.romSha = sha
    // Resume where the player left off (the on-background auto-state), if any.
    SaveStateStore.load(context, sha, SaveStateStore.AUTO_SLOT)?.let { blob ->
        runCatching { ctrl.loadState(blob) }
    }
    if (uri != null) {
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        RomLibrary.remember(context, uri.toString(), name ?: uri.lastPathSegment ?: "ROM")
    }
    return "Running" + (name?.let { " · $it" } ?: "")
}

/** Resolve a SAF document's human-readable display name for the recents list. */
private fun displayName(context: Context, uri: Uri): String {
    context.contentResolver.query(
        uri,
        arrayOf(OpenableColumns.DISPLAY_NAME),
        null,
        null,
        null,
    )?.use { c ->
        if (c.moveToFirst()) {
            val i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (i >= 0) return c.getString(i)
        }
    }
    return uri.lastPathSegment ?: "ROM"
}

private const val NES_WIDTH = 256
private const val NES_HEIGHT = 240

// NTSC frame period in nanoseconds (the wall-clock pacing floor for ROMs that
// emit little/no audio; sound-producing ROMs are paced by the blocking audio
// write). PAL/Dendy refinement is a later increment.
private const val FRAME_NANOS = 16_639_267L

/**
 * Low-latency mono audio sink fed by the core's [NesController.drainAudio].
 *
 * The core emits deterministic `f32` samples at the host rate; this is a pure
 * sink (the determinism contract is timing-only and lives outside the emitted
 * stream). A blocking `AudioTrack` write paces the emulation loop to real time
 * whenever sound is present — the audio clock, not the wall clock, drives frame
 * cadence — while a small buffer absorbs scheduling jitter.
 */
private class AudioPlayer(sampleRate: Int) {
    private val track: AudioTrack
    init {
        val minBuf = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_FLOAT,
        )
        // ~4 NES frames of headroom (>= the platform minimum) absorbs jitter
        // without adding noticeable latency.
        val bufBytes = maxOf(minBuf, sampleRate * 4 / 60 * 4)
        track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_GAME)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setBufferSizeInBytes(bufBytes)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .build()
        track.play()
    }

    /**
     * Write one frame of samples; blocks while the ring is full (paces the loop).
     *
     * UniFFI maps the core's `Vec<f32>` to a boxed `List<Float>`, so this copies
     * into a primitive `FloatArray` for the `AudioTrack` write. The per-frame
     * boxing is a known cost; a later increment moves the audio pull into the
     * `rustynes-android` JNI hot path (a primitive `float[]`/native ring) per the
     * v1.8.0 plan's Workstream C.
     */
    fun write(samples: List<Float>) {
        if (samples.isNotEmpty()) {
            val buf = samples.toFloatArray()
            track.write(buf, 0, buf.size, AudioTrack.WRITE_BLOCKING)
        }
    }

    fun release() {
        runCatching { track.pause(); track.flush(); track.stop() }
        track.release()
    }
}

@Composable
private fun EmulatorScreen(emulator: EmulatorHandle) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var frame by remember { mutableStateOf<ImageBitmap?>(null) }
    var status by remember { mutableStateOf("Open a .nes ROM to start") }
    var recents by remember { mutableStateOf(RomLibrary.recents(context)) }

    // SAF document picker — no broad storage permission required. The picked
    // bytes are handed straight to the core (no path), a persistable read grant
    // is taken, and the ROM is recorded in the recents list (Workstream E).
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val name = displayName(context, uri)
                val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                status = loadRom(context, emulator, bytes, uri, name)
                recents = RomLibrary.recents(context)
            }.onFailure { status = "Failed to load ROM: ${it.message}" }
        }
    }

    // Open a recent ROM via its persistable content URI.
    fun openRecent(rom: RecentRom) {
        runCatching {
            val uri = Uri.parse(rom.uri)
            val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
            status = loadRom(context, emulator, bytes, uri, rom.name)
            recents = RomLibrary.recents(context)
        }.onFailure { status = "Can't open ${rom.name}: ${it.message}" }
    }

    // Debug-only convenience: auto-load a ROM pushed to the app's external files
    // dir (`/sdcard/Android/data/<pkg>/files/autoload.nes`) so the render path
    // can be verified on-device without driving the SAF picker by hand. Never
    // shipped (BuildConfig.DEBUG-gated); release boots straight to the picker.
    LaunchedEffect(Unit) {
        if (BuildConfig.DEBUG && emulator.controller == null) {
            val auto = java.io.File(context.getExternalFilesDir(null), "autoload.nes")
            if (auto.exists()) {
                runCatching {
                    status = loadRom(context, emulator, auto.readBytes(), null, "autoload")
                }.onFailure { status = "Autoload failed: ${it.message}" }
            }
        }
    }

    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // The NES image takes the remaining vertical space and letterboxes
        // (ContentScale.Fit) — driving height from width would overflow on a
        // wide/foldable display and push the controls off-screen.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
                .background(Color.Black),
            contentAlignment = Alignment.Center,
        ) {
            val current = frame
            if (current != null) {
                Image(
                    bitmap = current,
                    contentDescription = "NES screen",
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Fit,
                    // Nearest-neighbour: preserve the crisp pixel grid.
                    filterQuality = FilterQuality.None,
                )
            } else {
                // Idle: status + the recent-ROMs list (tap to resume).
                Column(
                    modifier = Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(status, color = Color.White)
                    if (recents.isNotEmpty()) {
                        Spacer(Modifier.height(16.dp))
                        Text("Recent", color = Color.Gray)
                        recents.forEach { rom ->
                            Text(
                                rom.name,
                                color = Color(0xFFB39DDB),
                                modifier = Modifier
                                    .padding(vertical = 8.dp)
                                    .clickable { openRecent(rom) },
                            )
                        }
                    }
                }
            }
        }

        // Control bar: load / save / load / reset / pause / fast-forward / mute.
        // Horizontally scrollable so all controls reach on a narrow cover screen.
        var paused by remember { mutableStateOf(false) }
        var turbo by remember { mutableStateOf(false) }
        var muted by remember { mutableStateOf(false) }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 8.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(onClick = { picker.launch(arrayOf("*/*")) }) { Text("Open") }
            OutlinedButton(onClick = {
                val ctrl = emulator.controller
                val sha = emulator.romSha
                if (ctrl != null && sha != null) {
                    runCatching { SaveStateStore.save(context, sha, "1", ctrl.saveState()) }
                        .onSuccess { status = "Saved slot 1" }
                        .onFailure { status = "Save failed: ${it.message}" }
                }
            }) { Text("Save") }
            OutlinedButton(onClick = {
                val ctrl = emulator.controller
                val sha = emulator.romSha
                val blob = if (sha != null) SaveStateStore.load(context, sha, "1") else null
                if (ctrl != null && blob != null) {
                    runCatching { ctrl.loadState(blob) }
                        .onSuccess { status = "Loaded slot 1" }
                        .onFailure { status = "Load failed: ${it.message}" }
                } else {
                    status = "No save in slot 1"
                }
            }) { Text("Load") }
            OutlinedButton(onClick = { emulator.controller?.reset() }) { Text("Reset") }
            OutlinedButton(onClick = {
                paused = !paused
                emulator.paused = paused
            }) { Text(if (paused) "Resume" else "Pause") }
            OutlinedButton(onClick = {
                turbo = !turbo
                emulator.turbo = turbo
            }) { Text(if (turbo) ">> On" else ">>") }
            OutlinedButton(onClick = {
                muted = !muted
                emulator.muted = muted
            }) { Text(if (muted) "Unmute" else "Mute") }
        }

        TouchOverlay(emulator)
    }

    // Emulation loop: run frames + render audio on a background dispatcher, then
    // publish each frame to Compose. Pacing is audio-clocked when sound is present
    // (the blocking AudioTrack write paces the loop to real time) with a wall-clock
    // floor so silent ROMs still run at ~60 Hz.
    LaunchedEffect(Unit) {
        val reuse = Bitmap.createBitmap(NES_WIDTH, NES_HEIGHT, Bitmap.Config.ARGB_8888)
        val pixels = IntArray(NES_WIDTH * NES_HEIGHT)
        val audio = AudioPlayer(48_000)
        try {
            while (isActive) {
                val ctrl = emulator.controller
                if (ctrl == null || emulator.paused) {
                    delay(50)
                    continue
                }
                val turbo = emulator.turbo
                val start = System.nanoTime()
                // Emulate, play this frame's audio, and pack the framebuffer all
                // off the main thread (the blocking audio write and the 61k-pixel
                // RGBA->ARGB pack must never run on the UI thread). Only the cheap
                // setPixels + asImageBitmap stay on the UI thread.
                withContext(Dispatchers.Default) {
                    val fb = ctrl.runFrame()
                    val samples = ctrl.drainAudio()
                    // In fast-forward the audio is dropped (writing it would block
                    // the loop back to real time); otherwise play unless muted.
                    if (!turbo && !emulator.muted) audio.write(samples)
                    packRgbaToArgb(fb, pixels)
                }
                reuse.setPixels(pixels, 0, NES_WIDTH, 0, 0, NES_WIDTH, NES_HEIGHT)
                frame = reuse.asImageBitmap()
                // Fast-forward skips the pacing delay so the core runs ahead.
                if (!turbo) {
                    val remainingMs = (FRAME_NANOS - (System.nanoTime() - start)) / 1_000_000
                    if (remainingMs > 0) delay(remainingMs)
                }
            }
        } finally {
            audio.release()
        }
    }

    DisposableEffect(Unit) {
        onDispose { emulator.controller = null }
    }
}

/** Convert the core's RGBA8 framebuffer into packed ARGB_8888 pixels. */
private fun packRgbaToArgb(rgba: ByteArray, out: IntArray) {
    var i = 0
    var p = 0
    while (p < out.size) {
        val r = rgba[i].toInt() and 0xFF
        val g = rgba[i + 1].toInt() and 0xFF
        val b = rgba[i + 2].toInt() and 0xFF
        out[p] = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
        i += 4
        p += 1
    }
}

/** On-screen D-pad + A/B/Select/Start, feeding P1 via [NesController.setButton]. */
@Composable
private fun TouchOverlay(emulator: EmulatorHandle) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // D-pad cluster.
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            PadButton("▲", emulator, NesButton.UP)
            Row {
                PadButton("◀", emulator, NesButton.LEFT)
                PadButton("▶", emulator, NesButton.RIGHT)
            }
            PadButton("▼", emulator, NesButton.DOWN)
        }
        // Select / Start.
        Row {
            PadButton("SEL", emulator, NesButton.SELECT)
            PadButton("STA", emulator, NesButton.START)
        }
        // Face buttons.
        Row {
            PadButton("B", emulator, NesButton.B)
            PadButton("A", emulator, NesButton.A)
        }
    }
}

@Composable
private fun PadButton(label: String, emulator: EmulatorHandle, button: NesButton) {
    Box(
        modifier = Modifier
            .padding(4.dp)
            .size(56.dp)
            .background(Color.DarkGray)
            .pointerInput(button) {
                detectTapGestures(
                    onPress = {
                        emulator.controller?.setButton(0u, button, true)
                        // Suspends until release/cancel; the button is released
                        // either way so a slide-off can't leave it stuck.
                        tryAwaitRelease()
                        emulator.controller?.setButton(0u, button, false)
                    },
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.White)
    }
}
