package com.doublegate.rustynes

import android.content.Intent
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextDecoration
import androidx.core.net.toUri
import androidx.core.graphics.drawable.toBitmap
import androidx.compose.ui.unit.dp

/**
 * About + first-run onboarding dialogs (v1.8.3, items 8-9). Pure UI; nothing here
 * touches emulation.
 */

private const val ABOUT_TEXT =
    "RustyNES — a cycle-accurate Nintendo Entertainment System emulator written in " +
        "pure Rust.\n\n" +
        "License: MIT OR Apache-2.0\n" +
        "Author: DoubleGate\n" +
        "Accuracy: AccuracyCoin 100% (139/139); nestest 0-diff; blargg / kevtris suites green.\n\n" +
        "Features: 168 mapper families, the Famicom Disk System, Vs. System / " +
        "PlayChoice-10, rollback netplay, RetroAchievements, TAS movies + the TAStudio " +
        "editor, save-states, rewind, run-ahead, Lua scripting + automation, HD packs, " +
        "and A/V recording — all on a strict bit-determinism contract."

/** The About dialog: the RustyNES icon + the desktop project's About text. */
@Composable
fun AboutDialog(onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
        icon = {
            // Render the (adaptive) launcher icon as a bitmap — painterResource
            // throws on an AdaptiveIconDrawable, which was crashing the dialog.
            val ctx = LocalContext.current
            val icon = remember {
                ctx.packageManager.getApplicationIcon(ctx.packageName).toBitmap(160, 160).asImageBitmap()
            }
            Image(
                bitmap = icon,
                contentDescription = "RustyNES icon",
                modifier = Modifier.size(72.dp),
            )
        },
        title = { Text("RustyNES") },
        text = {
            val ctx = LocalContext.current
            Column(modifier = Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState())) {
                Text(ABOUT_TEXT)
                Spacer(Modifier.height(10.dp))
                Text(
                    "github.com/doublegate/RustyNES",
                    color = Color(0xFF4FC3F7),
                    textDecoration = TextDecoration.Underline,
                    modifier = Modifier.clickable {
                        runCatching {
                            ctx.startActivity(
                                Intent(Intent.ACTION_VIEW, "https://github.com/doublegate/RustyNES".toUri())
                                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                            )
                        }
                    },
                )
            }
        },
    )
}

/**
 * The first-run flow: a welcome dialog (intro + the "pill" hint) then a ROM-legal
 * dialog with a "Do not show again" checkbox. Shown until the user ticks the box
 * and closes; [onFinished] hides it for the session and (if ticked) [onSuppress]
 * persists the suppression.
 */
@Composable
fun OnboardingDialogs(onSuppress: () -> Unit, onFinished: () -> Unit) {
    var step by remember { mutableIntStateOf(0) }
    var dontShow by remember { mutableStateOf(false) }
    if (step == 0) {
        AlertDialog(
            onDismissRequest = {}, // advance only via Continue
            confirmButton = { TextButton(onClick = { step = 1 }) { Text("Continue…") } },
            title = { Text("Welcome to RustyNES") },
            text = {
                Text(
                    "A cycle-accurate NES emulator in your pocket.\n\n" +
                        "The controls hide for a clean view — tap the red \"MENU\" pill on the " +
                        "on-screen controller to open the menu bar (Open a ROM, save states, " +
                        "settings, and more). Tap it again to hide it.",
                )
            },
        )
    } else {
        AlertDialog(
            onDismissRequest = {},
            confirmButton = {
                TextButton(onClick = {
                    if (dontShow) onSuppress()
                    onFinished()
                }) { Text("Close") }
            },
            title = { Text("Play responsibly") },
            text = {
                Column {
                    Text(
                        "Only play game ROMs for which you own a physical copy. RustyNES " +
                            "distributes no commercial ROMs of any kind — supplying game files " +
                            "is your responsibility, and yours alone.",
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = dontShow, onCheckedChange = { dontShow = it })
                        Text("Do not show again")
                    }
                }
            },
        )
    }
}
