//
//  BuildChannel.swift
//
//  The distribution channel tag for the iOS build. RustyNES is permanently
//  open-source and income-free (ADR 0035): there is a single free channel with
//  every feature unlocked, no ads, no in-app purchase, and no tracking. There is
//  no App-Store monetization flavor and no StoreKit / ad-SDK compilation seam.
//
//  Kept as a tiny helper so the existing `BuildChannel.isFoss` call sites (e.g. the
//  developer debugger surface) keep reading cleanly; every property is constant.
//

import Foundation

enum BuildChannel {
    /// Always true — the only channel is the free, open-source one.
    static var isFoss: Bool { true }

    /// A short human-readable tag for the About screen / diagnostics.
    static var displayName: String { "FOSS" }
}
