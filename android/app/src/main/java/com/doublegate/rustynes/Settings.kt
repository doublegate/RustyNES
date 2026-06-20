package com.doublegate.rustynes

import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

/**
 * User settings (v1.8.3), Compose-observable and persisted in `SharedPreferences`.
 * Pure host/UI policy — none of it touches emulation determinism.
 */
/** App theme: follow the system, or force light/dark. */
enum class ThemeMode(val label: String) { System("System"), Light("Light"), Dark("Dark") }

/** On-screen button haptic strength (Off disables the Vibrator entirely). */
enum class HapticLevel(val label: String) { Off("Off"), Low("Low"), Medium("Medium"), High("High") }

/**
 * Which physical screen the controller is currently drawn on. Each mode remembers
 * its own controller size + opacity (item 5): Cover = folded outer screen,
 * Inner = unfolded large screen, Cast = while casting gameplay to an external
 * display (the controller then has the whole phone to itself).
 */
enum class ScreenMode(val label: String) { Cover("Cover"), Inner("Inner"), Cast("Cast") }

class AppSettings(context: Context) {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    private val _themeMode =
        mutableStateOf(ThemeMode.entries.getOrElse(prefs.getInt("theme", 0)) { ThemeMode.System })
    var themeMode: ThemeMode
        get() = _themeMode.value
        set(v) { _themeMode.value = v; prefs.edit().putInt("theme", v.ordinal).apply() }

    private val _filter =
        mutableStateOf(VideoFilter.entries.getOrElse(prefs.getInt("filter", 0)) { VideoFilter.None })
    var filter: VideoFilter
        get() = _filter.value
        set(v) { _filter.value = v; prefs.edit().putInt("filter", v.ordinal).apply() }

    private val _hapticLevel =
        mutableStateOf(HapticLevel.entries.getOrElse(prefs.getInt("hapticLvl", HapticLevel.Medium.ordinal)) { HapticLevel.Medium })
    var hapticLevel: HapticLevel
        get() = _hapticLevel.value
        set(v) { _hapticLevel.value = v; prefs.edit().putInt("hapticLvl", v.ordinal).apply() }

    private val _muted = mutableStateOf(prefs.getBoolean("muted", false))
    var muted: Boolean
        get() = _muted.value
        set(v) { _muted.value = v; prefs.edit().putBoolean("muted", v).apply() }

    /** Set once the user ticks "Do not show again" on the first-run dialogs. */
    private val _onboardingSuppressed = mutableStateOf(prefs.getBoolean("onboardDone", false))
    var onboardingSuppressed: Boolean
        get() = _onboardingSuppressed.value
        set(v) { _onboardingSuppressed.value = v; prefs.edit().putBoolean("onboardDone", v).apply() }

    /** Use the native wgpu SurfaceView renderer instead of the Compose Bitmap blit
     *  (v1.8.4, API 33+). Off by default; read once at launch (applies on restart). */
    private val _useGpuRenderer = mutableStateOf(prefs.getBoolean("gpuRenderer", false))
    var useGpuRenderer: Boolean
        get() = _useGpuRenderer.value
        set(v) { _useGpuRenderer.value = v; prefs.edit().putBoolean("gpuRenderer", v).apply() }

    // RetroAchievements (v1.8.6). The login token (NOT the password) is persisted so
    // a relaunch can token-login silently; the password is never stored. `raHardcore`
    // gates whether save-states/cheats are permitted while a game is identified.
    private val _raEnabled = mutableStateOf(prefs.getBoolean("raEnabled", false))
    var raEnabled: Boolean
        get() = _raEnabled.value
        set(v) { _raEnabled.value = v; prefs.edit().putBoolean("raEnabled", v).apply() }

    private val _raHardcore = mutableStateOf(prefs.getBoolean("raHardcore", false))
    var raHardcore: Boolean
        get() = _raHardcore.value
        set(v) { _raHardcore.value = v; prefs.edit().putBoolean("raHardcore", v).apply() }

    /** The RA username (persisted; pairs with the saved token for silent re-login). */
    var raUsername: String
        get() = prefs.getString("raUsername", "") ?: ""
        set(v) { prefs.edit().putString("raUsername", v).apply() }

    /** The RA login token — persisted (NEVER the password) for a silent re-login. */
    var raToken: String
        get() = prefs.getString("raToken", "") ?: ""
        set(v) { prefs.edit().putString("raToken", v).apply() }

