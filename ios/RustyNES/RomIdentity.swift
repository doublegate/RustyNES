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
/// - a file counts as moved only once its original is gone. A run that copied a
///   file but could not remove the original reports it PENDING, and the next run
///   finishes the move: it finds the identical copy already in place and removes
///   the leftover, which is a duplicate, never the only copy;
/// - equal keys touch nothing, and a second run finds nothing.
///
/// Retrying needs the legacy key, which iOS cannot recompute once the library entry
/// is rekeyed (the entry keeps no file to hash). `ROMLibrary` therefore stores it in
/// the entry (`LibraryEntry.pendingLegacyKey`, written with the rekey in one index
/// save) until `migrateFiles` reports nothing pending; `AppModel.openGame` re-runs
/// the migration on every open while it is set. Android needs no marker: it
/// recomputes the legacy key from the file on every open.
///
/// Moved here: `battery/<k>.sav`, every file in `states/<k>/` (slots, their
/// metadata and thumbnails) and `ra-progress/<k>.bin`, under
/// Application Support/RustyNES. NOT moved: iCloud save-state records
/// (`state-<k>-<n>`, remote), which the next upload re-creates under the new key.
enum RomKeyMigration {
    /// What one file's move did.
    enum Step {
        /// The file is under the new key and the original is gone.
        case moved
        /// Nothing under the old key: nothing to do.
        case absent
        /// The new key already holds DIFFERENT bytes: both stay, by rule. Final.
        case kept
        /// The copy, its check or the removal of the original failed. The original
        /// is still there and a later run retries it.
        case failed
    }

    /// What `migrateFiles` did: `moved` files, and `pending` files still under the
    /// old key that a later run can move. A file `kept` by rule is neither.
    struct Outcome {
        var moved = 0
        var pending = 0
        /// True when nothing is left for a later run to do.
        var isComplete: Bool { pending == 0 }

        mutating func add(_ step: Step) {
            switch step {
            case .moved: moved += 1
            case .failed: pending += 1
            case .absent, .kept: break
            }
        }

        mutating func add(_ other: Outcome) {
            moved += other.moved
            pending += other.pending
        }
    }

    /// Application Support/RustyNES, the root every store lives under.
    static var root: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("RustyNES", isDirectory: true)
    }

    /// Move the file stores keyed by `legacy` to `identity`. Never throws: a file
    /// that cannot be moved stays under `legacy` and is counted in `pending`. Safe
    /// to repeat; a run after a complete one finds nothing.
    @discardableResult
    static func migrateFiles(
        legacy: String, identity: String, root: URL = RomKeyMigration.root
    ) -> Outcome {
        var outcome = Outcome()
        guard !legacy.isEmpty, !identity.isEmpty, legacy != identity else { return outcome }
        outcome.add(moveFile(
            root.appendingPathComponent("battery/\(legacy).sav"),
            root.appendingPathComponent("battery/\(identity).sav")
        ))
        outcome.add(moveDir(
            root.appendingPathComponent("states/\(legacy)", isDirectory: true),
            root.appendingPathComponent("states/\(identity)", isDirectory: true)
        ))
        outcome.add(moveFile(
            root.appendingPathComponent("ra-progress/\(legacy).bin"),
            root.appendingPathComponent("ra-progress/\(identity).bin")
        ))
        return outcome
    }

    /// Copy `src` to an absent `dst`, verify it, then remove `src`.
    ///
    /// Until v2.9.9's review a `removeItem` that threw was swallowed by `try?` and
    /// reported "not moved" although the verified copy was already in place, so the
    /// file then sat under both keys and the next run, seeing `dst` taken, left the
    /// duplicate forever. Now a `dst` holding exactly `src`'s bytes is treated as
    /// that half-done move and finished, and a failed removal is `.failed` (pending),
    /// not a silent zero.
    static func moveFile(_ src: URL, _ dst: URL) -> Step {
        let fm = FileManager.default
        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: src.path, isDirectory: &isDir), !isDir.boolValue else {
            return .absent
        }
        if fm.fileExists(atPath: dst.path) {
            let original: Data
            let copy: Data
            do {
                original = try Data(contentsOf: src)
                copy = try Data(contentsOf: dst)
            } catch {
                NSLog("RustyNES: key migration could not compare \(src.lastPathComponent): \(error)")
                return .failed
            }
            // Different bytes: the never-overwrite rule keeps both.
            guard original == copy else { return .kept }
        } else if !copyVerified(src, dst) {
            return .failed
        }
        do {
            try fm.removeItem(at: src)
            return .moved
        } catch {
            NSLog("RustyNES: key migration copied \(src.lastPathComponent) but could not remove the original: \(error)")
            return .failed
        }
    }

    /// Copy the regular file `src` to an absent `dst` atomically and read it back.
    /// True only when `dst` now holds exactly `src`'s bytes; `src` is untouched.
    /// Every failure is logged (it used to be swallowed without a trace).
    static func copyVerified(_ src: URL, _ dst: URL) -> Bool {
        let fm = FileManager.default
        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: src.path, isDirectory: &isDir), !isDir.boolValue,
              !fm.fileExists(atPath: dst.path) else { return false }
        let bytes: Data
        do {
            bytes = try Data(contentsOf: src)
            try fm.createDirectory(
                at: dst.deletingLastPathComponent(), withIntermediateDirectories: true
            )
            try bytes.write(to: dst, options: .atomic)
        } catch {
            NSLog("RustyNES: key migration could not copy \(src.lastPathComponent): \(error)")
            return false
        }
        guard (try? Data(contentsOf: dst)) == bytes else {
            NSLog("RustyNES: key migration copy of \(src.lastPathComponent) reads back different")
            return false
        }
        return true
    }

    /// `moveFile` for each file in `src`; removes `src` once it is empty. A
    /// directory that cannot be listed although it exists is pending.
    private static func moveDir(_ src: URL, _ dst: URL) -> Outcome {
        let fm = FileManager.default
        var outcome = Outcome()
        guard fm.fileExists(atPath: src.path) else { return outcome }
        guard let files = try? fm.contentsOfDirectory(at: src, includingPropertiesForKeys: nil)
        else {
            NSLog("RustyNES: key migration could not list \(src.lastPathComponent)")
            outcome.pending += 1
            return outcome
        }
        for file in files {
            outcome.add(moveFile(file, dst.appendingPathComponent(file.lastPathComponent)))
        }
        // Only an EMPTY directory goes: a file kept by rule, or one whose move
        // failed, is still the user's copy. A failed removal of the empty directory
        // loses nothing and the next run, finding it empty again, retries it.
        if (try? fm.contentsOfDirectory(atPath: src.path))?.isEmpty == true {
            try? fm.removeItem(at: src)
        }
        return outcome
    }
}
