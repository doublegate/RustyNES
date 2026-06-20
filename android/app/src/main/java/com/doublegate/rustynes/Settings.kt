package com.doublegate.rustynes

import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
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

    private val _controllerScale = mutableFloatStateOf(prefs.getFloat("ctrlScale", 1.0f))
    var controllerScale: Float
        get() = _controllerScale.floatValue
        set(v) { _controllerScale.floatValue = v; prefs.edit().putFloat("ctrlScale", v).apply() }

    private val _controllerOpacity = mutableFloatStateOf(prefs.getFloat("ctrlOpacity", 1.0f))
    var controllerOpacity: Float
        get() = _controllerOpacity.floatValue
        set(v) { _controllerOpacity.floatValue = v; prefs.edit().putFloat("ctrlOpacity", v).apply() }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsSheet(settings: AppSettings, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val vibrator = remember { systemVibrator(context) }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp),
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
            }

            ToggleRow("Mute audio", settings.muted) { settings.muted = it }

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

            ControllerSizeSlider(settings.controllerScale, vibrator, settings.hapticLevel) {
                settings.controllerScale = it
            }
            LabeledSlider(
                "Controller opacity",
                settings.controllerOpacity,
                0.4f..1.0f,
                { settings.controllerOpacity = it },
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
    vibrator: android.os.Vibrator?,
    haptic: HapticLevel,
    onChange: (Float) -> Unit,
) {
    Column {
        Text("Controller size  ${(value * 100).roundToInt()}%")
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