    // Per-screen-mode (cover / inner / cast) controller size + opacity (item 5).
    // Each mode keeps its own values, so the controller is right on the narrow
    // cover screen, the large inner screen, and while casting.
    // Coerce persisted values into the slider ranges — out-of-range prefs (corrupt,
    // or a future range change) would otherwise make Material's Slider throw.
    private val scaleStates = ScreenMode.entries.associateWith {
        mutableFloatStateOf(prefs.getFloat("ctrlScale_${it.name}", 1.0f).coerceIn(0.25f, 1.1f))
    }
    private val opacityStates = ScreenMode.entries.associateWith {
        mutableFloatStateOf(prefs.getFloat("ctrlOpacity_${it.name}", 1.0f).coerceIn(0.4f, 1.0f))
    }

    fun controllerScale(mode: ScreenMode): Float = scaleStates.getValue(mode).floatValue
    fun setControllerScale(mode: ScreenMode, v: Float) {
        scaleStates.getValue(mode).floatValue = v
        prefs.edit().putFloat("ctrlScale_${mode.name}", v).apply()
    }

    fun controllerOpacity(mode: ScreenMode): Float = opacityStates.getValue(mode).floatValue
    fun setControllerOpacity(mode: ScreenMode, v: Float) {
        opacityStates.getValue(mode).floatValue = v
        prefs.edit().putFloat("ctrlOpacity_${mode.name}", v).apply()
    }

    // Per-filter shader params (v1.8.4) — tuned via the sliders that appear for the
    // selected filter on the GPU renderer. Defaults match the phone-tuned look.
    private fun floatState(key: String, default: Float) = mutableFloatStateOf(prefs.getFloat(key, default))
    private val _scanInt = floatState("scanInt", 0.5f)
    private val _scanRows = floatState("scanRows", 240f)
    private val _aperture = floatState("aperture", 0.10f)
    private val _ntscSat = floatState("ntscSat", 0.55f)
    private val _ntscSharp = floatState("ntscSharp", 0.08f)
    private val _ntscTint = floatState("ntscTint", 0f)
    private val _ntscPhase = floatState("ntscPhase", 0f)

    private fun putFloat(key: String, state: androidx.compose.runtime.MutableFloatState, v: Float) {
        state.floatValue = v
        prefs.edit().putFloat(key, v).apply()
    }

    var scanlineIntensity: Float
        get() = _scanInt.floatValue
        set(v) = putFloat("scanInt", _scanInt, v)
    var scanlineRows: Float
        get() = _scanRows.floatValue
        set(v) = putFloat("scanRows", _scanRows, v)
    var apertureMask: Float
        get() = _aperture.floatValue
        set(v) = putFloat("aperture", _aperture, v)
    var ntscSaturation: Float
        get() = _ntscSat.floatValue
        set(v) = putFloat("ntscSat", _ntscSat, v)
    var ntscSharpness: Float
        get() = _ntscSharp.floatValue
        set(v) = putFloat("ntscSharp", _ntscSharp, v)
    var ntscTint: Float
        get() = _ntscTint.floatValue
        set(v) = putFloat("ntscTint", _ntscTint, v)
    var ntscPhase: Float
        get() = _ntscPhase.floatValue
        set(v) = putFloat("ntscPhase", _ntscPhase, v)

