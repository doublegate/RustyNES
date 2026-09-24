//
//  BatterySave.swift
//
//  Cartridge battery saves on iOS (v2.7.4, frontend audit MOB-05 -- the iOS half
//  of FE-01). Until v2.7.4 the app kept no in-game save: the bridge exposed no
//  battery RAM, so a game saved in-game came back without it after the app was
//  closed, except inside a save state. The bridge now exports and imports it
//  (`NesController.hasBattery` / `batteryRam` / `loadBatteryRam`); this keeps it
//  at Application Support/RustyNES/battery/<rom-sha256>.sav, beside the
//  save-state directory and keyed by the same hash.
//
//  The rules are the desktop's and Android's, so the three hosts behave alike:
//  only a cartridge whose header sets the battery bit is persisted; a file of the
//  wrong size is never loaded and, that session, never overwritten (it is more
//  likely another game's or another emulator's than a damaged one of ours);
//  "clean" means equal to the last bytes written; writes are atomic.
//

import Foundation

/// What `BatterySaver.read(expected:)` found on disk.
enum SavedBattery {
    /// No save yet; the game starts from its power-on RAM.
    case none
    /// A save of the cartridge's size, to load before the first frame.
    case found(Data)
    /// A file that must not be loaded or overwritten; the string is for the user.
    case unusable(String)
}

/// One cartridge's battery RAM bound to its `.sav`. Every write goes through a
/// private serial queue, so the periodic flush (off the main thread) and the
/// final flush on close (waited for) never overlap.
///
/// SAFETY (`@unchecked Sendable`): `last` and `disabled` are touched only on
/// `queue`; `url` is immutable.
final class BatterySaver: @unchecked Sendable {
    let url: URL
    private var last: Data?
    private var disabled = false
    private let queue = DispatchQueue(label: "com.doublegate.rustynes.battery")

    init(url: URL) {
        self.url = url
    }

    /// The `.sav` URL for the ROM with SHA-256 `sha`.
    static func url(forSha sha: String) -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/battery/\(sha).sav")
    }

    /// Read the save, checking its size against the cartridge's (`expected`
    /// bytes) BEFORE reading it, so a huge or wrong file is never loaded into
    /// memory. An unusable file disables this saver for the session.
    func read(expected: Int) -> SavedBattery {
        queue.sync {
            let path = url.path
            guard FileManager.default.fileExists(atPath: path) else { return .none }
            let size = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? Int) ?? -1
            guard size == expected else {
                disabled = true
                return .unusable(
                    "The battery save is \(size) bytes; this game's is \(expected). "
                        + "It was left untouched and this session will not be saved."
                )
            }
            guard let data = try? Data(contentsOf: url), data.count == expected else {
                disabled = true
                return .unusable("The battery save could not be read.")
            }
            return .found(data)
        }
    }

    /// Record what the cartridge now holds (after a load, or at power-on) as clean.
    func baseline(_ current: Data) {
        queue.sync { last = current }
    }

    /// Stop persisting this session (a save the bridge refused must not be overwritten).
    func disable() {
        queue.sync { disabled = true }
    }

    /// Write `current` on the queue if it differs from the last write, without
    /// waiting. For the periodic flush.
    func flushLater(_ current: Data) {
        queue.async { self.writeIfChanged(current) }
    }

    /// Write `current` if it differs from the last write, and wait for it. For
    /// the paths that end a session (close, a game switch, backgrounding).
    func flushNow(_ current: Data) {
        queue.sync { writeIfChanged(current) }
    }

    /// Runs on `queue`. A failed write leaves the baseline alone, so the next
    /// flush retries; `.atomic` keeps the previous file intact.
    private func writeIfChanged(_ current: Data) {
        guard !disabled, !current.isEmpty, current != last else { return }
        do {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try current.write(to: url, options: .atomic)
            last = current
        } catch {
            NSLog("RustyNES: battery save failed: \(error)")
        }
    }
}
