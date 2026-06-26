//
//  CloudSaveStateSync.swift
//
//  iCloud (CloudKit) sync of the per-ROM `.rns` save-state slots across a user's
//  devices (v1.9.7). This is the HEAVY counterpart to the v1.9.5 CloudConfigSync,
//  which only mirrors small KV config; save-state blobs are far larger, so they ride
//  CloudKit's private database (one record per slot) rather than the KV store.
//
//  Design:
//    * One record per slot, keyed by ROM SHA-256 + slot index: recordName
//      "state-<sha>-<n>", record type "SaveState", default zone of the user's PRIVATE
//      database. Fields: sha (String), slot (Int64), savedAt (Date), frame (Int64),
//      blob (CKAsset = the .rns file), thumbnail (CKAsset, optional).
//    * On save -> upload in the background (force-overwrite, client-wins).
//    * On game open / app launch -> fetch the (up to four) known record IDs and
//      reconcile: pull any slot whose remote `savedAt` is newer than the local copy
//      into the sandbox (last-writer-wins by timestamp). Local-newer slots stay as-is
//      (already uploaded at save time).
//    * Per-slot status (synced / uploading / local-only / unavailable) is published
//      for the SaveStatesView indicator.
//
//  It is OPT-IN (off by default) and GRACEFUL when iCloud/CloudKit is unavailable (no
//  account, offline, capability not provisioned): it never blocks or fails a local
//  save / load -- uploads are fire-and-forget and reconciliation silently no-ops.
//
//  Heavy work stays OFF the main thread: CloudKit's async APIs suspend rather than
//  block, and the blob file copies run in a detached task. This @MainActor object only
//  holds the published status map (so SwiftUI observes it directly) and hops back to
//  the main actor to update it.
//
//  MAINTAINER CARRYOVER (cannot be done from Linux / the checked-in project alone):
//  the iCloud + CloudKit capability must be enabled for the App ID and the CloudKit
//  container created in the Apple Developer account (see RustyNES.entitlements). The
//  "SaveState" record type is created automatically on first upload in the CloudKit
//  development environment; promote the schema to production before release. Until the
//  container exists this object reports `unavailable` and stays out of the way.
//

import CloudKit
import Foundation

@MainActor
final class CloudSaveStateSync: ObservableObject {
    /// Per-slot sync state for the SaveStatesView indicator.
    enum SlotSyncState: Equatable {
        /// Sync disabled, or iCloud/CloudKit not available (no account / offline).
        case unavailable
        /// A local save exists but is not known to be in the cloud (yet / failed).
        case localOnly
        /// An upload is in flight.
        case uploading
        /// The local and cloud copies are reconciled.
        case synced
    }

    private static let recordType = "SaveState"
    private static let enabledKey = "cloudSaveStates"

    private let saveStates: SaveStateManager

    /// Per-slot status for the CURRENT game only (reset when the game changes).
    @Published private(set) var states: [Int: SlotSyncState] = [:]

    /// Whether the user's iCloud account is usable. Re-checked on enable / launch.
    @Published private(set) var accountAvailable = false

    /// Opt-in master toggle (persisted). Off by default. Toggling on re-checks the
    /// account and reconciles the current game.
    @Published var enabled: Bool {
        didSet {
            guard enabled != oldValue else { return }
            UserDefaults.standard.set(enabled, forKey: Self.enabledKey)
            Task { await refreshAccountAndReconcile() }
        }
    }

    /// The SHA of the game whose slots `states` describes (nil on the library screen).
    private var currentSha: String?

    init(saveStates: SaveStateManager) {
        self.saveStates = saveStates
        self.enabled = UserDefaults.standard.bool(forKey: Self.enabledKey)
    }

    // MARK: - Lifecycle wiring (driven by AppModel)

    /// Kick off an account check at launch (no game yet). Safe to call when disabled.
    func start() {
        Task { await refreshAccount() }
    }

    /// Switch to a new game's slots: reset the status map and, when enabled, reconcile
    /// the newer-remote slots into the sandbox. Pass nil when returning to the library.
    func setCurrentGame(sha: String?) {
        currentSha = sha
        states = [:]
        guard let sha, enabled else { return }
        seedLocalStates(sha: sha)
        Task { await reconcile(sha: sha) }
    }

    // MARK: - Status helpers

    /// The status to show for a slot. Returns `.unavailable` when sync is off so the
    /// view can hide the indicator entirely.
    func state(for slot: Int) -> SlotSyncState {
        enabled ? (states[slot] ?? .localOnly) : .unavailable
    }

    private func setState(_ slot: Int, _ value: SlotSyncState) {
        states[slot] = value
    }

    /// Seed each non-empty local slot as `localOnly` until reconciliation confirms it.
    private func seedLocalStates(sha: String) {
        for slot in 0..<SaveStateManager.slotCount {
            states[slot] = saveStates.slot(sha: sha, index: slot).isEmpty ? nil : .localOnly
        }
    }

    // MARK: - Account availability

    private func refreshAccount() async {
        let status = try? await CKContainer.default().accountStatus()
        accountAvailable = (status == .available)
    }

    private func refreshAccountAndReconcile() async {
        await refreshAccount()
        if let sha = currentSha, enabled {
            seedLocalStates(sha: sha)
            await reconcile(sha: sha)
        } else if !enabled {
            states = [:]
        }
    }

    // MARK: - Upload (on save)

