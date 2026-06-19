//! Per-game `<rom>.json` config overlay (v1.7.0 "Forge" Workstream H4).
//!
//! A small, additive, **frontend-only** layer that lets a single ROM carry its
//! own settings — the Mesen2 "per-game config" idea — without baking anything
//! into the deterministic core. On ROM load (after the header-excluded CRC32 is
//! known) the frontend looks for a `<rom-stem>.json` file in two places:
//!
//! 1. a **sibling** next to the ROM (`/path/to/Game.json` for `/path/to/Game.nes`), and
//! 2. a **config-dir overlay** (`<config-dir>/per-game/<CRC8>.json`).
//!
//! The config-dir overlay **wins** when both exist — exactly the v1.2.0
//! game-DB user-overlay precedence (user edits override what ships beside the
//! ROM). The editor only ever writes the config-dir overlay; it never touches a
//! sibling ROM file the user dropped in.
//!
//! ## The determinism firewall
//!
//! This is layered on the v1.2.0 game-DB ([`crate::game_db`]) and applied
//! through the SAME paths: header rewrites via
//! [`crate::game_db::apply_header_overrides`], mirroring via
//! [`rustynes_core::Nes::set_mirroring_override`], and Vs. DIP switches via
//! [`rustynes_core::Nes::set_vs_dip`]. The deterministic core + the test
//! harness (`AccuracyCoin`, the commercial oracle, `nestest`) build the `Nes`
//! directly and **never** consult any per-game file — so they stay
//! byte-identical. With no `<rom>.json` present every field reads back as its
//! serde default and the load path is a no-op, i.e. byte-identical to today.
//!
//! Any state-mutating override that could perturb the framebuffer/audio stream
//! (the overrides + the DIP value) flows through the same load-time / setter
//! path the v1.2.0 game-DB editor already uses, so the netplay /
//! TAS-replay / RA-hardcore gating that guards that path applies unchanged: a
//! netplay session resolves the override from the SHARED ROM CRC (both peers
//! see the same file or none) and the core's save-state carries the resolved
//! mirroring/DIP, keeping rollback consistent.

use serde::{Deserialize, Serialize};

use crate::game_db::GameDbEntry;

/// Region token used in the JSON overlay (matches the game-DB column tokens).
fn region_token(token: &str) -> Option<rustynes_core::rustynes_mappers::Region> {
    use rustynes_core::rustynes_mappers::Region;
    match token {
        "NTSC" => Some(Region::Ntsc),
        "PAL" => Some(Region::Pal),
        "Dendy" => Some(Region::Dendy),
        _ => None,
    }
}

/// Map a mirroring token to a `Mirroring`, or `None` if unrecognized.
///
/// Public so the load path can apply an explicit per-game mirroring override
/// post-construction (`"Horizontal"`, `"Vertical"`, `"FourScreen"`,
/// `"SingleScreenA"`, `"SingleScreenB"`).
#[must_use]
pub fn mirroring_from_token(token: &str) -> Option<rustynes_core::rustynes_mappers::Mirroring> {
    mirroring_token(token)
}

/// Mirroring token used in the JSON overlay (matches the game-DB column tokens).
fn mirroring_token(token: &str) -> Option<rustynes_core::rustynes_mappers::Mirroring> {
    use rustynes_core::rustynes_mappers::Mirroring;
    match token {
        "Horizontal" => Some(Mirroring::Horizontal),
        "Vertical" => Some(Mirroring::Vertical),
        "FourScreen" => Some(Mirroring::FourScreen),
        "SingleScreenA" => Some(Mirroring::SingleScreenA),
        "SingleScreenB" => Some(Mirroring::SingleScreenB),
        _ => None,
    }
}

/// The `overrides` block of a `<rom>.json` overlay.
///
/// The same load-time corrections the v1.2.0 game-DB carries, expressed as JSON.
/// Every field is optional; an absent field means "no correction, use the ROM's
/// iNES header / the vendored DB".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerGameOverrides {
    /// Region / timing token (`"NTSC"` / `"PAL"` / `"Dendy"`).
    pub region: Option<String>,
    /// iNES mapper-id override.
    pub mapper: Option<u16>,
    /// NES 2.0 submapper override.
    pub submapper: Option<u8>,
    /// Nametable-mirroring token (`"Horizontal"`, `"Vertical"`, `"FourScreen"`,
    /// `"SingleScreenA"`, `"SingleScreenB"`).
    pub mirroring: Option<String>,
}

