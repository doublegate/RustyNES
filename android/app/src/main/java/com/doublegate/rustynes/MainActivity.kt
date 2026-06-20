package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.view.KeyEvent
import java.security.MessageDigest
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
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
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import uniffi.rustynes_mobile.NesController
import uniffi.rustynes_mobile.RaLoginStatus
import uniffi.rustynes_mobile.RaToast

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

    /** Freemium entitlement (Workstream M); created in onCreate. */
    private lateinit var license: LicenseManager

    /** Thermal-throttle listener (perf/battery); cancels fast-forward when hot. */
    private var thermalListener: android.os.PowerManager.OnThermalStatusChangedListener? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        license = LicenseManager(applicationContext)
        // Only connect to Play Billing in the Play build; off-Play it can't transact.
        if (BuildConfig.PLAY_BUILD) license.connect()
        registerThermalBackoff()
        enableEdgeToEdge()
        hideSystemBars()
        setContent {
            val settings = remember { AppSettings(this@MainActivity) }
            val dark = when (settings.themeMode) {
                ThemeMode.System -> isSystemInDarkTheme()
                ThemeMode.Light -> false
                ThemeMode.Dark -> true
            }
            MaterialTheme(colorScheme = if (dark) darkColorScheme() else lightColorScheme()) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background,
                ) {
                    EmulatorScreen(emulator, license, settings)
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Re-verify entitlement against Play on each foreground (a purchase made
        // elsewhere, a refund, or a restore reflects here).
        if (BuildConfig.PLAY_BUILD && ::license.isInitialized) license.refreshEntitlement()
    }

    // Cancel fast-forward when the device starts thermally throttling — the NES
    // itself is light, but uncapped fast-forward can heat a phone; emulation
    // speed (and thus determinism) is unaffected, only the host pacing.
    private fun registerThermalBackoff() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val pm = getSystemService(POWER_SERVICE) as android.os.PowerManager
        val l = android.os.PowerManager.OnThermalStatusChangedListener { status ->
            if (status >= android.os.PowerManager.THERMAL_STATUS_SEVERE) emulator.turbo = false
        }
        pm.addThermalStatusListener(l)
        thermalListener = l
    }

    override fun onDestroy() {
        super.onDestroy()
        thermalListener?.let {
            (getSystemService(POWER_SERVICE) as android.os.PowerManager).removeThermalStatusListener(it)
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
        // RetroAchievements progress sidecar (v1.8.6) is persisted unconditionally —
        // it is unlock progress, not a save-state, so the freemium gate below does
        // not apply. A no-op when no RA session / game is loaded (empty blob).
        if (ctrl != null && sha != null) {
            runCatching {
                val blob = ctrl.raSerializeProgress()
                if (blob.isNotEmpty()) RaProgressStore.save(this, sha, blob)
            }
        }
        // Save-on-background is a paid feature in the Play build; sideload builds
        // (PLAY_BUILD=false) always persist. The demo never persists state.
        if (BuildConfig.PLAY_BUILD && (!::license.isInitialized || !license.isUnlocked)) return
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

    /** Raw bytes of the loaded ROM — kept so RetroAchievements can (re-)identify
     *  the game via `raLoadGame` if login completes after the ROM was opened. */
    var romBytes: ByteArray? = null

    /** Emulation paused (the loop idles, no frames advance). Read by the loop. */
    @Volatile
    var paused: Boolean = false

    /** Fast-forward: drop the frame-pace delay + audio so the core runs ahead. */
    @Volatile
    var turbo: Boolean = false

    /** Mute the audio sink (the core still produces samples; they're discarded). */
    @Volatile
    var muted: Boolean = false

    // P1 input is the OR of two sources that must not clobber each other: the
    // on-screen virtual controller (multi-touch) and any hardware gamepad/keyboard.
    // Each updates its own mask; applyP1() combines them into the bridge's single
    // late-latched P1 mask.
    @Volatile
    private var touchMask: Int = 0

    @Volatile
    private var keyMask: Int = 0

    /** Set the on-screen virtual-controller mask (the full set of pressed buttons). */
    fun setTouchMask(mask: Int) {
        // Touch + key updates can race (different threads); synchronize the
        // read-modify-combine so neither source clobbers the other's mask.
        synchronized(this) {
            touchMask = mask
            applyP1()
        }
    }

    /** Returns true if the key was consumed (a mapped NES button). */
    fun onKey(keyCode: Int, pressed: Boolean): Boolean {
        val bit = keyToBit(keyCode) ?: return false
        synchronized(this) {
            keyMask = if (pressed) keyMask or bit else keyMask and bit.inv()
            applyP1()
        }
        return true
    }

    private fun applyP1() {
        controller?.setButtons(0u, (touchMask or keyMask).toUByte())
    }

    private fun keyToBit(keyCode: Int): Int? = when (keyCode) {
        KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_DPAD_CENTER -> NesBit.A
        KeyEvent.KEYCODE_BUTTON_B -> NesBit.B
        KeyEvent.KEYCODE_BUTTON_START, KeyEvent.KEYCODE_ENTER -> NesBit.START
        KeyEvent.KEYCODE_BUTTON_SELECT, KeyEvent.KEYCODE_SPACE -> NesBit.SELECT
        KeyEvent.KEYCODE_DPAD_UP -> NesBit.UP
        KeyEvent.KEYCODE_DPAD_DOWN -> NesBit.DOWN
        KeyEvent.KEYCODE_DPAD_LEFT -> NesBit.LEFT
        KeyEvent.KEYCODE_DPAD_RIGHT -> NesBit.RIGHT
        else -> null
    }
}

/** NES controller button bits — matches `rustynes_core::Buttons`. */
object NesBit {
    const val A = 0x01
    const val B = 0x02
    const val SELECT = 0x04
    const val START = 0x08
    const val UP = 0x10
    const val DOWN = 0x20
    const val LEFT = 0x40
    const val RIGHT = 0x80
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
    unlocked: Boolean,
    settings: AppSettings,
): String {
    val ctrl = NesController(bytes, 48_000u)
    val sha = sha256Hex(bytes)
    emulator.controller = ctrl
    emulator.romSha = sha
    emulator.romBytes = bytes
    // Apply this game's remembered video filter (per-game DB), if any.
    GameConfig.filter(context, sha)?.let { f ->
        settings.filter = VideoFilter.entries.getOrElse(f) { VideoFilter.None }
    }
    // Auto-resume the on-background save-state is a paid feature; the demo always
    // cold-boots the ROM.
    if (unlocked) {
        SaveStateStore.load(context, sha, SaveStateStore.AUTO_SLOT)?.let { blob ->
            runCatching { ctrl.loadState(blob) }
        }
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

    /**
     * v1.8.4 hot path: write raw little-endian `f32` bytes straight to the
     * `PCM_FLOAT` track. The bytes come from `NesController.drainAudioBytes()` as a
     * single `ByteArray` (no per-sample `Float` boxing); `AudioTrack` reads them as
     * native-order PCM floats (Android is little-endian), so there's no float copy
     * either. Blocks while the ring is full (paces the loop), exactly like [write].
     */
    fun writeBytes(bytes: ByteArray) {
        if (bytes.isNotEmpty()) {
            // The samples are little-endian f32 (from `to_le_bytes`); set the buffer
            // order explicitly so the PCM_FLOAT track reads them correctly regardless
            // of the JVM's default (BIG_ENDIAN) — Android is LE, so this is identity.
            val bb = java.nio.ByteBuffer.wrap(bytes).order(java.nio.ByteOrder.LITTLE_ENDIAN)
            track.write(bb, bytes.size, AudioTrack.WRITE_BLOCKING)
        }
    }

    fun release() {
        runCatching { track.pause(); track.flush(); track.stop() }
        track.release()
    }
}

@Composable
private fun EmulatorScreen(emulator: EmulatorHandle, license: LicenseManager, settings: AppSettings) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val activity = context as? Activity
    // Freemium is active only in the Play build; sideload/dev builds are unlimited.
    val unlocked = !BuildConfig.PLAY_BUILD || license.isUnlocked
    var frame by remember { mutableStateOf<ImageBitmap?>(null) }
    // HD-pack (v1.8.5): `hdActive` switches the UI to the Bitmap path (the GPU
    // SurfaceView is fixed 256x240; HD output is upscaled), `hd` holds its bitmap.
    var hdActive by remember { mutableStateOf(false) }
    val hd = remember { HdRender() }
    // Lua scripting (v1.8.6): whether a script is loaded + its rolling log output.
    var scriptLoaded by remember { mutableStateOf(false) }
    var scriptLog by remember { mutableStateOf("") }
    // RetroAchievements (v1.8.6): coarse login status text, the signed-in user (or
    // null), and the live HUD toast queue drained from the bridge each frame.
    var raStatus by remember { mutableStateOf("Logged out") }
    var raUserName by remember { mutableStateOf<String?>(null) }
    var raToasts by remember { mutableStateOf<List<RaToast>>(emptyList()) }
    // Set after a game was opened so RA can (re-)identify it once login completes;
    // cleared once raLoadGame has been issued for the current login + ROM.
    var raGameLoaded by remember { mutableStateOf(false) }
    // Tracks the previous login status to detect the LOGGED_OUT -> LOGGED_IN edge.
    var raWasLoggedIn by remember { mutableStateOf(false) }
    // Off-main-thread scope for the one-shot SAF loads (HD-pack parse, config I/O).
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf("Open a .nes ROM to start") }
    var recents by remember { mutableStateOf(RomLibrary.recents(context)) }
    // Demo session clock: seconds remaining this launch (full unlock = no limit).
    var demoSecondsLeft by remember { mutableStateOf(DEMO_SESSION_SECONDS) }
    var demoExpired by remember { mutableStateOf(false) }
    // Settings are created at the theme root and passed in (v1.8.3).
    // Drive the audio-mute flag from the persisted setting.
    LaunchedEffect(settings.muted) { emulator.muted = settings.muted }
    var showSettings by remember { mutableStateOf(false) }
    var showStates by remember { mutableStateOf(false) }
    var showAbout by remember { mutableStateOf(false) }
    // First-run onboarding shows until the user ticks "Do not show again".
    var showOnboarding by remember { mutableStateOf(!settings.onboardingSuppressed) }
    var screenSize by remember { mutableStateOf(androidx.compose.ui.unit.IntSize.Zero) }

    // Casting (item 1): track external presentation displays and present the
    // gameplay there while the phone keeps the controller.
    val castManager = remember { CastManager(context) }
    DisposableEffect(Unit) {
        castManager.register()
        onDispose { castManager.unregister() }
    }
    // Current screen mode (item 5): each remembers its own controller size/opacity.
    // Cast wins; otherwise a narrow width means the folded cover screen.
    val config = androidx.compose.ui.platform.LocalConfiguration.current
    val screenMode = when {
        castManager.casting -> ScreenMode.Cast
        config.screenWidthDp < 520 -> ScreenMode.Cover
        else -> ScreenMode.Inner
    }

    // Native wgpu SurfaceView renderer (v1.8.4, Workstream B). Opt-in + API 33+;
    // read ONCE at launch so it never flips mid-session. When off (default), the
    // proven Compose Bitmap path is used unchanged. The emulation loop still packs
    // the Bitmap (for casting / the idle thumbnail); the SurfaceView just draws the
    // raw frame on the GPU.
    val gpuSurface = remember {
        if (settings.useGpuRenderer &&
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            NativeRenderer.ensureLoaded()
        ) {
            NesSurfaceView(context)
        } else {
            null
        }
    }
    // Drive the GPU renderer's filter + its tuning params from the settings; the
    // VideoFilter ordinals line up with the native filter codes. Re-applies live
    // whenever the selected filter OR any of its sliders change. No-op without it.
    LaunchedEffect(
        gpuSurface,
        settings.filter,
        settings.scanlineIntensity,
        settings.scanlineRows,
        settings.apertureMask,
        settings.ntscSaturation,
        settings.ntscSharpness,
        settings.ntscTint,
        settings.ntscPhase,
    ) {
        gpuSurface?.setFilter(settings.filter.ordinal, settings.filterParams(settings.filter))
    }

    // Per-game DB (v1.8.5): remember the chosen filter for the loaded ROM (by SHA),
    // so it reopens with that look. Fires on filter change; a no-op without a ROM.
    LaunchedEffect(settings.filter) {
        emulator.romSha?.takeIf { it.isNotEmpty() }?.let { sha ->
            // The JSON read+write is small but still disk I/O — keep it off the main
            // thread (this effect runs on the main dispatcher).
            withContext(Dispatchers.IO) {
                GameConfig.setFilter(context, sha, settings.filter.ordinal)
            }
        }
    }

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
                status = loadRom(context, emulator, bytes, uri, name, unlocked, settings)
                recents = RomLibrary.recents(context)
            }.onFailure { status = "Failed to load ROM: ${it.message}" }
        }
    }

    // SAF picker for a custom .pal palette (a 192-byte RGB table; extra colours,
    // e.g. a 512-colour Mesen palette, are ignored). Applied live to the running
    // core via the bridge; presentation-only, so determinism is untouched.
    val palettePicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                emulator.controller?.loadPalette(bytes)
                status = "Palette loaded: ${displayName(context, uri)}"
            }.onFailure { status = "Failed to load palette: ${it.message}" }
        }
    }

    // SAF picker to play a .rnm TAS movie (reads the bytes, seeks to its start).
    val moviePicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                emulator.controller?.moviePlay(bytes)
                status = "Playing movie: ${displayName(context, uri)}"
            }.onFailure { status = "Failed to play movie: ${it.message}" }
        }
    }

    // SAF document creator to save the just-recorded .rnm movie (finalizes + writes).
    val movieSaver = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = emulator.controller?.movieStopRecording() ?: ByteArray(0)
                context.contentResolver.openOutputStream(uri)!!.use { it.write(bytes) }
                status = "Saved movie (${bytes.size} bytes)"
            }.onFailure { status = "Failed to save movie: ${it.message}" }
        }
    }

    // SAF picker for an HD-pack .zip (Mesen-style hires.txt + PNG tiles). Loads it,
    // sizes the upscaled output bitmap, and switches the UI to the Bitmap path.
    val hdpackPicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            // Read + parse + decode the pack off the main thread (it can be large);
            // the bitmap/state updates land back on the main dispatcher.
            scope.launch {
                runCatching {
                    val dims = withContext(Dispatchers.IO) {
                        val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                        emulator.controller?.loadHdpackFromZipBytes(bytes)
                        emulator.controller?.hdpackDimensions() ?: listOf(0u, 0u)
                    }
                    val w = dims[0].toInt()
                    val h = dims[1].toInt()
                    if (w > 0 && h > 0) {
                        hd.w = w
                        hd.h = h
                        hd.bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                        hd.pixels = IntArray(w * h)
                        hdActive = true
                        status = "HD-pack loaded (${w}x$h)"
                    } else {
                        status = "HD-pack: no usable tiles"
                    }
                }.onFailure { status = "Failed to load HD-pack: ${it.message}" }
            }
        }
    }

    // SAF picker for a Lua script (.lua) — loaded into the sandboxed engine; its
    // on_frame callback then runs each frame and its print/log output is shown.
    val scriptPicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            scope.launch {
                runCatching {
                    val src = withContext(Dispatchers.IO) {
                        context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                            .decodeToString()
                    }
                    withContext(Dispatchers.IO) { emulator.controller?.loadScript(src) }
                    scriptLoaded = true
                    scriptLog = ""
                    status = "Script loaded: ${displayName(context, uri)}"
                }.onFailure { status = "Failed to load script: ${it.message}" }
            }
        }
    }

    // Identify the loaded ROM to RetroAchievements: compute its SHA-256, read any
    // saved progress sidecar, and call raLoadGame off-thread. A no-op unless RA is
    // enabled, a ROM is loaded, and the session is logged in. Marks raGameLoaded so
    // a login that completes after the ROM was opened can re-issue this once.
    fun raIdentifyGame() {
        val ctrl = emulator.controller ?: return
        val bytes = emulator.romBytes ?: return
        if (!settings.raEnabled) return
        scope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    if (!ctrl.raIsEnabled() || ctrl.raLoginStatus() != RaLoginStatus.LOGGED_IN) {
                        return@withContext
                    }
                    val sha = MessageDigest.getInstance("SHA-256").digest(bytes)
                    val shaHex = sha.joinToString("") { "%02x".format(it) }
                    val sidecar = RaProgressStore.load(context, shaHex)
                    ctrl.raLoadGame(bytes, sha, sidecar)
                }
                raGameLoaded = true
            }.onFailure { status = "RA load failed: ${it.message}" }
        }
    }

    // Persist the current RA progress sidecar (if any) for the loaded ROM.
    fun raSaveProgress() {
        val ctrl = emulator.controller ?: return
        val sha = emulator.romSha ?: return
        if (!settings.raEnabled) return
        runCatching {
            val blob = ctrl.raSerializeProgress()
            if (blob.isNotEmpty()) RaProgressStore.save(context, sha, blob)
        }
    }

    // Open a recent ROM via its persistable content URI.
    fun openRecent(rom: RecentRom) {
        runCatching {
            val uri = Uri.parse(rom.uri)
            val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
            status = loadRom(context, emulator, bytes, uri, rom.name, unlocked, settings)
            recents = RomLibrary.recents(context)
        }.onFailure { status = "Can't open ${rom.name}: ${it.message}" }
    }

    // Demo countdown: tick once per second while a ROM is running, unpaused, and
    // not yet unlocked; on expiry, pause the emulator and raise the unlock sheet.
    // Purchasing (unlocked -> true) cancels the limit immediately.
    LaunchedEffect(unlocked) {
        if (unlocked) {
            demoExpired = false
            return@LaunchedEffect
        }
        while (true) {
            kotlinx.coroutines.delay(1000)
            if (emulator.controller != null && !emulator.paused && !demoExpired) {
                demoSecondsLeft -= 1
                if (demoSecondsLeft <= 0) {
                    demoExpired = true
                    emulator.paused = true
                }
            }
        }
    }

    // RetroAchievements auto-login (v1.8.6): on first composition, if RA is enabled
    // and a token was saved from a prior password login, init the session and
    // token-login silently (fire-and-forget; status/toasts are polled in the loop).
    LaunchedEffect(Unit) {
        if (settings.raEnabled && settings.raToken.isNotEmpty() && settings.raUsername.isNotEmpty()) {
            withContext(Dispatchers.IO) {
                emulator.controller // touch (no-op); RA session is controller-scoped
                runCatching {
                    // The session is created lazily on the first ra_* call; init it
                    // and token-login. If no controller exists yet, the login still
                    // registers on the session and re-applies when a ROM is opened.
                    val ctrl = emulator.controller
                    if (ctrl != null) {
                        ctrl.raInit(settings.raHardcore)
                        ctrl.raLoginToken(settings.raUsername, settings.raToken)
                    }
                }
            }
        }
    }

    // (Re-)identify the loaded game to RA whenever the ROM changes (keyed by SHA).
    // A no-op until logged in; the login-edge handler in the loop re-issues it then.
    LaunchedEffect(emulator.romSha) {
        raGameLoaded = false
        raIdentifyGame()
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
                    status = loadRom(context, emulator, auto.readBytes(), null, "autoload", unlocked, settings)
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
            // GPU SurfaceView render path (opt-in, v1.8.4). Mounted continuously and
            // draws each submitted frame on the GPU (black until the first frame).
            // GPU path is bypassed while an HD-pack is active (its output is upscaled,
            // but the GPU texture is fixed 256x240 — HD goes through the Bitmap path).
            if (gpuSurface != null && !hdActive) {
                androidx.compose.ui.viewinterop.AndroidView(
                    factory = { gpuSurface },
                    modifier = Modifier.fillMaxSize(),
                )
            }
            if ((gpuSurface == null || hdActive) && current != null) {
                Image(
                    bitmap = current,
                    contentDescription = "NES screen",
                    modifier = Modifier
                        .fillMaxSize()
                        .onSizeChanged { screenSize = it }
                        .then(
                            if (videoFiltersSupported && settings.filter != VideoFilter.None && screenSize.width > 0) {
                                Modifier.graphicsLayer {
                                    renderEffect = buildRenderEffect(
                                        settings.filter,
                                        screenSize.width.toFloat(),
                                        screenSize.height.toFloat(),
                                    )
                                }
                            } else {
                                Modifier
                            },
                        ),
                    contentScale = ContentScale.Fit,
                    // Nearest-neighbour: preserve the crisp pixel grid.
                    filterQuality = FilterQuality.None,
                )
            }
            // RetroAchievements toast HUD (v1.8.6) — unlock + login/server messages.
            // Text-only cards (no badge images); error toasts tint red. They clear
            // when the next poll returns an empty queue.
            if (raToasts.isNotEmpty()) {
                Column(
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(8.dp),
                    horizontalAlignment = Alignment.End,
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    raToasts.forEach { toast ->
                        Column(
                            modifier = Modifier
                                .background(if (toast.isError) Color(0xD0701010) else Color(0xC0102030))
                                .padding(horizontal = 8.dp, vertical = 6.dp),
                        ) {
                            Text(
                                toast.title,
                                color = if (toast.isError) Color(0xFFFFCDD2) else Color.White,
                                fontSize = 12.sp,
                                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                            )
                            if (toast.detail.isNotEmpty()) {
                                Text(toast.detail, color = Color(0xFFCFD8DC), fontSize = 10.sp)
                            }
                        }
                    }
                }
            }
            // Lua script log overlay (v1.8.6) — the script's print/log output.
            if (scriptLoaded && scriptLog.isNotEmpty()) {
                Text(
                    scriptLog,
                    color = Color(0xFF7CFC00),
                    fontSize = 10.sp,
                    modifier = Modifier
                        .align(Alignment.TopStart)
                        .padding(8.dp)
                        .background(Color(0xC0000000))
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                )
            }
            if (current == null) {
                // Idle: status + the recent-ROMs list (tap to resume).
                Column(
                    modifier = Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(status, color = Color.White)
                    if (recents.isNotEmpty()) {
                        Spacer(Modifier.height(16.dp))
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Text("Recent", color = Color.Gray)
                            Text(
                                "Clear Recent",
                                color = Color(0xFFEF9A9A),
                                modifier = Modifier.clickable {
                                    RomLibrary.clear(context)
                                    recents = RomLibrary.recents(context)
                                },
                            )
                        }
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
            // While casting, a small banner over the (still-mirrored) phone picture.
            if (castManager.casting) {
                Text(
                    "Casting to ${castManager.displayName ?: "TV"}",
                    color = Color(0xFF80D8FF),
                    modifier = Modifier.align(Alignment.TopCenter).padding(8.dp),
                )
            }
        }

        // Control bar: open / states / reset / pause / fast-forward / settings.
        // Hidden until the controller's "RustyNES" pill is first tapped (it
        // toggles thereafter). Horizontally scrollable so all controls reach on a
        // narrow cover screen.
        var paused by remember { mutableStateOf(false) }
        var turbo by remember { mutableStateOf(false) }
        var controlsVisible by remember { mutableStateOf(false) }
        if (controlsVisible) {
            Row(
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 8.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(onClick = { picker.launch(arrayOf("*/*")) }) { Text("Open") }
            // Save-states are a paid feature; the demo hides the manager.
            if (unlocked) {
                OutlinedButton(onClick = { showStates = true }) { Text("States") }
            }
            OutlinedButton(onClick = { emulator.controller?.reset() }) { Text("Reset") }
            OutlinedButton(onClick = {
                paused = !paused
                emulator.paused = paused
            }) { Text(if (paused) "Resume" else "Pause") }
            OutlinedButton(onClick = {
                turbo = !turbo
                emulator.turbo = turbo
            }) { Text(if (turbo) ">> On" else ">>") }
            OutlinedButton(onClick = { showSettings = true }) { Text("Settings") }
            OutlinedButton(onClick = { showAbout = true }) { Text("About") }
            // Cast gameplay to a connected TV/external display (only when present).
            if (castManager.available) {
                OutlinedButton(onClick = { castManager.toggle() }) {
                    Text(if (castManager.casting) "Stop Cast" else "Cast to TV")
                }
            }
            // Demo: an always-visible unlock affordance + the session countdown.
            if (!unlocked) {
                val price = license.product
                    ?.oneTimePurchaseOfferDetails?.formattedPrice ?: "$2.99"
                Button(onClick = { activity?.let { license.purchase(it) } }) {
                    Text("Unlock $price")
                }
                val mins = demoSecondsLeft / 60
                val secs = demoSecondsLeft % 60
                Text(
                    "Demo · %d:%02d".format(mins, secs),
                    color = Color.Gray,
                )
            }
            // Debug-only (and only meaningful when the freemium is active, i.e. a
            // PLAY_BUILD debug build): simulate the Full Unlock without Play.
            if (BuildConfig.DEBUG && BuildConfig.PLAY_BUILD) {
                OutlinedButton(onClick = { license.debugForceUnlocked(!unlocked) }) {
                    Text(if (unlocked) "DBG:demo" else "DBG:unlock")
                }
            }
            }
        } // end control bar (toggled by the RustyNES pill)

        // The multi-touch virtual NES controller, sized to the NES-001 aspect
        // (123:53, the real NES-004 proportions). Its width is `controllerScale`
        // (0.25–1.1×) of the available width, centered, so it rescales for the
        // active display AND the user's preference; >1 overruns the screen edges
        // by design. Its hit regions remap in lockstep (art + regions both derive
        // from the measured size). The host Box reserves the LARGEST (110%)
        // controller height, so changing the size never reflows/shifts the
        // gameplay view above it (item 7).
        BoxWithConstraints(modifier = Modifier.fillMaxWidth()) {
            val mw = maxWidth // capture: not visible inside the inner BoxScope
            val reserved = mw * 1.1f * 53f / 123f
            Box(
                modifier = Modifier.fillMaxWidth().height(reserved),
                contentAlignment = Alignment.Center,
            ) {
                VirtualController(
                    emulator,
                    settings.hapticLevel,
                    { controlsVisible = !controlsVisible },
                    Modifier
                        .width(mw * settings.controllerScale(screenMode))
                        .aspectRatio(123f / 53f)
                        .alpha(settings.controllerOpacity(screenMode)),
                )
            }
        }
    }

    // Settings + save-state manager sheets (v1.8.3).
    if (showSettings) {
        SettingsSheet(
            settings,
            screenMode,
            onLoadPalette = { palettePicker.launch(arrayOf("*/*")) },
            onClearPalette = {
                emulator.controller?.clearPalette()
                status = "Palette reset to default"
            },
            onMovieRecord = {
                emulator.controller?.movieRecordFromPowerOn()
                status = "Recording movie from power-on"
            },
            onMoviePlay = { moviePicker.launch(arrayOf("*/*")) },
            onMovieSave = { movieSaver.launch("movie.rnm") },
            onMovieStop = {
                emulator.controller?.movieStop()
                status = "Movie stopped"
            },
            onLoadHdpack = { hdpackPicker.launch(arrayOf("*/*")) },
            onUnloadHdpack = {
                emulator.controller?.unloadHdpack()
                hdActive = false
                hd.bitmap = null
                status = "HD-pack unloaded"
            },
            onLoadScript = { scriptPicker.launch(arrayOf("*/*")) },
            onUnloadScript = {
                emulator.controller?.unloadScript()
                scriptLoaded = false
                scriptLog = ""
                status = "Script unloaded"
            },
            raStatus = raStatus,
            raUser = raUserName,
            onRaLogin = { user, pass ->
                // Off-thread, like the SAF loads: init the session at the current
                // hardcore setting and fire the async password login. Status/toasts
                // are polled in the emulation loop; on the LOGGED_IN edge the token
                // is persisted and the loaded game is identified.
                settings.raUsername = user
                scope.launch {
                    withContext(Dispatchers.IO) {
                        runCatching {
                            val ctrl = emulator.controller
                            if (ctrl != null) {
                                ctrl.raInit(settings.raHardcore)
                                ctrl.raLoginPassword(user, pass)
                                raStatus = "Logging in…"
                            } else {
                                raStatus = "Open a ROM first, then log in"
                            }
                        }
                    }
                }
            },
            onRaLogout = {
                scope.launch {
                    withContext(Dispatchers.IO) { runCatching { emulator.controller?.raLogout() } }
                    settings.raToken = ""
                    raUserName = null
                    raStatus = "Logged out"
                }
            },
            raEnabled = settings.raEnabled,
            onRaEnabledChange = { on ->
                settings.raEnabled = on
                if (on) {
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            runCatching { emulator.controller?.raInit(settings.raHardcore) }
                        }
                    }
                }
            },
            raHardcore = settings.raHardcore,
            onRaHardcoreChange = { hc ->
                settings.raHardcore = hc
                scope.launch {
                    withContext(Dispatchers.IO) {
                        runCatching { emulator.controller?.raSetHardcore(hc) }
                    }
                }
            },
            onDismiss = { showSettings = false },
        )
    }
    if (showStates) {
        StatesSheet(
            context, emulator.romSha, emulator,
            onStatus = { status = it },
            onDismiss = { showStates = false },
        )
    }
    if (showAbout) {
        AboutDialog(onDismiss = { showAbout = false })
    }
    if (showOnboarding) {
        OnboardingDialogs(
            onSuppress = { settings.onboardingSuppressed = true },
            onFinished = { showOnboarding = false },
        )
    }

    // Demo-expired gate: a blocking sheet over everything with Unlock + Restore.
    if (!unlocked && demoExpired) {
        DemoExpiredOverlay(
            price = license.product?.oneTimePurchaseOfferDetails?.formattedPrice ?: "$2.99",
            onUnlock = { activity?.let { license.purchase(it) } },
            onRestore = { license.refreshEntitlement() },
        )
    }

    // Emulation loop: run frames + render audio on a background dispatcher, then
    // publish each frame to Compose. Pacing is audio-clocked when sound is present
    // (the blocking AudioTrack write paces the loop to real time) with a wall-clock
    // floor so silent ROMs still run at ~60 Hz.
    LaunchedEffect(Unit) {
        val reuse = Bitmap.createBitmap(NES_WIDTH, NES_HEIGHT, Bitmap.Config.ARGB_8888)
        val pixels = IntArray(NES_WIDTH * NES_HEIGHT)
        val audio = AudioPlayer(48_000)
        // RetroAchievements is polled at a low cadence (every ~15 frames) — toasts
        // and login status don't need per-frame granularity, and skipping the FFI
        // round-trips keeps the hot path clean when RA is off.
        var raFrame = 0
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
                var usedHd = false
                var logLines: List<String> = emptyList()
                // Capture the HD bitmap once per iteration so an Unload on the UI
                // thread can't null it mid-frame (the local keeps the object alive).
                val hdBmp = if (hdActive) hd.bitmap else null
                withContext(Dispatchers.Default) {
                    val fb = ctrl.runFrame()
                    if (hdBmp != null) {
                        // HD-pack: composite the upscaled frame (Bitmap path only —
                        // the GPU SurfaceView is fixed at 256x240).
                        val comp = ctrl.compositeHdFrame()
                        if (comp.size == hd.w * hd.h * 4) {
                            packRgbaToArgb(comp, hd.pixels)
                            usedHd = true
                        } else {
                            packRgbaToArgb(fb, pixels)
                        }
                    } else {
                        // Bisqwit needs the palette-index frame + NTSC phase; submit it
                        // BEFORE the frame so the render thread pairs them (it consumes
                        // the frame first). Only fetched while that filter is active.
                        if (gpuSurface != null && settings.filter == VideoFilter.Bisqwit) {
                            gpuSurface.submitIndexFrame(ctrl.indexFramebufferBytes(), ctrl.ntscPhase().toInt())
                        }
                        // Hand the raw RGBA frame to the GPU SurfaceView (opt-in path);
                        // no-op when the GPU renderer is off.
                        gpuSurface?.submitFrame(fb)
                        packRgbaToArgb(fb, pixels)
                    }
                    // Hot path: drain audio as raw bytes (no per-sample Float boxing)
                    // and write straight to the PCM_FLOAT track.
                    val audioBytes = ctrl.drainAudioBytes()
                    // In fast-forward the audio is dropped (writing it would block
                    // the loop back to real time); otherwise play unless muted.
                    if (!turbo && !emulator.muted) audio.writeBytes(audioBytes)
                    // Lua: drain the script's print/log output (cheap when empty).
                    if (scriptLoaded) logLines = ctrl.drainScriptLog()
                }
                if (usedHd && hdBmp != null) {
                    hdBmp.setPixels(hd.pixels, 0, hd.w, 0, 0, hd.w, hd.h)
                    frame = hdBmp.asImageBitmap()
                } else {
                    reuse.setPixels(pixels, 0, NES_WIDTH, 0, 0, NES_WIDTH, NES_HEIGHT)
                    frame = reuse.asImageBitmap()
                }
                // Append new script log lines (keep the last 8 for the overlay).
                if (logLines.isNotEmpty()) {
                    scriptLog = (scriptLog.split("\n").filter { it.isNotEmpty() } + logLines)
                        .takeLast(8).joinToString("\n")
                }
                // Mirror the picture to the external display while casting (no-op
                // otherwise). Same main-thread publish point as the Compose frame.
                castManager.pushFrame(reuse)
                // RetroAchievements: poll the toast queue + login status at a low
                // cadence. Cheap when RA is off (a single `raIsEnabled` bool check).
                if (settings.raEnabled && (++raFrame % 15) == 0) {
                    val rctrl = emulator.controller
                    if (rctrl != null && rctrl.raIsEnabled()) {
                        val toasts = rctrl.raPollToasts()
                        if (toasts.isNotEmpty()) raToasts = toasts
                        val st = rctrl.raLoginStatus()
                        val loggedIn = st == RaLoginStatus.LOGGED_IN
                        if (loggedIn && !raWasLoggedIn) {
                            // LOGGED_OUT -> LOGGED_IN edge: persist the token + user
                            // (never the password) for silent re-login, surface the
                            // user, and identify the loaded game if not yet done.
                            rctrl.raToken()?.let { settings.raToken = it }
                            raUserName = rctrl.raUser()?.displayName ?: settings.raUsername
                            raStatus = "Signed in"
                            if (!raGameLoaded) raIdentifyGame()
                        } else if (!loggedIn && raWasLoggedIn) {
                            raUserName = null
                            raStatus = "Logged out"
                        } else if (st == RaLoginStatus.ERROR) {
                            raStatus = "Login failed"
                        }
                        raWasLoggedIn = loggedIn
                    }
                }
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
        onDispose {
            // Persist RA progress before tearing down the controller (ROM unload).
            raSaveProgress()
            emulator.controller = null
        }
    }
}

