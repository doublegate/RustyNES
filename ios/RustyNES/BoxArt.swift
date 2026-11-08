//
//  BoxArt.swift
//
//  v3.7.0 (D24, `T-MOBILE-EXTRAS` row 1): box art for the library, the iOS twin of
//  Android's `BoxArt.kt` (v1.8.8). Cover art comes from the community
//  libretro-thumbnails library, the no-intro-named PNG set RetroArch uses, and only
//  when the player asks ("Find box art" in a tile's context menu): RustyNES bundles
//  no art, and nothing is fetched on its own.
//
//  The candidate URLs come from the bridge (`boxArtCandidateUrls`, host-tested in
//  `rustynes-mobile`'s extras module), which applies Android's matching rules, so
//  both apps find the same cover for the same file name. A download is capped at
//  2 MiB, must decode as an image, and is cached durably (DurableWrite) as
//  `boxart/<sha>.png`, in the App Group container when it is provisioned (so the
//  home-screen widget can show it), else in Application Support.
//
//  UNCOMPILED on the Linux host this was written on (no Swift toolchain); the iOS CI
//  Simulator build compiles it, and device row W3 is its first run.
//

import Foundation
import UIKit

enum BoxArt {
    /// The largest cover accepted: Android's limit.
    static let maxBytes = 2 * 1024 * 1024

    /// Where the cover for `sha` is cached (it may not exist yet).
    static func cacheURL(forSha sha: String) -> URL {
        if let shared = ResumeWidgetShared.boxArtURL(sha: sha) { return shared }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("RustyNES/boxart", isDirectory: true)
            .appendingPathComponent("\(sha).png")
    }

    /// The cached cover for `sha`, or nil.
    static func cachedImage(forSha sha: String) -> UIImage? {
        UIImage(contentsOfFile: cacheURL(forSha: sha).path)
    }

    /// Fetch a cover for the game named `displayName`, trying each candidate in
    /// order, and cache the first that downloads and decodes. Returns whether one
    /// was cached. Never throws: a miss or a network error is `false`.
    static func fetch(displayName: String, sha: String) async -> Bool {
        for candidate in boxArtCandidateUrls(displayName: displayName) {
            guard let url = URL(string: candidate) else { continue }
            var request = URLRequest(url: url, timeoutInterval: 8)
            request.setValue("RustyNES-iOS", forHTTPHeaderField: "User-Agent")
            guard let (data, response) = try? await URLSession.shared.data(for: request),
                  (response as? HTTPURLResponse)?.statusCode == 200,
                  !data.isEmpty, data.count <= maxBytes,
                  UIImage(data: data) != nil
            else { continue }
            do {
                try DurableWrite.write(data, to: cacheURL(forSha: sha))
                return true
            } catch {
                return false
            }
        }
        return false
    }

    /// Forget the cached cover for `sha` (the library's "Remove" does this).
    static func remove(sha: String) {
        try? FileManager.default.removeItem(at: cacheURL(forSha: sha))
    }
}
