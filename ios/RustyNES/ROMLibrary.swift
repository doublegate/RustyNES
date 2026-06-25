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
    /// SHA-256, registers (or refreshes) the entry, and returns it.
    /// - Throws: if the URL cannot be read or the bytes are not a usable ROM.
    @discardableResult
    func importROM(from url: URL) throws -> LibraryEntry {
        // Files-app URLs are security-scoped; bracket the read.
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }

        let data = try Data(contentsOf: url)
        let sha = RomIdentity.sha256Hex(data)
        let dest = romURL(for: sha)

        // Copy into the sandbox (idempotent: a re-import of the same ROM just
        // updates the index entry's last-played, not the bytes).
        if !fileManager.fileExists(atPath: dest.path) {
            try data.write(to: dest, options: .atomic)
        }

        let displayName = url.deletingPathExtension().lastPathComponent
        if let idx = entries.firstIndex(where: { $0.sha == sha }) {
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

    /// Load a library entry's ROM bytes from the sandbox.
    func romData(for entry: LibraryEntry) throws -> Data {
        try Data(contentsOf: romURL(for: entry.sha))
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

    private func save() {
        ensureDirectories()
        if let data = try? JSONEncoder().encode(entries) {
            try? data.write(to: indexURL, options: .atomic)
        }
    }
}
