//
//  Billing.swift — RevenueCat wrapper for iOS.
//
//  The iOS counterpart of Android's Billing.kt, with identical responsibilities and
//  the same single-source-of-truth discipline: translate RevenueCat `CustomerInfo`
//  into one premium boolean and push it into the shared Rust core. Conforms to
//  `PurchasesDelegate` so entitlement changes propagate live.
//
//  Flows: bindEntitlement (initial + live), purchasePremium, restorePurchases.
//

import Foundation
import RevenueCat
import RustyNesMonetization

final class Billing: NSObject, PurchasesDelegate {
    private let core: AdPolicy

    init(core: AdPolicy) {
        self.core = core
        super.init()
    }

    /// Map a CustomerInfo to premium status and forward it to the Rust core.
    private func apply(_ info: CustomerInfo?) {
        let active = info?.entitlements[Config.entitlementPremium]?.isActive == true
        // OR-in the debug tester override so an async fetch can never clobber a local
        // unlock. In release this term is always false (see testerUnlockEnabled).
        core.setPremium(premium: active || testerUnlockEnabled())
    }

    /// INTERNAL DEV ONLY — force premium without a purchase, for local QA on a debug build.
    ///
    /// Still routes through the single source of truth (`core.setPremium` via `apply`), so it
    /// adds no second premium flag. Gated on `#if DEBUG` *and* an Info.plist boolean
    /// `RUSTYNES_TESTER_UNLOCK` (default false), so it is inert in any App Store / TestFlight
    /// build. TestFlight testers are unlocked instead via a RevenueCat promotional grant or an
    /// App Store **sandbox** purchase (runbook §5a, brief §9).
    private func testerUnlockEnabled() -> Bool {
        #if DEBUG
        return (Bundle.main.object(forInfoDictionaryKey: "RUSTYNES_TESTER_UNLOCK") as? Bool) ?? false
        #else
        return false
        #endif
    }

    /// Call once at startup. Sets the delegate (live updates) and does an initial fetch.
    func bindEntitlement() {
        if testerUnlockEnabled() { core.setPremium(premium: true) } // immediate (debug only)
        Purchases.shared.delegate = self
        Purchases.shared.getCustomerInfo { [weak self] info, _ in
            self?.apply(info)
        }
    }

    /// PurchasesDelegate — fires whenever RevenueCat receives updated customer info
    /// (purchase, restore, renewal, expiry). Keeps the core's premium flag current.
    func purchases(_ purchases: Purchases, receivedUpdated customerInfo: CustomerInfo) {
        apply(customerInfo)
    }

    /// Purchase the premium (remove-ads) package from the current offering.
    func purchasePremium(completion: @escaping (_ premium: Bool, _ error: Error?) -> Void) {
        Purchases.shared.getOfferings { [weak self] offerings, error in
            guard let self else { return }
            // Prefer the LIFETIME (non-consumable "Full Version / Remove Ads") package
            // explicitly rather than assuming it is first — so adding a tier/subscription
            // to the offering later can't silently change what gets purchased.
            let package = offerings?.current?.availablePackages.first { $0.packageType == .lifetime }
                ?? offerings?.current?.availablePackages.first
            guard let package else {
                completion(self.core.isPremium(), error) // misconfigured offering
                return
            }
            Purchases.shared.purchase(package: package) { _, customerInfo, error, userCancelled in
                if userCancelled {
                    completion(self.core.isPremium(), nil)
                    return
                }
                self.apply(customerInfo)
                completion(self.core.isPremium(), error)
            }
        }
    }

    /// Restore prior purchases. Surface behind a visible "Restore Purchases" control —
    /// Apple requires freely available restore for non-consumable entitlements.
    func restorePurchases(completion: @escaping (_ premium: Bool, _ error: Error?) -> Void) {
        Purchases.shared.restorePurchases { [weak self] customerInfo, error in
            guard let self else { return }
            self.apply(customerInfo)
            completion(self.core.isPremium(), error)
        }
    }
}
