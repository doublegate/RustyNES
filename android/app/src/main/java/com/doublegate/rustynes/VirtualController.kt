package com.doublegate.rustynes

import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.changedToDown
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import kotlin.math.hypot

/**
 * The on-screen virtual NES controller (Workstream v1.8.2).
 *
 * A single [Canvas] both **draws** an NES-001 control pad (styled per the controller
 * in `assets/RustyNES_Icon/rustynes.svg` — the `nes_controller()` geometry/palette)
 * and **collects multi-touch input** through one container-level pointer loop:
 * every pointer event recomputes the entire pressed-button set from *all* currently-
 * down pointers and hands it to [EmulatorHandle.setTouchMask]. That gives
 * simultaneous presses, D-pad diagonals (overlapping corner regions), and
 * slide-between-buttons for free — replacing the old per-button `detectTapGestures`
 * that only registered one input at a time.
 *
 * Because the drawn art **and** the hit regions both derive from the same measured
 * `size`, a resize (e.g. the Z Fold 7 cover↔inner fold) rescales and remaps them in
 * lockstep — they can never desync. The parent sizes this composable per posture.
 *
 * Determinism is unaffected: the mask flows through the existing late-latch path.
 */
@Composable
fun VirtualController(
    emulator: EmulatorHandle,
    hapticLevel: HapticLevel,
    onLogoTap: () -> Unit,
    modifier: Modifier,
) {
    // The live pressed-button mask, used both to drive input and to light the art.
    var mask by remember { mutableIntStateOf(0) }
    val context = LocalContext.current
    val vibrator = remember { systemVibrator(context) }
    // Remembered so the draw pass allocates no Paint (its textSize is set per use).
    val labelPaint = remember {
        android.graphics.Paint().apply {
            color = android.graphics.Color.parseColor("#E60012")
            textAlign = android.graphics.Paint.Align.CENTER
            isFakeBoldText = true
            isAntiAlias = true
        }
    }
    // The "RustyNES" wordmark uses the icon's Press Start 2P face + Nintendo red.
    val wordmarkPaint = remember {
        android.graphics.Paint().apply {
            color = android.graphics.Color.parseColor("#E60012")
            textAlign = android.graphics.Paint.Align.CENTER
            isAntiAlias = true
            typeface = androidx.core.content.res.ResourcesCompat.getFont(context, R.font.press_start_2p)
        }
    }
    Canvas(
        modifier = modifier.pointerInput(Unit) {
            // try/finally so a cancelled gesture (parent intercept, focus loss,
            // disposal) always clears the mask — otherwise the last-pressed
            // buttons stay stuck in the emulator until the next touch.
            try {
            awaitPointerEventScope {
                // Track every active pointer by id, so arbitrarily many fingers
                // (e.g. D-pad + B + A at once in SMB) are all live — recompute the
                // mask from the FULL set each event, not just this event's changes.
                val active = HashMap<PointerId, Offset>()
                while (true) {
                    val event = awaitPointerEvent()
                    val w = size.width.toFloat()
                    val h = size.height.toFloat()
                    val pill = logoPillRect(w, h)
                    for (change in event.changes) {
                        // The red "RustyNES" pill is a tap target (toggle the menu),
                        // not an NES button — fire on its press and keep it out of
                        // the button set.
                        val inPill = pill.contains(change.position)
                        if (inPill && change.changedToDown()) onLogoTap()
                        if (change.pressed && !inPill) {
                            active[change.id] = change.position
                        } else {
                            active.remove(change.id)
                        }
                        change.consume()
                    }
                    var m = 0
                    for (pos in active.values) m = m or hitTest(pos.x, pos.y, w, h)
                    if (m != mask) {
                        // Light tick when a new button engages (not on release).
                        if (m and mask.inv() != 0) tick(vibrator, hapticLevel)
                        mask = m
                        emulator.setTouchMask(m)
                    }
                }
            }
            } finally {
                mask = 0
                emulator.setTouchMask(0)
            }
        },
    ) {
        drawNesController(size.width, size.height, mask, labelPaint, wordmarkPaint)
    }
}

// --- haptics (system Vibrator — reliable where Compose's TextHandleMove tick is
//     too subtle / gated on Samsung) ---

fun systemVibrator(context: Context): Vibrator? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        (context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
    }

/** A short button-press tick at the chosen intensity (shared with the size slider). */
fun tick(vibrator: Vibrator?, level: HapticLevel) {
    if (level == HapticLevel.Off) return
    val v = vibrator ?: return
    if (!v.hasVibrator()) return
    val effect = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        val predef = when (level) {
            HapticLevel.Low -> VibrationEffect.EFFECT_TICK
            HapticLevel.High -> VibrationEffect.EFFECT_HEAVY_CLICK
            else -> VibrationEffect.EFFECT_CLICK
        }
        VibrationEffect.createPredefined(predef)
    } else {
        @Suppress("DEPRECATION")
        val ms = when (level) {
            HapticLevel.Low -> 10L
            HapticLevel.High -> 30L
            else -> 20L
        }
        VibrationEffect.createOneShot(ms, VibrationEffect.DEFAULT_AMPLITUDE)
    }
    runCatching { v.vibrate(effect) }
}