impl PerGameOverrides {
    /// `true` when no correction field is set (the whole block is inert).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.region.is_none()
            && self.mapper.is_none()
            && self.submapper.is_none()
            && self.mirroring.is_none()
    }

    /// Build a [`GameDbEntry`] (keyed on `crc`) from these overrides so they can
    /// flow through the existing [`crate::game_db::apply_header_overrides`] +
    /// mirroring path. Unrecognized region/mirroring tokens parse to `None`
    /// (i.e. no correction), matching the game-DB's lenient column parsing.
    #[must_use]
    pub fn to_game_db_entry(&self, crc: u32, title: String) -> GameDbEntry {
        GameDbEntry {
            crc,
            region: self.region.as_deref().and_then(region_token),
            mapper: self.mapper,
            submapper: self.submapper,
            mirroring: self.mirroring.as_deref().and_then(mirroring_token),
            title,
        }
    }
}

/// A parsed `<rom>.json` per-game config overlay.
///
/// All fields are `#[serde(default)]` / `Option`, so a missing or partial file
/// deserializes cleanly and an absent file yields no overlay at all (a no-op
/// load path, byte-identical to today). The `video` / `audio` / `input` blocks
/// are reserved free-form JSON values for forward-compat (they are persisted
/// round-trip but not yet consumed — the H4 surface that *is* consumed is the
/// `overrides` + the Vs. `dip_switches`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
// The reserved `video`/`audio`/`input` blocks are free-form `serde_json::Value`,
// which is `PartialEq` but not `Eq` (it can hold an `f64`), so the whole struct
// can only be `PartialEq` — the lint's `Eq` suggestion would not compile.
#[allow(clippy::derive_partial_eq_without_eq)]
#[serde(default)]
pub struct PerGameConfig {
    /// Load-time corrections (region / mapper / submapper / mirroring).
    pub overrides: PerGameOverrides,
    /// Vs. System / arcade DIP-switch byte (switch 1 = bit 0 .. switch 8 =
    /// bit 7). `None` = use the global `[vs] dip` / per-game DB precedence.
    pub dip_switches: Option<u8>,
    /// Reserved per-game video settings (forward-compat; round-tripped, not yet
    /// consumed by the load path — see the module docs).
    pub video: Option<serde_json::Value>,
    /// Reserved per-game audio settings (forward-compat).
    pub audio: Option<serde_json::Value>,
    /// Reserved per-game input settings (forward-compat).
    pub input: Option<serde_json::Value>,
    /// Free-form user notes (display only).
    pub notes: Option<String>,
}

impl PerGameConfig {
    /// `true` when the overlay carries nothing the load path acts on (no
    /// overrides, no DIP). A blank overlay applies nothing, so the default load
    /// path stays byte-identical.
    #[must_use]
    pub const fn is_inert(&self) -> bool {
        self.overrides.is_empty() && self.dip_switches.is_none()
    }
}

/// The config-dir overlay directory (`<config-dir>/per-game`). `None` when no
/// platform config dir is resolvable (e.g. a sandboxed/headless environment).
#[cfg(not(target_arch = "wasm32"))]
fn overlay_dir() -> Option<std::path::PathBuf> {
    crate::config::Config::default_data_dir().map(|d| d.join("per-game"))
}

/// The config-dir overlay file path for a ROM CRC (`<config-dir>/per-game/<CRC8>.json`).
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn overlay_path(crc: u32) -> Option<std::path::PathBuf> {
    overlay_dir().map(|d| d.join(format!("{crc:08X}.json")))
}

/// Parse a `<rom>.json` from raw bytes. Returns `None` on any I/O / parse
/// failure (a malformed overlay is ignored — the ROM still loads with no
/// overlay, never a crash).
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<PerGameConfig> {
    serde_json::from_slice(bytes).ok()
}

