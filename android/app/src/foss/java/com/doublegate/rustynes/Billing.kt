// FOSS-FLAVOR SOURCE SET (v2.0.1, ADR 0025). No-op stand-in for the Play-Billing
// `LicenseManager` (the real one is in `src/play/.../Billing.kt`). This file links NO
// `com.android.billingclient.*` — that is the whole point of the FOSS / F-Droid split.
//
// The FOSS build has NO freemium / demo gate: there is nothing to purchase, so the app
// is simply full-featured. [isUnlocked] is therefore a constant `true`, every entitlement
// call is a no-op, and [priceLabel] is a never-shown placeholder (the unlock affordance
// is only drawn when `!unlocked`, which never happens here, and behind `PLAY_BUILD`
// elsewhere). The public surface is byte-for-byte what `MainActivity` calls on the `play`
// twin, so the shared `MainActivity` (src/main) compiles against either flavor unchanged.
package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** The one-time "Full Unlock" product id — unused in FOSS (no Billing), kept for parity. */
const val FULL_UNLOCK_PRODUCT = "full_unlock"

/**
 * No-op FOSS entitlement manager. Always "unlocked" (the FOSS build ships every feature
 * for free), so the shell's demo gate is inert.
 */
@Suppress("UNUSED_PARAMETER")
class LicenseManager(private val appContext: Context) {

    /** FOSS is always fully unlocked (no freemium). State-backed to mirror the `play`
     *  twin's Compose-observable surface, though it never changes here. */
    var isUnlocked by mutableStateOf(true)
        private set

    /** Placeholder price label; never shown in FOSS (the unlock button is gated on
     *  `!isUnlocked`, which is never true). Mirrors the `play` twin's `String` surface. */
    val priceLabel: String
        get() = "$2.99"

    /** No Play connection in FOSS. */
    fun connect() {}

    /** No entitlement to refresh in FOSS. */
    fun refreshEntitlement() {}

    /** No purchase flow in FOSS. */
    fun purchase(activity: Activity) {}

    /** Debug unlock toggle is meaningless when already permanently unlocked; no-op. */
    fun debugForceUnlocked(value: Boolean) {}
}
