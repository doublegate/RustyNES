//
//  CrashReporter.swift
//
//  Opt-in, privacy-first crash reporting (v2.0.6 "Parity") — the iOS analogue of
//  the Android `CrashReporter` (Android v1.8.8 "Atlas"). Closes the v1.9.9
//  readiness gap that listed an "opt-in crash-reporting surface" as an
//  iOS-applicable deferral.
//
//  Privacy posture: **off by default** (explicit consent via the Settings toggle),
//  so the app's "Data Not Collected" privacy nutrition label is preserved. When the
//  user opts in, an uncaught-exception handler writes the stack trace to a **local**
//  file in the app sandbox that the user can read / copy — RustyNES uploads nothing
//  on its own, so `PrivacyInfo.xcprivacy` is unchanged.
//
//  Crash-handler discipline (why everything is pre-resolved at install time). The
//  uncaught-exception handler runs on an **arbitrary thread in an unstable process**
//  (potential heap corruption / deadlock), so it must touch as little as possible:
//   - It reads only **cached, plain-value statics** (device / OS / app version) and a
//     **pre-resolved log-directory URL** — never `UIDevice.current` (which is
//     `@MainActor`-isolated and would be an actor-isolation violation from the C
//     callback) and never `Bundle.main.infoDictionary` / directory creation.
//   - Log **pruning** (directory enumeration + file deletion) is done at
//     `install(enabled:)` (launch, main thread), NOT inside the handler, so a crash
//     never triggers risky filesystem walks that could cause a secondary crash before
//     the current report is written.
//  The handler therefore does the bare minimum: format one string and write one file.
//
//  Honesty note (what this catches, and what it does not). `NSSetUncaughtException`
//  `Handler` catches Objective-C / bridged `NSException` crashes. Pure-Swift runtime
//  traps (`fatalError`, force-unwrap of nil, out-of-bounds, integer overflow) abort
//  via a POSIX signal (SIGTRAP / SIGILL / SIGABRT), which an exception handler does
//  NOT see. Installing async-signal-unsafe file writes from a signal handler is
//  unsound, so — exactly as the Android side documents Firebase Crashlytics as the
//  maintainer option rather than pulling it in — Swift-trap capture is left to a
//  real third-party reporter a maintainer can add later (gated on this same opt-in
//  flag). This surface diagnoses the ObjC/`NSException` class (UIKit, KVO, bridged
//  Foundation) without a third-party SDK and without collecting anything by default.
//

import Foundation
import UIKit

/// Opt-in, local-only crash reporting. All members are static — there is one
/// process-wide handler, mirroring the Android `object CrashReporter`.
enum CrashReporter {
    /// UserDefaults key for the opt-in flag (shared with `AppModel`).
    static let enabledKey = "crashReportingEnabled"

    private static let dirName = "crash-logs"
    private static let maxLogs = 10

    /// Set once we chain the handler, so a runtime toggle doesn't double-install.
    private static var installed = false

    /// The handler that was installed before ours, chained after we record. Held as
    /// **static** (not captured) because `NSSetUncaughtExceptionHandler` takes a
    /// `@convention(c)` callback, which cannot capture local context — the closure
    /// below may only reference global / static state.
    private static var previousHandler: (@convention(c) (NSException) -> Void)?

    // Metadata cached at `install(enabled:)` on the main thread, so the crash handler
    // (arbitrary thread, unstable process) reads only plain values — never the
    // `@MainActor` `UIDevice.current` or `Bundle.main.infoDictionary`.
    private static var deviceModel = "Unknown"
    private static var systemVersion = "Unknown"
    private static var appVersion = "?"
    private static var appBuild = "?"
    /// The pre-resolved crash-log directory, so the handler does no URL resolution or
    /// directory creation. `nil` until `install(enabled:)` runs.
    private static var cachedLogDir: URL?