/// Resolve the effective per-game overlay for a freshly loaded ROM.
///
/// `rom_path` is the on-disk ROM path (used to find a `<rom-stem>.json` sibling)
/// — `None` for wasm / archive-extracted / drag-and-drop loads where there is no
/// stable sibling path; `crc` is the header-excluded ROM CRC32 (the same key the
/// game-DB uses) used for the config-dir overlay.
///
/// Precedence (config-dir overlay wins, mirroring the v1.2.0 game-DB
/// user-overlay rule): the config-dir overlay is preferred when present;
/// otherwise the sibling is used; otherwise `None` (no overlay, byte-identical
/// load).
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn resolve(crc: u32, rom_path: Option<&std::path::Path>) -> Option<PerGameConfig> {
    resolve_from_paths(overlay_path(crc).as_deref(), rom_path)
}

/// Core resolution logic, parameterized on the already-resolved config-dir
/// overlay path so it is testable without touching the platform config dir.
///
/// Precedence: the config-dir overlay is AUTHORITATIVE when its file is present
/// (a present-but-malformed user overlay yields `None`, NOT the sibling); only
/// when the config-dir overlay file is ABSENT do we consult the sibling.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_from_paths(
    overlay: Option<&std::path::Path>,
    rom_path: Option<&std::path::Path>,
) -> Option<PerGameConfig> {
    // 1) The config-dir overlay is AUTHORITATIVE when its file is present (the
    //    user's edits, keyed by CRC). If the file exists but fails to parse we
    //    return `None` (no overlay applied) rather than silently falling through
    //    to the sibling — a present-but-malformed user overlay must not be
    //    overridden by whatever sits beside the ROM.
    if let Some(path) = overlay
        && let Ok(bytes) = std::fs::read(path)
    {
        return parse(&bytes);
    }
    // 2) Only when the config-dir overlay is ABSENT, fall back to a sibling
    //    `<rom-stem>.json` next to the ROM.
    if let Some(rom_path) = rom_path {
        let sibling = rom_path.with_extension("json");
        // Never confuse the ROM itself for its sidecar (a `Game.json` ROM is
        // implausible, but be defensive).
        if sibling != rom_path
            && let Ok(bytes) = std::fs::read(&sibling)
            && let Some(cfg) = parse(&bytes)
        {
            return Some(cfg);
        }
    }
    None
}

