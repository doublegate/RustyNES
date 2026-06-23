/*
 * RustyNesApp.kt — Android Application entry point.
 *
 * Responsibilities:
 *   1. Construct the single shared `AdPolicy` from the Rust core (UniFFI binding).
 *   2. Initialize the AppLovin MAX SDK as early as possible (per AppLovin guidance,
 *      this maximizes ad pre-caching time and improves fill).
 *   3. Configure RevenueCat and immediately bind the premium entitlement into the
 *      core so the very first ad decision already reflects the user's paid status.
 *
 * The objects created here (core, billing, adGate) are process-wide singletons,
 * exposed via the Application instance so Activities can reach them. In a larger app
 * prefer Hilt/Koin; kept explicit here for a self-contained skeleton.
 *
 * Required Gradle dependencies (see build.gradle.kts):
 *   com.applovin:applovin-sdk        — MAX mediation SDK (init API used below)
 *   com.revenuecat.purchases:purchases — RevenueCat (entitlement source of truth)
 * Plus the UniFFI-generated `com.doublegate.rustynes.monetization.ffi` package and the native librustynes_core
 * .so files under src/main/jniLibs (produced by cargo-ndk — see README).
 */
package com.doublegate.rustynes.monetization

import com.doublegate.rustynes.BuildConfig

import android.app.Application
import android.os.SystemClock
import com.doublegate.rustynes.monetization.ffi.AdPolicy
import com.doublegate.rustynes.monetization.ffi.defaultAdConfig
import com.applovin.sdk.AppLovinMediationProvider
import com.applovin.sdk.AppLovinSdk
import com.applovin.sdk.AppLovinSdkInitializationConfiguration
import com.revenuecat.purchases.LogLevel
import com.revenuecat.purchases.Purchases
import com.revenuecat.purchases.PurchasesConfiguration

class RustyNesApp : Application() {

    /** Shared monetization core. `defaultAdConfig()` and `AdPolicy(...)` are UniFFI bindings. */
    lateinit var core: AdPolicy
        private set

    /** RevenueCat wrapper that keeps [core]'s premium flag current. */
    lateinit var billing: Billing
        private set

    override fun onCreate() {
        super.onCreate()

        // (1) Build the policy core. SystemClock.elapsedRealtime() is the monotonic
        // millisecond clock the Rust core expects; the core stores this as the launch
        // anchor for its grace window. The Rust `u64` maps to Kotlin `ULong`.
        core = AdPolicy(defaultAdConfig(), SystemClock.elapsedRealtime().toULong())

        // (2) Initialize AppLovin MAX with the current (config-builder) init API.
        // Do this before loading any ad. The SDK key comes from the AppLovin dashboard
        // (Account > General > Keys). The MAX mediation provider must be set explicitly.
        val initConfig = AppLovinSdkInitializationConfiguration
            .builder(BuildConfig.APPLOVIN_SDK_KEY, this)
            .setMediationProvider(AppLovinMediationProvider.MAX)
            .build()
        AppLovinSdk.getInstance(this).initialize(initConfig) { _ ->
            // SDK ready — Activities may now preload/show interstitials via AdGate.
        }

        // (3) Configure RevenueCat and wire the entitlement → core. Use the Android
        // (Google) public API key. Premium status is pushed into the core both now
        // (initial fetch) and on every future change (purchase / restore / expiry).
        Purchases.logLevel = LogLevel.DEBUG // drop to .INFO for release builds
        Purchases.configure(
            PurchasesConfiguration.Builder(this, BuildConfig.REVENUECAT_API_KEY).build()
        )
        billing = Billing(core)
        billing.bindEntitlement()
    }

    companion object {
        /**
         * The RevenueCat entitlement identifier configured in the RevenueCat
         * dashboard. A single entitlement (e.g. unlocked by a non-consumable
         * "remove ads" / full-version purchase) toggles every gate in the core.
         */
        const val ENTITLEMENT_PREMIUM = "premium"
    }
}
