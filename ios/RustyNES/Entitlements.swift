//
//  Entitlements.swift  (v1.9.1 "Patch" / v1.9.8 "Horizon")
//
//  DORMANT freemium-gate scaffold. Through the ENTIRE v1.9.x TestFlight train the
//  app is fully unlocked and free — TestFlight builds carry no purchase and the
//  privacy label stays "Data Not Collected". This type is the seam the v2.1.0
//  launch wires to the shared `rustynes-monetization` crate (RevenueCat +
//  StoreKit 2 + AppLovin MAX, the ad-supported $3.99 model; see ADR 0025 and
//  `to-dos/plans/v2.0.x-mobile-finalization-plan.md`). v1.9.8 "Horizon" lands the
//  dormant StoreKit 2 scaffolding (`StoreManager` below) + the foss/App-Store
//  channel seam (`BuildChannel`); this stub keeps the gate present-but-inert so the
//  v2.1.0 wiring is a drop-in (flip the source from this stub to the
//  rustynes-monetization entitlement state) rather than a retrofit — the iOS analog
//  of the Android dormant `Billing.kt`.
//
//  Determinism note: entitlement state never reaches the emulation core — it
//  gates only optional UI surfaces, exactly as `rustynes-monetization` documents.
//

import Combine
import Foundation
import StoreKit

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
    ///
    /// v2.1.0 shape (for reference): on the `foss` channel `isUnlocked` stays
    /// permanently `true`; on the `appstore` channel it becomes
    /// `StoreManager.shared.purchased || Self.debugForceUnlock`.
    func refresh() {
        // Dormant: nothing to query yet; `isUnlocked` stays true.
    }
}

/// DORMANT StoreKit 2 scaffold (v1.9.8 "Horizon"). Through v1.9.x this never runs —
/// `storeKitEnabled` is false and `BuildChannel.usesStoreKit` is false on the default
/// `foss` channel, so `products` stays empty and `purchased` stays false WITHOUT
/// gating anything (the app is fully unlocked via `Entitlements.isUnlocked == true`).
/// It is shaped now so the v2.1.0 App-Store wiring (the "$3.99 Full Version / Remove
/// Ads" unlock) is a drop-in: flip `storeKitEnabled` true on the `appstore` channel
/// and OR `purchased` into `Entitlements.isUnlocked`. The product fetch / purchase /
/// restore methods are real StoreKit 2 calls behind the dormant guards, so they are
/// ready to exercise the moment the flag flips. (StoreKit 2 requires iOS 15, which is
/// the deployment floor.)
@MainActor
final class StoreManager: ObservableObject {
    /// The single non-consumable "Full Version" product id (matched in App Store
    /// Connect at v2.1.0). Mirrors the Android one-time-unlock SKU.
    static let fullUnlockProductID = "com.doublegate.rustynes.fullunlock"

    /// Master kill-switch. **false through all of v1.9.x** — every method below
    /// returns immediately, so no StoreKit traffic occurs and nothing is gated.
    static let storeKitEnabled = false

    @Published private(set) var products: [Product] = []
    @Published private(set) var purchased = false

    /// Fetch the product catalog + reconcile existing entitlements. Dormant in v1.9.x.
    func refresh() async {
        guard Self.storeKitEnabled, BuildChannel.usesStoreKit else { return }
        do {
            products = try await Product.products(for: [Self.fullUnlockProductID])
            await reconcileEntitlements()
        } catch {
            // Dormant scaffold: swallow; the v2.1.0 wiring surfaces this to the user.
        }
    }

    /// Buy the Full Version unlock. Dormant in v1.9.x.
    func purchaseFullUnlock() async {
        guard Self.storeKitEnabled, BuildChannel.usesStoreKit,
              let product = products.first else { return }
        do {
            let result = try await product.purchase()
            if case .success(let verification) = result,
               case .verified(let transaction) = verification {
                purchased = true
                await transaction.finish()
            }
        } catch {
            // Dormant scaffold.
        }
    }

    /// Restore prior purchases. Dormant in v1.9.x.
    func restore() async {
        guard Self.storeKitEnabled, BuildChannel.usesStoreKit else { return }
        try? await AppStore.sync()
        await reconcileEntitlements()
    }

    /// Mark `purchased` from the current StoreKit entitlements.
    private func reconcileEntitlements() async {
        for await result in Transaction.currentEntitlements {
            if case .verified(let transaction) = result,
               transaction.productID == Self.fullUnlockProductID,
               transaction.revocationDate == nil {
                purchased = true
            }
        }
    }
}
