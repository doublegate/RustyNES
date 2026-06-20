package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.android.billingclient.api.AcknowledgePurchaseParams
import com.android.billingclient.api.BillingClient
import com.android.billingclient.api.BillingClientStateListener
import com.android.billingclient.api.BillingResult
import com.android.billingclient.api.PendingPurchasesParams
import com.android.billingclient.api.ProductDetails
import com.android.billingclient.api.Purchase
import com.android.billingclient.api.PurchasesUpdatedListener
import com.android.billingclient.api.QueryProductDetailsParams
import com.android.billingclient.api.QueryPurchasesParams

/** The one-time, non-consumable "Full Unlock" product id (set up in Play Console). */
const val FULL_UNLOCK_PRODUCT = "full_unlock"

/** Free-tier demo session length: 10 minutes (shortened in debug for testing). */
val DEMO_SESSION_SECONDS: Int = if (BuildConfig.DEBUG) 60 else 600

/**
 * Owns the freemium entitlement (Workstream M).
 *
 * Free download + a one-time, non-consumable in-app purchase ("Full Unlock",
 * $2.99) via Play Billing. [isUnlocked] is Compose-observable; the shell reads it
 * to gate the demo (save-states / resume / SRAM persistence + the session timer).
 *
 * The local cache (`SharedPreferences`) makes the unlocked state available
 * instantly and offline, but Play is the source of truth: every connection
 * re-queries `queryPurchasesAsync`, so a refund/clear flips the entitlement back.
 * A non-consumable purchase is owned forever and restored automatically across
 * reinstall / new device (no server needed).
 */
class LicenseManager(private val appContext: Context) {

    /** True once the Full Unlock is owned (or forced in a debug build). */
    var isUnlocked by mutableStateOf(false)
        private set

    /** The fetched product (for its localized price + the purchase flow). */
    var product by mutableStateOf<ProductDetails?>(null)
        private set

    private val prefs = appContext.getSharedPreferences("license", Context.MODE_PRIVATE)

    private val purchasesListener = PurchasesUpdatedListener { result, purchases ->
        if (result.responseCode == BillingClient.BillingResponseCode.OK && purchases != null) {
            purchases.forEach(::handlePurchase)
        }
    }

    private val client: BillingClient = BillingClient.newBuilder(appContext)
        .setListener(purchasesListener)
        .enablePendingPurchases(
            PendingPurchasesParams.newBuilder().enableOneTimeProducts().build(),
        )
        .build()

    init {
        // Optimistic offline value; Play re-verifies on connect.
        isUnlocked = prefs.getBoolean("unlocked", false)
    }

    /** Connect to Play and refresh the product + entitlement. Idempotent. */
    fun connect() {
        if (client.connectionState == BillingClient.ConnectionState.CONNECTED) {
            queryProduct(); refreshEntitlement(); return
        }
        client.startConnection(object : BillingClientStateListener {
            override fun onBillingSetupFinished(result: BillingResult) {
                if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                    queryProduct()
                    refreshEntitlement()
                }
            }
            override fun onBillingServiceDisconnected() {
                Log.i("RustyNES", "Billing disconnected")
            }
        })
    }

    /** Re-query owned purchases (also the "Restore purchase" action). */
    fun refreshEntitlement() {
        client.queryPurchasesAsync(
            QueryPurchasesParams.newBuilder()
                .setProductType(BillingClient.ProductType.INAPP)
                .build(),
        ) { _, purchases ->
            val owned = purchases.any {
                it.products.contains(FULL_UNLOCK_PRODUCT) &&
                    it.purchaseState == Purchase.PurchaseState.PURCHASED
            }
            applyUnlocked(owned)
            purchases.forEach(::handlePurchase)
        }
    }

    private fun queryProduct() {
        val params = QueryProductDetailsParams.newBuilder()
            .setProductList(
                listOf(
                    QueryProductDetailsParams.Product.newBuilder()
                        .setProductId(FULL_UNLOCK_PRODUCT)
                        .setProductType(BillingClient.ProductType.INAPP)
                        .build(),
                ),
            )
            .build()
        client.queryProductDetailsAsync(params) { _, result ->
            product = result.productDetailsList.firstOrNull()
        }
    }

    /** Launch the Play purchase flow for the Full Unlock. */
    fun purchase(activity: Activity) {
        val details = product ?: return
        val flowParams = com.android.billingclient.api.BillingFlowParams.newBuilder()
            .setProductDetailsParamsList(
                listOf(
                    com.android.billingclient.api.BillingFlowParams.ProductDetailsParams
                        .newBuilder()
                        .setProductDetails(details)
                        .build(),
                ),
            )
            .build()
        client.launchBillingFlow(activity, flowParams)
    }

    private fun handlePurchase(p: Purchase) {
        if (p.products.contains(FULL_UNLOCK_PRODUCT) &&
            p.purchaseState == Purchase.PurchaseState.PURCHASED
        ) {
            applyUnlocked(true)
            // A non-consumable purchase must be acknowledged within 3 days or
            // Play auto-refunds it.
            if (!p.isAcknowledged) {
                client.acknowledgePurchase(
                    AcknowledgePurchaseParams.newBuilder()
                        .setPurchaseToken(p.purchaseToken)
                        .build(),
                ) { /* acknowledged */ }
            }
        }
    }

    private fun applyUnlocked(value: Boolean) {
        isUnlocked = value
        prefs.edit().putBoolean("unlocked", value).apply()
    }

    /**
     * Debug-only override so the demo gating + unlock UI can be exercised on a
     * sideloaded build without a Play Console / license-tested account (the real
     * purchase flow can't run on a sideloaded APK). No-op in release.
     */
    fun debugForceUnlocked(value: Boolean) {
        if (BuildConfig.DEBUG) applyUnlocked(value)
    }
}
