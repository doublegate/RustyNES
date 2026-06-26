//
//  SaveStateManager.swift
//
//  Per-ROM `.rns` save-state slots, stored in the app sandbox at
//  Application Support/RustyNES/states/<sha256>/slot-<n>.rns. The blob is the
//  platform-independent snapshot the core produces (it loads on desktop / Android /
//  iOS alike). Mirrors the Android States.kt slot model.
//
//  v1.9.3: each slot now records sidecar metadata (a `slot-<n>.json` with the
//  frame index + save timestamp) and an optional `slot-<n>.png` thumbnail (the
//  most-recent NES framebuffer at save time). The `.rns` blob format is unchanged
//  and stays the single source of truth for the state itself.
//

import Foundation
import UIKit

/// One save-state slot for a game.
struct SaveSlot: Identifiable {
    let index: Int
    /// When the slot was last written, or nil if empty.
    let savedAt: Date?
    /// The core frame index captured at save time, or nil if unknown/empty.
    let frame: UInt64?
    /// A small preview of the framebuffer at save time, if one was captured.
    let thumbnail: UIImage?

    var id: Int { index }
    var isEmpty: Bool { savedAt == nil }
}

/// Reads/writes per-ROM save-state slots in the sandbox.
///
/// SAFETY (`@unchecked Sendable`): instances are effectively stateless -- the only
/// stored property is `FileManager.default`, which Apple documents as safe to use
/// concurrently from multiple threads for independent operations, and every write is
/// atomic. This lets the CloudKit sync (v1.9.7) call it from a detached task to keep
/// blob file I/O off the main thread.
final class SaveStateManager: @unchecked Sendable {
    /// Number of slots offered per game (matches the Android UI).
    static let slotCount = 4

    /// The slot the quick-save / quick-load shortcuts target (slot 1, zero-based 0).
    static let quickSlot = 0

    private let fileManager = FileManager.default

    /// Sidecar metadata persisted next to each `.rns` blob.
    private struct SlotMeta: Codable {
        var frame: UInt64
        var savedAt: TimeInterval
    }

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

    private func metaURL(sha: String, slot: Int) -> URL {
        dir(for: sha).appendingPathComponent("slot-\(slot).json")
    }

    private func thumbURL(sha: String, slot: Int) -> URL {
        dir(for: sha).appendingPathComponent("slot-\(slot).png")
    }

    /// The current state of every slot for a game.
    func slots(for sha: String) -> [SaveSlot] {
        (0..<Self.slotCount).map { i in slot(sha: sha, index: i) }
    }

    /// The current state of one slot.
    func slot(sha: String, index: Int) -> SaveSlot {
        let stateURL = slotURL(sha: sha, slot: index)
        guard fileManager.fileExists(atPath: stateURL.path) else {
            return SaveSlot(index: index, savedAt: nil, frame: nil, thumbnail: nil)
        }

        // Prefer the sidecar metadata; fall back to the blob's modification date so
        // older slots (written before v1.9.3) still show a timestamp.
        let meta = readMeta(sha: sha, slot: index)
        let savedAt: Date?
        if let meta {
            savedAt = Date(timeIntervalSince1970: meta.savedAt)
        } else {
            let attrs = try? fileManager.attributesOfItem(atPath: stateURL.path)
            savedAt = attrs?[.modificationDate] as? Date
        }

        let thumb = UIImage(contentsOfFile: thumbURL(sha: sha, slot: index).path)
        return SaveSlot(index: index, savedAt: savedAt, frame: meta?.frame, thumbnail: thumb)
    }

    /// Write a `.rns` blob to a slot, plus its metadata and optional thumbnail PNG.
    func write(_ data: Data, sha: String, slot: Int, frame: UInt64 = 0, thumbnailPNG: Data? = nil) throws {
        try fileManager.createDirectory(at: dir(for: sha), withIntermediateDirectories: true)
        try data.write(to: slotURL(sha: sha, slot: slot), options: .atomic)

        let meta = SlotMeta(frame: frame, savedAt: Date().timeIntervalSince1970)
        if let encoded = try? JSONEncoder().encode(meta) {
            try? encoded.write(to: metaURL(sha: sha, slot: slot), options: .atomic)
        }

        let thumb = thumbURL(sha: sha, slot: slot)
        if let png = thumbnailPNG {
            try? png.write(to: thumb, options: .atomic)
        } else {
            try? fileManager.removeItem(at: thumb)
        }
    }

    /// Read a slot's `.rns` blob, or nil if the slot is empty.
    func read(sha: String, slot: Int) -> Data? {
        try? Data(contentsOf: slotURL(sha: sha, slot: slot))
    }

    // MARK: - CloudKit sync support (v1.9.7)

    /// The on-disk URLs for a slot's blob / metadata / thumbnail. Exposed so the
    /// CloudKit sync can hand the blob + thumbnail to `CKAsset(fileURL:)` directly
    /// (no manual read into memory) when uploading.
    func fileURLs(sha: String, slot: Int) -> (state: URL, meta: URL, thumbnail: URL) {
        (slotURL(sha: sha, slot: slot), metaURL(sha: sha, slot: slot), thumbURL(sha: sha, slot: slot))
    }

    /// Write a slot pulled from the cloud, PRESERVING the remote `savedAt` (unlike
    /// `write`, which stamps "now"). Keeping the original timestamp is what makes the
    /// last-writer-wins reconciliation stable across repeated launches.
    func importRemote(
        stateData: Data, sha: String, slot: Int,
        frame: UInt64, savedAt: Date, thumbnailPNG: Data?
    ) throws {
        try fileManager.createDirectory(at: dir(for: sha), withIntermediateDirectories: true)
        try stateData.write(to: slotURL(sha: sha, slot: slot), options: .atomic)

        let meta = SlotMeta(frame: frame, savedAt: savedAt.timeIntervalSince1970)
        if let encoded = try? JSONEncoder().encode(meta) {
            try? encoded.write(to: metaURL(sha: sha, slot: slot), options: .atomic)
        }

        let thumb = thumbURL(sha: sha, slot: slot)
        if let png = thumbnailPNG {
            try? png.write(to: thumb, options: .atomic)
        } else {
            try? fileManager.removeItem(at: thumb)
        }
    }

    /// Delete a slot's saved state and its sidecars.
    func clear(sha: String, slot: Int) {
        try? fileManager.removeItem(at: slotURL(sha: sha, slot: slot))
        try? fileManager.removeItem(at: metaURL(sha: sha, slot: slot))
        try? fileManager.removeItem(at: thumbURL(sha: sha, slot: slot))
    }

    private func readMeta(sha: String, slot: Int) -> SlotMeta? {
        guard let data = try? Data(contentsOf: metaURL(sha: sha, slot: slot)) else { return nil }
        return try? JSONDecoder().decode(SlotMeta.self, from: data)
    }
}
