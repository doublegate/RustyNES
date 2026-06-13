//! Save-state file I/O (per-ROM directory keyed by SHA-256).
//!
//! Per `to-dos/phase-5-frontend-tooling/sprint-2-save-rewind.md` T-52-003:
//! the data directory layout is
//!
//! ```text
//! <data_dir>/saves/<rom_sha256_hex>/slotN.rns
//! ```
//!
//! where `slotN` is `slot0` (the "latest" slot, used by the bare
//! `Ctrl+S` / `Ctrl+L` keys) up to `slot9`. The directory is created
//! lazily on first save.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Number of save-state slots per ROM.
pub const NUM_SLOTS: u8 = 10;

/// Errors raised by [`save_to_slot`] / [`load_from_slot`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SaveError {
    /// I/O error reading or writing the save-state file.
    #[error("save-state I/O at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// Slot index >= [`NUM_SLOTS`].
    #[error("save-state slot {0} out of range (max {max})", max = NUM_SLOTS - 1)]
    InvalidSlot(u8),
}

fn map_io(path: &Path, source: io::Error) -> SaveError {
    SaveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Hex-encode a SHA-256.
#[must_use]
pub fn hex_sha256(hash: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for &b in hash {
        s.push(hex_nibble(b >> 4));
        s.push(hex_nibble(b & 0x0F));
    }
    s
}

const fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '?',
    }
}

/// Compute the file path for `(data_dir, rom_sha256, slot)`.
///
/// # Errors
///
/// Returns [`SaveError::InvalidSlot`] when `slot` is out of range.
pub fn slot_path(data_dir: &Path, rom_sha256: &[u8; 32], slot: u8) -> Result<PathBuf, SaveError> {
    if slot >= NUM_SLOTS {
        return Err(SaveError::InvalidSlot(slot));
    }
    Ok(data_dir
        .join("saves")
        .join(hex_sha256(rom_sha256))
        .join(format!("slot{slot}.rns")))
}

/// Persist `state` bytes to slot `slot` for the ROM identified by
/// `rom_sha256`.
///
/// # Errors
///
/// Returns [`SaveError`] on I/O failure or invalid slot.
pub fn save_to_slot(
    data_dir: &Path,
    rom_sha256: &[u8; 32],
    slot: u8,
    state: &[u8],
) -> Result<PathBuf, SaveError> {
    let path = slot_path(data_dir, rom_sha256, slot)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| map_io(parent, e))?;
    }
    fs::write(&path, state).map_err(|e| map_io(&path, e))?;
    Ok(path)
}

/// Read bytes from slot `slot` for the ROM identified by `rom_sha256`.
///
/// # Errors
///
/// Returns [`SaveError`] on I/O failure (including missing-file) or
/// invalid slot.
pub fn load_from_slot(
    data_dir: &Path,
    rom_sha256: &[u8; 32],
    slot: u8,
) -> Result<Vec<u8>, SaveError> {
    let path = slot_path(data_dir, rom_sha256, slot)?;
    fs::read(&path).map_err(|e| map_io(&path, e))
}

/// `true` if a slot file exists.
//
// Sprint 5-3 will surface this in the egui modal ("recently used slots"
// indicator). We allow `dead_code` rather than wait to land it.
#[must_use]
#[allow(dead_code)]
pub fn slot_exists(data_dir: &Path, rom_sha256: &[u8; 32], slot: u8) -> bool {
    slot_path(data_dir, rom_sha256, slot).is_ok_and(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn h(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    #[test]
    fn hex_sha256_zero() {
        assert_eq!(hex_sha256(&[0u8; 32]).len(), 64);
        assert_eq!(hex_sha256(&[0u8; 32]), "0".repeat(64));
    }

    #[test]
    fn hex_sha256_known() {
        let mut t = [0u8; 32];
        t[0] = 0xAB;
        t[31] = 0xCD;
        let s = hex_sha256(&t);
        assert!(s.starts_with("ab"));
        assert!(s.ends_with("cd"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let hash = h(0x42);
        let payload = b"some-snapshot-bytes".to_vec();
        let path = save_to_slot(tmp.path(), &hash, 0, &payload).unwrap();
        assert!(path.is_file());
        let back = load_from_slot(tmp.path(), &hash, 0).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn separate_slots_independent() {
        let tmp = TempDir::new().unwrap();
        let hash = h(0x42);
        save_to_slot(tmp.path(), &hash, 0, b"slot0").unwrap();
        save_to_slot(tmp.path(), &hash, 1, b"slot1").unwrap();
        assert_eq!(load_from_slot(tmp.path(), &hash, 0).unwrap(), b"slot0");
        assert_eq!(load_from_slot(tmp.path(), &hash, 1).unwrap(), b"slot1");
    }

    #[test]
    fn separate_roms_dont_collide() {
        let tmp = TempDir::new().unwrap();
        save_to_slot(tmp.path(), &h(0x01), 0, b"rom-a").unwrap();
        save_to_slot(tmp.path(), &h(0x02), 0, b"rom-b").unwrap();
        assert_eq!(load_from_slot(tmp.path(), &h(0x01), 0).unwrap(), b"rom-a");
        assert_eq!(load_from_slot(tmp.path(), &h(0x02), 0).unwrap(), b"rom-b");
    }

    #[test]
    fn invalid_slot_rejected() {
        let tmp = TempDir::new().unwrap();
        let err = save_to_slot(tmp.path(), &h(0x00), 99, b"nope").unwrap_err();
        assert!(matches!(err, SaveError::InvalidSlot(99)));
    }

    #[test]
    fn missing_slot_yields_io_error() {
        let tmp = TempDir::new().unwrap();
        let err = load_from_slot(tmp.path(), &h(0x00), 0).unwrap_err();
        assert!(matches!(err, SaveError::Io { .. }));
    }

    #[test]
    fn slot_exists_returns_true_only_after_save() {
        let tmp = TempDir::new().unwrap();
        assert!(!slot_exists(tmp.path(), &h(0x00), 0));
        save_to_slot(tmp.path(), &h(0x00), 0, b"x").unwrap();
        assert!(slot_exists(tmp.path(), &h(0x00), 0));
    }
}