// --- hit testing (regions derived from the same fractional geometry as the art) ---

private fun hitTest(px: Float, py: Float, w: Float, h: Float): Int {
    var m = 0
    val cy = h / 2f

    // D-pad: a square active area around the cross; direction from the offset, with
    // a small deadzone so a finger toward a corner registers a diagonal (two bits).
    val dCx = 0.205f * w
    val dCy = cy + 0.07f * h
    val dHalf = 0.31f * h
    if (kotlin.math.abs(px - dCx) < dHalf && kotlin.math.abs(py - dCy) < dHalf) {
        val dz = 0.07f * h
        val dx = px - dCx
        val dy = py - dCy
        if (dy < -dz) m = m or NesBit.UP
        if (dy > dz) m = m or NesBit.DOWN
        if (dx < -dz) m = m or NesBit.LEFT
        if (dx > dz) m = m or NesBit.RIGHT
    }

    // A / B: circles (with touch slop). NES layout: B left, A right.
    val abx = 0.805f * w
    val aby = cy + 0.10f * h
    val br = 0.118f * h * 1.30f
    if (hypot(px - (abx + 0.068f * w), py - aby) < br) m = m or NesBit.A
    if (hypot(px - (abx - 0.068f * w), py - aby) < br) m = m or NesBit.B

    // Select / Start: rounded rects (generous hit area for the small pills).
    val ssx = 0.485f * w
    val ssy = cy + 0.13f * h
    val sHw = 0.058f * w
    val sHh = 0.075f * h
    if (kotlin.math.abs(px - (ssx - 0.064f * w)) < sHw && kotlin.math.abs(py - ssy) < sHh) m = m or NesBit.SELECT
    if (kotlin.math.abs(px - (ssx + 0.064f * w)) < sHw && kotlin.math.abs(py - ssy) < sHh) m = m or NesBit.START

    return m
}

// --- drawing (palette + geometry from make_icon.py's nes_controller) ---

private val BODY = Color(0xFFCFCDC6)
private val BODY_EDGE = Color(0xFF6F6E69)
private val PLATE = Color(0xFF131315) // near-black central face (authentic NES)
private val PLATE_EDGE = Color(0xFF000000)
private val DPAD_WELL = Color(0xFF27272A)
private val DPAD = Color(0xFF161618)
private val DPAD_HUB = Color(0xFF26282D)
private val HOUSING = Color(0xFFC6C4BF)
private val HOUSING_EDGE = Color(0xFF7D7C77)
private val PILL = Color(0xFF2E3138)
private val BTN_WELL = Color(0xFF26282C)
private val BTN_RED = Color(0xFF9A1C1C)
private val BTN_RED_DISH = Color(0xFF6E1212) // concave centre of the A/B buttons
private val BTN_RED_RIM = Color(0x33FFFFFF) // subtle top rim highlight
private val DPAD_ARROW = Color(0xFF3A3D44) // inset directional arrows on the cross
private val DPAD_DIVOT = Color(0xFF101012) // recessed centre dish
private val STRIP_BLACK = Color(0xFF111113) // black inset behind the RustyNES label
private val RED = Color(0xFFE60012)
private val LIT = Color(0x66FFFFFF) // pressed-state highlight overlay

