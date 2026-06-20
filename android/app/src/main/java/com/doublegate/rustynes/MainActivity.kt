package com.doublegate.rustynes

import android.graphics.Bitmap
import android.os.Bundle
import android.view.KeyEvent
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
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize(), color = Color.Black) {
                    EmulatorScreen(emulator)
                }
            }
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

private const val NES_WIDTH = 256
private const val NES_HEIGHT = 240

@Composable
private fun EmulatorScreen(emulator: EmulatorHandle) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var frame by remember { mutableStateOf<ImageBitmap?>(null) }
    var status by remember { mutableStateOf("Open a .nes ROM to start") }

    // SAF document picker — no broad storage permission required. The picked
    // bytes are handed straight to the core (no path, exactly like the wasm
    // byte-picker). Persistable URI grants for a library view land in beta.4.
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = context.contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                emulator.controller = NesController(bytes, 48000u)
                status = "Running"
            }.onFailure { status = "Failed to load ROM: ${it.message}" }
        }
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
                    emulator.controller = NesController(auto.readBytes(), 48000u)
                    status = "Running (autoload)"
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
                Text(status, color = Color.White)
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(8.dp),
            horizontalArrangement = Arrangement.Center,
        ) {
            Button(onClick = { picker.launch(arrayOf("*/*")) }) { Text("Open ROM") }
        }

        TouchOverlay(emulator)
    }

    // Emulation loop: run frames on a background dispatcher, pacing to ~60 Hz,
    // and publish each frame to Compose. (beta.3 replaces the wall-clock delay
    // with audio-clocked pacing via the AAudio sink + DRC.)
    LaunchedEffect(Unit) {
        val reuse = Bitmap.createBitmap(NES_WIDTH, NES_HEIGHT, Bitmap.Config.ARGB_8888)
        val pixels = IntArray(NES_WIDTH * NES_HEIGHT)
        while (isActive) {
            val ctrl = emulator.controller
            if (ctrl == null) {
                delay(100)
                continue
            }
            val rgba = withContext(Dispatchers.Default) { ctrl.runFrame() }
            packRgbaToArgb(rgba, pixels)
            reuse.setPixels(pixels, 0, NES_WIDTH, 0, 0, NES_WIDTH, NES_HEIGHT)
            frame = reuse.asImageBitmap()
            delay(16) // ~60 FPS; refined in beta.3.
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
