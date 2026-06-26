//
//  HDPackStore.swift
//
//  Imported HD-packs (`.zip`, v1.9.5). The user imports an HD-pack archive through
//  the document picker; the bytes are copied into the sandbox at
//  Application Support/RustyNES/hdpacks/<id>.zip and fed to the core via
//  `NesController.loadHdpackFromZipBytes`. When a pack is active the frame loop
//  presents the composited HD frame (`compositeHdFrame()` at `hdpackDimensions()`)
//  through the renderer's HD path. HD-packs are surfaced per-game (referenced by the
//  per-game override's `hdpackId`).
//

import Foundation

/// One imported HD-pack (the id is the imported file's name stem).
struct HDPackFile: Identifiable, Hashable {
    let id: String
    var name: String { id }
}

/// Stores + lists imported HD-pack `.zip` archives in the sandbox.
@MainActor
final class HDPackStore: ObservableObject {
    @Published private(set) var packs: [HDPackFile] = []

    private let fileManager = FileManager.default

    init() {
        ensureDirectory()
        reload()
    }

    private var dir: URL {
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/hdpacks", isDirectory: true)
    }

    private func url(for id: String) -> URL {
        dir.appendingPathComponent("\(id).zip")
    }

    private func ensureDirectory() {
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    /// Import an HD-pack `.zip` from a (possibly security-scoped) picked URL. Copies
    /// the bytes into the sandbox keyed by the file's name stem and returns the id.
    /// The (potentially large) read + write run off the main actor so the UI doesn't
    /// block; the `packs` list is refreshed back on the main actor.
    /// - Throws: if the URL cannot be read or the bytes cannot be written.
    @discardableResult
    func importPack(from url: URL) async throws -> String {
        let id = url.deletingPathExtension().lastPathComponent
        let dest = self.url(for: id)
        ensureDirectory()
        try await Task.detached(priority: .userInitiated) {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let data = try Data(contentsOf: url)
            try data.write(to: dest, options: .atomic)
        }.value
        reload()
        return id
    }

    /// The bytes of an imported pack, or nil if the id is unknown / "" (none).
    func bytes(id: String) -> Data? {
        guard !id.isEmpty else { return nil }
        return try? Data(contentsOf: url(for: id))
    }

    /// Whether an imported pack with this id exists.
    func exists(id: String) -> Bool {
        !id.isEmpty && fileManager.fileExists(atPath: url(for: id).path)
    }

    /// Delete an imported pack.
    func remove(id: String) {
        try? fileManager.removeItem(at: url(for: id))
        reload()
    }

    private func reload() {
        let urls = (try? fileManager.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        packs = urls
            .filter { $0.pathExtension.lowercased() == "zip" }
            .map { HDPackFile(id: $0.deletingPathExtension().lastPathComponent) }
            .sorted { $0.id.localizedCaseInsensitiveCompare($1.id) == .orderedAscending }
    }
}
