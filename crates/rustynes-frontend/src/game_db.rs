#![allow(clippy::doc_markdown)] // iNES / CRC32 / PRG / CHR / TetaNES are domain terms.

//! Per-game database (v1.1.0 beta.1, Workstream B, T-110-B4).
//!
//! A CRC32-keyed table of per-game corrections, vendored from TetaNES'
//! `game_database.txt` (see `game_database.txt` for the attribution + format).
//! RustyNES currently consumes the **mirroring** column: at ROM load the
//! frontend computes the ROM's CRC32 and, if the DB lists it, applies a
//! nametable-mirroring override via [`rustynes_core::Nes::set_mirroring_override`]
//! — a load-time fix for ROMs whose iNES header carries the wrong mirroring
//! flag.
//!
//! This is **frontend-only**: the core test suites (AccuracyCoin, the commercial
//! oracle, `nestest`) construct the `Nes` directly and never consult this DB, so
//! they stay byte-identical. The override is deterministic (same CRC ⇒ same
//! mirroring) and persisted in the save-state, so netplay + rollback stay
//! consistent. Both peers in a netplay session resolve the same override from
//! the shared ROM.
//!
//! ## Key
//!
//! The key is the CRC32 of the PRG-ROM concatenated with the CHR-ROM, **excluding
//! the 16-byte iNES header (and the 512-byte trainer, if present)** — the
//! standard "ROM CRC" used by TetaNES / Nestopia. iNES-1.0 sizing is used; NES
//! 2.0 ROMs with extended sizes simply don't match (no override — safe).

use std::sync::OnceLock;

use rustynes_core::rustynes_mappers::{Mirroring, Region};

/// The vendored database text (CRC + correction columns; `#` lines are comments).
const DB_TEXT: &str = include_str!("game_database.txt");

/// A per-game database entry: a set of optional corrections keyed by ROM CRC32.
///
/// Each field is an *override* — `None` means "no correction, use the ROM's iNES
/// header." `mirroring` is applied post-construction via
/// [`rustynes_core::Nes::set_mirroring_override`]; `region` / `mapper` /
/// `submapper` are applied by patching the iNES header bytes before the core
/// parses them (see [`apply_header_overrides`]). This is **frontend-only**: the
/// core test suites construct the `Nes` directly and never consult the DB, so
/// `AccuracyCoin` / the commercial oracle stay byte-identical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameDbEntry {
    /// CRC32 of PRG-ROM + CHR-ROM (header/trainer excluded) — the key.
    pub crc: u32,
    /// Region / timing override (NTSC / PAL / Dendy).
    pub region: Option<Region>,
    /// iNES mapper-id override (for a ROM with a wrong header mapper).
    pub mapper: Option<u16>,
    /// NES 2.0 submapper override.
    pub submapper: Option<u8>,
    /// Nametable-mirroring override.
    pub mirroring: Option<Mirroring>,
    /// Game title (display only).
    pub title: String,
}

/// Parsed vendored table, sorted by CRC for binary search. Built once on first
/// use. Rows with no usable correction field are still kept (title-only) so the
/// editor can show the game name.
fn db() -> &'static [GameDbEntry] {
    static DB: OnceLock<Vec<GameDbEntry>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut rows: Vec<GameDbEntry> = DB_TEXT.lines().filter_map(parse_row).collect();
        rows.sort_unstable_by_key(|e| e.crc);
        rows.dedup_by_key(|e| e.crc);
        rows.shrink_to_fit();
        rows
    })
}

/// Map a database region token to a [`Region`], or `None` if unrecognized.
fn parse_region(token: &str) -> Option<Region> {
    match token {
        "NTSC" => Some(Region::Ntsc),
        "PAL" => Some(Region::Pal),
        "Dendy" => Some(Region::Dendy),
        _ => None,
    }
}

