/*
 * AdGate.kt — AppLovin MAX interstitial gate (PLAY-FLAVOR ONLY, v2.0.3, ADR 0025).
 *
 * Adapted verbatim in behaviour from the reference shell at
 * `crates/rustynes-monetization/shells/android/AdGate.kt`. It lives ONLY in the `play`
 * source set: it imports `com.applovin.*`, which the `foss` (F-Droid / GitHub-Releases)
 * artifact deliberately links none of. The `foss` twin needs no counterpart because the
 * shared `MonetizationGate` façade — real in `play`, no-op in `foss` — is the only class
 * `MainActivity` (src/main) touches; the ad gates are an internal `play`-side detail.
 *
 * The gate owns the MAX interstitial lifecycle (preload -> show -> reload) but defers
 * EVERY policy question to the shared Rust core (`AdPolicy`, a UniFFI object):
 *   - shouldShowInterstitial(nowMs)  decides if a break point is eligible
 *   - notifyInterstitialShown(nowMs) arms the cooldown, only after a real display
 *
 * Because the identical core drives the iOS gate, the two platforms cannot diverge on
 * cadence. The gate also no-ops automatically for premium users, since the core's
 * shouldShowInterstitial returns false whenever premium is set.
 */
package com.doublegate.rustynes.monetization

import android.app.Activity
import android.os.SystemClock
import com.applovin.mediation.MaxAd
import com.applovin.mediation.MaxAdListener
import com.applovin.mediation.MaxError
import com.applovin.mediation.ads.MaxInterstitialAd
import com.doublegate.rustynes.BuildConfig
import com.doublegate.rustynes.monetization.ffi.AdPolicy

/**
 * Interstitial gate. Construct once per Activity with the process-wide [AdPolicy] core.
 * Every showing is gated by the core at an explicit break point, never auto-shown.
 */
class AdGate(
    private val activity: Activity,
    private val core: AdPolicy,
) : MaxAdListener {

    private val interstitial: MaxInterstitialAd =
        MaxInterstitialAd(BuildConfig.MAX_INTERSTITIAL_AD_UNIT_ID, activity).apply {
            setListener(this@AdGate)
        }

    /** Monotonic ms, matching the clock model the Rust core was constructed with. */
    private fun nowMs(): ULong = SystemClock.elapsedRealtime().toULong()

    /** Warm the cache so a later show() is instant. Safe to call once after init. */
    fun preload() {
        if (!core.isPremium()) interstitial.loadAd()
    }

    /**
     * Show an interstitial iff the core allows it right now. Premium users, the launch
     * grace window, and the inter-ad cooldown are all handled inside the core, so the
     * caller can invoke this freely at any natural break point.
     */
    fun maybeShowInterstitial() {
        if (!core.shouldShowInterstitial(nowMs())) return
        if (interstitial.isReady) {
            interstitial.showAd()
        } else {
            interstitial.loadAd() // not cached yet; show on the next break point
        }
    }

    // --- MaxAdListener -------------------------------------------------------------

    override fun onAdLoaded(ad: MaxAd) {
        // Loaded and cached. We deliberately do NOT auto-show here; showing is gated by
        // the core at an explicit break point via maybeShowInterstitial().
    }

    override fun onAdDisplayed(ad: MaxAd) {
        // The ad is actually on screen — arm the cooldown now (not at decision time),
        // so a failed load never burns the interval.
        core.notifyInterstitialShown(nowMs())
    }

    override fun onAdHidden(ad: MaxAd) {
        // User dismissed the ad; immediately reload for the next eligible break.
        if (!core.isPremium()) interstitial.loadAd()
    }

    override fun onAdClicked(ad: MaxAd) { /* no-op */ }

    override fun onAdLoadFailed(adUnitId: String, error: MaxError) {
        // Optional: implement exponential backoff before retrying loadAd().
    }

    override fun onAdDisplayFailed(ad: MaxAd, error: MaxError) {
        // Display failed; reload so the next break point can try again.
        if (!core.isPremium()) interstitial.loadAd()
    }

    /**
     * Detach the listener so this gate (and the Activity it holds) is not retained by the
     * long-lived `MaxInterstitialAd`. Call from the owner's `onDestroy` (e.g. a rotation).
     */
    fun destroy() {
        interstitial.setListener(null)
    }
}
