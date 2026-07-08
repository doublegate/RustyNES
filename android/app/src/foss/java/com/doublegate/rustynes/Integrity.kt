// FOSS-FLAVOR SOURCE SET (v2.0.1, ADR 0025). No-op stand-in for the Play-Integrity
// `IntegrityManager` (the real one is in `src/play/.../Integrity.kt`). Links NO
// `com.google.android.play.core.integrity.*`. Anti-tamper attestation is a Play-only
// concern (and pointless on an open, freely-redistributable FOSS build), so this never
// requests a token and always reports [IntegrityVerdict.UNKNOWN] — which the app treats
// as "no signal". The shared `IntegrityVerdict` enum lives in `PlayFacadeShared.kt`.
package com.doublegate.rustynes

import android.content.Context

/** No-op FOSS integrity manager: never contacts Play Integrity; verdict is always UNKNOWN. */
@Suppress("UNUSED_PARAMETER")
class IntegrityManager(context: Context) {

    fun prepareToken() {}

    fun request(requestHash: String, onToken: (String) -> Unit) {}

    fun evaluateStub(token: String): IntegrityVerdict = IntegrityVerdict.UNKNOWN
}
