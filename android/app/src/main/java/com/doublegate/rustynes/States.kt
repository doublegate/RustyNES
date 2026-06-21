package com.doublegate.rustynes

import android.content.Context
import android.text.format.DateUtils
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/**
 * The save-state manager (v1.8.3): explicit numbered slots keyed by ROM SHA, each
 * with its last-saved time and Save / Load / Delete. Replaces the single Save/Load
 * pair. The `auto` resume slot is managed separately (save-on-background).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StatesSheet(
    context: Context,
    sha: String?,
    emulator: EmulatorHandle,
    onStatus: (String) -> Unit,
    // v1.8.8 "Atlas" (Workstreams D+E): notified with the slot id after a successful
    // user save, so the host can fire the "first save-state" PGS achievement and push
    // the slot to the cloud (both no-ops behind their default-off gates).
    onSlotSaved: (String) -> Unit = {},
    onDismiss: () -> Unit,
) {
    // Bump to recompute slot timestamps after a save/delete.
    var refresh by remember { mutableIntStateOf(0) }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("Save states")
            if (sha == null) {
                Text("Load a ROM to use save states.")
            } else {
                SaveStateStore.USER_SLOTS.forEach { slot ->
                    val ts = remember(refresh, slot, sha) {
                        SaveStateStore.lastModified(context, sha, slot)
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        // THM thumbnail beside the slot (v1.8.8 WS C), if one was
                        // captured. TODO(WS C): wire live-framebuffer capture at Save
                        // time (the emulation loop owns the pixel buffer) via
                        // SaveStateStore.saveThumb so this populates for new saves.
                        val thumb = remember(refresh, slot, sha) {
                            SaveStateStore.thumbFile(context, sha, slot).takeIf { it.exists() }
                        }
                        if (thumb != null) {
                            coil3.compose.AsyncImage(
                                model = thumb,
                                contentDescription = "Slot $slot thumbnail",
                                modifier = Modifier.size(48.dp, 45.dp),
                            )
                        }
                        Column(modifier = Modifier.weight(1f)) {
                            Text("Slot $slot")
                            Text(
                                if (ts > 0L) {
                                    DateUtils.getRelativeTimeSpanString(
                                        ts, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS,
                                    ).toString()
                                } else {
                                    "Empty"
                                },
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        OutlinedButton(onClick = {
                            val ctrl = emulator.controller
                            if (ctrl != null) {
                                runCatching { SaveStateStore.save(context, sha, slot, ctrl.saveState()) }
                                    .onSuccess { onStatus("Saved slot $slot"); refresh++; onSlotSaved(slot) }
                                    .onFailure { onStatus("Save failed: ${it.message}") }
                            }
                        }) { Text("Save") }
                        OutlinedButton(
                            enabled = ts > 0L,
                            onClick = {
                                val ctrl = emulator.controller
                                // The read itself can throw (I/O error); keep it in
                                // runCatching so a failure reports a status, not a crash.
                                runCatching {
                                    val blob = SaveStateStore.load(context, sha, slot)
                                    if (ctrl != null && blob != null) ctrl.loadState(blob)
                                    blob != null
                                }
                                    .onSuccess { ok ->
                                        if (ok == true) { onStatus("Loaded slot $slot"); onDismiss() }
                                    }
                                    .onFailure { onStatus("Load failed: ${it.message}") }
                            },
                        ) { Text("Load") }
                        TextButton(
                            enabled = ts > 0L,
                            onClick = {
                                SaveStateStore.delete(context, sha, slot)
                                onStatus("Deleted slot $slot"); refresh++
                            },
                        ) { Text("Delete") }
                    }
                }
            }
        }
    }
}
