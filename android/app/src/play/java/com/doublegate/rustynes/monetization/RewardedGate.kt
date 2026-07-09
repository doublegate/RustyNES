/*
 * RewardedGate.kt — AppLovin MAX rewarded-ad gate (PLAY-FLAVOR ONLY, v2.0.3, ADR 0025).
 *
 * Adapted from `crates/rustynes-monetization/shells/android/RewardedGate.kt`. Counterpart
 * to AdGate.kt (interstitials); like it, this lives only in the `play` source set (it
 * imports `com.applovin.*`). It is the free-tier play-time extender: when a free user is
 * out of budget and taps "Watch ad for +time", show a rewarded ad, and on the network's
 * REWARD callback call the shared core's grantRewardedTime() — never on load, show, or
 * dismiss, so the grant maps exactly to a qualifying view.
 *
 * Cadence/cap policy lives in the core: gate the offer on core.canOfferRewarded() and
 * label it with core.rewardGrantsRemaining(); grantRewardedTime() enforces the per-session
 * grant cap and returns false (a no-op) once it is reached.
 */
package com.doublegate.rustynes.monetization

import android.app.Activity
import com.applovin.mediation.MaxAd
import com.applovin.mediation.MaxError
import com.applovin.mediation.MaxReward
import com.applovin.mediation.MaxRewardedAdListener
import com.applovin.mediation.ads.MaxRewardedAd
import com.doublegate.rustynes.BuildConfig
import com.doublegate.rustynes.monetization.ffi.AdPolicy

/**
 * Rewarded gate. The [onResume] callback resumes the paused emulator after a granted
 * reward (or an early bail — the caller's resume path always runs).
 */
class RewardedGate(
    private val activity: Activity,
    private val core: AdPolicy,
    /** Called after a granted reward so the host can resume the paused emulator. */
    private val onResume: () -> Unit = {},
) : MaxRewardedAdListener {

    private val rewarded: MaxRewardedAd =
        MaxRewardedAd.getInstance(BuildConfig.MAX_REWARDED_AD_UNIT_ID, activity).apply {
            setListener(this@RewardedGate)
        }

    /** Warm the cache so the "+time" tap plays instantly. Call early, and ~60-90 s before run-out. */
    fun preload() {
        if (!core.isPremium()) rewarded.loadAd()
    }

    /**
     * Present a rewarded ad if one is ready. The caller should only reach here when the
     * core says an offer is allowed (core.canOfferRewarded()); premium users never do.
     * Returns false if no ad was ready (caller should fall back to the Full Version prompt
     * or the offline-grace path, core.canGrantOfflineGrace()).
     */
    fun show(): Boolean {
        if (core.isPremium() || !core.canOfferRewarded()) return false
        return if (rewarded.isReady) {
            rewarded.showAd()
            true
        } else {
            rewarded.loadAd() // not cached yet; the caller handles the no-ad-available case
            false
        }
    }

    // --- MaxRewardedAdListener ------------------------------------------------------

    /**
     * The ONLY place a grant happens. Fired when the user has watched the ad for the
     * required duration. grantRewardedTime() adds +reward_play_ms and enforces the
     * per-session cap (returns false once hit); then resume the emulator.
     */
    override fun onUserRewarded(ad: MaxAd, reward: MaxReward) {
        core.grantRewardedTime()
        onResume()
    }

    override fun onAdLoaded(ad: MaxAd) { /* cached; shown explicitly via show() */ }
    override fun onAdDisplayed(ad: MaxAd) { /* no-op */ }

    override fun onAdHidden(ad: MaxAd) {
        // Reload for the next run-out. If onUserRewarded did NOT fire (user bailed early),
        // no grant was made — the caller's resume path still runs the paused emulator.
        if (!core.isPremium()) rewarded.loadAd()
    }

    override fun onAdClicked(ad: MaxAd) { /* no-op */ }
    override fun onAdLoadFailed(adUnitId: String, error: MaxError) { /* optional: backoff retry */ }
    override fun onAdDisplayFailed(ad: MaxAd, error: MaxError) {
        if (!core.isPremium()) rewarded.loadAd()
    }

    /**
     * Detach the listener so this gate (and the Activity it holds) is not retained by the
     * shared `MaxRewardedAd` singleton. Call from the owner's `onDestroy`.
     */
    fun destroy() {
        rewarded.setListener(null)
    }
}
