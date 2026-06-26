//
//  CloudConfigSync.swift
//
//  iCloud key-value sync of the small app config (v1.9.5). Mirrors the global
//  Settings scalars (video filter, shader params, palette selection, mute, haptics)
//  across a user's devices via `NSUbiquitousKeyValueStore` — NOT save-states or
//  ROMs in this release (those stay local). Strategy: write each value to the cloud
//  store on change, observe `didChangeExternallyNotification` to pull remote changes
//  back, and reconcile with the local `UserDefaults` (last-writer-wins).
//
//  Gracefully no-ops when iCloud is unavailable (no account / capability not
//  enabled): the store API still works locally, and writes simply don't propagate.
//  The maintainer must enable the iCloud capability + a key-value container in the
//  Apple Developer account / Xcode target (documented carryover — see
//  RustyNES.entitlements).
//

import Foundation

/// A thin wrapper over `NSUbiquitousKeyValueStore` that mirrors a fixed set of
/// config keys and reports external (other-device) changes.
final class CloudConfigSync {
    private let store = NSUbiquitousKeyValueStore.default
    private var observer: NSObjectProtocol?

    /// Invoked on the main queue when the cloud store changes externally (another
    /// device wrote, or the initial sync arrived). The owner pulls the listed keys
    /// back into `UserDefaults` / its published state.
    var onExternalChange: (() -> Void)?

    /// The config keys mirrored to the cloud (a subset of the `UserDefaults` keys).
    let keys: [String]

    init(keys: [String]) {
        self.keys = keys
        observer = NotificationCenter.default.addObserver(
            forName: NSUbiquitousKeyValueStore.didChangeExternallyNotification,
            object: store,
            queue: .main
        ) { [weak self] _ in
            self?.onExternalChange?()
        }
        // Pull whatever the cloud already has for this account.
        store.synchronize()
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }

    /// Push a float to the cloud store.
    func setFloat(_ value: Float, forKey key: String) {
        store.set(Double(value), forKey: key)
        store.synchronize()
    }

    /// Push a bool to the cloud store.
    func setBool(_ value: Bool, forKey key: String) {
        store.set(value, forKey: key)
        store.synchronize()
    }

    /// Push an integer to the cloud store.
    func setInt(_ value: Int, forKey key: String) {
        store.set(Int64(value), forKey: key)
        store.synchronize()
    }

    /// Push a string to the cloud store.
    func setString(_ value: String, forKey key: String) {
        store.set(value, forKey: key)
        store.synchronize()
    }

    /// The cloud value for `key`, if any (nil when never written / iCloud absent).
    func float(forKey key: String) -> Float? {
        store.object(forKey: key) == nil ? nil : Float(store.double(forKey: key))
    }

    func bool(forKey key: String) -> Bool? {
        store.object(forKey: key) == nil ? nil : store.bool(forKey: key)
    }

    func int(forKey key: String) -> Int? {
        store.object(forKey: key) == nil ? nil : Int(store.longLong(forKey: key))
    }

    func string(forKey key: String) -> String? {
        store.string(forKey: key)
    }
}
