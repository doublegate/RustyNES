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
//    * On save -> upload in the background, inside a UIKit background task so it
//      finishes when the app is backgrounded. Conflict-safe since v2.7.4 (IOS-10):
//      the server record is fetched first and a NEWER remote save is left alone;
//      otherwise that record is updated and saved with `.ifServerRecordUnchanged`,
//      so a write from another device in between fails this upload rather than
//      being overwritten, and the per-record result decides the slot's status.
//      (Before v2.7.4 this was force-overwrite, client-wins, and an older save
//      uploaded later replaced a newer one.)
//    * On game open / app launch -> fetch the (up to four) known record IDs and
//      reconcile by `savedAt`: pull any slot whose remote copy is newer into the
//      sandbox; UPLOAD any slot whose local copy is newer, since that means its
//      upload never landed (v2.7.4, IOS-04 -- it used to be marked synced).
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
import MachO

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

    /// Kick off an account check at launch (no game yet). Safe to call when disabled:
    /// sync is opt-in, so a disabled sync never touches CloudKit at all.
    func start() {
        guard enabled else { return }
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
        guard enabled else {
            accountAvailable = false
            return
        }
        let status = try? await Self.container?.accountStatus()
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
        // v2.7.4 (frontend audit IOS-04): uploads start as the player saves, often
        // just before backgrounding; without a background task iOS suspended the
        // process mid-upload and the slot never reached iCloud.
        let background = BackgroundTask(name: "RustyNES cloud upload")
        Task {
            defer { background.end() }
            let uploadedAt = await performUpload(sha: sha, slot: slot)
            // Only reflect the result if the game hasn't changed underneath us.
            guard currentSha == sha else { return }
            // If the user cleared or overwrote the slot while the upload was in flight,
            // don't mark a now-absent (or changed) slot as synced/localOnly.
            let meta = saveStates.slot(sha: sha, index: slot)
            guard !meta.isEmpty else { states[slot] = nil; return }
            if let uploadedAt, (meta.savedAt ?? .distantPast) == uploadedAt {
                setState(slot, .synced)
            } else {
                setState(slot, .localOnly)
            }
        }
    }

    /// Returns the `savedAt` timestamp that was uploaded on success (so the caller can
    /// confirm the local slot still matches), or `nil` on failure / no-op.
    private func performUpload(sha: String, slot: Int) async -> Date? {
        // Re-check the account live rather than trusting a possibly-stale cached flag:
        // the initial async account check may not have finished when a save fires, which
        // would wrongly skip the upload and leave the slot `localOnly`.
        let status = try? await Self.container?.accountStatus()
        accountAvailable = (status == .available)
        guard accountAvailable, let database = Self.database else { return nil }
        let urls = saveStates.fileURLs(sha: sha, slot: slot)
        let meta = saveStates.slot(sha: sha, index: slot)
        guard !meta.isEmpty else { return nil }

        let savedAt = meta.savedAt ?? Date()
        let id = recordID(sha: sha, slot: slot)
        // v2.7.4 (frontend audit IOS-10): never overwrite a newer save blindly.
        // The upload used `.allKeys` (client wins), so a device holding an OLDER
        // save that uploaded later replaced a newer one, whatever the timestamps
        // said. Fetch the server's record first: if it is newer, leave it for
        // `reconcile` to pull; otherwise update THAT record and save it with
        // `.ifServerRecordUnchanged`, so a write from another device in between
        // fails this save instead of being lost. (A missing record, or a fetch
        // that fails offline, starts a new record; saving a new record over an
        // existing one also fails under that policy.)
        let existing = try? await database.record(for: id)
        if let existing, let remoteSaved = existing["savedAt"] as? Date, remoteSaved > savedAt {
            return nil
        }
        let record = existing ?? CKRecord(recordType: Self.recordType, recordID: id)
        // String / Int64 / Date all conform to CKRecordValueProtocol, so assign directly.
        record["sha"] = sha
        record["slot"] = Int64(slot)
        record["savedAt"] = savedAt
        record["frame"] = Int64(meta.frame ?? 0)
        record["blob"] = CKAsset(fileURL: urls.state)
        if FileManager.default.fileExists(atPath: urls.thumbnail.path) {
            record["thumbnail"] = CKAsset(fileURL: urls.thumbnail)
        } else {
            // No local thumbnail: clear the field so a stale remote thumbnail doesn't
            // persist after a thumbnail-less re-save (the record overwrites all keys).
            record["thumbnail"] = nil
        }

        do {
            let (saved, _) = try await database.modifyRecords(
                saving: [record], deleting: [], savePolicy: .ifServerRecordUnchanged, atomically: true
            )
            // The call succeeds as a whole even when this record's save failed
            // (e.g. `serverRecordChanged`); the per-record result is the answer.
            // Before v2.7.4 it was discarded, so such a failure read as success.
            switch saved[id] {
            case .success?:
                return savedAt
            case .failure(let error)?:
                // Typically `serverRecordChanged`: another device wrote first.
                NSLog("RustyNES: iCloud slot \(slot) not uploaded: \(error)")
                return nil
            case nil:
                NSLog("RustyNES: iCloud slot \(slot) upload returned no result")
                return nil
            }
        } catch {
            NSLog("RustyNES: iCloud slot \(slot) upload failed: \(error)")
            return nil
        }
    }

    // MARK: - Delete (on slot clear)

    /// Remove a slot's cloud record (best-effort) when the user deletes it locally.
    ///
    /// Best-effort means the local delete never waits on or fails with the cloud
    /// one, not that a failure goes unrecorded: it is logged like an upload
    /// failure. `modifyRecords` reports a per-record failure in its result rather
    /// than by throwing, so both are checked. `.unknownItem` is not logged: it
    /// only means the slot was never uploaded.
    func delete(sha: String, slot: Int) {
        states[slot] = nil
        guard enabled, let database = Self.database else { return }
        let id = recordID(sha: sha, slot: slot)
        Task {
            do {
                let (_, deleted) = try await database.modifyRecords(
                    saving: [], deleting: [id],
                    savePolicy: .allKeys, atomically: true
                )
                if case .failure(let error) = deleted[id],
                   (error as? CKError)?.code != .unknownItem {
                    NSLog("RustyNES: iCloud slot \(slot) not deleted: \(error)")
                }
            } catch {
                NSLog("RustyNES: iCloud slot \(slot) delete failed: \(error)")
            }
        }
    }

    // MARK: - Reconcile (on game open / launch)

    /// Fetch the (up to four) known slot records for `sha` and pull any whose remote
    /// copy is newer than the local one. Silently no-ops when offline / unavailable.
    func reconcile(sha: String) async {
        guard enabled else { return }
        await refreshAccount()
        guard accountAvailable, let database = Self.database else {
            markAllUnavailable()
            return
        }

        let ids = (0..<SaveStateManager.slotCount).map { recordID(sha: sha, slot: $0) }
        let results: [CKRecord.ID: Result<CKRecord, Error>]
        do {
            results = try await database.records(for: ids)
        } catch {
            // Offline / transient: keep the local-derived states, don't churn the
            // UI. Logged, not shown: a fetch failure changes nothing on screen.
            NSLog("RustyNES: iCloud reconcile fetch failed: \(error)")
            return
        }

        for (id, result) in results {
            // The user may have switched games while the fetch was in flight; drop the
            // rest so we don't write files for a game we already navigated away from.
            guard currentSha == sha else { return }
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
        let localMeta = saveStates.slot(sha: sha, index: slot)
        let localSlotMissing = localMeta.isEmpty
        let remoteSaved = (record["savedAt"] as? Date) ?? .distantPast
        let localSaved = localMeta.savedAt ?? .distantPast

        // Pull when there is NO local slot at all (a valid remote slot must seed an empty
        // device regardless of its timestamp -- both may be `.distantPast`), and
        // otherwise only when the remote copy is strictly newer.
        guard localSlotMissing || remoteSaved > localSaved else {
            guard currentSha == sha else { return }
            if localSaved > remoteSaved {
                // v2.7.4 (IOS-04): a newer local copy is one whose upload never
                // landed (a suspended or failed upload). It used to be marked
                // `.synced` here while iCloud kept the stale copy; upload it now.
                upload(sha: sha, slot: slot)
            } else {
                setState(slot, .synced)
            }
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

    /// The CloudKit container, or `nil` when this binary is not entitled to one.
    ///
    /// CloudKit raises an uncatchable trap -- not a `CKError` -- when the app
    /// creates a container without the `com.apple.developer.icloud-container-
    /// identifiers` entitlement: `CKContainer.default()` throws a `CKException`
    /// ("containerIdentifier can not be nil") and `CKContainer(identifier:)` stops
    /// in a `brk` inside its initialiser (both measured on the iOS 27 simulator).
    /// An unsigned simulator build, or a sideload whose profile lacks the iCloud
    /// capability, crashed at launch that way. So no container is created unless
    /// the entitlement is present, and every CloudKit call site below treats a
    /// `nil` container as "unavailable", exactly like a signed-out account.
    private static let container: CKContainer? =
        CloudKitEntitlement.isPresent ? CKContainer.default() : nil

    private static var database: CKDatabase? { container?.privateCloudDatabase }

    private func recordID(sha: String, slot: Int) -> CKRecord.ID {
        CKRecord.ID(recordName: "state-\(sha)-\(slot)")
    }

    /// Parse the slot index back out of a "state-<sha>-<n>" record name.
    private func slot(from id: CKRecord.ID) -> Int? {
        guard let dash = id.recordName.lastIndex(of: "-") else { return nil }
        return Int(id.recordName[id.recordName.index(after: dash)...])
    }
}

// MARK: - Entitlement probe

/// Whether this binary carries the CloudKit container entitlement, read with public
/// APIs only (iOS exposes no call that returns an app's own entitlements).
///
/// * Simulator: Xcode links the entitlements into the main executable's
///   `__TEXT,__entitlements` section ("Simulated.xcent"); an unsigned build
///   (`CODE_SIGNING_ALLOWED=NO`) has no such section.
/// * Device: a development or ad-hoc build embeds its provisioning profile, whose
///   `Entitlements` dictionary is what the signature was allowed to claim. App Store
///   and TestFlight builds carry no `embedded.mobileprovision`, and are always
///   signed with the capability, so a missing profile counts as entitled.
enum CloudKitEntitlement {
    private static let key = "com.apple.developer.icloud-container-identifiers"

    static let isPresent: Bool = {
        #if targetEnvironment(simulator)
        guard let entitlements = simulatorEntitlements() else { return false }
        #else
        guard let url = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision") else {
            return true
        }
        guard let entitlements = profileEntitlements(at: url) else { return false }
        #endif
        return !((entitlements[key] as? [Any])?.isEmpty ?? true)
    }()

    #if targetEnvironment(simulator)
    private static func simulatorEntitlements() -> [String: Any]? {
        guard let header = _dyld_get_image_header(0) else { return nil }
        var size: UInt = 0
        let raw = header.withMemoryRebound(to: mach_header_64.self, capacity: 1) {
            getsectiondata($0, "__TEXT", "__entitlements", &size)
        }
        guard let raw, size > 0 else { return nil }
        return plist(Data(bytes: raw, count: Int(size)))
    }
    #else
    /// The profile is a CMS envelope around an XML plist; the plist is read out of
    /// it by its delimiters rather than by verifying the signature, which is the
    /// kernel's job, not this probe's.
    private static func profileEntitlements(at url: URL) -> [String: Any]? {
        guard let data = try? Data(contentsOf: url),
              let start = data.range(of: Data("<?xml".utf8)),
              let end = data.range(of: Data("</plist>".utf8), in: start.lowerBound..<data.endIndex)
        else { return nil }
        let profile = plist(data.subdata(in: start.lowerBound..<end.upperBound))
        return profile?["Entitlements"] as? [String: Any]
    }
    #endif

    private static func plist(_ data: Data) -> [String: Any]? {
        (try? PropertyListSerialization.propertyList(from: data, format: nil)) as? [String: Any]
    }
}
