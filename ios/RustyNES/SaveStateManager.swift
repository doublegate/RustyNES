//
//  SaveStateManager.swift
//
//  Per-ROM `.rns` save-state slots, stored in the app sandbox at
//  Application Support/RustyNES/states/<sha256>/slot-<n>.rns. The blob is the
//  platform-independent snapshot the core produces (it loads on desktop / Android /
//  iOS alike). Mirrors the Android States.kt slot model.
//

import Foundation

/// One save-state slot for a game.
struct SaveSlot: Identifiable {
    let index: Int
    /// When the slot was last written, or nil if empty.
    let savedAt: Date?

    var id: Int { index }
    var isEmpty: Bool { savedAt == nil }
}

/// Reads/writes per-ROM save-state slots in the sandbox.
final class SaveStateManager {
    /// Number of slots offered per game (matches the Android UI).
    static let slotCount = 4

    private let fileManager = FileManager.default

    private var statesRoot: URL {
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/states", isDirectory: true)
    }

    private func dir(for sha: String) -> URL {
        statesRoot.appendingPathComponent(sha, isDirectory: true)
    }

    private func slotURL(sha: String, slot: Int) -> URL {
        dir(for: sha).appendingPathComponent("slot-\(slot).rns")
    }

    /// The current state of every slot for a game.
    func slots(for sha: String) -> [SaveSlot] {
        (0..<Self.slotCount).map { i in
            let url = slotURL(sha: sha, slot: i)
            let attrs = try? fileManager.attributesOfItem(atPath: url.path)
            let date = attrs?[.modificationDate] as? Date
            return SaveSlot(index: i, savedAt: date)
        }
    }

    /// Write a `.rns` blob to a slot.
    func write(_ data: Data, sha: String, slot: Int) throws {
        try fileManager.createDirectory(at: dir(for: sha), withIntermediateDirectories: true)
        try data.write(to: slotURL(sha: sha, slot: slot), options: .atomic)
    }

    /// Read a slot's `.rns` blob, or nil if the slot is empty.
    func read(sha: String, slot: Int) -> Data? {
        try? Data(contentsOf: slotURL(sha: sha, slot: slot))
    }

    /// Delete a slot's saved state.
    func clear(sha: String, slot: Int) {
        try? fileManager.removeItem(at: slotURL(sha: sha, slot: slot))
    }
}
