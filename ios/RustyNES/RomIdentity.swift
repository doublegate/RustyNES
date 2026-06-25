//
//  RomIdentity.swift
//
//  SHA-256 of ROM bytes — the stable key the whole stack uses for a cartridge
//  (the on-disk library path, save-state filenames, and the same key the Rust
//  bridge / RetroAchievements use). Mirrors the Android `MessageDigest` hashing.
//

import CryptoKit
import Foundation

enum RomIdentity {
    /// Lowercase-hex SHA-256 of the supplied bytes (the stable ROM key).
    static func sha256Hex(_ data: Data) -> String {
        let digest = SHA256.hash(data: data)
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    /// The raw 32-byte SHA-256 (for FFI calls that take the digest bytes, e.g.
    /// `NesController.raLoadGame` — not wired in the v1.9.0 MVP, kept for parity).
    static func sha256Bytes(_ data: Data) -> Data {
        Data(SHA256.hash(data: data))
    }
}