    /** The four shader params for [filter], in the order the native renderer wants:
     *  Scanlines [intensity, _, rows], CRT [intensity, mask, rows], NTSC
     *  [saturation, sharpness, tint, phase], None all-zero. */
    fun filterParams(filter: VideoFilter): FloatArray = when (filter) {
        VideoFilter.Scanlines -> floatArrayOf(scanlineIntensity, 0f, scanlineRows, 0f)
        VideoFilter.Crt -> floatArrayOf(scanlineIntensity, apertureMask, scanlineRows, 0f)
        VideoFilter.Ntsc -> floatArrayOf(ntscSaturation, ntscSharpness, ntscTint, ntscPhase)
        // Bisqwit: the picture knobs (contrast/sat/bright/hue) ride in `aux`; neutral
        // (all 0) is byte-identical to the desktop default. videoPhase is per-frame.
        VideoFilter.Bisqwit -> floatArrayOf(0f, 0f, 0f, 0f)
        VideoFilter.None -> floatArrayOf(0f, 0f, 0f, 0f)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsSheet(
    settings: AppSettings,
    mode: ScreenMode,
    onLoadPalette: () -> Unit = {},
    onClearPalette: () -> Unit = {},
    onMovieRecord: () -> Unit = {},
    onMoviePlay: () -> Unit = {},
    onMovieSave: () -> Unit = {},
    onMovieStop: () -> Unit = {},
    onLoadHdpack: () -> Unit = {},
    onUnloadHdpack: () -> Unit = {},
    onLoadScript: () -> Unit = {},
    onUnloadScript: () -> Unit = {},
    raStatus: String = "Logged out",
    raUser: String? = null,
    onRaLogin: (String, String) -> Unit = { _, _ -> },
    onRaLogout: () -> Unit = {},
    raEnabled: Boolean = false,
    onRaEnabledChange: (Boolean) -> Unit = {},
    raHardcore: Boolean = false,
    onRaHardcoreChange: (Boolean) -> Unit = {},
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    val vibrator = remember { systemVibrator(context) }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Settings")

            // Theme (Light / Dark / System).
            Text("Theme")
            Row(
                modifier = Modifier.fillMaxWidth().selectableGroup(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ThemeMode.entries.forEach { t ->
                    FilterChip(
                        selected = settings.themeMode == t,
                        onClick = { settings.themeMode = t },
                        label = { Text(t.label) },
                    )
                }
            }

            // Video filter (replaces the old FX cycle button).
            if (videoFiltersSupported) {
                Text("Video filter")
                Row(
                    modifier = Modifier.fillMaxWidth().selectableGroup(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    VideoFilter.entries.forEach { f ->
                        FilterChip(
                            selected = settings.filter == f,
                            onClick = { settings.filter = f },
                            label = { Text(f.label) },
                        )
                    }
                }

                // Tuning sliders for the GPU renderer — ONLY the ones for the
                // selected filter (None shows none). They drive the native shader's
                // params live; the AGSL/Bitmap path uses its own fixed look.
                if (settings.useGpuRenderer) {
                    when (settings.filter) {
                        VideoFilter.None -> {}
                        VideoFilter.Scanlines -> {
                            ParamSlider("Scanline intensity", settings.scanlineIntensity, 0f..1f) { settings.scanlineIntensity = it }
                            ParamSlider("Scanline count", settings.scanlineRows, 120f..480f, "%.0f") { settings.scanlineRows = it }
                        }
                        VideoFilter.Crt -> {
                            ParamSlider("Scanline intensity", settings.scanlineIntensity, 0f..1f) { settings.scanlineIntensity = it }
                            ParamSlider("Scanline count", settings.scanlineRows, 120f..480f, "%.0f") { settings.scanlineRows = it }
                            ParamSlider("Aperture mask", settings.apertureMask, 0f..0.5f) { settings.apertureMask = it }
                        }
                        VideoFilter.Ntsc -> {
                            ParamSlider("Saturation", settings.ntscSaturation, 0f..2f) { settings.ntscSaturation = it }
                            ParamSlider("Sharpness", settings.ntscSharpness, 0f..1f) { settings.ntscSharpness = it }
                            ParamSlider("Tint", settings.ntscTint, -0.5f..0.5f) { settings.ntscTint = it }
                            ParamSlider("Phase", settings.ntscPhase, 0f..1f) { settings.ntscPhase = it }
                        }
                        // Bisqwit runs at its neutral picture knobs (matching the
                        // desktop default); no sliders for now.
                        VideoFilter.Bisqwit -> {}
                    }
                }
            }

            // Custom NES palette (v1.8.5) — load a .pal file (a 192-byte RGB table)
            // applied live to the running core, or reset to the built-in palette.
            Text("Custom palette")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onLoadPalette) { Text("Load .pal…") }
                TextButton(onClick = onClearPalette) { Text("Reset") }
            }

            // TAS movie (v1.8.5) — record/play deterministic .rnm movies. Record
            // power-cycles; Stop & save writes the .rnm; Play seeks to its start.
            Text("TAS movie (.rnm)")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onMovieRecord) { Text("Record") }
                TextButton(onClick = onMovieSave) { Text("Stop & save…") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onMoviePlay) { Text("Play…") }
                TextButton(onClick = onMovieStop) { Text("Stop") }
            }

