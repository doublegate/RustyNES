//
//  RustyNesApp.swift — iOS entry point + monetization coordinator.
//
//  Mirrors the Android RustyNesApp.kt. It:
//    1. Builds the single shared `AdPolicy` from the Rust core (RustyNesMonetization module).
//    2. Initializes AppLovin MAX with the current config-builder init API.
//    3. Configures RevenueCat and binds the premium entitlement into the core.
//
//  The `Monetization` singleton holds the process-wide core, billing wrapper, and ad
//  gate so SwiftUI views can reach them through the environment or directly.
//
//  SDK sources (see Package.swift): RevenueCat (purchases-ios), AppLovinSDK, and the
//  RustyNesMonetization Swift package produced from the Rust core by cargo-swift (see README).
//

import SwiftUI
import AppLovinSDK
import RevenueCat
import RustyNesMonetization

/// Build-time configuration. Inject real values via an .xcconfig / Info.plist rather
/// than committing secrets. Placeholders keep the skeleton self-contained.
enum Config {
    static let appLovinSdkKey   = (Bundle.main.object(forInfoDictionaryKey: "APPLOVIN_SDK_KEY") as? String) ?? ""
    static let revenueCatApiKey = (Bundle.main.object(forInfoDictionaryKey: "REVENUECAT_API_KEY") as? String) ?? ""
    static let maxInterstitialAdUnitId =
        (Bundle.main.object(forInfoDictionaryKey: "MAX_INTERSTITIAL_AD_UNIT_ID") as? String) ?? ""

    /// RevenueCat entitlement identifier; a single entitlement gates everything.
    static let entitlementPremium = "premium"
}

/// Process-wide monetization coordinator.
final class Monetization {
    static let shared = Monetization()

    /// Shared policy core. `defaultAdConfig()` / `AdPolicy(config:nowMs:)` are RustyNesMonetization bindings.
    let core: AdPolicy
    let billing: Billing
    let adGate: AdGate

    private init() {
        // Monotonic milliseconds INCLUDING deep sleep, matching Android's
        // SystemClock.elapsedRealtime() (the clock the Rust core was built against).
        // `mach_continuous_time()` keeps ticking while the device sleeps, whereas
        // DispatchTime.uptimeNanoseconds pauses — using the latter would let the ad
        // cooldown + play-time pacing diverge between iOS and Android.
        core = AdPolicy(config: defaultAdConfig(), nowMs: Self.continuousMs())
        billing = Billing(core: core)
        adGate = AdGate(core: core)
    }

    /// Monotonic milliseconds including deep sleep — the iOS analogue of Android's
    /// `SystemClock.elapsedRealtime()`. Shared by the AdGate so both clocks agree.
    static func continuousMs() -> UInt64 {
        var info = mach_timebase_info()
        mach_timebase_info(&info)
        let nanos = mach_continuous_time() * UInt64(info.numer) / UInt64(info.denom)
        return nanos / 1_000_000
    }

    /// Call once at launch (from the App's init).
    func start() {
        // The ad + billing SDKs initialize ONLY in the App Store build. Sideload /
        // TestFlight-without-monetization builds (PLAY_BUILD off) stay full-featured and
        // ad-free, so the AppLovin + RevenueCat SDKs never initialize there.
        #if PLAY_BUILD
        // (2) Initialize AppLovin MAX (config-builder API). Do this as early as possible
        // so the SDK has maximum time to pre-cache mediated networks' ads.
        let initConfig = ALSdkInitializationConfiguration(sdkKey: Config.appLovinSdkKey) { builder in
            builder.mediationProvider = ALMediationProviderMAX
        }
        ALSdk.shared().initialize(with: initConfig) { _ in
            // SDK ready — views may now preload/show interstitials via adGate.
        }

        // (3) Configure RevenueCat and bind the entitlement → core (initial fetch +
        // live delegate updates on purchase / restore / expiry).
        #if DEBUG
        Purchases.logLevel = .debug
        #else
        Purchases.logLevel = .info
        #endif
        Purchases.configure(withAPIKey: Config.revenueCatApiKey)
        billing.bindEntitlement()
        #endif
    }
}

@main
struct RustyNesApp: App {
    init() {
        Monetization.shared.start()
    }

    var body: some Scene {
        WindowGroup {
            // Replace with the emulator's root view.
            Text("RustyNES")
        }
    }
}
