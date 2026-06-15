#![allow(clippy::doc_markdown)] // CRC32 / PRG / CHR / Galoob are domain terms.

//! Game Genie code-name database (v1.2.0 Workstream D, D3).
//!
//! A ROM-indexed (CRC32-keyed) table of well-known Game Genie codes drawn from
//! the public Galoob code books / community code lists. The cheat panel offers
//! the codes that match the loaded ROM as a pick-list; selecting one feeds the
//! EXISTING [`rustynes_core::GenieCode`] decode + [`crate::cheats`] persistence.
//!
//! This is **frontend-only**: the emulation core's Game Genie substitution
//! ([`rustynes_core::genie`]) is unchanged — this module never touches the core
//! and only ever produces strings that are validated through `GenieCode::new`
//! before being surfaced, so a malformed row is silently dropped and the
//! determinism / no-cheat firewall is untouched.
//!
//! ## Key
//!
//! The key is the same "ROM CRC" [`crate::game_db::rom_crc32`] computes — the
//! CRC32 of PRG-ROM + CHR-ROM, excluding the 16-byte iNES header (and any
//! 512-byte trainer). So a loaded ROM resolves its codes by the CRC already
//! computed at load.

use std::sync::OnceLock;

use rustynes_core::GenieCode;

/// The vendored database text (`#`/blank lines are comments; tab-separated rows).
/// Small + factual — compiled in for both native and wasm.
const DB_TEXT: &str = include_str!("genie_database.tsv");

/// One Game Genie code entry: a named code for a specific ROM (by CRC32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenieDbCode {
    /// CRC32 of PRG-ROM + CHR-ROM (header/trainer excluded) — the key.
    pub crc: u32,
    /// Game title (display only).
    pub game: String,
    /// Human-readable effect name (e.g. "Infinite lives").
    pub name: String,
    /// The canonical Game Genie code (6 or 8 characters), pre-validated.
    pub code: String,
}

/// Parse one tab-separated `CRC<TAB>Game<TAB>Effect<TAB>Code` row. Returns
/// `None` for comment / blank / malformed rows, or rows whose code does not
/// validate through [`GenieCode::new`] (so only usable codes are offered).
fn parse_row(line: &str) -> Option<GenieDbCode> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split('\t').map(str::trim);
    let crc = u32::from_str_radix(fields.next()?, 16).ok()?;
    let game = fields.next()?.to_string();
    let name = fields.next()?.to_string();
    let raw_code = fields.next()?;
    // Validate + canonicalize through the core decoder; drop unusable codes.
    let code = GenieCode::new(raw_code).ok()?.code().to_string();
    Some(GenieDbCode {
        crc,
        game,
        name,
        code,
    })
}

/// The parsed database, sorted by CRC for a grouped lookup. Built once.
fn db() -> &'static [GenieDbCode] {
    static DB: OnceLock<Vec<GenieDbCode>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut rows: Vec<GenieDbCode> = DB_TEXT.lines().filter_map(parse_row).collect();
        rows.sort_by(|a, b| a.crc.cmp(&b.crc).then_with(|| a.name.cmp(&b.name)));
        rows.shrink_to_fit();
        rows
    })
}

/// All known Game Genie codes for a ROM, by CRC32 — empty if none are listed.
#[must_use]
pub fn codes_for_crc(crc: u32) -> Vec<GenieDbCode> {
    db().iter().filter(|c| c.crc == crc).cloned().collect()
}

/// The game title listed for a CRC32 (the first matching row), if any.
#[must_use]
pub fn game_for_crc(crc: u32) -> Option<String> {
    db().iter().find(|c| c.crc == crc).map(|c| c.game.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_parses_and_all_codes_validate() {
        let db = db();
        assert!(!db.is_empty(), "vendored Genie DB must parse some rows");
        // Every surfaced code must decode (parse_row already enforces this; this
        // guards against a future format regression).
        for entry in db {
            assert!(
                GenieCode::new(&entry.code).is_ok(),
                "DB code {} ({}) must validate",
                entry.code,
                entry.name
            );
        }
    }

    #[test]
    fn lookup_returns_codes_for_a_known_crc() {
        // Super Mario Bros. (CRC 0x3337EC46) has several listed codes.
        let codes = codes_for_crc(0x3337_EC46);
        assert!(!codes.is_empty(), "SMB CRC must list codes");
        assert!(
            codes.iter().any(|c| c.name == "Infinite lives"),
            "SMB lists an 'Infinite lives' code"
        );
        assert_eq!(
            game_for_crc(0x3337_EC46).as_deref(),
            Some("Super Mario Bros.")
        );
    }

    #[test]
    fn lookup_miss_is_empty() {
        assert!(codes_for_crc(0xDEAD_BEEF).is_empty());
        assert_eq!(game_for_crc(0xDEAD_BEEF), None);
    }
}
