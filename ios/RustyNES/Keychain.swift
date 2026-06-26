//
//  Keychain.swift
//
//  A minimal wrapper over the Security framework's `SecItem*` API for the one
//  secret the app stores: the RetroAchievements login token (v1.9.6). The token is
//  a bearer credential to a third-party account, so it lives in the Keychain (NOT
//  UserDefaults / iCloud KVS), and is excluded from any config sync. Values are kept
//  as `kSecClassGenericPassword` items keyed by a stable string account name under
//  this app's service identifier.
//
//  Privacy note (maintainer carryover, see RetroAchievementsModel): RetroAchievements
//  is an opt-in account login to a third party. If/when this ships, the app's privacy
//  manifest (PrivacyInfo.xcprivacy) must disclose the account-login data collection.
//  The stored token here is the only credential persisted; the password is never
//  retained (it is forwarded to the bridge once and discarded).
//

import Foundation
import Security

/// A tiny string-secret store backed by the iOS Keychain.
enum Keychain {
    /// The Keychain service identifier (namespaces this app's items).
    private static let service = "com.doublegate.rustynes.secrets"

    /// Store (or replace) a string secret for `account`. Returns `true` on success.
    /// The item is `WhenUnlockedThisDeviceOnly`: it is never synced to iCloud
    /// Keychain and is unavailable while the device is locked (the app only reads it
    /// foregrounded, at game-load time).
    @discardableResult
    static func set(_ value: String, account: String) -> Bool {
        guard let data = value.data(using: .utf8) else { return false }
        // Delete any existing item first so this is an upsert.
        delete(account: account)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        ]
        return SecItemAdd(query as CFDictionary, nil) == errSecSuccess
    }

    /// Read the string secret for `account`, or `nil` if absent / unreadable.
    static func get(account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data,
              let string = String(data: data, encoding: .utf8)
        else { return nil }
        return string
    }

    /// Remove the secret for `account` (no-op if absent).
    static func delete(account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }

    /// Whether a secret exists for `account`.
    static func has(account: String) -> Bool {
        get(account: account) != nil
    }
}
