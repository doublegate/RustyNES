//
//  ResumeWidgetShared.swift
//
//  v3.7.0 (D24, `T-MOBILE-EXTRAS` row 1): what the app and its home-screen widget
//  share, compiled into BOTH targets (project.yml lists this directory in each). The
//  iOS twin of Android's `ResumeWidget.kt`, which shows the last-played game and
//  resumes it on a tap.
//
//  The two processes share an App Group (`group.com.doublegate.rustynes`, in both
//  targets' entitlements): its UserDefaults suite carries the last-played game's name
//  and identity, and its container holds the box art (BoxArt.swift caches there when
//  the group is available), so the widget can show the cover without the app running.
//
//  UNCOMPILED on the Linux host this was written on (no Swift toolchain); the iOS CI
//  Simulator build compiles it. The App Group must also be registered for both bundle
//  ids in the Apple Developer account before a signed (TestFlight) build: a
//  maintainer step, recorded in docs/ios.md and the run sheet (row W1).
//

import Foundation

enum ResumeWidgetShared {
    /// The App Group both targets declare.
    static let appGroup = "group.com.doublegate.rustynes"
    /// The widget's `kind` (its `StaticConfiguration` and the app's reload call).
    static let kind = "RustyNESResume"

    private static let nameKey = "resumeWidget.name"
    private static let shaKey = "resumeWidget.sha"
    private static let playedKey = "resumeWidget.played"

    /// The group's shared defaults, or nil when the group is not provisioned (an
    /// unsigned Simulator build): the widget then shows its empty state.
    static var defaults: UserDefaults? { UserDefaults(suiteName: appGroup) }

    /// The group container, or nil when the group is not provisioned.
    static var container: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup)
    }

    /// The last-played game, as the widget shows it.
    struct LastPlayed: Equatable {
        let name: String
        /// Lowercase-hex ROM identity: the library key, and the box-art file stem.
        let sha: String
        let played: Date
    }

    /// Record `game` as the last played (the app calls this when a game opens).
    static func write(_ game: LastPlayed) {
        guard let d = defaults else { return }
        d.set(game.name, forKey: nameKey)
        d.set(game.sha, forKey: shaKey)
        d.set(game.played.timeIntervalSince1970, forKey: playedKey)
    }

    /// The last-played game, or nil before any game has been opened.
    static func read() -> LastPlayed? {
        guard let d = defaults,
              let name = d.string(forKey: nameKey),
              let sha = d.string(forKey: shaKey)
        else { return nil }
        return LastPlayed(
            name: name,
            sha: sha,
            played: Date(timeIntervalSince1970: d.double(forKey: playedKey))
        )
    }

    /// The box-art file for `sha` in the group container (may not exist).
    static func boxArtURL(sha: String) -> URL? {
        container?.appendingPathComponent("boxart", isDirectory: true)
            .appendingPathComponent("\(sha).png")
    }
}

/// The widget's deep link: `rustynes://resume` reopens the last-played game.
enum ResumeWidgetLink {
    static let url = URL(string: "rustynes://resume")!

    static func isResume(_ url: URL) -> Bool {
        url.scheme?.lowercased() == "rustynes" && url.host?.lowercased() == "resume"
    }
}
