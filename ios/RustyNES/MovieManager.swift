//
//  MovieManager.swift
//
//  TAS movie (`.rnm`) storage (v1.9.5). Recorded movies (the bytes
//  `NesController.movieStopRecording()` returns) are written to the sandbox at
//  Application Support/RustyNES/movies/<name>.rnm, listed for playback, and can be
//  exported via the share sheet. External `.rnm` files are imported through the
//  document picker and played back. Determinism is preserved entirely by the core
//  (it records / replays the input stream); this manager is pure file plumbing.
//

import Foundation

/// One saved `.rnm` movie on disk.
struct MovieFile: Identifiable, Hashable {
    let url: URL
    let savedAt: Date?

    var id: URL { url }
    var name: String { url.deletingPathExtension().lastPathComponent }
}

/// Stores + lists saved `.rnm` movies in the sandbox.
@MainActor
final class MovieManager: ObservableObject {
    @Published private(set) var movies: [MovieFile] = []

    private let fileManager = FileManager.default

    init() {
        ensureDirectory()
        reload()
    }

    private var dir: URL {
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/movies", isDirectory: true)
    }

    private func ensureDirectory() {
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    /// Save recorded `.rnm` bytes under a name derived from the game + a timestamp,
    /// and return the written URL. Sanitises the name to a safe filename stem.
    @discardableResult
    func save(_ data: Data, gameName: String) throws -> URL {
        ensureDirectory()
        let stem = Self.fileStem(gameName: gameName)
        let url = dir.appendingPathComponent("\(stem).rnm")
        try data.write(to: url, options: .atomic)
        reload()
        return url
    }

    /// Read a movie's bytes for playback.
    func bytes(at url: URL) -> Data? {
        try? Data(contentsOf: url)
    }

    /// Delete a saved movie.
    func remove(at url: URL) {
        try? fileManager.removeItem(at: url)
        reload()
    }

    private static func fileStem(gameName: String) -> String {
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_ "))
        let safeName = gameName.unicodeScalars
            .map { allowed.contains($0) ? Character($0) : "_" }
            .reduce(into: "") { $0.append($1) }
            .trimmingCharacters(in: .whitespaces)
        let base = safeName.isEmpty ? "movie" : safeName

        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        return "\(base)-\(formatter.string(from: Date()))"
    }

    private func reload() {
        let keys: [URLResourceKey] = [.contentModificationDateKey]
        let urls = (try? fileManager.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: keys
        )) ?? []
        movies = urls
            .filter { $0.pathExtension.lowercased() == "rnm" }
            .map { url in
                let date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?
                    .contentModificationDate
                return MovieFile(url: url, savedAt: date)
            }
            .sorted { ($0.savedAt ?? .distantPast) > ($1.savedAt ?? .distantPast) }
    }
}
