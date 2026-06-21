package com.doublegate.rustynes

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Settings → "Controllers" screen (v1.8.7, #37 + #32).
 *
 * Lists every connected hardware pad with its assigned NES port (and a port picker),
 * a per-pad remapping flow (tap a NES button → "press a key / move an axis" → the
 * next input from that device is captured), the global A/B autofire toggle, and a
 * per-pad reset-to-default. State comes live from [GamepadManager] (which re-emits a
 * change signal on hot-plug / reassignment).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ControllersSheet(
    gamepad: GamepadManager,
    settings: AppSettings,
    onDismiss: () -> Unit,
) {
    // Re-read the live pad list whenever the manager signals a change (hot-plug,
    // reassignment, or a remap). A bump counter forces recomposition.
    var bump by remember { mutableStateOf(0) }
    DisposableEffect(gamepad) {
        val l: () -> Unit = { bump++ }
        gamepad.addChangeListener(l)
        onDispose { gamepad.removeChangeListener(l) }
    }
    // `bump` is read so the snapshot recomputes on every change signal.
    val pads = remember(bump) { gamepad.connectedPads() }
    var autofire by remember { mutableStateOf(gamepad.autofireAB) }

    // The active per-pad remap capture, if any: which pad + which NES action we're
    // (re)binding. Non-null shows the capture dialog that grabs the next input.
    var capture by remember { mutableStateOf<CaptureTarget?>(null) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Controllers", fontSize = 18.sp)

            // Global A/B autofire — turns plain A/B into pulsed turbo on every pad.
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Autofire A / B")
                Switch(
                    checked = autofire,
                    onCheckedChange = {
                        autofire = it
                        gamepad.setAutofire(it, settings)
                    },
                )
            }

            Text(
                "Up to four pads are mapped to NES ports in the order they connect. " +
                    "Four Score turns on automatically once three or more are present.",
                style = MaterialTheme.typography.bodySmall,
            )

            if (pads.isEmpty()) {
                Text("No controllers connected.", color = Color.Gray)
            }

            pads.forEach { pad ->
                HorizontalDivider()
                Text("${pad.name}  (P${pad.port + 1})", fontSize = 15.sp)

                // Port picker — reassign this pad to a port (swaps if taken).
                Text("Port", style = MaterialTheme.typography.bodySmall)
                Row(
                    modifier = Modifier.fillMaxWidth().selectableGroup(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    (0..3).forEach { p ->
                        FilterChip(
                            selected = pad.port == p,
                            onClick = { gamepad.assignPort(pad.deviceId, p) },
                            label = { Text("P${p + 1}") },
                        )
                    }
                }

                // Per-NES-button remap rows: show the bound input(s) + a "Rebind"
                // affordance that captures the next key/axis from THIS device.
                val profile = gamepad.profileFor(pad.descriptor)
                val bindings = profile.bindings()
                NES_BUTTON_ORDER.forEach { bit ->
                    val current = bindings.entries
                        .filter { it.value.bit == bit && !it.value.turbo }
                        .map { inputLabel(it.key) }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column {
                            Text(NesAction.label(NesAction.button(bit)))
                            Text(
                                if (current.isEmpty()) "unbound" else current.joinToString(", "),
                                style = MaterialTheme.typography.bodySmall,
                                color = Color.Gray,
                            )
                        }
                        TextButton(onClick = {
                            capture = CaptureTarget(pad.descriptor, NesAction.button(bit))
                        }) { Text("Rebind") }
                    }
                }

                // Turbo A / Turbo B rebind rows (autofire on a dedicated button).
                listOf(NesBit.A, NesBit.B).forEach { bit ->
                    val turboAction = NesAction.turbo(bit)
                    val current = bindings.entries
                        .filter { it.value.bit == bit && it.value.turbo }
                        .map { inputLabel(it.key) }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column {
                            Text(NesAction.label(turboAction))
                            Text(
                                if (current.isEmpty()) "unbound" else current.joinToString(", "),
                                style = MaterialTheme.typography.bodySmall,
                                color = Color.Gray,
                            )
                        }
                        TextButton(onClick = {
                            capture = CaptureTarget(pad.descriptor, turboAction)
                        }) { Text("Rebind") }
                    }
                }

                OutlinedButton(onClick = { gamepad.resetProfile(pad.descriptor, settings) }) {
                    Text("Reset to default")
                }
            }

            Spacer(Modifier.height(8.dp))
            TextButton(onClick = onDismiss) { Text("Done") }
        }
    }

    // Capture overlay: grab the next key/axis from the target device and bind it.
    capture?.let { target ->
        InputCaptureDialog(
            gamepad = gamepad,
            settings = settings,
            target = target,
            onDone = { capture = null },
        )
    }
}

/** Which pad + which NES action a rebind is currently capturing for. */
private data class CaptureTarget(val descriptor: String, val action: NesAction)

/**
 * Modal that captures the next input (a [android.view.KeyEvent] keycode or an analog
 * axis motion past its dead-zone) from the target device and binds it. The capture
 * is driven by [GamepadManager.beginCapture]; tapping outside or "Cancel" aborts.
 */
@Composable
private fun InputCaptureDialog(
    gamepad: GamepadManager,
    settings: AppSettings,
    target: CaptureTarget,
    onDone: () -> Unit,
) {
    DisposableEffect(target) {
        gamepad.beginCapture(target.descriptor) { input ->
            gamepad.remap(target.descriptor, input, target.action, settings)
            onDone()
        }
        onDispose { gamepad.cancelCapture() }
    }
    AlertDialog(
        onDismissRequest = { onDone() },
        title = { Text("Rebind ${NesAction.label(target.action)}") },
        text = {
            Box(modifier = Modifier.fillMaxWidth().padding(8.dp)) {
                Text("Press a button or pull a trigger on the controller…")
            }
        },
        confirmButton = {
            TextButton(onClick = { onDone() }) { Text("Cancel") }
        },
    )
}

/** NES face/d-pad buttons in the order shown in the remap list. */
private val NES_BUTTON_ORDER = listOf(
    NesBit.UP, NesBit.DOWN, NesBit.LEFT, NesBit.RIGHT,
    NesBit.A, NesBit.B, NesBit.SELECT, NesBit.START,
)

/** Human-readable label for a captured input (keycode or a trigger sentinel). */
private fun inputLabel(input: Int): String = when (input) {
    ControllerProfile.AXIS_LTRIGGER -> "L Trigger"
    ControllerProfile.AXIS_RTRIGGER -> "R Trigger"
    else -> android.view.KeyEvent.keyCodeToString(input)
        .removePrefix("KEYCODE_")
        .lowercase()
        .replaceFirstChar { it.uppercase() }
}
