//
//  AdGate.swift — AppLovin MAX interstitial gate for iOS.
//
//  Direct counterpart of Android's AdGate.kt. Owns the MAX interstitial lifecycle
//  (preload → show → reload) and defers every policy decision to the shared Rust core
//  (shouldShowInterstitial / notifyInterstitialShown). Identical core ⇒ identical
//  cadence on both platforms, and an automatic no-op for premium users.
//
//  Usage:
//    Monetization.shared.adGate.preload()           // once, after SDK init
//    Monetization.shared.adGate.maybeShowInterstitial()  // at a natural break point
//

import Foundation
import AppLovinSDK
import RustyNesMonetization

final class AdGate: NSObject, MAAdDelegate {
    private let core: AdPolicy
    private let interstitial: MAInterstitialAd

    init(core: AdPolicy) {
        self.core = core
        self.interstitial = MAInterstitialAd(adUnitIdentifier: Config.maxInterstitialAdUnitId)
        super.init()
        self.interstitial.delegate = self
    }

    /// Monotonic ms INCLUDING deep sleep, matching Android's SystemClock.elapsedRealtime()
    /// (the clock the Rust core was constructed with). `mach_continuous_time()` keeps
    /// ticking while the device sleeps — DispatchTime.uptimeNanoseconds pauses, which would
    /// diverge the ad cooldown / pacing from Android.
    private func nowMs() -> UInt64 {
        var info = mach_timebase_info()
        mach_timebase_info(&info)
        let nanos = mach_continuous_time() * UInt64(info.numer) / UInt64(info.denom)
        return nanos / 1_000_000
    }

    /// Warm the cache so a later show() is instant.
    func preload() {
        if !core.isPremium() { interstitial.load() }
    }

    /// Show an interstitial iff the core allows it now. Premium status, launch grace,
    /// and the inter-ad cooldown are all enforced inside the core.
    func maybeShowInterstitial() {
        guard core.shouldShowInterstitial(nowMs: nowMs()) else { return }
        if interstitial.isReady {
            interstitial.show()
        } else {
            interstitial.load() // not cached yet; show on the next break point
        }
    }

    // MARK: - MAAdDelegate

    func didLoad(_ ad: MAAd) {
        // Cached. We do not auto-show; showing is gated by the core via maybeShowInterstitial().
    }

    func didDisplay(_ ad: MAAd) {
        // On screen now — arm the cooldown (not at decision time) so a failed load
        // never consumes the interval.
        core.notifyInterstitialShown(nowMs: nowMs())
    }

    func didHide(_ ad: MAAd) {
        if !core.isPremium() { interstitial.load() } // reload for the next break
    }

    func didClick(_ ad: MAAd) { /* no-op */ }

    func didFailToLoadAd(forAdUnitIdentifier adUnitIdentifier: String, withError error: MAError) {
        // Optional: exponential backoff before retrying interstitial.load().
    }

    func didFail(toDisplay ad: MAAd, withError error: MAError) {
        if !core.isPremium() { interstitial.load() }
    }
}
