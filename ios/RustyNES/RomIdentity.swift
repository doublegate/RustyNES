//
//  RomIdentity.swift
//
//  The stable key the whole stack uses for a game: the on-disk library path,
//  battery saves, save-state directories, per-game overrides and RA progress.
//
//  v2.9.9 (re-audit NF-21): that key is now the core's ROM identity
//  (`Nes::rom_sha256`: SHA-256 of the bytes after the 16-byte iNES header, of the
//  unpacked image), not the SHA-256 of the whole file as read. `RomKeyMigration`
//  moves a game's stores from the old key the first time it is opened. Mirrors the
//  Android `RomKeyMigration` (Persistence.kt) rule for rule.
//

import CryptoKit
import Foundation

enum RomIdentity {
    /// Lowercase-hex SHA-256 of the supplied bytes: the WHOLE file, header and any
    /// zip container included. The key every store used until v2.9.9; now only the
    /// legacy key `RomKeyMigration` moves stores from.
    static func sha256Hex(_ data: Data) -> String {
        let digest = SHA256.hash(data: data)
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    /// v2.9.9 (NF-21) — the core's identity of a ROM file (the bridge's
    /// `rom_identity_of_file`: a `.zip` is unpacked, an iNES header left out),
    /// computed without building a console, so a disk needs no BIOS. Falls back to
    /// the whole-file hash for a file the bridge refuses (over its 16 MiB limit),
    /// which could not be loaded either.
    static func identityHex(_ data: Data) -> String {
        (try? romIdentityOfFile(rom: data)) ?? sha256Hex(data)
    }

    /// The raw 32-byte SHA-256 (for FFI calls that take the digest bytes, e.g.
    /// `NesController.raLoadGame` — not wired in the v1.9.0 MVP, kept for parity).
    static func sha256Bytes(_ data: Data) -> Data {
        Data(SHA256.hash(data: data))
    }
}

/// v2.9.9 (re-audit NF-21) — the one-time move of a game's FILE stores from the
/// whole-file key to the core's identity. The two in-memory stores move through
/// their owners (`ROMLibrary.rekey`, `GameOverrides.rekey`); `AppModel.openGame`
/// runs all three, library first.
///
/// The rules (the Android `RomKeyMigrationTest` pins them; this mirror is not
/// compiled in CI):
/// - a store moves only into an EMPTY new key; one already there is never
///   overwritten, and the old copy is then left where it was (an orphan, not a loss);
/// - a file is written atomically under the new key, read back and compared, and
///   only then is the original removed, so a failure at any point keeps it;
/// - equal keys touch nothing, and a second run finds nothing.
///
/// Moved here: `battery/<k>.sav`, every file in `states/<k>/` (slots, their
/// metadata and thumbnails) and `ra-progress/<k>.bin`, under
/// Application Support/RustyNES. NOT moved: iCloud save-state records
/// (`state-<k>-<n>`, remote), which the next upload re-creates under the new key.
enum RomKeyMigration {
    /// Application Support/RustyNES, the root every store lives under.
    static var root: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("RustyNES", isDirectory: true)
    }

    /// Move the file stores keyed by `legacy` to `identity`. Returns how many
    /// files moved; never throws (a file that cannot be moved stays under `legacy`).
    @discardableResult
    static func migrateFiles(
        legacy: String, identity: String, root: URL = RomKeyMigration.root
    ) -> Int {
        guard !legacy.isEmpty, !identity.isEmpty, legacy != identity else { return 0 }
        var moved = 0
        moved += moveFile(
            root.appendingPathComponent("battery/\(legacy).sav"),
            root.appendingPathComponent("battery/\(identity).sav")
        )
        moved += moveDir(
            root.appendingPathComponent("states/\(legacy)", isDirectory: true),
            root.appendingPathComponent("states/\(identity)", isDirectory: true)
        )
        moved += moveFile(
            root.appendingPathComponent("ra-progress/\(legacy).bin"),
            root.appendingPathComponent("ra-progress/\(identity).bin")
        )
        return moved
    }

    /// Copy `src` to an absent `dst`, verify it, then remove `src`. 1 if moved.
    static func moveFile(_ src: URL, _ dst: URL) -> Int {
        guard copyVerified(src, dst) else { return 0 }
        return (try? FileManager.default.removeItem(at: src)) != nil ? 1 : 0
    }

    /// Copy the regular file `src` to an absent `dst` atomically and read it back.
    /// True only when `dst` now holds exactly `src`'s bytes; `src` is untouched.
    static func copyVerified(_ src: URL, _ dst: URL) -> Bool {
        let fm = FileManager.default
        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: src.path, isDirectory: &isDir), !isDir.boolValue,
              !fm.fileExists(atPath: dst.path),
              let bytes = try? Data(contentsOf: src) else { return false }
        do {
            try fm.createDirectory(
                at: dst.deletingLastPathComponent(), withIntermediateDirectories: true
            )
            try bytes.write(to: dst, options: .atomic)
        } catch {
            return false
        }
        return (try? Data(contentsOf: dst)) == bytes
    }

    /// `moveFile` for each file in `src`; removes `src` once it is empty.
    private static func moveDir(_ src: URL, _ dst: URL) -> Int {
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(at: src, includingPropertiesForKeys: nil)
        else { return 0 }
        let moved = files.reduce(0) { $0 + moveFile($1, dst.appendingPathComponent($1.lastPathComponent)) }
        if (try? fm.contentsOfDirectory(atPath: src.path))?.isEmpty == true {
            try? fm.removeItem(at: src)
        }
        return moved
    }
}