/**
 * Render state for an active HD-pack (v1.8.5): the upscaled output bitmap + its
 * scratch ARGB buffer and dimensions. Plain holder (not Compose state) read by the
 * emulation loop; the `hdActive` flag that switches the UI to the Bitmap path is a
 * separate Compose state.
 */
private class HdRender {
    var bitmap: Bitmap? = null
    var pixels: IntArray = IntArray(0)
    var w = 0
    var h = 0
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

/** Blocking sheet shown when the free 10-minute demo session expires. */
@Composable
private fun DemoExpiredOverlay(price: String, onUnlock: () -> Unit, onRestore: () -> Unit) {
    Box(
        modifier = Modifier.fillMaxSize().background(Color(0xE6000000)),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Demo time's up", color = Color.White)
            Spacer(Modifier.height(8.dp))
            Text(
                "Unlock the full version to keep playing — save states, resume, " +
                    "and in-cart battery saves included.",
                color = Color.LightGray,
            )
            Spacer(Modifier.height(20.dp))
            Button(onClick = onUnlock) { Text("Unlock $price") }
            Spacer(Modifier.height(8.dp))
            androidx.compose.material3.TextButton(onClick = onRestore) {
                Text("Restore purchase")
            }
        }
    }
}

// The on-screen controls now live in `VirtualController.kt` — a single multi-touch
// Canvas (the old per-button `TouchOverlay`/`PadButton` registered one input at a
// time and was replaced in v1.8.2).
