/*
 * RewardedGate.kt — AppLovin MAX rewarded-ad gate for Android (the free-tier engine).
 *
 * Counterpart to AdGate.kt (interstitials). This owns the MAX rewarded lifecycle
 * (preload -> show -> reload) and is what extends the free-tier play budget: when the
 * user is out of time and taps "Watch ad for +11 min", show a rewarded ad, and on the
 * network's REWARD callback call the shared core's grantRewardedTime() — never on load,
 * show, or dismiss, so the grant maps exactly to a qualifying view.
 *
 * Cadence/cap policy lives in the core: gate the offer on core.canOfferRewarded() and
 * label it with core.rewardGrantsRemaining(); grantRewardedTime() enforces the 2-grant
 * cap and returns false (a no-op) once it is reached.
 *
 * Typical wiring from the run-out prompt:
 *   private val rewarded by lazy { RewardedGate(this, core) { resumeEmulator() } }
 *   override fun onCreate(...) { rewarded.preload() }
 *   // when the user taps "Watch ad for +11 min" (only shown if core.canOfferRewarded()):
 *   rewarded.show()
 */
package com.doublegate.rustynes.monetization

import android.app.Activity

import com.doublegate.rustynes.BuildConfig
import com.doublegate.rustynes.monetization.ffi.AdPolicy
import com.applovin.mediation.MaxAd
import com.applovin.mediation.MaxError
import com.applovin.mediation.MaxReward
import com.applovin.mediation.MaxRewardedAdListener
import com.applovin.mediation.ads.MaxRewardedAd

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

    /** Warm the cache so the "+11 min" tap plays instantly. Call early, and ~60-90 s before run-out. */
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
}
