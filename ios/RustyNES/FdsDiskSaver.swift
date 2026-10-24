//
//  FdsDiskSaver.swift
//
//  v3.6.0 (`T-IOS-FDS-PERIODIC`, #605's review): an FDS disk's save, written
//  once a second while the game changes it, as Android has done since #605's
//  review (`FdsSaves.flushIfDirty` on the emulation loop's 60-frame cadence) and
//  the desktop does every frame.
//
//  v3.2.0 wrote an FDS disk only at the lifecycle points: backgrounding, a game
//  switch, closing the game. A process that died between them -- a crash, a
//  jetsam kill under memory pressure, the debugger stopping it, a force-quit
//  from the app switcher while it was still active -- lost every disk write
//  since the last one, which for an FDS game is every in-game save. The
//  battery timer existed only for battery cartridges (`attachBattery`).
//
//  The write runs on this class's serial queue, never on the main actor: a disk
//  image is about 65 KiB a side, and `DurableWrite` syncs it through the drive's
//  cache, which can take milliseconds. The rules are `BatterySaver`'s:
//
//  - the dirty flag is cleared BEFORE the image is copied, so a disk write that
//    lands during the copy sets it again and is caught by the next tick (the
//    shape Cursor found on Android in #605);
//  - a failed write marks the disk for a retry, and the next tick writes it
//    again even though the flag is clear;
//  - the paths that end a session wait (`wait: true`): they first let any write
//    already queued finish, so its outcome is known before deciding whether a
//    retry is due.
//
//  Never compiled on the machine that wrote it (Linux); the device check is row
//  T14 of `docs/mobile-v2.9.3-run-sheet.md`.
//

import Foundation

/// SAFETY (`@unchecked Sendable`): `retry` is touched only under `lock`; writes
/// are serialised on `queue`.
final class FdsDiskSaver: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.doublegate.rustynes.fds")
    private let lock = NSLock()
    /// Disks (by identity) whose last write failed.
    private var retry: Set<String> = []

    /// Whether any disk still needs a write that failed.
    var hasRetries: Bool {
        lock.lock()
        defer { lock.unlock() }
        return !retry.isEmpty
    }

    private func needsRetry(_ sha: String) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return retry.contains(sha)
    }

    /// Write `core`'s disk to `url` if the game changed it since the last write,
    /// or if the last write of `sha` failed. `wait` false is the periodic tick
    /// (the write is queued and this returns at once); `wait` true is a session
    /// ending, which returns only after the write is done.
    func flush(_ core: EmulatorCore, sha: String, to url: URL, wait: Bool) {
        if wait { queue.sync {} } // an earlier queued write finishes and records first
        guard core.diskIsDirty || needsRetry(sha) else { return }
        core.clearDiskDirty()
        let bytes = core.diskImageBytes()
        let job = { self.write(bytes, sha: sha, to: url) }
        if wait {
            queue.sync(execute: job)
        } else {
            queue.async(execute: job)
        }
    }

    /// Runs on `queue`.
    private func write(_ bytes: Data, sha: String, to url: URL) {
        let ok: Bool
        do {
            try DurableWrite.write(bytes, to: url)
            ok = true
        } catch {
            ok = false
            NSLog("RustyNES: FDS disk save failed: \(error)")
        }
        lock.lock()
        if ok { retry.remove(sha) } else { retry.insert(sha) }
        lock.unlock()
    }
}
