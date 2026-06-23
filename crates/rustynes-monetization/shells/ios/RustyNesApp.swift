//
//  RustyNesApp.swift — iOS entry point + monetization coordinator.
//
//  Mirrors the Android RustyNesApp.kt. It:
//    1. Builds the single shared `AdPolicy` from the Rust core (RustyNesCore module).
//    2. Initializes AppLovin MAX with the current config-builder init API.
//    3. Configures RevenueCat and binds the premium entitlement into the core.
//
//  The `Monetization` singleton holds the process-wide core, billing wrapper, and ad
//  gate so SwiftUI views can reach them through the environment or directly.
//
//  SDK sources (see Package.swift): RevenueCat (purchases-ios), AppLovinSDK, and the
//  RustyNesCore Swift package produced from the Rust core by cargo-swift (see README).
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

    /// Shared policy core. `defaultAdConfig()` / `AdPolicy(config:nowMs:)` are RustyNesCore bindings.
    let core: AdPolicy
    let billing: Billing
    let adGate: AdGate

    private init() {
        // Monotonic milliseconds, matching the Rust core's clock model.
        let now = UInt64(DispatchTime.now().uptimeNanoseconds / 1_000_000)
        core = AdPolicy(config: defaultAdConfig(), nowMs: now)
        billing = Billing(core: core)
        adGate = AdGate(core: core)
    }

    /// Call once at launch (from the App's init).
    func start() {
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
        Purchases.logLevel = .debug // drop to .info for release builds
        Purchases.configure(withAPIKey: Config.revenueCatApiKey)
        billing.bindEntitlement()
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
