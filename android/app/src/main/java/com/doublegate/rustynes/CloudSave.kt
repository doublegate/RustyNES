package com.doublegate.rustynes

import android.content.Context
import android.util.Log
import com.google.android.gms.games.SnapshotsClient
import com.google.android.gms.games.snapshot.Snapshot
import com.google.android.gms.games.snapshot.SnapshotMetadataChange

/**
 * Play Games Services v2 cloud-save sync (v1.8.8 "Atlas", Workstream D).
 *
 * Syncs the existing per-ROM `.rns` save-states (the SHA-256-keyed
 * `states/<rom-sha256>/<slot>.rns` blobs written by [SaveStateStore]) to the cloud as
 * **PGS Snapshots**. The `.rns` format is platform-independent, so this also makes
 * desktop⇄Android save portability a first-class feature, not just a manual copy.
 *
 * PREPPED BEHIND A DEFAULT-OFF FLAG. Nothing here touches the Snapshots SDK unless
 * [BuildConfig.PGS_ENABLED] is true AND the user is signed in to PGS
 * ([PlayGamesManager.isSignedIn]) AND the in-app "Cloud saves" toggle is on. With the
 * flag off, every method is a cheap no-op and local saves are untouched.
 *
 * ## The cloud-save model (the PGS "independently-updatable units" best practice)
 * We do NOT sync one monolithic blob. Each (ROM-SHA, slot) pair is its own Snapshot —
 * the documented "divide saves into independently-updatable units" pattern — so a
 * save to one slot only uploads that slot, minimizing IO and conflict surface. The
 * Snapshot's unique name is derived deterministically from the ROM-SHA + slot
 * ([snapshotName]); it is the same on every device, so the same logical save resolves
 * to the same Snapshot.
 *
 * ## Conflict resolution
 * `open(...)` is called with `RESOLUTION_POLICY_MOST_RECENTLY_MODIFIED`, so the common
 * case (no divergence, or one side strictly newer) resolves automatically to the most
 * recently modified copy — a sensible **last-write-wins** for a single user across
 * their own devices. On a true divergent conflict the SDK hands BOTH copies to us via
 * `DataOrConflict.getConflict()`; we surface that to the caller through [onConflict]
 * with a [SaveConflict] offering **keep-local** / **keep-cloud**, then call
 * `resolveConflict`. (A richer 3-way merge UI is a deliberate TODO — last-write-wins +
 * the explicit keep-local/keep-cloud picker is the solid first version.)
 *
 * Snapshot APIs are blocking Tasks; run [pushSlot]/[pullSlot] off the main thread
 * (the callers already use the SAF/background executor pattern).
 */

/** A divergent cloud-save conflict surfaced to the UI (Workstream D). The caller picks
 *  a side and the chosen bytes are committed back to resolve it. */
data class SaveConflict(
    val conflictId: String,
    val sha: String,
    val slot: String,
    /** The locally-modified copy's bytes (this device). */
    val localBytes: ByteArray,
    /** The server copy's bytes (another device / a prior sync). */
    val cloudBytes: ByteArray,
    /** The Snapshot the chosen bytes are written into + committed via `resolveConflict`. */
    internal val resolutionSnapshot: Snapshot,
)