/// Map a database mirroring token to a [`Mirroring`], or `None` if unrecognized
/// (e.g. mapper-controlled rows that carry no usable static override).
fn parse_mirroring(token: &str) -> Option<Mirroring> {
    match token {
        "Horizontal" => Some(Mirroring::Horizontal),
        "Vertical" => Some(Mirroring::Vertical),
        "FourScreen" => Some(Mirroring::FourScreen),
        "SingleScreenA" => Some(Mirroring::SingleScreenA),
        "SingleScreenB" => Some(Mirroring::SingleScreenB),
        _ => None,
    }
}

/// Parse one `CRC, Region, Mapper, Sub-Mapper, ChrBanks, PrgRomBanks,
/// PrgRamBanks, Battery, Mirroring, Title` row into a [`GameDbEntry`]. Returns
/// `None` for comment / blank / malformed lines. The title is the final field
/// and may contain commas (it is split off with `splitn`).
fn parse_row(line: &str) -> Option<GameDbEntry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    // 10 columns; the 10th (title) keeps any embedded commas.
    let fields: Vec<&str> = line.splitn(10, ',').map(str::trim).collect();
    if fields.len() < 9 {
        return None;
    }
    let crc = u32::from_str_radix(fields[0], 16).ok()?;
    let region = parse_region(fields[1]);
    let mapper = fields[2].parse::<u16>().ok();
    let submapper = fields[3].parse::<u8>().ok();
    let mirroring = parse_mirroring(fields[8]);
    let title = fields
        .get(9)
        .map(|t| t.trim_matches('"').to_string())
        .unwrap_or_default();
    Some(GameDbEntry {
        crc,
        region,
        mapper,
        submapper,
        mirroring,
        title,
    })
}

/// Look up a ROM's effective database entry by CRC32 — the **user overlay**
/// (editable, persisted via the in-app ROM-DB editor) takes precedence over the
/// vendored base; `None` if listed in neither.
///
/// Returns an owned entry because the user overlay is mutable (behind a
/// `RwLock`). The vendored base alone is reachable via [`vendored_entry`].
#[must_use]
pub fn entry_for_crc(crc: u32) -> Option<GameDbEntry> {
    if let Ok(overlay) = user_overlay().read()
        && let Ok(i) = overlay.binary_search_by_key(&crc, |e| e.crc)
    {
        return Some(overlay[i].clone());
    }
    vendored_entry(crc).cloned()
}

/// Look up a ROM's entry in the **vendored base only** (ignoring the user
/// overlay) — used by the editor to show "reset to default".
#[must_use]
pub fn vendored_entry(crc: u32) -> Option<&'static GameDbEntry> {
    let db = db();
    db.binary_search_by_key(&crc, |e| e.crc)
        .ok()
        .map(|i| &db[i])
}

/// Look up a ROM's mirroring correction by CRC32, or `None` if not listed (or
/// listed without a usable mirroring override). Thin wrapper over
/// [`entry_for_crc`], kept for the existing load path.
#[must_use]
pub fn mirroring_for_crc(crc: u32) -> Option<Mirroring> {
    entry_for_crc(crc).and_then(|e| e.mirroring)
}

// ---------------------------------------------------------------------------
// User overlay (v1.2.0 Workstream B) — editable per-game corrections persisted
// to the data dir, overriding the vendored base by CRC. Behind a `RwLock` so
// the in-app ROM-DB editor can refresh it live. The core test suites never
// touch this (frontend-only), so the determinism firewall holds.
// ---------------------------------------------------------------------------

/// Path to the user-overlay file (`game_db_user.txt` in the data dir).
fn overlay_path() -> Option<std::path::PathBuf> {
    crate::config::Config::default_data_dir().map(|d| d.join("game_db_user.txt"))
}

/// The lazily-loaded, live-editable user overlay (sorted by CRC).
fn user_overlay() -> &'static std::sync::RwLock<Vec<GameDbEntry>> {
    static OVERLAY: OnceLock<std::sync::RwLock<Vec<GameDbEntry>>> = OnceLock::new();
    OVERLAY.get_or_init(|| std::sync::RwLock::new(load_overlay()))
}

