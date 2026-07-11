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

/// The curated starter database (`#`/blank lines are comments; tab-separated
/// rows). Small + factual — compiled in for both native and wasm. Keyed by the
/// header-EXCLUDED `crate::game_db::rom_crc32`.
const DB_TEXT: &str = include_str!("genie_database.tsv");

/// v2.1.3 — the bulk catalog (~10.8k codes / ~520 USA games) ingested from
/// libretro-database + No-Intro, keyed by the FULL-FILE `rom_crc32_full`.
/// Compiled into **both** native and wasm: at ~777 KB raw it gzips to only
/// ~128 KiB, which sits comfortably inside the wasm bundle's 5 MiB size budget
/// (the bundle is ~3.8 MiB gzip), so the browser demo carries the full game
/// coverage too rather than just the curated starter set above.
const FULL_DB_TEXT: &str = include_str!("genie_database_full.tsv");

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
    /// Effect category for grouping / filtering (e.g. "Lives", "Items",
    /// "Difficulty"). Defaults to "Misc" when the row omits the column.
    pub category: String,
}

/// Fallback category for rows that predate / omit the category column.
const DEFAULT_CATEGORY: &str = "Misc";

/// Parse one tab-separated `CRC<TAB>Game<TAB>Effect<TAB>Code[<TAB>Category]` row.
/// Returns `None` for comment / blank / malformed rows, or rows whose code does
/// not validate through [`GenieCode::new`] (so only usable codes are offered).
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
    // The category column is optional (older rows omit it).
    let category = fields
        .next()
        .filter(|c| !c.is_empty())
        .unwrap_or(DEFAULT_CATEGORY)
        .to_string();
    // Validate + canonicalize through the core decoder; drop unusable codes.
    let code = GenieCode::new(raw_code).ok()?.code().to_string();
    Some(GenieDbCode {
        crc,
        game,
        name,
        code,
        category,
    })
}

/// The parsed database, sorted by CRC for a grouped lookup. Built once.
fn db() -> &'static [GenieDbCode] {
    static DB: OnceLock<Vec<GenieDbCode>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut rows: Vec<GenieDbCode> = DB_TEXT.lines().filter_map(parse_row).collect();
        // v2.1.3 — append the bulk full-file-CRC catalog (the two key conventions
        // coexist; the picklist matches a ROM against both keys). Included on
        // every target — it gzips small enough to fit the wasm size budget.
        rows.extend(FULL_DB_TEXT.lines().filter_map(parse_row));
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

/// v2.1.3 — codes for ANY of a ROM's identifying CRC32s, deduplicated by code.
///
/// The keys are the header-excluded [`crate::game_db::rom_crc32`] and the
/// full-file No-Intro [`crate::game_db::rom_crc32_full`]. Matching on both lets a
/// loaded ROM resolve its codes whatever dump "flavor" the user has.
#[must_use]
pub fn codes_for_crcs(crcs: &[u32]) -> Vec<GenieDbCode> {
    let mut out: Vec<GenieDbCode> = Vec::new();
    for &crc in crcs {
        for code in codes_for_crc(crc) {
            if !out.iter().any(|c| c.code == code.code) {
                out.push(code);
            }
        }
    }
    out
}

/// The game title for the first of a ROM's CRC32s that matches. See
/// [`codes_for_crcs`].
#[must_use]
pub fn game_for_crcs(crcs: &[u32]) -> Option<String> {
    crcs.iter().find_map(|&c| game_for_crc(c))
}

/// The union of [`codes_for_crcs`] grouped by category (each group sorted by
/// effect name) — the multi-key variant of [`codes_for_crc_by_category`].
#[must_use]
pub fn codes_for_crcs_by_category(crcs: &[u32]) -> Vec<(String, Vec<GenieDbCode>)> {
    group_by_category(codes_for_crcs(crcs))
}

/// All known codes for a ROM grouped by category (each group sorted by effect
/// name). For the cheat panel's category-grouped pick-list.
#[must_use]
pub fn codes_for_crc_by_category(crc: u32) -> Vec<(String, Vec<GenieDbCode>)> {
    group_by_category(codes_for_crc(crc))
}

/// Sort a flat code list by (category, effect name) and collapse it into
/// per-category groups. Shared by the single- and multi-CRC by-category lookups.
fn group_by_category(mut codes: Vec<GenieDbCode>) -> Vec<(String, Vec<GenieDbCode>)> {
    codes.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut out: Vec<(String, Vec<GenieDbCode>)> = Vec::new();
    for entry in codes {
        match out.last_mut() {
            Some(last) if last.0 == entry.category => last.1.push(entry),
            _ => out.push((entry.category.clone(), vec![entry])),
        }
    }
    out
}