    /// Install the opt-in uncaught-`NSException` handler. A no-op unless `enabled`
    /// (the user's Settings opt-in) and never installs twice. `@MainActor` because it
    /// reads `UIDevice.current` (main-actor-isolated) while caching metadata; it is
    /// only ever called from the `@MainActor` `AppModel`. Chains to any previous
    /// handler so the normal crash / process kill still happens after we record; the
    /// handler re-checks the live opt-in flag at crash time, so toggling reporting off
    /// stops new logs immediately (without needing to uninstall).
    @MainActor
    static func install(enabled: Bool) {
        guard enabled, !installed else { return }
        installed = true

        // Resolve + cache everything the handler might need, now, on the main thread.
        deviceModel = UIDevice.current.model
        systemVersion = UIDevice.current.systemVersion
        appVersion = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        appBuild = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?"
        cachedLogDir = resolveLogDir()
        // Prune here (launch), NOT inside the crash handler — directory enumeration +
        // deletion is unsafe in an unstable post-crash process.
        pruneOldLogs()

        previousHandler = NSGetUncaughtExceptionHandler()
        // The closure captures NOTHING (only static references), so it is convertible
        // to the `@convention(c)` handler `NSSetUncaughtExceptionHandler` expects.
        NSSetUncaughtExceptionHandler { exception in
            // Re-check the live flag: honour a runtime opt-out.
            if UserDefaults.standard.bool(forKey: CrashReporter.enabledKey) {
                CrashReporter.writeLog(exception)
            }
            CrashReporter.previousHandler?(exception)
        }
    }

    /// Resolve the crash-log directory (`Application Support/RustyNES/crash-logs/`),
    /// creating it. Called at install time and from the (main-thread) Settings viewer;
    /// **never** from the crash handler, which uses the pre-resolved `cachedLogDir`.
    private static func resolveLogDir() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("RustyNES", isDirectory: true)
            .appendingPathComponent(dirName, isDirectory: true)
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        return base
    }

    /// Serialize one exception to a timestamped local file. Runs from the C exception
    /// handler on an arbitrary thread, so it touches only cached statics + the
    /// pre-resolved directory and does the minimum work: format one string, write one
    /// file. No pruning, no `UIDevice`, no directory creation here.
    private static func writeLog(_ exception: NSException) {
        guard let dir = cachedLogDir else { return }
        let stamp = timestamp()
        var body = "RustyNES crash report\n"
        body += "Time:    \(stamp)\n"
        body += "Version: \(appVersion) (\(appBuild))\n"
        body += "Device:  \(deviceModel) — iOS \(systemVersion)\n"
        body += "Name:    \(exception.name.rawValue)\n"
        body += "Reason:  \(exception.reason ?? "(none)")\n\n"
        body += exception.callStackSymbols.joined(separator: "\n")
        let url = dir.appendingPathComponent("crash-\(stamp).txt")
        try? body.write(to: url, atomically: true, encoding: .utf8)
    }

    /// `yyyyMMdd-HHmmss` in a fixed (US-POSIX) locale so filenames sort chronologically.
    private static func timestamp() -> String {
        let fmt = DateFormatter()
        fmt.locale = Locale(identifier: "en_US_POSIX")
        fmt.dateFormat = "yyyyMMdd-HHmmss"
        return fmt.string(from: Date())
    }

    /// Keep only the most recent `maxLogs` files (newest-first by filename). Called at
    /// install time (main thread), never from the crash handler.
    private static func pruneOldLogs() {
        let logs = savedLogs()
        guard logs.count > maxLogs else { return }
        for url in logs.dropFirst(maxLogs) {
            try? FileManager.default.removeItem(at: url)
        }
    }

    /// The saved crash-log files, newest first (for the Settings viewer). Main-thread /
    /// launch use only; resolves the directory lazily if `install` has not run.
    static func savedLogs() -> [URL] {
        let dir = cachedLogDir ?? resolveLogDir()
        let contents = (try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: nil)) ?? []
        return contents
            .filter { $0.lastPathComponent.hasPrefix("crash-") }
            .sorted { $0.lastPathComponent > $1.lastPathComponent }
    }

    /// Delete every saved crash log (the Settings "clear" action).
    static func clear() {
        for url in savedLogs() {
            try? FileManager.default.removeItem(at: url)
        }
    }
}