fn load_overlay() -> Vec<GameDbEntry> {
    let Some(path) = overlay_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut rows: Vec<GameDbEntry> = text.lines().filter_map(parse_row).collect();
    rows.sort_unstable_by_key(|e| e.crc);
    rows.dedup_by_key(|e| e.crc);
    rows
}

/// Insert or replace a user-overlay entry and persist the overlay to disk.
///
/// # Errors
///
/// Returns the underlying I/O error if the overlay file can't be written.
pub fn upsert_user_entry(entry: GameDbEntry) -> std::io::Result<()> {
    {
        let mut ov = user_overlay()
            .write()
            .expect("game-db overlay lock poisoned");
        match ov.binary_search_by_key(&entry.crc, |e| e.crc) {
            Ok(i) => ov[i] = entry,
            Err(i) => ov.insert(i, entry),
        }
    }
    persist_overlay()
}

/// Remove a user-overlay entry (reverting to the vendored base) and persist.
///
/// # Errors
///
/// Returns the underlying I/O error if the overlay file can't be written.
pub fn remove_user_entry(crc: u32) -> std::io::Result<()> {
    {
        let mut ov = user_overlay()
            .write()
            .expect("game-db overlay lock poisoned");
        if let Ok(i) = ov.binary_search_by_key(&crc, |e| e.crc) {
            ov.remove(i);
        }
    }
    persist_overlay()
}

fn persist_overlay() -> std::io::Result<()> {
    let Some(path) = overlay_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Snapshot the rows under a minimal lock, then serialize + write lock-free.
    let entries: Vec<GameDbEntry> = user_overlay()
        .read()
        .expect("game-db overlay lock poisoned")
        .clone();
    let mut out = String::from(
        "# RustyNES per-game user overrides (v1.2.0). Edited via Tools -> ROM Database.\n\
         # Columns: CRC, Region, Mapper, Sub-Mapper, ChrBanks, PrgRomBanks, PrgRamBanks, \
         Battery, Mirroring, Title\n",
    );
    for e in &entries {
        out.push_str(&serialize_row(e));
        out.push('\n');
    }
    // Atomic write: serialize to a sibling temp file, then rename over the
    // target. A crash/kill mid-write can't truncate or corrupt the user overlay
    // (rename is atomic on the same filesystem) — Gemini review, PR #74.
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, &path)
}

/// Serialize a [`GameDbEntry`] back to the 10-column row format `parse_row`
/// reads (unused columns left empty). Round-trips through `parse_row`.
fn serialize_row(e: &GameDbEntry) -> String {
    let region = match e.region {
        Some(Region::Ntsc) => "NTSC",
        Some(Region::Pal) => "PAL",
        Some(Region::Dendy) => "Dendy",
        _ => "",
    };
    let mapper = e.mapper.map(|m| m.to_string()).unwrap_or_default();
    let sub = e.submapper.map(|s| s.to_string()).unwrap_or_default();
    let mirroring = match e.mirroring {
        Some(Mirroring::Horizontal) => "Horizontal",
        Some(Mirroring::Vertical) => "Vertical",
        Some(Mirroring::FourScreen) => "FourScreen",
        Some(Mirroring::SingleScreenA) => "SingleScreenA",
        Some(Mirroring::SingleScreenB) => "SingleScreenB",
        _ => "",
    };
    format!(
        "{:08X}, {region}, {mapper}, {sub}, , , , , {mirroring}, \"{}\"",
        e.crc, e.title
    )
}

