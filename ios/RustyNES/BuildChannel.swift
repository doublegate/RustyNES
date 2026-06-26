//
//  BuildChannel.swift  (v1.9.8 "Horizon")
//
//  The foss / App-Store distribution seam, the iOS counterpart of the Android
//  `foss` / `play` flavor split (ADR 0025) and the iOS distribution decision in
//  ADR 0027 §3. A compile-time Active Compilation Condition (`APPSTORE_BUILD`)
//  selects the channel:
//
//  - DEFAULT (no `APPSTORE_BUILD`) = the `foss` channel: AltStore PAL / GitHub /
//    TestFlight. Ad-free, tracking-free, "Data Not Collected". This is what every
//    build does today.
//  - `APPSTORE_BUILD` defined = the `appstore` channel: the future home of the
//    StoreKit 2 + RevenueCat "$3.99 / Remove Ads" unlock and the AppLovin MAX ads
//    gated behind App Tracking Transparency.
//
//  v1.9.8 only LAYS the seam — BOTH channels currently behave identically (fully
//  unlocked, no ads, no tracking). The real split is flipped on at v2.1.0 (ADR 0027),
//  when `StoreManager` (see `Entitlements.swift`) and the `rustynes-monetization`
//  AdPolicy core are wired in. To produce the App-Store flavor, add `APPSTORE_BUILD`
//  to `SWIFT_ACTIVE_COMPILATION_CONDITIONS` for that build configuration (see the
//  documented note in `ios/project.yml`); the default scheme stays `foss`.
//

import Foundation

enum BuildChannel {
    enum Channel { case foss, appStore }

    /// The channel this binary was compiled for.
    static let current: Channel = {
        #if APPSTORE_BUILD
            return .appStore
        #else
            return .foss
        #endif
    }()

    static var isAppStore: Bool { current == .appStore }
    static var isFoss: Bool { current == .foss }

    /// Whether StoreKit / RevenueCat purchasing is even compiled in for this channel.
    /// (Still gated by `StoreManager.storeKitEnabled`, which stays false in v1.9.x.)
    static var usesStoreKit: Bool { isAppStore }

    /// Whether the ad SDK is compiled in for this channel. The foss channel is always
    /// ad-free; even on the App-Store channel ads stay dormant through v1.9.x.
    static var allowsAds: Bool { isAppStore }

    /// A short human-readable tag for the About screen / diagnostics.
    static var displayName: String {
        switch current {
        case .foss: return "FOSS"
        case .appStore: return "App Store"
        }
    }
}