/// Persist a per-game overlay to the **config-dir** overlay file.
///
/// Never touches a sibling ROM file. Writes atomically (temp file + rename) so a
/// crash/kill mid-write can't corrupt the overlay — the same discipline the
/// game-DB user overlay uses. An inert overlay (no overrides, no DIP) deletes
/// the file instead of writing an empty one, so "clear everything" reverts to
/// the byte-identical default load path.
///
/// # Errors
///
/// Returns the underlying I/O / serialization error if the file can't be
/// written, or if no config dir is resolvable.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_overlay(crc: u32, cfg: &PerGameConfig) -> std::io::Result<()> {
    let Some(path) = overlay_path(crc) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no config directory available for the per-game overlay",
        ));
    };
    if cfg.is_inert() {
        // Nothing to persist — remove any prior overlay so the load path reverts
        // to byte-identical. A missing file is fine.
        match std::fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    // Atomic write: serialize to a sibling temp file, then rename over the
    // target (atomic on the same filesystem).
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::rustynes_mappers::{Mirroring, Region};

    #[test]
    fn empty_json_is_inert_default() {
        // An empty object deserializes to all-default (no overrides, no DIP) so
        // the load path is a no-op — byte-identical to having no file at all.
        let cfg = parse(b"{}").expect("empty object parses");
        assert!(cfg.is_inert());
        assert!(cfg.overrides.is_empty());
        assert_eq!(cfg.dip_switches, None);
        assert_eq!(cfg, PerGameConfig::default());
    }

    #[test]
    fn malformed_json_yields_none() {
        // A malformed overlay is ignored (ROM still loads, no overlay applied).
        assert!(parse(b"not json").is_none());
        assert!(parse(b"{ \"overrides\": ").is_none());
    }

    #[test]
    fn partial_overlay_parses_with_defaults() {
        // A partial file (only a DIP) leaves every other field at its default.
        let cfg = parse(br#"{ "dip_switches": 170 }"#).expect("parses");
        assert_eq!(cfg.dip_switches, Some(170));
        assert!(cfg.overrides.is_empty());
        assert!(!cfg.is_inert(), "a DIP makes the overlay active");
    }

    #[test]
    fn overrides_map_to_game_db_entry() {
        let cfg = parse(
            br#"{ "overrides": { "region": "PAL", "mapper": 4, "submapper": 1,
                 "mirroring": "Vertical" }, "notes": "tested on hw" }"#,
        )
        .expect("parses");
        assert!(!cfg.is_inert());
        let entry = cfg.overrides.to_game_db_entry(0x1234, "Title".into());
        assert_eq!(entry.crc, 0x1234);
        assert_eq!(entry.region, Some(Region::Pal));
        assert_eq!(entry.mapper, Some(4));
        assert_eq!(entry.submapper, Some(1));
        assert_eq!(entry.mirroring, Some(Mirroring::Vertical));
        assert_eq!(cfg.notes.as_deref(), Some("tested on hw"));
    }

    #[test]
    fn unknown_tokens_parse_to_no_correction() {
        // A garbage region/mirroring token is treated as "no correction" (not an
        // error) — matching the game-DB's lenient column parsing.
        let ov = PerGameOverrides {
            region: Some("ZX".into()),
            mirroring: Some("Diagonal".into()),
            ..Default::default()
        };
        let entry = ov.to_game_db_entry(1, String::new());
        assert_eq!(entry.region, None);
        assert_eq!(entry.mirroring, None);
    }

    #[test]
    fn round_trips_through_json() {
        let cfg = PerGameConfig {
            overrides: PerGameOverrides {
                region: Some("NTSC".into()),
                mapper: Some(1),
                submapper: None,
                mirroring: Some("Horizontal".into()),
            },
            dip_switches: Some(0x0F),
            video: None,
            audio: None,
            input: None,
            notes: Some("note".into()),
        };
        let json = serde_json::to_vec(&cfg).expect("serialize");
        let back = parse(&json).expect("re-parse");
        assert_eq!(cfg, back);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn malformed_config_dir_overlay_yields_none_not_sibling() {
        // A PRESENT-but-malformed config-dir overlay is authoritative: it must
        // resolve to `None` (no overlay applied), NOT silently fall through to a
        // valid sibling beside the ROM.
        let dir = tempfile::tempdir().expect("tempdir");
        let overlay = dir.path().join("DEADBEEF.json");
        std::fs::write(&overlay, b"not json").expect("write overlay");
        // A perfectly valid sibling that must NOT win.
        let rom = dir.path().join("Game.nes");
        let sibling = dir.path().join("Game.json");
        std::fs::write(&sibling, br#"{ "dip_switches": 42 }"#).expect("write sibling");

        assert_eq!(
            resolve_from_paths(Some(&overlay), Some(&rom)),
            None,
            "a present-but-malformed config-dir overlay must yield None, not the sibling"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn absent_config_dir_overlay_falls_back_to_sibling() {
        // When the config-dir overlay is ABSENT, the sibling `<rom-stem>.json` is
        // consulted.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing_overlay = dir.path().join("DEADBEEF.json"); // never created
        let rom = dir.path().join("Game.nes");
        let sibling = dir.path().join("Game.json");
        std::fs::write(&sibling, br#"{ "dip_switches": 42 }"#).expect("write sibling");

        let cfg = resolve_from_paths(Some(&missing_overlay), Some(&rom))
            .expect("absent overlay -> sibling used");
        assert_eq!(cfg.dip_switches, Some(42));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn valid_config_dir_overlay_wins_over_sibling() {
        // A PRESENT, valid config-dir overlay wins over the sibling.
        let dir = tempfile::tempdir().expect("tempdir");
        let overlay = dir.path().join("DEADBEEF.json");
        std::fs::write(&overlay, br#"{ "dip_switches": 7 }"#).expect("write overlay");
        let rom = dir.path().join("Game.nes");
        let sibling = dir.path().join("Game.json");
        std::fs::write(&sibling, br#"{ "dip_switches": 42 }"#).expect("write sibling");

        let cfg = resolve_from_paths(Some(&overlay), Some(&rom)).expect("valid overlay resolves");
        assert_eq!(cfg.dip_switches, Some(7), "config-dir overlay wins");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn overlay_path_is_crc_keyed_under_per_game() {
        // The config-dir overlay path is `<...>/per-game/<CRC8>.json` (8 upper-hex
        // digits). Only assert the tail so the test is platform-agnostic.
        if let Some(path) = overlay_path(0x00AB_CDEF) {
            assert!(
                path.ends_with("per-game/00ABCDEF.json"),
                "unexpected overlay path: {}",
                path.display()
            );
        }
    }
}
