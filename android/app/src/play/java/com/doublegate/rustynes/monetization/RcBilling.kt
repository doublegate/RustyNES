/*
 * RcBilling.kt — RevenueCat entitlement wrapper (PLAY-FLAVOR ONLY, v2.0.3, ADR 0025).
 *
 * Adapted from `crates/rustynes-monetization/shells/android/Billing.kt`. Named `RcBilling`
 * (RevenueCat billing) to keep it unambiguously distinct from the pre-existing Play-Billing
 * `LicenseManager` (in `src/play/.../Billing.kt`, package `com.doublegate.rustynes`), which
 * drives the older one-time "$2.99 Full Unlock" IAP. This wrapper drives the ad-supported /
 * remove-ads monetization pivot (ADR 0025 + the 2026-06-23 pivot): a single RevenueCat
 * "premium" entitlement toggles every gate in the shared Rust core.
 *
 * This is the *only* place that knows about the store. It translates RevenueCat's
 * `CustomerInfo` into a single boolean (premium yes/no) and pushes it into the shared Rust
 * core via `AdPolicy.setPremium`. Everything else — including whether ads show — derives
 * from that one flag, so there is no second source of truth.
 *
 * Lives only in the `play` source set (imports `com.revenuecat.*`). The `foss` build links
 * no store SDK and ships premium-equivalent (everything free), so it needs no counterpart.
 */
package com.doublegate.rustynes.monetization

import android.app.Activity
import com.doublegate.rustynes.BuildConfig
import com.doublegate.rustynes.monetization.ffi.AdPolicy
import com.revenuecat.purchases.CustomerInfo
import com.revenuecat.purchases.PackageType
import com.revenuecat.purchases.Purchases
import com.revenuecat.purchases.PurchasesError
import com.revenuecat.purchases.PurchaseParams
import com.revenuecat.purchases.getCustomerInfoWith
import com.revenuecat.purchases.getOfferingsWith
import com.revenuecat.purchases.interfaces.UpdatedCustomerInfoListener
import com.revenuecat.purchases.models.StoreTransaction
import com.revenuecat.purchases.purchaseWith
import com.revenuecat.purchases.restorePurchasesWith

class RcBilling(private val core: AdPolicy) {

    /** Map a CustomerInfo to premium status and forward it to the Rust core. */
    private fun apply(info: CustomerInfo) {
        val active = info.entitlements[ENTITLEMENT_PREMIUM]?.isActive == true
        // OR-in the debug tester override so an async entitlement fetch can never clobber a
        // local unlock. In release this term is always false (see testerUnlockEnabled).
        core.setPremium(active || testerUnlockEnabled())
    }

    /**
     * INTERNAL DEV ONLY — force premium without a purchase, for local QA on a debug build.
     *
     * It still routes through the single source of truth (`core.setPremium` via [apply]),
     * so it adds no second premium flag. It is double-gated on `BuildConfig.DEBUG` *and* the
     * `TESTER_UNLOCK` build-config flag (true only in the debug build type), so it compiles
     * to a constant `false` in any build uploaded to Google Play — including the closed-test
     * track, which is a *release* build. Closed-test testers are unlocked instead via a
     * RevenueCat promotional grant or Google Play license testing.
     */
    private fun testerUnlockEnabled(): Boolean =
        BuildConfig.DEBUG && BuildConfig.TESTER_UNLOCK

    /**
     * Call once at startup. Performs an initial entitlement fetch and installs a listener so
     * any later change (a purchase completing, a restore, or a lapse) updates the core
     * immediately — no app restart needed.
     */
    fun bindEntitlement() {
        if (testerUnlockEnabled()) core.setPremium(true) // immediate local unlock (debug only)
        Purchases.sharedInstance.getCustomerInfoWith(
            onError = { /* offline / transient: core stays at its last known value */ },
            onSuccess = { info -> apply(info) },
        )
        Purchases.sharedInstance.updatedCustomerInfoListener =
            UpdatedCustomerInfoListener { info -> apply(info) }
    }

    /**
     * Purchase the premium (remove-ads) package from the current RevenueCat offering.
     * On success the listener above also fires, but we apply here too so the UI can react
     * synchronously in [onResult].
     */
    fun purchasePremium(
        activity: Activity,
        onResult: (premium: Boolean, error: PurchasesError?) -> Unit,
    ) {
        Purchases.sharedInstance.getOfferingsWith(
            onError = { error -> onResult(core.isPremium(), error) },
            onSuccess = { offerings ->
                // Prefer the LIFETIME (non-consumable "Full Version / Remove Ads") package
                // explicitly rather than assuming it is first — so adding a tier/subscription
                // to the offering later can't silently change what gets purchased.
                val pkg = offerings.current?.availablePackages
                    ?.firstOrNull { it.packageType == PackageType.LIFETIME }
                    ?: offerings.current?.availablePackages?.firstOrNull()
                if (pkg == null) {
                    onResult(core.isPremium(), null) // misconfigured offering
                    return@getOfferingsWith
                }
                Purchases.sharedInstance.purchaseWith(
                    PurchaseParams.Builder(activity, pkg).build(),
                    onError = { error, _ -> onResult(core.isPremium(), error) },
                    // RevenueCat 8.x hands back a nullable StoreTransaction (null for a
                    // restore / already-owned grant) — take it as `StoreTransaction?`.
                    onSuccess = { _: StoreTransaction?, info: CustomerInfo ->
                        apply(info)
                        onResult(core.isPremium(), null)
                    },
                )
            },
        )
    }

    /**
     * Restore prior purchases. Surface this behind a visible "Restore Purchases" control —
     * both Google and Apple expect freely available restore for non-consumable / lifetime
     * entitlements.
     */
    fun restorePurchases(onResult: (premium: Boolean, error: PurchasesError?) -> Unit) {
        // Use the RevenueCat 8.x KTX helper `restorePurchasesWith` (the base
        // `restorePurchases` takes a single ReceiveCustomerInfoCallback, not named lambdas).
        Purchases.sharedInstance.restorePurchasesWith(
            onError = { error -> onResult(core.isPremium(), error) },
            onSuccess = { info ->
                apply(info)
                onResult(core.isPremium(), null)
            },
        )
    }

    companion object {
        /**
         * The RevenueCat entitlement identifier configured in the RevenueCat dashboard. A
         * single entitlement (unlocked by a non-consumable "remove ads" / full-version
         * purchase) toggles every gate in the core.
         */
        const val ENTITLEMENT_PREMIUM = "premium"
    }
}