private fun DrawScope.drawNesController(
    w: Float,
    h: Float,
    mask: Int,
    label: android.graphics.Paint,
    wordmark: android.graphics.Paint,
) {
    val cy = h / 2f
    fun rr(r: Float) = androidx.compose.ui.geometry.CornerRadius(r, r)

    // Body + edge.
    drawRoundRect(BODY, Offset(0f, 0f), Size(w, h), rr(0.14f * h))
    drawRoundRect(BODY_EDGE, Offset(0f, 0f), Size(w, h), rr(0.14f * h), style = Stroke(0.012f * h))
    // Dark charcoal face plate.
    drawRoundRect(PLATE, Offset(0.045f * w, 0.11f * h), Size(0.91f * w, 0.78f * h), rr(0.07f * h))
    drawRoundRect(PLATE_EDGE, Offset(0.045f * w, 0.11f * h), Size(0.91f * w, 0.78f * h), rr(0.07f * h), style = Stroke(0.012f * h))

    // D-pad: recessed well + black cross + hub, with per-arm lit overlays.
    val dCx = 0.205f * w
    val dCy = cy + 0.07f * h
    val daL = 0.245f * h // arm half-length
    val daT = 0.088f * h // arm half-thickness
    drawRoundRect(DPAD_WELL, Offset(dCx - 0.30f * h, dCy - 0.30f * h), Size(0.60f * h, 0.60f * h), rr(0.10f * h))
    // vertical + horizontal bars
    drawRoundRect(DPAD, Offset(dCx - daT, dCy - daL), Size(2 * daT, 2 * daL), rr(0.03f * h))
    drawRoundRect(DPAD, Offset(dCx - daL, dCy - daT), Size(2 * daL, 2 * daT), rr(0.03f * h))
    drawCircle(DPAD_HUB, 0.105f * h, Offset(dCx, dCy))
    // Inset directional arrows on each arm + a recessed centre divot (NES detail).
    val aH = 0.05f * h
    val aW = 0.05f * h
    val ar = daL * 0.64f
    fun arrow(tip: Offset, b1: Offset, b2: Offset) {
        drawPath(
            Path().apply { moveTo(tip.x, tip.y); lineTo(b1.x, b1.y); lineTo(b2.x, b2.y); close() },
            DPAD_ARROW,
        )
    }
    arrow(Offset(dCx, dCy - ar), Offset(dCx - aW, dCy - ar + aH), Offset(dCx + aW, dCy - ar + aH))
    arrow(Offset(dCx, dCy + ar), Offset(dCx - aW, dCy + ar - aH), Offset(dCx + aW, dCy + ar - aH))
    arrow(Offset(dCx - ar, dCy), Offset(dCx - ar + aH, dCy - aW), Offset(dCx - ar + aH, dCy + aW))
    arrow(Offset(dCx + ar, dCy), Offset(dCx + ar - aH, dCy - aW), Offset(dCx + ar - aH, dCy + aW))
    drawCircle(DPAD_DIVOT, 0.058f * h, Offset(dCx, dCy))
    drawCircle(DPAD_ARROW, 0.058f * h, Offset(dCx, dCy), style = Stroke(0.006f * h))
    // lit arms
    if (mask and NesBit.UP != 0) drawRoundRect(LIT, Offset(dCx - daT, dCy - daL), Size(2 * daT, daL), rr(0.03f * h))
    if (mask and NesBit.DOWN != 0) drawRoundRect(LIT, Offset(dCx - daT, dCy), Size(2 * daT, daL), rr(0.03f * h))
    if (mask and NesBit.LEFT != 0) drawRoundRect(LIT, Offset(dCx - daL, dCy - daT), Size(daL, 2 * daT), rr(0.03f * h))
    if (mask and NesBit.RIGHT != 0) drawRoundRect(LIT, Offset(dCx, dCy - daT), Size(daL, 2 * daT), rr(0.03f * h))

    // Select / Start housing + pills + labels.
    val ssx = 0.485f * w
    val ssy = cy + 0.13f * h
    drawRoundRect(HOUSING, Offset(ssx - 0.125f * w, ssy - 0.10f * h), Size(0.25f * w, 0.20f * h), rr(0.10f * h))
    drawRoundRect(HOUSING_EDGE, Offset(ssx - 0.125f * w, ssy - 0.10f * h), Size(0.25f * w, 0.20f * h), rr(0.10f * h), style = Stroke(0.008f * h))
    val pw = 0.092f * w
    val ph = 0.066f * h
    // Inline (no per-frame listOf/Pair allocation on this 60fps draw path).
    val selBx = ssx - 0.064f * w
    drawRoundRect(PILL, Offset(selBx - pw / 2, ssy - ph / 2), Size(pw, ph), rr(ph / 2))
    if (mask and NesBit.SELECT != 0) drawRoundRect(LIT, Offset(selBx - pw / 2, ssy - ph / 2), Size(pw, ph), rr(ph / 2))
    val staBx = ssx + 0.064f * w
    drawRoundRect(PILL, Offset(staBx - pw / 2, ssy - ph / 2), Size(pw, ph), rr(ph / 2))
    if (mask and NesBit.START != 0) drawRoundRect(LIT, Offset(staBx - pw / 2, ssy - ph / 2), Size(pw, ph), rr(ph / 2))

    // "RustyNES" brand label — a grey NES-style panel (the width of the SELECT/START
    // housing) inset into a black recess on the dark face, evoking the console's
    // "Entertainment System" strip. The red wordmark is drawn into it in the labels
    // block below. No "Nintendo" branding anywhere.
    val stripW = 0.25f * w
    val stripH = 0.11f * h
    val stripL = ssx - stripW / 2
    val stripT = 0.275f * h - stripH / 2
    drawRoundRect(
        STRIP_BLACK, Offset(stripL - 0.008f * w, stripT - 0.006f * h),
        Size(stripW + 0.016f * w, stripH + 0.012f * h), rr(stripH / 2 + 0.006f * h),
    )
    drawRoundRect(HOUSING, Offset(stripL, stripT), Size(stripW, stripH), rr(stripH / 2))
    drawRoundRect(HOUSING_EDGE, Offset(stripL, stripT), Size(stripW, stripH), rr(stripH / 2), style = Stroke(0.006f * h))

    // Red racetrack logo capsule (upper-right of the plate) carrying a "RustyNES"
    // wordmark (drawn in the labels block below); it doubles as the menu toggle.
    val pill = logoPillRect(w, h)
    drawRoundRect(
        RED, Offset(pill.left, pill.top), Size(pill.width, pill.height),
        rr(pill.height / 2), style = Stroke(0.018f * h),
    )

    // A / B: housing + dark wells + concave red buttons + lit overlay.
    val abx = 0.805f * w
    val aby = cy + 0.10f * h
    val br = 0.118f * h
    drawRoundRect(HOUSING, Offset(abx - 0.155f * w, aby - 0.16f * h), Size(0.31f * w, 0.32f * h), rr(0.10f * h))
    drawRoundRect(HOUSING_EDGE, Offset(abx - 0.155f * w, aby - 0.16f * h), Size(0.31f * w, 0.32f * h), rr(0.10f * h), style = Stroke(0.008f * h))
    // Each button: dark well, red top, a darker concave dish (NES A/B are concave),
    // and a faint top rim highlight for the bevel.
    val bBx = abx - 0.068f * w
    drawCircle(BTN_WELL, br + 0.025f * h, Offset(bBx, aby))
    drawCircle(BTN_RED, br, Offset(bBx, aby))
    drawCircle(BTN_RED_DISH, br * 0.72f, Offset(bBx, aby))
    drawCircle(BTN_RED_RIM, br * 0.92f, Offset(bBx, aby - 0.01f * h), style = Stroke(0.012f * h))
    if (mask and NesBit.B != 0) drawCircle(LIT, br, Offset(bBx, aby))
    val aBx = abx + 0.068f * w
    drawCircle(BTN_WELL, br + 0.025f * h, Offset(aBx, aby))
    drawCircle(BTN_RED, br, Offset(aBx, aby))
    drawCircle(BTN_RED_DISH, br * 0.72f, Offset(aBx, aby))
    drawCircle(BTN_RED_RIM, br * 0.92f, Offset(aBx, aby - 0.01f * h), style = Stroke(0.012f * h))
    if (mask and NesBit.A != 0) drawCircle(LIT, br, Offset(aBx, aby))

    // Labels (native canvas): SELECT/START above the pills, B/A below the buttons.
    // The Paint is remembered by the caller (allocation-free draw pass).
    drawContext.canvas.nativeCanvas.apply {
        label.textSize = 0.06f * h
        drawText("SELECT", ssx - 0.064f * w, ssy - 0.13f * h, label)
        drawText("START", ssx + 0.064f * w, ssy - 0.13f * h, label)
        // "RustyNES" wordmark (icon's Press Start 2P + Nintendo red) in the grey
        // plate band above the SELECT/START labels, fit to the housing width (0.25w)
        // and centred between the plate top (~0.11h) and the red labels (~0.44h).
        wordmark.textSize = 100f
        val rw = wordmark.measureText("RustyNES")
        wordmark.textSize = minOf(0.21f * w / rw * 100f, 0.058f * h)
        val wfm = wordmark.fontMetrics
        drawText("RustyNES", ssx, 0.275f * h - (wfm.ascent + wfm.descent) / 2f, wordmark)
        label.textSize = 0.11f * h
        drawText("B", abx - 0.068f * w, aby + 0.255f * h, label)
        drawText("A", abx + 0.068f * w, aby + 0.255f * h, label)
        // "M E N U" centered inside the red pill — same red/bold face + size as
        // SELECT/START, with a width guard so it never touches the oval outline.
        label.textSize = 0.06f * h
        val menuW = label.measureText("M E N U")
        if (menuW > pill.width * 0.84f) label.textSize = 0.06f * h * (pill.width * 0.84f / menuW)
        val fm = label.fontMetrics
        drawText("M E N U", pill.center.x, pill.center.y - (fm.ascent + fm.descent) / 2f, label)
    }
}

/** The red racetrack "MENU" pill — shared by the art and the menu-toggle hit-test.
 *  Taller than a logo capsule so the "M E N U" glyphs can match the SELECT/START
 *  size and still clear the oval; kept centered around its original position. */
private fun logoPillRect(w: Float, h: Float): Rect {
    val lw = 0.16f * w
    val lh = 0.11f * h
    val left = 0.805f * w - lw / 2
    val top = 0.2225f * h
    return Rect(left, top, left + lw, top + lh)
}
