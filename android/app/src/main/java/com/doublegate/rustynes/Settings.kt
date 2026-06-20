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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

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

            LabeledSlider(
                "Controller size",
                settings.controllerScale,
                0.6f..1.1f,
                { settings.controllerScale = it },
            )
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
