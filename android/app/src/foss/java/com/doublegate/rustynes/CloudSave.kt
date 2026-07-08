// FOSS-FLAVOR SOURCE SET (v2.0.1, ADR 0025). No-op stand-in for the Play-Games-Snapshots
// `CloudSaveManager` (the real one is in `src/play/.../CloudSave.kt`). Links NO
// `com.google.android.gms.games.*`. Cloud save-sync is a Play-Games feature, so it is
// simply absent in FOSS; local save-states are unaffected (they go through the shared
// `SaveStateStore`). Public surface — including a Snapshot-free `SaveConflict` — matches
// the `play` twin so the shared conflict-picker UI in `MainActivity` compiles unchanged
// (it treats `SaveConflict` opaquely: it holds one and hands it back to `resolveConflict`).
package com.doublegate.rustynes

import android.content.Context

/**
 * FOSS `SaveConflict`: the same public shape the `play` twin exposes, minus the internal
 * Google `Snapshot` resolution handle (there is no Snapshots backend here). `MainActivity`
 * only ever holds one of these and passes it back to [CloudSaveManager.resolveConflict],
 * so its fields being inert is invisible. A conflict is never actually surfaced in FOSS.
 */
data class SaveConflict(
    val conflictId: String,
    val sha: String,
    val slot: String,
    /** The locally-modified copy's bytes (this device). */
    val localBytes: ByteArray,
    /** The server copy's bytes — always empty in FOSS (no cloud). */
    val cloudBytes: ByteArray,
) {
    // ByteArray members need structural equals/hashCode; generated component equality is
    // reference-based otherwise. FOSS never compares SaveConflicts, but define them so the
    // data class matches the play twin's contract and the compiler stays quiet.
    override fun equals(other: Any?): Boolean = this === other

    override fun hashCode(): Int = System.identityHashCode(this)
}

/** No-op FOSS cloud-save manager: never active; every push/pull/resolve reports failure. */
@Suppress("UNUSED_PARAMETER")
class CloudSaveManager(
    context: Context,
    private val pgs: PlayGamesManager,
) {
    /** Cloud sync is never active in FOSS. */
    fun isActive(settings: AppSettings): Boolean = false

    fun pushSlot(
        sha: String,
        slot: String,
        settings: AppSettings,
        onConflict: (SaveConflict) -> Unit = {},
        onDone: (Boolean) -> Unit = {},
    ) {
        onDone(false)
    }

    fun pullSlot(
        sha: String,
        slot: String,
        settings: AppSettings,
        onConflict: (SaveConflict) -> Unit = {},
        onDone: (Boolean) -> Unit = {},
    ) {
        onDone(false)
    }

    fun resolveConflict(conflict: SaveConflict, keepLocal: Boolean, onDone: (Boolean) -> Unit = {}) {
        onDone(false)
    }
}
