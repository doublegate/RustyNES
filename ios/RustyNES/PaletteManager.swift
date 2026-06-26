//
//  PaletteManager.swift
//
//  Imported custom NES palettes (`.pal`, v1.9.5). The user imports a `.pal` (>= 192
//  bytes, RGB triples) through the document picker; the bytes are copied into the
//  sandbox at Application Support/RustyNES/palettes/<id>.pal and fed to the core via
//  `NesController.loadPalette` (presentation-only — clearing it is byte-identical to
//  the built-in palette). The chosen palette id is persisted (global default +
//  per-game override) and re-applied when a game loads.
//

import Foundation

/// One imported palette (the id is the imported file's name stem).
struct PaletteFile: Identifiable, Hashable {
    let id: String
    var name: String { id }
}

/// Stores + lists imported `.pal` files in the sandbox.
@MainActor
final class PaletteManager: ObservableObject {
    @Published private(set) var palettes: [PaletteFile] = []

    private let fileManager = FileManager.default

    init() {
        ensureDirectory()
        reload()
    }

    private var dir: URL {
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/palettes", isDirectory: true)
    }

    private func url(for id: String) -> URL {
        dir.appendingPathComponent("\(id).pal")
    }

    private func ensureDirectory() {
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    /// Import a `.pal` from a (possibly security-scoped) picked URL. Copies the
    /// bytes into the sandbox keyed by the file's name stem and returns the id.
    /// - Throws: if the URL cannot be read or the bytes cannot be written.
    @discardableResult
    func importPalette(from url: URL) async throws -> String {
        let id = url.deletingPathExtension().lastPathComponent
        let dest = self.url(for: id)
        ensureDirectory()
        // The read + write run off the main actor for consistency with the other
        // importers; the `palettes` list refreshes back on the main actor.
        try await Task.detached(priority: .userInitiated) {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let data = try Data(contentsOf: url)
            try data.write(to: dest, options: .atomic)
        }.value
        reload()
        return id
    }

    /// The bytes of an imported palette, or nil if the id is unknown / "" (default).
    func bytes(id: String) -> Data? {
        guard !id.isEmpty else { return nil }
        return try? Data(contentsOf: url(for: id))
    }

    /// Whether an imported palette with this id exists.
    func exists(id: String) -> Bool {
        !id.isEmpty && fileManager.fileExists(atPath: url(for: id).path)
    }

    /// Delete an imported palette.
    func remove(id: String) {
        try? fileManager.removeItem(at: url(for: id))
        reload()
    }

    private func reload() {
        let urls = (try? fileManager.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)) ?? []
        palettes = urls
            .filter { $0.pathExtension.lowercased() == "pal" }
            .map { PaletteFile(id: $0.deletingPathExtension().lastPathComponent) }
            .sorted { $0.id.localizedCaseInsensitiveCompare($1.id) == .orderedAscending }
    }
}