/// Apply a database entry's `region` / `mapper` / `submapper` corrections.
///
/// Rewrites the iNES (or NES 2.0) header bytes of `bytes` in place, *before* the
/// core parses them. Mirroring is **not** applied here — it goes through the
/// post-construction [`rustynes_core::Nes::set_mirroring_override`] setter.
///
/// This keeps the determinism firewall intact: only the frontend patches the
/// header, the core sees a normal iNES image, and the CRC key (PRG+CHR, header
/// excluded) is unchanged so the lookup is stable across the patch.
///
/// Returns `true` if any byte was changed (the caller may want to log it).
pub fn apply_header_overrides(bytes: &mut [u8], entry: &GameDbEntry) -> bool {
    if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
        return false;
    }
    let is_nes2 = (bytes[7] & 0x0C) == 0x08;
    let mut changed = false;

    if let Some(mapper) = entry.mapper {
        // iNES: low nibble in flags6[7:4], high nibble in flags7[7:4].
        // NES 2.0 adds bits 8-11 in byte 8[3:0].
        let lo = (mapper & 0x0F) as u8;
        let mid = ((mapper >> 4) & 0x0F) as u8;
        let new6 = (bytes[6] & 0x0F) | (lo << 4);
        let new7 = (bytes[7] & 0x0F) | (mid << 4);
        if new6 != bytes[6] || new7 != bytes[7] {
            bytes[6] = new6;
            bytes[7] = new7;
            changed = true;
        }
        if is_nes2 {
            let hi = ((mapper >> 8) & 0x0F) as u8;
            let new8 = (bytes[8] & 0xF0) | hi;
            if new8 != bytes[8] {
                bytes[8] = new8;
                changed = true;
            }
        }
    }

    if let Some(submapper) = entry.submapper
        && is_nes2
    {
        let new8 = (bytes[8] & 0x0F) | ((submapper & 0x0F) << 4);
        if new8 != bytes[8] {
            bytes[8] = new8;
            changed = true;
        }
    }

    if let Some(region) = entry.region {
        if is_nes2 {
            // NES 2.0 region is byte 12, low two bits (0 = NTSC, 1 = PAL,
            // 2 = multi, 3 = Dendy).
            let code = match region {
                Region::Pal => 1,
                Region::Dendy => 3,
                _ => 0,
            };
            let new12 = (bytes[12] & 0xFC) | code;
            if new12 != bytes[12] {
                bytes[12] = new12;
                changed = true;
            }
        } else {
            // iNES 1.0 TV-system is byte 9 bit 0 (0 = NTSC, 1 = PAL); Dendy is
            // not representable in iNES 1.0, so map it to PAL timing's flag.
            let bit = u8::from(matches!(region, Region::Pal | Region::Dendy));
            let new9 = (bytes[9] & 0xFE) | bit;
            if new9 != bytes[9] {
                bytes[9] = new9;
                changed = true;
            }
        }
    }

    changed
}

/// Compute the "ROM CRC" of an iNES image: CRC32 of PRG-ROM + CHR-ROM.
///
/// Excludes the 16-byte header and any 512-byte trainer. Returns `None` if the
/// bytes are not a plausible iNES image or are too short for the header-declared
/// sizes.
#[must_use]
pub fn rom_crc32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
        return None;
    }
    let prg = (bytes[4] as usize) * 16 * 1024;
    let chr = (bytes[5] as usize) * 8 * 1024;
    let trainer: usize = if bytes[6] & 0x04 != 0 { 512 } else { 0 };
    let start = 16 + trainer;
    let end = start.checked_add(prg)?.checked_add(chr)?;
    if prg == 0 || end > bytes.len() {
        return None;
    }
    Some(crc32(&bytes[start..end]))
}

