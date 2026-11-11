package com.doublegate.rustynes

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.rustynes_mobile.CheatDbEntry
import uniffi.rustynes_mobile.NesController

/**
 * v3.7.0 (D24, `T-MOBILE-EXTRAS` row 3) — the Game Genie sheet, the Android app's
 * first cheat surface. It lists the codes the desktop's catalog has for the loaded
 * game (`NesController.cheatDatabaseCodes`, the same ~10.8k-code database the
 * desktop's Cheats panel offers, matched on both CRC keys by the bridge), grouped by
 * category, and takes a typed code. Ticking a code enables it through the existing
 * `cheatAddGenie` (which validates it and refuses it in RetroAchievements
 * hardcore); the ticked set is saved per game ([CheatStore]) and re-applied when the
 * game is opened again ([prepareRom]).
 *
 * The core keys active codes by address, so ticking a second code for one address
 * replaces the first: the ticks shown are re-read from `cheatGenieCodes` after every
 * change rather than assumed, and the saved set follows what is actually active.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CheatsSheet(
    controller: NesController?,
    romSha: String?,
    onStatus: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var version by remember { mutableStateOf(0) }
    val listed by produceState(emptyList<CheatDbEntry>(), controller) {
        value = withContext(Dispatchers.IO) {
            controller?.let { c -> runCatching { c.cheatDatabaseCodes() }.getOrDefault(emptyList()) }
                ?: emptyList()
        }
    }
    val game = remember(controller) { controller?.let { runCatching { it.cheatDatabaseGame() }.getOrNull() } }
    val active: Set<String> = remember(controller, version) {
        controller?.let { c -> runCatching { c.cheatGenieCodes().map { it.code }.toSet() }.getOrDefault(emptySet()) }
            ?: emptySet()
    }

    fun persist() {
        val c = controller ?: return
        val sha = romSha ?: return
        val now = runCatching { c.cheatGenieCodes().map { it.code }.toSet() }.getOrDefault(emptySet())
        CheatStore.save(context, sha, now)
        version++
    }

    fun setCode(code: String, on: Boolean) {
        val c = controller ?: return
        if (on) {
            runCatching { c.cheatAddGenie(code) }
                .onFailure { onStatus(context.getString(R.string.cheats_refused, it.message ?: code)) }
        } else {
            c.cheatRemoveGenie(code)
        }
        persist()
    }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(stringResource(R.string.cheats_title), style = MaterialTheme.typography.titleMedium)
            if (controller == null) {
                Text(stringResource(R.string.cheats_no_rom))
                return@Column
            }
            var typed by remember { mutableStateOf("") }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = typed,
                    onValueChange = { typed = it.uppercase().filter { ch -> ch.isLetter() }.take(8) },
                    label = { Text(stringResource(R.string.cheats_typed_label)) },
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                )
                Button(enabled = typed.length == 6 || typed.length == 8, onClick = {
                    setCode(typed, true)
                    typed = ""
                }) { Text(stringResource(R.string.cheats_add)) }
            }
            // The active codes the database does not list (typed ones), so they can be
            // switched off here too.
            val listedCodes = listed.map { it.code }.toSet()
            active.filter { it !in listedCodes }.sorted().forEach { code ->
                CheatRow(code, cheatDatabaseLabel(code), true) { on -> setCode(code, on) }
            }
            if (listed.isEmpty()) {
                Text(stringResource(R.string.cheats_none_listed), style = MaterialTheme.typography.bodySmall)
            } else {
                game?.let { Text(stringResource(R.string.cheats_listed_for, it), style = MaterialTheme.typography.bodySmall) }
                var lastCategory: String? = null
                listed.forEach { e ->
                    if (e.category != lastCategory) {
                        Text(e.category, style = MaterialTheme.typography.labelLarge)
                        lastCategory = e.category
                    }
                    CheatRow(e.code, e.name, e.code in active) { on -> setCode(e.code, on) }
                }
            }
        }
    }
}

/** The database's effect name for a typed code, or nothing. */
private fun cheatDatabaseLabel(code: String): String =
    runCatching { uniffi.rustynes_mobile.cheatDatabaseLookup(code)?.name }.getOrNull() ?: ""

@Composable
private fun CheatRow(code: String, name: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .toggleable(value = on, onValueChange = onChange, role = Role.Checkbox),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Checkbox(checked = on, onCheckedChange = null)
        Column(modifier = Modifier.weight(1f)) {
            Text(name.ifEmpty { code })
            if (name.isNotEmpty()) Text(code, style = MaterialTheme.typography.bodySmall)
        }
    }
}
