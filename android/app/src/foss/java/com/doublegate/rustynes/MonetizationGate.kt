// FOSS-FLAVOR SOURCE SET (v2.0.3, ADR 0025). No-op stand-in for the freemium /
// ad-supported monetization façade (the real one is in
// `src/play/.../MonetizationGate.kt`). This file links NO ads SDK
// (`com.applovin.*`), NO store SDK (`com.revenuecat.*`), and never even constructs the
// `AdPolicy` Rust core — that is the whole point of the FOSS / F-Droid split: the clean
// channel ships every feature for free, ad-free, with zero tracking.
//
// Its public surface is byte-for-byte what `MainActivity` (a `src/main` file) calls on
// the `play` twin, so the shared `MainActivity` compiles against either flavor
// unchanged. Because every method here is a no-op (or a constant "yes"), the FOSS build
// is behaviourally identical to the pre-monetization build: no session gate, no run-out
// timer, no paywall, no ads. This is the byte-identical-default guarantee ADR 0025
// requires for the F-Droid / GitHub-Releases artifact.
package com.doublegate.rustynes

import android.app.Activity
import androidx.compose.runtime.Composable
import com.doublegate.rustynes.monetization.ffi.PremiumFeature

/**
 * No-op FOSS monetization gate.
 *
 * The FOSS build has no freemium, no ads, and no play-time limit, so every query
 * answers in the user's favour and every side-effecting call is inert. The
 * [PremiumFeature] enum (a pure-Kotlin UniFFI type, no Google dependency) is referenced
 * only to keep the method signature identical to the `play` twin; no FFI method is ever
 * invoked here, so the monetization native library / JNA dispatcher is never loaded in
 * the FOSS process.
 */
@Suppress("UNUSED_PARAMETER")
class MonetizationGate(appContext: android.content.Context) {

    /** No app-session bookkeeping in FOSS. */
    fun beginSession() {}

    /** No ad preloading / SDK init in FOSS. */
    fun onActivityCreated(activity: Activity) {}

    /** No entitlement binding / ad refresh in FOSS. */
    fun onResume(activity: Activity) {}

    /** Detach any held Activity reference (nothing is held in FOSS). */
    fun onDestroy() {}

    /**
     * FOSS ships every feature unlocked, so every gate is open. Mirrors the `play`
     * twin's `featureEnabled(...)` surface; the argument is ignored.
     */
    fun featureEnabled(feature: PremiumFeature): Boolean = true

    /** No per-game play budget in FOSS. */
    fun startPlay() {}

    /** No play-time accounting in FOSS. */
    fun addActiveTime(deltaMs: Long) {}

    /** Play is always allowed in FOSS (no time gate). */
    fun isPlayAllowed(): Boolean = true

    /** No progress to persist in FOSS (there is no budget to carry across launches). */
    fun exportProgress(romKey: String) {}

    /** No progress to restore in FOSS. */
    fun restoreProgress(romKey: String) {}

    /**
     * The run-out paywall + countdown overlay. Draws NOTHING in FOSS (there is no
     * demo/paywall), so the shared `MainActivity` can place it unconditionally in its
     * Compose tree with no visual or behavioural change in the clean build.
     */
    @Composable
    fun RunOutOverlay(onResume: () -> Unit) {
        // Intentionally empty: no paywall in the ad-free FOSS channel.
    }
}
