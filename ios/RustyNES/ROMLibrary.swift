//
//  ROMLibrary.swift
//
//  The user's imported-ROM library. ROMs are USER-PROVIDED ONLY (via the Files /
//  document picker / share sheet) — RustyNES never bundles commercial ROMs. On
//  import the bytes are copied into the app sandbox at
//  Application Support/RustyNES/roms/<sha256>.nes, keyed by SHA-256 (the same key
//  the bridge, save-states, and RetroAchievements use). This mirrors the Android
//  GameLibrary, keyed identically.
//
//  Compliance posture (App Review Guideline 4.7): the app is a general-purpose NES
//  emulator that runs ONLY content the user supplies and owns. No content is
//  bundled or fetched.
//

import Foundation
import SwiftUI

/// One imported cartridge in the library.
struct LibraryEntry: Identifiable, Codable, Hashable {
    /// Lowercase-hex ROM SHA-256 (the stable key + the on-disk filename stem).
    let sha: String
    /// Display name (the imported file's name, sans extension).
    var name: String
    /// iNES/NES 2.0 mapper number, or -1 if unknown.
    var mapper: Int
    /// Region label ("NTSC" / "PAL" / "Dendy"), or "" if unknown.
    var region: String
    /// Last-played epoch seconds (0 = never played).
    var lastPlayed: TimeInterval
    /// User favorite flag.
    var favorite: Bool
    /// v2.9.9 (re-audit NF-21) — the pre-v2.9.9 whole-file key this entry was moved
    /// from, while stores under it may still need moving (see `RomKeyMigration`).
    /// Written in the same index save as the rekey, so a store whose move failed is
    /// retried on a later open, not stranded under a key nothing remembers; cleared
    /// once nothing is pending. nil for every other entry, and absent from an index
    /// written before v2.9.9 (an Optional decodes as nil when its key is missing).
    var pendingLegacyKey: String? = nil

    var id: String { sha }
}

/// The observable library model. Persists a JSON index in
/// Application Support/RustyNES/library.json and the ROM bytes alongside it.
@MainActor
final class ROMLibrary: ObservableObject {
    @Published private(set) var entries: [LibraryEntry] = []

    private let fileManager = FileManager.default

    init() {
        ensureDirectories()
        load()
    }

    // MARK: - Paths

    private var appSupport: URL {
        // Application Support is the right home for app-managed (non-user-facing)
        // content; it is excluded from the user's document browser.
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES", isDirectory: true)
    }

    private var romsDir: URL { appSupport.appendingPathComponent("roms", isDirectory: true) }
    private var indexURL: URL { appSupport.appendingPathComponent("library.json") }

    func romURL(for sha: String) -> URL {
        romsDir.appendingPathComponent("\(sha).nes")
    }

    private func ensureDirectories() {
        try? fileManager.createDirectory(at: romsDir, withIntermediateDirectories: true)
    }

    // MARK: - Import

