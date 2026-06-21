package com.doublegate.rustynes

import android.content.Context
import android.util.Log
import com.google.android.play.core.integrity.IntegrityManagerFactory
import com.google.android.play.core.integrity.StandardIntegrityManager
import com.google.android.play.core.integrity.StandardIntegrityManager.PrepareIntegrityTokenRequest
import com.google.android.play.core.integrity.StandardIntegrityManager.StandardIntegrityToken
import com.google.android.play.core.integrity.StandardIntegrityManager.StandardIntegrityTokenRequest

/**
 * Play Integrity API client (v1.8.8 "Atlas", Workstream L) — the anti-tamper /
 * anti-piracy layer that confirms a genuine, uncompromised, Play-recognized binary
 * BEFORE honoring/restoring the Full Unlock. (SafetyNet Attestation was turned down
 * January 2025; Play Integrity is the modern replacement.)
 *
 * PREPPED BEHIND A DEFAULT-OFF FLAG. Nothing here touches the Integrity SDK unless
 * [BuildConfig.PLAY_INTEGRITY_ENABLED] is true AND a non-zero
 * [BuildConfig.INTEGRITY_CLOUD_PROJECT_NUMBER] is set. With the flag off (the default,
 * and on every sideload build), [request] is a cheap no-op.
 *
 * ## Defense-in-depth, NOT the entitlement source of truth
 * **Play Billing stays the source of truth** for the $2.99 Full Unlock
 * (`queryPurchasesAsync` in [LicenseManager]). Integrity is a LAYER over it: a failed
 * or absent verdict must NEVER revoke a legitimate purchase — at worst it is a signal
 * the maintainer's server can weigh when deciding whether to honor a *restore*. The
 * app never blocks function on the verdict.
 *
 * ## Why the verdict handler is a STUB (maintainer ops)
 * Play Integrity returns an **encrypted, signed token**. Decrypting + verifying it
 * requires the maintainer's **linked Google Cloud project + a server endpoint** (the
 * verdict is meant to be evaluated server-side; never trusted on-device). This client
 * requests the token and hands the opaque string to [onToken]; the
 * `MEETS_DEVICE_INTEGRITY` / `PLAY_RECOGNIZED` / `appLicensingVerdict == LICENSED`
 * checks live in that server, which the maintainer wires up. The on-device
 * [evaluateStub] is a clearly-marked placeholder that returns [IntegrityVerdict.UNKNOWN].
 *
 * Uses the **Standard** request (warmed [prepareToken], replay-protected, few-hundred-
 * ms) per the Play Integrity guidance, not the deprecated Classic request.
 */

/** The (server-side, decrypted) integrity verdict outcome, as the app reasons about it.
 *  On-device this is always [UNKNOWN] until the maintainer's decryption endpoint lands. */
enum class IntegrityVerdict {
    /** Verdict not yet available (flag off, no cloud project, or no server endpoint). */
    UNKNOWN,

    /** Server confirmed a genuine, Play-recognized, licensed binary. */
    GENUINE,

    /** Server flagged a tampered / unrecognized / unlicensed binary. */
    TAMPERED,
}

class IntegrityManager(context: Context) {
    private val appContext = context.applicationContext

    /** The warmed token provider (Standard request). Null until [prepareToken] succeeds. */
    @Volatile
    private var tokenProvider: StandardIntegrityManager.StandardIntegrityTokenProvider? = null

    private val cloudProjectNumber: Long = BuildConfig.INTEGRITY_CLOUD_PROJECT_NUMBER

    /** Whether the client is configured to run at all (flag + a real cloud project). */
    private fun configured(): Boolean =
        BuildConfig.PLAY_INTEGRITY_ENABLED && cloudProjectNumber != 0L

    /**
     * Warm up the Standard integrity token provider (a server call; do it once, off the
     * cold-start path). No-op when not configured. Cheap to call again — it just
     * refreshes internal state.
     */
    fun prepareToken() {
        if (!configured()) return
        runCatching {
            val manager: StandardIntegrityManager = IntegrityManagerFactory.createStandard(appContext)
            manager.prepareIntegrityToken(
                PrepareIntegrityTokenRequest.builder()
                    .setCloudProjectNumber(cloudProjectNumber)
                    .build(),
            ).addOnSuccessListener { provider -> tokenProvider = provider }
                .addOnFailureListener { Log.w("RustyNES", "Integrity prepare failed", it) }
        }
    }

    /**
     * Request an integrity token for a [requestHash] (bind it to the action being
     * protected, e.g. a Full-Unlock restore; the server replays this hash). [onToken]
     * receives the opaque, encrypted token string for the maintainer's server to
     * decrypt + evaluate. No-op (does not call back) when not configured / not warmed.
     */
    fun request(requestHash: String, onToken: (String) -> Unit) {
        if (!configured()) return
        val provider = tokenProvider ?: run {
            // Not warmed yet — warm it and skip this request (the caller can retry).
            prepareToken()
            return
        }
        runCatching {
            provider.request(
                StandardIntegrityTokenRequest.builder()
                    .setRequestHash(requestHash)
                    .build(),
            ).addOnSuccessListener { response: StandardIntegrityToken ->
                onToken(response.token())
            }.addOnFailureListener { Log.w("RustyNES", "Integrity request failed", it) }
        }
    }

    /**
     * MAINTAINER-OPS STUB. The real verdict is produced by decrypting [token] on the
     * maintainer's server (the linked Cloud project's verdict-decryption endpoint) and
     * checking `MEETS_DEVICE_INTEGRITY` + `PLAY_RECOGNIZED` + `appLicensingVerdict`.
     * On-device we cannot (and must not) decrypt it, so this always returns
     * [IntegrityVerdict.UNKNOWN]. The app treats UNKNOWN as "no signal" — Billing
     * remains the entitlement truth, so nothing is revoked.
     *
     * To wire it up, the maintainer POSTs [token] to their endpoint and maps the
     * decrypted verdict back to GENUINE / TAMPERED here.
     */
    @Suppress("UNUSED_PARAMETER")
    fun evaluateStub(token: String): IntegrityVerdict = IntegrityVerdict.UNKNOWN
}
