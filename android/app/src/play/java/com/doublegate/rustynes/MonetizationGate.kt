// PLAY-FLAVOR SOURCE SET (v2.0.3, ADR 0025). The real freemium / ad-supported
// monetization façade. The `foss` twin (src/foss/.../MonetizationGate.kt) is a byte-
// compatible no-op that links no ads / store SDK, so the shared `MainActivity`
// (src/main) compiles against either flavor and the FOSS build stays behaviourally
// identical to the pre-monetization build.
//
// This file concentrates every proprietary monetization dependency the shared code must
// NOT see: it initializes AppLovin MAX + RevenueCat (adapting the RustyNesApp shell's
// process-init responsibilities) and owns the `AdPolicy` Rust core, the RevenueCat
// entitlement wrapper (`RcBilling`), and the interstitial / rewarded gates. All policy
// (ad cadence, feature gating, the free-tier play budget, offline grace) lives in the
// shared Rust core so Android and iOS cannot diverge; this class is only the platform
// wiring + the run-out paywall UI.
package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.applovin.sdk.AppLovinMediationProvider
import com.applovin.sdk.AppLovinSdk
import com.applovin.sdk.AppLovinSdkInitializationConfiguration
import com.doublegate.rustynes.monetization.AdGate
import com.doublegate.rustynes.monetization.RcBilling
import com.doublegate.rustynes.monetization.RewardedGate
import com.doublegate.rustynes.monetization.ffi.AdPolicy
import com.doublegate.rustynes.monetization.ffi.PlayProgress
import com.doublegate.rustynes.monetization.ffi.PremiumFeature
import com.doublegate.rustynes.monetization.ffi.defaultAdConfig
import com.revenuecat.purchases.LogLevel
import com.revenuecat.purchases.Purchases
import com.revenuecat.purchases.PurchasesConfiguration
import kotlinx.coroutines.delay

/**
 * The `play`-flavor monetization gate.
 *
 * Construction is cheap (it builds only the Rust [AdPolicy] core, which does no I/O); the
 * heavy SDK initialization (AppLovin, RevenueCat) is deferred to [onActivityCreated] so it
 * stays off the cold-start critical path, mirroring how `MainActivity` defers Play Billing.
 *
 * The core is anchored to `SystemClock.elapsedRealtime()` — the monotonic millisecond clock
 * the [AdGate] / [RewardedGate] also read — so the launch-grace window and ad cooldown share
 * one timebase.
 */
class MonetizationGate(private val appContext: Context) {

    /** The process-wide policy core (a UniFFI object over the pure-Rust `AdPolicy`). */
    private val core: AdPolicy =
        AdPolicy(defaultAdConfig(), SystemClock.elapsedRealtime().toULong())

    /** RevenueCat entitlement wrapper; created once the SDK is configured. */
    private var billing: RcBilling? = null

    /** Interstitial + rewarded gates; created once an Activity is available. */
    private var adGate: AdGate? = null
    private var rewardedGate: RewardedGate? = null

    /** The foreground Activity (needed to launch ads / the purchase dialog). */
    private var activityRef: Activity? = null

    /** Guards one-time SDK init (AppLovin + RevenueCat configure). */
    private var sdksInitialized = false

    private val prefs = appContext.getSharedPreferences("monetization", Context.MODE_PRIVATE)

    /**
     * Begin an app session: increment the persisted app-session index and hand it to the
     * core (drives first-session interstitial suppression + the generous first-session
     * budget). Call once at launch.
     */
    fun beginSession() {
        val next = prefs.getInt(KEY_SESSION_INDEX, 0) + 1
        prefs.edit().putInt(KEY_SESSION_INDEX, next).apply()
        core.beginSession(next.toUInt(), SystemClock.elapsedRealtime().toULong())
    }

    /**
     * One-time SDK init + gate creation, deferred off cold start. Idempotent: the SDKs are
     * configured only on the first call; the ad gates are (re)bound to the current Activity.
     */
    fun onActivityCreated(activity: Activity) {
        activityRef = activity
        if (!sdksInitialized) {
            // The play flavor is always PLAY_BUILD; guard kept for parity with the shell.
            if (BuildConfig.PLAY_BUILD) {
                val initConfig = AppLovinSdkInitializationConfiguration
                    .builder(BuildConfig.APPLOVIN_SDK_KEY, appContext)
                    .setMediationProvider(AppLovinMediationProvider.MAX)
                    .build()
                AppLovinSdk.getInstance(appContext).initialize(initConfig) { /* SDK ready */ }

                Purchases.logLevel = if (BuildConfig.DEBUG) LogLevel.DEBUG else LogLevel.INFO
                Purchases.configure(
                    PurchasesConfiguration.Builder(appContext, BuildConfig.REVENUECAT_API_KEY)
                        .build(),
                )
                billing = RcBilling(core).also { it.bindEntitlement() }
            }
            sdksInitialized = true
        }
        // (Re)create the ad gates bound to this Activity and warm their caches.
        adGate = AdGate(activity, core).also { it.preload() }
        rewardedGate = RewardedGate(activity, core) { /* resume handled by the caller */ }
            .also { it.preload() }
    }

    /** Foreground: re-verify the entitlement against RevenueCat (a purchase/refund elsewhere). */
    fun onResume(activity: Activity) {
        activityRef = activity
        billing?.bindEntitlement()
    }

    /** Drop the held Activity so a destroyed Activity is never touched. */
    fun onDestroy() {
        activityRef = null
    }

