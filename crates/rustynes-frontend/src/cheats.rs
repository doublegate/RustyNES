//! Per-ROM Game Genie cheat persistence (v1.6.0 Sprint 1).
//!
//! Cheats are stored under the data directory keyed by ROM SHA-256, in the
//! same one-directory-per-ROM spirit as the save-state slots:
//!
//! ```text
//! <data_dir>/cheats/<rom_sha256_hex>.toml
//! ```
//!
//! The file is a list of `{ code, enabled }` entries:
//!
//! ```toml
//! [[cheats]]
//! code = "SXIOPO"
//! enabled = true
//!
//! [[raw]]
//! address = 0x0042
//! value = 0x0A
//! compare = 0x03
//! enabled = true
//! ```
//!
//! Game Genie codes live in the `[[cheats]]` array; raw RAM cheats
//! (GameShark-style: write `value` to `$address` after every frame, optionally
//! gated on the current byte equalling `compare`) live in the `[[raw]]` array.
//! The `raw` field is `#[serde(default)]` so a cheat file written before
//! v1.7.0 (no `raw` key) still loads as an empty raw list.
//!
//! The cheats themselves are a runtime overlay on `rustynes_core::Nes` (NOT part
//! of the save-state); the on-disk file just lets the same set persist across
//! sessions for a given ROM. Loading and saving are native-only — the wasm32
//! build has no filesystem, so [`load`] / [`save`] are gated off there and the
//! in-memory editing (in the cheat panel) still works.

#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use crate::save_state::hex_sha256;

/// One persisted cheat: the canonical Game Genie code string plus whether it
/// is currently applied to the running emulator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheatEntry {
    /// Canonical (upper-case) 6- or 8-character Game Genie code.
    pub code: String,
    /// Whether this code is currently applied to the running `Nes`.
    pub enabled: bool,
}

/// One persisted raw RAM cheat (GameShark-style, v1.7.0): write `value` to the
/// CPU work-RAM byte at `address` after every produced frame, optionally only
/// when the current byte equals `compare`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawCheat {
    /// CPU work-RAM address (`$0000-$1FFF`; the core no-ops outside that range).
    pub address: u16,
    /// Byte value to poke into RAM.
    pub value: u8,
    /// Optional compare byte: when `Some(c)`, only poke when the current RAM
    /// byte equals `c` (a `GameShark` "if equals" cheat). `None` pokes always.
    #[serde(default)]
    pub compare: Option<u8>,
    /// Whether this raw cheat is currently applied to the running `Nes`.
    pub enabled: bool,
}

/// On-disk schema: a list of cheats for one ROM. Only the native `load`/`save`
/// paths construct it (cheat files are not persisted on wasm), so it is
/// `cfg`-gated out of the wasm build to avoid a `dead_code` warning there.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct CheatFile {
    /// The Game Genie cheat entries (TOML `[[cheats]]` array of tables).
    #[serde(default)]
    cheats: Vec<CheatEntry>,
    /// The raw RAM cheat entries (TOML `[[raw]]` array of tables). v1.7.0;
    /// `#[serde(default)]` so older Game-Genie-only files (no `raw` key) load.
    #[serde(default)]
    raw: Vec<RawCheat>,
}

/// Compute the cheat-file path for `(data_dir, rom_sha256)`.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn cheat_path(data_dir: &Path, rom_sha256: &[u8; 32]) -> PathBuf {
    data_dir
        .join("cheats")
        .join(format!("{}.toml", hex_sha256(rom_sha256)))
}

/// Both cheat lists for one ROM, as loaded from / saved to the per-ROM file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cheats {
    /// Game Genie codes.
    pub genie: Vec<CheatEntry>,
    /// Raw RAM cheats (v1.7.0).
    pub raw: Vec<RawCheat>,
}

/// Load the persisted cheats for the ROM identified by `rom_sha256`.
///
/// A missing file yields empty lists (no cheats configured yet). An
/// unreadable or syntactically-invalid file logs a warning and yields empty
/// lists so a corrupt cheat file never blocks loading a ROM. A pre-v1.7.0 file
/// (Game Genie only, no `raw` key) loads with an empty raw list.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn load(data_dir: &Path, rom_sha256: &[u8; 32]) -> Cheats {
    let path = cheat_path(data_dir, rom_sha256);
    let bytes = match fs::read_to_string(&path) {
        Ok(b) => b,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Cheats::default(),
        Err(e) => {
            eprintln!(
                "rustynes: cheats {} unreadable, ignoring: {e}",
                path.display()
            );
            return Cheats::default();
        }
    };
    match toml::from_str::<CheatFile>(&bytes) {
        Ok(f) => Cheats {
            genie: f.cheats,
            raw: f.raw,
        },
        Err(e) => {
            eprintln!(
                "rustynes: cheats {} unparseable, ignoring: {e}",
                path.display()
            );
            Cheats::default()
        }
    }
}