class CloudSaveManager(
    context: Context,
    private val pgs: PlayGamesManager,
) {
    private val appContext = context.applicationContext

    /** Whether cloud sync should run: the build flag, PGS sign-in, and the user toggle
     *  must all be true. The user toggle lives in [AppSettings.cloudSavesEnabled]. */
    fun isActive(settings: AppSettings): Boolean =
        BuildConfig.PGS_ENABLED && pgs.isSignedIn && settings.cloudSavesEnabled

    /**
     * The deterministic, cross-device Snapshot name for a (ROM-SHA, slot) unit.
     *
     * PGS Snapshot names must be 1..100 chars of [a-zA-Z0-9._-]. A SHA-256 hex (64
     * chars) + the short slot + a fixed prefix stays well under 100 and is already in
     * the allowed character set, so it needs no further sanitizing. Same logical save
     * => same name on every device => the same Snapshot.
     */
    private fun snapshotName(sha: String, slot: String): String = "rns.$sha.$slot"

    /**
     * Push a local `.rns` slot to the cloud as its own Snapshot (one independently-
     * updatable unit). On a divergent conflict, [onConflict] is invoked with both
     * copies and resolution is deferred to the user's choice. No-op when inactive or
     * the local slot is empty. [onDone] reports success (true) once committed.
     */
    fun pushSlot(
        sha: String,
        slot: String,
        settings: AppSettings,
        onConflict: (SaveConflict) -> Unit = {},
        onDone: (Boolean) -> Unit = {},
    ) {
        if (!isActive(settings)) { onDone(false); return }
        val bytes = SaveStateStore.load(appContext, sha, slot)
        if (bytes == null || bytes.isEmpty()) { onDone(false); return }
        val client = pgs.snapshotsClientOrNull()
        if (client == null) { onDone(false); return }
        client.open(snapshotName(sha, slot), true, SnapshotsClient.RESOLUTION_POLICY_MOST_RECENTLY_MODIFIED)
            .addOnSuccessListener { result ->
                if (result.isConflict) {
                    surfaceConflict(sha, slot, result.conflict, onConflict)
                    onDone(false)
                    return@addOnSuccessListener
                }
                val snapshot = result.data ?: run { onDone(false); return@addOnSuccessListener }
                snapshot.snapshotContents.writeBytes(bytes)
                client.commitAndClose(
                    snapshot,
                    SnapshotMetadataChange.Builder()
                        .setDescription("RustyNES save · slot $slot")
                        .build(),
                ).addOnSuccessListener { onDone(true) }
                    .addOnFailureListener { onDone(false) }
            }
            .addOnFailureListener {
                Log.w("RustyNES", "Cloud push failed for $sha/$slot", it)
                onDone(false)
            }
    }

    /**
     * Pull a cloud Snapshot down into the local `.rns` slot (e.g. on first open of a
     * ROM on a new device). On a divergent conflict, [onConflict] is invoked. [onDone]
     * reports whether a non-empty cloud copy was written locally. No-op when inactive.
     */
    fun pullSlot(
        sha: String,
        slot: String,
        settings: AppSettings,
        onConflict: (SaveConflict) -> Unit = {},
        onDone: (Boolean) -> Unit = {},
    ) {
        if (!isActive(settings)) { onDone(false); return }
        val client = pgs.snapshotsClientOrNull()
        if (client == null) { onDone(false); return }
        // createIfMissing=false: pulling a non-existent cloud save is just "nothing to do".
        client.open(snapshotName(sha, slot), false, SnapshotsClient.RESOLUTION_POLICY_MOST_RECENTLY_MODIFIED)
            .addOnSuccessListener { result ->
                if (result.isConflict) {
                    surfaceConflict(sha, slot, result.conflict, onConflict)
                    onDone(false)
                    return@addOnSuccessListener
                }
                val snapshot = result.data ?: run { onDone(false); return@addOnSuccessListener }
                val bytes = runCatching { snapshot.snapshotContents.readFully() }.getOrNull()
                // Close the read-only open without committing a change.
                runCatching { client.discardAndClose(snapshot) }
                if (bytes != null && bytes.isNotEmpty()) {
                    SaveStateStore.save(appContext, sha, slot, bytes)
                    onDone(true)
                } else {
                    onDone(false)
                }
            }
            .addOnFailureListener {
                // A missing snapshot surfaces as a failure here; that is benign (no cloud copy yet).
                onDone(false)
            }
    }

    /** Map an SDK SnapshotConflict to our [SaveConflict] and hand it to the UI.
     *  `getSnapshot()` is the server copy, `getConflictingSnapshot()` is this device's
     *  local copy; the server snapshot doubles as the handle we write the chosen bytes
     *  into and commit via `resolveConflict`. */
    private fun surfaceConflict(
        sha: String,
        slot: String,
        conflict: SnapshotsClient.SnapshotConflict?,
        onConflict: (SaveConflict) -> Unit,
    ) {
        if (conflict == null) return
        val serverSnapshot = conflict.snapshot
        val server = runCatching { serverSnapshot.snapshotContents.readFully() }.getOrNull() ?: ByteArray(0)
        val local = runCatching { conflict.conflictingSnapshot.snapshotContents.readFully() }.getOrNull() ?: ByteArray(0)
        onConflict(
            SaveConflict(
                conflictId = conflict.conflictId,
                sha = sha,
                slot = slot,
                localBytes = local,
                cloudBytes = server,
                resolutionSnapshot = serverSnapshot,
            ),
        )
    }

    /**
     * Resolve a surfaced conflict by committing the user's chosen bytes (keep-local or
     * keep-cloud). The chosen bytes are written into the SDK-provided resolution
     * Snapshot and committed against the conflict id. Also writes the choice locally so
     * the device's `.rns` matches what was kept.
     */
    fun resolveConflict(conflict: SaveConflict, keepLocal: Boolean, onDone: (Boolean) -> Unit = {}) {
        if (!BuildConfig.PGS_ENABLED || !pgs.isSignedIn) { onDone(false); return }
        val client = pgs.snapshotsClientOrNull()
        if (client == null) { onDone(false); return }
        val chosen = if (keepLocal) conflict.localBytes else conflict.cloudBytes
        conflict.resolutionSnapshot.snapshotContents.writeBytes(chosen)
        client.resolveConflict(conflict.conflictId, conflict.resolutionSnapshot)
            .addOnSuccessListener {
                SaveStateStore.save(appContext, conflict.sha, conflict.slot, chosen)
                onDone(true)
            }
            .addOnFailureListener { onDone(false) }
    }
}
