/*
 * AdGate.kt — AppLovin MAX interstitial gate for Android.
 *
 * The gate owns the MAX interstitial lifecycle (preload → show → reload) but defers
 * EVERY policy question to the shared Rust core:
 *   • shouldShowInterstitial(nowMs)  decides if a break point is eligible
 *   • notifyInterstitialShown(nowMs) arms the cooldown, only after a real display
 *
 * Because the identical core drives the iOS gate, the two platforms cannot diverge on
 * cadence. The gate also no-ops automatically for premium users, since the core's
 * shouldShowInterstitial returns false whenever premium is set.
 *
 * Typical wiring from an Activity:
 *   private val gate by lazy { AdGate(this, (application as RustyNesApp).core) }
 *   override fun onCreate(...) { gate.preload() }
 *   // at a natural break (ROM loaded, returned to menu, save-state taken):
 *   gate.maybeShowInterstitial()
 */
package com.doublegate.rustynes.monetization

import com.doublegate.rustynes.BuildConfig

import android.app.Activity
import android.os.SystemClock
import com.doublegate.rustynes.monetization.ffi.AdPolicy
import com.applovin.mediation.MaxAd
import com.applovin.mediation.MaxAdListener
import com.applovin.mediation.MaxError
import com.applovin.mediation.ads.MaxInterstitialAd

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
}