/// The database entry for a Game Genie code, if it is a known / cataloged one.
///
/// Validated + canonicalized first; this is the "what game + effect is this
/// code?" reverse lookup that lets the UI auto-identify a pasted code.
#[must_use]
pub fn entry_for_code(code: &str) -> Option<GenieDbCode> {
    let canon = GenieCode::new(code).ok()?.code().to_string();
    db().iter().find(|c| c.code == canon).cloned()
}

/// The full catalog, sorted by game -> category -> effect — for a "browse all"
/// view with game / category filters.
#[must_use]
pub fn all_codes() -> Vec<GenieDbCode> {
    let mut rows: Vec<GenieDbCode> = db().to_vec();
    rows.sort_by(|a, b| {
        a.game
            .cmp(&b.game)
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

/// The distinct categories present in the database, sorted (for a filter dropdown).
#[must_use]
pub fn categories() -> Vec<String> {
    let mut cats: Vec<String> = db().iter().map(|c| c.category.clone()).collect();
    cats.sort();
    cats.dedup();
    cats
}

/// The distinct game titles present in the database, sorted (for a filter dropdown).
#[must_use]
pub fn games() -> Vec<String> {
    let mut titles: Vec<String> = db().iter().map(|c| c.game.clone()).collect();
    titles.sort();
    titles.dedup();
    titles
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

    #[test]
    fn category_grouping_and_reverse_lookup() {
        // The category column parses.
        assert!(
            codes_for_crc(0x3337_EC46)
                .iter()
                .any(|c| c.category == "Lives")
        );
        // Category-grouped view: SMB has a "Movement" group.
        let grouped = codes_for_crc_by_category(0x3337_EC46);
        assert!(grouped.iter().any(
            |(cat, codes)| cat == "Movement" && codes.iter().all(|c| c.category == "Movement")
        ));
        // Reverse lookup: a pasted code identifies its catalog entry.
        let entry = entry_for_code("SXIOPO").expect("SXIOPO is cataloged");
        assert!(!entry.game.is_empty() && !entry.category.is_empty());
        assert!(entry_for_code("ZZZZZZ").is_none());
        // Filter sources are populated + sorted-unique.
        let cats = categories();
        assert!(cats.contains(&"Lives".to_string()) && cats.windows(2).all(|w| w[0] < w[1]));
        assert!(games().contains(&"Super Mario Bros.".to_string()));
    }

    #[test]
    fn multi_crc_lookup_unions_and_dedups() {
        // v2.1.3 — the multi-key lookup unions across every CRC and dedups by
        // code, so passing the same CRC twice yields the single-CRC result (no
        // duplicate rows) and never fewer codes than either key alone.
        let single = codes_for_crc(0x3337_EC46);
        let doubled = codes_for_crcs(&[0x3337_EC46, 0x3337_EC46]);
        assert_eq!(
            single.len(),
            doubled.len(),
            "a repeated CRC must not duplicate codes"
        );
        // A miss paired with a hit resolves to the hit's game (misses are skipped).
        assert_eq!(
            game_for_crcs(&[0xDEAD_BEEF, 0x3337_EC46]).as_deref(),
            Some("Super Mario Bros.")
        );
        // The empty key set matches nothing.
        assert!(codes_for_crcs(&[]).is_empty());
        assert_eq!(game_for_crcs(&[]), None);
        // The by-category union covers the same codes as the flat union.
        let flat = codes_for_crcs(&[0x3337_EC46]);
        let grouped: usize = codes_for_crcs_by_category(&[0x3337_EC46])
            .iter()
            .map(|(_, v)| v.len())
            .sum();
        assert_eq!(flat.len(), grouped);
    }

    /// v2.1.3 — the bulk full-file-CRC catalog ships on every target (it gzips
    /// small enough for the wasm budget), folded on top of the curated set.
    #[test]
    fn full_catalog_is_loaded_and_keyed_by_full_file_crc() {
        // The DB folds in the ~10.8k-code bulk catalog on top of the curated
        // starter set, so it is far larger than the starter set alone.
        assert!(
            db().len() > 5_000,
            "DB must include the bulk full-file catalog (got {})",
            db().len()
        );
        // Contra's full-file (No-Intro) CRC32 resolves to its codes — the key
        // convention the bulk catalog is indexed by (header INCLUDED), distinct
        // from the header-excluded curated key.
        let contra = codes_for_crc(0x37CF_0748);
        assert!(
            !contra.is_empty(),
            "Contra's full-file CRC must list bulk-catalog codes"
        );
        assert_eq!(game_for_crc(0x37CF_0748).as_deref(), Some("Contra"));
        // Every bulk-catalog code still round-trips through the core decoder
        // (parse_row enforces it; this guards the larger corpus explicitly).
        for entry in db() {
            assert!(
                GenieCode::new(&entry.code).is_ok(),
                "bulk DB code {} ({}) must validate",
                entry.code,
                entry.name
            );
        }
    }
}