            // HD-pack (v1.8.5) — load a Mesen-style .zip pack (hires.txt + PNG tiles).
            // While active the picture upscales through the Bitmap path (the GPU
            // renderer is bypassed, since its texture is fixed at 256x240).
            Text("HD-pack")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onLoadHdpack) { Text("Load .zip…") }
                TextButton(onClick = onUnloadHdpack) { Text("Unload") }
            }

            // Lua scripting (v1.8.6) — load a sandboxed `.lua` script (per-frame
            // callback, gated writes, no io/os/net); its print output shows on-screen.
            Text("Lua script")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onLoadScript) { Text("Load .lua…") }
                TextButton(onClick = onUnloadScript) { Text("Unload") }
            }

            // RetroAchievements (v1.8.6) — opt-in login + hardcore toggle. The token
            // (never the password) is persisted for a silent re-login. While logged in
            // the user + score are shown with a Log out button; otherwise the login
            // fields. Hardcore disables save-states/rewind while a game is identified.
            Text("RetroAchievements")
            ToggleRow("Enable RetroAchievements", raEnabled, onRaEnabledChange)
            if (raEnabled) {
                if (raUser != null) {
                    Text("Signed in as $raUser")
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(onClick = onRaLogout) { Text("Log out") }
                    }
                } else {
                    var raName by remember { mutableStateOf("") }
                    var raPass by remember { mutableStateOf("") }
                    OutlinedTextField(
                        value = raName,
                        onValueChange = { raName = it },
                        label = { Text("Username") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    OutlinedTextField(
                        value = raPass,
                        onValueChange = { raPass = it },
                        label = { Text("Password") },
                        singleLine = true,
                        visualTransformation = PasswordVisualTransformation(),
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Button(
                        onClick = { onRaLogin(raName.trim(), raPass) },
                        enabled = raName.isNotBlank() && raPass.isNotEmpty(),
                    ) { Text("Log in") }
                }
                ToggleRow("Hardcore mode", raHardcore, onRaHardcoreChange)
                Text(raStatus)
            }

            ToggleRow("Mute audio", settings.muted) { settings.muted = it }

            // Native wgpu SurfaceView renderer (v1.8.4, API 33+). Applies on restart.
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
                ToggleRow("GPU renderer (restart to apply)", settings.useGpuRenderer) {
                    settings.useGpuRenderer = it
                }
            }

            // Haptic intensity (Off / Low / Medium / High).
            Text("Haptics")
            Row(
                modifier = Modifier.fillMaxWidth().selectableGroup(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                HapticLevel.entries.forEach { level ->
                    FilterChip(
                        selected = settings.hapticLevel == level,
                        onClick = { settings.hapticLevel = level },
                        label = { Text(level.label) },
                    )
                }
            }

            // Per-screen-mode controller size + opacity (item 5). The active mode
            // is shown so it's clear which screen these apply to.
            Text("Controller — ${mode.label} screen")
            ControllerSizeSlider(settings.controllerScale(mode), mode, vibrator, settings.hapticLevel) {
                settings.setControllerScale(mode, it)
            }
            LabeledSlider(
                "Controller opacity (${mode.label})",
                settings.controllerOpacity(mode),
                0.4f..1.0f,
                { settings.setControllerOpacity(mode, it) },
            )
        }
    }
}

@Composable
private fun ToggleRow(label: String, value: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label)
        Switch(checked = value, onCheckedChange = onChange)
    }
}

@Composable
private fun LabeledSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    onChange: (Float) -> Unit,
) {
    Column {
        Text("$label  ${"%.0f".format(value * 100)}%")
        Slider(value = value, onValueChange = onChange, valueRange = range)
    }
}

/** A raw-value slider for a shader param (shows the value with [format], not a %). */
@Composable
private fun ParamSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    format: String = "%.2f",
    onChange: (Float) -> Unit,
) {
    Column {
        Text("$label  ${format.format(value)}")
        Slider(value = value, onValueChange = onChange, valueRange = range)
    }
}

/** The 25-110% snap points for the controller-size slider. */
private val SIZE_TICKS = listOf(0.25f, 0.5f, 0.75f, 1.0f, 1.1f)

/**
 * Controller-size slider: a continuous 25-110% drag (the controller overruns the
 * screen edges past 100%) with a haptic tick each time the value crosses a snap
 * point, plus a quick-tap row that snaps to 25/50/75/100/110%.
 */
@Composable
private fun ControllerSizeSlider(
    value: Float,
    mode: ScreenMode,
    vibrator: android.os.Vibrator?,
    haptic: HapticLevel,
    onChange: (Float) -> Unit,
) {
    Column {
        Text("Controller size (${mode.label})  ${(value * 100).roundToInt()}%")
        Slider(
            value = value,
            onValueChange = { nv ->
                if (SIZE_TICKS.any { (value < it) != (nv < it) }) tick(vibrator, haptic)
                onChange(nv)
            },
            valueRange = 0.25f..1.1f,
        )
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            SIZE_TICKS.forEach { t ->
                TextButton(onClick = { tick(vibrator, haptic); onChange(t) }) {
                    Text("${(t * 100).roundToInt()}%")
                }
            }
        }
    }
}
