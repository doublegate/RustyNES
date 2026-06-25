//
//  Entitlements.swift  (v1.9.1 "Patch")
//
//  DORMANT freemium-gate scaffold. Through the ENTIRE v1.9.x TestFlight train the
//  app is fully unlocked and free — TestFlight builds carry no purchase and the
//  privacy label stays "Data Not Collected". This type is the seam the v2.1.0
//  launch wires to the shared `rustynes-monetization` crate (RevenueCat +
//  StoreKit 2 + AppLovin MAX, the ad-supported $3.99 model; see ADR 0025 and
//  `to-dos/plans/v2.0.x-mobile-finalization-plan.md`). The full StoreKit 2 /
//  RevenueCat scaffolding lands at v1.9.8 "Horizon"; this v1.9.1 stub just makes
//  the gate present-but-inert so that wiring is a drop-in (flip the source from
//  this stub to the rustynes-monetization entitlement state) rather than a
//  retrofit — the iOS analog of the Android dormant `Billing.kt`.
//
//  Determinism note: entitlement state never reaches the emulation core — it
//  gates only optional UI surfaces, exactly as `rustynes-monetization` documents.
//

import Combine
import Foundation

/// The app's entitlement state. **Dormant in v1.9.x** — every feature is unlocked.
@MainActor
final class Entitlements: ObservableObject {
    /// Whether premium features are unlocked.
    ///
    /// v1.9.x: always `true` (free + full on TestFlight). v2.1.0: replaced by the
    /// `rustynes-monetization` query (purchases / restore / receipt), OR'd with
    /// `debugForceUnlock` so dev/QA builds never hit the gate.
    @Published private(set) var isUnlocked: Bool = true

    /// A debug-only forced full-unlock. Inert today (already unlocked); it keeps
    /// the override meaningful once the v2.1.0 wiring replaces the `true` above
    /// with a real entitlement source.
    static var debugForceUnlock: Bool {
        #if DEBUG
            return true
        #else
            return false
        #endif
    }

    init() {}

    /// Re-evaluate entitlements. A no-op in v1.9.x; v2.1.0 queries
    /// `rustynes-monetization` (which itself respects `debugForceUnlock`). Kept so
    /// call sites (`.task { entitlements.refresh() }`) exist from v1.9.1 and don't
    /// need to be added during the v2.1.0 wiring.
    func refresh() {
        // Dormant: nothing to query yet; `isUnlocked` stays true.
    }
}