    /// Import a ROM from a (possibly security-scoped) URL handed back by the
    /// document picker / share sheet. Copies the bytes into the sandbox keyed by
    /// SHA-256, registers (or refreshes) the entry, and returns it. This does NOT
    /// validate ROM *content* — it only reads, hashes, and stores the bytes;
    /// whether the file is a usable ROM is decided later, when `EmulatorCore`
    /// constructs the `NesController` (which surfaces a load error separately).
    /// - Throws: if the URL cannot be read or the bytes cannot be written to the
    ///   sandbox.
    @discardableResult
    func importROM(from url: URL) async throws -> LibraryEntry {
        // The (potentially large) read + hash + write run off the main actor so
        // the UI doesn't block; the index mutation hops back to the actor. Mirror
        // HDPackStore.importPack: capture the main-actor-derived destination dir
        // as a value before the detached task.
        let romsDir = self.romsDir
        // Files-app URLs are security-scoped. Acquire the scope on the main actor and
        // hold it across the off-main read (security-scoped access is actor-bound, so
        // it must NOT be started inside the detached task). The detached closure
        // captures only `url` / `romsDir` — both Sendable.
        guard url.startAccessingSecurityScopedResource() else {
            throw AppError.fileAccessDenied
        }
        defer { url.stopAccessingSecurityScopedResource() }
        // v2.9.9 (re-audit NF-21): keyed by the core's identity (`identityHex`), no
        // longer the whole file's hash, which is kept as `legacy` only to find an
        // entry imported before v2.9.9.
        let (sha, legacy) = try await Task.detached(priority: .userInitiated) {
            let data = try Data(contentsOf: url)
            let sha = RomIdentity.identityHex(data)
            let legacy = RomIdentity.sha256Hex(data)
            let dest = romsDir.appendingPathComponent("\(sha).nes")

            // Copy into the sandbox (idempotent: a re-import of the same ROM just
            // updates the index entry's last-played, not the bytes).
            if !FileManager.default.fileExists(atPath: dest.path) {
                try data.write(to: dest, options: .atomic)
            }
            return (sha, legacy)
        }.value

        let displayName = url.deletingPathExtension().lastPathComponent
        if let idx = entries.firstIndex(where: { $0.sha == sha }) {
            entries[idx].name = displayName
            save()
            return entries[idx]
        }
        // An entry imported before v2.9.9 under the whole-file key: return it, and
        // `AppModel.openGame` moves it and its saves to the identity. Adding a
        // second entry here would leave its saves behind under the old key.
        if legacy != sha, let idx = entries.firstIndex(where: { $0.sha == legacy }) {
            if fileManager.fileExists(atPath: romURL(for: legacy).path) {
                // The old entry has its bytes: the copy under the identity is a
                // duplicate, dropped here; `ROMLibrary.rekey` re-creates it from the
                // old file at open.
                try? fileManager.removeItem(at: romURL(for: sha))
            } else {
                // The old entry's file is missing, so the copy under the identity is
                // the ONLY copy of these bytes: keep it and point the entry at it
                // now, marked so `openGame` still moves the stores the old key holds.
                // Should the index not save, the entry keeps its old key and the copy
                // stays; a later import retries.
                _ = replaceKey(at: idx, with: sha, pendingLegacyKey: legacy)
            }
            entries[idx].name = displayName
            save()
            return entries[idx]
        }
        let entry = LibraryEntry(
            sha: sha,
            name: displayName,
            mapper: -1,
            region: "",
            lastPlayed: 0,
            favorite: false
        )
        entries.insert(entry, at: 0)
        save()
        return entry
    }

    /// Load a library entry's ROM bytes from the sandbox. The read runs off the
    /// main actor so a large ROM doesn't block the UI.
    func romData(for entry: LibraryEntry) async throws -> Data {
        let url = romURL(for: entry.sha)
        return try await Task.detached(priority: .userInitiated) {
            try Data(contentsOf: url)
        }.value
    }

    // MARK: - Mutations

    /// Record that a game was just opened, and backfill its metadata from a loaded
    /// core's `RomInfo`.
    func markPlayed(_ sha: String, info: RomInfo) {
        guard let idx = entries.firstIndex(where: { $0.sha == sha }) else { return }
        entries[idx].lastPlayed = Date().timeIntervalSince1970
        entries[idx].mapper = Int(info.mapperId)
        entries[idx].region = regionLabel(info.region)
        // Re-sort most-recent-first.
        entries.sort { $0.lastPlayed > $1.lastPlayed }
        save()
    }

    /// v2.9.9 (re-audit NF-21) — move the entry keyed `legacy` (the whole file's
    /// hash, the key until v2.9.9) to `identity`, the core's ROM identity, with its
    /// ROM copy. Returns the entry the game now has under `identity`: an entry
    /// already there is returned untouched (the legacy one then stays, as do its
    /// files), and nil means nothing moved -- the caller keeps the legacy key for
    /// this session. The ROM file is copied, read back and compared before the
    /// index changes, and the old file is removed only after the index is SAVED:
    /// when the save fails the in-memory key is rolled back, the copy removed and
    /// the old file kept, so `library.json` never names a key whose file is gone.
    /// The moved entry carries `pendingLegacyKey` until `finishMigration` clears it.
    func rekey(from legacy: String, to identity: String) -> LibraryEntry? {
        if let existing = entries.first(where: { $0.sha == identity }) { return existing }
        guard legacy != identity,
              let idx = entries.firstIndex(where: { $0.sha == legacy }) else { return nil }
        let src = romURL(for: legacy)
        let dst = romURL(for: identity)
        if fileManager.fileExists(atPath: dst.path) {
            // An orphan under the identity (no entry names it): replace it.
            try? fileManager.removeItem(at: dst)
        }
        guard RomKeyMigration.copyVerified(src, dst) else { return nil }
        guard replaceKey(at: idx, with: identity, pendingLegacyKey: legacy) else {
            // The index still names `legacy`: keep its file, drop the copy (a
            // verified duplicate of it), and let the caller keep the old key.
            try? fileManager.removeItem(at: dst)
            return nil
        }
        removeLegacyROM(legacy)
        return entries[idx]
    }