    /// Upload one slot to the cloud (fire-and-forget). A no-op when disabled. Never
    /// throws to the caller -- a failure just leaves the slot `localOnly`.
    func upload(sha: String, slot: Int) {
        guard enabled else { return }
        setState(slot, .uploading)
        Task {
            let ok = await performUpload(sha: sha, slot: slot)
            // Only reflect the result if the game hasn't changed underneath us.
            guard currentSha == sha else { return }
            setState(slot, ok ? .synced : .localOnly)
        }
    }

    private func performUpload(sha: String, slot: Int) async -> Bool {
        guard accountAvailable else { return false }
        let urls = saveStates.fileURLs(sha: sha, slot: slot)
        let meta = saveStates.slot(sha: sha, index: slot)
        guard !meta.isEmpty else { return false }

        let record = CKRecord(recordType: Self.recordType, recordID: recordID(sha: sha, slot: slot))
        // String / Int64 / Date all conform to CKRecordValueProtocol, so assign directly.
        record["sha"] = sha
        record["slot"] = Int64(slot)
        record["savedAt"] = meta.savedAt ?? Date()
        record["frame"] = Int64(meta.frame ?? 0)
        record["blob"] = CKAsset(fileURL: urls.state)
        if FileManager.default.fileExists(atPath: urls.thumbnail.path) {
            record["thumbnail"] = CKAsset(fileURL: urls.thumbnail)
        }

        do {
            // `.allKeys` = client-wins: overwrite any existing server record regardless
            // of its change tag, matching the last-writer-wins contract.
            _ = try await Self.database.modifyRecords(
                saving: [record], deleting: [], savePolicy: .allKeys, atomically: true
            )
            return true
        } catch {
            return false
        }
    }

    // MARK: - Delete (on slot clear)

    /// Remove a slot's cloud record (best-effort) when the user deletes it locally.
    func delete(sha: String, slot: Int) {
        states[slot] = nil
        guard enabled else { return }
        Task {
            _ = try? await Self.database.modifyRecords(
                saving: [], deleting: [recordID(sha: sha, slot: slot)],
                savePolicy: .allKeys, atomically: true
            )
        }
    }

    // MARK: - Reconcile (on game open / launch)

    /// Fetch the (up to four) known slot records for `sha` and pull any whose remote
    /// copy is newer than the local one. Silently no-ops when offline / unavailable.
    func reconcile(sha: String) async {
        guard enabled else { return }
        await refreshAccount()
        guard accountAvailable else {
            markAllUnavailable()
            return
        }

        let ids = (0..<SaveStateManager.slotCount).map { recordID(sha: sha, slot: $0) }
        let results: [CKRecord.ID: Result<CKRecord, Error>]
        do {
            results = try await Self.database.records(for: ids)
        } catch {
            // Offline / transient: keep the local-derived states, don't churn the UI.
            return
        }

        for (id, result) in results {
            guard let slot = slot(from: id) else { continue }
            switch result {
            case .success(let record):
                await reconcileSlot(sha: sha, slot: slot, record: record)
            case .failure:
                // No remote record for this slot (unknownItem) -> it is local-only if a
                // local save exists, else absent (no indicator).
                guard currentSha == sha else { return }
                if saveStates.slot(sha: sha, index: slot).isEmpty {
                    states[slot] = nil
                } else {
                    setState(slot, .localOnly)
                }
            }
        }
    }

    private func reconcileSlot(sha: String, slot: Int, record: CKRecord) async {
        let remoteSaved = (record["savedAt"] as? Date) ?? .distantPast
        let localSaved = saveStates.slot(sha: sha, index: slot).savedAt ?? .distantPast

        // Local copy is current (newer or equal) -- already uploaded; nothing to pull.
        guard remoteSaved > localSaved else {
            if currentSha == sha { setState(slot, .synced) }
            return
        }

        guard let blob = record["blob"] as? CKAsset, let blobURL = blob.fileURL else {
            return
        }
        let thumbURL = (record["thumbnail"] as? CKAsset)?.fileURL
        let frame = UInt64(exactly: (record["frame"] as? Int64) ?? 0) ?? 0

        // Copy the downloaded blob/thumbnail into the sandbox off the main thread.
        let manager = saveStates
        let ok = await Task.detached(priority: .utility) { () -> Bool in
            guard let data = try? Data(contentsOf: blobURL) else { return false }
            let thumbData = thumbURL.flatMap { try? Data(contentsOf: $0) }
            do {
                try manager.importRemote(
                    stateData: data, sha: sha, slot: slot,
                    frame: frame, savedAt: remoteSaved, thumbnailPNG: thumbData
                )
                return true
            } catch {
                return false
            }
        }.value

        guard currentSha == sha else { return }
        setState(slot, ok ? .synced : .localOnly)
    }

    private func markAllUnavailable() {
        for slot in states.keys { states[slot] = .unavailable }
    }

    // MARK: - Record identity

    private static var database: CKDatabase { CKContainer.default().privateCloudDatabase }

    private func recordID(sha: String, slot: Int) -> CKRecord.ID {
        CKRecord.ID(recordName: "state-\(sha)-\(slot)")
    }

    /// Parse the slot index back out of a "state-<sha>-<n>" record name.
    private func slot(from id: CKRecord.ID) -> Int? {
        guard let dash = id.recordName.lastIndex(of: "-") else { return nil }
        return Int(id.recordName[id.recordName.index(after: dash)...])
    }
}
