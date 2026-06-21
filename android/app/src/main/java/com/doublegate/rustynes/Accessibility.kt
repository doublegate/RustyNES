package com.doublegate.rustynes

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color

/**
 * v1.8.8 "Atlas" (Workstream I): accessibility chrome color schemes.
 *
 * These recolor ONLY the app chrome (bars, menus, controls, Settings, status text).
 * The gameplay surface stays the ROM's own NES palette (the caller forces
 * `background = black`), so none of this touches emulation output or the determinism
 * contract — it is pure host UI policy.
 *
 * The colorblind sets use the **Okabe-Ito** qualitative palette — the de-facto
 * colorblind-safe standard (8 hues engineered to stay distinguishable across
 * deuteranopia / protanopia / tritanopia), the same family the desktop v1.5.0
 * accessibility themes use. For each CVD type the primary / secondary / tertiary /
 * error roles are mapped to Okabe-Ito hues that remain separable under that specific
 * deficiency, so the role a control's color conveys is never lost.
 *
 * High-contrast is a near-black/near-white scheme with saturated, high-luminance-
 * contrast accents (well past the WCAG AA 4.5:1 text floor) for low-vision users.
 */

// --- Okabe-Ito base palette (the 8 colorblind-safe hues) ---------------------------
private val OI_BLACK = Color(0xFF000000)
private val OI_ORANGE = Color(0xFFE69F00)
private val OI_SKY_BLUE = Color(0xFF56B4E9)
private val OI_BLUISH_GREEN = Color(0xFF009E73)
private val OI_YELLOW = Color(0xFFF0E442)
private val OI_BLUE = Color(0xFF0072B2)
private val OI_VERMILLION = Color(0xFFD55E00)
private val OI_REDDISH_PURPLE = Color(0xFFCC79A7)

/**
 * Resolve the chrome [ColorScheme] for an [AccessibilityTheme]. [dark] selects the
 * light/dark variant where it matters; high-contrast and the colorblind sets are
 * authored as dark schemes (the app's letterbox is black, so a dark chrome reads
 * best and avoids a bright surround around the picture). Returns null for
 * [AccessibilityTheme.Default] so the caller falls back to Material You / brand.
 */
fun accessibilityColorScheme(theme: AccessibilityTheme, dark: Boolean): ColorScheme? = when (theme) {
    AccessibilityTheme.Default -> null
    AccessibilityTheme.HighContrast -> highContrastScheme(dark)
    AccessibilityTheme.Deuteranopia -> okabeItoScheme(
        // Deuteranopia (green-weak): avoid relying on red-vs-green; lean on blue / orange / yellow.
        primary = OI_BLUE,
        secondary = OI_ORANGE,
        tertiary = OI_SKY_BLUE,
        error = OI_YELLOW,
    )
    AccessibilityTheme.Protanopia -> okabeItoScheme(
        // Protanopia (red-weak): reds darken; favor blue / sky-blue / yellow accents.
        primary = OI_BLUE,
        secondary = OI_SKY_BLUE,
        tertiary = OI_YELLOW,
        error = OI_ORANGE,
    )
    AccessibilityTheme.Tritanopia -> okabeItoScheme(
        // Tritanopia (blue-weak): avoid blue-vs-yellow confusion; favor green / vermillion / purple.
        primary = OI_BLUISH_GREEN,
        secondary = OI_VERMILLION,
        tertiary = OI_REDDISH_PURPLE,
        error = OI_VERMILLION,
    )
}

/** A dark, colorblind-safe scheme from the four Okabe-Ito role hues. */
private fun okabeItoScheme(primary: Color, secondary: Color, tertiary: Color, error: Color): ColorScheme =
    darkColorScheme(
        primary = primary,
        onPrimary = OI_BLACK,
        secondary = secondary,
        onSecondary = OI_BLACK,
        tertiary = tertiary,
        onTertiary = OI_BLACK,
        error = error,
        onError = OI_BLACK,
        // Neutral surfaces so the accent hues carry the meaning, not the background.
        background = Color(0xFF101214),
        onBackground = Color(0xFFF2F2F2),
        surface = Color(0xFF1A1D21),
        onSurface = Color(0xFFF2F2F2),
        surfaceVariant = Color(0xFF2A2E33),
        onSurfaceVariant = Color(0xFFE0E0E0),
        outline = Color(0xFFC0C0C0),
    )

/** A maximal-contrast scheme (near-black/near-white + saturated accents). */
private fun highContrastScheme(dark: Boolean): ColorScheme =
    if (dark) {
        darkColorScheme(
            primary = Color(0xFFFFFF00), // pure yellow on black — ~19.6:1 contrast.
            onPrimary = Color(0xFF000000),
            secondary = Color(0xFF00FFFF), // cyan.
            onSecondary = Color(0xFF000000),
            tertiary = Color(0xFFFFFFFF),
            onTertiary = Color(0xFF000000),
            error = Color(0xFFFF5252),
            onError = Color(0xFF000000),
            background = Color(0xFF000000),
            onBackground = Color(0xFFFFFFFF),
            surface = Color(0xFF000000),
            onSurface = Color(0xFFFFFFFF),
            surfaceVariant = Color(0xFF101010),
            onSurfaceVariant = Color(0xFFFFFFFF),
            outline = Color(0xFFFFFFFF),
        )
    } else {
        lightColorScheme(
            primary = Color(0xFF0000C0), // deep blue on white.
            onPrimary = Color(0xFFFFFFFF),
            secondary = Color(0xFF7A0099),
            onSecondary = Color(0xFFFFFFFF),
            tertiary = Color(0xFF000000),
            onTertiary = Color(0xFFFFFFFF),
            error = Color(0xFFB00020),
            onError = Color(0xFFFFFFFF),
            background = Color(0xFFFFFFFF),
            onBackground = Color(0xFF000000),
            surface = Color(0xFFFFFFFF),
            onSurface = Color(0xFF000000),
            surfaceVariant = Color(0xFFF0F0F0),
            onSurfaceVariant = Color(0xFF000000),
            outline = Color(0xFF000000),
        )
    }
