//
//  GameOverrides.swift
//
//  Per-ROM display-settings overrides (v1.9.5 "Power-user feature port"), keyed by
//  the ROM SHA-256 — the same stable key the library, save-states, and the bridge
//  use. Each game can remember its own video filter + shader params + chosen
//  palette + HD-pack, independent of the global defaults. Persisted as a single
//  JSON map in Application Support/RustyNES/overrides.json. Mirrors the desktop /
//  Android per-game `<rom>.json` config-override model (a simplified subset: just
//  the display settings the iOS app exposes).
//

import Foundation

/// A full snapshot of a game's display settings. Present in the store only when the
/// user has opted a game into custom settings; absent means "use the global
/// defaults". `paletteId` / `hdpackId` are empty strings for "built-in palette" /
/// "no HD-pack" respectively (the same id space `PaletteManager` / `HDPackStore`
/// use).
struct GameDisplaySettings: Codable, Equatable {
    var filter: UInt8
    var scanlineIntensity: Float
    var crtMask: Float
    var ntscSaturation: Float
    var ntscSharpness: Float
    var ntscTint: Float
    var ntscPhase: Float
    /// The chosen palette id (an imported `.pal` stem), or "" for the built-in NES
    /// palette.
    var paletteId: String
    /// The chosen HD-pack id (an imported pack stem), or "" for no HD-pack.
    var hdpackId: String
}

/// The observable per-ROM override store. Loads/saves a JSON map at
/// Application Support/RustyNES/overrides.json.
@MainActor
final class GameOverrides: ObservableObject {
    @Published private(set) var overrides: [String: GameDisplaySettings] = [:]

    private let fileManager = FileManager.default

    init() { load() }

    private var fileURL: URL {
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/overrides.json")
    }

    /// The override for `sha`, or nil if the game uses the global defaults.
    func settings(for sha: String) -> GameDisplaySettings? { overrides[sha] }

    /// Whether `sha` has a per-game override.
    func has(_ sha: String) -> Bool { overrides[sha] != nil }

    /// Set (or replace) the override for `sha`.
    func set(_ settings: GameDisplaySettings, for sha: String) {
        overrides[sha] = settings
        save()
    }

    /// v2.9.9 (re-audit NF-21) — move `legacy`'s override to `identity`, once: only
    /// when `identity` has none (an existing one is never overwritten, and the old
    /// entry then stays). The map is written whole and atomically. True when
    /// nothing is left to do (moved, nothing to move, or kept by that rule); false
    /// when the map could not be written, in which case the move is rolled back in
    /// memory too, so this session and `overrides.json` agree and a later open (the
    /// library entry's `pendingLegacyKey`) retries it.
    @discardableResult
    func rekey(from legacy: String, to identity: String) -> Bool {
        guard legacy != identity, overrides[identity] == nil,
              let settings = overrides[legacy] else { return true }
        overrides[identity] = settings
        overrides.removeValue(forKey: legacy)
        guard save() else {
            overrides[legacy] = settings
            overrides.removeValue(forKey: identity)
            return false
        }
        return true
    }

    /// Remove `sha`'s override (revert it to the global defaults).
    func clear(for sha: String) {
        overrides.removeValue(forKey: sha)
        save()
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL) else { return }
        if let decoded = try? JSONDecoder().decode([String: GameDisplaySettings].self, from: data) {
            overrides = decoded
        }
    }

    /// Write the map atomically. True when `overrides.json` now holds `overrides`.
    @discardableResult
    private func save() -> Bool {
        let dir = fileURL.deletingLastPathComponent()
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        do {
            let data = try JSONEncoder().encode(overrides)
            try data.write(to: fileURL, options: .atomic)
            return true
        } catch {
            NSLog("RustyNES: per-game overrides not saved: \(error)")
            return false
        }
    }
}
