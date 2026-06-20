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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipRect
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
    // SELECT/START/B/A/MENU use a bold sans; only "RustyNES" uses the icon's
    // Press Start 2P face. Both are NES red.
    val labelPaint = remember {
        android.graphics.Paint().apply {
            color = android.graphics.Color.parseColor("#CE2018")
            textAlign = android.graphics.Paint.Align.CENTER
            isAntiAlias = true
            // The actual "NES Controller" typeface (SELECT/START/B/A/MENU).
            typeface = androidx.core.content.res.ResourcesCompat.getFont(context, R.font.nes_controller)
        }
    }
    val wordmarkPaint = remember {
        android.graphics.Paint().apply {
            color = android.graphics.Color.parseColor("#CE2018")
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
                // Pointers that went down on the MENU pill are "owned" by it for the
                // whole gesture, so dragging out of the pill never presses a button.
                val pillPointers = HashSet<PointerId>()
                while (true) {
                    val event = awaitPointerEvent()
                    val w = size.width.toFloat()
                    val h = size.height.toFloat()
                    val pill = logoPillRect(w, h)
                    for (change in event.changes) {
                        // The red MENU pill is a tap target (toggle the menu), not an
                        // NES button — fire on its press and claim the pointer.
                        if (change.changedToDown() && pill.contains(change.position)) {
                            onLogoTap()
                            pillPointers.add(change.id)
                        }
                        if (change.pressed && change.id !in pillPointers) {
                            active[change.id] = change.position
                        } else {
                            active.remove(change.id)
                        }
                        if (!change.pressed) pillPointers.remove(change.id)
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

internal fun systemVibrator(context: Context): Vibrator? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        (context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager)?.defaultVibrator
    } else {
        @Suppress("DEPRECATION")
        context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
    }

/** A short button-press tick at the chosen intensity (shared with the size slider). */
internal fun tick(vibrator: Vibrator?, level: HapticLevel) {
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

    // D-pad: a square active area around the cross; direction from the offset, with
    // a small deadzone so a finger toward a corner registers a diagonal (two bits).
    val dCx = DPAD_CX * w
    val dCy = DPAD_CY * h
    val dHalf = 0.225f * h
    if (kotlin.math.abs(px - dCx) < dHalf && kotlin.math.abs(py - dCy) < dHalf) {
        val dz = 0.05f * h
        val dx = px - dCx
        val dy = py - dCy
        if (dy < -dz) m = m or NesBit.UP
        if (dy > dz) m = m or NesBit.DOWN
        if (dx < -dz) m = m or NesBit.LEFT
        if (dx > dz) m = m or NesBit.RIGHT
    }

    // A / B: circles (with touch slop). NES layout: B left, A right.
    val abY = AB_CY * h
    val br = 0.082f * w
    if (hypot(px - AB_AX * w, py - abY) < br) m = m or NesBit.A
    if (hypot(px - AB_BX * w, py - abY) < br) m = m or NesBit.B

    // Select / Start: rounded rects (generous hit area for the small black pills).
    val ssY = SS_CY * h
    val sHw = 0.055f * w
    val sHh = 0.075f * h
    if (kotlin.math.abs(px - SS_SELX * w) < sHw && kotlin.math.abs(py - ssY) < sHh) m = m or NesBit.SELECT
    if (kotlin.math.abs(px - SS_STAX * w) < sHw && kotlin.math.abs(py - ssY) < sHh) m = m or NesBit.START

    return m
}

// --- shared geometry (fractions of w,h), measured from the real NES-004 layout;
//     used by BOTH the art and the hit-test so they can never desync ---
private const val DPAD_CX = 0.165f
private const val DPAD_CY = 0.59f
private const val SS_CX = 0.461f // SELECT/START white housing centre x
private const val SS_CY = 0.708f // ...and centre y
private const val SS_SELX = 0.393f
private const val SS_STAX = 0.530f
private const val SS_LABELY = 0.505f // the red SELECT/START labels (their grey stripe)
private const val AB_CY = 0.707f
private const val AB_BX = 0.703f
private const val AB_AX = 0.831f
private const val AB_LABELY = 0.89f
private const val RUSTY_CY = 0.351f // the grey "RustyNES" stripe, above SELECT/START

// --- drawing (palette + geometry from make_icon.py's nes_controller) ---

// Sampled from the reference images (flat layout #1 + photo #2).
private val BODY = Color(0xFFD3CFC6) // light warm-grey plastic shell
private val BODY_EDGE = Color(0xFF8E8B82)
private val FACE = Color(0xFF141416) // near-black central face
private val CROSS = Color(0xFF141416) // D-pad cross (matches the face)
private val CROSS_OUT = Color(0xFFDEDEDE) // white cross outline
private val ARROW_DK = Color(0xFF000000) // dark directional-arrow outlines on the cross
private val CROSS_FACE = Color(0xFF28282B) // dark-grey cross face: lighter than the black arrows/circle, but darker than before
private val STRIPE = Color(0xFF858585) // grey decorative stripes (centre)
private val HOUSING_W = Color(0xFFDEDEDE) // white SELECT/START + A/B housings
private val HOUSING_E = Color(0xFF9A988F)
private val PILL_BLK = Color(0xFF141416) // black SELECT/START pills
private val BTN_RED = Color(0xFFE81810) // A/B base red
private val BTN_RED_HI = Color(0xFFF8584C) // domed-button highlight (upper-left)
private val BTN_RED_LO = Color(0xFFCC1810) // subtle recessed-centre tint (concave dish)
private val RED = Color(0xFFCE2018) // labels + RustyNES + MENU pill
private val LIT = Color(0x44FFFFFF) // pressed-state highlight overlay

private fun DrawScope.drawNesController(
    w: Float,
    h: Float,
    mask: Int,
    label: android.graphics.Paint,
    wordmark: android.graphics.Paint,
) {
    fun rr(r: Float) = androidx.compose.ui.geometry.CornerRadius(r, r)

    // Body + edge, then the near-black central face. The white-plastic borders are
    // asymmetric like the real shell (measured): thick top, thin bottom, thin sides.
    drawRoundRect(BODY, Offset(0f, 0f), Size(w, h), rr(0.022f * h))
    drawRoundRect(BODY_EDGE, Offset(0f, 0f), Size(w, h), rr(0.022f * h), style = Stroke(0.014f * h))
    val faceT = 0.167f * h
    val faceB = 0.939f * h
    // Sharp (un-rounded) corners where the black face meets the white border.
    drawRect(FACE, Offset(0.027f * w, faceT), Size(0.946f * w, faceB - faceT))

    // Four grey decorative stripes down the centre; the top + bottom ones are
    // truncated where they meet the face/border (clipped to the face). The
    // "RustyNES" stripe is RUSTY_CY, the SELECT/START stripe is SS_LABELY.
    val stL = 0.321f * w
    val stW = 0.28f * w
    clipRect(0.027f * w, faceT, 0.973f * w, faceB) {
        // Bottom stripe at 0.924 so its top clears the SELECT/START housing bottom
        // by the measured 0.046h gap.
        for (cyf in floatArrayOf(0.199f, RUSTY_CY, SS_LABELY, 0.924f)) {
            drawRoundRect(STRIPE, Offset(stL, cyf * h - 0.059f * h), Size(stW, 0.118f * h), rr(0.042f * h))
        }
    }

    // SELECT / START: a WHITE housing with two black pills (red labels drawn later).
    val ssY = SS_CY * h
    val hsW = 0.28f * w
    val hsH = 0.222f * h
    val hsL = SS_CX * w - hsW / 2
    val hsT = ssY - hsH / 2
    drawRoundRect(HOUSING_W, Offset(hsL, hsT), Size(hsW, hsH), rr(0.055f * h))
    drawRoundRect(HOUSING_E, Offset(hsL, hsT), Size(hsW, hsH), rr(0.055f * h), style = Stroke(0.006f * h))
    // Black inset border inside the white housing (the recessed frame on the real
    // controller's SELECT/START plate).
    val ins = 0.016f * w
    drawRoundRect(PILL_BLK, Offset(hsL + ins, hsT + 0.022f * h), Size(hsW - 2 * ins, hsH - 0.044f * h), rr(0.038f * h), style = Stroke(0.007f * h))
    val pw = 0.079f * w
    val ph = 0.072f * h
    val selX = SS_SELX * w
    drawRoundRect(PILL_BLK, Offset(selX - pw / 2, ssY - ph / 2), Size(pw, ph), rr(ph / 2))
    if (mask and NesBit.SELECT != 0) drawRoundRect(LIT, Offset(selX - pw / 2, ssY - ph / 2), Size(pw, ph), rr(ph / 2))
    val staX = SS_STAX * w
    drawRoundRect(PILL_BLK, Offset(staX - pw / 2, ssY - ph / 2), Size(pw, ph), rr(ph / 2))
    if (mask and NesBit.START != 0) drawRoundRect(LIT, Offset(staX - pw / 2, ssY - ph / 2), Size(pw, ph), rr(ph / 2))

    // D-pad: a black cross with a WHITE OUTLINE (draw white bars, then inset black
    // bars), white-outline arrows near the tips pointing outward, and a centre
    // circle. Authentic NES-004 detailing.
    val dCx = DPAD_CX * w
    val dCy = DPAD_CY * h
    val daL = 0.216f * h // half-length incl. outline (measured 0.2277h outer)
    val daT = 0.07f * h // half-thickness (measured arm width 0.0586w)
    val ow = 0.012f * h
    drawRoundRect(CROSS_OUT, Offset(dCx - daT - ow, dCy - daL - ow), Size(2 * (daT + ow), 2 * (daL + ow)), rr(0.025f * h))
    drawRoundRect(CROSS_OUT, Offset(dCx - daL - ow, dCy - daT - ow), Size(2 * (daL + ow), 2 * (daT + ow)), rr(0.025f * h))
    drawRoundRect(CROSS, Offset(dCx - daT, dCy - daL), Size(2 * daT, 2 * daL), rr(0.02f * h))
    drawRoundRect(CROSS, Offset(dCx - daL, dCy - daT), Size(2 * daL, 2 * daT), rr(0.02f * h))
    // Lighter grey cross FACE inset within the black band, so the black arrows +
    // centre circle read against it (matching the reference).
    val fb = 0.018f * h // black-band thickness
    drawRoundRect(CROSS_FACE, Offset(dCx - daT + fb, dCy - daL + fb), Size(2 * (daT - fb), 2 * (daL - fb)), rr(0.014f * h))
    drawRoundRect(CROSS_FACE, Offset(dCx - daL + fb, dCy - daT + fb), Size(2 * (daL - fb), 2 * (daT - fb)), rr(0.014f * h))
    // Dark, head+shaft directional arrows (outline) near each tip, pointing out.
    fun dpadArrow(dxn: Int, dyn: Int) {
        // Compact arrow pushed toward the arm END (tip near the arm tip), same size:
        // a wide triangle head + a short, moderately-wide shaft (~half the head width).
        val dist = daL * 0.64f
        val hw = 0.045f * h // head half-width
        val hh = 0.052f * h // head (triangle) length
        val sw = 0.023f * h // shaft half-width (~half the head)
        val sh = 0.044f * h // shaft length
        fun pt(along: Float, perp: Float) =
            Offset(dCx + dxn * along - dyn * perp, dCy + dyn * along + dxn * perp)
        val tipA = dist + (hh + sh) / 2f
        val p = Path().apply {
            val t = pt(tipA, 0f); moveTo(t.x, t.y)
            val a = pt(tipA - hh, hw); lineTo(a.x, a.y)
            val b = pt(tipA - hh, sw); lineTo(b.x, b.y)
            val c = pt(tipA - hh - sh, sw); lineTo(c.x, c.y)
            val d = pt(tipA - hh - sh, -sw); lineTo(d.x, d.y)
            val e = pt(tipA - hh, -sw); lineTo(e.x, e.y)
            val f = pt(tipA - hh, -hw); lineTo(f.x, f.y)
            close()
        }
        drawPath(p, ARROW_DK, style = Stroke(0.009f * h))
    }
    dpadArrow(0, -1)
    dpadArrow(0, 1)
    dpadArrow(-1, 0)
    dpadArrow(1, 0)
    // Centre circle — hollow (grey face fill) with a black outline.
    drawCircle(CROSS_FACE, 0.05f * h, Offset(dCx, dCy))
    drawCircle(ARROW_DK, 0.05f * h, Offset(dCx, dCy), style = Stroke(0.007f * h))
    // Lit arms.
    if (mask and NesBit.UP != 0) drawRoundRect(LIT, Offset(dCx - daT, dCy - daL), Size(2 * daT, daL), rr(0.018f * h))
    if (mask and NesBit.DOWN != 0) drawRoundRect(LIT, Offset(dCx - daT, dCy), Size(2 * daT, daL), rr(0.018f * h))
    if (mask and NesBit.LEFT != 0) drawRoundRect(LIT, Offset(dCx - daL, dCy - daT), Size(daL, 2 * daT), rr(0.018f * h))
    if (mask and NesBit.RIGHT != 0) drawRoundRect(LIT, Offset(dCx, dCy - daT), Size(daL, 2 * daT), rr(0.018f * h))

    // The red racetrack "MENU" pill — occupies the real controller's wordmark spot
    // (top-right, above A/B) and doubles as the menu toggle.
    val pill = logoPillRect(w, h)
    drawRoundRect(RED, Offset(pill.left, pill.top), Size(pill.width, pill.height), rr(pill.height / 2), style = Stroke(0.016f * h))

    // A / B: two WHITE rounded-square housings, each holding a raised, lit red
    // button — a radial gradient (highlight upper-left through base to a lower-right
    // shadow) gives the domed/concave 3D look from the photo reference.
    val abY = AB_CY * h
    val sqW = 0.112f * w
    val sqH = 0.271f * h
    val br = 0.046f * w // circle dia 0.092w (measured); ~9% margin in the square
    for (bx in floatArrayOf(AB_BX, AB_AX)) {
        val cx = bx * w
        drawRoundRect(HOUSING_W, Offset(cx - sqW / 2, abY - sqH / 2), Size(sqW, sqH), rr(0.035f * h))
        drawRoundRect(HOUSING_E, Offset(cx - sqW / 2, abY - sqH / 2), Size(sqW, sqH), rr(0.035f * h), style = Stroke(0.005f * h))
        // CONCAVE dish: a recessed (darker) centre that lightens toward the rim,
        // with the catch-light pooled on the lower rim (gradient centre nudged up).
        drawCircle(
            brush = Brush.radialGradient(
                colors = listOf(BTN_RED_LO, BTN_RED, BTN_RED_HI),
                center = Offset(cx, abY - br * 0.22f),
                radius = br * 1.18f,
            ),
            radius = br, center = Offset(cx, abY),
        )
        val bit = if (bx == AB_AX) NesBit.A else NesBit.B
        if (mask and bit != 0) drawCircle(LIT, br, Offset(cx, abY))
    }

    // Labels (native canvas) — ALL in the icon's bundled Press Start 2P face (the
    // quintessential NES-era "Nintendo-style" font), NES red. Each is drawn
    // per-character with ~12%-tightened cells so the wide monospace cells don't
    // leave glyph gaps (the s/t pair, etc.), centred on (cx, cy).
    drawContext.canvas.nativeCanvas.apply {
        // Helper: centre a string vertically on cy and draw it (natural layout).
        fun draw(s: String, cx: Float, cy: Float) {
            val fm = label.fontMetrics
            drawText(s, cx, cy - (fm.ascent + fm.descent) / 2f, label)
        }
        // The NES font's caps are 0.439x the textSize, so sizes are ~2.3x the
        // target glyph height (SELECT/START caps 0.038h, B/A 0.0455h).
        label.textSize = 0.087f * h
        draw("SELECT", SS_SELX * w, SS_LABELY * h)
        draw("START", SS_STAX * w, SS_LABELY * h)
        label.textSize = 0.104f * h
        // B/A sit at the bottom-RIGHT of each square (right third), not centred.
        draw("B", AB_BX * w + 0.05f * w, AB_LABELY * h)
        draw("A", AB_AX * w + 0.05f * w, AB_LABELY * h)
        // "M E N U" sized to fill most of the red pill (~75% of the snug fit).
        label.textSize = 100f
        label.textSize = minOf(pill.width * 0.645f / label.measureText("M E N U") * 100f, 0.116f * h)
        draw("M E N U", pill.center.x, pill.center.y)

        // "RustyNES" in Press Start 2P, natural advances — but close ONLY the gap
        // before 't' (index 3), where the s/t cell pair otherwise looks split.
        wordmark.textSize = 100f
        wordmark.textSize = minOf(stW * 0.82f / wordmark.measureText("RustyNES") * 100f, 0.06f * h)
        val adv = wordmark.measureText("RustyNES") / 8f
        val gap = adv * 0.32f
        val totalW = adv * 8f - gap
        val wfm = wordmark.fontMetrics
        val rbY = RUSTY_CY * h - (wfm.ascent + wfm.descent) / 2f
        var rx = SS_CX * w - totalW / 2f + adv / 2f
        "RustyNES".forEachIndexed { i, ch ->
            if (i == 3) rx -= gap
            drawText(ch.toString(), rx, rbY, wordmark)
            rx += adv
        }
    }
}

/** The red racetrack "MENU" pill — shared by the art and the menu-toggle hit-test.
 *  Sits where the real controller's wordmark is (top-right, above the A/B buttons),
 *  sized to match that wordmark and tall enough for the SELECT/START-size glyphs. */
private fun logoPillRect(w: Float, h: Float): Rect {
    val lw = 0.185f * w
    val lh = 0.105f * h
    val cx = (AB_BX + AB_AX) / 2f * w // centred left-right above the A/B squares
    val cy = 0.33f * h // the real controller's wordmark y (measured 0.330)
    return Rect(cx - lw / 2, cy - lh / 2, cx + lw / 2, cy + lh / 2)
}
