//
//  RewardedGate.swift — AppLovin MAX rewarded-ad gate for iOS (the free-tier engine).
//
//  Direct counterpart of Android's RewardedGate.kt and the sibling of AdGate.swift
//  (interstitials). Owns the MAX rewarded lifecycle (preload -> show -> reload) and
//  extends the free-tier play budget: on the network's REWARD callback (didRewardUser)
//  it calls the shared core's grantRewardedTime() — never on load/show/dismiss — then
//  resumes the paused emulator.
//
//  Cadence/cap policy lives in the core: gate the offer on core.canOfferRewarded() and
//  label it with core.rewardGrantsRemaining(); grantRewardedTime() enforces the 11-grant
//  cap and returns false (no-op) once reached.
//
//  Usage from the run-out prompt:
//    let rewarded = RewardedGate(core: core) { resumeEmulator() }
//    rewarded.preload()                 // early, and ~60-90 s before run-out
//    if core.canOfferRewarded() { _ = rewarded.show() }   // on "Watch ad for +2 min"
//

import Foundation
import AppLovinSDK
import RustyNesMonetization

final class RewardedGate: NSObject, MARewardedAdDelegate {
    private let core: AdPolicy
    private let rewarded: MARewardedAd
    /// Called after a granted reward so the host can resume the paused emulator.
    private let onResume: () -> Void

    init(core: AdPolicy, onResume: @escaping () -> Void = {}) {
        self.core = core
        self.rewarded = MARewardedAd.shared(withAdUnitIdentifier: Config.maxRewardedAdUnitId)
        self.onResume = onResume
        super.init()
        self.rewarded.delegate = self
    }

    /// Warm the cache so the "+2 min" tap plays instantly. Call early, and ~60-90 s before run-out.
    func preload() {
        if !core.isPremium() { rewarded.load() }
    }

    /// Present a rewarded ad if one is ready. Reach here only when core.canOfferRewarded()
    /// is true. Returns false if no ad was ready (caller falls back to the Full Version
    /// prompt or the offline-grace path, core.canGrantOfflineGrace()).
    @discardableResult
    func show() -> Bool {
        if core.isPremium() || !core.canOfferRewarded() { return false }
        if rewarded.isReady {
            rewarded.show()
            return true
        }
        rewarded.load() // not cached yet; caller handles the no-ad-available case
        return false
    }

    // MARK: - MARewardedAdDelegate

    /// The ONLY place a grant happens — the user watched for the required duration.
    /// grantRewardedTime() adds +reward_play_ms (enforcing the per-session cap), then resume.
    func didRewardUser(forAd ad: MAAd, with reward: MAReward) {
        _ = core.grantRewardedTime()
        onResume()
    }

    func didLoad(_ ad: MAAd) { /* cached; shown explicitly via show() */ }
    func didDisplay(_ ad: MAAd) { /* no-op */ }

    func didHide(_ ad: MAAd) {
        // Reload for the next run-out. If didRewardUser did NOT fire (user bailed early),
        // no grant was made; the caller's resume path still runs the paused emulator.
        if !core.isPremium() { rewarded.load() }
    }

    func didClick(_ ad: MAAd) { /* no-op */ }
    func didFail(toLoadAdForAdUnitIdentifier adUnitIdentifier: String, withError error: MAError) {
        // optional: exponential backoff before retrying load()
    }
    func didFail(toDisplay ad: MAAd, withError error: MAError) {
        if !core.isPremium() { rewarded.load() }
    }
}
