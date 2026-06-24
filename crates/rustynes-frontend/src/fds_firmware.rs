//! Famicom Disk System firmware (`disksys.rom`) recognition (v1.8.9).
//!
//! The FDS needs an 8 KiB BIOS the emulator can't ship (it is Nintendo's). This
//! classifies a candidate file so the Settings → FDS picker can tell the user
//! whether what they pointed at is the right thing: the hard gate is the 8 KiB
//! size; a SHA-256 match against the known dump is an extra "recognized" badge.
//! An 8 KiB file whose hash isn't in the table is still reported as usable (dumps
//! vary), just not positively recognized.

use sha2::{Digest, Sha256};

/// The required FDS BIOS size, in bytes (mapped at `$E000-$FFFF`).
pub const BIOS_SIZE: usize = 8192;

/// Known-good `disksys.rom` dumps, keyed by lowercase SHA-256 hex.
const KNOWN: &[(&str, &str)] = &[(
    "99c18490ed9002d9c6d999b9d8d15be5c051bdfa7cc7e73318053c9a994b0178",
    "Nintendo FDS BIOS (disksys.rom)",
)];

/// The verdict for a candidate firmware file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiosStatus {
    /// Wrong size — not an FDS BIOS (carries the actual byte length).
    WrongSize(usize),
    /// 8 KiB and the SHA-256 matches a known dump (carries its label).
    Recognized(&'static str),
    /// 8 KiB but the SHA-256 is not in the table — usable, just unverified
    /// (carries the lowercase SHA-256 hex so the user can check it externally).
    Unverified(String),
}

impl BiosStatus {
    /// Whether the file is the right size to load as an FDS BIOS at all.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        !matches!(self, Self::WrongSize(_))
    }
}

/// Lowercase SHA-256 hex of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use core::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Classify a candidate `disksys.rom`.
#[must_use]
pub fn classify(bytes: &[u8]) -> BiosStatus {
    if bytes.len() != BIOS_SIZE {
        return BiosStatus::WrongSize(bytes.len());
    }
    let hex = sha256_hex(bytes);
    KNOWN
        .iter()
        .find(|(h, _)| *h == hex)
        .map_or(BiosStatus::Unverified(hex), |(_, label)| {
            BiosStatus::Recognized(label)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_size_is_rejected() {
        let s = classify(&[0u8; 4096]);
        assert_eq!(s, BiosStatus::WrongSize(4096));
        assert!(!s.is_usable());
    }

    #[test]
    fn correct_size_unknown_hash_is_unverified_but_usable() {
        // 8 KiB of zeroes is the right size but not a real dump.
        let s = classify(&[0u8; BIOS_SIZE]);
        match &s {
            BiosStatus::Unverified(hex) => assert_eq!(hex.len(), 64),
            other => panic!("expected Unverified, got {other:?}"),
        }
        assert!(s.is_usable());
    }

    #[test]
    fn sha256_hex_is_lowercase_64_chars() {
        let hex = sha256_hex(b"rustynes");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}