    /** Whether [feature] is unlocked for the current entitlement (delegates to the core). */
    fun featureEnabled(feature: PremiumFeature): Boolean = core.featureEnabled(feature)

    /** Reset the per-game free-tier play budget (call when a ROM loads). */
    fun startPlay() = core.startPlay()

    /** Feed elapsed unpaused play time into the free-tier budget. */
    fun addActiveTime(deltaMs: Long) = core.addActiveTime(deltaMs.toULong())

    /** Whether play is currently allowed (false once a free user's budget is exhausted). */
    fun isPlayAllowed(): Boolean = core.isPlayAllowed()

    /**
     * Persist the current free-tier play progress keyed by [romKey], so backgrounding then
     * relaunching a free session cannot reset the budget. No-op for premium (nothing to gate).
     */
    fun exportProgress(romKey: String) {
        val p = core.exportProgress()
        prefs.edit()
            .putLong(progressKey(romKey, "budget"), p.budgetMs.toLong())
            .putLong(progressKey(romKey, "consumed"), p.consumedMs.toLong())
            .putInt(progressKey(romKey, "grants"), p.rewardGrantsThisSession.toInt())
            .putBoolean(progressKey(romKey, "grace"), p.offlineGraceUsed)
            .apply()
    }

    /** Restore a previously [exportProgress]'d snapshot for [romKey] (no-op if none stored). */
    fun restoreProgress(romKey: String) {
        if (!prefs.contains(progressKey(romKey, "budget"))) return
        core.restoreProgress(
            PlayProgress(
                budgetMs = prefs.getLong(progressKey(romKey, "budget"), 0L).toULong(),
                consumedMs = prefs.getLong(progressKey(romKey, "consumed"), 0L).toULong(),
                rewardGrantsThisSession = prefs.getInt(progressKey(romKey, "grants"), 0).toUInt(),
                offlineGraceUsed = prefs.getBoolean(progressKey(romKey, "grace"), false),
            ),
        )
    }

    /**
     * The run-out paywall + countdown overlay.
     *
     * A free user sees a live mm:ss countdown of their remaining budget; when it hits zero
     * a modal offers the three continuation paths the core arbitrates — watch a rewarded ad
     * for more time (only while `canOfferRewarded()`), buy the Full Version (RevenueCat), or
     * a one-time offline-grace continuation when no ad is available. Premium users and the
     * FOSS build (its twin) never draw anything.
     *
     * Drive-by-tick: the [AdPolicy] core is not Compose-observable, so a 1 Hz [LaunchedEffect]
     * samples `playTimeRemainingMs()` into Compose state — cheap, and keeps the overlay's
     * recomposition scope tight (only the countdown text and the paywall visibility read it).
     *
     * @param onResume invoked after a granted reward / purchase / grace so the caller resumes
     *   the paused emulator.
     */
    @Composable
    fun RunOutOverlay(onResume: () -> Unit) {
        var remainingMs by remember { mutableLongStateOf(0L) }
        var premium by remember { mutableStateOf(core.isPremium()) }
        LaunchedEffect(Unit) {
            while (true) {
                premium = core.isPremium()
                remainingMs = core.playTimeRemainingMs()?.toLong() ?: Long.MAX_VALUE
                delay(1000L)
            }
        }
        // Premium (or an unlimited/unmetered session): draw nothing.
        if (premium || remainingMs == Long.MAX_VALUE) return

        // Still within budget: a lightweight top-anchored countdown (mm:ss) so the free
        // user sees time ticking down toward the run-out paywall.
        if (core.isPlayAllowed()) {
            Column(
                modifier = Modifier.fillMaxSize().padding(top = 12.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    text = stringResource(R.string.paywall_countdown, formatMmSs(remainingMs)),
                    style = MaterialTheme.typography.labelLarge,
                    color = Color.White,
                )
            }
            return
        }

        // Budget exhausted: the full run-out paywall modal.
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = 0.85f))
                .padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = stringResource(R.string.paywall_runout_title),
                style = MaterialTheme.typography.headlineSmall,
                color = Color.White,
            )
            Text(
                text = stringResource(R.string.paywall_runout_body),
                style = MaterialTheme.typography.bodyMedium,
                color = Color.White,
            )
            if (core.canOfferRewarded()) {
                Button(onClick = {
                    // A granted reward resumes via the gate's callback; the boolean tells us
                    // whether an ad was actually ready (else fall through to purchase/grace).
                    if (rewardedGate?.show() != true) { /* no ad ready: user can buy/grace */ }
                }) {
                    Text(stringResource(R.string.paywall_watch_ad))
                }
            }
            if (core.canGrantOfflineGrace()) {
                Button(onClick = {
                    if (core.grantOfflineGrace()) onResume()
                }) {
                    Text(stringResource(R.string.paywall_offline_grace))
                }
            }
            Button(onClick = {
                val act = activityRef ?: return@Button
                billing?.purchasePremium(act) { isPremium, _ ->
                    if (isPremium) {
                        premium = true
                        onResume()
                    }
                }
            }) {
                Text(stringResource(R.string.paywall_full_version))
            }
        }
    }

    private fun progressKey(romKey: String, field: String): String = "prog_${romKey}_$field"

    /** Format a non-negative millisecond duration as `m:ss` for the countdown chip. */
    private fun formatMmSs(ms: Long): String {
        val totalSeconds = (ms.coerceAtLeast(0L)) / 1000L
        return "%d:%02d".format(totalSeconds / 60L, totalSeconds % 60L)
    }

    companion object {
        private const val KEY_SESSION_INDEX = "app_session_index"
    }
}