/// Persist both cheat lists for the ROM identified by `rom_sha256`, creating
/// the `cheats` directory if missing. Best-effort: failures are logged, not
/// fatal.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(data_dir: &Path, rom_sha256: &[u8; 32], genie: &[CheatEntry], raw: &[RawCheat]) {
    let path = cheat_path(data_dir, rom_sha256);
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        eprintln!(
            "rustynes: cheats dir {} create failed: {e}",
            parent.display()
        );
        return;
    }
    let file = CheatFile {
        cheats: genie.to_vec(),
        raw: raw.to_vec(),
    };
    match toml::to_string_pretty(&file) {
        Ok(s) => {
            if let Err(e) = fs::write(&path, s) {
                eprintln!("rustynes: cheats {} write failed: {e}", path.display());
            }
        }
        Err(e) => eprintln!("rustynes: cheats serialize failed: {e}"),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const fn h(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    #[test]
    fn load_missing_file_is_empty() {
        let tmp = TempDir::new().unwrap();
        let back = load(tmp.path(), &h(0x00));
        assert!(back.genie.is_empty());
        assert!(back.raw.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let tmp = TempDir::new().unwrap();
        let genie = vec![
            CheatEntry {
                code: "SXIOPO".into(),
                enabled: true,
            },
            CheatEntry {
                code: "GXSOPO".into(),
                enabled: false,
            },
        ];
        let raw = vec![
            RawCheat {
                address: 0x0042,
                value: 0x0A,
                compare: None,
                enabled: true,
            },
            RawCheat {
                address: 0x07FF,
                value: 0xFF,
                compare: Some(0x03),
                enabled: false,
            },
        ];
        save(tmp.path(), &h(0x42), &genie, &raw);
        let back = load(tmp.path(), &h(0x42));
        assert_eq!(back.genie, genie);
        assert_eq!(back.raw, raw);
    }

    #[test]
    fn separate_roms_dont_collide() {
        let tmp = TempDir::new().unwrap();
        save(
            tmp.path(),
            &h(0x01),
            &[CheatEntry {
                code: "AAAAAA".into(),
                enabled: true,
            }],
            &[],
        );
        save(
            tmp.path(),
            &h(0x02),
            &[CheatEntry {
                code: "BBBBBB".into(),
                enabled: true,
            }],
            &[],
        );
        assert_eq!(load(tmp.path(), &h(0x01)).genie[0].code, "AAAAAA");
        assert_eq!(load(tmp.path(), &h(0x02)).genie[0].code, "BBBBBB");
    }

    #[test]
    fn corrupt_file_yields_empty() {
        let tmp = TempDir::new().unwrap();
        let path = cheat_path(tmp.path(), &h(0x07));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "this is = = not toml").unwrap();
        let back = load(tmp.path(), &h(0x07));
        assert!(back.genie.is_empty());
        assert!(back.raw.is_empty());
    }

    /// Back-compat: a pre-v1.7.0 cheat file (Game Genie only, no `raw` key)
    /// still loads — the `raw` list comes back empty.
    #[test]
    fn legacy_file_without_raw_key_still_loads() {
        let tmp = TempDir::new().unwrap();
        let path = cheat_path(tmp.path(), &h(0x09));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Exactly the schema written before raw RAM cheats existed.
        fs::write(&path, "[[cheats]]\ncode = \"SXIOPO\"\nenabled = true\n").unwrap();
        let back = load(tmp.path(), &h(0x09));
        assert_eq!(back.genie.len(), 1);
        assert_eq!(back.genie[0].code, "SXIOPO");
        assert!(back.genie[0].enabled);
        assert!(back.raw.is_empty());
    }

    /// A raw cheat with no `compare` key deserializes to `compare: None`.
    #[test]
    fn raw_cheat_without_compare_loads_as_none() {
        let tmp = TempDir::new().unwrap();
        let path = cheat_path(tmp.path(), &h(0x0A));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[[raw]]\naddress = 66\nvalue = 10\nenabled = true\n").unwrap();
        let back = load(tmp.path(), &h(0x0A));
        assert_eq!(back.raw.len(), 1);
        assert_eq!(back.raw[0].address, 66);
        assert_eq!(back.raw[0].value, 10);
        assert_eq!(back.raw[0].compare, None);
        assert!(back.raw[0].enabled);
    }
}