    /// v2.9.9 (re-audit NF-21) — the migration of `identity` from `legacy` left
    /// nothing pending: clear the entry's marker. Also retries the removal of the
    /// old ROM file, which `rekey` may have failed to remove (a duplicate by then).
    /// Returns the updated entry; when the index cannot be saved the marker stays,
    /// in memory and on disk, and the next open repeats the (idempotent) migration.
    func finishMigration(of identity: String, from legacy: String) -> LibraryEntry? {
        guard let idx = entries.firstIndex(where: { $0.sha == identity }) else { return nil }
        guard entries[idx].pendingLegacyKey == legacy else { return entries[idx] }
        entries[idx].pendingLegacyKey = nil
        if !save() { entries[idx].pendingLegacyKey = legacy }
        removeLegacyROM(legacy)
        return entries[idx]
    }

    /// Point the entry at `idx` at `identity`, keeping its user fields, and save.
    /// False, with the entry unchanged in memory, when the index cannot be written.
    private func replaceKey(at idx: Int, with identity: String, pendingLegacyKey: String) -> Bool {
        let old = entries[idx]
        entries[idx] = LibraryEntry(
            sha: identity,
            name: old.name,
            mapper: old.mapper,
            region: old.region,
            lastPlayed: old.lastPlayed,
            favorite: old.favorite,
            pendingLegacyKey: pendingLegacyKey
        )
        guard save() else {
            entries[idx] = old
            return false
        }
        return true
    }

    /// Remove the old key's ROM file once no entry names that key. Its bytes are
    /// then under the identity, so a failure leaves a duplicate (logged, retried by
    /// `finishMigration`), never the only copy.
    private func removeLegacyROM(_ legacy: String) {
        guard !entries.contains(where: { $0.sha == legacy }) else { return }
        let url = romURL(for: legacy)
        guard fileManager.fileExists(atPath: url.path) else { return }
        do {
            try fileManager.removeItem(at: url)
        } catch {
            NSLog("RustyNES: could not remove the pre-v2.9.9 ROM copy \(legacy): \(error)")
        }
    }

    func toggleFavorite(_ sha: String) {
        guard let idx = entries.firstIndex(where: { $0.sha == sha }) else { return }
        entries[idx].favorite.toggle()
        save()
    }

    /// Remove an entry and its copied ROM bytes from the sandbox.
    func remove(_ sha: String) {
        entries.removeAll { $0.sha == sha }
        try? fileManager.removeItem(at: romURL(for: sha))
        save()
    }

    private func regionLabel(_ region: NesRegion) -> String {
        switch region {
        case .ntsc: return "NTSC"
        case .pal: return "PAL"
        case .dendy: return "Dendy"
        }
    }

    // MARK: - Persistence

    private func load() {
        guard let data = try? Data(contentsOf: indexURL) else { return }
        if let decoded = try? JSONDecoder().decode([LibraryEntry].self, from: data) {
            entries = decoded.sorted { $0.lastPlayed > $1.lastPlayed }
        }
    }

    /// Write the index atomically. True when `library.json` now holds `entries`.
    /// Most callers ignore a failure (the in-memory list stays right and the next
    /// save rewrites it whole); the key migration does not, because removing a file
    /// the on-disk index still names would strand that entry.
    @discardableResult
    private func save() -> Bool {
        ensureDirectories()
        do {
            let data = try JSONEncoder().encode(entries)
            try data.write(to: indexURL, options: .atomic)
            return true
        } catch {
            NSLog("RustyNES: library index not saved: \(error)")
            return false
        }
    }
}