/// IEEE CRC-32 (reflected, polynomial `0xEDB8_8320`) — the zip/PNG CRC, matching
/// TetaNES' `compute_crc32`. Table-less; runs once per ROM load.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // The CRC-32 of "123456789" is the standard 0xCBF43926 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn db_parses_and_is_sorted() {
        let db = db();
        assert!(!db.is_empty(), "vendored DB must parse some rows");
        assert!(
            db.windows(2).all(|w| w[0].crc < w[1].crc),
            "DB must be strictly sorted by CRC (binary-search invariant)"
        );
        // A known entry from the vendored file: Mega Man 3 (Europe) 0x1388B3 =
        // Vertical mirroring, PAL region, mapper 4.
        assert_eq!(mirroring_for_crc(0x0013_88B3), Some(Mirroring::Vertical));
        let entry = entry_for_crc(0x0013_88B3).expect("Mega Man 3 (Europe) listed");
        assert_eq!(entry.region, Some(Region::Pal));
        assert_eq!(entry.mapper, Some(4));
        assert!(entry.title.contains("Mega Man 3"));
    }

    #[test]
    fn lookup_miss_is_none() {
        assert_eq!(mirroring_for_crc(0xDEAD_BEEF), None);
    }

    #[test]
    fn header_overrides_rewrite_mapper_and_region() {
        // Minimal iNES 1.0 header (mapper 0, NTSC). Override to mapper 4 + PAL.
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1; // 1 PRG bank
        rom[5] = 1; // 1 CHR bank
        let entry = GameDbEntry {
            crc: 0,
            region: Some(Region::Pal),
            mapper: Some(4),
            submapper: None,
            mirroring: None,
            title: "Test".into(),
        };
        assert!(apply_header_overrides(&mut rom, &entry));
        // Mapper 4: low nibble in flags6[7:4], high nibble (0) in flags7[7:4].
        assert_eq!(rom[6] >> 4, 4, "mapper low nibble");
        assert_eq!(rom[7] >> 4, 0, "mapper high nibble");
        assert_eq!(rom[9] & 1, 1, "iNES 1.0 PAL TV-system bit");
        // Re-applying the same override is now a no-op (idempotent).
        assert!(!apply_header_overrides(&mut rom, &entry));
    }

    #[test]
    fn overlay_row_round_trips_through_parse() {
        // The editor serializes user-overlay entries with `serialize_row`; they
        // must parse back identically via `parse_row` (same on-disk format as
        // the vendored DB).
        let entry = GameDbEntry {
            crc: 0x0013_88B3,
            region: Some(Region::Pal),
            mapper: Some(4),
            submapper: Some(1),
            mirroring: Some(Mirroring::Vertical),
            title: "Mega Man 3 (Europe) (Rev A).nes".into(),
        };
        let row = serialize_row(&entry);
        let parsed = parse_row(&row).expect("serialized row parses");
        assert_eq!(parsed, entry);

        // A sparse entry (only mirroring) also round-trips (empty columns -> None).
        let sparse = GameDbEntry {
            crc: 0xABCD_1234,
            region: None,
            mapper: None,
            submapper: None,
            mirroring: Some(Mirroring::Horizontal),
            title: "Homebrew".into(),
        };
        assert_eq!(parse_row(&serialize_row(&sparse)), Some(sparse));
    }

    #[test]
    fn header_overrides_noop_for_non_ines() {
        let mut not_a_rom = b"not a rom".to_vec();
        let entry = GameDbEntry {
            crc: 0,
            region: Some(Region::Pal),
            mapper: Some(4),
            submapper: None,
            mirroring: None,
            title: String::new(),
        };
        assert!(!apply_header_overrides(&mut not_a_rom, &entry));
    }

    #[test]
    fn rom_crc32_rejects_non_ines() {
        assert_eq!(rom_crc32(b"not a rom"), None);
        assert_eq!(rom_crc32(&[]), None);
    }

    #[test]
    fn rom_crc32_computes_over_prg_chr() {
        // Minimal iNES: header (16) + 1x16KB PRG + 1x8KB CHR, all 0xAA.
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1; // 1 PRG bank
        rom[5] = 1; // 1 CHR bank
        for b in &mut rom[16..] {
            *b = 0xAA;
        }
        let crc = rom_crc32(&rom).expect("valid iNES");
        // CRC over the 24KB of 0xAA, independent of the header bytes.
        let expected = crc32(&vec![0xAAu8; 16 * 1024 + 8 * 1024]);
        assert_eq!(crc, expected);
    }
}
